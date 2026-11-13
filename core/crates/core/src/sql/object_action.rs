//! The statements behind the object tree's menu: rename, truncate, drop,
//! maintenance and comments (`docs/decisions/0017-tree-actions.md`). Each is
//! written for its engine and shown before it runs, as edits are (ADR 0010);
//! an action an engine has no statement for is refused here, so the menu can
//! say so instead of the server.

use serde::{Deserialize, Serialize};

use super::lex::{Kind, lex};
use super::literal::quoted;
use super::{SqlGrammar, quote_name, table_name};
use crate::catalog::{ObjectInfo, ObjectKind, TableRef};
use crate::{DriverError, Result};

/// A maintenance task on one table, as its engine names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Maintenance {
    /// PostgreSQL `VACUUM`: frees the space of dead rows for reuse.
    Vacuum,
    /// PostgreSQL `VACUUM FULL`: rewrites the table, locking it meanwhile.
    VacuumFull,
    /// `ANALYZE` (PostgreSQL, SQLite): refreshes the planner's statistics.
    Analyze,
    /// PostgreSQL `VACUUM ANALYZE`.
    VacuumAnalyze,
    /// `REINDEX` (PostgreSQL, SQLite): builds the table's indexes again.
    Reindex,
    /// SQLite `VACUUM`: rewrites the whole database file.
    VacuumDatabase,
    /// SQL Server `UPDATE STATISTICS`.
    UpdateStatistics,
    /// SQL Server `ALTER INDEX ALL ... REBUILD`.
    Rebuild,
    /// SQL Server `ALTER INDEX ALL ... REORGANIZE`.
    Reorganize,
    /// SQL Server `DBCC CHECKTABLE`: checks the table's pages and structure.
    CheckTable,
}

/// The tasks an engine offers, in the order the menu lists them.
pub fn maintenance_tasks(grammar: SqlGrammar) -> &'static [Maintenance] {
    match grammar {
        SqlGrammar::Postgres => &[
            Maintenance::VacuumAnalyze,
            Maintenance::Vacuum,
            Maintenance::Analyze,
            Maintenance::Reindex,
            Maintenance::VacuumFull,
        ],
        SqlGrammar::SqlServer => &[
            Maintenance::UpdateStatistics,
            Maintenance::Reorganize,
            Maintenance::Rebuild,
            Maintenance::CheckTable,
        ],
        SqlGrammar::Sqlite => &[Maintenance::Analyze, Maintenance::Reindex, Maintenance::VacuumDatabase],
    }
}

/// What the menu asks to do with one object.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ObjectAction {
    Rename {
        name: String,
    },
    Truncate,
    /// `cascade` also drops what depends on it (PostgreSQL only).
    Drop {
        cascade: bool,
    },
    Maintain {
        task: Maintenance,
    },
    /// An empty comment removes it.
    Comment {
        text: String,
    },
}

/// The statements `action` runs on `object`, in order.
pub fn action_statements(grammar: SqlGrammar, object: &ObjectInfo, action: &ObjectAction) -> Result<Vec<String>> {
    let name = table_name(
        grammar,
        &TableRef {
            schema: object.schema.clone(),
            name: object.name.clone(),
        },
    );
    match action {
        ObjectAction::Rename { name: new } => rename(grammar, object, &name, new),
        ObjectAction::Truncate => {
            table_only(object, "truncate")?;
            Ok(vec![match grammar {
                // SQLite has no TRUNCATE; a DELETE with no WHERE empties the table the fast way.
                SqlGrammar::Sqlite => format!("DELETE FROM {name}"),
                SqlGrammar::Postgres | SqlGrammar::SqlServer => format!("TRUNCATE TABLE {name}"),
            }])
        }
        ObjectAction::Drop { cascade } => {
            let what = match (object.kind, grammar) {
                (ObjectKind::Table, _) => "TABLE",
                (ObjectKind::View, _) => "VIEW",
                (ObjectKind::MaterializedView, SqlGrammar::Postgres) => "MATERIALIZED VIEW",
                (ObjectKind::Sequence, SqlGrammar::Postgres | SqlGrammar::SqlServer) => "SEQUENCE",
                (ObjectKind::Procedure, SqlGrammar::SqlServer) => "PROCEDURE",
                (ObjectKind::Function | ObjectKind::TableFunction, SqlGrammar::SqlServer) => "FUNCTION",
                (ObjectKind::Index, SqlGrammar::Sqlite) => "INDEX",
                (ObjectKind::Trigger, SqlGrammar::Sqlite) => "TRIGGER",
                _ => return Err(DriverError::unsupported("Kemable cannot drop this kind of object yet")),
            };
            let tail = if *cascade && grammar == SqlGrammar::Postgres {
                " CASCADE"
            } else {
                ""
            };
            Ok(vec![format!("DROP {what} {name}{tail}")])
        }
        ObjectAction::Maintain { task } => {
            table_only(object, "maintain")?;
            if !maintenance_tasks(grammar).contains(task) {
                return Err(DriverError::unsupported("this engine has no such maintenance task"));
            }
            Ok(vec![match task {
                Maintenance::Vacuum => format!("VACUUM {name}"),
                Maintenance::VacuumFull => format!("VACUUM FULL {name}"),
                Maintenance::Analyze => format!("ANALYZE {name}"),
                Maintenance::VacuumAnalyze => format!("VACUUM ANALYZE {name}"),
                Maintenance::Reindex if grammar == SqlGrammar::Postgres => format!("REINDEX TABLE {name}"),
                Maintenance::Reindex => format!("REINDEX {name}"),
                Maintenance::VacuumDatabase => "VACUUM".to_owned(),
                Maintenance::UpdateStatistics => format!("UPDATE STATISTICS {name}"),
                Maintenance::Rebuild => format!("ALTER INDEX ALL ON {name} REBUILD"),
                Maintenance::Reorganize => format!("ALTER INDEX ALL ON {name} REORGANIZE"),
                Maintenance::CheckTable => format!("DBCC CHECKTABLE ({})", quoted(grammar, &name)),
            }])
        }
        ObjectAction::Comment { text } => comment(grammar, object, &name, text),
    }
}

fn table_only(object: &ObjectInfo, what: &str) -> Result<()> {
    if object.kind == ObjectKind::Table {
        Ok(())
    } else {
        Err(DriverError::unsupported(format!("only a table can {what}")))
    }
}

fn rename(grammar: SqlGrammar, object: &ObjectInfo, name: &str, new: &str) -> Result<Vec<String>> {
    let new = new.trim();
    if new.is_empty() {
        return Err(DriverError::config("a new name is needed"));
    }
    if new == object.name {
        return Err(DriverError::config("the new name is the old one"));
    }
    match grammar {
        SqlGrammar::Postgres => {
            let what = match object.kind {
                ObjectKind::Table => "TABLE",
                ObjectKind::View => "VIEW",
                ObjectKind::MaterializedView => "MATERIALIZED VIEW",
                ObjectKind::Sequence => "SEQUENCE",
                // A routine is named with its argument types, which the tree does not hold.
                _ => {
                    return Err(DriverError::unsupported(
                        "Kemable cannot rename this kind of object yet",
                    ));
                }
            };
            Ok(vec![format!(
                "ALTER {what} {name} RENAME TO {}",
                quote_name(grammar, new)
            )])
        }
        SqlGrammar::SqlServer => {
            if !matches!(object.kind, ObjectKind::Table | ObjectKind::View) {
                return Err(DriverError::unsupported("Kemable renames tables and views only"));
            }
            // sp_rename takes the new name as it is, not bracketed: brackets would become part of it.
            Ok(vec![format!(
                "EXEC sp_rename {}, {}",
                quoted(grammar, name),
                quoted(grammar, new)
            )])
        }
        SqlGrammar::Sqlite => {
            if object.kind != ObjectKind::Table {
                return Err(DriverError::unsupported("SQLite renames tables only"));
            }
            Ok(vec![format!(
                "ALTER TABLE {name} RENAME TO {}",
                quote_name(grammar, new)
            )])
        }
    }
}

fn comment(grammar: SqlGrammar, object: &ObjectInfo, name: &str, text: &str) -> Result<Vec<String>> {
    match grammar {
        SqlGrammar::Postgres => {
            let what = match object.kind {
                ObjectKind::Table => "TABLE",
                ObjectKind::View => "VIEW",
                ObjectKind::MaterializedView => "MATERIALIZED VIEW",
                ObjectKind::Sequence => "SEQUENCE",
                _ => {
                    return Err(DriverError::unsupported(
                        "Kemable cannot comment on this kind of object yet",
                    ));
                }
            };
            let value = if text.is_empty() {
                "NULL".to_owned()
            } else {
                quoted(grammar, text)
            };
            Ok(vec![format!("COMMENT ON {what} {name} IS {value}")])
        }
        SqlGrammar::SqlServer => {
            let level1 = match object.kind {
                ObjectKind::Table => "TABLE",
                ObjectKind::View => "VIEW",
                ObjectKind::Procedure => "PROCEDURE",
                ObjectKind::Function | ObjectKind::TableFunction => "FUNCTION",
                ObjectKind::Sequence => "SEQUENCE",
                _ => {
                    return Err(DriverError::unsupported(
                        "Kemable cannot comment on this kind of object yet",
                    ));
                }
            };
            let schema = object.schema.as_deref().unwrap_or("dbo");
            let target = format!(
                "@level0type = N'SCHEMA', @level0name = {}, @level1type = N'{level1}', @level1name = {}",
                quoted(grammar, schema),
                quoted(grammar, &object.name)
            );
            let exists = format!(
                "EXISTS (SELECT 1 FROM sys.extended_properties WHERE major_id = OBJECT_ID({}) AND minor_id = 0 AND name = N'MS_Description')",
                quoted(grammar, name)
            );
            // One statement either way, so it runs the same however the script is split.
            Ok(vec![if text.is_empty() {
                format!("IF {exists} EXEC sys.sp_dropextendedproperty @name = N'MS_Description', {target}")
            } else {
                let value = quoted(grammar, text);
                format!(
                    "IF {exists} EXEC sys.sp_updateextendedproperty @name = N'MS_Description', @value = {value}, {target} \
                     ELSE EXEC sys.sp_addextendedproperty @name = N'MS_Description', @value = {value}, {target}"
                )
            }])
        }
        SqlGrammar::Sqlite => Err(DriverError::unsupported("SQLite keeps no comments on its objects")),
    }
}

/// A view's definition to edit and run again: the one statement that
/// replaces it (`CREATE OR REPLACE VIEW`, SQL Server's `CREATE OR ALTER`;
/// SQLite drops the view first). `ddl` is the driver's `object_ddl`.
pub fn view_definition(grammar: SqlGrammar, object: &ObjectInfo, ddl: &str) -> String {
    let ddl = ddl.trim();
    match grammar {
        // The driver already writes CREATE OR REPLACE VIEW.
        SqlGrammar::Postgres => ddl.to_owned(),
        SqlGrammar::SqlServer => {
            // OBJECT_DEFINITION keeps the comments before its CREATE: find the word itself.
            let words: Vec<_> = lex(ddl, grammar)
                .tokens
                .into_iter()
                .filter(|token| token.kind == Kind::Word)
                .take(2)
                .collect();
            match words.as_slice() {
                [create, next, ..] if create.is_word(ddl, "CREATE") && !next.is_word(ddl, "OR") => {
                    format!("{}CREATE OR ALTER{}", &ddl[..create.start], &ddl[create.end..])
                }
                _ => ddl.to_owned(),
            }
        }
        SqlGrammar::Sqlite => {
            let name = table_name(
                grammar,
                &TableRef {
                    schema: object.schema.clone().filter(|schema| schema != "main"),
                    name: object.name.clone(),
                },
            );
            format!("DROP VIEW IF EXISTS {name};\n{ddl}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(kind: ObjectKind) -> ObjectInfo {
        ObjectInfo {
            schema: Some("dbo".into()),
            name: "Khách hàng".into(),
            kind,
        }
    }

    fn one(grammar: SqlGrammar, object: &ObjectInfo, action: ObjectAction) -> String {
        let mut statements = action_statements(grammar, object, &action).unwrap();
        assert_eq!(statements.len(), 1);
        statements.remove(0)
    }

    #[test]
    fn each_engine_renames_its_own_way() {
        let table = object(ObjectKind::Table);
        let rename = || ObjectAction::Rename {
            name: "Khách] mới".into(),
        };
        assert_eq!(
            one(SqlGrammar::Postgres, &table, rename()),
            r#"ALTER TABLE "dbo"."Khách hàng" RENAME TO "Khách] mới""#
        );
        // sp_rename reads the new name as it is: no brackets around it.
        assert_eq!(
            one(SqlGrammar::SqlServer, &table, rename()),
            "EXEC sp_rename N'[dbo].[Khách hàng]', N'Khách] mới'"
        );
        assert_eq!(
            one(SqlGrammar::Sqlite, &table, rename()),
            r#"ALTER TABLE "dbo"."Khách hàng" RENAME TO "Khách] mới""#
        );
        assert_eq!(
            one(SqlGrammar::Postgres, &object(ObjectKind::MaterializedView), rename()),
            r#"ALTER MATERIALIZED VIEW "dbo"."Khách hàng" RENAME TO "Khách] mới""#
        );
        for (grammar, kind) in [
            (SqlGrammar::Sqlite, ObjectKind::View),
            (SqlGrammar::Postgres, ObjectKind::Function),
            (SqlGrammar::SqlServer, ObjectKind::Procedure),
        ] {
            assert!(
                action_statements(grammar, &object(kind), &rename()).is_err(),
                "{grammar:?} {kind:?}"
            );
        }
        let blank = ObjectAction::Rename { name: "  ".into() };
        assert_eq!(
            action_statements(SqlGrammar::Postgres, &table, &blank)
                .unwrap_err()
                .code,
            crate::ErrorCode::ConfigInvalid
        );
    }

    #[test]
    fn truncate_and_drop_are_written_for_the_kind_and_the_engine() {
        let table = object(ObjectKind::Table);
        assert_eq!(
            one(SqlGrammar::SqlServer, &table, ObjectAction::Truncate),
            "TRUNCATE TABLE [dbo].[Khách hàng]"
        );
        assert_eq!(
            one(SqlGrammar::Sqlite, &table, ObjectAction::Truncate),
            r#"DELETE FROM "dbo"."Khách hàng""#
        );
        assert!(action_statements(SqlGrammar::Postgres, &object(ObjectKind::View), &ObjectAction::Truncate).is_err());

        let drop = |cascade| ObjectAction::Drop { cascade };
        assert_eq!(
            one(SqlGrammar::Postgres, &table, drop(true)),
            r#"DROP TABLE "dbo"."Khách hàng" CASCADE"#
        );
        assert_eq!(
            one(SqlGrammar::SqlServer, &table, drop(true)),
            "DROP TABLE [dbo].[Khách hàng]"
        );
        assert_eq!(
            one(SqlGrammar::SqlServer, &object(ObjectKind::TableFunction), drop(false)),
            "DROP FUNCTION [dbo].[Khách hàng]"
        );
        assert_eq!(
            one(SqlGrammar::Postgres, &object(ObjectKind::MaterializedView), drop(false)),
            r#"DROP MATERIALIZED VIEW "dbo"."Khách hàng""#
        );
        assert!(action_statements(SqlGrammar::Postgres, &object(ObjectKind::Function), &drop(false)).is_err());
    }

    #[test]
    fn maintenance_offers_and_writes_the_engines_own_tasks() {
        let table = object(ObjectKind::Table);
        let task = |task| ObjectAction::Maintain { task };
        assert_eq!(
            one(SqlGrammar::Postgres, &table, task(Maintenance::VacuumAnalyze)),
            r#"VACUUM ANALYZE "dbo"."Khách hàng""#
        );
        assert_eq!(
            one(SqlGrammar::Postgres, &table, task(Maintenance::Reindex)),
            r#"REINDEX TABLE "dbo"."Khách hàng""#
        );
        assert_eq!(
            one(SqlGrammar::Sqlite, &table, task(Maintenance::Reindex)),
            r#"REINDEX "dbo"."Khách hàng""#
        );
        assert_eq!(
            one(SqlGrammar::Sqlite, &table, task(Maintenance::VacuumDatabase)),
            "VACUUM"
        );
        assert_eq!(
            one(SqlGrammar::SqlServer, &table, task(Maintenance::Rebuild)),
            "ALTER INDEX ALL ON [dbo].[Khách hàng] REBUILD"
        );
        assert_eq!(
            one(SqlGrammar::SqlServer, &table, task(Maintenance::CheckTable)),
            "DBCC CHECKTABLE (N'[dbo].[Khách hàng]')"
        );
        // A task of another engine is refused, not sent.
        assert!(action_statements(SqlGrammar::SqlServer, &table, &task(Maintenance::Vacuum)).is_err());
        for grammar in [SqlGrammar::Postgres, SqlGrammar::SqlServer, SqlGrammar::Sqlite] {
            assert!(!maintenance_tasks(grammar).is_empty());
        }
    }

    #[test]
    fn a_comment_is_set_and_an_empty_one_removed() {
        let table = object(ObjectKind::Table);
        let comment = |text: &str| ObjectAction::Comment { text: text.into() };
        assert_eq!(
            one(SqlGrammar::Postgres, &table, comment("Danh sách 'khách'")),
            r#"COMMENT ON TABLE "dbo"."Khách hàng" IS 'Danh sách ''khách'''"#
        );
        assert_eq!(
            one(SqlGrammar::Postgres, &table, comment("")),
            r#"COMMENT ON TABLE "dbo"."Khách hàng" IS NULL"#
        );
        let set = one(SqlGrammar::SqlServer, &table, comment("Khách"));
        assert!(
            set.starts_with(
                "IF EXISTS (SELECT 1 FROM sys.extended_properties WHERE major_id = OBJECT_ID(N'[dbo].[Khách hàng]')"
            ),
            "{set}"
        );
        assert!(
            set.contains("sp_updateextendedproperty @name = N'MS_Description', @value = N'Khách'")
                && set.contains("ELSE EXEC sys.sp_addextendedproperty"),
            "{set}"
        );
        assert!(
            set.contains("@level1type = N'TABLE', @level1name = N'Khách hàng'"),
            "{set}"
        );
        assert!(one(SqlGrammar::SqlServer, &table, comment("")).contains("sp_dropextendedproperty"));
        assert!(action_statements(SqlGrammar::Sqlite, &table, &comment("x")).is_err());
    }

    #[test]
    fn a_views_definition_becomes_the_statement_that_replaces_it() {
        let view = object(ObjectKind::View);
        assert_eq!(
            view_definition(
                SqlGrammar::SqlServer,
                &view,
                "-- orders\nCREATE VIEW dbo.v AS SELECT 1 AS x"
            ),
            "-- orders\nCREATE OR ALTER VIEW dbo.v AS SELECT 1 AS x"
        );
        assert_eq!(
            view_definition(SqlGrammar::SqlServer, &view, "create or alter view dbo.v as select 1"),
            "create or alter view dbo.v as select 1"
        );
        let sqlite = ObjectInfo {
            schema: Some("main".into()),
            name: "v".into(),
            kind: ObjectKind::View,
        };
        assert_eq!(
            view_definition(SqlGrammar::Sqlite, &sqlite, "CREATE VIEW v AS SELECT 1;"),
            "DROP VIEW IF EXISTS \"v\";\nCREATE VIEW v AS SELECT 1;"
        );
        assert_eq!(
            view_definition(SqlGrammar::Postgres, &view, "CREATE OR REPLACE VIEW x AS SELECT 1;"),
            "CREATE OR REPLACE VIEW x AS SELECT 1;"
        );
    }
}
