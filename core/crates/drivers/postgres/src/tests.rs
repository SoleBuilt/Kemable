use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::BytesMut;
use paboable_core::catalog::{ColumnMeta, ObjectInfo, ObjectKind, TableRef};
use paboable_core::config::{ConnectionConfig, Secret, Target, TlsMode};
use paboable_core::connection::Connection;
use paboable_core::driver::Driver;
use paboable_core::sink::ResultSink;
use paboable_core::sql::{self, SqlGrammar};
use paboable_core::value::{Value, ValueKind};
use paboable_core::{CallOptions, ErrorCode, format};
use paboable_store::{ResultStore, WindowReader, WindowRequest};
use postgres_protocol::IsNull;
use postgres_protocol::types as wire;
use tokio_postgres::types::Type;

use crate::ddl::{self, Column, Fill, Sequence, Table};
use crate::decode::{self, array_text, interval_text, numeric_text, range_text};
use crate::errors::byte_offset;
use crate::{PostgresDriver, StatementMode};

// --- without a server ---------------------------------------------------------

fn numeric(weight: i16, sign: u16, scale: u16, digits: &[u16]) -> Vec<u8> {
    let mut raw = Vec::new();
    raw.extend_from_slice(&u16::try_from(digits.len()).unwrap().to_be_bytes());
    raw.extend_from_slice(&weight.to_be_bytes());
    raw.extend_from_slice(&sign.to_be_bytes());
    raw.extend_from_slice(&scale.to_be_bytes());
    for digit in digits {
        raw.extend_from_slice(&digit.to_be_bytes());
    }
    raw
}

#[test]
fn numerics_keep_their_scale_and_every_digit() {
    assert_eq!(numeric_text(&numeric(0, 0, 0, &[])).as_deref(), Some("0"));
    assert_eq!(numeric_text(&numeric(0, 0, 1, &[1, 5000])).as_deref(), Some("1.5"));
    assert_eq!(
        numeric_text(&numeric(1, 0, 3, &[1, 2345, 6780])).as_deref(),
        Some("12345.678")
    );
    assert_eq!(numeric_text(&numeric(-1, 0, 4, &[1])).as_deref(), Some("0.0001"));
    assert_eq!(numeric_text(&numeric(-1, 0x4000, 1, &[5000])).as_deref(), Some("-0.5"));
    assert_eq!(numeric_text(&numeric(1, 0, 0, &[1])).as_deref(), Some("10000"));
    assert_eq!(numeric_text(&numeric(0, 0, 2, &[7])).as_deref(), Some("7.00"));
    assert_eq!(numeric_text(&numeric(0, 0xC000, 0, &[])).as_deref(), Some("NaN"));
    assert_eq!(numeric_text(&numeric(0, 0xF000, 0, &[])).as_deref(), Some("-Infinity"));
}

fn interval(micros: i64, days: i32, months: i32) -> Vec<u8> {
    let mut raw = micros.to_be_bytes().to_vec();
    raw.extend_from_slice(&days.to_be_bytes());
    raw.extend_from_slice(&months.to_be_bytes());
    raw
}

#[test]
fn intervals_read_as_psql_shows_them() {
    assert_eq!(interval_text(&interval(0, 0, 0)).as_deref(), Some("00:00:00"));
    let time = ((4 * 60 + 5) * 60 + 6) * 1_000_000 + 789_000;
    assert_eq!(
        interval_text(&interval(time, 3, 14)).as_deref(),
        Some("1 year 2 mons 3 days 04:05:06.789")
    );
    assert_eq!(
        interval_text(&interval(2 * 3_600_000_000, -1, 0)).as_deref(),
        Some("-1 days +02:00:00")
    );
    assert_eq!(
        interval_text(&interval(-90_000_000, 0, 0)).as_deref(),
        Some("-00:01:30")
    );
    assert_eq!(interval_text(&interval(0, 1, 1)).as_deref(), Some("1 mon 1 day"));
}

fn array(element: u32, values: &[Option<&str>], encode: fn(&str, &mut BytesMut)) -> Vec<u8> {
    let mut buf = BytesMut::new();
    let dimension = wire::ArrayDimension {
        len: i32::try_from(values.len()).unwrap(),
        lower_bound: 1,
    };
    wire::array_to_sql(
        Some(dimension),
        element,
        values.iter().copied(),
        |value, buf| match value {
            Some(text) => {
                encode(text, buf);
                Ok(IsNull::No)
            }
            None => Ok(IsNull::Yes),
        },
        &mut buf,
    )
    .unwrap();
    buf.to_vec()
}

#[test]
fn arrays_read_as_psql_shows_them() {
    let ints = array(23, &[Some("1"), None, Some("3")], |text, buf| {
        wire::int4_to_sql(text.parse().unwrap(), buf)
    });
    assert_eq!(array_text(&Type::INT4, &ints).as_deref(), Some("{1,NULL,3}"));

    let texts = array(
        25,
        &[Some("a b"), Some("NULL"), Some(""), Some("q\"x"), Some("plain")],
        wire::text_to_sql,
    );
    assert_eq!(
        array_text(&Type::TEXT, &texts).as_deref(),
        Some(r#"{"a b","NULL","","q\"x",plain}"#)
    );
}

#[test]
fn ranges_read_as_psql_shows_them() {
    let mut buf = BytesMut::new();
    wire::range_to_sql(
        |buf| {
            wire::int4_to_sql(1, buf);
            Ok(wire::RangeBound::Inclusive(IsNull::No))
        },
        |buf| {
            wire::int4_to_sql(5, buf);
            Ok(wire::RangeBound::Exclusive(IsNull::No))
        },
        &mut buf,
    )
    .unwrap();
    assert_eq!(range_text(&Type::INT4, &buf).as_deref(), Some("[1,5)"));

    let mut buf = BytesMut::new();
    wire::range_to_sql(
        |_| Ok(wire::RangeBound::Unbounded),
        |_| Ok(wire::RangeBound::Unbounded),
        &mut buf,
    )
    .unwrap();
    assert_eq!(range_text(&Type::INT4, &buf).as_deref(), Some("(,)"));

    let mut buf = BytesMut::new();
    wire::empty_range_to_sql(&mut buf);
    assert_eq!(range_text(&Type::INT4, &buf).as_deref(), Some("empty"));
}

#[test]
fn dates_and_timestamps_move_to_the_unix_epoch_and_keep_infinity() {
    let mut buf = BytesMut::new();
    wire::date_to_sql(0, &mut buf);
    assert_eq!(decode::decode(&Type::DATE, &buf), Value::Date(10_957));
    let mut buf = BytesMut::new();
    wire::date_to_sql(i32::MAX, &mut buf);
    assert_eq!(decode::decode(&Type::DATE, &buf), Value::Other("infinity".into()));

    let mut buf = BytesMut::new();
    wire::timestamp_to_sql(0, &mut buf);
    assert_eq!(
        format::display(&decode::decode(&Type::TIMESTAMP, &buf)),
        "2000-01-01 00:00:00"
    );
    assert_eq!(
        format::display(&decode::decode(&Type::TIMESTAMPTZ, &buf)),
        "2000-01-01 00:00:00+00:00"
    );
}

#[test]
fn scalar_types_map_to_their_kinds() {
    let mut buf = BytesMut::new();
    wire::float4_to_sql(0.1, &mut buf);
    assert_eq!(decode::decode(&Type::FLOAT4, &buf), Value::Float32(0.1));
    assert_eq!(decode::kind_of(&Type::FLOAT4), ValueKind::Float32);

    let mut buf = BytesMut::new();
    wire::int8_to_sql(-1234, &mut buf);
    assert_eq!(decode::decode(&Type::MONEY, &buf), Value::Decimal("-12.34".into()));

    let mut buf = BytesMut::new();
    wire::inet_to_sql("192.168.1.0".parse().unwrap(), 24, &mut buf);
    assert_eq!(decode::decode(&Type::CIDR, &buf), Value::Other("192.168.1.0/24".into()));
    let mut buf = BytesMut::new();
    wire::inet_to_sql("10.0.0.1".parse().unwrap(), 32, &mut buf);
    assert_eq!(decode::decode(&Type::INET, &buf), Value::Other("10.0.0.1".into()));

    assert_eq!(
        decode::decode(&Type::JSONB, b"\x01{\"a\":1}"),
        Value::Json("{\"a\":1}".into())
    );
    assert_eq!(
        decode::decode(&Type::TEXT, "Việt".as_bytes()),
        Value::Text("Việt".into())
    );
}

#[test]
fn unknown_types_show_printable_bytes_as_text_and_the_rest_as_bytes() {
    let extension = Type::new(
        "citext".into(),
        99_999,
        tokio_postgres::types::Kind::Simple,
        "public".into(),
    );
    assert_eq!(decode::decode(&extension, b"Hello"), Value::Text("Hello".into()));
    assert_eq!(
        decode::decode(&extension, &[1, 0, 0, 0]),
        Value::Bytes(vec![1, 0, 0, 0])
    );
}

#[test]
fn supabase_transaction_pooler_gets_unnamed_statements() {
    let none = BTreeMap::new();
    assert_eq!(
        StatementMode::for_target("aws-0-ap-southeast-1.pooler.supabase.com", 6543, &none),
        StatementMode::Unnamed
    );
    assert_eq!(
        StatementMode::for_target("aws-0-ap-southeast-1.pooler.supabase.com", 5432, &none),
        StatementMode::Prepared
    );
    assert_eq!(
        StatementMode::for_target("localhost", 5432, &none),
        StatementMode::Prepared
    );
    let forced = BTreeMap::from([("statement_mode".to_owned(), "unnamed".to_owned())]);
    assert_eq!(
        StatementMode::for_target("localhost", 5432, &forced),
        StatementMode::Unnamed
    );
}

#[test]
fn error_positions_count_characters_from_one() {
    assert_eq!(byte_offset("select x", 8), Some(7));
    assert_eq!(byte_offset("select 'đ', x", 13), Some(13));
    assert_eq!(byte_offset("abc", 4), Some(3));
    assert_eq!(byte_offset("abc", 0), None);
}

// --- DDL assembly, without a server ----------------------------------------------

fn column(name: &str, type_name: &str) -> Column {
    Column {
        name: name.into(),
        type_name: type_name.into(),
        collation: None,
        fill: Fill::Nothing,
        not_null: false,
        comment: None,
    }
}

#[test]
fn comment_literals_double_their_quotes_and_keep_multibyte_text() {
    assert_eq!(ddl::literal("Khách hàng"), "'Khách hàng'");
    assert_eq!(ddl::literal("O'Brien's"), "'O''Brien''s'");
    assert_eq!(ddl::literal(""), "''");
    assert_eq!(ddl::literal("đơn 'gấp' 🚚"), "'đơn ''gấp'' 🚚'");
    // A backslash reads back the same whatever standard_conforming_strings says.
    assert_eq!(ddl::literal(r"C:\temp"), r"E'C:\\temp'");
    assert_eq!(ddl::literal(r"a\'b"), r"E'a\\''b'");
}

#[test]
fn a_column_line_writes_collation_fill_and_not_null_in_order() {
    assert_eq!(ddl::column_line(&column("id", "integer")), r#""id" integer"#);

    let mut name = column(r#"Tên "khách""#, "character varying(100)");
    name.collation = Some(r#""C""#.into());
    name.fill = Fill::Default("'Khách'::character varying".into());
    name.not_null = true;
    assert_eq!(
        ddl::column_line(&name),
        r#""Tên ""khách""" character varying(100) COLLATE "C" DEFAULT 'Khách'::character varying NOT NULL"#
    );

    let mut id = column("id", "bigint");
    id.fill = Fill::Identity { always: true };
    id.not_null = true;
    assert_eq!(
        ddl::column_line(&id),
        r#""id" bigint GENERATED ALWAYS AS IDENTITY NOT NULL"#
    );
    id.fill = Fill::Identity { always: false };
    assert_eq!(
        ddl::column_line(&id),
        r#""id" bigint GENERATED BY DEFAULT AS IDENTITY NOT NULL"#
    );

    let mut total = column("total", "numeric");
    total.fill = Fill::Generated {
        expression: "(price * 2::numeric)".into(),
        stored: true,
    };
    assert_eq!(
        ddl::column_line(&total),
        r#""total" numeric GENERATED ALWAYS AS ((price * 2::numeric)) STORED"#
    );
    total.fill = Fill::Generated {
        expression: "(price * 2::numeric)".into(),
        stored: false,
    };
    assert_eq!(
        ddl::column_line(&total),
        r#""total" numeric GENERATED ALWAYS AS ((price * 2::numeric)) VIRTUAL"#
    );
}

#[test]
fn a_column_fill_is_read_from_identity_generated_and_the_stored_expression() {
    assert_eq!(Fill::from_catalog("", "", None), Fill::Nothing);
    assert_eq!(
        Fill::from_catalog("", "", Some("now()".into())),
        Fill::Default("now()".into())
    );
    assert_eq!(Fill::from_catalog("a", "", None), Fill::Identity { always: true });
    assert_eq!(Fill::from_catalog("d", "", None), Fill::Identity { always: false });
    assert_eq!(
        Fill::from_catalog("", "s", Some("(a + 1)".into())),
        Fill::Generated {
            expression: "(a + 1)".into(),
            stored: true,
        }
    );
    assert_eq!(
        Fill::from_catalog("", "v", Some("(a + 1)".into())),
        Fill::Generated {
            expression: "(a + 1)".into(),
            stored: false,
        }
    );
}

#[test]
fn a_table_script_has_columns_then_constraints_then_indexes_then_comments() {
    let table = Table {
        schema: "bán hàng".into(),
        name: "orders".into(),
        comment: Some("Đơn hàng của 'khách'".into()),
        ..Table::default()
    };
    let mut id = column("id", "integer");
    id.fill = Fill::Identity { always: true };
    id.not_null = true;
    let mut note = column("note", "text");
    note.comment = Some("Khách hàng".into());
    let constraints = [
        ("orders_pkey".to_owned(), "PRIMARY KEY (id)".to_owned()),
        ("orders_note_check".to_owned(), "CHECK (note <> ''::text)".to_owned()),
    ];
    let indexes = [r#"CREATE INDEX orders_note_idx ON "bán hàng".orders USING btree (note)"#.to_owned()];
    assert_eq!(
        ddl::table_ddl(&table, &[id, note], &constraints, &indexes),
        concat!(
            "CREATE TABLE \"bán hàng\".\"orders\" (\n",
            "    \"id\" integer GENERATED ALWAYS AS IDENTITY NOT NULL,\n",
            "    \"note\" text,\n",
            "    CONSTRAINT \"orders_pkey\" PRIMARY KEY (id),\n",
            "    CONSTRAINT \"orders_note_check\" CHECK (note <> ''::text)\n",
            ");\n\n",
            "CREATE INDEX orders_note_idx ON \"bán hàng\".orders USING btree (note);\n\n",
            "COMMENT ON TABLE \"bán hàng\".\"orders\" IS 'Đơn hàng của ''khách''';\n\n",
            "COMMENT ON COLUMN \"bán hàng\".\"orders\".\"note\" IS 'Khách hàng';",
        )
    );
}

#[test]
fn partitioned_unlogged_and_foreign_tables_keep_their_clauses() {
    let parent = Table {
        schema: "public".into(),
        name: "events".into(),
        partition_key: Some("RANGE (at)".into()),
        ..Table::default()
    };
    assert_eq!(
        ddl::table_ddl(&parent, &[column("at", "date")], &[], &[]),
        "CREATE TABLE \"public\".\"events\" (\n    \"at\" date\n) PARTITION BY RANGE (at);"
    );

    let partition = Table {
        schema: "public".into(),
        name: "events_2024".into(),
        unlogged: true,
        options: vec!["fillfactor=70".into()],
        partition_of: Some((
            "public".into(),
            "events".into(),
            "FOR VALUES FROM ('2024-01-01') TO ('2025-01-01')".into(),
        )),
        ..Table::default()
    };
    assert_eq!(
        ddl::table_ddl(&partition, &[column("at", "date")], &[], &[]),
        concat!(
            "CREATE UNLOGGED TABLE \"public\".\"events_2024\" (\n    \"at\" date\n) WITH (fillfactor=70);\n\n",
            "ALTER TABLE \"public\".\"events\" ATTACH PARTITION \"public\".\"events_2024\" ",
            "FOR VALUES FROM ('2024-01-01') TO ('2025-01-01');",
        )
    );

    let foreign = Table {
        schema: "public".into(),
        name: "remote".into(),
        server: Some((
            "files".into(),
            vec!["filename=/tmp/a b.csv".into(), "format=csv".into()],
        )),
        comment: Some("x".into()),
        ..Table::default()
    };
    assert_eq!(
        ddl::table_ddl(&foreign, &[column("a", "text")], &[], &[]),
        concat!(
            "CREATE FOREIGN TABLE \"public\".\"remote\" (\n    \"a\" text\n) ",
            "SERVER \"files\" OPTIONS (\"filename\" '/tmp/a b.csv', \"format\" 'csv');\n\n",
            "COMMENT ON FOREIGN TABLE \"public\".\"remote\" IS 'x';",
        )
    );
}

#[test]
fn a_view_is_one_create_or_replace_statement_ending_with_a_semicolon() {
    assert_eq!(
        ddl::view_ddl(
            "public",
            "big",
            &[],
            " SELECT id\n   FROM orders\n  WHERE total > 100::numeric;\n"
        ),
        "CREATE OR REPLACE VIEW \"public\".\"big\" AS\nSELECT id\n   FROM orders\n  WHERE total > 100::numeric;"
    );
    assert_eq!(
        ddl::view_ddl("public", "v", &["check_option=local".into()], " SELECT 1"),
        "CREATE OR REPLACE VIEW \"public\".\"v\" WITH (check_option=local) AS\nSELECT 1;"
    );
}

#[test]
fn a_materialized_view_keeps_its_indexes_and_says_when_it_holds_no_data() {
    let indexes = ["CREATE UNIQUE INDEX m_id ON public.m USING btree (id)".to_owned()];
    assert_eq!(
        ddl::materialized_view_ddl("public", "m", &[], " SELECT 1 AS id;", false, &indexes, Some("Tóm tắt")),
        concat!(
            "CREATE MATERIALIZED VIEW \"public\".\"m\" AS\nSELECT 1 AS id\nWITH NO DATA;\n\n",
            "CREATE UNIQUE INDEX m_id ON public.m USING btree (id);\n\n",
            "COMMENT ON MATERIALIZED VIEW \"public\".\"m\" IS 'Tóm tắt';",
        )
    );
    assert_eq!(
        ddl::materialized_view_ddl("public", "m", &[], " SELECT 1 AS id;", true, &[], None),
        "CREATE MATERIALIZED VIEW \"public\".\"m\" AS\nSELECT 1 AS id;"
    );
}

#[test]
fn a_sequence_script_names_every_setting() {
    let sequence = Sequence {
        type_name: "bigint".into(),
        increment: "-1".into(),
        min: "-9223372036854775808".into(),
        max: "-1".into(),
        start: "-1".into(),
        cache: "20".into(),
        cycle: false,
    };
    assert_eq!(
        ddl::sequence_ddl("public", "down", false, &sequence),
        concat!(
            "CREATE SEQUENCE \"public\".\"down\"\n",
            "    AS bigint\n",
            "    INCREMENT BY -1\n",
            "    MINVALUE -9223372036854775808\n",
            "    MAXVALUE -1\n",
            "    START WITH -1\n",
            "    CACHE 20\n",
            "    NO CYCLE;",
        )
    );
    let cycling = Sequence {
        cycle: true,
        ..sequence
    };
    let script = ddl::sequence_ddl("public", "down", true, &cycling);
    assert!(script.starts_with("CREATE UNLOGGED SEQUENCE \"public\".\"down\"\n"));
    assert!(script.ends_with("\n    CYCLE;"));
}

#[test]
fn routine_overloads_end_with_a_semicolon_and_a_blank_line_between() {
    let definitions = [
        "CREATE OR REPLACE FUNCTION public.f(a integer)\n RETURNS integer\n LANGUAGE sql\nAS $function$select a$function$\n"
            .to_owned(),
        "CREATE OR REPLACE PROCEDURE public.f()\n LANGUAGE sql\nBEGIN ATOMIC\n SELECT 1;\nEND\n".to_owned(),
    ];
    assert_eq!(
        ddl::routines_ddl(&definitions),
        concat!(
            "CREATE OR REPLACE FUNCTION public.f(a integer)\n RETURNS integer\n LANGUAGE sql\n",
            "AS $function$select a$function$;\n\n",
            "CREATE OR REPLACE PROCEDURE public.f()\n LANGUAGE sql\nBEGIN ATOMIC\n SELECT 1;\nEND;",
        )
    );
}

#[test]
fn only_relations_and_routines_have_a_ddl_query() {
    assert_eq!(ddl::relkinds(ObjectKind::Table), Some("rpf"));
    assert_eq!(ddl::relkinds(ObjectKind::View), Some("v"));
    assert_eq!(ddl::relkinds(ObjectKind::MaterializedView), Some("m"));
    assert_eq!(ddl::relkinds(ObjectKind::Sequence), Some("S"));
    assert_eq!(ddl::prokinds(ObjectKind::Function), Some("f"));
    assert_eq!(ddl::prokinds(ObjectKind::Procedure), Some("p"));
    for kind in [ObjectKind::Index, ObjectKind::Trigger, ObjectKind::Key] {
        assert_eq!(ddl::relkinds(kind), None);
        assert_eq!(ddl::prokinds(kind), None);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_target_is_refused_before_connecting() {
    let config = ConnectionConfig {
        engine: "postgres".into(),
        target: Target::File {
            path: "x.db".into(),
            read_only: false,
        },
        options: BTreeMap::new(),
    };
    let error = PostgresDriver
        .connect(&config, CallOptions::default())
        .await
        .err()
        .expect("a refusal");
    assert_eq!(error.code, ErrorCode::ConfigInvalid);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_port_fails_to_connect() {
    let config = ConnectionConfig {
        engine: "postgres".into(),
        target: Target::Network {
            host: "127.0.0.1".into(),
            port: Some(1),
            user: None,
            password: Secret::default(),
            database: None,
            tls: TlsMode::Disable,
        },
        options: BTreeMap::new(),
    };
    let error = PostgresDriver
        .connect(&config, CallOptions::default())
        .await
        .err()
        .expect("a refusal");
    assert_eq!(error.code, ErrorCode::Connect);
}

// --- with a server: PABOABLE_POSTGRES_URL=postgres://user:pw@host/db -----------

const LIVE: &str = "needs PostgreSQL at $PABOABLE_POSTGRES_URL";

fn live_config() -> ConnectionConfig {
    let url = std::env::var("PABOABLE_POSTGRES_URL").expect("PABOABLE_POSTGRES_URL is set");
    paboable_core::connection_url::parse(&url).expect("a valid URL")
}

async fn live() -> Arc<dyn Connection> {
    PostgresDriver
        .connect(&live_config(), CallOptions::default())
        .await
        .expect("the server accepts the connection")
}

#[derive(Default)]
struct Collect {
    store: Mutex<Option<Arc<ResultStore>>>,
    commands: Mutex<Vec<Option<u64>>>,
    notices: Mutex<Vec<String>>,
}

impl ResultSink for Collect {
    fn begin_set(&self, columns: Vec<ColumnMeta>) {
        *self.store.lock().unwrap() = Some(Arc::new(ResultStore::new(columns, Some(1000))));
    }

    fn push_rows(&self, values: &[Value]) -> bool {
        self.store
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|store| store.push_rows(values))
    }

    fn end_set(&self) {
        if let Some(store) = self.store.lock().unwrap().as_ref() {
            store.finish();
        }
    }

    fn command_done(&self, rows_affected: Option<u64>) {
        self.commands.lock().unwrap().push(rows_affected);
    }

    fn notice(&self, text: &str) {
        self.notices.lock().unwrap().push(text.to_owned());
    }
}

async fn run(conn: &Arc<dyn Connection>, sql: &str) -> Arc<Collect> {
    let sink = Arc::new(Collect::default());
    conn.execute(sql, sink.clone(), CallOptions::default())
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
    sink
}

fn texts(sink: &Collect) -> Vec<String> {
    let store = sink.store.lock().unwrap().clone().expect("a result set");
    let width = store.columns().len();
    let bytes = store.fetch_window(WindowRequest {
        first_row: 0,
        row_count: 1,
        first_column: 0,
        column_count: u32::try_from(width).unwrap(),
        max_chars: 200,
    });
    let window = WindowReader::new(&bytes).unwrap();
    (0..width)
        .map(|column| {
            if window.is_null(0, column) {
                "NULL".into()
            } else {
                window.text(0, column).to_owned()
            }
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn every_mapped_type_reads_as_psql_shows_it() {
    let _ = LIVE;
    let conn = live().await;
    let sink = run(
        &conn,
        "select 1::int2, 2::int4, 3::int8, 0.1::float4, 2.5::float8, 12345.678::numeric, 'NaN'::numeric, \
         true, 'đ'::text, '\\x00ff'::bytea, 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'::uuid, '{\"a\": 1}'::json, \
         '{\"a\": 1}'::jsonb, '2024-02-29'::date, 'infinity'::date, '2023-11-14 22:13:20.304865'::timestamp, \
         '2023-11-14 22:13:20+00'::timestamptz, '04:05:06.5'::time, '1 year 2 mons 3 days 04:05:06.789'::interval, \
         array[1, null, 3], '[1,5)'::int4range, '192.168.1.0/24'::cidr, '10.0.0.1'::inet, null::text, \
         '12:00:00+07'::timetz, B'1011'::bit varying, '(1.5,2)'::point",
    )
    .await;
    assert_eq!(
        texts(&sink),
        [
            "1",
            "2",
            "3",
            "0.1",
            "2.5",
            "12345.678",
            "NaN",
            "true",
            "đ",
            "0x00ff",
            "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
            "{\"a\": 1}",
            "{\"a\": 1}",
            "2024-02-29",
            "infinity",
            "2023-11-14 22:13:20.304865",
            "2023-11-14 22:13:20+00:00",
            "04:05:06.5",
            "1 year 2 mons 3 days 04:05:06.789",
            "{1,NULL,3}",
            "[1,5)",
            "192.168.1.0/24",
            "10.0.0.1",
            "NULL",
            "12:00:00+07",
            "1011",
            "(1.5,2)",
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn commands_report_rows_affected_and_empty_results_keep_their_columns() {
    let conn = live().await;
    run(&conn, "create temp table paboable_t (id int primary key, name text)").await;
    let sink = run(&conn, "insert into paboable_t values (1, 'a'), (2, 'b'), (3, 'c')").await;
    assert_eq!(*sink.commands.lock().unwrap(), [Some(3)]);
    let sink = run(&conn, "select * from paboable_t where false").await;
    let store = sink.store.lock().unwrap().clone().expect("a set with no rows");
    assert_eq!(store.columns().len(), 2);
    assert_eq!(store.state().total_rows, 0);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn the_row_limit_stops_the_server_and_the_next_statement_still_runs() {
    let conn = live().await;
    let sink = Arc::new(Collect::default());
    let outcome = conn
        .execute(
            "select g from generate_series(1, 5000000) g",
            sink.clone(),
            CallOptions::default(),
        )
        .await
        .unwrap();
    assert!(outcome.limited);
    assert_eq!(sink.store.lock().unwrap().clone().unwrap().state().total_rows, 1000);
    assert_eq!(texts(&*run(&conn, "select 42").await), ["42"]);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn a_cancel_stops_the_statement_on_the_server() {
    let conn = live().await;
    let options = CallOptions::default();
    let cancel = options.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    let error = conn
        .execute("select pg_sleep(30)", Arc::new(Collect::default()), options)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the server stopped the sleep"
    );
    assert_eq!(texts(&*run(&conn, "select 1").await), ["1"]);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn a_deadline_reports_a_timeout() {
    let conn = live().await;
    let options = CallOptions::default().with_timeout(Duration::from_millis(300));
    let error = conn
        .execute("select pg_sleep(30)", Arc::new(Collect::default()), options)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Timeout);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn a_syntax_error_carries_its_sqlstate_and_byte_position() {
    let conn = live().await;
    let error = conn
        .execute("select 'đ', frm", Arc::new(Collect::default()), CallOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Query);
    assert_eq!(error.engine_code.as_deref(), Some("42703"));
    assert_eq!(error.position.map(|p| p.byte_offset), Some(13));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn notices_reach_the_sink() {
    let conn = live().await;
    let sink = run(&conn, "do $$ begin raise notice 'hello from plpgsql'; end $$").await;
    // A notice can arrive just after the statement's reply; give it a moment.
    for _ in 0..20 {
        if !sink.notices.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(*sink.notices.lock().unwrap(), ["hello from plpgsql"]);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn the_catalog_lists_schemas_tables_and_columns() {
    let conn = live().await;
    let schema = format!("paboable_test_{}", std::process::id());
    // A run that failed half way leaves its schema behind; remove those first.
    run(
        &conn,
        "do $$ declare s text; begin \
           for s in select nspname from pg_namespace where nspname like 'paboable\\_test\\_%' loop \
             execute format('drop schema %I cascade', s); \
           end loop; end $$",
    )
    .await;
    run(&conn, &format!("create schema {schema}")).await;
    run(
        &conn,
        &format!("create table {schema}.items (id int primary key, name text not null default 'x')"),
    )
    .await;
    run(&conn, &format!("create view {schema}.v as select 1 as one")).await;

    let schemas = conn.list_schemas(CallOptions::default()).await.unwrap();
    assert!(schemas.iter().any(|s| s.name == schema));
    let objects = conn.list_objects(Some(&schema), CallOptions::default()).await.unwrap();
    let names: Vec<_> = objects.iter().map(|o| (o.name.as_str(), o.kind)).collect();
    assert_eq!(names, [("items", ObjectKind::Table), ("v", ObjectKind::View)]);
    let columns = conn
        .fetch_columns(
            &TableRef {
                schema: Some(schema.clone()),
                name: "items".into(),
            },
            CallOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0].primary_key, Some(1));
    assert!(!columns[1].nullable);
    assert_eq!(columns[1].default.as_deref(), Some("'x'::text"));

    run(&conn, &format!("drop schema {schema} cascade")).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn the_databases_this_login_may_open_are_listed_with_the_current_one() {
    let conn = live().await;
    let current = conn.current_database().expect("PostgreSQL reports its database");
    let databases = conn.list_databases(CallOptions::default()).await.unwrap();
    let marked: Vec<_> = databases
        .iter()
        .filter(|db| db.is_current)
        .map(|db| db.name.as_str())
        .collect();
    assert_eq!(marked, [current.as_str()]);
    // Templates take no connections from a client, so they are not listed.
    assert!(
        databases
            .iter()
            .all(|db| db.name != "template0" && db.name != "template1")
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn a_wrong_password_is_an_auth_error() {
    let mut config = live_config();
    if let Target::Network { password, .. } = &mut config.target {
        *password = Secret::new("certainly-not-the-password");
    }
    let error = PostgresDriver
        .connect(&config, CallOptions::default())
        .await
        .err()
        .expect("a refusal");
    assert_eq!(error.code, ErrorCode::Auth);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs PostgreSQL at $PABOABLE_POSTGRES_URL"]
async fn object_ddl_scripts_what_the_server_holds_and_the_script_runs_again() {
    let conn = live().await;
    let schema = format!("paboable_ddl_{}", std::process::id());
    let copy = format!("{schema}_copy");
    for name in [&schema, &copy] {
        run(&conn, &format!("drop schema if exists {name} cascade")).await;
    }
    run(&conn, &format!("create schema {schema}")).await;
    run(
        &conn,
        &format!(
            "create table {schema}.customers (\
               id int generated always as identity primary key, \
               name text collate \"C\" not null, \
               email text, \
               unique (email))"
        ),
    )
    .await;
    run(&conn, &format!("comment on table {schema}.customers is 'Khách hàng'")).await;
    run(
        &conn,
        &format!("comment on column {schema}.customers.name is 'Tên của khách ''VIP'''"),
    )
    .await;
    run(
        &conn,
        &format!(
            "create table {schema}.orders (\
               id bigint generated by default as identity, \
               customer_id int not null references {schema}.customers (id) on delete cascade, \
               total numeric(12, 2) not null default 0, \
               placed_at timestamptz default now(), \
               with_tax numeric generated always as (total * 1.1) stored, \
               constraint orders_pkey primary key (id), \
               constraint orders_total_check check (total >= 0))"
        ),
    )
    .await;
    run(
        &conn,
        &format!("create index orders_placed_at_idx on {schema}.orders (placed_at desc)"),
    )
    .await;
    run(
        &conn,
        &format!("create view {schema}.big_orders as select id, total from {schema}.orders where total > 100"),
    )
    .await;
    run(
        &conn,
        &format!(
            "create function {schema}.order_count(c int) returns bigint language sql stable \
             as $$ select count(*) from {schema}.orders where customer_id = c $$"
        ),
    )
    .await;
    run(
        &conn,
        &format!("comment on function {schema}.order_count(int) is 'Đếm đơn'"),
    )
    .await;

    let object = |schema: &str, name: &str, kind: ObjectKind| ObjectInfo {
        schema: Some(schema.to_owned()),
        name: name.to_owned(),
        kind,
    };
    let ddl_of = |object: ObjectInfo| {
        let conn = conn.clone();
        async move {
            conn.object_ddl(&object, CallOptions::default())
                .await
                .unwrap_or_else(|error| panic!("{object:?}: {error}"))
        }
    };

    let customers = ddl_of(object(&schema, "customers", ObjectKind::Table)).await;
    assert!(customers.starts_with(&format!("CREATE TABLE \"{schema}\".\"customers\" (\n")));
    assert!(customers.contains("\"id\" integer GENERATED ALWAYS AS IDENTITY NOT NULL"));
    assert!(customers.contains("\"name\" text COLLATE \"C\" NOT NULL"));
    let primary = customers.find("CONSTRAINT \"customers_pkey\" PRIMARY KEY (id)");
    let unique = customers.find("CONSTRAINT \"customers_email_key\" UNIQUE (email)");
    assert!(primary.is_some() && unique.is_some() && primary < unique, "{customers}");
    assert!(customers.contains(&format!("COMMENT ON TABLE \"{schema}\".\"customers\" IS 'Khách hàng';")));
    assert!(customers.contains(&format!(
        "COMMENT ON COLUMN \"{schema}\".\"customers\".\"name\" IS 'Tên của khách ''VIP''';"
    )));
    // The key's own index comes with its constraint, not as an index.
    assert!(!customers.contains("CREATE UNIQUE INDEX"), "{customers}");

    let orders = ddl_of(object(&schema, "orders", ObjectKind::Table)).await;
    assert!(orders.contains("\"id\" bigint GENERATED BY DEFAULT AS IDENTITY NOT NULL"));
    assert!(orders.contains("\"customer_id\" integer NOT NULL"));
    // The server may write the default as `0` or `0::numeric`, as its version deparses it.
    let total = orders
        .lines()
        .find(|line| line.contains("\"total\" numeric(12,2) DEFAULT 0"))
        .unwrap_or_else(|| panic!("{orders}"));
    assert!(total.ends_with(" NOT NULL,"), "{total}");
    assert!(orders.contains("\"placed_at\" timestamp with time zone DEFAULT now()"));
    assert!(orders.contains("\"with_tax\" numeric GENERATED ALWAYS AS ("));
    assert!(orders.contains(") STORED"));
    let primary = orders.find("CONSTRAINT \"orders_pkey\" PRIMARY KEY (id)");
    let check = orders.find("CONSTRAINT \"orders_total_check\" CHECK (");
    let foreign = orders.find("CONSTRAINT \"orders_customer_id_fkey\" FOREIGN KEY (customer_id) REFERENCES ");
    assert!(primary.is_some() && check.is_some() && foreign.is_some(), "{orders}");
    assert!(primary < check && check < foreign, "{orders}");
    assert!(orders.contains("ON DELETE CASCADE"));
    assert!(orders.contains("CREATE INDEX orders_placed_at_idx ON "));
    assert!(orders.contains("(placed_at DESC);"));
    assert!(orders.ends_with(';'));

    let view = ddl_of(object(&schema, "big_orders", ObjectKind::View)).await;
    assert!(view.starts_with(&format!("CREATE OR REPLACE VIEW \"{schema}\".\"big_orders\" AS\n")));
    assert!(view.ends_with(';'));

    // By its bare name, and by the name with arguments that list_objects shows.
    let function = ddl_of(object(&schema, "order_count", ObjectKind::Function)).await;
    assert!(function.starts_with("CREATE OR REPLACE FUNCTION "), "{function}");
    assert!(function.contains("RETURNS bigint"));
    assert!(function.ends_with(';'));
    let listed = conn.list_objects(Some(&schema), CallOptions::default()).await.unwrap();
    let listed = listed
        .into_iter()
        .find(|object| object.kind == ObjectKind::Function)
        .expect("the function is listed");
    assert_eq!(listed.name, "order_count(c integer)");
    assert_eq!(ddl_of(listed.clone()).await, function);

    let comment = |object: ObjectInfo| {
        let conn = conn.clone();
        async move { conn.object_comment(&object, CallOptions::default()).await }
    };
    assert_eq!(
        comment(object(&schema, "customers", ObjectKind::Table))
            .await
            .unwrap()
            .as_deref(),
        Some("Khách hàng")
    );
    assert_eq!(
        comment(object(&schema, "orders", ObjectKind::Table)).await.unwrap(),
        None
    );
    assert_eq!(comment(listed).await.unwrap().as_deref(), Some("Đếm đơn"));
    assert_eq!(
        comment(object(&schema, "missing", ObjectKind::Table))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let error = conn
        .object_ddl(&object(&schema, "missing", ObjectKind::View), CallOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    let error = conn
        .object_ddl(
            &object(&schema, "orders_pkey", ObjectKind::Index),
            CallOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);

    // The scripts run again in a second schema, and read back the same there.
    run(&conn, &format!("create schema {copy}")).await;
    for script in [&customers, &orders, &view, &function] {
        let script = script.replace(&schema, &copy);
        for span in sql::split(&script, SqlGrammar::Postgres) {
            run(&conn, span.text(&script)).await;
        }
    }
    for (name, kind, original) in [
        ("customers", ObjectKind::Table, &customers),
        ("orders", ObjectKind::Table, &orders),
        ("big_orders", ObjectKind::View, &view),
    ] {
        assert_eq!(
            ddl_of(object(&copy, name, kind)).await,
            original.replace(&schema, &copy),
            "{name} reads back as it was written"
        );
    }

    for name in [&schema, &copy] {
        run(&conn, &format!("drop schema {name} cascade")).await;
    }
}
