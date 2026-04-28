use zk_audio::{signer, audio_parser};

#[test]
fn test_text_sign() {
    let name = "260426";
    let priv_key = signer::import_key(&format!("signers/private_p256_{}.pem", name));
    let pub_key = signer::import_pub_key(&format!("signers/public_p256_{}.pem", name));

    let text = "Test text";
    let sig = signer::sign_text(text, &priv_key);
    assert!(signer::verify_text(text, &sig, &pub_key));
}

#[test]
fn test_audio_sign() {
    // Import keys
    let name = "260426";
    let priv_key = signer::import_key(&format!("signers/private_p256_{}.pem", name));
    let pub_key = signer::import_pub_key(&format!("signers/public_p256_{}.pem", name));

    // Load and hash audio
    let name = "test_12.16.01";
    let samples = audio_parser::read_samples(&format!("raw_audios/{}.wav", name));
    let audio_hash = audio_parser::hash_samples(&samples);

    // Sign and Verify
    let sig = signer::sign_hash(&audio_hash, &priv_key);
    assert!(signer::verify_hash(&audio_hash, &sig, &pub_key));
}