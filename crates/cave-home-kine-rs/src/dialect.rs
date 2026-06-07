// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Per-driver SQL generation — kine's `generic` backend dialect layer.
//!
//! kine runs the *same* etcd-MVCC semantics over SQLite, Postgres and MySQL by
//! keeping one set of query templates and varying only the small, driver-
//! specific pieces: the auto-increment primary key type, the blob column type,
//! and the bind-parameter placeholder syntax (`?` for SQLite/MySQL, `$1..$N`
//! for Postgres). This module is that variance, isolated as pure string
//! generation so it can be tested exhaustively without a database.
//!
//! Reference: `k3s-io/kine` `pkg/drivers/generic/generic.go` (the shared
//! `columns` / `revSQL` / `compactRevSQL` / `listSQL` constants and the `q()`
//! placeholder rebinder) plus the per-driver `pkg/drivers/{sqlite,pgsql,mysql}`
//! overrides (column types, DDL). Faithful reproduction of the query structure,
//! Apache-2.0.

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
