#![forbid(unsafe_code)]

//! Platform-integration spike support.

mod envelope;
mod keyring_store;
mod process_group;

pub use envelope::{EncryptedRecord, EnvelopeCipher, EnvelopeError};
pub use keyring_store::{MemorySecretStore, OsSecretStore, SecretStore, SecretStoreError};
pub use process_group::{
    GroupedProcess, ProcessGroupError, ProcessIdentity, ProcessTree, ProcessTreeProbe,
};
