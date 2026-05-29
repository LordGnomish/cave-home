// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The SQL row schema and query templates kine *would* issue — modelled, not
//! executed.
//!
//! kine's whole trick is that the etcd MVCC semantics implemented purely in the
//! other modules of this crate map onto a single SQL table and a small fixed
//! set of statements. This module records that mapping as typed Rust: the
//! column schema ([`KINE_TABLE`], [`Column`]) and the canonical statements
//! ([`Statement`]) as parameterised template strings.
//!
//! **Nothing here runs.** There is no SQL driver, no connection, no execution.
//! The real driver (`SQLite` / `Postgres` / `MySQL` / dqlite), the transactions, the
//! row-locking and the prepared-statement plumbing are all **deferred to
//! Phase-1b** (see the parity manifest). What this module *guarantees* is that
//! the in-memory logic in [`crate::store`] / [`crate::range`] /
//! [`crate::compact`] has a faithful, documented SQL shape — the contract a
//! future driver must satisfy.
//!
//! Reference: kine generic backend DDL + statements (`k3s-io/kine`,
//! `pkg/drivers/generic/generic.go`). The column set (`id`, `name`,
//! `created`, `deleted`, `create_revision`, `prev_revision`, `lease`,
//! `value`, `old_value`) is reproduced faithfully; the statement text below is
//! a representative, behaviourally-equivalent template, not a verbatim copy.

/// A column in kine's backing table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    /// Column name as it appears in the DDL.
    pub name: &'static str,
    /// SQL type (ANSI-ish; a driver narrows it, e.g. `BLOB` vs `BYTEA`).
    pub sql_type: &'static str,
    /// Human note on the column's role in the MVCC model.
    pub role: &'static str,
}

/// The kine table name.
pub const KINE_TABLE: &str = "kine";

/// kine's column schema. The auto-increment `id` is the global revision; the
/// `(name, id)` pairing reproduces etcd's per-key revision history.
pub const SCHEMA: &[Column] = &[
    Column {
        name: "id",
        sql_type: "INTEGER PRIMARY KEY AUTOINCREMENT",
        role: "global revision — monotonic, one per write (etcd main revision)",
    },
    Column { name: "name", sql_type: "TEXT", role: "the etcd key" },
    Column {
        name: "created",
        sql_type: "INTEGER",
        role: "1 if this row created the key's current generation",
    },
    Column {
        name: "deleted",
        sql_type: "INTEGER",
        role: "1 if this row is a tombstone (DELETE event)",
    },
    Column {
        name: "create_revision",
        sql_type: "INTEGER",
        role: "revision the current generation of the key was created at",
    },
    Column {
        name: "prev_revision",
        sql_type: "INTEGER",
        role: "the row id this row supersedes (for prev_kv / watch)",
    },
    Column { name: "lease", sql_type: "INTEGER", role: "attached lease id, 0 = none" },
    Column { name: "value", sql_type: "BLOB", role: "the stored bytes" },
    Column {
        name: "old_value",
        sql_type: "BLOB",
        role: "previous value, carried for etcd prev_kv on update/delete",
    },
];

/// The DDL that creates the kine table, assembled from [`SCHEMA`]. Returned as
/// a `String` so a driver could feed it to a migration — but this crate never
/// executes it.
#[must_use]
pub fn create_table_ddl() -> String {
    let cols = SCHEMA
        .iter()
        .map(|c| format!("  {} {}", c.name, c.sql_type))
        .collect::<Vec<_>>()
        .join(",\n");
    format!("CREATE TABLE IF NOT EXISTS {KINE_TABLE} (\n{cols}\n);")
}

/// The canonical statements kine issues, as parameterised templates.
///
/// Each variant maps onto exactly one pure operation in this crate. The `?`
/// placeholders are positional bind parameters; a Postgres driver would rewrite
/// them to `$1..`. The text is representative of kine's generic backend, not a
/// verbatim copy (Charter §6 honest port-method: behavioural, not line-by-line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Statement {
    /// Insert one new row (the append-only write). Maps to
    /// [`crate::store::Store::put`] / `create` / `update`.
    Insert,
    /// Insert a tombstone row. Maps to [`crate::store::Store::delete`].
    InsertTombstone,
    /// Current-state point/range read: latest non-deleted row per key in
    /// `[name >= ? AND name < ?]`. Maps to [`crate::range::execute`].
    RangeCurrent,
    /// Historical read: latest row per key with `id <= ?` (the read revision).
    /// Maps to a [`crate::range::RangeRequest::at_revision`] read.
    RangeAtRevision,
    /// The store's current head revision (`MAX(id)`).
    CurrentRevision,
    /// Compaction delete: superseded / tombstoned rows at or below the compact
    /// revision. Maps to [`crate::compact::compact`].
    Compact,
    /// Watch poll: rows with `id > ?` in the watched range, ordered by `id`.
    /// Maps to [`crate::watch::watch`].
    WatchAfter,
    /// Delete all rows for keys holding a given lease (lease expiry). Maps to
    /// [`crate::lease::revoke_keys`].
    DeleteByLease,
}

impl Statement {
    /// The parameterised SQL template for this statement. **Never executed** —
    /// this is the documented contract for a future driver.
    #[must_use]
    pub const fn template(self) -> &'static str {
        match self {
            Self::Insert => {
                "INSERT INTO kine (name, created, deleted, create_revision, prev_revision, lease, value, old_value) \
                 VALUES (?, ?, 0, ?, ?, ?, ?, ?)"
            }
            Self::InsertTombstone => {
                "INSERT INTO kine (name, created, deleted, create_revision, prev_revision, lease, value, old_value) \
                 VALUES (?, 0, 1, ?, ?, 0, '', ?)"
            }
            Self::RangeCurrent => {
                "SELECT kv.id, kv.name, kv.create_revision, kv.lease, kv.value \
                 FROM kine kv \
                 INNER JOIN (SELECT name, MAX(id) AS mid FROM kine \
                             WHERE name >= ? AND name < ? GROUP BY name) latest \
                 ON kv.name = latest.name AND kv.id = latest.mid \
                 WHERE kv.deleted = 0 ORDER BY kv.name LIMIT ?"
            }
            Self::RangeAtRevision => {
                "SELECT kv.id, kv.name, kv.create_revision, kv.lease, kv.value, kv.deleted \
                 FROM kine kv \
                 INNER JOIN (SELECT name, MAX(id) AS mid FROM kine \
                             WHERE name >= ? AND name < ? AND id <= ? GROUP BY name) latest \
                 ON kv.name = latest.name AND kv.id = latest.mid \
                 WHERE kv.deleted = 0 ORDER BY kv.name LIMIT ?"
            }
            Self::CurrentRevision => "SELECT MAX(id) FROM kine",
            Self::Compact => {
                "DELETE FROM kine WHERE id <= ? AND id NOT IN \
                 (SELECT MAX(id) FROM kine GROUP BY name) \
                 OR (deleted = 1 AND id <= ?)"
            }
            Self::WatchAfter => {
                "SELECT id, name, created, deleted, create_revision, lease, value \
                 FROM kine WHERE id > ? AND name >= ? AND name < ? ORDER BY id ASC"
            }
            Self::DeleteByLease => "SELECT name FROM kine WHERE lease = ? AND deleted = 0",
        }
    }
}

/// Every statement kine relies on. Useful for a driver to prepare them all up
/// front, and for tests to assert the set is complete and non-empty.
pub const ALL_STATEMENTS: &[Statement] = &[
    Statement::Insert,
    Statement::InsertTombstone,
    Statement::RangeCurrent,
    Statement::RangeAtRevision,
    Statement::CurrentRevision,
    Statement::Compact,
    Statement::WatchAfter,
    Statement::DeleteByLease,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_the_kine_mvcc_columns() {
        let names: Vec<_> = SCHEMA.iter().map(|c| c.name).collect();
        for required in ["id", "name", "deleted", "create_revision", "lease", "value"] {
            assert!(names.contains(&required), "missing column {required}");
        }
    }

    #[test]
    fn ddl_mentions_table_and_primary_key() {
        let ddl = create_table_ddl();
        assert!(ddl.contains("CREATE TABLE"));
        assert!(ddl.contains("kine"));
        assert!(ddl.contains("PRIMARY KEY"));
    }

    #[test]
    fn every_statement_has_a_non_empty_template() {
        for s in ALL_STATEMENTS {
            assert!(!s.template().is_empty(), "{s:?} has no template");
        }
    }

    #[test]
    fn range_templates_select_latest_per_key() {
        // The MVCC "max id per name" idiom must appear in both range plans.
        assert!(Statement::RangeCurrent.template().contains("MAX(id)"));
        assert!(Statement::RangeAtRevision.template().contains("MAX(id)"));
        // Historical read is bounded by the read revision.
        assert!(Statement::RangeAtRevision.template().contains("id <= ?"));
    }

    #[test]
    fn statement_set_is_complete_and_unique() {
        // No duplicate variants in the catalogue.
        for (i, a) in ALL_STATEMENTS.iter().enumerate() {
            for b in &ALL_STATEMENTS[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert_eq!(ALL_STATEMENTS.len(), 8);
    }
}
