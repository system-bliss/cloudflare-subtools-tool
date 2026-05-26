use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;

use crate::models::EncryptedSecret;

const ITERATIONS: u32 = 210_000;
const KEY_LENGTH: usize = 32;

fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; KEY_LENGTH];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, ITERATIONS, &mut key);
    key
}

pub fn encrypt_secret(secret: &str, password: &str) -> Result<EncryptedSecret, String> {
    if password.is_empty() {
        return Err("Master password is required".into());
    }

    let mut salt = [0u8; 16];
    let mut iv = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut iv);

    let key = derive_key(password, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher init: {}", e))?;
    let nonce = Nonce::from_slice(&iv);

    let ciphertext = cipher
        .encrypt(nonce, secret.as_bytes())
        .map_err(|e| format!("Encrypt: {}", e))?;

    // The ciphertext includes the auth tag appended by AES-GCM
    let tag_start = ciphertext.len().saturating_sub(16);
    let encrypted = &ciphertext[..tag_start];
    let auth_tag = &ciphertext[tag_start..];

    Ok(EncryptedSecret {
        version: 1,
        algorithm: "aes-256-gcm".into(),
        kdf: "pbkdf2-sha256".into(),
        iterations: ITERATIONS,
        salt: STANDARD.encode(&salt),
        iv: STANDARD.encode(&iv),
        ciphertext: STANDARD.encode(encrypted),
        auth_tag: STANDARD.encode(auth_tag),
    })
}

pub fn decrypt_secret(encrypted: &EncryptedSecret, password: &str) -> Result<String, String> {
    if encrypted.version != 1
        || encrypted.algorithm != "aes-256-gcm"
        || encrypted.kdf != "pbkdf2-sha256"
    {
        return Err("Unsupported encrypted secret format".into());
    }

    if password.is_empty() {
        return Err("Master password is required".into());
    }

    let salt = STANDARD.decode(&encrypted.salt).map_err(|e| format!("Salt decode: {}", e))?;
    let iv = STANDARD.decode(&encrypted.iv).map_err(|e| format!("IV decode: {}", e))?;
    let ciphertext =
        STANDARD.decode(&encrypted.ciphertext).map_err(|e| format!("Ciphertext decode: {}", e))?;
    let auth_tag =
        STANDARD.decode(&encrypted.auth_tag).map_err(|e| format!("AuthTag decode: {}", e))?;

    let key = derive_key(password, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher init: {}", e))?;

    // Recombine ciphertext + auth tag
    let mut combined = ciphertext;
    combined.extend_from_slice(&auth_tag);

    let nonce = Nonce::from_slice(&iv);
    let plaintext = cipher
        .decrypt(nonce, combined.as_ref())
        .map_err(|_| "Unable to decrypt secret".to_string())?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8: {}", e))
}
