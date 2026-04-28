//! Utilities for ECDSA signing and verification.

use k256::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier};
use p256::ecdsa::{SigningKey, Signature, VerifyingKey};
use p256::ecdsa::signature::Signer;
use pkcs8::{DecodePrivateKey, DecodePublicKey};
use sha2::{Sha256, Digest};

/* --- DISK OPERATIONS --- */

/// Returns imported PEM (PKCS#8) P-256 private key from disk.
pub fn import_key(name: &str) -> SigningKey {
    let pem = std::fs::read_to_string(name).unwrap();
    SigningKey::from_pkcs8_pem(&pem).unwrap()
}

/// Returns imported PEM (PKCS#8) P-256 public key from disk.
pub fn import_pub_key(name: &str) -> VerifyingKey {
    let pem = std::fs::read_to_string(name).unwrap();
    VerifyingKey::from_public_key_pem(&pem).unwrap()
}

/* --- STRING OPERATIONS --- */

/// Returns signed string with key.
/// The same as `sing_hash(hash_text(text), key)`.
/// * Uses SHA256 hash and P-256 curve for signing.
pub fn sign_text(text: &str, key: &SigningKey) -> Signature {
    key.sign(text.as_bytes())
}

/// Returns SHA256 hashed string as byte vec
pub fn hash_text(text: &str) -> Vec<u8> {
    let hash = Sha256::digest(text.as_bytes());
    hash.to_vec()
}

pub fn verify_text(text: &str, sig: &Signature, key: &VerifyingKey) -> bool {
    let hash = Sha256::digest(text.as_bytes());
    key.verify_prehash(&hash, sig).is_ok()
}

/* --- HASH OPERATIONS --- */

/// Returns the signature of a pre-hash
pub fn sign_hash(hash: &Vec<u8>, key: &SigningKey) -> Signature {
    key.sign_prehash(hash).unwrap()
}

/// Verifies the signature of a pre-hash
pub fn verify_hash(hash: &Vec<u8>, sig: &Signature, key: &VerifyingKey) -> bool {
    key.verify_prehash(hash, sig).is_ok()
}

/* --- UNIT TESTS --- */

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::{SigningKey, VerifyingKey};
    use p256::elliptic_curve::rand_core::OsRng;

    const TEST_TEXT: &str = "hello zk audio";

    fn test_keys() -> (SigningKey, VerifyingKey) {
        let signing_key = SigningKey::random(&mut OsRng);
        let verify_key = VerifyingKey::from(&signing_key);
        (signing_key, verify_key)
    }

    #[test]
    fn test_hash_text_is_deterministic() {
        let h1 = hash_text(TEST_TEXT);
        let h2 = hash_text(TEST_TEXT);

        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 32);
    }

    #[test]
    fn test_hash_text_changes_with_input() {
        let h1 = hash_text("abc");
        let h2 = hash_text("abcd");

        assert_ne!(h1, h2);
    }

    #[test]
    fn test_sign_text_and_verify_text() {
        let (sk, vk) = test_keys();

        let sig = sign_text(TEST_TEXT, &sk);

        assert!(verify_text(TEST_TEXT, &sig, &vk));
    }

    #[test]
    fn test_verify_text_fails_for_modified_text() {
        let (sk, vk) = test_keys();

        let sig = sign_text(TEST_TEXT, &sk);

        assert!(!verify_text("tampered text", &sig, &vk));
    }

    #[test]
    fn test_sign_hash_and_verify_hash() {
        let (sk, vk) = test_keys();

        let hash = hash_text(TEST_TEXT);
        let sig = sign_hash(&hash, &sk);

        assert!(verify_hash(&hash, &sig, &vk));
    }

    #[test]
    fn test_verify_hash_fails_for_modified_hash() {
        let (sk, vk) = test_keys();

        let hash = hash_text(TEST_TEXT);
        let sig = sign_hash(&hash, &sk);

        let bad_hash = hash_text("different text");

        assert!(!verify_hash(&bad_hash, &sig, &vk));
    }

    #[test]
    fn test_sign_text_matches_verify_hash() {
        let (sk, vk) = test_keys();

        let sig = sign_text(TEST_TEXT, &sk);
        let hash = hash_text(TEST_TEXT);

        assert!(verify_hash(&hash, &sig, &vk));
    }

    #[test]
    fn test_sign_hash_matches_verify_text() {
        let (sk, vk) = test_keys();

        let hash = hash_text(TEST_TEXT);
        let sig = sign_hash(&hash, &sk);

        assert!(verify_text(TEST_TEXT, &sig, &vk));
    }

    #[test]
    fn test_signature_rejected_with_wrong_key() {
        let (sk1, _) = test_keys();
        let (_, vk2) = test_keys();

        let sig = sign_text(TEST_TEXT, &sk1);

        assert!(!verify_text(TEST_TEXT, &sig, &vk2));
    }
}