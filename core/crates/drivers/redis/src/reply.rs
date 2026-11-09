//! A command's reply as a result set, shaped by what the command returns:
//! a flat field-value array as two columns, a scan as its items with the
//! next cursor as a notice, a stream entry as its id and its fields as JSON.
//!
//! | Reply | Result |
//! |---|---|
//! | `OK` | done, no rows |
//! | other status, integer, string, nil | one row, column `value` |
//! | `HGETALL`, `CONFIG GET`, `HRANDFIELD ... WITHVALUES` | `field`, `value` |
//! | `ZRANGE` and the like `WITHSCORES`, `ZPOPMIN`, `ZPOPMAX` | `member`, `score` |
//! | `SCAN`, `SSCAN` / `HSCAN` / `ZSCAN` | the items as above; the next cursor as a notice |
//! | `XRANGE`, `XREVRANGE` | `id`, `fields` (JSON) |
//! | any other array | one row per element, column `value` |
//!
//! A string that is UTF-8 is text, any other is bytes. Nested replies that fit
//! no shape are written as `redis-cli` prints them on one line.

use std::sync::Arc;

use paboable_core::catalog::ColumnMeta;
use paboable_core::sink::{ExecOutcome, PUSH_BATCH_ROWS, ResultSink};
use paboable_core::value::{Value, ValueKind};

use crate::resp::Reply;

/// Emits `reply` to `sink` for the command `args`.
pub fn emit(args: &[Vec<u8>], reply: Reply, sink: &Arc<dyn ResultSink>) -> ExecOutcome {
    let name = args
        .first()
        .map(|arg| String::from_utf8_lossy(arg).to_ascii_uppercase())
        .unwrap_or_default();
    let has = |word: &str| args.iter().skip(1).any(|arg| arg.eq_ignore_ascii_case(word.as_bytes()));
    match reply {
        Reply::Status(text) if text == "OK" => {
            sink.command_done(None);
            ExecOutcome::default()
        }
        Reply::Array(items) => match name.as_str() {
            "CONFIG" if args.get(1).is_some_and(|word| word.eq_ignore_ascii_case(b"GET")) => {
                pairs(sink, ("field", ValueKind::Text), items, false)
            }
            "HGETALL" => pairs(sink, ("field", ValueKind::Text), items, false),
            "HRANDFIELD" if has("WITHVALUES") => pairs(sink, ("field", ValueKind::Text), items, false),
            "ZPOPMIN" | "ZPOPMAX" => pairs(sink, ("member", ValueKind::Text), items, true),
            _ if name.starts_with('Z') && has("WITHSCORES") => pairs(sink, ("member", ValueKind::Text), items, true),
            "SCAN" | "SSCAN" | "HSCAN" | "ZSCAN" => scan(&name, sink, items),
            "XRANGE" | "XREVRANGE" => entries(sink, items),
            _ => list(sink, items),
        },
        single => {
            let value = scalar(single);
            let kind = value.kind();
            sink.begin_set(vec![column("value", kind)]);
            sink.push_rows(&[value]);
            sink.end_set();
            ExecOutcome::default()
        }
    }
}

fn column(name: &str, kind: ValueKind) -> ColumnMeta {
    ColumnMeta {
        name: name.to_owned(),
        type_name: String::new(),
        kind,
    }
}

/// A reply that is not an array, as one value.
pub fn scalar(reply: Reply) -> Value {
    match reply {
        Reply::Status(text) | Reply::Error(text) => Value::Text(text),
        Reply::Integer(value) => Value::Int(value),
        Reply::Bulk(bytes) => bytes_value(bytes),
        Reply::Nil => Value::Null,
        Reply::Array(items) => Value::Text(render(&Reply::Array(items))),
    }
}

pub fn bytes_value(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(text) => Value::Text(text),
        Err(error) => Value::Bytes(error.into_bytes()),
    }
}

/// A score as Redis writes it (`1.5`, `inf`, `-inf`).
pub fn score(reply: Reply) -> Value {
    match reply.text().and_then(|text| text.parse::<f64>().ok()) {
        Some(value) => Value::Float(value),
        None => scalar(reply),
    }
}

/// Streams rows to `sink` in batches; `false` once it takes no more.
pub struct Rows<'a> {
    sink: &'a Arc<dyn ResultSink>,
    width: usize,
    batch: Vec<Value>,
    pub limited: bool,
}

impl<'a> Rows<'a> {
    pub fn begin(sink: &'a Arc<dyn ResultSink>, columns: Vec<ColumnMeta>) -> Self {
        let width = columns.len();
        sink.begin_set(columns);
        Self {
            sink,
            width,
            batch: Vec::with_capacity(width * PUSH_BATCH_ROWS),
            limited: false,
        }
    }

    pub fn push(&mut self, row: impl IntoIterator<Item = Value>) -> bool {
        if self.limited {
            return false;
        }
        self.batch.extend(row);
        if self.batch.len() >= self.width * PUSH_BATCH_ROWS {
            return self.flush();
        }
        true
    }

    fn flush(&mut self) -> bool {
        if !self.batch.is_empty() {
            let more = self.sink.push_rows(&self.batch);
            self.batch.clear();
            if !more {
                self.limited = true;
            }
        }
        !self.limited
    }

    pub fn end(mut self) -> ExecOutcome {
        self.flush();
        self.sink.end_set();
        ExecOutcome { limited: self.limited }
    }
}

fn pairs(sink: &Arc<dyn ResultSink>, first: (&str, ValueKind), items: Vec<Reply>, scored: bool) -> ExecOutcome {
    let second = if scored {
        column("score", ValueKind::Float)
    } else {
        column("value", ValueKind::Text)
    };
    let mut rows = Rows::begin(sink, vec![column(first.0, first.1), second]);
    push_pairs(&mut rows, items, scored);
    rows.end()
}

pub fn push_pairs(rows: &mut Rows<'_>, items: Vec<Reply>, scored: bool) -> bool {
    let mut items = items.into_iter();
    while let Some(key) = items.next() {
        let value = items.next().unwrap_or(Reply::Nil);
        let value = if scored { score(value) } else { scalar(value) };
        if !rows.push([scalar(key), value]) {
            return false;
        }
    }
    true
}

fn scan(name: &str, sink: &Arc<dyn ResultSink>, items: Vec<Reply>) -> ExecOutcome {
    let mut items = items.into_iter();
    let cursor = items.next().and_then(|reply| reply.text()).unwrap_or_default();
    let found = match items.next() {
        Some(Reply::Array(found)) => found,
        _ => Vec::new(),
    };
    sink.notice(&if cursor == "0" {
        "The scan is done.".to_owned()
    } else {
        format!("Next cursor: {cursor}")
    });
    match name {
        "HSCAN" => pairs(sink, ("field", ValueKind::Text), found, false),
        "ZSCAN" => pairs(sink, ("member", ValueKind::Text), found, true),
        _ => {
            let label = if name == "SCAN" { "key" } else { "member" };
            let mut rows = Rows::begin(sink, vec![column(label, ValueKind::Text)]);
            for item in found {
                if !rows.push([scalar(item)]) {
                    break;
                }
            }
            rows.end()
        }
    }
}

fn entries(sink: &Arc<dyn ResultSink>, items: Vec<Reply>) -> ExecOutcome {
    let mut rows = Rows::begin(
        sink,
        vec![column("id", ValueKind::Text), column("fields", ValueKind::Json)],
    );
    push_entries(&mut rows, items);
    rows.end()
}

/// Stream entries as rows; the last id pushed, to read on from.
pub fn push_entries(rows: &mut Rows<'_>, items: Vec<Reply>) -> Option<String> {
    let mut last = None;
    for item in items {
        let Reply::Array(parts) = item else { continue };
        let mut parts = parts.into_iter();
        let id = parts.next().and_then(|reply| reply.text()).unwrap_or_default();
        let fields = match parts.next() {
            Some(Reply::Array(fields)) => fields,
            _ => Vec::new(),
        };
        last = Some(id.clone());
        if !rows.push([Value::Text(id), Value::Json(fields_json(fields))]) {
            break;
        }
    }
    last
}

fn fields_json(fields: Vec<Reply>) -> String {
    let mut out = String::from("{");
    let mut fields = fields.into_iter();
    let mut first = true;
    while let Some(key) = fields.next() {
        let value = fields.next().unwrap_or(Reply::Nil);
        if !first {
            out.push(',');
        }
        first = false;
        json_string(&mut out, &key.text().unwrap_or_default());
        out.push(':');
        match value.text() {
            Some(text) => json_string(&mut out, &text),
            None => out.push_str("null"),
        }
    }
    out.push('}');
    out
}

fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if u32::from(ch) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn list(sink: &Arc<dyn ResultSink>, items: Vec<Reply>) -> ExecOutcome {
    let kind = items
        .first()
        .map_or(ValueKind::Text, |item| scalar(item.clone()).kind());
    let mut rows = Rows::begin(sink, vec![column("value", kind)]);
    for item in items {
        if !rows.push([scalar(item)]) {
            break;
        }
    }
    rows.end()
}

/// A reply on one line, as `redis-cli` prints it.
fn render(reply: &Reply) -> String {
    match reply {
        Reply::Status(text) => text.clone(),
        Reply::Error(text) => format!("(error) {text}"),
        Reply::Integer(value) => value.to_string(),
        Reply::Bulk(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Reply::Nil => "(nil)".to_owned(),
        Reply::Array(items) => format!("[{}]", items.iter().map(render).collect::<Vec<_>>().join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paboable_store::ResultStore;
    use std::sync::Mutex;

    /// Keeps each set's columns and rows as text, and the notices.
    #[derive(Default)]
    struct Collect {
        sets: Mutex<Vec<(Vec<String>, ResultStore)>>,
        notices: Mutex<Vec<String>>,
        done: Mutex<usize>,
    }

    impl ResultSink for Collect {
        fn begin_set(&self, columns: Vec<ColumnMeta>) {
            let names = columns.iter().map(|column| column.name.clone()).collect();
            self.sets
                .lock()
                .unwrap()
                .push((names, ResultStore::new(columns, Some(3))));
        }
        fn push_rows(&self, values: &[Value]) -> bool {
            self.sets.lock().unwrap().last().unwrap().1.push_rows(values)
        }
        fn end_set(&self) {}
        fn command_done(&self, _rows_affected: Option<u64>) {
            *self.done.lock().unwrap() += 1;
        }
        fn notice(&self, text: &str) {
            self.notices.lock().unwrap().push(text.to_owned());
        }
    }

    fn run(args: &[&str], reply: Reply) -> (Arc<Collect>, ExecOutcome) {
        let collect = Arc::new(Collect::default());
        let sink: Arc<dyn ResultSink> = collect.clone();
        let args: Vec<Vec<u8>> = args.iter().map(|arg| arg.as_bytes().to_vec()).collect();
        let outcome = emit(&args, reply, &sink);
        (collect, outcome)
    }

    /// Each set's column names and its rows as text.
    type Shown = Vec<(Vec<String>, Vec<Vec<Option<String>>>)>;

    fn rows(collect: &Collect) -> Shown {
        collect
            .sets
            .lock()
            .unwrap()
            .iter()
            .map(|(names, store)| {
                let count = store.state().total_rows;
                let rows = (0..count).map(|row| store.row_texts(row).unwrap_or_default()).collect();
                (names.clone(), rows)
            })
            .collect()
    }

    fn bulk(text: &str) -> Reply {
        Reply::Bulk(text.as_bytes().to_vec())
    }

    fn texts(list: &[&str]) -> Vec<Option<String>> {
        list.iter().map(|text| Some((*text).to_owned())).collect()
    }

    #[test]
    fn ok_is_done_and_a_single_reply_is_one_row() {
        let (collect, _) = run(&["SET", "k", "v"], Reply::Status("OK".into()));
        assert_eq!(*collect.done.lock().unwrap(), 1);
        assert!(rows(&collect).is_empty());

        let (collect, _) = run(&["GET", "missing"], Reply::Nil);
        assert_eq!(rows(&collect), [(vec!["value".to_owned()], vec![vec![None]])]);
        let (collect, _) = run(&["INCR", "n"], Reply::Integer(7));
        assert_eq!(rows(&collect)[0].1, [texts(&["7"])]);
        let (collect, _) = run(&["GET", "b"], Reply::Bulk(vec![0xff, 0x00]));
        assert_eq!(
            rows(&collect)[0].1,
            [texts(&["0xff00"])],
            "bytes show as the core formats them"
        );
    }

    #[test]
    fn a_hash_is_field_and_value_and_scores_are_numbers() {
        let (collect, _) = run(
            &["hgetall", "h"],
            Reply::Array(vec![bulk("a"), bulk("1"), bulk("b"), bulk("2")]),
        );
        assert_eq!(
            rows(&collect),
            [(
                vec!["field".to_owned(), "value".to_owned()],
                vec![texts(&["a", "1"]), texts(&["b", "2"])]
            )]
        );
        let (collect, _) = run(
            &["ZRANGE", "z", "0", "-1", "withscores"],
            Reply::Array(vec![bulk("m"), bulk("1.5"), bulk("n"), bulk("inf")]),
        );
        let shown = rows(&collect);
        assert_eq!(shown[0].0, ["member", "score"]);
        assert_eq!(shown[0].1, [texts(&["m", "1.5"]), texts(&["n", "Infinity"])]);
    }

    #[test]
    fn a_scan_shows_its_items_and_says_where_to_go_on() {
        let (collect, _) = run(
            &["SCAN", "0", "MATCH", "user:*"],
            Reply::Array(vec![bulk("17"), Reply::Array(vec![bulk("user:1"), bulk("user:2")])]),
        );
        assert_eq!(
            rows(&collect),
            [(vec!["key".to_owned()], vec![texts(&["user:1"]), texts(&["user:2"])])]
        );
        assert_eq!(*collect.notices.lock().unwrap(), ["Next cursor: 17"]);
    }

    #[test]
    fn a_stream_entry_is_its_id_and_its_fields_as_json() {
        let entry = Reply::Array(vec![bulk("1-0"), Reply::Array(vec![bulk("name"), bulk("a \"b\"\n")])]);
        let (collect, _) = run(&["XRANGE", "s", "-", "+"], Reply::Array(vec![entry]));
        assert_eq!(rows(&collect)[0].1, [texts(&["1-0", "{\"name\":\"a \\\"b\\\"\\n\"}"])]);
    }

    #[test]
    fn other_arrays_are_one_row_each_and_stop_at_the_cap() {
        let items = (0..5).map(Reply::Integer).collect();
        let (collect, outcome) = run(&["LRANGE", "l", "0", "-1"], Reply::Array(items));
        assert_eq!(rows(&collect)[0].1.len(), 3);
        assert!(outcome.limited);
        let nested = Reply::Array(vec![Reply::Array(vec![bulk("a"), Reply::Nil, Reply::Integer(1)])]);
        let (collect, _) = run(&["COMMAND", "INFO", "get"], nested);
        assert_eq!(rows(&collect)[0].1, [texts(&["[a, (nil), 1]"])]);
    }
}
