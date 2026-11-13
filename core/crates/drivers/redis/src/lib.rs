//! Redis (and servers that speak its protocol, such as Valkey and Upstash)
//! over a small RESP2 client of its own (`docs/decisions/0008-key-value-engines.md`).
//!
//! The console runs one command per line (`paboable_core::command`); keys are
//! found with `SCAN`, never `KEYS`, and a key is read in steps (`HSCAN`,
//! `SSCAN`, `LRANGE`, `ZRANGE`, `XRANGE` by 1,000) until the result's row cap.

mod link;
mod listen;
mod reply;
mod resp;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use paboable_core::catalog::{ColumnInfo, ColumnMeta, DatabaseInfo, ObjectInfo, SchemaInfo, TableRef};
use paboable_core::command::parse_args;
use paboable_core::config::{ConnectionConfig, Target, TlsMode};
use paboable_core::connection::Connection;
use paboable_core::driver::Driver;
use paboable_core::engine::{DataModel, EngineInfo, TargetKind};
use paboable_core::keys::{KeyDetail, KeyInfo, KeyKind, KeyPage, KeyScan};
use paboable_core::live::{LiveTopic, LiveTopicKind};
use paboable_core::sink::{ExecOutcome, ResultSink};
use paboable_core::value::{Value, ValueKind};
use paboable_core::{CallOptions, DriverError, ErrorCode, Result};
use rustls::ClientConfig;

use crate::link::{Endpoint, Link, query_error};
use crate::reply::Rows;
use crate::resp::Reply;

pub static INFO: EngineInfo = EngineInfo {
    id: "redis",
    display_name: "Redis",
    data_model: DataModel::KeyValue,
    targets: &[TargetKind::Network],
    default_port: Some(6379),
    sql_grammar: None,
    live_topics: &[LiveTopicKind::Channel, LiveTopicKind::Pattern],
    has_databases: true,
    tree_folders: &[],
};

/// How many elements one step of reading a key asks for.
const STEP: usize = 1000;

/// Commands that turn the connection into something else than request and
/// reply (a subscription, a replica stream) or change what it is logged in
/// as; the console refuses them.
const REFUSED: &[&str] = &[
    "SUBSCRIBE",
    "PSUBSCRIBE",
    "SSUBSCRIBE",
    "MONITOR",
    "SYNC",
    "PSYNC",
    "HELLO",
    "RESET",
    "QUIT",
    "AUTH",
];

pub struct RedisDriver;

#[async_trait]
impl Driver for RedisDriver {
    fn info(&self) -> &'static EngineInfo {
        &INFO
    }

    async fn connect(&self, config: &ConnectionConfig, options: CallOptions) -> Result<Arc<dyn Connection>> {
        let link = Link::open(endpoint(config)?, &options).await?;
        let version = match link.one(vec![b"INFO".to_vec(), b"server".to_vec()], &options).await? {
            Reply::Bulk(bytes) => info_field(&String::from_utf8_lossy(&bytes), "redis_version"),
            _ => None,
        };
        Ok(Arc::new(RedisConnection { link, version }))
    }

    async fn listen(
        &self,
        config: &ConnectionConfig,
        topics: &[LiveTopic],
        sink: Arc<dyn ResultSink>,
        options: CallOptions,
    ) -> Result<ExecOutcome> {
        listen::listen(&endpoint(config)?, topics, sink.as_ref(), &options, listen::KEEPALIVE).await
    }
}

/// Where a config's link goes and how it logs in.
fn endpoint(config: &ConnectionConfig) -> Result<Endpoint> {
    let Target::Network {
        host,
        port,
        user,
        password,
        database,
        tls,
    } = &config.target
    else {
        return Err(DriverError::config("Redis is reached over the network"));
    };
    if host.trim().is_empty() {
        return Err(DriverError::config("the host is empty"));
    }
    let database = match database.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
        None => 0,
        Some(name) => name
            .parse::<u32>()
            .map_err(|_| DriverError::config(format!("{name} is not a database number")))?,
    };
    Ok(Endpoint {
        host: host.trim().to_owned(),
        port: port.unwrap_or(6379),
        user: user.clone(),
        password: password.clone(),
        database,
        tls: tls_config(*tls)?,
    })
}

/// Redis has no TLS negotiation: a link is plain or TLS from its first byte.
/// TLS always checks the certificate against Windows' trusted roots and the host name.
fn tls_config(mode: TlsMode) -> Result<Option<Arc<ClientConfig>>> {
    if matches!(mode, TlsMode::Disable | TlsMode::Prefer) {
        return Ok(None);
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls_error = |error: rustls::Error| DriverError::new(ErrorCode::Tls, error.to_string());
    let verifier = rustls_platform_verifier::Verifier::new(provider.clone()).map_err(tls_error)?;
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(Some(Arc::new(config)))
}

fn info_field(info: &str, field: &str) -> Option<String> {
    info.lines()
        .find_map(|line| line.strip_prefix(field)?.strip_prefix(':'))
        .map(|value| value.trim().to_owned())
}

pub struct RedisConnection {
    link: Link,
    version: Option<String>,
}

fn command(words: &[&str]) -> Vec<Vec<u8>> {
    words.iter().map(|word| word.as_bytes().to_vec()).collect()
}

fn with_key(words: &[&str], key: &str, rest: &[&str]) -> Vec<Vec<u8>> {
    let mut out = command(words);
    out.push(key.as_bytes().to_vec());
    out.extend(command(rest));
    out
}

/// A reply that is an error becomes a failure; any other is returned.
fn ok(reply: Reply) -> Result<Reply> {
    match reply {
        Reply::Error(text) => Err(query_error(&text)),
        other => Ok(other),
    }
}

fn kind_of(type_name: &str) -> Option<KeyKind> {
    Some(match type_name {
        "string" => KeyKind::String,
        "list" => KeyKind::List,
        "set" => KeyKind::Set,
        "zset" => KeyKind::SortedSet,
        "hash" => KeyKind::Hash,
        "stream" => KeyKind::Stream,
        "none" => return None,
        _ => KeyKind::Other,
    })
}

fn type_name(kind: KeyKind) -> Option<&'static str> {
    Some(match kind {
        KeyKind::String => "string",
        KeyKind::List => "list",
        KeyKind::Set => "set",
        KeyKind::SortedSet => "zset",
        KeyKind::Hash => "hash",
        KeyKind::Stream => "stream",
        KeyKind::Other => return None,
    })
}

/// `PTTL`'s reply: -1 never expires, -2 is gone.
fn ttl(reply: &Reply) -> Option<i64> {
    reply.integer().filter(|ttl| *ttl >= 0)
}

fn column(name: &str, kind: ValueKind) -> ColumnMeta {
    ColumnMeta {
        name: name.to_owned(),
        type_name: String::new(),
        kind,
    }
}

/// A scan step's reply: the next cursor and the items.
fn scan_reply(reply: Reply) -> Result<(String, Vec<Reply>)> {
    match ok(reply)? {
        Reply::Array(parts) => {
            let mut parts = parts.into_iter();
            let cursor = parts
                .next()
                .and_then(|reply| reply.text())
                .unwrap_or_else(|| "0".to_owned());
            let items = match parts.next() {
                Some(Reply::Array(items)) => items,
                _ => Vec::new(),
            };
            Ok((cursor, items))
        }
        _ => Err(DriverError::new(ErrorCode::Io, "a scan reply that is not an array")),
    }
}

/// The id right after `id` (`ms-seq`), to read a stream on from it on any Redis version.
fn next_stream_id(id: &str) -> Option<String> {
    let (ms, seq) = id.split_once('-')?;
    let (ms, seq) = (ms.parse::<u64>().ok()?, seq.parse::<u64>().ok()?);
    Some(match seq.checked_add(1) {
        Some(seq) => format!("{ms}-{seq}"),
        None => format!("{}-0", ms.checked_add(1)?),
    })
}

impl RedisConnection {
    /// Reads a collection in steps until it ends or the sink takes no more.
    async fn read_steps(
        &self,
        key: &str,
        kind: KeyKind,
        sink: &Arc<dyn ResultSink>,
        options: &CallOptions,
    ) -> Result<bool> {
        let mut rows = match kind {
            KeyKind::Hash => Rows::begin(
                sink,
                vec![column("field", ValueKind::Text), column("value", ValueKind::Text)],
            ),
            KeyKind::List => Rows::begin(
                sink,
                vec![column("index", ValueKind::Int), column("value", ValueKind::Text)],
            ),
            KeyKind::Set => Rows::begin(sink, vec![column("member", ValueKind::Text)]),
            KeyKind::SortedSet => Rows::begin(
                sink,
                vec![column("member", ValueKind::Text), column("score", ValueKind::Float)],
            ),
            KeyKind::Stream => Rows::begin(
                sink,
                vec![column("id", ValueKind::Text), column("fields", ValueKind::Json)],
            ),
            KeyKind::String | KeyKind::Other => return Ok(false),
        };
        let step = STEP.to_string();
        match kind {
            KeyKind::Hash | KeyKind::Set => {
                let scan = if kind == KeyKind::Hash { "HSCAN" } else { "SSCAN" };
                let mut cursor = "0".to_owned();
                loop {
                    let reply = self
                        .link
                        .one(with_key(&[scan], key, &[&cursor, "COUNT", &step]), options)
                        .await?;
                    let (next, items) = scan_reply(reply)?;
                    let more = if kind == KeyKind::Hash {
                        reply::push_pairs(&mut rows, items, false)
                    } else {
                        items.into_iter().all(|item| rows.push([reply::scalar(item)]))
                    };
                    if !more || next == "0" {
                        break;
                    }
                    cursor = next;
                }
            }
            KeyKind::List | KeyKind::SortedSet => {
                let mut start = 0usize;
                loop {
                    let stop = (start + STEP - 1).to_string();
                    let first = start.to_string();
                    let read = if kind == KeyKind::List {
                        with_key(&["LRANGE"], key, &[&first, &stop])
                    } else {
                        with_key(&["ZRANGE"], key, &[&first, &stop, "WITHSCORES"])
                    };
                    let Reply::Array(items) = ok(self.link.one(read, options).await?)? else {
                        break;
                    };
                    let count = if kind == KeyKind::List {
                        items.len()
                    } else {
                        items.len() / 2
                    };
                    let more = if kind == KeyKind::List {
                        items.into_iter().enumerate().all(|(offset, item)| {
                            let index = i64::try_from(start + offset).unwrap_or(i64::MAX);
                            rows.push([Value::Int(index), reply::scalar(item)])
                        })
                    } else {
                        reply::push_pairs(&mut rows, items, true)
                    };
                    if !more || count < STEP {
                        break;
                    }
                    start += STEP;
                }
            }
            KeyKind::Stream => {
                let mut from = "-".to_owned();
                loop {
                    let read = with_key(&["XRANGE"], key, &[&from, "+", "COUNT", &step]);
                    let Reply::Array(items) = ok(self.link.one(read, options).await?)? else {
                        break;
                    };
                    let count = items.len();
                    let last = reply::push_entries(&mut rows, items);
                    if rows.limited || count < STEP {
                        break;
                    }
                    match last.as_deref().and_then(next_stream_id) {
                        Some(next) => from = next,
                        None => break,
                    }
                }
            }
            KeyKind::String | KeyKind::Other => {}
        }
        Ok(rows.end().limited)
    }
}

#[async_trait]
impl Connection for RedisConnection {
    fn engine(&self) -> &'static EngineInfo {
        &INFO
    }

    fn server_version(&self) -> Option<String> {
        self.version.clone()
    }

    fn current_database(&self) -> Option<String> {
        Some(self.link.database().to_string())
    }

    async fn ping(&self, options: CallOptions) -> Result<()> {
        ok(self.link.one(command(&["PING"]), &options).await?).map(drop)
    }

    /// Database 0 up to the server's `databases` setting; 16, Redis's
    /// default, when the login may not read it.
    async fn list_databases(&self, options: CallOptions) -> Result<Vec<DatabaseInfo>> {
        let count = match self
            .link
            .one(command(&["CONFIG", "GET", "databases"]), &options)
            .await?
        {
            Reply::Array(items) => items
                .get(1)
                .and_then(Reply::integer)
                .filter(|count| (1..=10_000).contains(count)),
            _ => None,
        }
        .unwrap_or(16);
        let current = i64::from(self.link.database());
        Ok((0..count)
            .map(|index| DatabaseInfo {
                name: index.to_string(),
                is_current: index == current,
            })
            .collect())
    }

    async fn list_schemas(&self, _options: CallOptions) -> Result<Vec<SchemaInfo>> {
        Ok(Vec::new())
    }

    async fn list_objects(&self, _schema: Option<&str>, _options: CallOptions) -> Result<Vec<ObjectInfo>> {
        Ok(Vec::new())
    }

    async fn fetch_columns(&self, _table: &TableRef, _options: CallOptions) -> Result<Vec<ColumnInfo>> {
        Err(DriverError::unsupported("Redis has no tables"))
    }

    async fn execute(&self, statement: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<ExecOutcome> {
        let args = parse_args(statement)?;
        let Some(name) = args
            .first()
            .map(|word| String::from_utf8_lossy(word).to_ascii_uppercase())
        else {
            return Ok(ExecOutcome::default());
        };
        if name == "SELECT" {
            return Err(DriverError::unsupported(
                "SELECT switches the database: put it on a line of its own",
            ));
        }
        if REFUSED.contains(&name.as_str()) {
            return Err(DriverError::unsupported(format!("{name} is not run from the console")));
        }
        let reply = ok(self.link.one(args.clone(), &options).await?)?;
        Ok(reply::emit(&args, reply, &sink))
    }

    async fn scan_keys(&self, scan: &KeyScan, options: CallOptions) -> Result<KeyPage> {
        let cursor = scan.cursor.clone().unwrap_or_else(|| "0".to_owned());
        let pattern = if scan.pattern.is_empty() {
            "*"
        } else {
            scan.pattern.as_str()
        };
        let count = scan.count.clamp(1, 100_000).to_string();
        let mut words = vec!["SCAN", cursor.as_str(), "MATCH", pattern, "COUNT", count.as_str()];
        if let Some(name) = scan.kind.and_then(type_name) {
            words.extend(["TYPE", name]);
        }
        let (next, items) = scan_reply(self.link.one(command(&words), &options).await?)?;
        let names: Vec<Vec<u8>> = items
            .into_iter()
            .filter_map(|item| match item {
                Reply::Bulk(bytes) => Some(bytes),
                _ => None,
            })
            .collect();
        // One round trip for every key's type and time to live.
        let lookups: Vec<Vec<Vec<u8>>> = names
            .iter()
            .flat_map(|name| {
                [
                    vec![b"TYPE".to_vec(), name.clone()],
                    vec![b"PTTL".to_vec(), name.clone()],
                ]
            })
            .collect();
        let replies = if lookups.is_empty() {
            Vec::new()
        } else {
            self.link.call(&lookups, &options).await?
        };
        let mut keys = Vec::with_capacity(names.len());
        for (name, pair) in names.into_iter().zip(replies.chunks(2)) {
            // A key that expired between the scan and the lookup is left out.
            let Some(kind) = pair.first().and_then(Reply::text).as_deref().and_then(kind_of) else {
                continue;
            };
            keys.push(KeyInfo {
                name: String::from_utf8_lossy(&name).into_owned(),
                kind,
                ttl_ms: pair.get(1).and_then(ttl),
            });
        }
        Ok(KeyPage {
            keys,
            cursor: (next != "0").then_some(next),
        })
    }

    async fn read_key(&self, key: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<KeyDetail> {
        let replies = self
            .link
            .call(
                &[with_key(&["TYPE"], key, &[]), with_key(&["PTTL"], key, &[])],
                &options,
            )
            .await?;
        let type_text = replies.first().and_then(Reply::text).unwrap_or_default();
        let kind = kind_of(&type_text).ok_or_else(|| DriverError::not_found(format!("key {key}")))?;
        let ttl_ms = replies.get(1).and_then(ttl);
        let length_command = match kind {
            KeyKind::String => "STRLEN",
            KeyKind::List => "LLEN",
            KeyKind::Set => "SCARD",
            KeyKind::SortedSet => "ZCARD",
            KeyKind::Hash => "HLEN",
            KeyKind::Stream => "XLEN",
            KeyKind::Other => {
                sink.notice(&format!("{type_text} values are read with that module's own commands."));
                return Ok(KeyDetail {
                    kind,
                    ttl_ms,
                    length: 0,
                });
            }
        };
        let length = ok(self.link.one(with_key(&[length_command], key, &[]), &options).await?)?
            .integer()
            .and_then(|length| u64::try_from(length).ok())
            .unwrap_or(0);
        if kind == KeyKind::String {
            let value = ok(self.link.one(with_key(&["GET"], key, &[]), &options).await?)?;
            let value = reply::scalar(value);
            sink.begin_set(vec![column("value", value.kind())]);
            sink.push_rows(&[value]);
            sink.end_set();
        } else {
            self.read_steps(key, kind, &sink, &options).await?;
        }
        Ok(KeyDetail { kind, ttl_ms, length })
    }

    async fn close(&self) -> Result<()> {
        self.link.close().await;
        Ok(())
    }
}
