// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The real `SQLite` storage backend — kine's single-binary default datastore.
//!
//! This is the actual driver, not a model: it opens a real `SQLite` database
//! (embedded, bundled — no server, one file), runs the kine DDL from
//! [`crate::dialect`], and executes every etcd-MVCC operation as live SQL
//! against it. The append-only row log lives in the `kine` table; the auto-
//! increment `id` is the global revision; the latest-row-per-key join is pushed
//! down to `SQLite`. The pure semantics proven in [`crate::store`] /
//! [`crate::range`] / [`crate::watch`] / [`crate::compact`] are here realised on
//! real storage.
//!
//! Keys are k8s/etcd registry paths and are stored in the `name TEXT` column as
//! UTF-8, exactly as kine does (kine keys are always valid UTF-8 strings); a
//! non-UTF-8 key is rejected. Prefix scans use `name LIKE 'prefix%'`, the
//! convention kine relies on (registry keys never contain `%`/`_`).
//!
//! Reference: `k3s-io/kine` `pkg/drivers/generic/generic.go` (the SQL) and
//! `pkg/logstructured/sqllog` (the create/update/delete/list/after/compact
//! flow). Faithful behavioural port on a real driver, Apache-2.0.

#![cfg(feature = "sqlite")]

use std::path::Path;

use rusqlite::Connection;

use crate::dialect::{Dialect, Driver, COMPACT_REV_KEY};
use crate::error::{KineError, Result};
use crate::range::{RangeEnd, RangeRequest, RangeResponse};
use crate::revision::Revision;
use crate::store::Row;
use crate::watch::{EventKind, WatchEvent};

/// A live `SQLite`-backed kine datastore.
pub struct SqliteStore {
    conn: Connection,
    dialect: Dialect,
}

/// The latest row for a key: `(id, create_revision, deleted, value)`.
type LatestRow = (i64, i64, bool, Vec<u8>);

impl SqliteStore {
    /// Open an in-memory store (each instance is independent). Used for tests
    /// and ephemeral nodes.
    ///
    /// # Errors
    /// [`KineError::Backend`] if the database cannot be opened or migrated.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(backend)?;
        Self::bootstrap(conn)
    }

    /// Open (creating if absent) a file-backed store at `path` — the persistent
    /// single-binary datastore.
    ///
    /// # Errors
    /// [`KineError::Backend`] if the file cannot be opened or migrated.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(backend)?;
        Self::bootstrap(conn)
    }

    /// Run the DDL + seed the compacted-floor sentinel row.
    fn bootstrap(conn: Connection) -> Result<Self> {
        let dialect = Dialect::new(Driver::Sqlite);
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(backend)?;
        conn.execute(&dialect.create_table_sql(), []).map_err(backend)?;
        for ix in dialect.index_sqls() {
            conn.execute(&ix, []).map_err(backend)?;
        }
        let store = Self { conn, dialect };
        store.ensure_compact_sentinel()?;
        Ok(store)
    }

    /// Ensure the `compact_rev_key` sentinel row exists (floor starts at 0).
    fn ensure_compact_sentinel(&self) -> Result<()> {
        let exists: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM kine WHERE name = ?)",
                [COMPACT_REV_KEY],
                |r| r.get(0),
            )
            .map_err(backend)?;
        if !exists {
            // The sentinel is pinned at id 0 (explicit rowid) so it does NOT
            // consume revision 1: user writes auto-increment from 1, matching
            // etcd's "first write is revision 1". deleted=1 keeps it out of
            // every current-state read; prev_revision holds the compacted floor.
            self.conn
                .execute(
                    "INSERT INTO kine(id, name, created, deleted, create_revision, \
                     prev_revision, lease, value, old_value) \
                     VALUES(0, ?, 0, 1, 0, 0, 0, x'', x'')",
                    [COMPACT_REV_KEY],
                )
                .map_err(backend)?;
        }
        Ok(())
    }

    /// The current store header revision (`MAX(id)` over real rows).
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub fn current_revision(&self) -> Result<Revision> {
        // The sentinel row occupies an id but is not a user revision; the
        // header is the max id of any real row, or 0 if only the sentinel.
        let rev: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(id), 0) FROM kine WHERE name <> ?",
                [COMPACT_REV_KEY],
                |r| r.get(0),
            )
            .map_err(backend)?;
        Ok(rev)
    }

    /// The compacted-revision floor (`0` if never compacted).
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub fn compacted_revision(&self) -> Result<Revision> {
        let floor: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(prev_revision), 0) FROM kine WHERE name = ?",
                [COMPACT_REV_KEY],
                |r| r.get(0),
            )
            .map_err(backend)?;
        Ok(floor)
    }

    /// The latest row (live or tombstone) for `key`: `(id, create_revision,
    /// deleted, value)`. `None` if the key was never written.
    fn latest(&self, key: &str) -> Result<Option<LatestRow>> {
        self.conn
            .query_row(
                "SELECT id, create_revision, deleted, value FROM kine \
                 WHERE name = ? ORDER BY id DESC LIMIT 1",
                [key],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)? != 0,
                        r.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(backend(other)),
            })
    }

    /// Insert one append-only row and return its id (the new revision). For a
    /// brand-new generation (`created`), `create_revision` is fixed up to the
    /// inserted id after the fact, reproducing kine's "id is the create rev".
    ///
    /// The argument list is the kine row's column set (name + the eight value
    /// columns); grouping them into a struct would only obscure the 1:1 mapping
    /// onto the `INSERT` bind order.
    #[allow(clippy::too_many_arguments)]
    fn insert(
        &self,
        key: &str,
        created: bool,
        deleted: bool,
        create_revision: i64,
        prev_revision: i64,
        lease: i64,
        value: &[u8],
        old_value: &[u8],
    ) -> Result<i64> {
        self.conn
            .execute(
                &self.dialect.insert_sql(),
                rusqlite::params![
                    key,
                    i64::from(created),
                    i64::from(deleted),
                    create_revision,
                    prev_revision,
                    lease,
                    value,
                    old_value,
                ],
            )
            .map_err(backend)?;
        let id = self.conn.last_insert_rowid();
        if created {
            self.conn
                .execute("UPDATE kine SET create_revision = ? WHERE id = ?", [id, id])
                .map_err(backend)?;
        }
        Ok(id)
    }

    /// Create `key` only if it has no live row. Returns the new revision, or
    /// `None` if a live row already exists (etcd create-if-absent).
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub fn create(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Option<Revision>> {
        let k = key_str(key)?;
        let latest = self.latest(&k)?;
        if latest.as_ref().is_some_and(|(_, _, deleted, _)| !deleted) {
            return Ok(None);
        }
        // prev_revision points at the row this one supersedes — the prior
        // tombstone when re-creating a deleted key, else 0. This keeps the
        // unique (name, prev_revision) chain intact across generations.
        let prev_id = latest.map_or(0, |(id, _, _, _)| id);
        let rev = self.insert(&k, true, false, 0, prev_id, lease, value, &[])?;
        Ok(Some(rev))
    }

    /// Update `key` only if it has a live row (etcd compare-and-put). Keeps the
    /// generation's `create_revision`; carries the previous value as `old_value`.
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub fn update(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Option<Revision>> {
        let k = key_str(key)?;
        let Some((prev_id, create_rev, deleted, old)) = self.latest(&k)? else {
            return Ok(None);
        };
        if deleted {
            return Ok(None);
        }
        let rev = self.insert(&k, false, false, create_rev, prev_id, lease, value, &old)?;
        Ok(Some(rev))
    }

    /// Unconditional put: create if absent, else update. Always writes a row.
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub fn put(&mut self, key: &[u8], value: &[u8], lease: i64) -> Result<Revision> {
        let k = key_str(key)?;
        match self.latest(&k)? {
            Some((prev_id, create_rev, false, old)) => {
                self.insert(&k, false, false, create_rev, prev_id, lease, value, &old)
            }
            // Absent or tombstoned: start a new generation, superseding the
            // tombstone row (prev_id) when one exists so the unique chain holds.
            other => {
                let prev_id = other.map_or(0, |(id, _, _, _)| id);
                self.insert(&k, true, false, 0, prev_id, lease, value, &[])
            }
        }
    }

    /// Delete `key` by appending a tombstone, if it has a live row. Returns the
    /// tombstone revision, or `None` if the key was already absent.
    ///
    /// # Errors
    /// [`KineError::EmptyKey`] / [`KineError::Backend`].
    pub fn delete(&mut self, key: &[u8]) -> Result<Option<Revision>> {
        let k = key_str(key)?;
        let Some((prev_id, create_rev, deleted, old)) = self.latest(&k)? else {
            return Ok(None);
        };
        if deleted {
            return Ok(None);
        }
        let rev = self.insert(&k, false, true, create_rev, prev_id, 0, &[], &old)?;
        Ok(Some(rev))
    }

    /// Every *live* key whose latest row is attached to lease `lease_id`, sorted
    /// ascending. Used by the lease layer to find what a lease owns at expiry or
    /// revoke. Lease `0` ("no lease") owns nothing and returns empty. The
    /// `compact_rev_key` sentinel (lease 0) is never included.
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub fn keys_with_lease(&self, lease_id: i64) -> Result<Vec<Vec<u8>>> {
        if lease_id == 0 {
            return Ok(Vec::new());
        }
        // The latest row per key (the current state); keep only the live ones
        // bound to this lease. Mirrors kine's "delete the lease's keys" scan.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT kv.name FROM kine AS kv \
                 JOIN (SELECT name, MAX(id) AS mid FROM kine GROUP BY name) AS latest \
                   ON kv.name = latest.name AND kv.id = latest.mid \
                 WHERE kv.deleted = 0 AND kv.lease = ? AND kv.name <> ? \
                 ORDER BY kv.name ASC",
            )
            .map_err(backend)?;
        let rows: rusqlite::Result<Vec<Vec<u8>>> = stmt
            .query_map(rusqlite::params![lease_id, COMPACT_REV_KEY], |r| {
                Ok(r.get::<_, String>(0)?.into_bytes())
            })
            .map_err(backend)?
            .collect();
        rows.map_err(backend)
    }

    /// Revoke lease `lease_id`: tombstone every live key attached to it, exactly
    /// as etcd deletes a lease's keys when it expires or is revoked. Each
    /// deletion appends a proper tombstone row (its own revision), so the change
    /// shows up in watches. Returns how many keys were deleted.
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub fn revoke_lease_keys(&mut self, lease_id: i64) -> Result<usize> {
        let keys = self.keys_with_lease(lease_id)?;
        let mut deleted = 0;
        for key in keys {
            if self.delete(&key)?.is_some() {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// The physical size of the database file in bytes (`page_count *
    /// page_size`), etcd's `Status.dbSize`. For an in-memory store this is the
    /// in-RAM page allocation.
    ///
    /// # Errors
    /// [`KineError::Backend`] on a query failure.
    pub fn db_size(&self) -> Result<i64> {
        let pages: i64 =
            self.conn.query_row("PRAGMA page_count", [], |r| r.get(0)).map_err(backend)?;
        let page_size: i64 =
            self.conn.query_row("PRAGMA page_size", [], |r| r.get(0)).map_err(backend)?;
        Ok(pages.saturating_mul(page_size))
    }

    /// Defragment the datastore (`VACUUM`): rebuild the file so the free pages
    /// left behind by compaction's `DELETE`s are returned to the filesystem.
    /// This is etcd's `Defragment` maintenance op, realised on `SQLite`. Returns
    /// the number of bytes reclaimed (`0` if the file did not shrink).
    ///
    /// # Errors
    /// [`KineError::Backend`] if the rebuild fails.
    pub fn defragment(&self) -> Result<i64> {
        let before = self.db_size()?;
        self.conn.execute_batch("VACUUM").map_err(backend)?;
        let after = self.db_size()?;
        Ok((before - after).max(0))
    }

    /// Execute a [`RangeRequest`] as live SQL and return the current-state (or
    /// historical) view, sorted by key, with etcd's `count`/`more` flags.
    ///
    /// # Errors
    /// Validation errors from the request, revision guards
    /// ([`KineError::FutureRevision`] / [`KineError::Compacted`]), or
    /// [`KineError::Backend`].
    pub fn range(&self, req: &RangeRequest) -> Result<RangeResponse> {
        validate_range(req)?;
        let header = self.current_revision()?;
        let read_rev = resolve_read(req.revision, header)?;
        let floor = self.compacted_revision()?;
        if req.revision != 0 && req.revision < floor {
            return Err(KineError::Compacted { requested: req.revision, compacted: floor });
        }

        let mut rows = self.list_rows(req, read_rev)?;
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
    fn list_rows(&self, req: &RangeRequest, read_rev: Revision) -> Result<Vec<Row>> {
        let at_rev = req.revision != 0;
        let sql = if at_rev { self.dialect.list_revision_sql() } else { self.dialect.list_current_sql() };
        let pattern = like_pattern(req);
        let mut stmt = self.conn.prepare(&sql).map_err(backend)?;

        let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<Row> {
            // projection: crev, compact, theid, name, created, deleted,
            // create_revision, prev_revision, lease, value, old_value
            Ok(Row {
                key: r.get::<_, String>(3)?.into_bytes(),
                create_revision: r.get(6)?,
                mod_revision: r.get(2)?,
                value: r.get(9)?,
                lease: r.get(8)?,
                deleted: r.get::<_, i64>(5)? != 0,
            })
        };

        let collected: rusqlite::Result<Vec<Row>> = if at_rev {
            // binds: pattern, read_rev, include_deleted(0)
            stmt.query_map(rusqlite::params![pattern, read_rev, 0_i64], map_row)
                .map_err(backend)?
                .collect()
        } else {
            stmt.query_map(rusqlite::params![pattern, 0_i64], map_row)
                .map_err(backend)?
                .collect()
        };
        let mut rows = collected.map_err(backend)?;
        // The sentinel and out-of-interval keys are filtered here (LIKE handles
        // Single/Prefix/AllKeys; Explicit intervals are post-filtered).
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
    pub fn watch_after(&self, filter: &RangeRequest, start_revision: Revision) -> Result<Vec<WatchEvent>> {
        if start_revision < 0 {
            return Err(KineError::NegativeRevision { revision: start_revision });
        }
        let floor = self.compacted_revision()?;
        if start_revision != 0 && start_revision < floor {
            return Err(KineError::Compacted { requested: start_revision, compacted: floor });
        }
        validate_range(filter)?;

        let pattern = like_pattern(filter);
        let mut stmt = self.conn.prepare(&self.dialect.after_sql()).map_err(backend)?;
        let rows: rusqlite::Result<Vec<WatchEvent>> = stmt
            .query_map(rusqlite::params![pattern, start_revision], |r| {
                let deleted = r.get::<_, i64>(5)? != 0;
                Ok(WatchEvent {
                    kind: if deleted { EventKind::Delete } else { EventKind::Put },
                    key: r.get::<_, String>(3)?.into_bytes(),
                    value: r.get(9)?,
                    revision: r.get(2)?,
                    create_revision: r.get(6)?,
                })
            })
            .map_err(backend)?
            .collect();
        let mut events = rows.map_err(backend)?;
        events.retain(|e| e.key != COMPACT_REV_KEY.as_bytes() && contains(filter, &e.key));
        Ok(events)
    }

    /// Compact history up to and including `target`: physically delete every
    /// superseded / tombstoned row at or below `target` that is not the surviving
    /// live row of its key, then advance the recorded floor.
    ///
    /// # Errors
    /// [`KineError::NegativeRevision`] / [`KineError::CompactFutureRevision`] /
    /// [`KineError::CompactionNotForward`] / [`KineError::Backend`].
    pub fn compact(&mut self, target: Revision) -> Result<crate::compact::CompactReport> {
        if target < 0 {
            return Err(KineError::NegativeRevision { revision: target });
        }
        let current = self.current_revision()?;
        if target > current {
            return Err(KineError::CompactFutureRevision { requested: target, current });
        }
        let floor = self.compacted_revision()?;
        if target <= floor {
            return Err(KineError::CompactionNotForward { requested: target, current: floor });
        }

        let before: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM kine WHERE name <> ?", [COMPACT_REV_KEY], |r| r.get(0))
            .map_err(backend)?;

        // Delete rows at/below target that are NOT the surviving live row of
        // their key: superseded values, and tombstones whose key has no later
        // generation. The protected set is the *latest* row per key, but only
        // when that latest row is live — a key whose newest row is a tombstone
        // is dead and protects nothing (both its rows are compactable). This
        // mirrors the decision core's `latest_row_revision_per_key` + is_live
        // survivor rule.
        let tx = self.conn.unchecked_transaction().map_err(backend)?;
        tx.execute(
            "DELETE FROM kine WHERE name <> ?1 AND id <= ?2 AND id NOT IN ( \
                 SELECT kv.id FROM kine AS kv \
                 JOIN (SELECT name, MAX(id) AS mid FROM kine GROUP BY name) AS latest \
                   ON kv.name = latest.name AND kv.id = latest.mid \
                 WHERE kv.deleted = 0 \
             )",
            rusqlite::params![COMPACT_REV_KEY, target],
        )
        .map_err(backend)?;
        tx.execute(&self.dialect.update_compact_sql(), [target]).map_err(backend)?;
        tx.commit().map_err(backend)?;

        let after: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM kine WHERE name <> ?", [COMPACT_REV_KEY], |r| r.get(0))
            .map_err(backend)?;
        let removed = usize::try_from(before - after).unwrap_or(0);
        Ok(crate::compact::CompactReport {
            compacted: target,
            removed,
            remaining: usize::try_from(after).unwrap_or(0),
        })
    }
}

/// Map a key's bytes to the UTF-8 string the `name` column stores.
fn key_str(key: &[u8]) -> Result<String> {
    if key.is_empty() {
        return Err(KineError::EmptyKey);
    }
    String::from_utf8(key.to_vec())
        .map_err(|_| KineError::Backend { message: "key is not valid UTF-8".into() })
}

/// The `LIKE` pattern for a request's interval: exact for a point get, `p%` for
/// a prefix, `%` for the whole keyspace, and the broadest safe prefix for an
/// explicit interval (which is then post-filtered in [`contains`]).
fn like_pattern(req: &RangeRequest) -> String {
    match &req.end {
        RangeEnd::Single => String::from_utf8_lossy(&req.key).into_owned(),
        RangeEnd::Prefix => format!("{}%", String::from_utf8_lossy(&req.key)),
        RangeEnd::AllKeys => "%".to_string(),
        RangeEnd::Explicit(end) => {
            let common = common_prefix(&req.key, end);
            format!("{}%", String::from_utf8_lossy(common))
        }
    }
}

/// The shared leading bytes of two keys.
fn common_prefix<'a>(a: &'a [u8], b: &[u8]) -> &'a [u8] {
    let n = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    &a[..n]
}

/// Does `candidate` fall in `req`'s interval? Mirrors `RangeRequest::contains`.
fn contains(req: &RangeRequest, candidate: &[u8]) -> bool {
    match &req.end {
        RangeEnd::Single => candidate == req.key.as_slice(),
        RangeEnd::Prefix => candidate.starts_with(&req.key),
        RangeEnd::AllKeys => true,
        RangeEnd::Explicit(end) => candidate >= req.key.as_slice() && candidate < end.as_slice(),
    }
}

/// Validate a range request the same way [`crate::range::execute`] does.
fn validate_range(req: &RangeRequest) -> Result<()> {
    if req.limit < 0 {
        return Err(KineError::NegativeLimit { limit: req.limit });
    }
    match &req.end {
        RangeEnd::AllKeys => Ok(()),
        RangeEnd::Single | RangeEnd::Prefix => {
            if req.key.is_empty() {
                Err(KineError::EmptyKey)
            } else {
                Ok(())
            }
        }
        RangeEnd::Explicit(end) => {
            if req.key.is_empty() {
                Err(KineError::EmptyKey)
            } else if end.as_slice() <= req.key.as_slice() {
                Err(KineError::InvalidRange)
            } else {
                Ok(())
            }
        }
    }
}

/// Resolve a request revision against the header (mirrors `Clock::resolve_read`).
const fn resolve_read(requested: Revision, header: Revision) -> Result<Revision> {
    if requested < 0 {
        return Err(KineError::NegativeRevision { revision: requested });
    }
    if requested == 0 {
        return Ok(header);
    }
    if requested > header {
        return Err(KineError::FutureRevision { requested, current: header });
    }
    Ok(requested)
}

/// Wrap a rusqlite error as a backend error. Takes the error by value so it can
/// be used directly as a `Result::map_err` argument.
#[allow(clippy::needless_pass_by_value)]
fn backend(e: rusqlite::Error) -> KineError {
    KineError::Backend { message: e.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn store() -> SqliteStore {
        SqliteStore::open_in_memory().unwrap()
    }

    /// A unique temp-file path for an on-disk test store, removed if it lingers.
    fn tmp_path(tag: &str) -> std::path::PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("kine-{tag}-{}-{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn keys(resp: &RangeResponse) -> Vec<Vec<u8>> {
        resp.kvs.iter().map(|r| r.key.clone()).collect()
    }

    #[test]
    fn create_then_range_returns_value_at_revision_one() {
        let mut s = store();
        assert_eq!(s.create(b"/reg/a", b"1", 0).unwrap(), Some(1));
        let resp = s.range(&RangeRequest::key(b"/reg/a")).unwrap();
        assert_eq!(resp.kvs.len(), 1);
        assert_eq!(resp.kvs[0].value, b"1");
        assert_eq!(resp.kvs[0].create_revision, 1);
        assert_eq!(resp.kvs[0].mod_revision, 1);
    }

    #[test]
    fn create_is_rejected_when_live_row_exists() {
        let mut s = store();
        s.create(b"a", b"1", 0).unwrap();
        assert_eq!(s.create(b"a", b"2", 0).unwrap(), None);
        assert_eq!(s.range(&RangeRequest::key(b"a")).unwrap().kvs[0].value, b"1");
        assert_eq!(s.current_revision().unwrap(), 1);
    }

    #[test]
    fn update_keeps_create_revision_and_advances_mod_revision() {
        let mut s = store();
        s.create(b"a", b"1", 0).unwrap();
        assert_eq!(s.update(b"a", b"2", 0).unwrap(), Some(2));
        let row = &s.range(&RangeRequest::key(b"a")).unwrap().kvs[0];
        assert_eq!(row.create_revision, 1);
        assert_eq!(row.mod_revision, 2);
        assert_eq!(row.value, b"2");
    }

    #[test]
    fn update_absent_key_is_a_noop() {
        let mut s = store();
        assert_eq!(s.update(b"ghost", b"x", 0).unwrap(), None);
        assert_eq!(s.current_revision().unwrap(), 0);
    }

    #[test]
    fn put_creates_then_updates() {
        let mut s = store();
        assert_eq!(s.put(b"a", b"1", 0).unwrap(), 1);
        assert_eq!(s.put(b"a", b"2", 0).unwrap(), 2);
        let row = &s.range(&RangeRequest::key(b"a")).unwrap().kvs[0];
        assert_eq!(row.create_revision, 1);
        assert_eq!(row.mod_revision, 2);
        assert_eq!(row.value, b"2");
    }

    #[test]
    fn delete_tombstones_and_hides_the_key() {
        let mut s = store();
        s.create(b"a", b"1", 0).unwrap();
        assert_eq!(s.delete(b"a").unwrap(), Some(2));
        assert!(s.range(&RangeRequest::key(b"a")).unwrap().kvs.is_empty());
    }

    #[test]
    fn delete_absent_key_is_a_noop() {
        let mut s = store();
        assert_eq!(s.delete(b"ghost").unwrap(), None);
        assert_eq!(s.current_revision().unwrap(), 0);
    }

    #[test]
    fn recreate_after_delete_starts_a_new_generation() {
        let mut s = store();
        s.create(b"a", b"1", 0).unwrap(); // 1
        s.delete(b"a").unwrap(); //          2
        assert_eq!(s.create(b"a", b"3", 0).unwrap(), Some(3));
        let row = &s.range(&RangeRequest::key(b"a")).unwrap().kvs[0];
        assert_eq!(row.create_revision, 3, "fresh generation");
        assert_eq!(row.mod_revision, 3);
        assert_eq!(row.value, b"3");
    }

    #[test]
    fn revision_is_monotonic_across_mixed_ops() {
        let mut s = store();
        s.create(b"a", b"1", 0).unwrap();
        s.put(b"b", b"2", 0).unwrap();
        s.update(b"a", b"1b", 0).unwrap();
        s.delete(b"b").unwrap();
        assert_eq!(s.current_revision().unwrap(), 4);
    }

    #[test]
    fn empty_key_is_rejected_on_every_mutation() {
        let mut s = store();
        assert_eq!(s.create(b"", b"1", 0), Err(KineError::EmptyKey));
        assert_eq!(s.update(b"", b"1", 0), Err(KineError::EmptyKey));
        assert_eq!(s.put(b"", b"1", 0), Err(KineError::EmptyKey));
        assert_eq!(s.delete(b""), Err(KineError::EmptyKey));
    }

    #[test]
    fn lease_is_stored_on_the_row() {
        let mut s = store();
        s.create(b"a", b"1", 77).unwrap();
        assert_eq!(s.range(&RangeRequest::key(b"a")).unwrap().kvs[0].lease, 77);
    }

    fn seeded() -> SqliteStore {
        let mut s = store();
        s.create(b"/reg/a", b"1", 0).unwrap();
        s.create(b"/reg/b", b"2", 0).unwrap();
        s.create(b"/reg/c", b"3", 0).unwrap();
        s.create(b"/other/x", b"9", 0).unwrap();
        s
    }

    #[test]
    fn point_get_missing_key_is_empty() {
        let s = seeded();
        let resp = s.range(&RangeRequest::key(b"/reg/zzz")).unwrap();
        assert!(resp.kvs.is_empty());
        assert_eq!(resp.count, 0);
    }

    #[test]
    fn prefix_scan_selects_only_the_subtree_sorted() {
        let s = seeded();
        let resp = s.range(&RangeRequest::prefix(b"/reg/")).unwrap();
        assert_eq!(
            keys(&resp),
            vec![b"/reg/a".to_vec(), b"/reg/b".to_vec(), b"/reg/c".to_vec()]
        );
        assert_eq!(resp.count, 3);
    }

    #[test]
    fn prefix_scan_excludes_sibling_subtrees() {
        let s = seeded();
        let resp = s.range(&RangeRequest::prefix(b"/reg/")).unwrap();
        assert!(!resp.kvs.iter().any(|r| r.key == b"/other/x"));
    }

    #[test]
    fn explicit_interval_is_half_open() {
        let s = seeded();
        let resp = s.range(&RangeRequest::interval(b"/reg/a", b"/reg/c")).unwrap();
        assert_eq!(keys(&resp), vec![b"/reg/a".to_vec(), b"/reg/b".to_vec()]);
    }

    #[test]
    fn all_keys_returns_everything_live() {
        let s = seeded();
        assert_eq!(s.range(&RangeRequest::all()).unwrap().count, 4);
    }

    #[test]
    fn deleted_keys_are_absent_from_current_view() {
        let mut s = seeded();
        s.delete(b"/reg/b").unwrap();
        let resp = s.range(&RangeRequest::prefix(b"/reg/")).unwrap();
        assert_eq!(keys(&resp), vec![b"/reg/a".to_vec(), b"/reg/c".to_vec()]);
    }

    #[test]
    fn historical_read_sees_old_value() {
        let mut s = store();
        s.create(b"k", b"v1", 0).unwrap(); // 1
        s.update(b"k", b"v2", 0).unwrap(); // 2
        assert_eq!(s.range(&RangeRequest::key(b"k")).unwrap().kvs[0].value, b"v2");
        let past = s.range(&RangeRequest::key(b"k").at_revision(1)).unwrap();
        assert_eq!(past.kvs[0].value, b"v1");
        assert_eq!(past.revision, 1);
    }

    #[test]
    fn historical_read_before_create_is_empty() {
        let mut s = store();
        s.create(b"first", b"x", 0).unwrap(); // 1
        s.create(b"k", b"v", 0).unwrap(); //      2
        let past = s.range(&RangeRequest::key(b"k").at_revision(1)).unwrap();
        assert!(past.kvs.is_empty());
    }

    #[test]
    fn limit_truncates_and_sets_more_with_full_count() {
        let s = seeded();
        let resp = s.range(&RangeRequest::prefix(b"/reg/").with_limit(2)).unwrap();
        assert_eq!(resp.kvs.len(), 2);
        assert!(resp.more);
        assert_eq!(resp.count, 3);
        assert_eq!(resp.kvs[0].key, b"/reg/a");
        assert_eq!(resp.kvs[1].key, b"/reg/b");
    }

    #[test]
    fn future_revision_read_is_rejected() {
        let mut s = store();
        s.create(b"k", b"v", 0).unwrap();
        assert_eq!(
            s.range(&RangeRequest::key(b"k").at_revision(99)),
            Err(KineError::FutureRevision { requested: 99, current: 1 })
        );
    }

    fn history() -> SqliteStore {
        let mut s = store();
        s.create(b"/a", b"1", 0).unwrap(); // 1 PUT
        s.create(b"/b", b"2", 0).unwrap(); // 2 PUT
        s.update(b"/a", b"1b", 0).unwrap(); // 3 PUT
        s.delete(b"/b").unwrap(); //           4 DELETE
        s
    }

    #[test]
    fn watch_from_zero_replays_all_changes_in_order() {
        let s = history();
        let evs = s.watch_after(&RangeRequest::all(), 0).unwrap();
        let revs: Vec<_> = evs.iter().map(|e| e.revision).collect();
        assert_eq!(revs, vec![1, 2, 3, 4]);
    }

    #[test]
    fn watch_classifies_put_and_delete() {
        let s = history();
        let evs = s.watch_after(&RangeRequest::all(), 0).unwrap();
        assert_eq!(evs[0].kind, EventKind::Put);
        assert_eq!(evs[3].kind, EventKind::Delete);
        assert_eq!(evs[3].key, b"/b");
        assert!(evs[3].value.is_empty());
    }

    #[test]
    fn watch_starts_strictly_after_start_revision() {
        let s = history();
        let evs = s.watch_after(&RangeRequest::all(), 2).unwrap();
        let revs: Vec<_> = evs.iter().map(|e| e.revision).collect();
        assert_eq!(revs, vec![3, 4]);
    }

    #[test]
    fn watch_filters_to_a_prefix() {
        let mut s = store();
        s.create(b"/ns/x", b"1", 0).unwrap();
        s.create(b"/other", b"2", 0).unwrap();
        s.create(b"/ns/y", b"3", 0).unwrap();
        let evs = s.watch_after(&RangeRequest::prefix(b"/ns/"), 0).unwrap();
        let ks: Vec<_> = evs.iter().map(|e| e.key.clone()).collect();
        assert_eq!(ks, vec![b"/ns/x".to_vec(), b"/ns/y".to_vec()]);
    }

    #[test]
    fn compaction_drops_superseded_values_but_keeps_current() {
        let mut s = store();
        s.create(b"k", b"v1", 0).unwrap(); // 1
        s.update(b"k", b"v2", 0).unwrap(); // 2
        s.update(b"k", b"v3", 0).unwrap(); // 3
        let report = s.compact(2).unwrap();
        assert_eq!(report.compacted, 2);
        assert!(report.removed >= 1);
        assert_eq!(s.range(&RangeRequest::key(b"k")).unwrap().kvs[0].value, b"v3");
    }

    #[test]
    fn compaction_removes_tombstone_at_or_below_floor() {
        let mut s = store();
        s.create(b"k", b"v1", 0).unwrap(); // 1
        s.delete(b"k").unwrap(); //           2 tombstone
        s.compact(2).unwrap();
        assert!(s.range(&RangeRequest::key(b"k")).unwrap().kvs.is_empty());
        // physically gone
        let n: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM kine WHERE name = 'k'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn historical_read_below_compacted_floor_is_rejected() {
        let mut s = store();
        s.create(b"k", b"v1", 0).unwrap();
        s.update(b"k", b"v2", 0).unwrap();
        s.update(b"k", b"v3", 0).unwrap();
        s.compact(2).unwrap();
        assert_eq!(
            s.range(&RangeRequest::key(b"k").at_revision(1)),
            Err(KineError::Compacted { requested: 1, compacted: 2 })
        );
    }

    #[test]
    fn compaction_must_move_forward() {
        let mut s = store();
        s.create(b"k", b"v", 0).unwrap();
        s.update(b"k", b"v2", 0).unwrap();
        s.compact(2).unwrap();
        assert_eq!(
            s.compact(2),
            Err(KineError::CompactionNotForward { requested: 2, current: 2 })
        );
    }

    #[test]
    fn compaction_rejects_future_revision() {
        let mut s = store();
        s.create(b"k", b"v", 0).unwrap();
        assert_eq!(
            s.compact(50),
            Err(KineError::CompactFutureRevision { requested: 50, current: 1 })
        );
    }

    #[test]
    fn data_survives_close_and_reopen_on_disk() {
        // The anti-stub proof: write through one handle, drop it, reopen the
        // SAME file, and read the data back from real storage.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("kine-persist-{}-{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);

        {
            let mut s = SqliteStore::open(&path).unwrap();
            s.create(b"/reg/persisted", b"durable", 0).unwrap();
            s.update(b"/reg/persisted", b"durable2", 0).unwrap();
            assert_eq!(s.current_revision().unwrap(), 2);
        } // handle dropped, connection closed

        let s = SqliteStore::open(&path).unwrap();
        let resp = s.range(&RangeRequest::key(b"/reg/persisted")).unwrap();
        assert_eq!(resp.kvs[0].value, b"durable2");
        assert_eq!(resp.kvs[0].create_revision, 1);
        assert_eq!(resp.kvs[0].mod_revision, 2);
        assert_eq!(s.current_revision().unwrap(), 2);

        drop(s);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn keys_with_lease_lists_only_matching_live_keys() {
        let mut s = store();
        s.create(b"/a", b"1", 7).unwrap(); // lease 7
        s.create(b"/b", b"2", 7).unwrap(); // lease 7
        s.create(b"/c", b"3", 9).unwrap(); // lease 9
        s.create(b"/d", b"4", 0).unwrap(); // no lease
        let mut got = s.keys_with_lease(7).unwrap();
        got.sort();
        assert_eq!(got, vec![b"/a".to_vec(), b"/b".to_vec()]);
        assert_eq!(s.keys_with_lease(9).unwrap(), vec![b"/c".to_vec()]);
        assert!(s.keys_with_lease(123).unwrap().is_empty());
    }

    #[test]
    fn keys_with_lease_ignores_tombstoned_keys() {
        let mut s = store();
        s.create(b"/a", b"1", 7).unwrap();
        s.delete(b"/a").unwrap(); // tombstone — no longer attached
        assert!(s.keys_with_lease(7).unwrap().is_empty());
    }

    #[test]
    fn revoke_lease_keys_tombstones_all_attached_keys() {
        let mut s = store();
        s.create(b"/a", b"1", 7).unwrap();
        s.create(b"/b", b"2", 7).unwrap();
        s.create(b"/c", b"3", 0).unwrap();
        let deleted = s.revoke_lease_keys(7).unwrap();
        assert_eq!(deleted, 2, "both lease-7 keys revoked");
        assert!(s.range(&RangeRequest::key(b"/a")).unwrap().kvs.is_empty());
        assert!(s.range(&RangeRequest::key(b"/b")).unwrap().kvs.is_empty());
        // the unleased key is untouched
        assert_eq!(s.range(&RangeRequest::key(b"/c")).unwrap().kvs[0].value, b"3");
    }

    #[test]
    fn revoke_lease_keys_for_no_lease_is_a_noop() {
        let mut s = store();
        s.create(b"/a", b"1", 0).unwrap();
        assert_eq!(s.revoke_lease_keys(0).unwrap(), 0);
        assert_eq!(s.range(&RangeRequest::key(b"/a")).unwrap().kvs.len(), 1);
    }

    #[test]
    fn db_size_is_positive_and_grows_with_writes() {
        let mut s = SqliteStore::open(&tmp_path("dbsize")).unwrap();
        let empty = s.db_size().unwrap();
        assert!(empty > 0, "an initialised db occupies pages");
        for i in 0..200 {
            s.put(format!("/reg/{i}").as_bytes(), &vec![b'x'; 256], 0).unwrap();
        }
        assert!(s.db_size().unwrap() > empty, "writes grow the file");
    }

    #[test]
    fn defragment_reclaims_space_after_compaction_and_keeps_data() {
        let mut s = SqliteStore::open(&tmp_path("defrag")).unwrap();
        // Churn one key through many revisions so most rows become superseded.
        for i in 0..500 {
            s.put(b"/hot", format!("v{i}").as_bytes(), 0).unwrap();
        }
        s.put(b"/keep", b"final", 0).unwrap();
        let before = s.db_size().unwrap();
        let current = s.current_revision().unwrap();
        s.compact(current - 1).unwrap(); // drop all superseded history
        let reclaimed = s.defragment().unwrap();
        let after = s.db_size().unwrap();
        assert!(after <= before, "VACUUM never grows the file");
        assert!(reclaimed >= 0);
        // the surviving live rows are intact after the rebuild
        assert_eq!(s.range(&RangeRequest::key(b"/hot")).unwrap().kvs[0].value, b"v499");
        assert_eq!(s.range(&RangeRequest::key(b"/keep")).unwrap().kvs[0].value, b"final");
    }
}
