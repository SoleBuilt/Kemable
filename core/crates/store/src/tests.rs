//! A filter on the rows held, with the sort, windows, copies and row reads
//! that go through it (`docs/decisions/0011-filtering-a-result.md`).

use kemable_core::catalog::ColumnMeta;
use kemable_core::error::ErrorCode;
use kemable_core::sql::{FilterOp, ResultFilter};
use kemable_core::value::{Value, ValueKind};

use crate::{CellRange, CellValue, ResultStore, RowSpan, SortKey, WindowReader, WindowRequest};

fn column(name: &str, kind: ValueKind) -> ColumnMeta {
    ColumnMeta {
        name: name.into(),
        type_name: String::new(),
        kind,
    }
}

fn condition(column: usize, op: FilterOp, value: &str) -> ResultFilter {
    ResultFilter {
        column,
        op,
        value: value.to_owned(),
        second: String::new(),
    }
}

fn between(column: usize, low: &str, high: &str) -> ResultFilter {
    ResultFilter {
        second: high.to_owned(),
        ..condition(column, FilterOp::Between, low)
    }
}

/// Column `column` of every row shown, as a window gives it, NULL as `NULL`.
fn shown(store: &ResultStore, column: u32) -> Vec<String> {
    let bytes = store.fetch_window(WindowRequest {
        first_row: 0,
        row_count: 1000,
        first_column: column,
        column_count: 1,
        max_chars: 100,
    });
    let window = WindowReader::new(&bytes).unwrap();
    (0..window.row_count)
        .map(|row| {
            if window.is_null(row, 0) {
                "NULL".to_owned()
            } else {
                window.text(row, 0).to_owned()
            }
        })
        .collect()
}

/// `n` an integer and `name` text, one row per pair; `None` is NULL.
fn people() -> ResultStore {
    let store = ResultStore::new(vec![column("n", ValueKind::Int), column("name", ValueKind::Text)], None);
    let rows: [(Option<i64>, Option<&str>); 6] = [
        (Some(3), Some("Alice")),
        (Some(10), Some("bob")),
        (None, Some("Carol")),
        (Some(-2), None),
        (Some(7), Some("alicia")),
        (Some(3), Some("Ñandú")),
    ];
    let values: Vec<Value> = rows
        .iter()
        .flat_map(|(n, name)| {
            [
                n.map_or(Value::Null, Value::Int),
                name.map_or(Value::Null, |name| Value::Text(name.to_owned())),
            ]
        })
        .collect();
    assert!(store.push_rows(&values));
    store.finish();
    store
}

fn names_with(store: &ResultStore, filters: &[ResultFilter], match_any: bool) -> Vec<String> {
    store.filter(filters, match_any).unwrap();
    shown(store, 1)
}

#[test]
fn each_operator_keeps_the_rows_it_names() {
    let store = people();
    let cases: [(ResultFilter, &[&str]); 16] = [
        (condition(0, FilterOp::Equals, "3"), &["Alice", "Ñandú"]),
        (condition(0, FilterOp::NotEquals, "3"), &["bob", "NULL", "alicia"]),
        (condition(0, FilterOp::Greater, "3"), &["bob", "alicia"]),
        (condition(0, FilterOp::GreaterOrEqual, "7"), &["bob", "alicia"]),
        (condition(0, FilterOp::Less, "3"), &["NULL"]),
        (condition(0, FilterOp::LessOrEqual, " 3 "), &["Alice", "NULL", "Ñandú"]),
        (condition(0, FilterOp::In, "10, -2,x"), &["bob", "NULL"]),
        (between(0, "3", "7"), &["Alice", "alicia", "Ñandú"]),
        (condition(1, FilterOp::Contains, "LIC"), &["Alice", "alicia"]),
        (condition(1, FilterOp::NotContains, "lic"), &["bob", "Carol", "Ñandú"]),
        (condition(1, FilterOp::StartsWith, "ñan"), &["Ñandú"]),
        (condition(1, FilterOp::EndsWith, "OL"), &["Carol"]),
        (condition(1, FilterOp::Equals, "alice"), &[]),
        (condition(1, FilterOp::Greater, "a"), &["bob", "alicia", "Ñandú"]),
        (condition(1, FilterOp::IsNull, ""), &["NULL"]),
        (
            condition(0, FilterOp::IsNotNull, ""),
            &["Alice", "bob", "NULL", "alicia", "Ñandú"],
        ),
    ];
    for (filter, expected) in cases {
        let op = filter.op;
        assert_eq!(names_with(&store, &[filter], false), expected, "{op:?}");
    }
}

#[test]
fn conditions_join_with_all_or_with_any() {
    let store = people();
    let filters = [
        condition(0, FilterOp::Greater, "2"),
        condition(1, FilterOp::Contains, "a"),
    ];
    assert_eq!(names_with(&store, &filters, false), ["Alice", "alicia", "Ñandú"]);
    assert_eq!(
        names_with(&store, &filters, true),
        ["Alice", "bob", "Carol", "alicia", "Ñandú"]
    );
    let state = store.state();
    assert_eq!((state.shown_rows, state.total_rows, state.filtered), (5, 6, true));
}

#[test]
fn null_matches_only_is_null_and_is_not_null() {
    let store = people();
    // As in SQL, a NULL is neither equal nor unequal to a value.
    assert_eq!(
        names_with(&store, &[condition(1, FilterOp::NotEquals, "x")], false).len(),
        5
    );
    assert!(!names_with(&store, &[condition(1, FilterOp::NotContains, "zzz")], false).contains(&"NULL".to_owned()));
    store.filter(&[condition(0, FilterOp::IsNull, "")], false).unwrap();
    assert_eq!(shown(&store, 1), ["Carol"]);
    store.filter(&[condition(0, FilterOp::NotEquals, "")], false).unwrap();
    assert_eq!(store.state().shown_rows, 0);
}

#[test]
fn a_value_that_is_not_a_number_matches_no_number() {
    let store = people();
    for op in [FilterOp::Equals, FilterOp::NotEquals, FilterOp::Greater, FilterOp::Less] {
        store.filter(&[condition(0, op, "abc")], false).unwrap();
        assert_eq!(store.state().shown_rows, 0, "{op:?}");
    }
}

#[test]
fn numbers_compare_exactly_across_kinds() {
    let store = ResultStore::new(vec![column("x", ValueKind::Decimal)], None);
    let values = [
        Value::Decimal("1.50".into()),
        Value::Int(2),
        Value::UInt(u64::MAX),
        Value::Float(1000.0),
        Value::Decimal("12345678901234567890.1".into()),
        Value::Float(f64::NAN),
    ];
    assert!(store.push_rows(&values));
    let kept = |filter: ResultFilter| {
        store.filter(&[filter], false).unwrap();
        shown(&store, 0)
    };
    assert_eq!(kept(condition(0, FilterOp::Equals, "1.5")), ["1.50"]);
    assert_eq!(kept(condition(0, FilterOp::Equals, "1e3")), ["1000"]);
    assert_eq!(
        kept(condition(0, FilterOp::Equals, "18446744073709551615")),
        ["18446744073709551615"]
    );
    assert_eq!(
        kept(condition(0, FilterOp::Greater, "12345678901234567890")),
        // NaN orders after every number (ADR 0005).
        ["18446744073709551615", "12345678901234567890.1", "NaN"]
    );
    assert_eq!(kept(between(0, "1.5", "2")), ["1.50", "2"]);
    assert_eq!(kept(condition(0, FilterOp::Equals, "NaN")), ["NaN"]);
}

#[test]
fn dates_and_timestamps_compare_by_instant() {
    let store = ResultStore::new(
        vec![
            column("day", ValueKind::Date),
            column("at", ValueKind::Timestamp),
            column("zoned", ValueKind::TimestampTz),
        ],
        None,
    );
    // 2023-11-14 22:13:20 UTC, shown at +07:00 as 2023-11-15 05:13:20.
    let instant = 1_700_000_000_000_000;
    assert!(store.push_rows(&[
        Value::Date(19_675),
        Value::Timestamp(instant),
        Value::TimestampTz {
            micros: instant,
            offset_seconds: 7 * 3600,
        },
        Value::Date(19_676),
        Value::Timestamp(instant + 500_000),
        Value::Null,
    ]));
    let count = |filter: ResultFilter| {
        store.filter(&[filter], false).unwrap();
        store.state().shown_rows
    };
    assert_eq!(count(condition(0, FilterOp::Equals, "2023-11-14")), 1);
    assert_eq!(count(condition(0, FilterOp::Greater, "2023-11-14 00:00:01")), 1);
    assert_eq!(count(condition(1, FilterOp::Equals, "2023-11-14T22:13:20.5")), 1);
    assert_eq!(count(between(1, "2023-11-14", "2023-11-15")), 2);
    // The offset typed is read; without one, the zoned value compares in UTC.
    assert_eq!(count(condition(2, FilterOp::Equals, "2023-11-15 05:13:20+07:00")), 1);
    assert_eq!(count(condition(2, FilterOp::Equals, "2023-11-14 22:13:20")), 1);
    assert_eq!(count(condition(2, FilterOp::Equals, "2023-11-15 05:13:20")), 0);
    assert_eq!(count(condition(0, FilterOp::Equals, "14/11/2023")), 0);
    // Contains reads the text the grid shows.
    assert_eq!(count(condition(2, FilterOp::Contains, "05:13")), 1);
}

#[test]
fn other_kinds_read_the_value_as_they_need_it() {
    let store = ResultStore::new(
        vec![
            column("ok", ValueKind::Bool),
            column("id", ValueKind::Uuid),
            column("blob", ValueKind::Bytes),
        ],
        None,
    );
    assert!(store.push_rows(&[
        Value::Bool(true),
        Value::Uuid([0xab; 16]),
        Value::Bytes(vec![0xca, 0xfe]),
        Value::Bool(false),
        Value::Uuid([0x01; 16]),
        Value::Bytes(vec![0x00]),
    ]));
    let count = |filter: ResultFilter| {
        store.filter(&[filter], false).unwrap();
        store.state().shown_rows
    };
    assert_eq!(count(condition(0, FilterOp::Equals, "TRUE")), 1);
    assert_eq!(count(condition(0, FilterOp::NotEquals, "yes")), 1);
    assert_eq!(
        count(condition(1, FilterOp::Equals, "ABABABAB-ABAB-ABAB-ABAB-ABABABABABAB")),
        1
    );
    assert_eq!(count(condition(2, FilterOp::Equals, "0xCAFE")), 1);
    assert_eq!(count(condition(2, FilterOp::Contains, "AF")), 1);
    assert_eq!(count(condition(2, FilterOp::StartsWith, "0x")), 2);
}

#[test]
fn a_filter_then_a_sort_sorts_only_the_rows_kept() {
    let store = people();
    store.filter(&[condition(0, FilterOp::IsNotNull, "")], false).unwrap();
    store.sort(&[SortKey {
        column: 0,
        descending: true,
    }]);
    assert_eq!(shown(&store, 0), ["10", "7", "3", "3", "-2"]);
    assert_eq!(shown(&store, 1), ["bob", "alicia", "Alice", "Ñandú", "NULL"]);

    // A new filter keeps the sort.
    store.filter(&[condition(0, FilterOp::Less, "8")], false).unwrap();
    assert_eq!(shown(&store, 0), ["7", "3", "3", "-2"]);

    // Clearing the sort shows the rows kept in arrival order.
    store.sort(&[]);
    assert_eq!(shown(&store, 0), ["3", "-2", "7", "3"]);
}

#[test]
fn clearing_the_filter_shows_every_row_and_keeps_the_sort() {
    let store = people();
    store.sort(&[SortKey {
        column: 0,
        descending: false,
    }]);
    store.filter(&[condition(0, FilterOp::Greater, "3")], false).unwrap();
    assert_eq!(shown(&store, 0), ["7", "10"]);
    store.filter(&[], false).unwrap();
    assert_eq!(shown(&store, 0), ["NULL", "-2", "3", "3", "7", "10"]);
    let state = store.state();
    assert_eq!((state.shown_rows, state.filtered), (6, false));
    store.sort(&[]);
    assert_eq!(shown(&store, 1), ["Alice", "bob", "Carol", "NULL", "alicia", "Ñandú"]);
}

#[test]
fn a_sort_by_text_or_by_several_columns_keeps_equal_rows_in_arrival_order_both_ways() {
    // Texts that share long starts, differ only past a key's width, or are
    // equal: the rows keys cannot tell apart, which the comparison settles.
    let names = [
        "khách 12345678901234567890",
        "khách 12345678901234567891",
        "https://example.com/a/very/long/path/that/goes/on/2",
        "https://example.com/a/very/long/path/that/goes/on/10",
        "item2",
        "item10",
        "item02",
        "Item1",
        "",
        "item2",
    ];
    let store = ResultStore::new(vec![column("name", ValueKind::Text), column("n", ValueKind::Int)], None);
    let values: Vec<Value> = (0..600usize)
        .flat_map(|row| {
            let name = if row % 11 == 0 {
                Value::Null
            } else {
                Value::Text(names[row * 7 % names.len()].to_owned())
            };
            [name, Value::Int(i64::try_from(row % 3).unwrap())]
        })
        .collect();
    assert!(store.push_rows(&values));
    let row = |index: u64| (store.cell_text(index, 0).unwrap(), store.cell_text(index, 1).unwrap());
    // What a stable sort by the documented order gives, from the same cells.
    let arrival: Vec<_> = (0..600).map(row).collect();
    let by_name = |a: &Option<String>, b: &Option<String>| match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(a), Some(b)) => crate::order::natural(a, b),
    };
    let number = |text: &Option<String>| text.as_deref().unwrap_or("0").parse::<i64>().unwrap();

    for descending in [false, true] {
        store.sort(&[SortKey { column: 0, descending }]);
        let mut expected = arrival.clone();
        expected.sort_by(|a, b| {
            let order = by_name(&a.0, &b.0);
            if descending { order.reverse() } else { order }
        });
        assert_eq!(
            (0..600).map(row).collect::<Vec<_>>(),
            expected,
            "descending {descending}"
        );

        // The number first, then the name the other way: the keys order the
        // first column and the comparison the second.
        store.sort(&[
            SortKey { column: 1, descending },
            SortKey {
                column: 0,
                descending: !descending,
            },
        ]);
        let mut expected = arrival.clone();
        expected.sort_by(|a, b| {
            let first = number(&a.1).cmp(&number(&b.1));
            let second = by_name(&a.0, &b.0);
            if descending {
                first.reverse().then(second)
            } else {
                first.then(second.reverse())
            }
        });
        assert_eq!(
            (0..600).map(row).collect::<Vec<_>>(),
            expected,
            "two keys, descending {descending}"
        );
    }
}

#[test]
fn a_sort_by_several_columns_orders_as_a_stable_sort_of_the_cells_would() {
    // Enough rows that a group is dealt out by the radix sort again, texts
    // that share a start longer than a key, texts that are the same, numbers
    // that end what the texts share, and NULLs.
    const ROWS: usize = 20_000;
    let store = ResultStore::new(
        vec![
            column("group", ValueKind::Int),
            column("site", ValueKind::Text),
            column("n", ValueKind::Int),
            column("at", ValueKind::Timestamp),
        ],
        None,
    );
    let mut reference: Vec<(Option<i64>, Option<String>, i64, i64)> = Vec::with_capacity(ROWS);
    for row in 0..ROWS {
        let mixed = i64::try_from(row * 7919 % 20_011).unwrap();
        let group = (row % 13 != 0).then_some(mixed % 7);
        let site = match row % 5 {
            0 => None,
            1 => Some("https://example.com/khach-hang/chi-tiet/trang-chu".to_owned()),
            2 => Some(format!("https://example.com/khach-hang/chi-tiet/{}", mixed % 400)),
            3 => Some(format!("https://example.com/khach-hang/chi-tiet/0{}/sua", mixed % 50)),
            _ => Some(format!("khách {}", mixed % 3)),
        };
        reference.push((group, site, mixed, mixed % 97 - 40));
    }
    let values: Vec<Value> = reference
        .iter()
        .flat_map(|(group, site, n, at)| {
            [
                group.map_or(Value::Null, Value::Int),
                site.clone().map_or(Value::Null, Value::Text),
                Value::Int(*n),
                Value::Timestamp(*at),
            ]
        })
        .collect();
    assert!(store.push_rows(&values));

    // NULL first; a text by the natural order.
    fn nullable<T>(a: &Option<T>, b: &Option<T>, order: impl Fn(&T, &T) -> std::cmp::Ordering) -> std::cmp::Ordering {
        match (a, b) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(a), Some(b)) => order(a, b),
        }
    }
    let by_column =
        |column: usize, a: &(Option<i64>, Option<String>, i64, i64), b: &(Option<i64>, Option<String>, i64, i64)| {
            match column {
                0 => nullable(&a.0, &b.0, i64::cmp),
                1 => nullable(&a.1, &b.1, |a, b| crate::order::natural(a, b)),
                2 => a.2.cmp(&b.2),
                _ => a.3.cmp(&b.3),
            }
        };
    let key = |column, descending| SortKey { column, descending };
    for keys in [
        vec![key(1, false)],
        vec![key(1, true)],
        vec![key(0, false), key(2, true)],
        vec![key(0, true), key(1, false)],
        vec![key(1, false), key(3, true), key(0, false)],
        vec![key(3, false), key(1, true), key(2, false)],
        vec![key(0, false), key(3, false), key(1, false), key(2, true)],
    ] {
        store.sort(&keys);
        // The order rows arrived in, sorted stably: what the store must show.
        let mut expected: Vec<usize> = (0..ROWS).collect();
        expected.sort_by(|&a, &b| {
            keys.iter()
                .map(|key| {
                    let order = by_column(key.column, &reference[a], &reference[b]);
                    if key.descending { order.reverse() } else { order }
                })
                .find(|order| order.is_ne())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (shown, &row) in expected.iter().enumerate() {
            // `n` is different in every row, so it names the row shown.
            assert_eq!(
                store.cell_text(shown as u64, 2),
                Some(Some(reference[row].2.to_string())),
                "row {shown} of the sort by {keys:?}"
            );
        }
    }
}

#[test]
fn copies_and_row_reads_go_through_the_filter() {
    let store = people();
    store.filter(&[condition(1, FilterOp::Contains, "a")], false).unwrap();
    let everything = CellRange {
        first_row: 0,
        row_count: u64::MAX,
        first_column: 0,
        column_count: 2,
    };
    assert_eq!(
        store.copy_tsv(everything, true).unwrap(),
        "n\tname\r\n3\tAlice\r\n\tCarol\r\n7\talicia\r\n3\tÑandú\r\n"
    );
    assert_eq!(
        store.row_texts(2),
        Some(vec![Some("7".to_owned()), Some("alicia".to_owned())])
    );
    assert_eq!(store.row_texts(4), None);
    assert_eq!(store.cell(1, 0), Some(CellValue::Null));
    assert_eq!(store.cell_text(3, 1), Some(Some("Ñandú".to_owned())));
    let bytes = store.fetch_window(WindowRequest {
        first_row: 3,
        row_count: 10,
        first_column: 0,
        column_count: 2,
        max_chars: 10,
    });
    let window = WindowReader::new(&bytes).unwrap();
    assert_eq!((window.total_rows, window.row_count), (4, 1));
}

#[test]
fn a_filter_names_a_column_by_index_when_names_repeat() {
    // A join of two tables, each with an `id`.
    let store = ResultStore::new(vec![column("id", ValueKind::Int), column("id", ValueKind::Int)], None);
    assert!(store.push_rows(&[Value::Int(1), Value::Int(2), Value::Int(2), Value::Int(1)]));
    store.filter(&[condition(1, FilterOp::Equals, "2")], false).unwrap();
    assert_eq!(shown(&store, 0), ["1"]);
    store.filter(&[condition(0, FilterOp::Equals, "2")], false).unwrap();
    assert_eq!(shown(&store, 0), ["2"]);
}

#[test]
fn rows_arriving_under_a_filter_are_tested_as_they_arrive() {
    let store = ResultStore::new(vec![column("n", ValueKind::Int)], None);
    assert!(store.push_rows(&[Value::Int(1), Value::Int(20)]));
    store.filter(&[condition(0, FilterOp::Greater, "5")], false).unwrap();
    assert!(store.push_rows(&[Value::Int(30), Value::Int(2), Value::Int(40)]));
    assert_eq!(shown(&store, 0), ["20", "30", "40"]);
    assert_eq!(store.state().total_rows, 5);
}

#[test]
fn a_sql_condition_or_a_bad_column_is_refused_and_the_filter_stays() {
    let store = people();
    store.filter(&[condition(0, FilterOp::Equals, "3")], false).unwrap();
    let raw = store.filter(&[condition(0, FilterOp::Raw, "n > 1")], false);
    assert_eq!(raw.err().map(|error| error.code), Some(ErrorCode::FilterRaw));
    let past = store.filter(&[condition(2, FilterOp::Equals, "1")], false);
    assert_eq!(past.err().map(|error| error.code), Some(ErrorCode::FilterInvalid));
    let empty_in = store.filter(&[condition(0, FilterOp::In, ",")], false);
    assert_eq!(empty_in.err().map(|error| error.code), Some(ErrorCode::FilterInvalid));
    assert_eq!(shown(&store, 1), ["Alice", "Ñandú"]);
}

#[test]
fn a_filter_reads_across_chunks() {
    let store = ResultStore::new(vec![column("n", ValueKind::Int)], None);
    let values: Vec<Value> = (0..crate::CHUNK_ROWS as i64 + 10).map(Value::Int).collect();
    assert!(store.push_rows(&values));
    let last = crate::CHUNK_ROWS as i64 + 5;
    store
        .filter(&[condition(0, FilterOp::In, &format!("3, {last}"))], false)
        .unwrap();
    assert_eq!(shown(&store, 0), ["3".to_owned(), last.to_string()]);
}

#[test]
#[ignore = "a timing to read by hand: cargo test -p kemable-store --release -- --ignored --nocapture"]
fn a_million_rows_filter_and_sort_in_time() {
    let store = ResultStore::new(vec![column("n", ValueKind::Int), column("name", ValueKind::Text)], None);
    let values: Vec<Value> = (0..1_000_000i64)
        .flat_map(|n| [Value::Int(n % 9973), Value::Text(format!("name {n}"))])
        .collect();
    assert!(store.push_rows(&values));
    let started = std::time::Instant::now();
    store
        .filter(
            &[
                condition(0, FilterOp::Greater, "5000"),
                condition(1, FilterOp::Contains, "7"),
            ],
            false,
        )
        .unwrap();
    let filtered = started.elapsed();
    store.sort(&[SortKey {
        column: 0,
        descending: false,
    }]);
    println!(
        "filter {filtered:?} kept {}, then sort {:?}",
        store.state().shown_rows,
        started.elapsed() - filtered
    );
}

#[test]
#[ignore = "a timing to read by hand: cargo test -p kemable-store --release -- --ignored --nocapture"]
fn two_million_rows_sort_by_a_number_a_text_and_a_time() {
    let store = ResultStore::new(
        vec![
            column("n", ValueKind::Int),
            column("name", ValueKind::Text),
            column("at", ValueKind::TimestampTz),
            column("group", ValueKind::Int),
            column("site", ValueKind::Text),
        ],
        None,
    );
    // Shuffled so no column arrives sorted: n * 7919 mod a prime visits every value once.
    let values: Vec<Value> = (0..2_000_000i64)
        .flat_map(|n| {
            let mixed = n * 7919 % 2_000_003;
            [
                Value::Int(mixed),
                Value::Text(format!("khách {mixed}")),
                Value::TimestampTz {
                    micros: 1_700_000_000_000_000 + mixed * 1_000,
                    offset_seconds: 0,
                },
                // A hundred groups of 20,000 rows each, for a sort by two columns.
                Value::Int(mixed % 100),
                // Texts alike past a key's width, which only a comparison of the cells tells apart.
                Value::Text(format!("https://example.com/khach-hang/chi-tiet/{mixed}")),
            ]
        })
        .collect();
    assert!(store.push_rows(&values));
    for (column, what) in [(0, "number"), (1, "text"), (2, "time")] {
        let started = std::time::Instant::now();
        store.sort(&[SortKey {
            column,
            descending: false,
        }]);
        println!("sort 2,000,000 rows by a {what}: {:?}", started.elapsed());
    }
    // The paths that still compare cells: several sort columns, and texts
    // that share more than a key holds.
    let key = |column| SortKey {
        column,
        descending: false,
    };
    for (keys, what) in [
        (vec![key(3), key(0)], "a group, then a number"),
        (vec![key(3), key(1)], "a group, then a text"),
        (vec![key(4)], "a text with a long shared start"),
    ] {
        let started = std::time::Instant::now();
        store.sort(&keys);
        println!("sort 2,000,000 rows by {what}: {:?}", started.elapsed());
    }
}

#[test]
fn rows_picked_one_by_one_copy_in_the_order_given_through_the_filter() {
    let store = people();
    let span = |first_row, row_count| RowSpan { first_row, row_count };
    assert_eq!(
        store
            .copy_tsv_rows(&[span(0, 1), span(2, 2), span(5, 1)], 0, 2, true)
            .unwrap(),
        "n\tname\r\n3\tAlice\r\n\tCarol\r\n-2\t\r\n3\tÑandú\r\n"
    );
    // One column, a span past the end cut to the rows there are.
    assert_eq!(
        store.copy_tsv_rows(&[span(1, 1), span(4, 99)], 1, 1, false).unwrap(),
        "bob\r\nalicia\r\nÑandú\r\n"
    );
    // Through the filter, as a rectangle copy goes.
    store.filter(&[condition(1, FilterOp::Contains, "a")], false).unwrap();
    assert_eq!(
        store.copy_tsv_rows(&[span(3, 1), span(0, 1)], 1, 1, false).unwrap(),
        "Ñandú\r\nAlice\r\n"
    );
    assert_eq!(store.copy_tsv_rows(&[], 0, 2, false).unwrap(), "");
}
