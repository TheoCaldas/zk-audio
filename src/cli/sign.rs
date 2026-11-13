//! Option A: mock-sign a source.

use nova_snark::traits::Engine;

use crate::audio_parser;
use crate::bench::config::comp_size_for;
use crate::cli::bundle::{self, SignatureBundle};
use crate::cli::prompt;
use crate::cli::{newest_key, stem, Timer};
use crate::poseidon::BatchHashKind;
use crate::signer;

type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

pub fn run() -> Result<(), String> {
    println!();
    println!("--- A) mock-sign a source ---");
    println!("Signs the Poseidon commitment to a source stream.");
    println!("This is a MOCK of the signing layer: the key is generated locally,");
    println!("where the protocol assumes an attested capture device.");
    println!();

    let audio = prompt::existing_path("audio source path: ").ok_or("cancelled")?;
    let key_default = newest_key().unwrap_or_else(|| "signers/private_p256.pem".to_string());
    let key_path = prompt::text_or("private key path", &key_default).ok_or("cancelled")?;
    if !std::path::Path::new(&key_path).exists() {
        return Err(format!("no such key: {key_path} (run ./generate.keys.sh)"));
    }
    let out_dir = prompt::text_or("output directory", ".").ok_or("cancelled")?;

    // h_orig depends on both, so they are part of the signed statement.
    let bit_depth = prompt::usize_or("bit depth", 16).ok_or("cancelled")?;
    let batch_size = prompt::usize_or("batch size", 100).ok_or("cancelled")?;

    let comp_size = comp_size_for(bit_depth);
    let samples_per_step = batch_size * comp_size;

    let (samples, file_bits) = audio_parser::read_samples_signed(&audio);
    if file_bits as usize != bit_depth {
        println!(
            "note: {audio} is {file_bits}-bit but the commitment was asked for at \
             {bit_depth}-bit"
        );
    }
    // The commitment covers exactly the samples folded, so this must match
    // what the proving step will cover - otherwise the two commit to
    // different prefixes of the same file and can never agree.
    let max_folds = samples.len() / samples_per_step;
    if max_folds == 0 {
        return Err(format!(
            "{} samples is less than one step of {samples_per_step}",
            samples.len()
        ));
    }
    println!(
        "{} samples = {max_folds} folds of {samples_per_step}",
        samples.len()
    );
    let folds = loop {
        let f = prompt::usize_or("folds to commit (must equal what you will prove)", max_folds)
            .ok_or("cancelled")?;
        if f >= 1 && f <= max_folds {
            break f;
        }
        println!("  enter 1-{max_folds}");
    };
    let usable = folds * samples_per_step;
    let samples = &samples[..usable];

    let t = Timer::start();
    let h_orig = audio_parser::hex_stream_commitment::<Fq>(
        samples,
        bit_depth as u32,
        samples_per_step,
        BatchHashKind::PRODUCTION,
    );
    println!("commitment computed in {}", t.elapsed());

    let key = signer::import_key(&key_path);
    let signature = signer::sign_hash(&signer::hash_text(&h_orig), &key);

    // Derive the public half from the private key rather than asking for
    // it, so the two cannot disagree.
    let verifying = p256::ecdsa::VerifyingKey::from(&key);
    let public_key_pem = {
        use p256::pkcs8::EncodePublicKey;
        verifying
            .to_public_key_pem(Default::default())
            .map_err(|e| e.to_string())?
    };

    let out_path = format!("{}/{}.sig.json", out_dir.trim_end_matches('/'), stem(&audio));
    bundle::write(
        &out_path,
        &SignatureBundle {
            kind: SignatureBundle::KIND.to_string(),
            h_orig: h_orig.clone(),
            signature_der_hex: hex::encode(signature.to_der().as_bytes()),
            public_key_pem,
            bit_depth,
            batch_size,
            samples: usable,
            source_file: audio.clone(),
        },
    )?;

    println!();
    println!("h_orig    {h_orig}");
    println!("signature {out_path}");
    Ok(())
}
