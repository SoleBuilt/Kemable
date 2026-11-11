//! Rows of made-up data for a table, for Generate Rows
//! (`docs/decisions/0018-export-import-generate.md`). Each column gets values
//! that fit its type and, when its name says what it holds (email, phone,
//! name, city, ...), values that look like it. A seed makes the rows the same
//! every time, so the preview shows what runs. Columns the server fills are
//! left out; an integer key counts up from the first free number.

use crate::catalog::ColumnInfo;
use crate::import::field_literal;
use crate::sql::SqlGrammar;

/// A small, fast generator; the data only needs to look varied, not be secure.
pub struct Random(u64);

impl Random {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `low..=high`.
    pub fn range(&mut self, low: i64, high: i64) -> i64 {
        let span = u64::try_from(high.saturating_sub(low)).unwrap_or(0).saturating_add(1);
        low.saturating_add(i64::try_from(self.next_u64() % span.max(1)).unwrap_or(0))
    }

    fn pick<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        let index = usize::try_from(self.next_u64() % items.len().max(1) as u64).unwrap_or(0);
        items.get(index).copied().unwrap_or("")
    }
}

const FIRST: &[&str] = &[
    "An", "Bình", "Chi", "Dũng", "Giang", "Hà", "Hùng", "Lan", "Linh", "Minh", "Nam", "Ngọc", "Phương", "Quang",
    "Thảo", "Trang", "Tuấn", "Vy", "Anna", "David", "Emma", "James", "Laura", "Michael", "Olivia", "Peter", "Sarah",
    "Thomas",
];
const LAST: &[&str] = &[
    "Nguyễn", "Trần", "Lê", "Phạm", "Hoàng", "Huỳnh", "Phan", "Vũ", "Đặng", "Bùi", "Smith", "Johnson", "Brown",
    "Miller", "Davis", "Wilson", "Taylor", "Clark",
];
const CITIES: &[&str] = &[
    "Hà Nội",
    "Hồ Chí Minh",
    "Đà Nẵng",
    "Hải Phòng",
    "Cần Thơ",
    "Huế",
    "Nha Trang",
    "Singapore",
    "Bangkok",
    "Tokyo",
    "London",
    "Paris",
    "Berlin",
    "New York",
    "Sydney",
];
const STREETS: &[&str] = &[
    "Lê Lợi",
    "Trần Hưng Đạo",
    "Nguyễn Huệ",
    "Hai Bà Trưng",
    "Lý Thường Kiệt",
    "Main Street",
    "High Street",
    "Park Avenue",
];
const COMPANIES: &[&str] = &[
    "Phương Nam",
    "Sao Mai",
    "Hòa Bình",
    "Bình Minh",
    "Contoso",
    "Fabrikam",
    "Northwind",
    "Tailspin",
];
const WORDS: &[&str] = &[
    "alpha",
    "beta",
    "gamma",
    "delta",
    "order",
    "invoice",
    "payment",
    "report",
    "sample",
    "review",
    "update",
    "note",
    "đơn hàng",
    "thanh toán",
    "giao hàng",
    "khách",
    "báo cáo",
    "ghi chú",
];
const DOMAINS: &[&str] = &["example.com", "example.org", "example.net", "mail.example"];

/// What a column's name says it holds.
fn hint(name: &str) -> &'static str {
    let name = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| name.contains(word));
    if has(&["email", "e_mail", "mail"]) {
        "email"
    } else if has(&["phone", "mobile", "tel", "sdt", "dien_thoai", "điện thoại"]) {
        "phone"
    } else if has(&["first_name", "firstname", "given"]) {
        "first"
    } else if has(&["last_name", "lastname", "surname", "family"]) {
        "last"
    } else if has(&["city", "town", "thanh_pho", "thành phố", "tinh"]) {
        "city"
    } else if has(&["address", "street", "dia_chi", "địa chỉ"]) {
        "address"
    } else if has(&["company", "customer", "vendor", "supplier", "cong_ty", "công ty"]) {
        "company"
    } else if has(&["name", "ten", "tên", "full"]) {
        "name"
    } else if has(&["code", "sku", "ma_", "mã", "no_", "number"]) {
        "code"
    } else if has(&["url", "website", "link"]) {
        "url"
    } else {
        ""
    }
}

/// The length a character type holds (`nvarchar(20)` is 20); None for no limit.
fn text_length(type_name: &str) -> Option<usize> {
    let open = type_name.find('(')?;
    let inside = type_name.get(open + 1..type_name.find(')')?)?;
    let first = inside.split(',').next()?.trim();
    first.parse().ok()
}

fn shorten(text: String, limit: Option<usize>) -> String {
    match limit {
        Some(limit) if text.chars().count() > limit => text.chars().take(limit).collect(),
        _ => text,
    }
}

/// One made-up value for `column`, as the text a CSV field would hold, or
/// None for NULL. `row` is the row's number from 0, for keys and codes.
fn value(random: &mut Random, column: &ColumnInfo, row: u64, key_start: Option<i64>) -> Option<String> {
    let lower = column.type_name.to_ascii_lowercase();
    let base = lower.split('(').next().unwrap_or("").trim();
    if let Some(start) = key_start {
        return Some(start.saturating_add(i64::try_from(row).unwrap_or(i64::MAX)).to_string());
    }
    // A nullable column is sometimes empty, as real data is.
    if column.nullable && column.primary_key.is_none() && random.range(0, 9) == 0 {
        return None;
    }
    let text = |random: &mut Random| -> String {
        let limit = text_length(&column.type_name);
        let made = match hint(&column.name) {
            "email" => format!(
                "{}.{}{}@{}",
                random.pick(&["an", "binh", "chi", "linh", "minh", "emma", "james", "sarah"]),
                random.pick(&["nguyen", "tran", "le", "pham", "smith", "brown"]),
                random.range(1, 999),
                random.pick(DOMAINS)
            ),
            "phone" => format!("09{:08}", random.range(0, 99_999_999)),
            "first" => random.pick(FIRST).to_owned(),
            "last" => random.pick(LAST).to_owned(),
            "city" => random.pick(CITIES).to_owned(),
            "address" => format!(
                "{} {}, {}",
                random.range(1, 300),
                random.pick(STREETS),
                random.pick(CITIES)
            ),
            "company" => format!(
                "{} {}",
                random.pick(&["Công ty", "Cửa hàng", ""]),
                random.pick(COMPANIES)
            )
            .trim()
            .to_owned(),
            "name" => format!("{} {}", random.pick(LAST), random.pick(FIRST)),
            "code" => format!("{}{:05}", random.pick(&["A", "B", "C", "K", "X"]), row + 1),
            "url" => format!(
                "https://{}/{}",
                random.pick(DOMAINS),
                random.pick(WORDS).replace(' ', "-")
            ),
            _ => {
                let words = random.range(1, 4);
                (0..words).map(|_| random.pick(WORDS)).collect::<Vec<_>>().join(" ")
            }
        };
        shorten(made, limit)
    };
    Some(match base {
        "bit" | "bool" | "boolean" => if random.range(0, 1) == 1 { "1" } else { "0" }.to_owned(),
        "tinyint" => random.range(0, 255).to_string(),
        "smallint" | "int2" => random.range(1, 30_000).to_string(),
        "int" | "integer" | "int4" | "bigint" | "int8" | "mediumint" => random.range(1, 100_000).to_string(),
        "decimal" | "numeric" | "money" | "smallmoney" | "real" | "float" | "double" | "double precision"
        | "float4" | "float8" | "number" => format!("{}.{:02}", random.range(0, 99_999), random.range(0, 99)),
        "date" => date(random),
        "datetime"
        | "datetime2"
        | "smalldatetime"
        | "timestamp"
        | "timestamptz"
        | "timestamp with time zone"
        | "timestamp without time zone"
        | "datetimeoffset" => {
            format!(
                "{} {:02}:{:02}:{:02}",
                date(random),
                random.range(0, 23),
                random.range(0, 59),
                random.range(0, 59)
            )
        }
        "time" | "time without time zone" => format!(
            "{:02}:{:02}:{:02}",
            random.range(0, 23),
            random.range(0, 59),
            random.range(0, 59)
        ),
        "uniqueidentifier" | "uuid" => uuid(random),
        "json" | "jsonb" => format!("{{\"n\": {}}}", random.range(1, 1000)),
        "binary"
        | "varbinary"
        | "bytea"
        | "blob"
        | "image"
        | "timestamp(rowversion)"
        | "xml"
        | "geometry"
        | "geography" => {
            return None;
        }
        _ => text(random),
    })
}

fn date(random: &mut Random) -> String {
    format!(
        "{}-{:02}-{:02}",
        random.range(2020, 2026),
        random.range(1, 12),
        random.range(1, 28)
    )
}

fn uuid(random: &mut Random) -> String {
    let (a, b) = (random.next_u64(), random.next_u64());
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        a & 0x0FFF,
        (b >> 48) & 0x3FFF | 0x8000,
        b & 0xFFFF_FFFF_FFFF
    )
}

/// The columns a generated row fills: every column the server does not.
pub fn filled_columns(columns: &[ColumnInfo]) -> Vec<&ColumnInfo> {
    columns.iter().filter(|column| !column.generated).collect()
}

/// The single integer key column rows number from, if the table has one the
/// server does not fill.
pub fn counted_key(columns: &[ColumnInfo]) -> Option<&ColumnInfo> {
    let mut keys = columns.iter().filter(|column| column.primary_key.is_some());
    match (keys.next(), keys.next()) {
        (Some(key), None) if !key.generated => {
            let base = key.type_name.to_ascii_lowercase();
            [
                "int", "integer", "bigint", "smallint", "int4", "int8", "int2", "numeric", "decimal",
            ]
            .iter()
            .any(|word| base.split('(').next().is_some_and(|name| name.trim() == *word))
            .then_some(key)
        }
        _ => None,
    }
}

/// Rows `first..first + count` of the stream `seed` makes, as literals for
/// `columns` (the filled ones, in order). `key_start` is the counted key's
/// first number. The same seed and row give the same values.
pub fn rows(
    grammar: SqlGrammar,
    columns: &[&ColumnInfo],
    key: Option<&str>,
    key_start: i64,
    seed: u64,
    first: u64,
    count: u64,
) -> Vec<Vec<String>> {
    (first..first.saturating_add(count))
        .map(|row| {
            // Each row has its own stream, so any slice of rows can be made alone.
            let mut random = Random::new(seed ^ row.wrapping_mul(0xD6E8_FEB8_6659_FD93));
            columns
                .iter()
                .map(|column| {
                    let start = (Some(column.name.as_str()) == key).then_some(key_start);
                    match value(&mut random, column, row, start) {
                        Some(text) => field_literal(grammar, &column.type_name, &text, false),
                        None => "NULL".to_owned(),
                    }
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, type_name: &str, nullable: bool, primary_key: Option<u32>, generated: bool) -> ColumnInfo {
        ColumnInfo {
            ordinal: 0,
            name: name.into(),
            type_name: type_name.into(),
            nullable,
            default: None,
            primary_key,
            generated,
        }
    }

    #[test]
    fn rows_fit_each_columns_type_and_name_and_repeat_for_a_seed() {
        let table = [
            column("Id", "int", false, Some(1), false),
            column("Email", "nvarchar(50)", false, None, false),
            column("Code", "varchar(4)", false, None, false),
            column("Balance", "decimal(18,2)", false, None, false),
            column("Created", "datetime2", false, None, false),
            column("Twice", "int", true, None, true),
        ];
        let filled = filled_columns(&table);
        assert_eq!(filled.len(), 5, "the computed column is the server's");
        let key = counted_key(&table).map(|column| column.name.as_str());
        assert_eq!(key, Some("Id"));
        let made = rows(SqlGrammar::SqlServer, &filled, key, 101, 7, 0, 3);
        assert_eq!(made.len(), 3);
        assert_eq!(made[0][0], "101");
        assert_eq!(made[2][0], "103");
        assert!(
            made[0][1].starts_with("N'") && made[0][1].contains('@'),
            "{:?}",
            made[0]
        );
        assert!(
            made[0][2].chars().count() <= 4 + 3,
            "varchar(4) is cut to four: {}",
            made[0][2]
        );
        assert!(
            made[0][3].contains('.') && !made[0][3].starts_with('N'),
            "a decimal goes in bare: {}",
            made[0][3]
        );
        // The same seed and rows give the same values; a slice alone matches the whole.
        assert_eq!(made, rows(SqlGrammar::SqlServer, &filled, key, 101, 7, 0, 3));
        assert_eq!(made[2..], rows(SqlGrammar::SqlServer, &filled, key, 101, 7, 2, 1)[..]);
        assert_ne!(made, rows(SqlGrammar::SqlServer, &filled, key, 101, 8, 0, 3));
    }

    #[test]
    fn a_key_the_server_fills_or_a_text_key_is_not_counted() {
        let identity = [column("id", "int", false, Some(1), true)];
        assert!(counted_key(&identity).is_none());
        let text = [column("code", "nvarchar(20)", false, Some(1), false)];
        assert!(counted_key(&text).is_none());
        let pair = [
            column("a", "int", false, Some(1), false),
            column("b", "int", false, Some(2), false),
        ];
        assert!(counted_key(&pair).is_none());
    }

    #[test]
    fn uuids_and_dates_are_well_formed() {
        let mut random = Random::new(1);
        let id = uuid(&mut random);
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        let day = date(&mut random);
        assert_eq!(day.len(), 10);
        assert!(random.range(5, 5) == 5);
    }
}
