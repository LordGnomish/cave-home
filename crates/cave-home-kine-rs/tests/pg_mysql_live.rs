// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Live external-datastore driver tests for the real `Postgres` and `MySQL`
//! backends. These need a running server, so — exactly like upstream kine's CI
//! matrix — they are gated on a DSN environment variable and **skip-pass** when
//! it is absent:
//!
//! * `KINE_PG_DSN`    e.g. `postgres://kine:kine@127.0.0.1:5432/kine`
//! * `KINE_MYSQL_DSN` e.g. `mysql://kine:kine@127.0.0.1:3306/kine`
//!
//! Run them with, e.g.:
//! `KINE_PG_DSN=postgres://… cargo test -p cave-home-kine-rs --features postgres`
//!
//! Each test drives the full apiserver-shaped lifecycle (create → read → update
//! → delete → lease attach/revoke → watch → compact → defragment) over a real
//! connection, proving the driver end to end against a genuine server.

#![cfg(any(feature = "postgres", feature = "mysql"))]

/// A run-unique key prefix so concurrent / repeated runs don't collide on a
/// shared database.
#[allow(dead_code)]
fn unique_prefix(driver: &str) -> String {
    format!("/kine-it/{driver}/{}/", std::process::id())
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_full_apiserver_cycle_when_dsn_present() {
    use cave_home_kine_rs::postgres::PgStore;
    use cave_home_kine_rs::RangeRequest;

    let Ok(dsn) = std::env::var("KINE_PG_DSN") else {
        eprintln!("SKIP postgres live test: KINE_PG_DSN not set");
        return;
    };

    let mut s = PgStore::connect(&dsn).await.expect("connect to postgres");
    let pfx = unique_prefix("pg");
    let k = format!("{pfx}k").into_bytes();

    // create → read → update → read → delete → gone
    assert!(s.create(&k, b"v1", 0).await.unwrap().is_some());
    assert_eq!(s.range(&RangeRequest::key(&k)).await.unwrap().kvs[0].value, b"v1");
    assert!(s.update(&k, b"v2", 0).await.unwrap().is_some());
    let row = s.range(&RangeRequest::key(&k)).await.unwrap().kvs.remove(0);
    assert_eq!(row.value, b"v2");
    assert!(row.mod_revision > row.create_revision, "update advanced mod_revision");
    assert!(s.delete(&k).await.unwrap().is_some());
    assert!(s.range(&RangeRequest::key(&k)).await.unwrap().kvs.is_empty());

    // lease attach → list → revoke
    let lk = format!("{pfx}leased").into_bytes();
    s.create(&lk, b"x", 4242).await.unwrap();
    assert_eq!(s.keys_with_lease(4242).await.unwrap(), vec![lk.clone()]);
    assert_eq!(s.revoke_lease_keys(4242).await.unwrap(), 1);
    assert!(s.range(&RangeRequest::key(&lk)).await.unwrap().kvs.is_empty());

    // watch replays our change
    let wk = format!("{pfx}watched").into_bytes();
    let start = s.current_revision().await.unwrap();
    s.create(&wk, b"w", 0).await.unwrap();
    let evs = s.watch_after(&RangeRequest::prefix(pfx.as_bytes()), start).await.unwrap();
    assert!(evs.iter().any(|e| e.key == wk && e.value == b"w"), "watch saw the new key");

    // compact (lenient: a shared db may already have a higher floor) + defrag
    let cur = s.current_revision().await.unwrap();
    let _ = s.compact(cur - 1).await; // Ok or CompactionNotForward — both acceptable
    assert!(s.db_size().await.unwrap() > 0);
    s.defragment().await.expect("vacuum");
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn mysql_full_apiserver_cycle_when_dsn_present() {
    use cave_home_kine_rs::mysql::MysqlStore;
    use cave_home_kine_rs::RangeRequest;

    let Ok(dsn) = std::env::var("KINE_MYSQL_DSN") else {
        eprintln!("SKIP mysql live test: KINE_MYSQL_DSN not set");
        return;
    };

    let mut s = MysqlStore::connect(&dsn).await.expect("connect to mysql");
    let pfx = unique_prefix("my");
    let k = format!("{pfx}k").into_bytes();

    assert!(s.create(&k, b"v1", 0).await.unwrap().is_some());
    assert_eq!(s.range(&RangeRequest::key(&k)).await.unwrap().kvs[0].value, b"v1");
    assert!(s.update(&k, b"v2", 0).await.unwrap().is_some());
    let row = s.range(&RangeRequest::key(&k)).await.unwrap().kvs.remove(0);
    assert_eq!(row.value, b"v2");
    assert!(row.mod_revision > row.create_revision);
    assert!(s.delete(&k).await.unwrap().is_some());
    assert!(s.range(&RangeRequest::key(&k)).await.unwrap().kvs.is_empty());

    let lk = format!("{pfx}leased").into_bytes();
    s.create(&lk, b"x", 4242).await.unwrap();
    assert_eq!(s.keys_with_lease(4242).await.unwrap(), vec![lk.clone()]);
    assert_eq!(s.revoke_lease_keys(4242).await.unwrap(), 1);
    assert!(s.range(&RangeRequest::key(&lk)).await.unwrap().kvs.is_empty());

    let wk = format!("{pfx}watched").into_bytes();
    let start = s.current_revision().await.unwrap();
    s.create(&wk, b"w", 0).await.unwrap();
    let evs = s.watch_after(&RangeRequest::prefix(pfx.as_bytes()), start).await.unwrap();
    assert!(evs.iter().any(|e| e.key == wk && e.value == b"w"));

    let cur = s.current_revision().await.unwrap();
    let _ = s.compact(cur - 1).await;
    assert!(s.db_size().await.unwrap() > 0);
    s.defragment().await.expect("optimize table");
}
