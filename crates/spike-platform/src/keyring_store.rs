use std::{collections::HashMap, sync::Mutex};

use thiserror::Error;

/// Minimal boundary for binary secrets. Implementations must never use a file fallback.
pub trait SecretStore {
    /// Gets a binary secret, returning `None` when no credential exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected credential store cannot be accessed.
    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, SecretStoreError>;
    /// Stores a binary secret in the selected credential store.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected credential store rejects the write.
    fn set(&self, service: &str, account: &str, value: &[u8]) -> Result<(), SecretStoreError>;
    /// Removes a binary secret from the selected credential store.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected credential store rejects the deletion.
    fn delete(&self, service: &str, account: &str) -> Result<(), SecretStoreError>;
}

#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("the operating-system credential store is unavailable")]
    Unavailable,
    #[error("the operating-system credential store is locked or inaccessible")]
    Locked,
    #[error("the operating-system credential store rejected the request")]
    Rejected,
    #[error("in-memory secret store lock was poisoned")]
    MemoryLock,
}

/// Native OS credential store; it deliberately has no plaintext fallback.
#[derive(Debug, Default)]
pub struct OsSecretStore;

impl SecretStore for OsSecretStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
        let entry = keyring::v1::Entry::new(service, account).map_err(|error| map_error(&error))?;
        match entry.get_secret() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::v1::Error::NoEntry) => Ok(None),
            Err(error) => Err(map_error(&error)),
        }
    }

    fn set(&self, service: &str, account: &str, value: &[u8]) -> Result<(), SecretStoreError> {
        keyring::v1::Entry::new(service, account)
            .map_err(|error| map_error(&error))?
            .set_secret(value)
            .map_err(|error| map_error(&error))
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretStoreError> {
        let entry = keyring::v1::Entry::new(service, account).map_err(|error| map_error(&error))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::v1::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(&error)),
        }
    }
}

fn map_error(error: &keyring::v1::Error) -> SecretStoreError {
    match error {
        keyring::v1::Error::NoDefaultStore | keyring::v1::Error::NotSupportedByStore(_) => {
            SecretStoreError::Unavailable
        }
        keyring::v1::Error::NoStorageAccess(_) => SecretStoreError::Locked,
        _ => SecretStoreError::Rejected,
    }
}

/// Deterministic store used by envelope tests; not available from the application binary.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    values: Mutex<HashMap<(String, String), Vec<u8>>>,
}

impl SecretStore for MemorySecretStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
        let values = self
            .values
            .lock()
            .map_err(|_| SecretStoreError::MemoryLock)?;
        Ok(values
            .get(&(service.to_owned(), account.to_owned()))
            .cloned())
    }

    fn set(&self, service: &str, account: &str, value: &[u8]) -> Result<(), SecretStoreError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| SecretStoreError::MemoryLock)?;
        values.insert((service.to_owned(), account.to_owned()), value.to_vec());
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretStoreError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| SecretStoreError::MemoryLock)?;
        values.remove(&(service.to_owned(), account.to_owned()));
        Ok(())
    }
}
