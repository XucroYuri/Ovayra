use spike_platform::{
    EncryptedRecord, EnvelopeCipher, EnvelopeError, MemorySecretStore, SecretStore,
};

#[test]
fn checkpoint_round_trip_never_serializes_plaintext() {
    let store = MemorySecretStore::default();
    let cipher = EnvelopeCipher::load_or_create(&store, "test-installation").unwrap();
    let plaintext = b"https://upload.invalid/session/sensitive";
    let record = cipher.seal(plaintext, b"ovayra-upload-session-v1").unwrap();
    let json = serde_json::to_vec(&record).unwrap();
    assert!(
        !json
            .windows(plaintext.len())
            .any(|window| window == plaintext)
    );
    assert_eq!(
        cipher.open(&record, b"ovayra-upload-session-v1").unwrap(),
        plaintext
    );
}

#[test]
fn tampering_is_rejected() {
    let store = MemorySecretStore::default();
    let cipher = EnvelopeCipher::load_or_create(&store, "test-installation").unwrap();
    let mut record = cipher.seal(b"secret", b"context").unwrap();
    record.ciphertext[0] ^= 1;
    assert!(cipher.open(&record, b"context").is_err());
}

#[test]
fn rejects_wrong_associated_data() {
    let store = MemorySecretStore::default();
    let cipher = EnvelopeCipher::load_or_create(&store, "test-installation").unwrap();
    let record = cipher.seal(b"secret", b"context").unwrap();

    assert!(matches!(
        cipher.open(&record, b"different-context"),
        Err(EnvelopeError::Authentication)
    ));
}

#[test]
fn ciphertext_limit_includes_the_authentication_tag() {
    let store = MemorySecretStore::default();
    let cipher = EnvelopeCipher::load_or_create(&store, "test-installation").unwrap();
    let largest_plaintext = vec![0_u8; (16 * 1024) - 16];

    let record = cipher.seal(&largest_plaintext, b"context").unwrap();
    assert_eq!(record.ciphertext.len(), 16 * 1024);
    assert_eq!(cipher.open(&record, b"context").unwrap(), largest_plaintext);
    assert!(matches!(
        cipher.seal(&vec![0_u8; 16 * 1024], b"context"),
        Err(EnvelopeError::RecordTooLarge)
    ));
}

#[test]
fn rejects_malformed_record_headers_and_oversized_ciphertext() {
    let store = MemorySecretStore::default();
    let cipher = EnvelopeCipher::load_or_create(&store, "test-installation").unwrap();
    let record = cipher.seal(b"secret", b"context").unwrap();

    let unsupported_version = EncryptedRecord {
        version: 2,
        ..record.clone()
    };
    assert!(matches!(
        cipher.open(&unsupported_version, b"context"),
        Err(EnvelopeError::UnsupportedVersion)
    ));
    let invalid_nonce = EncryptedRecord {
        nonce: vec![0; 11],
        ..record.clone()
    };
    assert!(matches!(
        cipher.open(&invalid_nonce, b"context"),
        Err(EnvelopeError::InvalidNonce)
    ));
    let oversized = EncryptedRecord {
        ciphertext: vec![0; (16 * 1024) + 1],
        ..record
    };
    assert!(matches!(
        cipher.open(&oversized, b"context"),
        Err(EnvelopeError::RecordTooLarge)
    ));
}

#[test]
fn rejects_an_invalid_stored_master_key_length() {
    let store = MemorySecretStore::default();
    store
        .set("com.ovayra.desktop", "test-installation", &[0_u8; 31])
        .unwrap();

    assert!(matches!(
        EnvelopeCipher::load_or_create(&store, "test-installation"),
        Err(EnvelopeError::InvalidKeyLength)
    ));
}

#[test]
fn uses_a_fresh_nonce_for_each_record() {
    let store = MemorySecretStore::default();
    let cipher = EnvelopeCipher::load_or_create(&store, "test-installation").unwrap();

    let first = cipher.seal(b"same plaintext", b"context").unwrap();
    let second = cipher.seal(b"same plaintext", b"context").unwrap();

    assert_ne!(first.nonce, second.nonce);
}
