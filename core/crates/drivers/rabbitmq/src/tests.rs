//! What runs without a broker, and the live tests that wait for one at
//! `$KEMABLE_RABBITMQ_URL` (`amqp://user:password@host:5672/vhost`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kemable_core::catalog::ColumnMeta;
use kemable_core::config::{ConnectionConfig, Secret, Target, TlsMode};
use kemable_core::driver::Driver;
use kemable_core::live::{LiveTopicKind, topics_of};
use kemable_core::sink::ResultSink;
use kemable_core::value::Value;
use kemable_core::{CallOptions, ErrorCode};
use lapin::BasicProperties;
use lapin::message::Delivery;
use lapin::types::{AMQPValue, DecimalValue, FieldArray, FieldTable, LongString};

use super::{RabbitmqDriver, binding, row, uri, values};

fn config(
    host: &str,
    port: Option<u16>,
    user: &str,
    password: &str,
    vhost: Option<&str>,
    tls: TlsMode,
) -> ConnectionConfig {
    ConnectionConfig {
        engine: "rabbitmq".into(),
        target: Target::Network {
            host: host.into(),
            port,
            user: Some(user.into()),
            password: Secret::new(password),
            database: vhost.map(str::to_owned),
            tls,
        },
        options: BTreeMap::new(),
        ssh: None,
        dial: None,
    }
}

#[test]
fn a_config_becomes_an_amqp_uri_with_its_login_and_virtual_host_encoded() {
    assert_eq!(
        uri(&config(
            "mq.local",
            None,
            "app",
            "p@ss/word",
            Some("orders"),
            TlsMode::Disable
        ))
        .unwrap(),
        "amqp://app:p%40ss%2Fword@mq.local:5672/orders?heartbeat=30"
    );
    // TLS: amqps on its own port; the default virtual host is /.
    assert_eq!(
        uri(&config("mq.local", None, "app", "pw", None, TlsMode::VerifyFull)).unwrap(),
        "amqps://app:pw@mq.local:5671/%2F?heartbeat=30"
    );
    // No user: RabbitMQ's guest, with its password when none is typed.
    let mut guest = config("localhost", Some(5673), "", "", None, TlsMode::Disable);
    assert_eq!(
        uri(&guest).unwrap(),
        "amqp://guest:guest@localhost:5673/%2F?heartbeat=30"
    );

    // Through an SSH tunnel: its local end, and no TLS.
    guest.dial = Some(std::net::SocketAddr::from(([127, 0, 0, 1], 41000)));
    assert_eq!(
        uri(&guest).unwrap(),
        "amqp://guest:guest@127.0.0.1:41000/%2F?heartbeat=30"
    );
    let mut secure = config("mq", None, "a", "b", None, TlsMode::VerifyFull);
    secure.dial = guest.dial;
    assert_eq!(uri(&secure).unwrap_err().code, ErrorCode::ConfigInvalid);

    assert_eq!(
        uri(&config(" ", None, "a", "b", None, TlsMode::Disable))
            .unwrap_err()
            .code,
        ErrorCode::ConfigInvalid
    );
}

#[test]
fn a_binding_is_an_exchange_and_a_key_everything_when_none_is_given() {
    assert_eq!(binding("amq.topic:orders.*"), ("amq.topic", "orders.*"));
    assert_eq!(binding("events"), ("events", "#"));
    assert_eq!(binding("events:"), ("events", "#"));
    assert_eq!(binding("a:b:c"), ("a", "b:c"));
    let topics = topics_of(&[
        (LiveTopicKind::Binding, "amq.topic:orders.# events"),
        (LiveTopicKind::Queue, "jobs"),
    ]);
    assert_eq!(topics.len(), 3);
    assert_eq!(topics[2].kind, LiveTopicKind::Queue);
}

#[test]
fn a_message_is_a_row_with_its_body_as_text_or_bytes_and_its_properties_as_json() {
    let mut headers = FieldTable::default();
    headers.insert("attempt".into(), AMQPValue::LongInt(2));
    headers.insert(
        "tags".into(),
        AMQPValue::FieldArray(FieldArray::from(vec![AMQPValue::LongString(LongString::from("a\"b"))])),
    );
    headers.insert(
        "price".into(),
        AMQPValue::DecimalValue(DecimalValue { scale: 2, value: 1999 }),
    );
    headers.insert(
        "small".into(),
        AMQPValue::DecimalValue(DecimalValue { scale: 3, value: 5 }),
    );
    let mut delivery = Delivery::mock(
        1,
        "amq.topic".into(),
        "orders.new".into(),
        true,
        "đơn hàng #1".as_bytes().to_vec(),
    );
    delivery.properties = BasicProperties::default()
        .with_content_type("application/json".into())
        .with_message_id("m-1".into())
        .with_delivery_mode(2)
        .with_headers(headers);
    let values = row(&delivery);
    assert_eq!(values[1], Value::Text("amq.topic".into()));
    assert_eq!(values[2], Value::Text("orders.new".into()));
    assert_eq!(values[3], Value::Text("đơn hàng #1".into()));
    assert_eq!(
        values[4],
        Value::Json(
            r#"{"content_type":"application/json","message_id":"m-1","delivery_mode":2,"headers":{"attempt":2,"price":"19.99","small":"0.005","tags":["a\"b"]}}"#
                .into()
        )
    );
    assert_eq!(values[5], Value::Bool(true));

    let binary = Delivery::mock(2, "".into(), "q".into(), false, vec![0xff, 0x00]);
    let values = row(&binary);
    assert_eq!(values[3], Value::Bytes(vec![0xff, 0x00]));
    assert_eq!(values[4], Value::Null, "no property set");
    assert_eq!(values::json(&AMQPValue::Double(f64::NAN)), "null");
}

#[tokio::test]
async fn a_closed_port_is_a_connect_error_and_a_file_target_a_config_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let refused = RabbitmqDriver
        .connect(
            &config("127.0.0.1", Some(port), "guest", "guest", None, TlsMode::Disable),
            CallOptions::default().with_timeout(Duration::from_secs(10)),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(refused.code, ErrorCode::Connect);

    let mut file = config("x", None, "a", "b", None, TlsMode::Disable);
    file.target = Target::File {
        path: "x".into(),
        read_only: false,
    };
    let error = RabbitmqDriver
        .connect(&file, CallOptions::default())
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::ConfigInvalid);
}

// --- Against a real broker ---------------------------------------------------------

fn live_config() -> ConnectionConfig {
    let url = std::env::var("KEMABLE_RABBITMQ_URL").expect("KEMABLE_RABBITMQ_URL is set");
    kemable_core::connection_url::parse(&url).expect("an amqp:// URL")
}

#[derive(Default)]
struct Heard {
    rows: Mutex<Vec<Vec<Value>>>,
    began: Mutex<bool>,
}

impl ResultSink for Heard {
    fn begin_set(&self, _columns: Vec<ColumnMeta>) {
        *self.began.lock().unwrap() = true;
    }

    fn push_rows(&self, values: &[Value]) -> bool {
        let mut rows = self.rows.lock().unwrap();
        rows.push(values.to_vec());
        rows.len() < 2
    }

    fn end_set(&self) {}

    fn command_done(&self, _rows_affected: Option<u64>) {}

    fn notice(&self, _text: &str) {}
}

#[tokio::test]
#[ignore = "needs RabbitMQ at $KEMABLE_RABBITMQ_URL"]
async fn a_live_binding_hears_what_the_console_publishes() {
    let config = live_config();
    let heard = Arc::new(Heard::default());
    let listening = {
        let heard = Arc::clone(&heard);
        let config = config.clone();
        tokio::spawn(async move {
            let topics = topics_of(&[(LiveTopicKind::Binding, "amq.topic:kemable.test.#")]);
            RabbitmqDriver
                .listen(&config, &topics, heard, CallOptions::default())
                .await
        })
    };
    while !*heard.began.lock().unwrap() {
        tokio::task::yield_now().await;
    }
    let console = RabbitmqDriver.connect(&config, CallOptions::default()).await.unwrap();
    let sink = Arc::new(Heard::default());
    for body in ["một", "two"] {
        console
            .execute(
                &format!("publish amq.topic kemable.test.x {body}"),
                sink.clone(),
                CallOptions::default(),
            )
            .await
            .unwrap();
    }
    let outcome = listening.await.unwrap().unwrap();
    assert!(outcome.limited);
    let rows = heard.rows.lock().unwrap().clone();
    assert_eq!(rows[0][3], Value::Text("một".into()));
    assert_eq!(rows[1][2], Value::Text("kemable.test.x".into()));
    console.close().await.unwrap();
}

#[tokio::test]
#[ignore = "needs RabbitMQ at $KEMABLE_RABBITMQ_URL"]
async fn a_closed_connection_opens_again_for_the_next_publish() {
    let console = RabbitmqDriver
        .connect(&live_config(), CallOptions::default())
        .await
        .unwrap();
    // As after sleep or a broker restart: the link is gone under the console.
    console.close().await.unwrap();
    let sink = Arc::new(Heard::default());
    console
        .execute("publish amq.topic kemable.test.again x", sink, CallOptions::default())
        .await
        .expect("the publish goes through on a new connection");
    console.ping(CallOptions::default()).await.unwrap();
    console.close().await.unwrap();
}
