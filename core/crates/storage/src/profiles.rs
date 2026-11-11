//! Credential profiles: a user and a password kept under a name, for many
//! saved connections to sign in with (docs/decisions/0015-credential-profiles.md).
//!
//! The profiles are `profiles.json` beside the connections, written the same
//! way (whole, through a temporary file, never over a file that cannot be
//! read). The file holds the name and the user; the password goes to the
//! [`SecretVault`] under `profile/<id>/password`.

use std::path::{Path, PathBuf};

use paboable_core::config::{ConnectionConfig, Secret, Target};
use paboable_core::{DriverError, ErrorCode, Result};
use serde::{Deserialize, Serialize};

use crate::SecretVault;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialProfile {
    /// Empty for a profile not saved yet; `save` gives it one.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub user: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProfilesFile {
    version: u32,
    profiles: Vec<CredentialProfile>,
}

const FILE_VERSION: u32 = 1;

pub fn profile_password_key(id: &str) -> String {
    format!("profile/{id}/password")
}

fn io_error(what: &str, path: &Path, error: impl std::fmt::Display) -> DriverError {
    DriverError::new(ErrorCode::Io, format!("{what} {}: {error}", path.display()))
}

pub struct CredentialProfiles {
    path: PathBuf,
}

impl CredentialProfiles {
    pub fn in_folder(folder: &Path) -> Self {
        Self {
            path: folder.join("profiles.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<ProfilesFile> {
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|error| io_error("cannot understand", &self.path, error))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ProfilesFile {
                version: FILE_VERSION,
                profiles: Vec::new(),
            }),
            Err(error) => Err(io_error("cannot read", &self.path, error)),
        }
    }

    fn write(&self, file: &ProfilesFile) -> Result<()> {
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder).map_err(|error| io_error("cannot create", folder, error))?;
        }
        let text = serde_json::to_vec_pretty(file).map_err(|error| DriverError::internal(error.to_string()))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(|error| io_error("cannot write", &temporary, error))?;
        std::fs::rename(&temporary, &self.path).map_err(|error| io_error("cannot replace", &self.path, error))
    }

    /// Every profile, by name.
    pub fn list(&self) -> Result<Vec<CredentialProfile>> {
        let mut profiles = self.read()?.profiles;
        profiles.sort_by_key(|profile| profile.name.to_lowercase());
        Ok(profiles)
    }

    pub fn get(&self, id: &str) -> Result<CredentialProfile> {
        self.read()?
            .profiles
            .into_iter()
            .find(|profile| profile.id == id)
            .ok_or_else(|| DriverError::not_found("credential profile"))
    }

    /// Adds `profile`, or replaces the one with its id. Names are unique,
    /// ignoring case, since the connect form picks a profile by its name.
    /// `password`, when given, replaces the kept one; `None` keeps it.
    pub fn save(
        &self,
        mut profile: CredentialProfile,
        password: Option<Secret>,
        vault: &dyn SecretVault,
    ) -> Result<CredentialProfile> {
        profile.name = profile.name.trim().to_owned();
        profile.user = profile.user.trim().to_owned();
        if profile.name.is_empty() {
            return Err(DriverError::config("a credential profile needs a name"));
        }
        if profile.user.is_empty() {
            return Err(DriverError::config("a credential profile needs a user"));
        }
        let mut file = self.read()?;
        let taken = file
            .profiles
            .iter()
            .any(|other| other.id != profile.id && other.name.to_lowercase() == profile.name.to_lowercase());
        if taken {
            return Err(DriverError::config(format!(
                "a credential profile named {} already exists",
                profile.name
            )));
        }
        if profile.id.is_empty() {
            profile.id = uuid::Uuid::new_v4().to_string();
        }
        if let Some(password) = password {
            vault.set(&profile_password_key(&profile.id), &password)?;
        }
        match file.profiles.iter_mut().find(|saved| saved.id == profile.id) {
            Some(saved) => *saved = profile.clone(),
            None => file.profiles.push(profile.clone()),
        }
        file.version = FILE_VERSION;
        self.write(&file)?;
        Ok(profile)
    }

    /// Removes the profile and its password. The connections that used it are
    /// [`crate::ConnectionLibrary::delete_profile`]'s to change.
    pub(crate) fn remove(&self, id: &str, vault: &dyn SecretVault) -> Result<()> {
        let mut file = self.read()?;
        file.profiles.retain(|profile| profile.id != id);
        self.write(&file)?;
        vault.delete(&profile_password_key(id))
    }

    /// Signs `config` in as the profile: its user, and its kept password or
    /// none. A target without a login (a file, an HTTP token) is left alone.
    pub fn apply(&self, id: &str, vault: &dyn SecretVault, config: &mut ConnectionConfig) -> Result<()> {
        let profile = self.get(id)?;
        if let Target::Network { user, password, .. } = &mut config.target {
            *user = Some(profile.user);
            *password = vault.get(&profile_password_key(id))?.unwrap_or_default();
        }
        Ok(())
    }
}
