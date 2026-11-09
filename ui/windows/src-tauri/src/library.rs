//! Saved connections for the Windows app: the core's library file in the
//! app's data folder, with secrets in Windows Credential Manager.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use paboable_core::config::Secret;
use paboable_core::{DriverError, ErrorCode, Result};
use paboable_storage::queries::SavedQueries;
use paboable_storage::{ConnectionLibrary, SecretVault};

/// The name Credential Manager lists Paboable's secrets under ("Generic
/// Credentials", target `<key>.Paboable`).
const SERVICE: &str = "Paboable";

/// Windows Credential Manager, through keyring's native backend.
pub struct CredentialVault;

fn vault_error(error: keyring::Error) -> DriverError {
    DriverError::new(ErrorCode::Io, format!("Windows Credential Manager: {error}"))
}

impl SecretVault for CredentialVault {
    fn get(&self, key: &str) -> Result<Option<Secret>> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(vault_error)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(Secret::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(vault_error(error)),
        }
    }

    fn set(&self, key: &str, secret: &Secret) -> Result<()> {
        keyring::Entry::new(SERVICE, key)
            .map_err(vault_error)?
            .set_password(secret.expose())
            .map_err(vault_error)
    }

    fn delete(&self, key: &str) -> Result<()> {
        match keyring::Entry::new(SERVICE, key)
            .map_err(vault_error)?
            .delete_credential()
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(vault_error(error)),
        }
    }
}

/// The library and its vault. One lock covers every change, so two windows
/// saving at once cannot interleave their read-modify-write.
pub struct Library {
    library: ConnectionLibrary,
    queries: SavedQueries,
    vault: CredentialVault,
    lock: Mutex<()>,
}

impl Library {
    pub fn in_folder(folder: &Path) -> Self {
        Self {
            library: ConnectionLibrary::in_folder(folder),
            queries: SavedQueries::in_folder(folder),
            vault: CredentialVault,
            lock: Mutex::new(()),
        }
    }

    /// Runs `work` with the saved queries, one change at a time.
    pub fn with_queries<T>(&self, work: impl FnOnce(&SavedQueries) -> Result<T>) -> Result<T> {
        let _guard: MutexGuard<'_, ()> = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        work(&self.queries)
    }

    /// Runs `work` with the library and vault, one change at a time.
    pub fn with<T>(&self, work: impl FnOnce(&ConnectionLibrary, &dyn SecretVault) -> Result<T>) -> Result<T> {
        let _guard: MutexGuard<'_, ()> = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        work(&self.library, &self.vault)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "writes to and removes from Windows Credential Manager"]
    fn the_vault_keeps_reads_and_forgets_a_secret() {
        let vault = CredentialVault;
        let key = format!("paboable-test/{}/password", std::process::id());
        vault.set(&key, &Secret::new("mật khẩu ✓")).unwrap();
        assert_eq!(
            vault
                .get(&key)
                .unwrap()
                .map(|secret| secret.expose().to_owned())
                .as_deref(),
            Some("mật khẩu ✓")
        );
        vault.delete(&key).unwrap();
        assert!(vault.get(&key).unwrap().is_none());
        // Deleting what is not there is not an error.
        vault.delete(&key).unwrap();
    }
}
