//! Storage codec boundary for future authenticated encryption at rest.
//! Currently only plaintext-v1 is implemented. Requesting encrypted-v1 fails
//! closed; it MUST NOT silently fall back to plaintext.
use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::Path};

#[allow(dead_code)] // Reserved serialized envelope for a future authenticated-at-rest codec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedEnvelopeV1 {
    pub format: String,    // "medchron-envelope-v1"
    pub algorithm: String, // future: "AES-256-GCM"
    pub key_id: String,    // reference only, NEVER store key material
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageMode {
    Plaintext,
    EncryptedUnavailable,
}

pub fn mode() -> io::Result<StorageMode> {
    match env::var("MEDCHRON_STORAGE_MODE").as_deref() {
        Ok("plaintext-v1") | Err(env::VarError::NotPresent) => Ok(StorageMode::Plaintext),
        Ok("encrypted-v1") => Ok(StorageMode::EncryptedUnavailable),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid MEDCHRON_STORAGE_MODE",
        )),
    }
}

pub fn validate_mode() -> io::Result<()> {
    match mode()? {
        StorageMode::Plaintext => Ok(()),
        StorageMode::EncryptedUnavailable => Err(io::Error::new(io::ErrorKind::Unsupported,
            "encrypted-v1 is reserved for a future authenticated encryption implementation; refusing plaintext fallback")),
    }
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    validate_mode()?;
    fs::write(path, bytes)
}

pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    validate_mode()?;
    fs::read(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_schema_is_serializable() {
        let envelope = EncryptedEnvelopeV1 {
            format: "medchron-envelope-v1".into(),
            algorithm: "AES-256-GCM".into(),
            key_id: "local-key-reference".into(),
            nonce_b64: String::new(),
            ciphertext_b64: String::new(),
        };
        assert!(serde_json::to_string(&envelope).unwrap().contains("key_id"));
    }
}
