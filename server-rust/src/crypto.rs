//! 加密/解密工具模块
//!
//! 使用 AES-256-GCM 加密敏感配置值（如 API Key）。
//! 加密密钥从应用的 SECRET_KEY 派生（SHA-256）。
//! 存储格式：ENC:<base64(nonce + ciphertext + tag)>

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sha2::{Digest, Sha256};
use tracing::warn;

const ENC_PREFIX: &str = "ENC:";

/// 从 SECRET_KEY 派生 AES-256 密钥
fn derive_key(secret_key: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret_key);
    hasher.update(b"acda-q-encrypt-ai-key");
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// 加密明文字符串，返回 "ENC:<base64>" 格式
pub fn encrypt(plaintext: &str, secret_key: &str) -> anyhow::Result<String> {
    let key = derive_key(secret_key.as_bytes());
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;

    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    // nonce(12) + ciphertext(含tag)
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(format!("ENC:{}", B64.encode(&combined)))
}

/// 解密 "ENC:<base64>" 格式的值，返回明文
/// 如果不是 ENC: 前缀，原样返回（兼容未加密的值）
pub fn decrypt(value: &str, secret_key: &str) -> String {
    if !value.starts_with(ENC_PREFIX) {
        return value.to_string();
    }

    let encoded = &value[ENC_PREFIX.len()..];
    let combined = match B64.decode(encoded) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to decode encrypted value: {}", e);
            return value.to_string();
        }
    };

    if combined.len() < 12 {
        warn!("Encrypted value too short");
        return value.to_string();
    }

    let key = derive_key(secret_key.as_bytes());
    let cipher = match Aes256Gcm::new_from_slice(&key) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to create cipher: {}", e);
            return value.to_string();
        }
    };

    let nonce = Nonce::from_slice(&combined[..12]);
    let ciphertext = &combined[12..];

    match cipher.decrypt(nonce, ciphertext) {
        Ok(plaintext) => String::from_utf8(plaintext).unwrap_or_else(|e| {
            warn!("Decrypted value is not valid UTF-8: {}", e);
            value.to_string()
        }),
        Err(e) => {
            warn!("Decryption failed (wrong SECRET_KEY?): {}", e);
            value.to_string()
        }
    }
}

/// 判断值是否已加密
pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(ENC_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let secret = "test-secret-key-at-least-32-chars-long-123";
        let plaintext = "sk-test-api-key-12345";

        let encrypted = encrypt(plaintext, secret).unwrap();
        assert!(encrypted.starts_with("ENC:"));
        assert_ne!(encrypted, plaintext);

        let decrypted = decrypt(&encrypted, secret);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_passthrough() {
        let secret = "test-secret-key-at-least-32-chars-long-123";
        let plaintext = "sk-plain-api-key";

        // 未加密的值原样返回
        assert_eq!(decrypt(plaintext, secret), plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let secret1 = "test-secret-key-at-least-32-chars-long-123";
        let secret2 = "wrong-secret-key-at-least-32-chars-long-99";

        let encrypted = encrypt("secret", secret1).unwrap();
        // 用错误的 key 解密会失败，返回原文
        let result = decrypt(&encrypted, secret2);
        assert_eq!(result, encrypted); // 返回 ENC:... 原文
    }
}
