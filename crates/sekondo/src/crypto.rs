//! AES-256-GCM encryption of tokens at rest. Stored as `nonce || ciphertext`; the character ID is
//! bound as associated data, so a ciphertext copied onto another character's row fails to decrypt.

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use secrecy::{ExposeSecret, SecretString};

use crate::config::SecretKey;

const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("stored token is too short to be valid")]
    TooShort,
    #[error("token could not be decrypted (wrong key, wrong character, or tampered data)")]
    Decrypt,
    #[error("decrypted token is not valid UTF-8")]
    NotUtf8,
}

#[derive(Clone)]
pub struct TokenCipher {
    cipher: Aes256Gcm,
}

impl std::fmt::Debug for TokenCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenCipher(<redacted>)")
    }
}

impl TokenCipher {
    pub fn new(key: &SecretKey) -> Self {
        Self {
            cipher: Aes256Gcm::new_from_slice(&key.0).expect("key is 32 bytes"),
        }
    }

    pub fn encrypt(&self, character_id: i64, token: &SecretString) -> Vec<u8> {
        let nonce_bytes: [u8; NONCE_LEN] = rand::random();
        let ciphertext = self
            .cipher
            .encrypt(
                &Nonce::from(nonce_bytes),
                Payload {
                    msg: token.expose_secret().as_bytes(),
                    aad: &character_id.to_be_bytes(),
                },
            )
            .expect("AES-GCM encryption does not fail for in-memory input");
        [nonce_bytes.as_slice(), &ciphertext].concat()
    }

    pub fn decrypt(&self, character_id: i64, stored: &[u8]) -> Result<SecretString, CryptoError> {
        if stored.len() <= NONCE_LEN {
            return Err(CryptoError::TooShort);
        }
        let (nonce, ciphertext) = stored.split_at(NONCE_LEN);
        let plaintext = self
            .cipher
            .decrypt(
                &Nonce::try_from(nonce).map_err(|_| CryptoError::TooShort)?,
                Payload {
                    msg: ciphertext,
                    aad: &character_id.to_be_bytes(),
                },
            )
            .map_err(|_| CryptoError::Decrypt)?;
        String::from_utf8(plaintext)
            .map(SecretString::from)
            .map_err(|_| CryptoError::NotUtf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher(byte: u8) -> TokenCipher {
        TokenCipher::new(&SecretKey([byte; 32]))
    }

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn round_trips() {
        let c = cipher(1);
        let stored = c.encrypt(42, &secret("refresh-token-value"));
        assert_eq!(
            c.decrypt(42, &stored).unwrap().expose_secret(),
            "refresh-token-value"
        );
    }

    #[test]
    fn stored_bytes_do_not_contain_the_plain_token() {
        let stored = cipher(1).encrypt(42, &secret("refresh-token-value"));
        assert!(!stored.windows(19).any(|w| w == b"refresh-token-value"));
    }

    #[test]
    fn every_encryption_uses_a_fresh_nonce() {
        let c = cipher(1);
        let a = c.encrypt(42, &secret("same"));
        let b = c.encrypt(42, &secret("same"));
        assert_ne!(a, b);
        assert_ne!(a[..NONCE_LEN], b[..NONCE_LEN]);
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let c = cipher(1);
        let mut stored = c.encrypt(42, &secret("token"));
        let last = stored.len() - 1;
        stored[last] ^= 0x01;
        assert!(matches!(c.decrypt(42, &stored), Err(CryptoError::Decrypt)));
        let mut stored = c.encrypt(42, &secret("token"));
        stored[0] ^= 0x01; // nonce
        assert!(matches!(c.decrypt(42, &stored), Err(CryptoError::Decrypt)));
    }

    #[test]
    fn a_ciphertext_moved_to_another_character_is_rejected() {
        let c = cipher(1);
        let stored = c.encrypt(42, &secret("token"));
        assert!(matches!(c.decrypt(43, &stored), Err(CryptoError::Decrypt)));
    }

    #[test]
    fn a_wrong_key_is_rejected() {
        let stored = cipher(1).encrypt(42, &secret("token"));
        assert!(matches!(
            cipher(2).decrypt(42, &stored),
            Err(CryptoError::Decrypt)
        ));
    }

    #[test]
    fn truncated_data_is_rejected() {
        assert!(matches!(
            cipher(1).decrypt(42, &[1, 2, 3]),
            Err(CryptoError::TooShort)
        ));
        assert!(matches!(
            cipher(1).decrypt(42, &[]),
            Err(CryptoError::TooShort)
        ));
    }

    #[test]
    fn debug_output_hides_the_key() {
        assert_eq!(format!("{:?}", cipher(1)), "TokenCipher(<redacted>)");
    }
}
