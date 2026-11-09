use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

use paboable_core::config::{ConnectionConfig, Secret, Target, TlsMode};
use paboable_core::{ErrorCode, Result};

use super::*;

/// A vault in memory, for tests.
#[derive(Default)]
struct MemoryVault(Mutex<HashMap<String, String>>);

impl SecretVault for MemoryVault {
    fn get(&self, key: &str) -> Result<Option<Secret>> {
        Ok(self.0.lock().unwrap().get(key).map(Secret::new))
    }

    fn set(&self, key: &str, secret: &Secret) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(key.to_owned(), secret.expose().to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

/// A folder under the system temp folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("paboable-storage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn postgres(password: &str) -> SavedConnection {
    SavedConnection {
        id: String::new(),
        name: "Local Postgres".into(),
        config: ConnectionConfig {
            engine: "postgres".into(),
            target: Target::Network {
                host: "localhost".into(),
                port: Some(5432),
                user: Some("postgres".into()),
                password: Secret::new(password),
                database: None,
                tls: TlsMode::Prefer,
            },
            options: BTreeMap::new(),
        },
        save_password: true,
        last_used: None,
    }
}

fn password_of(config: &ConnectionConfig) -> &str {
    match &config.target {
        Target::Network { password, .. } => password.expose(),
        _ => "",
    }
}

#[test]
fn a_saved_password_goes_to_the_vault_and_never_to_the_file() {
    let folder = Folder::new("vault");
    let library = ConnectionLibrary::in_folder(&folder.0);
    let vault = MemoryVault::default();

    let saved = library.save(postgres("s3cret-đ"), None, &vault).unwrap();
    assert!(!saved.id.is_empty());
    let file = std::fs::read_to_string(library.path()).unwrap();
    assert!(!file.contains("s3cret"), "{file}");

    let (_, config) = library.resolve(&saved.id, &vault).unwrap();
    assert_eq!(password_of(&config), "s3cret-đ");
}

#[test]
fn turning_off_save_password_removes_it_from_the_vault() {
    let folder = Folder::new("forget");
    let library = ConnectionLibrary::in_folder(&folder.0);
    let vault = MemoryVault::default();
    let mut saved = library.save(postgres("pw"), None, &vault).unwrap();

    saved.save_password = false;
    library.save(saved.clone(), None, &vault).unwrap();
    assert!(vault.get(&password_key(&saved.id)).unwrap().is_none());
    let (_, config) = library.resolve(&saved.id, &vault).unwrap();
    assert_eq!(password_of(&config), "");
}

#[test]
fn saving_again_keeps_the_place_and_the_kept_password() {
    let folder = Folder::new("upsert");
    let library = ConnectionLibrary::in_folder(&folder.0);
    let vault = MemoryVault::default();
    let first = library.save(postgres("one"), None, &vault).unwrap();
    library
        .save(
            SavedConnection {
                name: "Second".into(),
                ..postgres("two")
            },
            None,
            &vault,
        )
        .unwrap();

    // Editing the first without retyping its password keeps the kept one.
    let renamed = SavedConnection {
        name: "Renamed".into(),
        ..first.clone()
    };
    library.save(renamed, None, &vault).unwrap();
    let names: Vec<_> = library.list().unwrap().into_iter().map(|saved| saved.name).collect();
    assert_eq!(names, ["Renamed", "Second"]);
    let (_, config) = library.resolve(&first.id, &vault).unwrap();
    assert_eq!(password_of(&config), "one");
}

#[test]
fn deleting_removes_the_connection_and_its_secret() {
    let folder = Folder::new("delete");
    let library = ConnectionLibrary::in_folder(&folder.0);
    let vault = MemoryVault::default();
    let saved = library.save(postgres("pw"), None, &vault).unwrap();
    library.delete(&saved.id, &vault).unwrap();
    assert!(library.list().unwrap().is_empty());
    assert!(vault.get(&password_key(&saved.id)).unwrap().is_none());
    assert_eq!(
        library.resolve(&saved.id, &vault).map(|_| ()).unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[test]
fn a_file_that_cannot_be_read_is_never_written_over() {
    let folder = Folder::new("damaged");
    let library = ConnectionLibrary::in_folder(&folder.0);
    std::fs::create_dir_all(&folder.0).unwrap();
    std::fs::write(library.path(), b"{ not json").unwrap();

    let error = library.save(postgres("pw"), None, &MemoryVault::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::Io);
    assert_eq!(std::fs::read(library.path()).unwrap(), b"{ not json");
}

#[test]
fn a_missing_file_is_an_empty_library_and_names_are_required() {
    let folder = Folder::new("empty");
    let library = ConnectionLibrary::in_folder(&folder.0);
    assert!(library.list().unwrap().is_empty());
    let unnamed = SavedConnection {
        name: "  ".into(),
        ..postgres("pw")
    };
    assert_eq!(
        library.save(unnamed, None, &MemoryVault::default()).unwrap_err().code,
        ErrorCode::ConfigInvalid
    );
}

#[test]
fn touching_records_when_it_was_opened() {
    let folder = Folder::new("touch");
    let library = ConnectionLibrary::in_folder(&folder.0);
    let saved = library.save(postgres("pw"), None, &MemoryVault::default()).unwrap();
    library.touch(&saved.id).unwrap();
    assert!(library.list().unwrap()[0].last_used.is_some_and(|millis| millis > 0));
}

// --- query history ------------------------------------------------------------

use crate::history::{HistoryEntry, HistoryQuery, Outcome, QueryHistory, WITHHELD, holds_password};
use crate::queries::{SavedQueries, SavedQuery};

fn entry(statement: &str, connection: &str) -> HistoryEntry {
    HistoryEntry {
        id: 0,
        ran_at: 1,
        connection: connection.into(),
        engine: "postgres".into(),
        database: Some("demo".into()),
        statement: statement.into(),
        duration_ms: 12,
        rows: Some(3),
        outcome: Outcome::Succeeded,
        error: None,
    }
}

#[test]
fn history_comes_back_newest_first_and_filters_by_text_and_connection() {
    let history = QueryHistory::in_memory().unwrap();
    history.record(&entry("select * from orders", "Local PG")).unwrap();
    history
        .record(&entry("select * from customers where name like 'Nguyễn%'", "Local PG"))
        .unwrap();
    let last = history.record(&entry("select 1", "Local MSSQL")).unwrap();

    let all = history.search(&HistoryQuery::default()).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].id, last);

    let found = history
        .search(&HistoryQuery {
            text: "NGUYỄN".to_lowercase(),
            ..HistoryQuery::default()
        })
        .unwrap();
    assert_eq!(found.len(), 1);
    let pg = history
        .search(&HistoryQuery {
            connection: Some("Local PG".into()),
            ..HistoryQuery::default()
        })
        .unwrap();
    assert_eq!(pg.len(), 2);
    let older = history
        .search(&HistoryQuery {
            before: Some(last),
            limit: Some(1),
            ..HistoryQuery::default()
        })
        .unwrap();
    assert_eq!(older.len(), 1);
    assert!(older[0].id < last);
}

#[test]
fn history_search_treats_percent_and_underscore_as_text() {
    let history = QueryHistory::in_memory().unwrap();
    history.record(&entry("select 'a_b'", "c")).unwrap();
    history.record(&entry("select 'axb'", "c")).unwrap();
    let found = history
        .search(&HistoryQuery {
            text: "a_b".into(),
            ..HistoryQuery::default()
        })
        .unwrap();
    assert_eq!(found.len(), 1);
}

#[test]
fn a_statement_holding_a_password_is_kept_as_a_note() {
    assert!(holds_password("CREATE USER app WITH PASSWORD 'hunter2'"));
    assert!(holds_password("ALTER LOGIN sa WITH PASSWORD = 'x'"));
    assert!(!holds_password("select password_changed_at from users"));
    let history = QueryHistory::in_memory().unwrap();
    history
        .record(&entry("alter role app password 'hunter2'", "c"))
        .unwrap();
    let kept = history.search(&HistoryQuery::default()).unwrap();
    assert_eq!(kept[0].statement, WITHHELD);
}

#[test]
fn history_entries_delete_one_or_all() {
    let history = QueryHistory::in_memory().unwrap();
    let first = history.record(&entry("select 1", "c")).unwrap();
    history.record(&entry("select 2", "c")).unwrap();
    history.delete(first).unwrap();
    assert_eq!(history.len().unwrap(), 1);
    history.clear().unwrap();
    assert!(history.is_empty().unwrap());
}

#[test]
fn saved_queries_show_their_own_connection_and_the_global_ones() {
    let folder = Folder::new("queries");
    let queries = SavedQueries::in_folder(&folder.0);
    let mine = SavedQuery {
        id: String::new(),
        name: "Top customers".into(),
        sql: "select * from customers".into(),
        connection_id: Some("pg".into()),
        updated_at: 0,
    };
    let saved = queries.save(mine, 10).unwrap();
    queries
        .save(
            SavedQuery {
                name: "anywhere".into(),
                connection_id: None,
                ..saved.clone()
            }
            .with_new_id(),
            11,
        )
        .unwrap();
    queries
        .save(
            SavedQuery {
                name: "other".into(),
                connection_id: Some("ms".into()),
                ..saved.clone()
            }
            .with_new_id(),
            12,
        )
        .unwrap();

    let names: Vec<_> = queries
        .list(Some("pg"))
        .unwrap()
        .into_iter()
        .map(|query| query.name)
        .collect();
    assert_eq!(names, ["anywhere", "Top customers"]);

    queries
        .save(
            SavedQuery {
                sql: "select 2".into(),
                ..saved.clone()
            },
            20,
        )
        .unwrap();
    let edited = queries
        .list(Some("pg"))
        .unwrap()
        .into_iter()
        .find(|query| query.id == saved.id)
        .unwrap();
    assert_eq!((edited.sql.as_str(), edited.updated_at), ("select 2", 20));

    queries.delete(&saved.id).unwrap();
    assert_eq!(queries.list(Some("pg")).unwrap().len(), 1);
    assert_eq!(
        queries
            .save(
                SavedQuery {
                    name: " ".into(),
                    ..saved
                },
                0
            )
            .unwrap_err()
            .code,
        ErrorCode::ConfigInvalid
    );
}

trait WithNewId {
    fn with_new_id(self) -> Self;
}

impl WithNewId for SavedQuery {
    fn with_new_id(self) -> Self {
        SavedQuery {
            id: String::new(),
            ..self
        }
    }
}
