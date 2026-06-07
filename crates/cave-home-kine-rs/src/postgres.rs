// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The real `PostgreSQL` storage backend — kine's external-datastore driver.
//!
//! This is the live Postgres driver, not a model: it dials a real server with
//! [`tokio_postgres`], runs the kine DDL from [`crate::dialect`], and executes
//! every etcd-MVCC operation as parameterised SQL over the wire. It shares the
//! exact query text of the [`SQLite`](crate::sqlite) backend (generated once in
//! [`crate::dialect`] and rebound to `$N` placeholders for Postgres), so the two
//! drivers stay in lock-step; only the connection / type-mapping differs.
//!
//! The append-only `kine` row log is identical to the `SQLite` schema, with
//! 64-bit (`BIGSERIAL` / `BIGINT`) ids and revisions. Postgres's strict typing
//! requires the `(kv.deleted = 0 OR ?)` include-tombstones flag to bind as a
//! real `bool`, which is the one place the parameter type diverges from `SQLite`.
//!
//! Reference: `k3s-io/kine` `pkg/drivers/pgsql`. Faithful behavioural port on a
//! real driver, Apache-2.0.
//!
//! ## Testing
//!
//! Live tests need a running server and are gated on the `KINE_PG_DSN`
//! environment variable (see `tests/pg_mysql_live.rs`), mirroring upstream
//! kine's own CI matrix. The shared SQL these methods issue is unit-tested in
//! [`crate::dialect`], and the MVCC sequencing mirrors the `SQLite` backend's
//! verified logic.

#![cfg(feature = "postgres")]

use tokio_postgres::{Client, GenericClient, NoTls};

use crate::backend::{contains, key_str, like_pattern, resolve_read, validate_range};
use crate::dialect::{Dialect, Driver, COMPACT_REV_KEY};
use crate::error::{KineError, Result};
use crate::range::{RangeRequest, RangeResponse};
use crate::revision::Revision;
use crate::store::Row;
use crate::watch::{EventKind, WatchEvent};

/// A live `PostgreSQL`-backed kine datastore.
pub struct PgStore {
    client: Client,
    dialect: Dialect,
}

/// The latest row for a key: `(id, create_revision, deleted, value)`.
type LatestRow = (i64, i64, bool, Vec<u8>);

impl PgStore {
    /// Connect to `dsn` (a `postgres://…` connection string), run the kine DDL,
    /// and seed the compacted-floor sentinel. The connection's background task is
    /// spawned onto the current Tokio runtime.
    ///
    /// # Errors
    /// [`KineError::Backend`] if the connection or migration fails.
    pub async fn connect(dsn: &str) -> Result<Self> {
        let (client, connection) = tokio_postgres::connect(dsn, NoTls).await.map_err(pg_err)?;
        // The connection object drives the protocol; it must be polled to make
        // progress, so run it on its own task for the client's lifetime.
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let store = Self { client, dialect: Dialect::new(Driver::Postgres) };
        store.bootstrap().await?;
        Ok(store)
    }

    /// Create the table + indexes (idempotent) and the sentinel row.
    async fn bootstrap(&self) -> Result<()> {
        self.client.batch_execute(&self.dialect.create_table_sql()).await.map_err(pg_err)?;
        for ix in self.dialect.index_sqls() {
            // Index creation races are benign across replicas; ignore "already
            // exists" by using IF NOT EXISTS (the dialect emits it for Postgres).
            self.client.batch_execute(&ix).await.map_err(pg_err)?;
        }
        self.ensure_compact_sentinel().await
    }

    /// Ensure the `compact_rev_key` sentinel row exists (floor starts at 0),
    /// pinned at id 0 so user writes auto-increment from 1.
    async fn ensure_compact_sentinel(&self) -> Result<()> {
        let sentinel = COMPACT_REV_KEY;
        let row = self
            .client
            .query_one("SELECT EXISTS(SELECT 1 FROM kine WHERE name = $1)", &[&sentinel])
            .await
            .map_err(pg_err)?;
        if row.get::<_, bool>(0) {
            return Ok(());
        }
        // id, name, created, deleted, create_revision, prev_revision, lease,
        // value, old_value — deleted=1 keeps it out of every current read.
        let empty: &[u8] = &[];
        self.client
            .execute(
                &self.dialect.fill_sql(),
                &[&0_i64, &sentinel, &0_i64, &1_i64, &0_i64, &0_i64, &0_i64, &empty, &empty],
            )
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    /// The current store header revision (`MAX(id)` over real rows).
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub async fn current_revision(&self) -> Result<Revision> {
        let sentinel = COMPACT_REV_KEY;
        let row = self
            .client
            .query_one(&self.dialect.header_revision_sql(), &[&sentinel])
            .await
            .map_err(pg_err)?;
        Ok(row.get::<_, i64>(0))
    }

    /// The compacted-revision floor (`0` if never compacted).
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub async fn compacted_revision(&self) -> Result<Revision> {
        let sentinel = COMPACT_REV_KEY;
        let row = self
            .client
            .query_one(&self.dialect.compacted_floor_sql(), &[&sentinel])
            .await
            .map_err(pg_err)?;
        Ok(row.get::<_, i64>(0))
    }

    /// Create `key` only if it has no live row (etcd create-if-absent).
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub async fn create(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Option<Revision>> {
        let k = key_str(key)?;
        let txn = self.client.transaction().await.map_err(pg_err)?;
        let latest = latest_in(&txn, self.dialect, &k).await?;
        if latest.as_ref().is_some_and(|(_, _, deleted, _)| !deleted) {
            txn.rollback().await.map_err(pg_err)?;
            return Ok(None);
        }
        let prev_id = latest.map_or(0, |(id, _, _, _)| id);
        let rev = insert_in(&txn, self.dialect, &k, true, false, 0, prev_id, lease, value, &[]).await?;
        txn.commit().await.map_err(pg_err)?;
        Ok(Some(rev))
    }

    /// Update `key` only if it has a live row (etcd compare-and-put).
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub async fn update(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Option<Revision>> {
        let k = key_str(key)?;
        let txn = self.client.transaction().await.map_err(pg_err)?;
        let Some((prev_id, create_rev, deleted, old)) = latest_in(&txn, self.dialect, &k).await? else {
            txn.rollback().await.map_err(pg_err)?;
            return Ok(None);
        };
        if deleted {
            txn.rollback().await.map_err(pg_err)?;
            return Ok(None);
        }
        let rev = insert_in(&txn, self.dialect, &k, false, false, create_rev, prev_id, lease, value, &old).await?;
        txn.commit().await.map_err(pg_err)?;
        Ok(Some(rev))
    }

    /// Unconditional put: create if absent, else update. Always writes a row.
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub async fn put(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Revision> {
        let k = key_str(key)?;
        let txn = self.client.transaction().await.map_err(pg_err)?;
        let rev = match latest_in(&txn, self.dialect, &k).await? {
            Some((prev_id, create_rev, false, old)) => {
                insert_in(&txn, self.dialect, &k, false, false, create_rev, prev_id, lease, value, &old).await?
            }
            other => {
                let prev_id = other.map_or(0, |(id, _, _, _)| id);
                insert_in(&txn, self.dialect, &k, true, false, 0, prev_id, lease, value, &[]).await?
            }
        };
        txn.commit().await.map_err(pg_err)?;
        Ok(rev)
    }

    /// Delete `key` by appending a tombstone, if it has a live row.
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub async fn delete(&mut self, key: &[u8]) -> Result<Option<Revision>> {
        let k = key_str(key)?;
        let txn = self.client.transaction().await.map_err(pg_err)?;
        let Some((prev_id, create_rev, deleted, old)) = latest_in(&txn, self.dialect, &k).await? else {
            txn.rollback().await.map_err(pg_err)?;
            return Ok(None);
        };
        if deleted {
            txn.rollback().await.map_err(pg_err)?;
            return Ok(None);
        }
        let rev = insert_in(&txn, self.dialect, &k, false, true, create_rev, prev_id, 0, &[], &old).await?;
        txn.commit().await.map_err(pg_err)?;
        Ok(Some(rev))
    }

    /// Execute a [`RangeRequest`] and return the current-state (or historical)
    /// view, sorted by key, with etcd's `count`/`more` flags.
    ///
    /// # Errors
    /// Request / revision-guard errors, or [`KineError::Backend`].
    pub async fn range(&self, req: &RangeRequest) -> Result<RangeResponse> {
        validate_range(req)?;
        let header = self.current_revision().await?;
        let read_rev = resolve_read(req.revision, header)?;
        let floor = self.compacted_revision().await?;
        if req.revision != 0 && req.revision < floor {
            return Err(KineError::Compacted { requested: req.revision, compacted: floor });
        }

        let mut rows = self.list_rows(req, read_rev).await?;
        rows.sort_by(|a, b| a.key.cmp(&b.key));
        let count = rows.len() as i64;
        let (kvs, more) = if req.limit > 0 && count > req.limit {
            let take = usize::try_from(req.limit).unwrap_or(usize::MAX);
            (rows[..take].to_vec(), true)
        } else {
            (rows, false)
        };
        Ok(RangeResponse { revision: read_rev, kvs, more, count })
    }

    /// Run the latest-row-per-key list query for `req` at `read_rev`.
    async fn list_rows(&self, req: &RangeRequest, read_rev: Revision) -> Result<Vec<Row>> {
        let at_rev = req.revision != 0;
        let pattern = like_pattern(req);
        // Postgres demands a real boolean for the `OR ?` include-tombstones flag.
        let include_deleted = false;
        let pg_rows = if at_rev {
            self.client
                .query(&self.dialect.list_revision_sql(), &[&pattern, &read_rev, &include_deleted])
                .await
        } else {
            self.client
                .query(&self.dialect.list_current_sql(), &[&pattern, &include_deleted])
                .await
        }
        .map_err(pg_err)?;

        let mut rows: Vec<Row> = pg_rows.iter().map(project_row).collect();
        rows.retain(|row| row.key != COMPACT_REV_KEY.as_bytes() && contains(req, &row.key));
        Ok(rows)
    }

    /// The watch poll: every change in `filter` with `mod_revision >
    /// start_revision`, as ordered `PUT`/`DELETE` events. Faithful to kine's
    /// `After`.
    ///
    /// # Errors
    /// [`KineError::NegativeRevision`] / [`KineError::Compacted`], filter
    /// validation, or [`KineError::Backend`].
    pub async fn watch_after(&self, filter: &RangeRequest, start_revision: Revision) -> Result<Vec<WatchEvent>> {
        if start_revision < 0 {
            return Err(KineError::NegativeRevision { revision: start_revision });
        }
        let floor = self.compacted_revision().await?;
        if start_revision != 0 && start_revision < floor {
            return Err(KineError::Compacted { requested: start_revision, compacted: floor });
        }
        validate_range(filter)?;

        let pattern = like_pattern(filter);
        let pg_rows =
            self.client.query(&self.dialect.after_sql(), &[&pattern, &start_revision]).await.map_err(pg_err)?;
        let mut events: Vec<WatchEvent> = pg_rows.iter().map(project_event).collect();
        events.retain(|e| e.key != COMPACT_REV_KEY.as_bytes() && contains(filter, &e.key));
        Ok(events)
    }

    /// Compact history up to and including `target`: physically delete every
    /// superseded / tombstoned row at or below `target` that is not the surviving
    /// live row of its key, then advance the recorded floor.
    ///
    /// # Errors
    /// Revision-guard errors or [`KineError::Backend`].
    pub async fn compact(&mut self, target: Revision) -> Result<crate::compact::CompactReport> {
        if target < 0 {
            return Err(KineError::NegativeRevision { revision: target });
        }
        let current = self.current_revision().await?;
        if target > current {
            return Err(KineError::CompactFutureRevision { requested: target, current });
        }
        let floor = self.compacted_revision().await?;
        if target <= floor {
            return Err(KineError::CompactionNotForward { requested: target, current: floor });
        }

        let before = self.count_rows().await?;
        let sentinel = COMPACT_REV_KEY;
        let txn = self.client.transaction().await.map_err(pg_err)?;
        txn.execute(&self.dialect.compact_delete_sql(), &[&sentinel, &target]).await.map_err(pg_err)?;
        txn.execute(&self.dialect.update_compact_sql(), &[&target]).await.map_err(pg_err)?;
        txn.commit().await.map_err(pg_err)?;
        let after = self.count_rows().await?;

        let removed = usize::try_from(before - after).unwrap_or(0);
        Ok(crate::compact::CompactReport {
            compacted: target,
            removed,
            remaining: usize::try_from(after).unwrap_or(0),
        })
    }

    /// Count real (non-sentinel) rows.
    async fn count_rows(&self) -> Result<i64> {
        let sentinel = COMPACT_REV_KEY;
        let row = self.client.query_one(&self.dialect.count_rows_sql(), &[&sentinel]).await.map_err(pg_err)?;
        Ok(row.get::<_, i64>(0))
    }

    /// Every *live* key attached to lease `lease_id`, sorted ascending.
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub async fn keys_with_lease(&self, lease_id: i64) -> Result<Vec<Vec<u8>>> {
        if lease_id == 0 {
            return Ok(Vec::new());
        }
        let sentinel = COMPACT_REV_KEY;
        let rows =
            self.client.query(&self.dialect.keys_with_lease_sql(), &[&lease_id, &sentinel]).await.map_err(pg_err)?;
        Ok(rows.iter().map(|r| r.get::<_, String>(0).into_bytes()).collect())
    }

    /// Revoke lease `lease_id`: tombstone every live key attached to it.
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub async fn revoke_lease_keys(&mut self, lease_id: i64) -> Result<usize> {
        let keys = self.keys_with_lease(lease_id).await?;
        let mut deleted = 0;
        for key in keys {
            if self.delete(&key).await?.is_some() {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// The physical size of the kine relation in bytes (etcd's `Status.dbSize`).
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub async fn db_size(&self) -> Result<i64> {
        let row = self.client.query_one("SELECT pg_total_relation_size('kine')", &[]).await.map_err(pg_err)?;
        Ok(row.get::<_, i64>(0))
    }

    /// Defragment the relation (`VACUUM FULL`): rebuild it so the free space left
    /// by compaction's `DELETE`s is returned to the filesystem. Returns the bytes
    /// reclaimed (`0` if it did not shrink). Cannot run inside a transaction, so
    /// it is issued as a standalone statement.
    ///
    /// # Errors
    /// [`KineError::Backend`] if the rebuild fails.
    pub async fn defragment(&self) -> Result<i64> {
        let before = self.db_size().await?;
        self.client.batch_execute("VACUUM FULL kine").await.map_err(pg_err)?;
        let after = self.db_size().await?;
        Ok((before - after).max(0))
    }
}

/// The latest row for `key` via `client` (a `Client` or `Transaction`).
async fn latest_in<C: GenericClient + Sync>(client: &C, dialect: Dialect, key: &str) -> Result<Option<LatestRow>> {
    let row = client.query_opt(&dialect.latest_row_sql(), &[&key]).await.map_err(pg_err)?;
    Ok(row.map(|r| {
        (r.get::<_, i64>(0), r.get::<_, i64>(1), r.get::<_, i64>(2) != 0, r.get::<_, Vec<u8>>(3))
    }))
}

/// Insert one append-only row and return its id (the new revision). For a brand
/// new generation (`created`), `create_revision` is fixed up to the inserted id,
/// reproducing kine's "id is the create rev".
#[allow(clippy::too_many_arguments)]
async fn insert_in<C: GenericClient + Sync>(
    client: &C,
    dialect: Dialect,
    key: &str,
    created: bool,
    deleted: bool,
    create_revision: i64,
    prev_revision: i64,
    lease: i64,
    value: &[u8],
    old_value: &[u8],
) -> Result<i64> {
    let insert = format!("{} RETURNING id", dialect.insert_sql());
    let created_i = i64::from(created);
    let deleted_i = i64::from(deleted);
    let row = client
        .query_one(
            &insert,
            &[&key, &created_i, &deleted_i, &create_revision, &prev_revision, &lease, &value, &old_value],
        )
        .await
        .map_err(pg_err)?;
    let id: i64 = row.get(0);
    if created {
        client.execute(&dialect.set_create_revision_sql(), &[&id, &id]).await.map_err(pg_err)?;
    }
    Ok(id)
}

/// Project a list-query row onto a kine [`Row`]. The column layout is shared
/// with the `SQLite` backend: `crev, compact, id, name, created, deleted,
/// create_revision, prev_revision, lease, value, old_value`.
fn project_row(r: &tokio_postgres::Row) -> Row {
    Row {
        key: r.get::<_, String>(3).into_bytes(),
        create_revision: r.get::<_, i64>(6),
        mod_revision: r.get::<_, i64>(2),
        value: r.get::<_, Vec<u8>>(9),
        lease: r.get::<_, i64>(8),
        deleted: r.get::<_, i64>(5) != 0,
    }
}

/// Project an after-query row onto a [`WatchEvent`].
fn project_event(r: &tokio_postgres::Row) -> WatchEvent {
    let deleted = r.get::<_, i64>(5) != 0;
    WatchEvent {
        kind: if deleted { EventKind::Delete } else { EventKind::Put },
        key: r.get::<_, String>(3).into_bytes(),
        value: r.get::<_, Vec<u8>>(9),
        prev_value: r.get::<_, Vec<u8>>(10),
        revision: r.get::<_, i64>(2),
        create_revision: r.get::<_, i64>(6),
    }
}

/// Wrap a `tokio_postgres` error as a backend error.
#[allow(clippy::needless_pass_by_value)]
fn pg_err(e: tokio_postgres::Error) -> KineError {
    KineError::Backend { message: e.to_string() }
}
