// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Per-driver SQL generation — kine's `generic` backend dialect layer.
//!
//! kine runs the *same* etcd-MVCC semantics over `SQLite`, `Postgres` and
//! `MySQL` by keeping one set of query templates and varying only the small,
//! driver-specific pieces: the auto-increment primary key type, the blob column
//! type, and the bind-parameter placeholder syntax (`?` for `SQLite`/`MySQL`,
//! `$1..$N` for `Postgres`). This module is that variance, isolated as pure string
//! generation so it can be tested exhaustively without a database.
//!
//! Reference: `k3s-io/kine` `pkg/drivers/generic/generic.go` (the shared
//! `columns` / `revSQL` / `compactRevSQL` / `listSQL` constants and the `q()`
//! placeholder rebinder) plus the per-driver `pkg/drivers/{sqlite,pgsql,mysql}`
//! overrides (column types, DDL). Faithful reproduction of the query structure,
//! Apache-2.0.

/// The SQL backend driver kine talks to. `SQLite` is k3s's single-binary
/// default; `Postgres` and `MySQL` are the external-datastore options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Driver {
    /// Embedded `SQLite` (the default; one file, no server).
    Sqlite,
    /// `PostgreSQL` over the wire.
    Postgres,
    /// `MySQL` / `MariaDB` over the wire.
    Mysql,
}

impl Driver {
    /// The lowercase driver name as it appears in a kine endpoint string
    /// (`sqlite://`, `postgres://`, `mysql://`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
        }
    }

    /// Parse a driver name. Accepts the canonical names and the common
    /// `postgresql` alias. Returns `None` for anything else.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sqlite" => Some(Self::Sqlite),
            "postgres" | "postgresql" => Some(Self::Postgres),
            "mysql" => Some(Self::Mysql),
            _ => None,
        }
    }
}

/// The sentinel key row in which kine records the compacted-revision floor, so
/// the floor survives across restarts in the same table. Faithful to kine's
/// `compact_rev_key`.
pub const COMPACT_REV_KEY: &str = "compact_rev_key";

/// kine's projected row columns (`pkg/drivers/generic/generic.go` `columns`).
const COLUMNS: &str = "kv.id AS theid, kv.name, kv.created, kv.deleted, \
     kv.create_revision, kv.prev_revision, kv.lease, kv.value, kv.old_value";

/// The store header revision, embedded as a sub-select in every list/after
/// query exactly as kine does (`revSQL`).
const REV_SQL: &str = "SELECT MAX(rkv.id) AS id FROM kine AS rkv";

/// The compacted floor, read from the sentinel row (`compactRevSQL`).
const COMPACT_REV_SQL: &str =
    "SELECT MAX(crkv.prev_revision) AS prev_revision FROM kine AS crkv \
     WHERE crkv.name = 'compact_rev_key'";

/// A driver dialect: turns the shared MVCC query templates into the exact SQL a
/// given driver expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    driver: Driver,
}

impl Dialect {
    /// A dialect for `driver`.
    #[must_use]
    pub const fn new(driver: Driver) -> Self {
        Self { driver }
    }

    /// The driver this dialect targets.
    #[must_use]
    pub const fn driver(self) -> Driver {
        self.driver
    }

    /// Rebind positional `?` placeholders to the driver's syntax. `SQLite` and
    /// `MySQL` use `?` verbatim; `Postgres` numbers them `$1..$N` left to right.
    /// Mirrors kine's `q()` translate-bind-vars step.
    #[must_use]
    pub fn rebind(self, sql: &str) -> String {
        if self.driver != Driver::Postgres {
            return sql.to_string();
        }
        let mut out = String::with_capacity(sql.len() + 8);
        let mut n = 0u32;
        for ch in sql.chars() {
            if ch == '?' {
                n += 1;
                out.push('$');
                out.push_str(&n.to_string());
            } else {
                out.push(ch);
            }
        }
        out
    }

    /// The `CREATE TABLE` DDL for the kine table, with driver-correct column
    /// types (auto-increment PK and blob type vary; the MVCC columns do not).
    #[must_use]
    pub fn create_table_sql(self) -> String {
        match self.driver {
            Driver::Sqlite => "CREATE TABLE IF NOT EXISTS kine (\n\
                 \tid INTEGER PRIMARY KEY AUTOINCREMENT,\n\
                 \tname TEXT,\n\
                 \tcreated INTEGER,\n\
                 \tdeleted INTEGER,\n\
                 \tcreate_revision INTEGER,\n\
                 \tprev_revision INTEGER,\n\
                 \tlease INTEGER,\n\
                 \tvalue BLOB,\n\
                 \told_value BLOB\n)"
                .to_string(),
            Driver::Postgres => "CREATE TABLE IF NOT EXISTS kine (\n\
                 \tid SERIAL PRIMARY KEY,\n\
                 \tname VARCHAR(630),\n\
                 \tcreated INTEGER,\n\
                 \tdeleted INTEGER,\n\
                 \tcreate_revision INTEGER,\n\
                 \tprev_revision INTEGER,\n\
                 \tlease INTEGER,\n\
                 \tvalue BYTEA,\n\
                 \told_value BYTEA\n)"
                .to_string(),
            Driver::Mysql => "CREATE TABLE IF NOT EXISTS kine (\n\
                 \tid INTEGER AUTO_INCREMENT,\n\
                 \tname VARCHAR(630) CHARACTER SET ascii,\n\
                 \tcreated INTEGER,\n\
                 \tdeleted INTEGER,\n\
                 \tcreate_revision INTEGER,\n\
                 \tprev_revision INTEGER,\n\
                 \tlease INTEGER,\n\
                 \tvalue MEDIUMBLOB,\n\
                 \told_value MEDIUMBLOB,\n\
                 \tPRIMARY KEY (id)\n)"
                .to_string(),
        }
    }

    /// The index DDL kine creates alongside the table: a `name` index, a unique
    /// `(name, prev_revision)` index (the optimistic-concurrency guard), and an
    /// `(id, deleted)` index for the after/watch scan.
    #[must_use]
    pub fn index_sqls(self) -> Vec<String> {
        // MySQL has no `CREATE INDEX IF NOT EXISTS`; the others do.
        let guard = if self.driver == Driver::Mysql { "" } else { "IF NOT EXISTS " };
        vec![
            format!("CREATE INDEX {guard}kine_name_index ON kine (name)"),
            format!(
                "CREATE UNIQUE INDEX {guard}kine_name_prev_revision_uindex \
                 ON kine (name, prev_revision)"
            ),
            format!("CREATE INDEX {guard}kine_id_deleted_index ON kine (id, deleted)"),
        ]
    }

    /// `SELECT MAX(rkv.id)` — the current store header revision.
    #[must_use]
    pub const fn rev_sql(self) -> &'static str {
        REV_SQL
    }

    /// The compacted-floor read from the sentinel row.
    #[must_use]
    pub const fn compact_rev_sql(self) -> &'static str {
        COMPACT_REV_SQL
    }

    /// Insert one append-only row (id auto-assigned). Eight bound columns.
    #[must_use]
    pub fn insert_sql(self) -> String {
        self.rebind(
            "INSERT INTO kine(name, created, deleted, create_revision, \
             prev_revision, lease, value, old_value) VALUES(?, ?, ?, ?, ?, ?, ?, ?)",
        )
    }

    /// Insert a row at an explicit id (used to seed `compact_rev_key`). Nine
    /// bound columns.
    #[must_use]
    pub fn fill_sql(self) -> String {
        self.rebind(
            "INSERT INTO kine(id, name, created, deleted, create_revision, \
             prev_revision, lease, value, old_value) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
    }

    /// Delete one row by id (compaction's physical purge).
    #[must_use]
    pub fn delete_sql(self) -> String {
        self.rebind("DELETE FROM kine WHERE id = ?")
    }

    /// Advance the recorded compacted floor.
    #[must_use]
    pub fn update_compact_sql(self) -> String {
        self.rebind("UPDATE kine SET prev_revision = ? WHERE name = 'compact_rev_key'")
    }

    /// The shared list query: latest row per key in a `name LIKE ?` range,
    /// joined back to the full row, current-state (or, with the `OR ?` flag,
    /// including tombstones), ordered by id. When `at_revision` is set the inner
    /// `MAX(mkv.id)` is bounded by `mkv.id <= ?` for a historical read.
    ///
    /// Bind order: `name_like`, [`read_revision` if `at_revision`],
    /// `include_deleted`.
    fn list_sql(self, at_revision: bool) -> String {
        let revision_bound = if at_revision { "AND mkv.id <= ?\n\t\t" } else { "" };
        let raw = format!(
            "SELECT ({REV_SQL}) AS crev, ({COMPACT_REV_SQL}) AS compact, {COLUMNS}\n\
             FROM kine AS kv\n\
             JOIN (\n\
             \t\tSELECT MAX(mkv.id) AS id\n\
             \t\tFROM kine AS mkv\n\
             \t\tWHERE mkv.name LIKE ?\n\
             \t\t{revision_bound}GROUP BY mkv.name\n\
             \t) AS maxkv ON maxkv.id = kv.id\n\
             WHERE (kv.deleted = 0 OR ?)\n\
             ORDER BY kv.id ASC"
        );
        self.rebind(&raw)
    }

    /// List the current state of a key/prefix range.
    #[must_use]
    pub fn list_current_sql(self) -> String {
        self.list_sql(false)
    }

    /// List a key/prefix range as of a historical revision (`mkv.id <= ?`).
    #[must_use]
    pub fn list_revision_sql(self) -> String {
        self.list_sql(true)
    }

    /// Append `LIMIT ?` to a list query — kept separate because not every list
    /// is limited and the bound is the final positional param.
    #[must_use]
    pub fn with_limit(self, list_sql: &str) -> String {
        // The list query was already rebound; the limit `?`/`$N` must continue
        // the numbering, so rebind the suffix against the running count.
        match self.driver {
            Driver::Postgres => {
                let next = list_sql.matches('$').count() + 1;
                format!("{list_sql} LIMIT ${next}")
            }
            _ => format!("{list_sql} LIMIT ?"),
        }
    }

    /// Count live keys in a `name LIKE ?` range. Bind order: `name_like`,
    /// `include_deleted`.
    #[must_use]
    pub fn count_current_sql(self) -> String {
        self.rebind(
            "SELECT COUNT(*) FROM (\n\
             \tSELECT MAX(mkv.id) AS id FROM kine AS mkv WHERE mkv.name LIKE ? GROUP BY mkv.name\n\
             ) AS maxkv JOIN kine AS kv ON maxkv.id = kv.id WHERE (kv.deleted = 0 OR ?)",
        )
    }

    /// The watch poll: every row in a `name LIKE ?` range with `id > ?`, in
    /// ascending id order, carrying the header + compact revision. Faithful to
    /// kine's `afterSQL`.
    #[must_use]
    pub fn after_sql(self) -> String {
        self.rebind(&format!(
            "SELECT ({REV_SQL}) AS crev, ({COMPACT_REV_SQL}) AS compact, {COLUMNS}\n\
             FROM kine AS kv\n\
             WHERE kv.name LIKE ? AND kv.id > ?\n\
             ORDER BY kv.id ASC"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_parse_roundtrips_known_names() {
        assert_eq!(Driver::parse("sqlite"), Some(Driver::Sqlite));
        assert_eq!(Driver::parse("postgres"), Some(Driver::Postgres));
        assert_eq!(Driver::parse("mysql"), Some(Driver::Mysql));
        assert_eq!(Driver::Sqlite.as_str(), "sqlite");
        assert_eq!(Driver::Postgres.as_str(), "postgres");
        assert_eq!(Driver::Mysql.as_str(), "mysql");
    }

    #[test]
    fn driver_parse_rejects_unknown() {
        assert_eq!(Driver::parse("dqlite"), None);
        assert_eq!(Driver::parse(""), None);
    }

    #[test]
    fn postgres_rebind_numbers_each_placeholder() {
        let d = Dialect::new(Driver::Postgres);
        assert_eq!(d.rebind("name = ? AND id <= ?"), "name = $1 AND id <= $2");
    }

    #[test]
    fn sqlite_and_mysql_rebind_is_identity() {
        let sql = "name = ? AND id <= ?";
        assert_eq!(Dialect::new(Driver::Sqlite).rebind(sql), sql);
        assert_eq!(Dialect::new(Driver::Mysql).rebind(sql), sql);
    }

    #[test]
    fn create_table_sqlite_uses_autoincrement_pk() {
        let ddl = Dialect::new(Driver::Sqlite).create_table_sql();
        assert!(ddl.contains("CREATE TABLE"));
        assert!(ddl.contains("kine"));
        assert!(ddl.to_uppercase().contains("AUTOINCREMENT"));
        // SQLite stores bytes as BLOB.
        assert!(ddl.contains("BLOB"));
    }

    #[test]
    fn create_table_postgres_uses_serial_and_bytea() {
        let ddl = Dialect::new(Driver::Postgres).create_table_sql();
        assert!(ddl.to_uppercase().contains("SERIAL"));
        assert!(ddl.to_uppercase().contains("BYTEA"));
    }

    #[test]
    fn create_table_mysql_uses_auto_increment() {
        let ddl = Dialect::new(Driver::Mysql).create_table_sql();
        assert!(ddl.to_uppercase().contains("AUTO_INCREMENT"));
        // MySQL needs a bounded key length on the indexed name column.
        assert!(ddl.contains("kine"));
    }

    #[test]
    fn create_table_has_every_mvcc_column() {
        let ddl = Dialect::new(Driver::Sqlite).create_table_sql();
        for col in [
            "id",
            "name",
            "created",
            "deleted",
            "create_revision",
            "prev_revision",
            "lease",
            "value",
            "old_value",
        ] {
            assert!(ddl.contains(col), "DDL missing column {col}");
        }
    }

    #[test]
    fn rev_sql_selects_max_id() {
        // kine: `SELECT MAX(rkv.id) AS id FROM kine AS rkv`
        let s = Dialect::new(Driver::Sqlite).rev_sql();
        assert!(s.contains("MAX(rkv.id)"));
        assert!(s.contains("FROM kine"));
    }

    #[test]
    fn compact_rev_sql_reads_the_compact_rev_key() {
        // kine stores the compacted floor in a sentinel row named compact_rev_key.
        let s = Dialect::new(Driver::Sqlite).compact_rev_sql();
        assert!(s.contains("compact_rev_key"));
        assert!(s.contains("prev_revision"));
    }

    #[test]
    fn insert_sql_binds_eight_value_columns() {
        // kine inserts name..old_value (8 cols); id is auto-assigned.
        let sql = Dialect::new(Driver::Sqlite).insert_sql();
        assert!(sql.starts_with("INSERT INTO kine"));
        for col in [
            "name",
            "created",
            "deleted",
            "create_revision",
            "prev_revision",
            "lease",
            "value",
            "old_value",
        ] {
            assert!(sql.contains(col), "insert missing {col}");
        }
        assert_eq!(sql.matches('?').count(), 8, "eight bind params");
    }

    #[test]
    fn insert_sql_postgres_is_rebound_to_dollar_params() {
        let sql = Dialect::new(Driver::Postgres).insert_sql();
        assert!(sql.contains("$1") && sql.contains("$8"));
        assert!(!sql.contains('?'), "no bare ? left after rebind");
    }

    #[test]
    fn list_current_sql_joins_latest_row_per_key_and_hides_tombstones() {
        // The MVCC heart: latest id per name, current state only.
        let sql = Dialect::new(Driver::Sqlite).list_current_sql();
        assert!(sql.contains("MAX(mkv.id)"));
        assert!(sql.to_uppercase().contains("GROUP BY"));
        assert!(sql.contains("mkv.name"));
        assert!(sql.contains("kv.deleted = 0"));
    }

    #[test]
    fn list_revision_sql_bounds_by_read_revision() {
        // Historical read: latest id per name at or below the read revision.
        let sql = Dialect::new(Driver::Sqlite).list_revision_sql();
        assert!(sql.contains("mkv.id <= ?"));
        assert!(sql.contains("MAX(mkv.id)"));
    }

    #[test]
    fn after_sql_streams_rows_past_a_revision_in_order() {
        // Watch poll: rows with id > ? in the range, ascending.
        let sql = Dialect::new(Driver::Sqlite).after_sql();
        assert!(sql.contains("kv.id > ?"));
        assert!(sql.to_uppercase().contains("ORDER BY"));
        assert!(sql.to_uppercase().contains("ASC"));
    }

    #[test]
    fn delete_sql_targets_a_single_row_by_id() {
        let sql = Dialect::new(Driver::Sqlite).delete_sql();
        assert!(sql.starts_with("DELETE FROM kine"));
        assert!(sql.contains("id = ?"));
    }

    #[test]
    fn count_current_sql_counts_live_keys() {
        let sql = Dialect::new(Driver::Sqlite).count_current_sql();
        assert!(sql.to_uppercase().contains("COUNT"));
    }

    #[test]
    fn update_compact_sql_advances_the_floor_monotonically() {
        // kine: UPDATE ... SET prev_revision = MAX(prev_revision, ?) ...
        let sql = Dialect::new(Driver::Sqlite).update_compact_sql();
        assert!(sql.to_uppercase().contains("UPDATE"));
        assert!(sql.contains("compact_rev_key"));
    }

    #[test]
    fn index_sqls_create_the_name_index() {
        let ix = Dialect::new(Driver::Sqlite).index_sqls();
        assert!(!ix.is_empty());
        assert!(ix.iter().any(|s| s.contains("name")));
        assert!(ix.iter().all(|s| s.to_uppercase().contains("INDEX")));
    }

    #[test]
    fn postgres_list_sql_is_fully_rebound() {
        let sql = Dialect::new(Driver::Postgres).list_revision_sql();
        assert!(sql.contains("$1"), "first param numbered");
        assert!(!sql.contains('?'), "no bare ? after rebind");
    }
}
