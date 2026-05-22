// SPDX-License-Identifier: Apache-2.0
//! Image resolver / pull tests.
//!
//! Mirrors upstream's `core/remotes/docker/resolver_test.go` happy
//! paths plus the bearer-challenge-retry flow. Live network is
//! forbidden (Charter §7) — we use `httpmock` for every HTTP test.

use std::sync::Arc;

use cave_home_containerd_rs::content::{Digest, Store as ContentStore};
use cave_home_containerd_rs::image::auth::{first_bearer, parse_challenge};
use cave_home_containerd_rs::image::{Reference, Resolver};
use httpmock::{Method, MockServer};
use tempfile::TempDir;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn test_parse_bearer_challenge_extracts_realm_and_scope() {
    let h = "Bearer realm=\"https://auth.example.com/token\",service=\"registry.example.com\",scope=\"repository:library/alpine:pull\"";
    let c = parse_challenge(h).unwrap();
    assert_eq!(c.scheme, "bearer");
    assert_eq!(c.parameters.get("realm").map(String::as_str), Some("https://auth.example.com/token"));
    assert_eq!(c.parameters.get("service").map(String::as_str), Some("registry.example.com"));
    assert_eq!(c.parameters.get("scope").map(String::as_str), Some("repository:library/alpine:pull"));
}

#[test]
fn test_parse_basic_challenge_returns_basic_scheme() {
    let c = parse_challenge("Basic realm=\"r\"").unwrap();
    assert_eq!(c.scheme, "basic");
}

#[test]
fn test_first_bearer_picks_bearer_among_many() {
    let headers = vec!["Basic realm=\"r\"", "Bearer realm=\"x\""];
    let c = first_bearer(headers).unwrap();
    assert_eq!(c.scheme, "bearer");
}

#[test]
fn test_reference_parse_canonical_tag() {
    let r = Reference::parse("registry.example.com:5000/library/alpine:3.20").unwrap();
    assert_eq!(r.host, "registry.example.com:5000");
    assert_eq!(r.repository, "library/alpine");
    assert_eq!(r.reference, "3.20");
}

#[test]
fn test_reference_parse_default_tag_when_missing() {
    let r = Reference::parse("registry.example.com/foo/bar").unwrap();
    assert_eq!(r.reference, "latest");
}

#[test]
fn test_reference_parse_with_digest() {
    let r = Reference::parse(
        "r.example.com/p@sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    )
    .unwrap();
    assert_eq!(r.repository, "p");
    assert_eq!(
        r.reference,
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn test_reference_manifest_and_blob_urls_are_v2_compliant() {
    let r = Reference {
        host: "r.example.com".to_owned(),
        repository: "library/alpine".to_owned(),
        reference: "3.20".to_owned(),
    };
    assert_eq!(
        r.manifest_url(),
        "https://r.example.com/v2/library/alpine/manifests/3.20"
    );
    let dgst = Digest::from_bytes(b"x");
    assert_eq!(
        r.blob_url(&dgst),
        format!("https://r.example.com/v2/library/alpine/blobs/{dgst}")
    );
}

// --- HTTP-driven resolver flows -----------------------------------

fn resolver_with_root() -> (TempDir, Arc<ContentStore>, Resolver) {
    let td = tempfile::tempdir().unwrap();
    let store = rt().block_on(async {
        Arc::new(ContentStore::open(td.path()).await.unwrap())
    });
    // Allow plain HTTP for httpmock in tests.
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let resolver = Resolver::new(client, store.clone());
    (td, store, resolver)
}

#[test]
fn test_resolve_succeeds_on_200_with_correct_digest() {
    let server = MockServer::start();
    let manifest_body = br#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"digest":"sha256:00","size":1},"layers":[]}"#;
    let dgst = Digest::from_bytes(manifest_body);
    let _m = server.mock(|when, then| {
        when.method(Method::GET).path("/v2/lib/img/manifests/v1");
        then.status(200)
            .header("Content-Type", "application/vnd.oci.image.manifest.v1+json")
            .header("Docker-Content-Digest", &dgst.to_string())
            .body(manifest_body);
    });

    let (_td, _store, resolver) = resolver_with_root();
    // Hijack the manifest URL by overriding host with the mock server.
    let r = Reference {
        host: format!("{}:{}", server.host(), server.port()),
        repository: "lib/img".to_owned(),
        reference: "v1".to_owned(),
    };
    // Override https→http for httpmock (test-only escape hatch).
    let resolved = rt().block_on(async {
        resolver.resolve_with_scheme(&r, "http").await.unwrap()
    });
    assert_eq!(resolved.digest, dgst);
    assert_eq!(resolved.manifest, manifest_body);
}

#[test]
fn test_resolve_retries_on_401_with_bearer_token() {
    let server = MockServer::start();
    let manifest_body = br#"{"schemaVersion":2}"#;
    let dgst = Digest::from_bytes(manifest_body);

    // 1) GET manifests → 401 with WWW-Authenticate Bearer
    let token_realm = format!("http://{}:{}/token", server.host(), server.port());
    let _challenge = server.mock(|when, then| {
        when.method(Method::GET).path("/v2/lib/img/manifests/v1").header_exists("X-Test-First");
        then.status(401)
            .header("WWW-Authenticate", &format!("Bearer realm=\"{token_realm}\",service=\"reg\",scope=\"repository:lib/img:pull\""));
    });
    // 2) GET token → 200 with token JSON
    let _token = server.mock(|when, then| {
        when.method(Method::GET).path("/token").query_param("service", "reg");
        then.status(200).json_body(serde_json::json!({"token":"abc.def"}));
    });
    // 3) GET manifests with Authorization: Bearer abc.def → 200
    let _ok = server.mock(|when, then| {
        when.method(Method::GET)
            .path("/v2/lib/img/manifests/v1")
            .header("Authorization", "Bearer abc.def");
        then.status(200)
            .header("Docker-Content-Digest", &dgst.to_string())
            .body(manifest_body);
    });

    let (_td, _store, resolver) = resolver_with_root();
    let r = Reference {
        host: format!("{}:{}", server.host(), server.port()),
        repository: "lib/img".to_owned(),
        reference: "v1".to_owned(),
    };
    let resolved = rt().block_on(async {
        resolver.resolve_with_scheme(&r, "http").await.unwrap()
    });
    assert_eq!(resolved.digest, dgst);
}

#[test]
fn test_fetch_blob_writes_into_content_store() {
    let server = MockServer::start();
    let blob = b"hello-blob";
    let dgst = Digest::from_bytes(blob);
    let _m = server.mock(|when, then| {
        when.method(Method::GET)
            .path(format!("/v2/lib/img/blobs/{dgst}"));
        then.status(200).body(blob);
    });

    let (_td, store, resolver) = resolver_with_root();
    let r = Reference {
        host: format!("{}:{}", server.host(), server.port()),
        repository: "lib/img".to_owned(),
        reference: "v1".to_owned(),
    };
    rt().block_on(async {
        resolver.fetch_blob_with_scheme(&r, &dgst, "http").await.unwrap();
        assert!(store.exists(&dgst).await);
    });
}
