//! Saved connections.
//!
//! The library is one JSON file, `connections.json`, in the folder the app
//! names. It never holds a secret: a saved password or token goes to the
//! [`SecretVault`] the app passes in (Windows Credential Manager in the
//! Windows app), under `connection/<id>/password`.
//!
//! Every change reads the file, changes it, and writes it back whole through
//! a temporary file and a rename, so a crash leaves the old file or the new
//! one, never half of one. A file that cannot be read is never written over:
//! the change fails instead, so a damaged library is not lost to a save.
//!
//! A connection may sign in with a credential profile instead of its own
//! user and password ([`profiles`]); `resolve` fills them in from it.
//!
//! A kept secret is bound to where it is sent (`docs/decisions/0033-bound-passwords.md`):
//! the vault keeps, beside it, the engine, host, port and user (and SSH server)
//! it was kept for. The file may be shared with other Windows users, the vault
//! never is, so a connection someone else pointed elsewhere gets no kept secret.

pub mod history;
pub mod profiles;
pub mod queries;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use kemable_core::config::{ConnectionConfig, Secret, SshTunnel, Target};
use kemable_core::{DriverError, ErrorCode, Result};
use serde::{Deserialize, Serialize};

use crate::profiles::CredentialProfiles;

/// Where secrets live. The app implements it with the platform's store.
pub trait SecretVault: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Secret>>;
    fn set(&self, key: &str, secret: &Secret) -> Result<()>;
    /// Removing a key that is not there succeeds.
    fn delete(&self, key: &str) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedConnection {
    /// Empty for a connection not saved yet; `save` gives it one.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// The connection without its secret.
    pub config: ConnectionConfig,
    /// Whether the secret is kept in the vault. Without it, connecting asks.
    #[serde(default)]
    pub save_password: bool,
    /// When it was last opened, in milliseconds since the Unix epoch.
    #[serde(default)]
    pub last_used: Option<i64>,
    /// The credential profile it signs in with. Its user is copied into the
    /// config for display; its password is the profile's, never the connection's.
    #[serde(default)]
    pub profile_id: Option<String>,
    /// Its colour, one of [`CONNECTION_COLORS`], shown as a dot beside its
    /// name (design system D8). `None` is the plain grey.
    #[serde(default)]
    pub color: Option<String>,
}

/// The colours a connection can take: the names of the UI's `--conn-*` tokens.
pub const CONNECTION_COLORS: &[&str] = &["red", "orange", "green", "teal", "blue", "purple", "gray"];

#[derive(Debug, Default, Serialize, Deserialize)]
struct LibraryFile {
    version: u32,
    connections: Vec<SavedConnection>,
}

const FILE_VERSION: u32 = 1;

pub struct ConnectionLibrary {
    path: PathBuf,
    profiles: CredentialProfiles,
}

pub fn password_key(id: &str) -> String {
    format!("connection/{id}/password")
}

/// Where the vault keeps a connection's SSH password or key passphrase
/// (`docs/decisions/0024-ssh-tunnels.md`).
pub fn ssh_key(id: &str) -> String {
    format!("connection/{id}/ssh")
}

/// Where the vault keeps what the connection's password was kept for: [`secret_target`].
pub fn password_for_key(id: &str) -> String {
    format!("connection/{id}/password-for")
}

/// Where the vault keeps what the connection's SSH secret was kept for: [`ssh_target`].
pub fn ssh_for_key(id: &str) -> String {
    format!("connection/{id}/ssh-for")
}

/// Where a connection's password or token goes: the engine, the host, the port,
/// the user and the SSH server it passes through; for an HTTP engine its endpoint.
/// None for a file, which has no secret.
pub fn secret_target(config: &ConnectionConfig) -> Option<String> {
    let through = config.ssh.as_ref().map(ssh_target).unwrap_or_default();
    match &config.target {
        Target::Network { host, port, user, .. } => Some(format!(
            "{}|{}|{}|{}|{}",
            config.engine,
            host.trim().to_lowercase(),
            port.map(|port| port.to_string()).unwrap_or_default(),
            user.as_deref().unwrap_or_default(),
            through,
        )),
        Target::Http { endpoint, .. } => Some(format!("{}|{}", config.engine, endpoint.trim())),
        Target::File { .. } => None,
    }
}

/// Where an SSH password goes: the SSH server, its port and the user.
pub fn ssh_target(ssh: &SshTunnel) -> String {
    format!("ssh|{}|{}|{}", ssh.host.trim().to_lowercase(), ssh.port(), ssh.user)
}

/// The kept secrets `resolve` left out, because the saved connection no longer
/// points where they were kept for. Connecting asks for them again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Moved {
    /// The password or token.
    pub password: bool,
    /// The SSH password or key passphrase.
    pub ssh: bool,
}

enum Kept {
    None,
    Here(Secret),
    Elsewhere,
}

/// The secret under `key` when it was kept for `target`.
fn kept_for(vault: &dyn SecretVault, key: &str, for_key: &str, target: &str) -> Result<Kept> {
    let Some(secret) = vault.get(key)? else {
        return Ok(Kept::None);
    };
    match vault.get(for_key)? {
        Some(bound) if bound.expose() == target => Ok(Kept::Here(secret)),
        Some(_) => Ok(Kept::Elsewhere),
        // Kept before secrets were bound: it belongs to the connection as it is
        // saved now, and is bound to that once.
        None => {
            vault.set(for_key, &Secret::new(target))?;
            Ok(Kept::Here(secret))
        }
    }
}

/// A secret left blank in the form keeps the kept one, and follows the user's own
/// edit from `before` to `after`, but only while it was kept for `before`: an
/// edit saved over a connection someone else moved leaves it where it was.
fn follow(vault: &dyn SecretVault, key: &str, for_key: &str, before: Option<&str>, after: &str) -> Result<()> {
    let Some(before) = before else {
        return Ok(());
    };
    if before == after || vault.get(key)?.is_none() {
        return Ok(());
    }
    match vault.get(for_key)? {
        Some(bound) if bound.expose() != before => Ok(()),
        _ => vault.set(for_key, &Secret::new(after)),
    }
}

/// Clears the SSH secret a config carries, if it goes through SSH.
fn clear_ssh_secret(config: &mut ConnectionConfig) {
    if let Some(ssh) = &mut config.ssh {
        ssh.set_secret(Secret::default());
    }
}

fn io_error(what: &str, path: &Path, error: impl std::fmt::Display) -> DriverError {
    DriverError::new(ErrorCode::Io, format!("{what} {}: {error}", path.display()))
}

/// The file is there but cannot be read: it is left alone, and no change is written over it.
fn unreadable(what: &str, path: &Path, error: impl std::fmt::Display) -> DriverError {
    DriverError::new(ErrorCode::Unreadable, format!("{what} {}: {error}", path.display()))
}

/// The secret a config carries: a network password or an HTTP token.
fn secret_of(config: &ConnectionConfig) -> Option<&Secret> {
    match &config.target {
        Target::Network { password, .. } => Some(password),
        Target::Http { token, .. } => Some(token),
        Target::File { .. } => None,
    }
}

fn set_secret(config: &mut ConnectionConfig, secret: Secret) {
    match &mut config.target {
        Target::Network { password, .. } => *password = secret,
        Target::Http { token, .. } => *token = secret,
        Target::File { .. } => {}
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

impl ConnectionLibrary {
    /// A library kept in `folder`. The folder is created on the first save.
    pub fn in_folder(folder: &Path) -> Self {
        Self {
            path: folder.join("connections.json"),
            profiles: CredentialProfiles::in_folder(folder),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The credential profiles kept beside the connections.
    pub fn profiles(&self) -> &CredentialProfiles {
        &self.profiles
    }

    fn read(&self) -> Result<LibraryFile> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LibraryFile {
                    version: FILE_VERSION,
                    connections: Vec::new(),
                });
            }
            Err(error) => return Err(unreadable("cannot read", &self.path, error)),
        };
        let mut file: LibraryFile =
            serde_json::from_slice(&bytes).map_err(|error| unreadable("cannot understand", &self.path, error))?;
        // A secret in the file (written by hand, say) is never used or kept.
        for connection in &mut file.connections {
            set_secret(&mut connection.config, Secret::default());
            clear_ssh_secret(&mut connection.config);
        }
        Ok(file)
    }

    fn write(&self, file: &LibraryFile) -> Result<()> {
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder).map_err(|error| io_error("cannot create", folder, error))?;
        }
        let text = serde_json::to_vec_pretty(file).map_err(|error| DriverError::internal(error.to_string()))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(|error| io_error("cannot write", &temporary, error))?;
        std::fs::rename(&temporary, &self.path).map_err(|error| io_error("cannot replace", &self.path, error))
    }

    /// Every saved connection, in the order they were added.
    pub fn list(&self) -> Result<Vec<SavedConnection>> {
        Ok(self.read()?.connections)
    }

    /// Adds `connection`, or replaces the saved one with its id. The secret
    /// is `secret` when given, else the one in its config; it goes to the
    /// vault when `save_password` is on and is removed from it otherwise.
    pub fn save(
        &self,
        mut connection: SavedConnection,
        secret: Option<Secret>,
        vault: &dyn SecretVault,
    ) -> Result<SavedConnection> {
        if connection.name.trim().is_empty() {
            return Err(DriverError::config("a saved connection needs a name"));
        }
        let file = self.read()?;
        if connection.id.is_empty() {
            connection.id = uuid::Uuid::new_v4().to_string();
        }
        let id = connection.id.clone();
        let before = file
            .connections
            .iter()
            .find(|saved| saved.id == id)
            .map(|saved| saved.config.clone());
        // A tunnel cannot ask for its secret when it connects, so a secret
        // given is kept whatever Save password says; a blank one keeps the one
        // kept, and a connection without SSH keeps none.
        match &connection.config.ssh {
            Some(ssh) if !ssh.secret().is_empty() => {
                vault.set(&ssh_key(&id), ssh.secret())?;
                vault.set(&ssh_for_key(&id), &Secret::new(ssh_target(ssh)))?;
            }
            Some(ssh) => {
                let was = before.as_ref().and_then(|config| config.ssh.as_ref()).map(ssh_target);
                follow(
                    vault,
                    &ssh_key(&id),
                    &ssh_for_key(&id),
                    was.as_deref(),
                    &ssh_target(ssh),
                )?;
            }
            None => {
                vault.delete(&ssh_key(&id))?;
                vault.delete(&ssh_for_key(&id))?;
            }
        }
        clear_ssh_secret(&mut connection.config);
        // A colour the UI has no token for would draw nothing: it is dropped.
        connection.color = connection
            .color
            .take()
            .filter(|color| CONNECTION_COLORS.contains(&color.as_str()));
        if let Some(profile_id) = &connection.profile_id {
            // The profile holds the login: the connection keeps no secret of its own.
            let profile = self.profiles.get(profile_id)?;
            if let Target::Network { user, .. } = &mut connection.config.target {
                *user = Some(profile.user);
            }
            connection.save_password = false;
            vault.delete(&password_key(&id))?;
            vault.delete(&password_for_key(&id))?;
            set_secret(&mut connection.config, Secret::default());
            return self.put(file, connection);
        }
        let secret = secret
            .or_else(|| secret_of(&connection.config).cloned())
            .unwrap_or_default();
        let key = password_key(&id);
        let target = secret_target(&connection.config);
        if connection.save_password && !secret.is_empty() {
            vault.set(&key, &secret)?;
            if let Some(target) = &target {
                vault.set(&password_for_key(&id), &Secret::new(target.as_str()))?;
            }
        } else if !connection.save_password {
            vault.delete(&key)?;
            vault.delete(&password_for_key(&id))?;
        } else if let Some(target) = &target {
            let was = before.as_ref().and_then(secret_target);
            follow(vault, &key, &password_for_key(&id), was.as_deref(), target)?;
        }
        set_secret(&mut connection.config, Secret::default());
        self.put(file, connection)
    }

    /// Writes `connection` into `file` in its place, or at the end.
    fn put(&self, mut file: LibraryFile, connection: SavedConnection) -> Result<SavedConnection> {
        match file.connections.iter_mut().find(|saved| saved.id == connection.id) {
            Some(saved) => *saved = connection.clone(),
            None => file.connections.push(connection.clone()),
        }
        file.version = FILE_VERSION;
        self.write(&file)?;
        Ok(connection)
    }

    /// Deletes a credential profile and its password. The connections that
    /// signed in with it keep its user and ask for a password from now on.
    /// Returns how many there were.
    pub fn delete_profile(&self, id: &str, vault: &dyn SecretVault) -> Result<usize> {
        let mut file = self.read()?;
        let mut used = 0;
        for connection in &mut file.connections {
            if connection.profile_id.as_deref() == Some(id) {
                connection.profile_id = None;
                used += 1;
            }
        }
        if used > 0 {
            self.write(&file)?;
        }
        self.profiles.remove(id, vault)?;
        Ok(used)
    }

    pub fn delete(&self, id: &str, vault: &dyn SecretVault) -> Result<()> {
        let mut file = self.read()?;
        file.connections.retain(|saved| saved.id != id);
        self.write(&file)?;
        vault.delete(&ssh_key(id))?;
        vault.delete(&ssh_for_key(id))?;
        vault.delete(&password_for_key(id))?;
        vault.delete(&password_key(id))
    }

    /// The saved connection and its config with the secret filled in from the
    /// vault, when one is kept for where the connection points now, or with the
    /// login of its credential profile; and the kept secrets left out because
    /// the connection points elsewhere.
    pub fn resolve(&self, id: &str, vault: &dyn SecretVault) -> Result<(SavedConnection, ConnectionConfig, Moved)> {
        let saved = self
            .read()?
            .connections
            .into_iter()
            .find(|saved| saved.id == id)
            .ok_or_else(|| DriverError::not_found("saved connection"))?;
        let mut config = saved.config.clone();
        let mut moved = Moved::default();
        if let Some(profile_id) = &saved.profile_id {
            self.profiles.apply(profile_id, vault, &mut config)?;
        } else if saved.save_password
            && let Some(target) = secret_target(&saved.config)
        {
            match kept_for(vault, &password_key(id), &password_for_key(id), &target)? {
                Kept::Here(secret) => set_secret(&mut config, secret),
                Kept::Elsewhere => moved.password = true,
                Kept::None => {}
            }
        }
        if let Some(ssh) = &mut config.ssh {
            match kept_for(vault, &ssh_key(id), &ssh_for_key(id), &ssh_target(ssh))? {
                Kept::Here(secret) => ssh.set_secret(secret),
                Kept::Elsewhere => moved.ssh = true,
                Kept::None => {}
            }
        }
        Ok((saved, config, moved))
    }

    /// Keeps `secret` as the password of the saved connection `id`, for where it
    /// points now, when it keeps its password: the user typed it to connect after
    /// the connection was moved.
    pub fn keep_password(&self, id: &str, secret: &Secret, vault: &dyn SecretVault) -> Result<()> {
        let file = self.read()?;
        let Some(saved) = file.connections.iter().find(|saved| saved.id == id) else {
            return Err(DriverError::not_found("saved connection"));
        };
        if !saved.save_password || saved.profile_id.is_some() || secret.is_empty() {
            return Ok(());
        }
        let Some(target) = secret_target(&saved.config) else {
            return Ok(());
        };
        vault.set(&password_key(id), secret)?;
        vault.set(&password_for_key(id), &Secret::new(target))
    }

    /// Records that `id` was opened now.
    pub fn touch(&self, id: &str) -> Result<()> {
        let mut file = self.read()?;
        if let Some(saved) = file.connections.iter_mut().find(|saved| saved.id == id) {
            saved.last_used = Some(now_millis());
            self.write(&file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
