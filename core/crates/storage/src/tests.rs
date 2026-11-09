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
