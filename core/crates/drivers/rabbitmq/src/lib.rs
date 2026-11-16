//! RabbitMQ over AMQP 0-9-1, with lapin on tokio and rustls with ring
//! (`docs/decisions/0026-rabbitmq.md`).
//!
//! A connection opens one AMQP connection and checks the login. Its console
//! runs one command per line (`kemable_core::command`): `publish <exchange>
//! <routing key> <body>`, to send what a Live tab hears. The Live tab binds a
//! queue of its own to exchanges (a copy of what they route), or consumes
//! queues by name (what it takes, the queue no longer holds); each message is
//! one row.

mod values;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures_util::StreamExt;
use futures_util::stream::select_all;
use kemable_core::catalog::{ColumnInfo, ColumnMeta, ObjectInfo, SchemaInfo, TableRef};
use kemable_core::command::parse_args;
use kemable_core::config::{ConnectionConfig, Target, TlsMode};
use kemable_core::connection::Connection;
use kemable_core::driver::Driver;
use kemable_core::engine::{DataModel, EngineInfo, TargetKind};
use kemable_core::live::{LiveTopic, LiveTopicKind};
use kemable_core::sink::{ExecOutcome, ResultSink};
use kemable_core::value::{Value, ValueKind};
use kemable_core::{CallOptions, DriverError, ErrorCode, Result};
use lapin::Confirmation;
use lapin::message::Delivery;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, BasicQosOptions, ConfirmSelectOptions, QueueBindOptions,
    QueueDeclareOptions,
};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Channel, ConnectionProperties};

pub static INFO: EngineInfo = EngineInfo {
    id: "rabbitmq",
    display_name: "RabbitMQ",
    data_model: DataModel::Queue,
    targets: &[TargetKind::Network],
    default_port: Some(5672),
    sql_grammar: None,
    live_topics: &[LiveTopicKind::Binding, LiveTopicKind::Queue],
    has_databases: false,
    tree_folders: &[],
};

/// The port AMQP over TLS listens on by default.
const TLS_PORT: u16 = 5671;
/// How many messages a consuming Live tab takes ahead of the ones it stored.
const PREFETCH: u16 = 200;
/// The heartbeat asked for, in seconds: a dead link is noticed within two.
const HEARTBEAT: u16 = 30;

pub struct RabbitmqDriver;

/// Encodes `text` for a part of an AMQP URI: everything but letters, digits
/// and `-._~` as `%XX`.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The AMQP URI for a config: `amqp://` or `amqps://`, the login, the host
/// (or an SSH tunnel's local end), and the virtual host (`/` when none).
pub(crate) fn uri(config: &ConnectionConfig) -> Result<String> {
    let Target::Network {
        host,
        port,
        user,
        password,
        database,
        tls,
    } = &config.target
    else {
        return Err(DriverError::config("RabbitMQ is reached over the network"));
    };
    let host = host.trim();
    if host.is_empty() {
        return Err(DriverError::config("the host is empty"));
    }
    let secure = *tls != TlsMode::Disable;
    let (host, port) = match config.dial {
        // TLS would check the certificate against 127.0.0.1, which names no server.
        Some(_) if secure => {
            return Err(DriverError::config("RabbitMQ through SSH connects without TLS"));
        }
        Some(dial) => (dial.ip().to_string(), dial.port()),
        None => (host.to_owned(), port.unwrap_or(if secure { TLS_PORT } else { 5672 })),
    };
    let user = user.as_deref().filter(|user| !user.is_empty()).unwrap_or("guest");
    let password = if password.is_empty() && user == "guest" {
        "guest"
    } else {
        password.expose()
    };
    let vhost = database
        .as_deref()
        .map(str::trim)
        .filter(|vhost| !vhost.is_empty())
        .unwrap_or("/");
    let scheme = if secure { "amqps" } else { "amqp" };
    Ok(format!(
        "{scheme}://{}:{}@{host}:{port}/{}?heartbeat={HEARTBEAT}",
        encode(user),
        encode(password),
        encode(vhost)
    ))
}

/// A lapin error as a code: a refused login, a broker's refusal with its
/// reply code, or the link gone.
pub(crate) fn map(error: &lapin::Error, connecting: bool) -> DriverError {
    use lapin::ErrorKind;
    match error.kind() {
        ErrorKind::ProtocolError(amqp) => {
            let code = amqp.get_id();
            let kind = match code {
                // ACCESS_REFUSED, and NOT_ALLOWED for a virtual host the user may not open.
                403 | 530 if connecting => ErrorCode::Auth,
                _ => ErrorCode::Query,
            };
            DriverError::new(kind, amqp.get_message().as_str().to_owned()).with_engine_code(code.to_string())
        }
        ErrorKind::AuthProviderError(text) => DriverError::new(ErrorCode::Auth, text.clone()),
        ErrorKind::IOError(_) | ErrorKind::MissingHeartbeatError | ErrorKind::RuntimeShutdownError(_) => {
            DriverError::new(
                if connecting {
                    ErrorCode::Connect
                } else {
                    ErrorCode::Lost
                },
                error.to_string(),
            )
        }
        _ if connecting => DriverError::new(ErrorCode::Connect, error.to_string()),
        _ => DriverError::new(ErrorCode::Query, error.to_string()),
    }
}

async fn open(config: &ConnectionConfig, options: &CallOptions) -> Result<lapin::Connection> {
    let uri = uri(config)?;
    options.check()?;
    options
        .run(async {
            lapin::Connection::connect(
                &uri,
                ConnectionProperties::default().with_connection_name("Kemable".into()),
            )
            .await
            .map_err(|error| map(&error, true))
        })
        .await
}

pub struct RabbitmqConnection {
    /// What opened it, to open it again once the broker or the network closed it.
    config: ConnectionConfig,
    link: tokio::sync::Mutex<Link>,
}

/// One AMQP connection and the channel the console publishes on.
struct Link {
    connection: lapin::Connection,
    channel: Channel,
}

/// A channel to publish on, in confirm mode: the broker says whether it took
/// each message, so a publish to an exchange that is not there fails instead
/// of reporting done.
async fn publishing_channel(
    connection: &lapin::Connection,
    options: &CallOptions,
    connecting: bool,
) -> Result<Channel> {
    options
        .run(async {
            let channel = connection
                .create_channel()
                .await
                .map_err(|error| map(&error, connecting))?;
            channel
                .confirm_select(ConfirmSelectOptions::default())
                .await
                .map_err(|error| map(&error, connecting))?;
            Ok(channel)
        })
        .await
}

#[async_trait]
impl Driver for RabbitmqDriver {
    fn info(&self) -> &'static EngineInfo {
        &INFO
    }

    async fn connect(&self, config: &ConnectionConfig, options: CallOptions) -> Result<Arc<dyn Connection>> {
        let connection = open(config, &options).await?;
        let channel = publishing_channel(&connection, &options, true).await?;
        Ok(Arc::new(RabbitmqConnection {
            config: config.clone(),
            link: tokio::sync::Mutex::new(Link { connection, channel }),
        }))
    }

    async fn listen(
        &self,
        config: &ConnectionConfig,
        topics: &[LiveTopic],
        sink: Arc<dyn ResultSink>,
        options: CallOptions,
    ) -> Result<ExecOutcome> {
        listen(config, topics, sink.as_ref(), &options).await
    }
}

/// `exchange` or `exchange:key`; the key is `#` (everything, on a topic
/// exchange) when none is given.
pub(crate) fn binding(name: &str) -> (&str, &str) {
    match name.split_once(':') {
        Some((exchange, key)) if !key.is_empty() => (exchange, key),
        Some((exchange, _)) => (exchange, "#"),
        None => (name, "#"),
    }
}

fn columns() -> Vec<ColumnMeta> {
    let column = |name: &str, type_name: &str, kind| ColumnMeta {
        name: name.to_owned(),
        type_name: type_name.to_owned(),
        kind,
    };
    vec![
        column("received", "timestamptz", ValueKind::TimestampTz),
        column("exchange", "text", ValueKind::Text),
        column("routing_key", "text", ValueKind::Text),
        column("body", "text", ValueKind::Text),
        column("properties", "json", ValueKind::Json),
        column("redelivered", "bool", ValueKind::Bool),
    ]
}

/// One message as a row; a body that is not UTF-8 stays bytes.
pub(crate) fn row(delivery: &Delivery) -> [Value; 6] {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_micros()).unwrap_or(i64::MAX));
    let body = match std::str::from_utf8(&delivery.data) {
        Ok(text) => Value::Text(text.to_owned()),
        Err(_) => Value::Bytes(delivery.data.clone()),
    };
    [
        Value::TimestampTz {
            micros,
            offset_seconds: 0,
        },
        Value::Text(delivery.exchange.as_str().to_owned()),
        Value::Text(delivery.routing_key.as_str().to_owned()),
        body,
        values::properties(&delivery.properties),
        Value::Bool(delivery.redelivered),
    ]
}

async fn listen(
    config: &ConnectionConfig,
    topics: &[LiveTopic],
    sink: &dyn ResultSink,
    options: &CallOptions,
) -> Result<ExecOutcome> {
    if topics.is_empty() {
        return Err(DriverError::config("no exchange or queue to listen to"));
    }
    if topics
        .iter()
        .any(|topic| !matches!(topic.kind, LiveTopicKind::Binding | LiveTopicKind::Queue))
    {
        return Err(DriverError::unsupported("RabbitMQ listens to exchanges and queues"));
    }
    let connection = open(config, options).await?;
    let outcome = consume(&connection, topics, sink, options).await;
    // Closing the connection deletes the listener's own queue and cancels its consumers.
    let _ = connection.close(200, "Kemable stopped listening".into()).await;
    outcome
}

async fn consume(
    connection: &lapin::Connection,
    topics: &[LiveTopic],
    sink: &dyn ResultSink,
    options: &CallOptions,
) -> Result<ExecOutcome> {
    let refused = |error: lapin::Error| map(&error, false);
    let channel = options
        .run(async { connection.create_channel().await.map_err(refused) })
        .await?;
    let mut consumers = Vec::new();
    let bindings: Vec<&LiveTopic> = topics
        .iter()
        .filter(|topic| topic.kind == LiveTopicKind::Binding)
        .collect();
    let mut takes = false;
    options
        .run(async {
            channel
                .basic_qos(PREFETCH, BasicQosOptions::default())
                .await
                .map_err(refused)?;
            if !bindings.is_empty() {
                // A queue of the listener's own: the server names it, and drops it when the link goes.
                let own = channel
                    .queue_declare(
                        "".into(),
                        QueueDeclareOptions {
                            exclusive: true,
                            auto_delete: true,
                            ..QueueDeclareOptions::default()
                        },
                        FieldTable::default(),
                    )
                    .await
                    .map_err(refused)?;
                for topic in &bindings {
                    let (exchange, key) = binding(&topic.name);
                    channel
                        .queue_bind(
                            own.name().clone(),
                            exchange.into(),
                            key.into(),
                            QueueBindOptions::default(),
                            FieldTable::default(),
                        )
                        .await
                        .map_err(refused)?;
                }
                let copies = channel
                    .basic_consume(
                        own.name().clone(),
                        "kemable-live".into(),
                        BasicConsumeOptions {
                            no_ack: true,
                            ..BasicConsumeOptions::default()
                        },
                        FieldTable::default(),
                    )
                    .await
                    .map_err(refused)?;
                consumers.push(copies.map(|delivery| (false, delivery)).boxed());
            }
            for topic in topics.iter().filter(|topic| topic.kind == LiveTopicKind::Queue) {
                // Passive: a queue that is not there is an error, never made here.
                channel
                    .queue_declare(
                        topic.name.as_str().into(),
                        QueueDeclareOptions {
                            passive: true,
                            ..QueueDeclareOptions::default()
                        },
                        FieldTable::default(),
                    )
                    .await
                    .map_err(refused)?;
                let taken = channel
                    .basic_consume(
                        topic.name.as_str().into(),
                        format!("kemable-live-{}", consumers.len()).into(),
                        BasicConsumeOptions::default(),
                        FieldTable::default(),
                    )
                    .await
                    .map_err(refused)?;
                consumers.push(taken.map(|delivery| (true, delivery)).boxed());
                takes = true;
            }
            Ok(())
        })
        .await?;
    if takes {
        sink.notice("Messages taken from a queue are acknowledged: the queue no longer holds them.");
    }

    sink.begin_set(columns());
    let mut deliveries = select_all(consumers);
    let outcome = loop {
        tokio::select! {
            biased;
            () = options.stopped() => break Err(options.stop_reason()),
            next = deliveries.next() => match next {
                None => break Err(DriverError::new(ErrorCode::Lost, "the server ended the consumers")),
                Some((_, Err(error))) => break Err(map(&error, false)),
                Some((acknowledge, Ok(delivery))) => {
                    let more = sink.push_rows(&row(&delivery));
                    // Stored first, then acknowledged: a message is never lost between the two.
                    if acknowledge {
                        let _ = delivery.ack(BasicAckOptions::default()).await;
                    }
                    if !more {
                        break Ok(ExecOutcome { limited: true });
                    }
                }
            },
        }
    };
    sink.end_set();
    outcome
}

impl RabbitmqConnection {
    /// The channel to publish on. A connection the network or the broker
    /// closed (after sleep, a restart) is opened again, and a channel the
    /// broker closed (a refused publish) is made again on its connection.
    async fn channel(&self, options: &CallOptions) -> Result<Channel> {
        let mut link = self.link.lock().await;
        if !link.connection.status().connected() {
            let connection = open(&self.config, options).await?;
            let channel = publishing_channel(&connection, options, true).await?;
            *link = Link { connection, channel };
        } else if !link.channel.status().connected() {
            link.channel = publishing_channel(&link.connection, options, false).await?;
        }
        Ok(link.channel.clone())
    }

    /// `publish <exchange> <routing key> <body>`: the body is the rest of the
    /// words, joined by spaces, or one quoted word; `""` is the default exchange.
    /// Resolves once the broker confirmed it, to whether a queue took it: a
    /// message no binding routes is returned (it is published `mandatory`).
    async fn publish(&self, words: &[Vec<u8>], options: &CallOptions) -> Result<bool> {
        let [_, exchange, key, body @ ..] = words else {
            return Err(DriverError::config("publish <exchange> <routing key> <body>"));
        };
        let exchange = String::from_utf8_lossy(exchange);
        let key = String::from_utf8_lossy(key);
        let body = body.join(&b' ');
        let channel = self.channel(options).await?;
        let confirm = options
            .run(async {
                channel
                    .basic_publish(
                        exchange.as_ref().into(),
                        key.as_ref().into(),
                        BasicPublishOptions {
                            mandatory: true,
                            ..BasicPublishOptions::default()
                        },
                        &body,
                        BasicProperties::default().with_content_type("text/plain".into()),
                    )
                    .await
                    .map_err(|error| map(&error, false))
            })
            .await?;
        // An exchange that is not there closes the channel (404): the confirm
        // fails with the broker's words, and the next publish opens a channel again.
        let confirmation = options
            .run(async { confirm.await.map_err(|error| map(&error, false)) })
            .await?;
        match confirmation {
            Confirmation::Ack(None) | Confirmation::NotRequested => Ok(true),
            Confirmation::Ack(Some(_)) => Ok(false),
            Confirmation::Nack(_) => Err(DriverError::new(
                ErrorCode::Query,
                "the broker refused the message (basic.nack)",
            )),
        }
    }
}

#[async_trait]
impl Connection for RabbitmqConnection {
    fn engine(&self) -> &'static EngineInfo {
        &INFO
    }

    /// lapin keeps the broker's own properties to itself.
    fn server_version(&self) -> Option<String> {
        None
    }

    async fn ping(&self, options: CallOptions) -> Result<()> {
        self.channel(&options).await.map(drop)
    }

    async fn list_schemas(&self, _options: CallOptions) -> Result<Vec<SchemaInfo>> {
        Ok(Vec::new())
    }

    async fn list_objects(&self, _schema: Option<&str>, _options: CallOptions) -> Result<Vec<ObjectInfo>> {
        Ok(Vec::new())
    }

    async fn fetch_columns(&self, _table: &TableRef, _options: CallOptions) -> Result<Vec<ColumnInfo>> {
        Err(DriverError::unsupported("RabbitMQ has no tables"))
    }

    async fn execute(&self, statement: &str, sink: Arc<dyn ResultSink>, options: CallOptions) -> Result<ExecOutcome> {
        let words = parse_args(statement)?;
        let Some(command) = words
            .first()
            .map(|word| String::from_utf8_lossy(word).to_ascii_lowercase())
        else {
            return Ok(ExecOutcome::default());
        };
        match command.as_str() {
            "publish" => {
                if !self.publish(&words, &options).await? {
                    sink.notice("The broker returned the message: no queue is bound to take it.");
                }
                sink.command_done(None);
                Ok(ExecOutcome::default())
            }
            _ => Err(DriverError::unsupported(format!(
                "{command}: the console knows publish <exchange> <routing key> <body>"
            ))),
        }
    }

    async fn close(&self) -> Result<()> {
        let link = self.link.lock().await;
        if !link.connection.status().connected() {
            return Ok(());
        }
        link.connection
            .close(200, "Kemable closed the connection".into())
            .await
            .map_err(|error| map(&error, false))
    }
}
