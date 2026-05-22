// SPDX-License-Identifier: Apache-2.0
//! Content store tests — port of upstream's
//! `plugins/content/local/store_test.go` (sha256 happy paths +
//! digest-mismatch + already-exists + walk + delete).

use cave_home_containerd_rs::content::{Digest, Store, StoreError};
use tempfile::TempDir;

fn store_root() -> TempDir {
    tempfile::tempdir().unwrap()
}

#[tokio::test]
async fn test_open_creates_layout() {
    let td = store_root();
    let _store = Store::open(td.path()).await.unwrap();
    assert!(td.path().join("blobs/sha256").is_dir());
    assert!(td.path().join("ingest").is_dir());
}

#[tokio::test]
async fn test_digest_from_bytes_matches_known_sha256() {
    // Standard "" → e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    let d = Digest::from_bytes(b"");
    assert_eq!(
        d.to_string(),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[tokio::test]
async fn test_digest_parse_accepts_canonical() {
    let s = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let d = Digest::parse(s).unwrap();
    assert_eq!(d.to_string(), s);
}

#[tokio::test]
async fn test_digest_parse_rejects_garbage() {
    assert!(matches!(
        Digest::parse("md5:00"),
        Err(StoreError::InvalidDigest(_))
    ));
    assert!(matches!(
        Digest::parse("sha256:zz"),
        Err(StoreError::InvalidDigest(_))
    ));
}

#[tokio::test]
async fn test_write_then_read_roundtrips() {
    let td = store_root();
    let s = Store::open(td.path()).await.unwrap();
    let bytes = b"hello containerd";
    let dgst = Digest::from_bytes(bytes);

    s.write(&dgst, bytes).await.unwrap();
    let out = s.read(&dgst).await.unwrap();
    assert_eq!(out, bytes);
}

#[tokio::test]
async fn test_write_rejects_digest_mismatch() {
    let td = store_root();
    let s = Store::open(td.path()).await.unwrap();
    let wrong = Digest::from_bytes(b"NOT THE BYTES");
    let err = s.write(&wrong, b"these are the bytes").await.unwrap_err();
    assert!(matches!(err, StoreError::DigestMismatch { .. }));
    // No file should have landed in blobs/.
    assert!(!s.exists(&wrong).await);
}

#[tokio::test]
async fn test_double_write_returns_already_exists() {
    let td = store_root();
    let s = Store::open(td.path()).await.unwrap();
    let bytes = b"abc";
    let dgst = Digest::from_bytes(bytes);

    s.write(&dgst, bytes).await.unwrap();
    let err = s.write(&dgst, bytes).await.unwrap_err();
    assert!(matches!(err, StoreError::AlreadyExists(_)));
}

#[tokio::test]
async fn test_info_returns_size() {
    let td = store_root();
    let s = Store::open(td.path()).await.unwrap();
    let bytes = vec![0u8; 4096];
    let dgst = Digest::from_bytes(&bytes);
    s.write(&dgst, &bytes).await.unwrap();
    let info = s.info(&dgst).await.unwrap();
    assert_eq!(info.size, 4096);
    assert_eq!(info.digest, dgst);
}

#[tokio::test]
async fn test_info_missing_returns_not_found() {
    let td = store_root();
    let s = Store::open(td.path()).await.unwrap();
    let dgst = Digest::from_bytes(b"never-written");
    let err = s.info(&dgst).await.unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}

#[tokio::test]
async fn test_delete_removes_blob() {
    let td = store_root();
    let s = Store::open(td.path()).await.unwrap();
    let bytes = b"to-be-deleted";
    let dgst = Digest::from_bytes(bytes);
    s.write(&dgst, bytes).await.unwrap();
    s.delete(&dgst).await.unwrap();
    assert!(!s.exists(&dgst).await);
    assert!(matches!(
        s.delete(&dgst).await.unwrap_err(),
        StoreError::NotFound(_)
    ));
}

#[tokio::test]
async fn test_walk_visits_all_committed() {
    let td = store_root();
    let s = Store::open(td.path()).await.unwrap();
    let payloads: Vec<&[u8]> = vec![b"alpha", b"beta", b"gamma"];
    let mut want: Vec<Digest> = Vec::new();
    for p in &payloads {
        let d = Digest::from_bytes(p);
        s.write(&d, p).await.unwrap();
        want.push(d);
    }

    let mut got: Vec<Digest> = Vec::new();
    s.walk(|info| got.push(info.digest.clone())).await.unwrap();
    got.sort_by_key(|d| d.hex().to_owned());
    want.sort_by_key(|d| d.hex().to_owned());
    assert_eq!(got, want);
}
