use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, Generate, KeyInit, Payload},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::{SecretStore, SecretStoreError};

const MASTER_KEY_SERVICE: &str = "com.ovayra.desktop";
/// AES-GCM appends its 128-bit authentication tag to the ciphertext.
const AUTHENTICATION_TAG_BYTES: usize = 16;
/// The complete stored ciphertext, including the authentication tag, is capped at 16 KiB.
const MAX_CIPHERTEXT_BYTES: usize = 16 * 1024;
const MAX_PLAINTEXT_BYTES: usize = MAX_CIPHERTEXT_BYTES - AUTHENTICATION_TAG_BYTES;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedRecord {
    pub version: u8,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum EnvelopeError {
    #[error("secret-store operation failed")]
    SecretStore(#[from] SecretStoreError),
    #[error("stored master key has an invalid length")]
    InvalidKeyLength,
    #[error("encrypted record has an unsupported version")]
    UnsupportedVersion,
    #[error("encrypted record has an invalid nonce")]
    InvalidNonce,
    #[error("encrypted record is too large")]
    RecordTooLarge,
    #[error("encrypted record authentication failed")]
    Authentication,
}

/// AES-256-GCM envelope for small encrypted checkpoint records.
pub struct EnvelopeCipher {
    key: Zeroizing<[u8; 32]>,
}

impl EnvelopeCipher {
    /// Loads an existing envelope key or creates a new one in the supplied secret store.
    ///
    /// # Errors
    ///
    /// Returns an error when the secret store fails or has an invalid key length.
    pub fn load_or_create(store: &impl SecretStore, account: &str) -> Result<Self, EnvelopeError> {
        let key = if let Some(key) = store.get(MASTER_KEY_SERVICE, account)? {
            let key = Zeroizing::new(key);
            let key: [u8; 32] = key
                .as_slice()
                .try_into()
                .map_err(|_| EnvelopeError::InvalidKeyLength)?;
            Zeroizing::new(key)
        } else {
            let key = Zeroizing::new(<[u8; 32]>::generate());
            store.set(MASTER_KEY_SERVICE, account, key.as_slice())?;
            key
        };
        Ok(Self { key })
    }

    /// Encrypts a small record with AES-256-GCM and caller-selected associated data.
    ///
    /// # Errors
    ///
    /// Returns an error if encryption fails or the ciphertext would exceed the size limit.
    pub fn seal(
        &self,
        plaintext: &[u8],
        associated_data: &[u8],
    ) -> Result<EncryptedRecord, EnvelopeError> {
        if plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err(EnvelopeError::RecordTooLarge);
        }
        let cipher = Aes256Gcm::new_from_slice(self.key.as_slice())
            .map_err(|_| EnvelopeError::InvalidKeyLength)?;
        let nonce = <[u8; 12]>::generate();
        let ciphertext = cipher
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: associated_data,
                },
            )
            .map_err(|_| EnvelopeError::Authentication)?;
        if ciphertext.len() > MAX_CIPHERTEXT_BYTES {
            return Err(EnvelopeError::RecordTooLarge);
        }
        Ok(EncryptedRecord {
            version: 1,
            nonce: nonce.to_vec(),
            ciphertext,
        })
    }

    /// Authenticates and decrypts an encrypted record.
    ///
    /// # Errors
    ///
    /// Returns an error when the record is malformed, oversized, or fails authentication.
    pub fn open(
        &self,
        record: &EncryptedRecord,
        associated_data: &[u8],
    ) -> Result<Vec<u8>, EnvelopeError> {
        if record.version != 1 {
            return Err(EnvelopeError::UnsupportedVersion);
        }
        if record.nonce.len() != 12 {
            return Err(EnvelopeError::InvalidNonce);
        }
        if record.ciphertext.len() > MAX_CIPHERTEXT_BYTES {
            return Err(EnvelopeError::RecordTooLarge);
        }
        let cipher = Aes256Gcm::new_from_slice(self.key.as_slice())
            .map_err(|_| EnvelopeError::InvalidKeyLength)?;
        let nonce: [u8; 12] = record
            .nonce
            .as_slice()
            .try_into()
            .map_err(|_| EnvelopeError::InvalidNonce)?;
        cipher
            .decrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: &record.ciphertext,
                    aad: associated_data,
                },
            )
            .map_err(|_| EnvelopeError::Authentication)
    }
}
