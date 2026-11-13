//! Option C: verify a bundle.

use nova_snark::traits::Engine;

use crate::audio_parser;
use crate::cli::bundle::{self, ProofBundle, SignatureBundle};
use crate::cli::prompt;
use crate::cli::Timer;
use crate::circuit;
use crate::poseidon::BatchHashKind;
use crate::signer;

type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

/// One checked property and its outcome.
struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
}

pub fn run() -> Result<(), String> {
    println!();
    println!("--- C) verify a bundle ---");

    let sig_path = prompt::existing_path("signature path: ").ok_or("cancelled")?;
    let proof_path = prompt::existing_path("proof path: ").ok_or("cancelled")?;
    let audio_path = prompt::existing_path("edited audio path: ").ok_or("cancelled")?;

    let sig: SignatureBundle = bundle::read(&sig_path)?;
    let proof: ProofBundle = bundle::read(&proof_path)?;
    if sig.kind != SignatureBundle::KIND {
        return Err(format!("{sig_path} is not a {}", SignatureBundle::KIND));
    }
    if proof.kind != ProofBundle::KIND {
        return Err(format!("{proof_path} is not a {}", ProofBundle::KIND));
    }

    let mut checks = Vec::new();

    // The two bundles must describe the same run. A mismatch is a reportable
    // failure, not a crash.
    let consistent = sig.h_orig == proof.h_orig
        && sig.bit_depth == proof.bit_depth
        && sig.batch_size == proof.batch_size;
    checks.push(Check {
        name: "bundles agree",
        ok: consistent,
        detail: if consistent {
            format!("same h_orig, bd={}, B={}", proof.bit_depth, proof.batch_size)
        } else {
            "signature and proof describe different runs".to_string()
        },
    });

    // Re-derive the verifier key from the circuit. A vk shipped with a proof
    // proves nothing, since a forger supplies both.
    let r1cs = format!("artifacts/{}.r1cs", proof.circuit_name);
    if !std::path::Path::new(&r1cs).exists() {
        return Err(format!(
            "no compiled circuit for {} ({r1cs} missing). A verifier must hold \
             the circuit, since the circuit is the statement.",
            proof.circuit_name
        ));
    }
    println!();
    println!("deriving the verifier key from {} (~30s)...", proof.circuit_name);
    let t = Timer::start();
    let vk = circuit::derive_vk(&r1cs).map_err(|e| e.to_string())?;
    println!("  done in {}", t.elapsed());

    let proof_json = serde_json::to_vec(&proof.proof).map_err(|e| e.to_string())?;
    let t = Timer::start();
    let terminal = circuit::verify_compressed(&proof_json, &vk, proof.iterations, &proof.z0);
    let verify_time = t.elapsed();

    let terminal = match terminal {
        Ok(state) => {
            checks.push(Check {
                name: "proof",
                ok: true,
                detail: format!("CompressedSNARK verified, {} folds, {verify_time}", proof.iterations),
            });
            Some(state)
        }
        Err(e) => {
            checks.push(Check { name: "proof", ok: false, detail: e.to_string() });
            None
        }
    };

    // The proof's own terminal state must match the commitments the bundle
    // claims; otherwise the bundle's h_edit is unrelated to what was proven.
    if let Some(state) = &terminal {
        let claimed = state.first().map(|s| s == &proof.h_orig).unwrap_or(false)
            && state.get(1).map(|s| s == &proof.h_edit).unwrap_or(false);
        checks.push(Check {
            name: "bundle commitments",
            ok: claimed,
            detail: if claimed {
                "h_orig and h_edit match the proof's terminal state".to_string()
            } else {
                "the bundle's commitments are not the proof's terminal state".to_string()
            },
        });
    }

    // Recompute the edited stream's commitment from the audio on disk.
    let samples_per_step = proof.batch_size * proof.comp_size;
    let (samples, _bits) = audio_parser::read_samples_signed(&audio_path);
    let usable = samples.len() / samples_per_step * samples_per_step;
    let recomputed = if usable == 0 {
        None
    } else {
        Some(audio_parser::hex_stream_commitment::<Fq>(
            &samples[..usable],
            proof.bit_depth as u32,
            samples_per_step,
            BatchHashKind::for_circuit(&proof.circuit_name),
        ))
    };
    let audio_ok = recomputed.as_deref() == Some(proof.h_edit.as_str());
    checks.push(Check {
        name: "edited audio",
        ok: audio_ok,
        detail: match &recomputed {
            Some(h) if audio_ok => format!("recomputed h_edit matches ({} samples)", usable),
            Some(h) => format!("recomputed {h}, expected {}", proof.h_edit),
            None => "file is shorter than one fold".to_string(),
        },
    });

    // Signature over the committed original, with the key from the bundle.
    let sig_ok = match p256::ecdsa::VerifyingKey::from_public_key_pem(&sig.public_key_pem) {
        Ok(vk) => match hex::decode(&sig.signature_der_hex)
            .ok()
            .and_then(|d| p256::ecdsa::Signature::from_der(&d).ok())
        {
            Some(s) => signer::verify_hash(&signer::hash_text(&sig.h_orig), &s, &vk),
            None => false,
        },
        Err(_) => false,
    };
    checks.push(Check {
        name: "signature",
        ok: sig_ok,
        detail: if sig_ok {
            "valid over h_orig, using the key in the bundle".to_string()
        } else {
            "does not verify".to_string()
        },
    });

    println!();
    for c in &checks {
        println!("{:<20} {}   {}", c.name, if c.ok { "OK  " } else { "FAIL" }, c.detail);
    }

    let all = checks.iter().all(|c| c.ok);
    println!();
    if all {
        println!(
            "VERDICT: the published audio is provably {} of an original committed \
             as h_orig and signed by the key in this bundle.",
            proof.edit
        );
        println!();
        println!("That key is NOT a trust anchor: a forger ships their own key and");
        println!("their own signature and every check above still passes. Real");
        println!("verification compares the key against an out-of-band source.");
        if proof.edit == "crop" {
            println!("Note: a crop proof does not bind the excerpt to a larger original.");
        }
    } else {
        println!("VERDICT: FAILED - see the failing check above.");
    }
    Ok(())
}

use p256::pkcs8::DecodePublicKey;
