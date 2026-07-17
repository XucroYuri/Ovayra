#![forbid(unsafe_code)]

//! Platform-integration spike support.

mod envelope;
mod keyring_store;

pub use envelope::{EncryptedRecord, EnvelopeCipher, EnvelopeError};
pub use keyring_store::{MemorySecretStore, OsSecretStore, SecretStore, SecretStoreError};
