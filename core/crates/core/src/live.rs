//! What a Live tab listens to (`docs/decisions/0020-live-tab.md`): topics,
//! named the way the engine names them, read from the text a user types.
//!
//! [`crate::driver::Driver::listen`] takes the topics and streams what
//! arrives on them into a [`crate::sink::ResultSink`] as rows of one set.

use serde::{Deserialize, Serialize};

/// The kinds of topic an engine can listen to; an engine lists its own in
/// `EngineInfo::live_topics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveTopicKind {
    /// One channel by its name: PostgreSQL `LISTEN`, Redis `SUBSCRIBE`.
    Channel,
    /// Every channel a glob matches: Redis `PSUBSCRIBE`.
    Pattern,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTopic {
    pub kind: LiveTopicKind,
    pub name: String,
    /// The name was written in double quotes. An engine that folds a bare
    /// name, as PostgreSQL folds `Orders` to `orders`, keeps this one as written.
    pub exact: bool,
}

/// The topics in what a user typed: `channels` and `patterns` each hold
/// names separated by spaces, commas or new lines. A name in double quotes
/// keeps its spaces and commas, with `""` for a quote inside it. A name
/// given twice is listened to once.
pub fn topics(channels: &str, patterns: &str) -> Vec<LiveTopic> {
    let mut found: Vec<LiveTopic> = Vec::new();
    for (text, kind) in [(channels, LiveTopicKind::Channel), (patterns, LiveTopicKind::Pattern)] {
        for (name, exact) in names(text) {
            let topic = LiveTopic { kind, name, exact };
            if !found.contains(&topic) {
                found.push(topic);
            }
        }
    }
    found
}

/// Each name in `text`, and whether it was quoted.
fn names(text: &str) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&next) = chars.peek() {
        if next.is_whitespace() || next == ',' {
            chars.next();
        } else if next == '"' {
            chars.next();
            let mut name = String::new();
            while let Some(inside) = chars.next() {
                if inside != '"' {
                    name.push(inside);
                } else if chars.peek() == Some(&'"') {
                    chars.next();
                    name.push('"');
                } else {
                    break;
                }
            }
            if !name.is_empty() {
                found.push((name, true));
            }
        } else {
            let mut name = String::new();
            while let Some(&inside) = chars.peek() {
                if inside.is_whitespace() || inside == ',' {
                    break;
                }
                name.push(inside);
                chars.next();
            }
            found.push((name, false));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(name: &str, exact: bool) -> LiveTopic {
        LiveTopic {
            kind: LiveTopicKind::Channel,
            name: name.to_owned(),
            exact,
        }
    }

    #[test]
    fn names_are_split_at_spaces_commas_and_new_lines() {
        assert_eq!(
            topics("orders, jobs\n  audit,,", ""),
            vec![
                channel("orders", false),
                channel("jobs", false),
                channel("audit", false)
            ]
        );
        assert_eq!(topics("  \n , ", " "), Vec::new());
    }

    #[test]
    fn a_quoted_name_keeps_its_spaces_commas_and_quotes() {
        assert_eq!(
            topics(r#""Order Events", "a,b" "say ""hi""" plain"#, ""),
            vec![
                channel("Order Events", true),
                channel("a,b", true),
                channel(r#"say "hi""#, true),
                channel("plain", false),
            ]
        );
    }

    #[test]
    fn an_unclosed_quote_takes_the_rest_and_an_empty_one_is_nothing() {
        assert_eq!(
            topics(r#""to the end, all"#, ""),
            vec![channel("to the end, all", true)]
        );
        assert_eq!(topics(r#""" x"#, ""), vec![channel("x", false)]);
    }

    #[test]
    fn a_name_given_twice_is_one_topic_but_a_quoted_one_is_its_own() {
        assert_eq!(topics(r#"a a "a""#, ""), vec![channel("a", false), channel("a", true)]);
    }

    #[test]
    fn patterns_follow_the_channels() {
        assert_eq!(
            topics("news", "__keyspace@0__:* user:*"),
            vec![
                channel("news", false),
                LiveTopic {
                    kind: LiveTopicKind::Pattern,
                    name: "__keyspace@0__:*".to_owned(),
                    exact: false,
                },
                LiveTopic {
                    kind: LiveTopicKind::Pattern,
                    name: "user:*".to_owned(),
                    exact: false,
                },
            ]
        );
    }
}
