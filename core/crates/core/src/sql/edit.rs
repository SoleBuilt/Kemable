//! The statements that save a table tab's edits (`docs/decisions/0010-editing-rows.md`):
//! an `UPDATE` per changed row, a `DELETE` per deleted row and an `INSERT`
//! per new row, each naming its row by the primary key's original values.
//!
//! An original value is written exactly, as the literal its kind needs
//! (`value_literal`); a typed value is text the server converts to the
//! column's type, as a filter's value is (`literal::typed`).

use super::literal::{typed, value_literal};
use super::{SqlGrammar, quote_name as ident, table_name};
use crate::catalog::TableRef;
use crate::error::DriverError;
use crate::value::Value;

/// What a cell becomes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "text")]
pub enum NewValue {
    Null,
    /// The column's default, for a new row's cell left empty.
    Default,
    Text(String),
}

/// One row's edit, with its key's original values.
#[derive(Debug, Clone, PartialEq)]
pub enum RowEdit {
    Update {
        key: Vec<(String, Value)>,
        set: Vec<(String, NewValue)>,
    },
    Delete {
        key: Vec<(String, Value)>,
    },
    Insert {
        values: Vec<(String, NewValue)>,
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
                .map(|(column, value)| format!("{} = {}", ident(grammar, column), new_value(grammar, value)))
                .collect();
            Ok(format!(
                "UPDATE {name} SET {} WHERE {}",
                assignments.join(", "),
                key_condition(grammar, key)?
            ))
        }
        RowEdit::Delete { key } => Ok(format!("DELETE FROM {name} WHERE {}", key_condition(grammar, key)?)),
        RowEdit::Insert { values } => {
            let given: Vec<&(String, NewValue)> =
                values.iter().filter(|(_, value)| *value != NewValue::Default).collect();
            if given.is_empty() {
                return Ok(format!("INSERT INTO {name} DEFAULT VALUES"));
            }
            let columns: Vec<String> = given.iter().map(|(column, _)| ident(grammar, column)).collect();
            let literals: Vec<String> = given.iter().map(|(_, value)| new_value(grammar, value)).collect();
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

fn new_value(grammar: SqlGrammar, value: &NewValue) -> String {
    match value {
        NewValue::Null => "NULL".to_owned(),
        NewValue::Default => "DEFAULT".to_owned(),
        NewValue::Text(text) => typed(grammar, text),
    }
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

    fn statement(grammar: SqlGrammar, edit: RowEdit) -> String {
        edit_statement(grammar, &table(), &edit).unwrap_or_else(|error| panic!("{error:?}"))
    }

    #[test]
    fn an_update_sets_the_changed_columns_where_the_key_was() {
        let edit = RowEdit::Update {
            key: vec![("id".into(), Value::Int(7))],
            set: vec![
                ("note".into(), NewValue::Text("it's done".into())),
                ("paid_at".into(), NewValue::Null),
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
                ("id".into(), NewValue::Default),
                ("name".into(), NewValue::Text("Bút".into())),
                ("price".into(), NewValue::Text("12.5".into())),
                ("note".into(), NewValue::Null),
            ],
        };
        assert_eq!(
            statement(SqlGrammar::Sqlite, edit),
            "INSERT INTO \"public\".\"orders\" (\"name\", \"price\", \"note\") VALUES ('Bút', 12.5, NULL)"
        );
        let empty = RowEdit::Insert {
            values: vec![("id".into(), NewValue::Default)],
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
    fn a_null_key_is_matched_with_is_null() {
        let edit = RowEdit::Delete {
            key: vec![("id".into(), Value::Null)],
        };
        assert!(statement(SqlGrammar::Postgres, edit).ends_with("WHERE \"id\" IS NULL"));
    }
}
