//! Firebase: Firestore's collections and documents and the Realtime
//! Database, through their REST APIs, signed in as a service account
//! (`docs/decisions/0027-firebase.md`).
//!
//! The tree shows Firestore's collections and the Realtime Database's
//! children of its root. The console (`console`) reads: `get` a collection,
//! a document or a query, `rtdb` a path. A document is a row: its id, one
//! column per field the first page has (by name, as Firestore sends them),
//! the rest as JSON, and when it changed. A Live tab listens to the Realtime
//! Database's paths (`live`).

mod api;
mod auth;
mod console;
mod live;
mod values;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use kemable_core::catalog::{ColumnInfo, ColumnMeta, ObjectInfo, ObjectKind, SchemaInfo, TableRef};
use kemable_core::command::parse_args;
use kemable_core::config::ConnectionConfig;
use kemable_core::connection::Connection;
use kemable_core::driver::Driver;
use kemable_core::engine::{DataModel, EngineInfo, TargetKind, TreeFolder};
use kemable_core::live::{LiveTopic, LiveTopicKind};
use kemable_core::sink::{ExecOutcome, PUSH_BATCH_ROWS, ResultSink};
use kemable_core::value::{Value, ValueKind};
use kemable_core::{CallOptions, DriverError, Result};
use serde_json::{Map, Value as Json};

use crate::api::Firebase;
use crate::console::Command;

/// The tree's two schemas.
pub const FIRESTORE: &str = "Firestore";
pub const REALTIME: &str = "Realtime Database";

pub static INFO: EngineInfo = EngineInfo {
    id: "firebase",
    display_name: "Firebase",
    data_model: DataModel::Document,
    targets: &[TargetKind::Http],
    default_port: None,
    sql_grammar: None,
    live_topics: &[LiveTopicKind::Path],
    has_databases: false,
    tree_folders: &[
        TreeFolder {
            label: "Collections",
            kinds: &[ObjectKind::Collection],
        },
        TreeFolder {
            label: "Children",
            kinds: &[ObjectKind::Key],
        },
    ],
};

pub struct FirebaseDriver;

pub struct FirebaseConnection {
    firebase: Firebase,
}

#[async_trait]
impl Driver for FirebaseDriver {
    fn info(&self) -> &'static EngineInfo {
        &INFO
    }

    async fn connect(&self, config: &ConnectionConfig, options: CallOptions) -> Result<Arc<dyn Connection>> {
        let firebase = Firebase::new(config)?;
        options.run(firebase.sign_in()).await?;
        Ok(Arc::new(FirebaseConnection { firebase }))
    }

    /// The Realtime Database's paths, each a stream of server-sent events (`live`).
    async fn listen(
        &self,
        config: &ConnectionConfig,
        topics: &[LiveTopic],
        sink: Arc<dyn ResultSink>,
        options: CallOptions,
    ) -> Result<ExecOutcome> {
        let firebase = Firebase::new(config)?;
        live::listen(&firebase, topics, sink.as_ref(), &options).await
    }
}

/// The columns of a page of documents: their id, each field the page has
/// (in the order first seen), the fields only later documents have (JSON),
/// and when each last changed.
struct DocumentColumns {
    fields: Vec<String>,
}

impl DocumentColumns {
    fn of(documents: &[Json]) -> Self {
        let mut fields: Vec<String> = Vec::new();
        for document in documents {
            for name in document
                .get("fields")
                .and_then(Json::as_object)
                .into_iter()
                .flatten()
                .map(|(name, _)| name)
            {
                if !fields.contains(name) {
                    fields.push(name.clone());
                }
            }
        }
        Self { fields }
    }

    fn meta(&self, documents: &[Json]) -> Vec<ColumnMeta> {
        let column = |name: &str, kind| ColumnMeta {
            name: name.to_owned(),
            type_name: String::new(),
            kind,
        };
        let mut columns = vec![column("id", ValueKind::Text)];
        for field in &self.fields {
            // The kind of the first document that has it.
            let kind = documents
                .iter()
                .find_map(|document| {
                    document.pointer(&format!("/fields/{}", field.replace('~', "~0").replace('/', "~1")))
                })
                .map_or(ValueKind::Json, |typed| values::kind_of(&values::value(typed)));
            columns.push(column(field, kind));
        }
        columns.push(column("other_fields", ValueKind::Json));
        columns.push(column("update_time", ValueKind::TimestampTz));
        columns
    }

    fn row(&self, document: &Json, into: &mut Vec<Value>) {
        let name = document.get("name").and_then(Json::as_str).unwrap_or_default();
        into.push(Value::Text(name.rsplit('/').next().unwrap_or_default().to_owned()));
        let fields = document.get("fields").and_then(Json::as_object);
        for field in &self.fields {
            into.push(
                fields
                    .and_then(|fields| fields.get(field))
                    .map_or(Value::Null, values::value),
            );
        }
        let others: Map<String, Json> = fields
            .into_iter()
            .flatten()
            .filter(|(name, _)| !self.fields.contains(name))
            .map(|(name, typed)| (name.clone(), values::plain(typed)))
            .collect();
        into.push(if others.is_empty() {
            Value::Null
        } else {
            Value::Json(Json::Object(others).to_string())
        });
        into.push(
            document
                .get("updateTime")
                .and_then(Json::as_str)
                .and_then(values::rfc3339_micros)
                .map_or(Value::Null, |micros| Value::TimestampTz {
                    micros,
                    offset_seconds: 0,
                }),
        );
    }
}

/// Pushes documents in batches; false once the sink is full.
fn push_documents(columns: &DocumentColumns, documents: &[Json], sink: &dyn ResultSink) -> bool {
    for batch in documents.chunks(PUSH_BATCH_ROWS) {
        let mut cells = Vec::new();
        for document in batch {
            columns.row(document, &mut cells);
        }
        if !sink.push_rows(&cells) {
            return false;
        }
    }
    true
}

/// How many times its text's size a JSON value takes once parsed, about.
const PARSED_GROWTH: u64 = 4;

impl FirebaseConnection {
    async fn get(
        &self,
        path: &str,
        query: Option<&Json>,
        sink: &dyn ResultSink,
        options: &CallOptions,
    ) -> Result<ExecOutcome> {
        let firebase = &self.firebase;
        if path.split('/').count().is_multiple_of(2) {
            let document = options.run(firebase.document(path)).await?;
            let documents = [document];
            let columns = DocumentColumns::of(&documents);
            sink.begin_set(columns.meta(&documents));
            push_documents(&columns, &documents, sink);
            sink.end_set();
            return Ok(ExecOutcome::default());
        }
        if let Some(query) = query {
            let documents = options.run(firebase.run_query(path, query)).await?;
            let columns = DocumentColumns::of(&documents);
            sink.begin_set(columns.meta(&documents));
            let limited = !push_documents(&columns, &documents, sink);
            sink.end_set();
            return Ok(ExecOutcome { limited });
        }
        // Page after page until the collection ends or the result is full.
        let (first, mut next) = options.run(firebase.documents(path, None)).await?;
        let columns = DocumentColumns::of(&first);
        sink.begin_set(columns.meta(&first));
        let mut limited = !push_documents(&columns, &first, sink);
        while !limited && let Some(token) = next.take() {
            let (documents, after) = options.run(firebase.documents(path, Some(&token))).await?;
            limited = !push_documents(&columns, &documents, sink);
            next = after;
        }
        sink.end_set();
        Ok(ExecOutcome { limited })
    }

    /// A path of the Realtime Database: each child a row (its key, then its
    /// fields when the children are objects, else its value), or one row
    /// for a value that has no children.
    async fn rtdb(
        &self,
        path: &str,
        limit: Option<u64>,
        sink: &dyn ResultSink,
        options: &CallOptions,
    ) -> Result<ExecOutcome> {
        // A read with no limit is one JSON value, held whole before its first
        // row: its text is bounded so the parsed value stays inside the
        // sink's budget, which takes several times the text's size.
        let max_bytes = match limit {
            Some(_) => None,
            None => sink.byte_limit().map(|budget| budget / PARSED_GROWTH),
        };
        let value = options.run(self.firebase.rtdb(path, false, limit, max_bytes)).await?;
        let column = |name: &str, kind| ColumnMeta {
            name: name.to_owned(),
            type_name: String::new(),
            kind,
        };
        let Json::Object(children) = value else {
            let cell = values::rtdb_value(&value);
            sink.begin_set(vec![column("value", values::kind_of(&cell))]);
            sink.push_rows(&[cell]);
            sink.end_set();
            return Ok(ExecOutcome::default());
        };
        let mut fields: Vec<String> = Vec::new();
        let mut objects = true;
        for child in children.values() {
            match child {
                Json::Object(inner) => {
                    for name in inner.keys() {
                        if !fields.contains(name) {
                            fields.push(name.clone());
                        }
                    }
                }
                _ => objects = false,
            }
        }
        let mut columns = vec![column("key", ValueKind::Text)];
        if objects {
            columns.extend(fields.iter().map(|name| column(name, ValueKind::Json)));
        } else {
            fields.clear();
            columns.push(column("value", ValueKind::Json));
        }
        sink.begin_set(columns);
        let mut limited = false;
        let entries: Vec<(&String, &Json)> = children.iter().collect();
        for batch in entries.chunks(PUSH_BATCH_ROWS) {
            let mut cells = Vec::new();
            for (key, child) in batch {
                cells.push(Value::Text((*key).clone()));
                if objects {
                    for field in &fields {
                        cells.push(child.get(field).map_or(Value::Null, values::rtdb_value));
                    }
                } else {
                    cells.push(values::rtdb_value(child));
                }
            }
            if !sink.push_rows(&cells) {
                limited = true;
                break;
            }
        }
        sink.end_set();
        Ok(ExecOutcome { limited })
    }
}

#[async_trait]
impl Connection for FirebaseConnection {
    fn engine(&self) -> &'static EngineInfo {
        &INFO
    }

    fn server_version(&self) -> Option<String> {
        Some(self.firebase.project.clone())
    }

    async fn ping(&self, options: CallOptions) -> Result<()> {
        options.run(self.firebase.sign_in()).await
    }

    async fn list_schemas(&self, _options: CallOptions) -> Result<Vec<SchemaInfo>> {
        Ok(vec![
            SchemaInfo {
                name: FIRESTORE.to_owned(),
                is_default: true,
            },
            SchemaInfo {
                name: REALTIME.to_owned(),
                is_default: false,
            },
        ])
    }

    async fn list_objects(&self, schema: Option<&str>, options: CallOptions) -> Result<Vec<ObjectInfo>> {
        match schema.unwrap_or(FIRESTORE) {
            REALTIME => {
                let root = options.run(self.firebase.rtdb("", true, None, None)).await?;
                let keys: BTreeMap<String, Json> = match root {
                    Json::Object(children) => children.into_iter().collect(),
                    _ => BTreeMap::new(),
                };
                Ok(keys
                    .into_keys()
                    .map(|name| ObjectInfo {
                        schema: Some(REALTIME.to_owned()),
                        name,
                        kind: ObjectKind::Key,
                    })
                    .collect())
            }
            _ => {
                let ids = options.run(self.firebase.collection_ids(None)).await?;
                Ok(ids
                    .into_iter()
                    .map(|name| ObjectInfo {
                        schema: Some(FIRESTORE.to_owned()),
                        name,
                        kind: ObjectKind::Collection,
                    })
                    .collect())
            }
        }
    }

    async fn fetch_columns(&self, _table: &TableRef, _options: CallOptions) -> Result<Vec<ColumnInfo>> {
        Err(DriverError::unsupported("a document has fields, not columns"))
    }

    async fn execute(&self, statement: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<ExecOutcome> {
        let words = parse_args(statement)?;
        if words.is_empty() {
            return Ok(ExecOutcome::default());
        }
        match console::parse(&words)? {
            Command::Get { path, query } => self.get(&path, query.as_ref(), sink.as_ref(), &options).await,
            Command::Rtdb { path, limit } => self.rtdb(&path, limit, sink.as_ref(), &options).await,
        }
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}
