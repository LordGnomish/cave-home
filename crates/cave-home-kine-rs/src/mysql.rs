// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The real `MySQL` / `MariaDB` storage backend — kine's other external driver.
//!
//! Like [`crate::postgres`], this is a live driver over [`mysql_async`], sharing
//! the exact query text generated in [`crate::dialect`] (`?` placeholders, no
//! rebinding needed for `MySQL`). The append-only `kine` row log is the same
//! schema with 64-bit `BIGINT` ids/revisions.
//!
//! Two MySQL-specific wrinkles versus `SQLite`/Postgres:
//! * there is no `INSERT … RETURNING`, so the new revision is read back from
//!   `LAST_INSERT_ID()`;
//! * inserting the sentinel at explicit id `0` requires the session
//!   `NO_AUTO_VALUE_ON_ZERO` SQL mode, otherwise `MySQL` would auto-assign id 1.
//!
//! Reference: `k3s-io/kine` `pkg/drivers/mysql`. Faithful behavioural port on a
//! real driver, Apache-2.0.
//!
//! ## Testing
//!
//! Live tests are gated on the `KINE_MYSQL_DSN` environment variable (see
//! `tests/pg_mysql_live.rs`), as kine's own CI matrix needs a running `MySQL`.
//! The shared SQL is unit-tested in [`crate::dialect`] and the MVCC sequencing
//! mirrors the verified `SQLite` backend.

#![cfg(feature = "mysql")]

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row as MyRow, TxOpts, Value};

use crate::backend::{contains, key_str, like_pattern, resolve_read, validate_range};
use crate::dialect::{Dialect, Driver, COMPACT_REV_KEY};
use crate::error::{KineError, Result};
use crate::range::{RangeRequest, RangeResponse};
use crate::revision::Revision;
use crate::store::Row;
use crate::watch::{EventKind, WatchEvent};

/// A live `MySQL`-backed kine datastore.
pub struct MysqlStore {
    conn: Conn,
    dialect: Dialect,
}

/// The latest row for a key: `(id, create_revision, deleted, value)`.
type LatestRow = (i64, i64, bool, Vec<u8>);

impl MysqlStore {
    /// Connect to `dsn` (a `mysql://…` connection string), run the kine DDL, and
    /// seed the compacted-floor sentinel.
    ///
    /// # Errors
    /// [`KineError::Backend`] if the connection or migration fails.
    pub async fn connect(dsn: &str) -> Result<Self> {
        let conn = Conn::from_url(dsn).await.map_err(my_err)?;
        let mut store = Self { conn, dialect: Dialect::new(Driver::Mysql) };
        store.bootstrap().await?;
        Ok(store)
    }

    /// Create the table + indexes (only when newly created, since `MySQL` lacks
    /// `CREATE INDEX IF NOT EXISTS`) and the sentinel row.
    async fn bootstrap(&mut self) -> Result<()> {
        // Honour an explicit id 0 on the AUTO_INCREMENT column for the sentinel.
        self.conn
            .query_drop("SET SESSION sql_mode = CONCAT(@@sql_mode, ',NO_AUTO_VALUE_ON_ZERO')")
            .await
            .map_err(my_err)?;
        let existed: Option<i64> = self
            .conn
            .query_first(
                "SELECT COUNT(*) FROM information_schema.tables \
                 WHERE table_schema = DATABASE() AND table_name = 'kine'",
            )
            .await
            .map_err(my_err)?;
        self.conn.query_drop(self.dialect.create_table_sql()).await.map_err(my_err)?;
        if existed.unwrap_or(0) == 0 {
            for ix in self.dialect.index_sqls() {
                self.conn.query_drop(ix).await.map_err(my_err)?;
            }
        }
        self.ensure_compact_sentinel().await
    }

    /// Ensure the `compact_rev_key` sentinel row exists, pinned at id 0.
    async fn ensure_compact_sentinel(&mut self) -> Result<()> {
        let sentinel = COMPACT_REV_KEY;
        let exists: Option<i64> = self
            .conn
            .exec_first("SELECT EXISTS(SELECT 1 FROM kine WHERE name = ?)", vec![bytes(sentinel.as_bytes())])
            .await
            .map_err(my_err)?;
        if exists.unwrap_or(0) != 0 {
            return Ok(());
        }
        // id, name, created, deleted, create_revision, prev_revision, lease,
        // value, old_value.
        self.conn
            .exec_drop(
                self.dialect.fill_sql(),
                vec![
                    Value::Int(0),
                    bytes(sentinel.as_bytes()),
                    Value::Int(0),
                    Value::Int(1),
                    Value::Int(0),
                    Value::Int(0),
                    Value::Int(0),
                    bytes(&[]),
                    bytes(&[]),
                ],
            )
            .await
            .map_err(my_err)?;
        Ok(())
    }

    /// The current store header revision (`MAX(id)` over real rows).
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub async fn current_revision(&mut self) -> Result<Revision> {
        let sentinel = COMPACT_REV_KEY;
        let v: Option<i64> = self
            .conn
            .exec_first(self.dialect.header_revision_sql(), vec![bytes(sentinel.as_bytes())])
            .await
            .map_err(my_err)?;
        Ok(v.unwrap_or(0))
    }

    /// The compacted-revision floor (`0` if never compacted).
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub async fn compacted_revision(&mut self) -> Result<Revision> {
        let sentinel = COMPACT_REV_KEY;
        let v: Option<i64> = self
            .conn
            .exec_first(self.dialect.compacted_floor_sql(), vec![bytes(sentinel.as_bytes())])
            .await
            .map_err(my_err)?;
        Ok(v.unwrap_or(0))
    }

    /// Create `key` only if it has no live row (etcd create-if-absent).
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub async fn create(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Option<Revision>> {
        let k = key_str(key)?;
        let dialect = self.dialect;
        let mut tx = self.conn.start_transaction(TxOpts::default()).await.map_err(my_err)?;
        let latest = latest_in(&mut tx, dialect, &k).await?;
        if latest.as_ref().is_some_and(|(_, _, deleted, _)| !deleted) {
            tx.rollback().await.map_err(my_err)?;
            return Ok(None);
        }
        let prev_id = latest.map_or(0, |(id, _, _, _)| id);
        let rev = insert_in(&mut tx, dialect, &k, true, false, 0, prev_id, lease, value, &[]).await?;
        tx.commit().await.map_err(my_err)?;
        Ok(Some(rev))
    }

    /// Update `key` only if it has a live row (etcd compare-and-put).
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub async fn update(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Option<Revision>> {
        let k = key_str(key)?;
        let dialect = self.dialect;
        let mut tx = self.conn.start_transaction(TxOpts::default()).await.map_err(my_err)?;
        let Some((prev_id, create_rev, deleted, old)) = latest_in(&mut tx, dialect, &k).await? else {
            tx.rollback().await.map_err(my_err)?;
            return Ok(None);
        };
        if deleted {
            tx.rollback().await.map_err(my_err)?;
            return Ok(None);
        }
        let rev = insert_in(&mut tx, dialect, &k, false, false, create_rev, prev_id, lease, value, &old).await?;
        tx.commit().await.map_err(my_err)?;
        Ok(Some(rev))
    }

    /// Unconditional put: create if absent, else update.
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub async fn put(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Revision> {
        let k = key_str(key)?;
        let dialect = self.dialect;
        let mut tx = self.conn.start_transaction(TxOpts::default()).await.map_err(my_err)?;
        let rev = match latest_in(&mut tx, dialect, &k).await? {
            Some((prev_id, create_rev, false, old)) => {
                insert_in(&mut tx, dialect, &k, false, false, create_rev, prev_id, lease, value, &old).await?
            }
            other => {
                let prev_id = other.map_or(0, |(id, _, _, _)| id);
                insert_in(&mut tx, dialect, &k, true, false, 0, prev_id, lease, value, &[]).await?
            }
        };
        tx.commit().await.map_err(my_err)?;
        Ok(rev)
    }

    /// Delete `key` by appending a tombstone, if it has a live row.
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub async fn delete(&mut self, key: &[u8]) -> Result<Option<Revision>> {
        let k = key_str(key)?;
        let dialect = self.dialect;
        let mut tx = self.conn.start_transaction(TxOpts::default()).await.map_err(my_err)?;
        let Some((prev_id, create_rev, deleted, old)) = latest_in(&mut tx, dialect, &k).await? else {
            tx.rollback().await.map_err(my_err)?;
            return Ok(None);
        };
        if deleted {
            tx.rollback().await.map_err(my_err)?;
            return Ok(None);
        }
        let rev = insert_in(&mut tx, dialect, &k, false, true, create_rev, prev_id, 0, &[], &old).await?;
        tx.commit().await.map_err(my_err)?;
        Ok(Some(rev))
    }

    /// Execute a [`RangeRequest`] and return the current-state (or historical)
    /// view, sorted by key, with etcd's `count`/`more` flags.
    ///
    /// # Errors
    /// Request / revision-guard errors, or [`KineError::Backend`].
    pub async fn range(&mut self, req: &RangeRequest) -> Result<RangeResponse> {
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
    async fn list_rows(&mut self, req: &RangeRequest, read_rev: Revision) -> Result<Vec<Row>> {
        let at_rev = req.revision != 0;
        let pattern = like_pattern(req);
        // MySQL accepts an integer 0/1 in the `OR ?` boolean context.
        let my_rows: Vec<MyRow> = if at_rev {
            self.conn
                .exec(
                    self.dialect.list_revision_sql(),
                    vec![bytes(pattern.as_bytes()), Value::Int(read_rev), Value::Int(0)],
                )
                .await
        } else {
            self.conn
                .exec(self.dialect.list_current_sql(), vec![bytes(pattern.as_bytes()), Value::Int(0)])
                .await
        }
        .map_err(my_err)?;

        let mut rows: Vec<Row> = my_rows.iter().map(project_row).collect();
        rows.retain(|row| row.key != COMPACT_REV_KEY.as_bytes() && contains(req, &row.key));
        Ok(rows)
    }

    /// The watch poll: every change in `filter` with `mod_revision >
    /// start_revision`, as ordered `PUT`/`DELETE` events.
    ///
    /// # Errors
    /// [`KineError::NegativeRevision`] / [`KineError::Compacted`], filter
    /// validation, or [`KineError::Backend`].
    pub async fn watch_after(&mut self, filter: &RangeRequest, start_revision: Revision) -> Result<Vec<WatchEvent>> {
        if start_revision < 0 {
            return Err(KineError::NegativeRevision { revision: start_revision });
        }
        let floor = self.compacted_revision().await?;
        if start_revision != 0 && start_revision < floor {
            return Err(KineError::Compacted { requested: start_revision, compacted: floor });
        }
        validate_range(filter)?;

        let pattern = like_pattern(filter);
        let my_rows: Vec<MyRow> = self
            .conn
            .exec(self.dialect.after_sql(), vec![bytes(pattern.as_bytes()), Value::Int(start_revision)])
            .await
            .map_err(my_err)?;
        let mut events: Vec<WatchEvent> = my_rows.iter().map(project_event).collect();
        events.retain(|e| e.key != COMPACT_REV_KEY.as_bytes() && contains(filter, &e.key));
        Ok(events)
    }

    /// Compact history up to and including `target`.
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
        let dialect = self.dialect;
        let mut tx = self.conn.start_transaction(TxOpts::default()).await.map_err(my_err)?;
        tx.exec_drop(dialect.compact_delete_sql(), vec![bytes(sentinel.as_bytes()), Value::Int(target)])
            .await
            .map_err(my_err)?;
        tx.exec_drop(dialect.update_compact_sql(), vec![Value::Int(target)]).await.map_err(my_err)?;
        tx.commit().await.map_err(my_err)?;
        let after = self.count_rows().await?;

        let removed = usize::try_from(before - after).unwrap_or(0);
        Ok(crate::compact::CompactReport {
            compacted: target,
            removed,
            remaining: usize::try_from(after).unwrap_or(0),
        })
    }

    /// Count real (non-sentinel) rows.
    async fn count_rows(&mut self) -> Result<i64> {
        let sentinel = COMPACT_REV_KEY;
        let v: Option<i64> =
            self.conn.exec_first(self.dialect.count_rows_sql(), vec![bytes(sentinel.as_bytes())]).await.map_err(my_err)?;
        Ok(v.unwrap_or(0))
    }

    /// Every *live* key attached to lease `lease_id`, sorted ascending.
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub async fn keys_with_lease(&mut self, lease_id: i64) -> Result<Vec<Vec<u8>>> {
        if lease_id == 0 {
            return Ok(Vec::new());
        }
        let sentinel = COMPACT_REV_KEY;
        let rows: Vec<MyRow> = self
            .conn
            .exec(self.dialect.keys_with_lease_sql(), vec![Value::Int(lease_id), bytes(sentinel.as_bytes())])
            .await
            .map_err(my_err)?;
        Ok(rows.iter().map(|r| get_string(r, 0).into_bytes()).collect())
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

    /// The kine table's size in bytes (`data_length + index_length`), etcd's
    /// `Status.dbSize`.
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub async fn db_size(&mut self) -> Result<i64> {
        let v: Option<i64> = self
            .conn
            .query_first(
                "SELECT COALESCE(data_length + index_length, 0) FROM information_schema.tables \
                 WHERE table_schema = DATABASE() AND table_name = 'kine'",
            )
            .await
            .map_err(my_err)?;
        Ok(v.unwrap_or(0))
    }

    /// Defragment the table (`OPTIMIZE TABLE`): rebuild it to reclaim the free
    /// space left by compaction. Returns the bytes reclaimed (`0` if it did not
    /// shrink).
    ///
    /// # Errors
    /// [`KineError::Backend`] if the rebuild fails.
    pub async fn defragment(&mut self) -> Result<i64> {
        let before = self.db_size().await?;
        self.conn.query_drop("OPTIMIZE TABLE kine").await.map_err(my_err)?;
        let after = self.db_size().await?;
        Ok((before - after).max(0))
    }
}

/// The latest row for `key` via `q` (a `Conn` or `Transaction`).
async fn latest_in<Q: Queryable>(q: &mut Q, dialect: Dialect, key: &str) -> Result<Option<LatestRow>> {
    let row: Option<MyRow> =
        q.exec_first(dialect.latest_row_sql(), vec![bytes(key.as_bytes())]).await.map_err(my_err)?;
    Ok(row.map(|r| (get_i64(&r, 0), get_i64(&r, 1), get_i64(&r, 2) != 0, get_bytes(&r, 3))))
}

/// Insert one append-only row and return its id (the new revision, read from
/// `LAST_INSERT_ID()`). For a brand-new generation, `create_revision` is fixed
/// up to the inserted id.
#[allow(clippy::too_many_arguments)]
async fn insert_in<Q: Queryable>(
    q: &mut Q,
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
    q.exec_drop(
        dialect.insert_sql(),
        vec![
            bytes(key.as_bytes()),
            Value::Int(i64::from(created)),
            Value::Int(i64::from(deleted)),
            Value::Int(create_revision),
            Value::Int(prev_revision),
            Value::Int(lease),
            bytes(value),
            bytes(old_value),
        ],
    )
    .await
    .map_err(my_err)?;
    // LAST_INSERT_ID() is connection-scoped and, inside this transaction, returns
    // the id the AUTO_INCREMENT just assigned. It is portable across Conn and
    // Transaction (the `last_insert_id()` accessor is not on the Queryable trait).
    let id: i64 = q
        .query_first("SELECT LAST_INSERT_ID()")
        .await
        .map_err(my_err)?
        .filter(|id| *id != 0)
        .ok_or_else(|| KineError::Backend {
            message: "MySQL returned no LAST_INSERT_ID after insert".into(),
        })?;
    if created {
        q.exec_drop(dialect.set_create_revision_sql(), vec![Value::Int(id), Value::Int(id)])
            .await
            .map_err(my_err)?;
    }
    Ok(id)
}

/// A `Value::Bytes` from a byte slice.
fn bytes(b: &[u8]) -> Value {
    Value::Bytes(b.to_vec())
}

/// Read an `i64` column (defaulting to 0 on a NULL / missing column).
fn get_i64(r: &MyRow, idx: usize) -> i64 {
    r.get::<i64, usize>(idx).unwrap_or_default()
}

/// Read a byte-blob column (defaulting to empty).
fn get_bytes(r: &MyRow, idx: usize) -> Vec<u8> {
    r.get::<Vec<u8>, usize>(idx).unwrap_or_default()
}

/// Read a string column (the `name`), defaulting to empty.
fn get_string(r: &MyRow, idx: usize) -> String {
    r.get::<String, usize>(idx).unwrap_or_default()
}

/// Project a list-query row onto a kine [`Row`]. The column layout is shared
/// with the `SQLite` backend: `crev, compact, id, name, created, deleted,
/// create_revision, prev_revision, lease, value, old_value`.
fn project_row(r: &MyRow) -> Row {
    Row {
        key: get_string(r, 3).into_bytes(),
        create_revision: get_i64(r, 6),
        mod_revision: get_i64(r, 2),
        value: get_bytes(r, 9),
        lease: get_i64(r, 8),
        deleted: get_i64(r, 5) != 0,
    }
}

/// Project an after-query row onto a [`WatchEvent`].
fn project_event(r: &MyRow) -> WatchEvent {
    let deleted = get_i64(r, 5) != 0;
    WatchEvent {
        kind: if deleted { EventKind::Delete } else { EventKind::Put },
        key: get_string(r, 3).into_bytes(),
        value: get_bytes(r, 9),
        prev_value: get_bytes(r, 10),
        revision: get_i64(r, 2),
        create_revision: get_i64(r, 6),
    }
}

/// Wrap a `mysql_async` error as a backend error.
#[allow(clippy::needless_pass_by_value)]
fn my_err(e: mysql_async::Error) -> KineError {
    KineError::Backend { message: e.to_string() }
}
