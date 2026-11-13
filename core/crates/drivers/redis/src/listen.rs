//! Pub/sub for the Live tab (`docs/decisions/0020-live-tab.md`): a connection
//! of its own sends `SUBSCRIBE` and `PSUBSCRIBE` and then only reads; each
//! message the server pushes becomes one row. Closing the connection is what
//! ends the subscriptions on the server.
//!
//! A subscriber that hears nothing cannot tell a quiet channel from a dead
//! link, and a cloud load balancer drops a link left idle for a few minutes.
//! So the listener sends `PING` on a timer, which Redis answers in this mode
//! too; a `PING` left unanswered until the next one ends the listening as lost.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kemable_core::catalog::ColumnMeta;
use kemable_core::live::{LiveTopic, LiveTopicKind};
use kemable_core::sink::{ExecOutcome, ResultSink};
use kemable_core::value::{Value, ValueKind};
use kemable_core::{CallOptions, DriverError, ErrorCode, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::time::{Instant, MissedTickBehavior, interval_at};

use crate::link::{self, Endpoint, Stream, lost, query_error, read_error};
use crate::reply::bytes_value;
use crate::resp::{self, Reply};

/// How often a listener asks the server whether the link is still up.
pub(crate) const KEEPALIVE: Duration = Duration::from_secs(30);

enum Event {
    Stopped,
    /// The socket has bytes to read, or is at its end (`false`).
    Data(std::io::Result<bool>),
    Ping,
}

pub(crate) async fn listen(
    endpoint: &Endpoint,
    topics: &[LiveTopic],
    sink: &dyn ResultSink,
    options: &CallOptions,
    keepalive: Duration,
) -> Result<ExecOutcome> {
    let names = |kind: LiveTopicKind| -> Vec<&str> {
        topics
            .iter()
            .filter(|topic| topic.kind == kind)
            .map(|topic| topic.name.as_str())
            .collect()
    };
    let channels = names(LiveTopicKind::Channel);
    let patterns = names(LiveTopicKind::Pattern);
    if channels.is_empty() && patterns.is_empty() {
        return Err(DriverError::config("no channel or pattern to listen to"));
    }

    let mut stream = options.run(link::connect(endpoint)).await?;
    options.run(subscribe(&mut stream, &channels, &patterns, sink)).await?;

    // Rows wait here until the server has taken every topic: only then does
    // the set begin, so a caller that sees it begin has missed nothing after.
    let mut waiting = channels.len() + patterns.len();
    let mut early: Vec<[Value; 4]> = Vec::new();
    let mut pings = interval_at(Instant::now() + keepalive, keepalive);
    // A late tick (a busy machine, a laptop waking up) must not be followed at
    // once by the next: that one would find the PING just sent still unanswered
    // and end a healthy link as lost. Each PING gets a whole `keepalive` to come back.
    pings.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut unanswered = false;
    let outcome = loop {
        let event = tokio::select! {
            biased;
            () = options.stopped() => Event::Stopped,
            filled = stream.fill_buf() => Event::Data(filled.map(|buffer| !buffer.is_empty())),
            _ = pings.tick() => Event::Ping,
        };
        match event {
            Event::Stopped => break Err(options.stop_reason()),
            Event::Ping if unanswered => {
                break Err(DriverError::new(ErrorCode::Lost, "the server did not answer PING"));
            }
            Event::Ping => {
                unanswered = true;
                if let Err(error) = send(&mut stream, &[&["PING"]]).await {
                    break Err(error);
                }
            }
            Event::Data(Err(error)) => break Err(lost(error)),
            Event::Data(Ok(false)) => break Err(DriverError::new(ErrorCode::Lost, "the server closed the connection")),
            Event::Data(Ok(true)) => {
                // Anything the server sends shows the link is up.
                unanswered = false;
                let reply = match options
                    .run(async { resp::read(&mut stream).await.map_err(read_error) })
                    .await
                {
                    Ok(reply) => reply,
                    Err(error) => break Err(error),
                };
                match pushed(reply) {
                    Pushed::Message(row) if waiting > 0 => early.push(row),
                    Pushed::Message(row) => {
                        if !sink.push_rows(&row) {
                            break Ok(ExecOutcome { limited: true });
                        }
                    }
                    Pushed::Subscribed if waiting > 0 => {
                        waiting -= 1;
                        if waiting == 0 {
                            sink.begin_set(columns());
                            if !early.drain(..).all(|row| sink.push_rows(&row)) {
                                break Ok(ExecOutcome { limited: true });
                            }
                        }
                    }
                    Pushed::Refused(text) => break Err(query_error(&text)),
                    Pushed::Subscribed | Pushed::Other => {}
                }
            }
        }
    };
    if waiting == 0 {
        sink.end_set();
    }
    outcome
}

/// Asks for every topic. Key events come only when the server is set to
/// send them, so a topic that names them checks the setting first, while the
/// connection still takes commands.
async fn subscribe(stream: &mut Stream, channels: &[&str], patterns: &[&str], sink: &dyn ResultSink) -> Result<()> {
    if channels.iter().chain(patterns).any(|name| name.starts_with("__key")) {
        send(stream, &[&["CONFIG", "GET", "notify-keyspace-events"]]).await?;
        let setting = match resp::read(stream).await.map_err(read_error)? {
            Reply::Array(items) => items.get(1).and_then(Reply::text),
            // A managed server may refuse CONFIG: then nothing is known.
            _ => None,
        };
        if setting.as_deref() == Some("") {
            sink.notice(
                "notify-keyspace-events is empty, so this server sends no key events. \
                 Turn them on with: CONFIG SET notify-keyspace-events KEA",
            );
        }
    }
    let mut commands: Vec<Vec<&str>> = Vec::new();
    for (command, names) in [("SUBSCRIBE", channels), ("PSUBSCRIBE", patterns)] {
        if !names.is_empty() {
            commands.push(std::iter::once(command).chain(names.iter().copied()).collect());
        }
    }
    let commands: Vec<&[&str]> = commands.iter().map(Vec::as_slice).collect();
    send(stream, &commands).await
}

async fn send(stream: &mut Stream, commands: &[&[&str]]) -> Result<()> {
    for command in commands {
        stream.write_all(&resp::encode(command)).await.map_err(lost)?;
    }
    stream.flush().await.map_err(lost)
}

/// What a subscribed connection can be sent.
enum Pushed {
    /// A message, as a row.
    Message([Value; 4]),
    /// The server took one channel or pattern.
    Subscribed,
    /// An error reply, such as `NOPERM` for a channel the login may not read.
    Refused(String),
    /// A `PING`'s answer, or a kind this client does not ask for.
    Other,
}

fn pushed(reply: Reply) -> Pushed {
    let mut items = match reply {
        Reply::Array(items) => items.into_iter(),
        Reply::Error(text) => return Pushed::Refused(text),
        _ => return Pushed::Other,
    };
    let kind = items.next().and_then(|kind| kind.text()).unwrap_or_default();
    let mut text = || match items.next() {
        Some(Reply::Bulk(bytes)) => bytes_value(bytes),
        _ => Value::Null,
    };
    match kind.as_str() {
        "message" => {
            let channel = text();
            Pushed::Message([received(), channel, Value::Null, text()])
        }
        "pmessage" => {
            let pattern = text();
            let channel = text();
            Pushed::Message([received(), channel, pattern, text()])
        }
        "subscribe" | "psubscribe" => Pushed::Subscribed,
        _ => Pushed::Other,
    }
}

/// When a message arrived here; Redis sends no time with it.
fn received() -> Value {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_micros()).unwrap_or(i64::MAX));
    Value::TimestampTz {
        micros,
        offset_seconds: 0,
    }
}

fn columns() -> Vec<ColumnMeta> {
    let column = |name: &str, kind| ColumnMeta {
        name: name.to_owned(),
        type_name: String::new(),
        kind,
    };
    vec![
        column("received", ValueKind::TimestampTz),
        column("channel", ValueKind::Text),
        column("pattern", ValueKind::Text),
        column("message", ValueKind::Text),
    ]
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use kemable_core::config::Secret;
    use tokio::io::BufReader;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

    use super::*;

    fn bulk(bytes: &[u8]) -> Reply {
        Reply::Bulk(bytes.to_vec())
    }

    fn encode_reply(reply: &Reply, out: &mut Vec<u8>) {
        match reply {
            Reply::Status(text) => out.extend(format!("+{text}\r\n").into_bytes()),
            Reply::Error(text) => out.extend(format!("-{text}\r\n").into_bytes()),
            Reply::Integer(value) => out.extend(format!(":{value}\r\n").into_bytes()),
            Reply::Nil => out.extend(b"$-1\r\n"),
            Reply::Bulk(bytes) => {
                out.extend(format!("${}\r\n", bytes.len()).into_bytes());
                out.extend(bytes);
                out.extend(b"\r\n");
            }
            Reply::Array(items) => {
                out.extend(format!("*{}\r\n", items.len()).into_bytes());
                for item in items {
                    encode_reply(item, out);
                }
            }
        }
    }

    /// What the server in the test does once a client has subscribed.
    #[derive(Default)]
    struct Script {
        /// Sent after the last subscribe command is confirmed.
        messages: Vec<Reply>,
        /// Sent in place of confirming a subscribe command.
        refuse: Option<&'static str>,
        /// Close the socket after the messages.
        then_close: bool,
        /// Answer `PING`; a server that does not is a dead link.
        answer_pings: bool,
        /// `notify-keyspace-events`, as `CONFIG GET` reports it.
        key_events: &'static str,
    }

    /// A pub/sub server: it confirms each channel and pattern, plays its
    /// script, and reports every command it was sent.
    async fn serve(script: Script, commands_expected: usize) -> (Endpoint, UnboundedReceiver<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen, commands) = unbounded_channel();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (read, mut write) = socket.into_split();
            let mut read = BufReader::new(read);
            let mut subscribed = 0;
            let mut taken = 0;
            while let Ok(Reply::Array(words)) = resp::read(&mut read).await {
                let words: Vec<String> = words.iter().map(|word| word.text().unwrap_or_default()).collect();
                let _ = seen.send(words.clone());
                let mut out = Vec::new();
                match words[0].as_str() {
                    "CONFIG" => encode_reply(
                        &Reply::Array(vec![
                            bulk(b"notify-keyspace-events"),
                            bulk(script.key_events.as_bytes()),
                        ]),
                        &mut out,
                    ),
                    "PING" if script.answer_pings => {
                        encode_reply(&Reply::Array(vec![bulk(b"pong"), bulk(b"")]), &mut out);
                    }
                    "PING" => {}
                    kind @ ("SUBSCRIBE" | "PSUBSCRIBE") => {
                        taken += 1;
                        if let Some(refusal) = script.refuse {
                            encode_reply(&Reply::Error(refusal.to_owned()), &mut out);
                        } else {
                            for name in &words[1..] {
                                subscribed += 1;
                                let confirmed = vec![
                                    bulk(kind.to_ascii_lowercase().as_bytes()),
                                    bulk(name.as_bytes()),
                                    Reply::Integer(subscribed),
                                ];
                                encode_reply(&Reply::Array(confirmed), &mut out);
                            }
                        }
                        if taken == commands_expected {
                            for message in &script.messages {
                                encode_reply(message, &mut out);
                            }
                        }
                    }
                    other => panic!("a listener sent {other}"),
                }
                if write.write_all(&out).await.is_err() {
                    return;
                }
                if taken == commands_expected && script.then_close {
                    return;
                }
            }
        });
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port,
            user: None,
            password: Secret::default(),
            database: 0,
            tls: None,
        };
        (endpoint, commands)
    }

    fn message(channel: &str, payload: &[u8]) -> Reply {
        Reply::Array(vec![bulk(b"message"), bulk(channel.as_bytes()), bulk(payload)])
    }

    fn topic(kind: LiveTopicKind, name: &str) -> LiveTopic {
        LiveTopic {
            kind,
            name: name.to_owned(),
            exact: false,
        }
    }

    fn channel(name: &str) -> LiveTopic {
        topic(LiveTopicKind::Channel, name)
    }

    /// Keeps the rows it is given, up to `limit`, and says when each arrives.
    struct Heard {
        columns: Mutex<Vec<String>>,
        rows: Mutex<Vec<Vec<Value>>>,
        notices: Mutex<Vec<String>>,
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
                notices: Mutex::default(),
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
            assert!(!self.columns.lock().unwrap().is_empty(), "a row before the set began");
            let mut rows = self.rows.lock().unwrap();
            rows.push(values.to_vec());
            let _ = self.arrived.send(());
            rows.len() < self.limit
        }

        fn end_set(&self) {
            *self.ended.lock().unwrap() = true;
        }

        fn command_done(&self, _rows_affected: Option<u64>) {}

        fn notice(&self, text: &str) {
            self.notices.lock().unwrap().push(text.to_owned());
        }
    }

    fn text(value: &str) -> Value {
        Value::Text(value.to_owned())
    }

    #[tokio::test]
    async fn channels_and_patterns_are_taken_and_each_message_is_a_row_until_it_is_stopped() {
        let script = Script {
            messages: vec![
                message("news", "xin chào".as_bytes()),
                Reply::Array(vec![
                    bulk(b"pmessage"),
                    bulk(b"user:*"),
                    bulk(b"user:7"),
                    bulk(&[0xff, 0x00]),
                ]),
            ],
            ..Script::default()
        };
        let (endpoint, mut commands) = serve(script, 2).await;
        let (heard, mut arrivals) = Heard::new(usize::MAX);
        let options = CallOptions::default();
        let stop = options.cancel.clone();
        let sink = heard.clone();
        let listening = tokio::spawn(async move {
            let topics = [
                channel("news"),
                channel("jobs"),
                topic(LiveTopicKind::Pattern, "user:*"),
            ];
            listen(&endpoint, &topics, sink.as_ref(), &options, KEEPALIVE).await
        });

        arrivals.recv().await.unwrap();
        arrivals.recv().await.unwrap();
        assert_eq!(commands.recv().await.unwrap(), ["SUBSCRIBE", "news", "jobs"]);
        assert_eq!(commands.recv().await.unwrap(), ["PSUBSCRIBE", "user:*"]);
        assert_eq!(
            *heard.columns.lock().unwrap(),
            ["received", "channel", "pattern", "message"]
        );
        assert_eq!(
            heard.rows(),
            [
                [text("news"), Value::Null, text("xin chào")],
                [text("user:7"), text("user:*"), Value::Bytes(vec![0xff, 0x00])],
            ]
        );

        stop.cancel();
        assert_eq!(listening.await.unwrap().unwrap_err().code, ErrorCode::Cancelled);
        assert!(*heard.ended.lock().unwrap());
    }

    #[tokio::test]
    async fn a_listener_stops_at_the_row_limit() {
        let script = Script {
            messages: vec![message("a", b"1"), message("a", b"2"), message("a", b"3")],
            ..Script::default()
        };
        let (endpoint, _commands) = serve(script, 1).await;
        let (heard, _arrivals) = Heard::new(2);
        let outcome = listen(
            &endpoint,
            &[channel("a")],
            heard.as_ref(),
            &CallOptions::default(),
            KEEPALIVE,
        )
        .await
        .unwrap();
        assert!(outcome.limited);
        assert_eq!(heard.rows().len(), 2);
        assert!(*heard.ended.lock().unwrap());
    }

    #[tokio::test]
    async fn a_listener_whose_server_goes_away_ends_as_lost_with_the_rows_it_had() {
        let script = Script {
            messages: vec![message("a", b"last words")],
            then_close: true,
            ..Script::default()
        };
        let (endpoint, _commands) = serve(script, 1).await;
        let (heard, _arrivals) = Heard::new(usize::MAX);
        let error = listen(
            &endpoint,
            &[channel("a")],
            heard.as_ref(),
            &CallOptions::default(),
            KEEPALIVE,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Lost);
        assert_eq!(heard.rows(), [[text("a"), Value::Null, text("last words")]]);
        assert!(*heard.ended.lock().unwrap());
    }

    #[tokio::test]
    async fn a_channel_the_login_may_not_read_fails_with_the_servers_word() {
        let script = Script {
            refuse: Some("NOPERM this user has no permissions to access one of the channels used as arguments"),
            ..Script::default()
        };
        let (endpoint, _commands) = serve(script, 1).await;
        let (heard, _arrivals) = Heard::new(usize::MAX);
        let error = listen(
            &endpoint,
            &[channel("secret")],
            heard.as_ref(),
            &CallOptions::default(),
            KEEPALIVE,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Auth);
        assert_eq!(error.engine_code.as_deref(), Some("NOPERM"));
        assert!(
            heard.columns.lock().unwrap().is_empty(),
            "no set begins for a listener that never listened"
        );
        assert!(!*heard.ended.lock().unwrap());
    }

    #[tokio::test]
    async fn no_topic_is_refused_before_anything_connects() {
        let endpoint = Endpoint {
            host: "256.0.0.1".into(),
            port: 1,
            user: None,
            password: Secret::default(),
            database: 0,
            tls: None,
        };
        let (heard, _arrivals) = Heard::new(usize::MAX);
        let error = listen(&endpoint, &[], heard.as_ref(), &CallOptions::default(), KEEPALIVE)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ConfigInvalid);
    }

    #[tokio::test]
    async fn a_quiet_listener_pings_and_an_answered_ping_keeps_it_listening() {
        let script = Script {
            answer_pings: true,
            ..Script::default()
        };
        let (endpoint, mut commands) = serve(script, 1).await;
        let (heard, _arrivals) = Heard::new(usize::MAX);
        let options = CallOptions::default();
        let stop = options.cancel.clone();
        let sink = heard.clone();
        let listening = tokio::spawn(async move {
            listen(
                &endpoint,
                &[channel("quiet")],
                sink.as_ref(),
                &options,
                Duration::from_millis(20),
            )
            .await
        });

        assert_eq!(commands.recv().await.unwrap(), ["SUBSCRIBE", "quiet"]);
        // A third ping is sent only if the first two were answered.
        for _ in 0..3 {
            assert_eq!(commands.recv().await.unwrap(), ["PING"]);
        }
        stop.cancel();
        assert_eq!(listening.await.unwrap().unwrap_err().code, ErrorCode::Cancelled);
    }

    #[tokio::test]
    async fn a_ping_left_unanswered_ends_the_listening_as_lost() {
        let (endpoint, _commands) = serve(Script::default(), 1).await;
        let (heard, _arrivals) = Heard::new(usize::MAX);
        let error = listen(
            &endpoint,
            &[channel("quiet")],
            heard.as_ref(),
            &CallOptions::default(),
            Duration::from_millis(20),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Lost);
        assert!(*heard.ended.lock().unwrap());
    }

    #[tokio::test]
    async fn key_events_say_so_when_the_server_is_not_set_to_send_them() {
        for (key_events, noticed) in [("", true), ("KEA", false)] {
            let script = Script {
                key_events,
                then_close: true,
                ..Script::default()
            };
            let (endpoint, mut commands) = serve(script, 1).await;
            let (heard, _arrivals) = Heard::new(usize::MAX);
            let topics = [topic(LiveTopicKind::Pattern, "__keyspace@0__:*")];
            let _ = listen(&endpoint, &topics, heard.as_ref(), &CallOptions::default(), KEEPALIVE).await;
            assert_eq!(
                commands.recv().await.unwrap(),
                ["CONFIG", "GET", "notify-keyspace-events"]
            );
            assert_eq!(
                heard.notices.lock().unwrap().len(),
                usize::from(noticed),
                "{key_events:?}"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs Redis at $KEMABLE_REDIS_URL"]
    async fn a_real_server_delivers_a_publish_to_the_listener() {
        use kemable_core::driver::Driver;

        use crate::RedisDriver;

        let url = std::env::var("KEMABLE_REDIS_URL").expect("KEMABLE_REDIS_URL is set");
        let config = kemable_core::connection_url::parse(&url).expect("a valid URL");
        let name = format!("kemable:live:{}", std::process::id());
        let (heard, mut arrivals) = Heard::new(1);
        let sink: Arc<dyn ResultSink> = heard.clone();
        let listening = {
            let (config, topics) = (config.clone(), [channel(&name)]);
            tokio::spawn(async move { RedisDriver.listen(&config, &topics, sink, CallOptions::default()).await })
        };

        // A message published before the subscription is taken reaches nobody, so publish until one arrives.
        let publisher = RedisDriver.connect(&config, CallOptions::default()).await.unwrap();
        let publish = format!("PUBLISH {name} \"xin chào\"");
        loop {
            let (discard, _) = Heard::new(usize::MAX);
            publisher
                .execute(&publish, discard, CallOptions::default())
                .await
                .unwrap();
            let waited = tokio::time::timeout(Duration::from_millis(200), arrivals.recv()).await;
            if waited.is_ok() {
                break;
            }
        }
        assert!(listening.await.unwrap().unwrap().limited);
        assert_eq!(heard.rows(), [[text(&name), Value::Null, text("xin chào")]]);
    }
}
