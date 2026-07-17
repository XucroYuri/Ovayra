use spike_platform::{EnvelopeCipher, MemorySecretStore};

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
