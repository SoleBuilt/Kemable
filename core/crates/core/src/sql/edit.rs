//! The statements that save a table tab's edits (`docs/decisions/0010-editing-rows.md`):
//! an `UPDATE` per changed row, a `DELETE` per deleted row and an `INSERT`
//! per new row, each naming its row by the primary key's original values.
//!
//! An original value is written exactly, as the literal its kind needs
//! (`value_literal`); a typed value is text the server converts to the
//! column's type, as a filter's value is (`literal::typed`), but for a
//! binary column, where it is the hex of the bytes.

use super::literal::{parse_hex, typed, value_literal};
use super::{SqlGrammar, quote_name as ident, table_name};
use crate::catalog::{ColumnMeta, TableRef};
use crate::error::DriverError;
use crate::value::{Value, ValueKind};

/// What a cell becomes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "text")]
pub enum NewValue {
    Null,
    /// The column's default, for a new row's cell left empty.
    Default,
    Text(String),
}

/// One row's edit, with its key's original values. A new value goes with
/// its column as the page's result set describes it, so it is written for
/// the column's type.
#[derive(Debug, Clone, PartialEq)]
pub enum RowEdit {
    Update {
        key: Vec<(String, Value)>,
        set: Vec<(ColumnMeta, NewValue)>,
    },
    Delete {
        key: Vec<(String, Value)>,
    },
    Insert {
        values: Vec<(ColumnMeta, NewValue)>,
    },
}

/// The statement that saves `edit`.
pub fn edit_statement(grammar: SqlGrammar, table: &TableRef, edit: &RowEdit) -> Result<String, DriverError> {
    let name = table_name(grammar, table);
    match edit {
        RowEdit::Update { key, set } => {
            if set.is_empty() {
                return Err(DriverError::config("an update changes at least one column"));
            }
            let assignments: Vec<String> = set
                .iter()
                .map(|(column, value)| {
                    Ok(format!(
                        "{} = {}",
                        ident(grammar, &column.name),
                        new_value(grammar, column, value)?
                    ))
                })
                .collect::<Result<_, DriverError>>()?;
            Ok(format!(
                "UPDATE {name} SET {} WHERE {}",
                assignments.join(", "),
                key_condition(grammar, key)?
            ))
        }
        RowEdit::Delete { key } => Ok(format!("DELETE FROM {name} WHERE {}", key_condition(grammar, key)?)),
        RowEdit::Insert { values } => {
            let given: Vec<&(ColumnMeta, NewValue)> =
                values.iter().filter(|(_, value)| *value != NewValue::Default).collect();
            if given.is_empty() {
                return Ok(format!("INSERT INTO {name} DEFAULT VALUES"));
            }
            let columns: Vec<String> = given.iter().map(|(column, _)| ident(grammar, &column.name)).collect();
            let literals: Vec<String> = given
                .iter()
                .map(|(column, value)| new_value(grammar, column, value))
                .collect::<Result<_, DriverError>>()?;
            Ok(format!(
                "INSERT INTO {name} ({}) VALUES ({})",
                columns.join(", "),
                literals.join(", ")
            ))
        }
    }
}

fn key_condition(grammar: SqlGrammar, key: &[(String, Value)]) -> Result<String, DriverError> {
    if key.is_empty() {
        return Err(DriverError::config(
            "a row is named by its primary key, and this table has none",
        ));
    }
    key.iter()
        .map(|(column, value)| {
            let column = ident(grammar, column);
            Ok(match value {
                Value::Null => format!("{column} IS NULL"),
                value => format!(
                    "{column} = {}",
                    value_literal(grammar, value)
                        .ok_or_else(|| DriverError::config("a key of this type cannot be written as a literal"))?
                ),
            })
        })
        .collect::<Result<Vec<_>, DriverError>>()
        .map(|parts| parts.join(" AND "))
}

fn new_value(grammar: SqlGrammar, column: &ColumnMeta, value: &NewValue) -> Result<String, DriverError> {
    Ok(match value {
        NewValue::Null => "NULL".to_owned(),
        NewValue::Default => "DEFAULT".to_owned(),
        // The editor starts a binary cell from its hex: the edit is bytes, never the hex's text.
        NewValue::Text(text) if column.kind == ValueKind::Bytes => match parse_hex(text) {
            Some(bytes) => value_literal(grammar, &Value::Bytes(bytes)).unwrap_or_else(|| "NULL".to_owned()),
            // SQLite keeps any value in any column, so text that is not hex stays text.
            None if grammar == SqlGrammar::Sqlite => typed(grammar, text),
            None => {
                return Err(DriverError::config(format!(
                    "{} holds bytes: write them as hex, such as 0x00ff",
                    column.name
                )));
            }
        },
        NewValue::Text(text) => typed(grammar, text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TableRef {
        TableRef {
            schema: Some("public".into()),
            name: "orders".into(),
        }
    }

    fn column(name: &str, type_name: &str, kind: ValueKind) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            type_name: type_name.into(),
            kind,
        }
    }

    fn text(name: &str) -> ColumnMeta {
        column(name, "text", ValueKind::Text)
    }

    fn statement(grammar: SqlGrammar, edit: RowEdit) -> String {
        edit_statement(grammar, &table(), &edit).unwrap_or_else(|error| panic!("{error:?}"))
    }

    #[test]
    fn an_update_sets_the_changed_columns_where_the_key_was() {
        let edit = RowEdit::Update {
            key: vec![("id".into(), Value::Int(7))],
            set: vec![
                (text("note"), NewValue::Text("it's done".into())),
                (column("paid_at", "timestamp", ValueKind::Timestamp), NewValue::Null),
            ],
        };
        assert_eq!(
            statement(SqlGrammar::Postgres, edit.clone()),
            "UPDATE \"public\".\"orders\" SET \"note\" = 'it''s done', \"paid_at\" = NULL WHERE \"id\" = 7"
        );
        assert_eq!(
            statement(SqlGrammar::SqlServer, edit),
            "UPDATE [public].[orders] SET [note] = N'it''s done', [paid_at] = NULL WHERE [id] = 7"
        );
    }

    #[test]
    fn a_key_of_several_columns_names_each() {
        let edit = RowEdit::Delete {
            key: vec![
                ("order_id".into(), Value::Int(1)),
                ("sku".into(), Value::Text("A-1".into())),
            ],
        };
        assert_eq!(
            statement(SqlGrammar::Sqlite, edit),
            "DELETE FROM \"public\".\"orders\" WHERE \"order_id\" = 1 AND \"sku\" = 'A-1'"
        );
    }

    #[test]
    fn an_insert_leaves_out_defaults_and_a_row_of_defaults_says_so() {
        let edit = RowEdit::Insert {
            values: vec![
                (column("id", "INTEGER", ValueKind::Int), NewValue::Default),
                (text("name"), NewValue::Text("Bút".into())),
                (column("price", "REAL", ValueKind::Float), NewValue::Text("12.5".into())),
                (text("note"), NewValue::Null),
            ],
        };
        assert_eq!(
            statement(SqlGrammar::Sqlite, edit),
            "INSERT INTO \"public\".\"orders\" (\"name\", \"price\", \"note\") VALUES ('Bút', 12.5, NULL)"
        );
        let empty = RowEdit::Insert {
            values: vec![(column("id", "int4", ValueKind::Int), NewValue::Default)],
        };
        assert_eq!(
            statement(SqlGrammar::Postgres, empty),
            "INSERT INTO \"public\".\"orders\" DEFAULT VALUES"
        );
    }

    #[test]
    fn a_row_without_a_key_or_an_update_without_a_change_is_refused() {
        let no_key = RowEdit::Delete { key: Vec::new() };
        assert!(edit_statement(SqlGrammar::Postgres, &table(), &no_key).is_err());
        let no_change = RowEdit::Update {
            key: vec![("id".into(), Value::Int(1))],
            set: Vec::new(),
        };
        assert!(edit_statement(SqlGrammar::Postgres, &table(), &no_change).is_err());
    }

    #[test]
    fn a_binary_cell_is_written_as_the_bytes_its_hex_names() {
        let edit = |kind: ValueKind, value: &str| RowEdit::Update {
            key: vec![("id".into(), Value::Int(1))],
            set: vec![(column("data", "bytea", kind), NewValue::Text(value.into()))],
        };
        assert_eq!(
            statement(SqlGrammar::Postgres, edit(ValueKind::Bytes, "0x00fe")),
            "UPDATE \"public\".\"orders\" SET \"data\" = '\\x00fe'::bytea WHERE \"id\" = 1"
        );
        assert_eq!(
            statement(SqlGrammar::SqlServer, edit(ValueKind::Bytes, "0x00FE")),
            "UPDATE [public].[orders] SET [data] = 0x00fe WHERE [id] = 1"
        );
        assert_eq!(
            statement(SqlGrammar::Sqlite, edit(ValueKind::Bytes, "0x")),
            "UPDATE \"public\".\"orders\" SET \"data\" = X'' WHERE \"id\" = 1"
        );
        // Text that is not hex: refused where the column holds only bytes, text in SQLite.
        assert!(edit_statement(SqlGrammar::Postgres, &table(), &edit(ValueKind::Bytes, "abc")).is_err());
        assert!(edit_statement(SqlGrammar::SqlServer, &table(), &edit(ValueKind::Bytes, "0x0")).is_err());
        assert!(statement(SqlGrammar::Sqlite, edit(ValueKind::Bytes, "abc")).contains("= 'abc'"));
        // The same text in a text column is text.
        assert!(statement(SqlGrammar::Postgres, edit(ValueKind::Text, "0x00fe")).contains("= '0x00fe'"));
        let insert = RowEdit::Insert {
            values: vec![(
                column("data", "varbinary(4)", ValueKind::Bytes),
                NewValue::Text("0x0102".into()),
            )],
        };
        assert!(statement(SqlGrammar::SqlServer, insert).ends_with("VALUES (0x0102)"));
    }

    #[test]
    fn a_null_key_is_matched_with_is_null() {
        let edit = RowEdit::Delete {
            key: vec![("id".into(), Value::Null)],
        };
        assert!(statement(SqlGrammar::Postgres, edit).ends_with("WHERE \"id\" IS NULL"));
    }
}
