//! Option A: mock-sign a source.

use nova_snark::traits::Engine;

use crate::audio_parser;
use crate::bench::config::comp_size_for;
use crate::cli::bundle::{self, SignatureBundle};
use crate::cli::prompt;
use crate::cli::{newest_key, stem, Timer};
use crate::poseidon::BatchHashKind;
use crate::protocol;
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
    if samples.is_empty() {
        return Err(format!("{audio} has no samples"));
    }
    // The whole stream by default: a real signer commits to what it
    // captured, not knowing what will later be proven about it. A prefix
    // is offered because the length is part of the signed statement, so a
    // proof over fewer samples than were signed cannot be bound to the
    // commitment - and in this mock the same person usually chooses both.
    // A tail below a step boundary is silence-padded up to one rather
    // than dropped.
    let n_samples = loop {
        let n = prompt::usize_or("how many samples to sign", samples.len()).ok_or("cancelled")?;
        if n >= 1 && n <= samples.len() {
            break n;
        }
        println!("  enter 1-{}", samples.len());
    };
    let samples = &samples[..n_samples];
    let padded_samples = audio_parser::padded_len(n_samples, samples_per_step);
    let folds = padded_samples / samples_per_step;
    if padded_samples > n_samples {
        println!(
            "{n_samples} samples: committing to {folds} steps of {samples_per_step} \
             ({} padded with silence to the last step boundary)",
            padded_samples - n_samples
        );
    } else {
        println!("{n_samples} samples = {folds} steps of {samples_per_step}");
    }

    let t = Timer::start();
    let h_orig = audio_parser::hex_stream_commitment::<Fq>(
        &samples,
        bit_depth as u32,
        samples_per_step,
        BatchHashKind::PRODUCTION,
    );
    println!("commitment computed in {}", t.elapsed());

    let key = signer::import_key(&key_path);
    let signature = signer::sign_hash(&protocol::signed_digest(&h_orig, n_samples), &key);

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
            samples: n_samples,
            padded_samples,
            source_file: audio.clone(),
        },
    )?;

    println!();
    println!("h_orig    {h_orig}");
    println!("signature {out_path}");
    Ok(())
}
