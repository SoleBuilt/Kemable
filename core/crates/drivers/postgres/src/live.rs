//! `LISTEN` for the Live tab (`docs/decisions/0020-live-tab.md`): a session of
//! its own takes the channels and stays open; each `NOTIFY` the server sends
//! it becomes one row. Closing the session is what ends the listening on the
//! server, so a stop needs no `UNLISTEN`.

use std::future::poll_fn;
use std::time::{SystemTime, UNIX_EPOCH};

use paboable_core::catalog::ColumnMeta;
use paboable_core::config::ConnectionConfig;
use paboable_core::live::{LiveTopic, LiveTopicKind};
use paboable_core::sink::{ExecOutcome, ResultSink};
use paboable_core::sql::quote_ident;
use paboable_core::value::{Value, ValueKind};
use paboable_core::{CallOptions, DriverError, ErrorCode, Result};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use tokio_postgres::{AsyncMessage, Client, Config, NoTls, Notification};

use crate::connection::{SessionSettings, StatementMode};
use crate::errors::{self, Phase};
use crate::tls::Tls;

/// What the session's socket hands the listener.
enum Arrival {
    Notified(Notification),
    Failed(DriverError),
}

/// Stops the task that runs the session's socket when the listening ends,
/// however it ends.
struct Session(JoinHandle<()>);

impl Drop for Session {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(crate) async fn listen(
    config: &ConnectionConfig,
    topics: &[LiveTopic],
    sink: &dyn ResultSink,
    options: &CallOptions,
) -> Result<ExecOutcome> {
    let statement = listen_statement(topics)?;
    let SessionSettings { pg, tls, mode } = SessionSettings::read(config, options)?;
    options.check()?;

    let (arrived, mut arrivals) = unbounded_channel();
    let (client, task) = options.run(open(&pg, &tls, arrived)).await?;
    let _session = Session(task);
    if mode == StatementMode::Unnamed {
        // The pooler hands each transaction to any server session, so the one
        // that took LISTEN is not the one a later NOTIFY would reach.
        sink.notice(
            "A pooler in transaction mode does not keep LISTEN. \
             Connect to the database directly (port 5432) to receive notifications.",
        );
    }
    options
        .run(async {
            client
                .batch_execute(&statement)
                .await
                .map_err(|error| errors::map(&error, Some(&statement), Phase::Run))
        })
        .await?;

    sink.begin_set(columns());
    let outcome = loop {
        tokio::select! {
            biased;
            () = options.stopped() => break Err(options.stop_reason()),
            arrival = arrivals.recv() => match arrival {
                Some(Arrival::Notified(notification)) => {
                    if !sink.push_rows(&row(&notification)) {
                        break Ok(ExecOutcome { limited: true });
                    }
                }
                Some(Arrival::Failed(error)) => break Err(error),
                None => break Err(DriverError::new(ErrorCode::Lost, "the server closed the listening session")),
            },
        }
    };
    sink.end_set();
    outcome
}

/// `LISTEN` for every channel. A bare name folds to lower case, as it would
/// typed after `LISTEN` in psql; a quoted one is kept as written.
pub(crate) fn listen_statement(topics: &[LiveTopic]) -> Result<String> {
    if topics.is_empty() {
        return Err(DriverError::config("no channel to listen to"));
    }
    let mut statement = String::new();
    for topic in topics {
        if topic.kind != LiveTopicKind::Channel {
            return Err(DriverError::unsupported(
                "PostgreSQL listens to channels by name, not by pattern",
            ));
        }
        let name = if topic.exact {
            topic.name.clone()
        } else {
            topic.name.to_ascii_lowercase()
        };
        if !statement.is_empty() {
            statement.push_str("; ");
        }
        statement.push_str("LISTEN ");
        statement.push_str(&quote_ident(&name));
    }
    Ok(statement)
}

fn columns() -> Vec<ColumnMeta> {
    let column = |name: &str, type_name: &str, kind| ColumnMeta {
        name: name.to_owned(),
        type_name: type_name.to_owned(),
        kind,
    };
    vec![
        column("received", "timestamptz", ValueKind::TimestampTz),
        column("channel", "text", ValueKind::Text),
        column("payload", "text", ValueKind::Text),
        column("pid", "int4", ValueKind::Int),
    ]
}

/// One notification as a row: when it arrived here (the server sends no
/// time), its channel, its payload and the backend that sent it.
fn row(notification: &Notification) -> [Value; 4] {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_micros()).unwrap_or(i64::MAX));
    [
        Value::TimestampTz {
            micros,
            offset_seconds: 0,
        },
        Value::Text(notification.channel().to_owned()),
        Value::Text(notification.payload().to_owned()),
        Value::Int(i64::from(notification.process_id())),
    ]
}

async fn open(pg: &Config, tls: &Tls, arrived: UnboundedSender<Arrival>) -> Result<(Client, JoinHandle<()>)> {
    let map = |error: tokio_postgres::Error| errors::map(&error, None, Phase::Connect);
    Ok(match tls {
        Tls::Plain => {
            let (client, connection) = pg.connect(NoTls).await.map_err(map)?;
            (client, tokio::spawn(forward(connection, arrived)))
        }
        Tls::Rustls(maker) => {
            let (client, connection) = pg.connect(maker.clone()).await.map_err(map)?;
            (client, tokio::spawn(forward(connection, arrived)))
        }
    })
}

/// Runs the session's socket and hands on each notification; a socket that
/// fails says why once, then the channel closes.
async fn forward<S, T>(mut connection: tokio_postgres::Connection<S, T>, arrived: UnboundedSender<Arrival>)
where
    S: AsyncRead + AsyncWrite + Unpin,
    T: AsyncRead + AsyncWrite + Unpin,
{
    while let Some(message) = poll_fn(|cx| connection.poll_message(cx)).await {
        let arrival = match message {
            Ok(AsyncMessage::Notification(notification)) => Arrival::Notified(notification),
            Ok(_) => continue,
            Err(error) => Arrival::Failed(errors::map(&error, None, Phase::Run)),
        };
        let failed = matches!(arrival, Arrival::Failed(_));
        if arrived.send(arrival).is_err() || failed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use paboable_core::config::{Secret, Target, TlsMode};
    use paboable_core::driver::Driver;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc::UnboundedReceiver;
    use tokio::sync::oneshot;

    use super::*;
    use crate::PostgresDriver;

    fn topic(name: &str, exact: bool) -> LiveTopic {
        LiveTopic {
            kind: LiveTopicKind::Channel,
            name: name.to_owned(),
            exact,
        }
    }

    #[test]
    fn a_bare_channel_folds_to_lower_case_and_a_quoted_one_is_kept() {
        let statement = listen_statement(&[topic("Orders", false), topic("Job \"Queue\"", true)]).unwrap();
        assert_eq!(statement, r#"LISTEN "orders"; LISTEN "Job ""Queue""""#);
    }

    #[test]
    fn a_pattern_or_no_channel_is_refused_before_anything_connects() {
        let pattern = LiveTopic {
            kind: LiveTopicKind::Pattern,
            name: "o*".to_owned(),
            exact: false,
        };
        assert_eq!(listen_statement(&[pattern]).unwrap_err().code, ErrorCode::Unsupported);
        assert_eq!(listen_statement(&[]).unwrap_err().code, ErrorCode::ConfigInvalid);
    }

    fn message(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    fn notification(pid: i32, channel: &str, payload: &str) -> Vec<u8> {
        let mut body = pid.to_be_bytes().to_vec();
        body.extend_from_slice(channel.as_bytes());
        body.push(0);
        body.extend_from_slice(payload.as_bytes());
        body.push(0);
        message(b'A', &body)
    }

    async fn frame(socket: &mut TcpStream, tagged: bool) -> Option<(u8, Vec<u8>)> {
        let tag = if tagged { socket.read_u8().await.ok()? } else { 0 };
        let length = usize::try_from(socket.read_u32().await.ok()?).ok()?;
        let mut body = vec![0; length.checked_sub(4)?];
        socket.read_exact(&mut body).await.ok()?;
        Some((tag, body))
    }

    /// A PostgreSQL backend just big enough for a listener: it trusts the
    /// login, takes one query, sends `notifications`, then closes the socket
    /// or holds it open. The query it was sent comes back on the receiver.
    async fn backend(notifications: Vec<Vec<u8>>, then_close: bool) -> (u16, oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (asked, query) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            frame(&mut socket, false).await.unwrap();
            let mut ready = message(b'R', &0u32.to_be_bytes());
            ready.extend(message(b'K', &[0, 0, 0, 42, 0, 0, 0, 7]));
            ready.extend(message(b'Z', b"I"));
            socket.write_all(&ready).await.unwrap();

            let (tag, body) = frame(&mut socket, true).await.unwrap();
            assert_eq!(tag, b'Q');
            let text = String::from_utf8_lossy(&body).trim_end_matches('\0').to_owned();
            let mut reply = Vec::new();
            for _ in 0..text.matches("LISTEN").count() {
                reply.extend(message(b'C', b"LISTEN\0"));
            }
            reply.extend(message(b'Z', b"I"));
            for notification in notifications {
                reply.extend(notification);
            }
            socket.write_all(&reply).await.unwrap();
            let _ = asked.send(text);
            if !then_close {
                // Until the listener goes away.
                while frame(&mut socket, true).await.is_some() {}
            }
        });
        (port, query)
    }

    fn config(port: u16) -> ConnectionConfig {
        ConnectionConfig {
            engine: "postgres".into(),
            target: Target::Network {
                host: "127.0.0.1".into(),
                port: Some(port),
                user: Some("me".into()),
                password: Secret::default(),
                database: Some("app".into()),
                tls: TlsMode::Disable,
            },
            options: Default::default(),
        }
    }

    /// Keeps the rows it is given, up to `limit`, and says when each arrives.
    struct Heard {
        columns: Mutex<Vec<String>>,
        rows: Mutex<Vec<Vec<Value>>>,
        ended: Mutex<bool>,
        limit: usize,
        arrived: UnboundedSender<()>,
    }

    impl Heard {
        fn new(limit: usize) -> (Arc<Self>, UnboundedReceiver<()>) {
            let (arrived, arrivals) = unbounded_channel();
            let heard = Self {
                columns: Mutex::default(),
                rows: Mutex::default(),
                ended: Mutex::new(false),
                limit,
                arrived,
            };
            (Arc::new(heard), arrivals)
        }

        /// Each row without the time it arrived.
        fn rows(&self) -> Vec<Vec<Value>> {
            self.rows.lock().unwrap().iter().map(|row| row[1..].to_vec()).collect()
        }
    }

    impl ResultSink for Heard {
        fn begin_set(&self, columns: Vec<ColumnMeta>) {
            *self.columns.lock().unwrap() = columns.into_iter().map(|column| column.name).collect();
        }

        fn push_rows(&self, values: &[Value]) -> bool {
            let mut rows = self.rows.lock().unwrap();
            rows.push(values.to_vec());
            let _ = self.arrived.send(());
            rows.len() < self.limit
        }

        fn end_set(&self) {
            *self.ended.lock().unwrap() = true;
        }

        fn command_done(&self, _rows_affected: Option<u64>) {}

        fn notice(&self, _text: &str) {}
    }

    #[tokio::test]
    async fn a_listener_takes_its_channels_and_each_notification_is_a_row_until_it_is_stopped() {
        let (port, query) = backend(
            vec![
                notification(42, "orders", "{\"id\": 1}"),
                notification(43, "Job Queue", ""),
            ],
            false,
        )
        .await;
        let (heard, mut arrivals) = Heard::new(usize::MAX);
        let options = CallOptions::default();
        let stop = options.cancel.clone();
        let sink: Arc<dyn ResultSink> = heard.clone();
        let listening = tokio::spawn(async move {
            let topics = [topic("Orders", false), topic("Job Queue", true)];
            PostgresDriver.listen(&config(port), &topics, sink, options).await
        });

        arrivals.recv().await.unwrap();
        arrivals.recv().await.unwrap();
        assert_eq!(query.await.unwrap(), r#"LISTEN "orders"; LISTEN "Job Queue""#);
        assert_eq!(
            *heard.columns.lock().unwrap(),
            ["received", "channel", "payload", "pid"]
        );
        assert_eq!(
            heard.rows(),
            [
                [
                    Value::Text("orders".into()),
                    Value::Text("{\"id\": 1}".into()),
                    Value::Int(42)
                ],
                [
                    Value::Text("Job Queue".into()),
                    Value::Text(String::new()),
                    Value::Int(43)
                ],
            ]
        );
        assert!(matches!(heard.rows.lock().unwrap()[0][0], Value::TimestampTz { micros, .. } if micros > 0));

        stop.cancel();
        assert_eq!(listening.await.unwrap().unwrap_err().code, ErrorCode::Cancelled);
        assert!(*heard.ended.lock().unwrap());
    }

    #[tokio::test]
    async fn a_listener_stops_at_the_row_limit() {
        let (port, _query) = backend(
            vec![
                notification(1, "a", "1"),
                notification(1, "a", "2"),
                notification(1, "a", "3"),
            ],
            false,
        )
        .await;
        let (heard, _arrivals) = Heard::new(2);
        let outcome = PostgresDriver
            .listen(
                &config(port),
                &[topic("a", false)],
                heard.clone(),
                CallOptions::default(),
            )
            .await
            .unwrap();
        assert!(outcome.limited);
        assert_eq!(heard.rows().len(), 2);
        assert!(*heard.ended.lock().unwrap());
    }

    #[tokio::test]
    async fn a_listener_whose_server_goes_away_ends_as_lost_with_the_rows_it_had() {
        let (port, _query) = backend(vec![notification(1, "a", "last words")], true).await;
        let (heard, _arrivals) = Heard::new(usize::MAX);
        let error = PostgresDriver
            .listen(
                &config(port),
                &[topic("a", false)],
                heard.clone(),
                CallOptions::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Lost);
        assert_eq!(heard.rows().len(), 1);
        assert!(*heard.ended.lock().unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
    async fn a_real_server_delivers_a_notify_to_the_listener() {
        let url = std::env::var("PABOABLE_POSTGRES_URL").expect("PABOABLE_POSTGRES_URL is set");
        let config = paboable_core::connection_url::parse(&url).expect("a valid URL");
        let channel = format!("Paboable Live {}", std::process::id());
        let (heard, mut arrivals) = Heard::new(1);
        let sink: Arc<dyn ResultSink> = heard.clone();
        let listening = {
            let (config, topics) = (config.clone(), [topic(&channel, true)]);
            tokio::spawn(async move {
                PostgresDriver
                    .listen(&config, &topics, sink, CallOptions::default())
                    .await
            })
        };

        // A notification sent before the LISTEN is taken reaches nobody, so send until one arrives.
        let sender = PostgresDriver.connect(&config, CallOptions::default()).await.unwrap();
        let notify = format!("select pg_notify('{channel}', 'xin chào')");
        loop {
            let (discard, _) = Heard::new(usize::MAX);
            sender.execute(&notify, discard, CallOptions::default()).await.unwrap();
            let waited = tokio::time::timeout(std::time::Duration::from_millis(200), arrivals.recv()).await;
            if waited.is_ok() {
                break;
            }
        }
        assert!(listening.await.unwrap().unwrap().limited);
        assert_eq!(
            heard.rows()[0][..2],
            [Value::Text(channel), Value::Text("xin chào".into())]
        );
    }
}
