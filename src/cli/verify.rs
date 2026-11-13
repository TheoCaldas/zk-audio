//! Option C: verify a bundle.

use nova_snark::traits::Engine;

use crate::audio_parser;
use crate::cli::bundle::{self, ProofBundle, SignatureBundle};
use crate::cli::prompt;
use crate::cli::{fmt_secs, Timer};
use crate::circuit;
use crate::poseidon::BatchHashKind;
use crate::protocol;
use crate::signer;

type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

/// One checked property, its outcome, and what it cost to check.
struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
    secs: f64,
}

pub fn run() -> Result<(), String> {
    println!();
    println!("--- C) verify a bundle ---");

    let sig_path = prompt::existing_path("signature path: ").ok_or("cancelled")?;
    let proof_path = prompt::existing_path("proof path: ").ok_or("cancelled")?;
    let audio_path = prompt::existing_path("edited audio path: ").ok_or("cancelled")?;

    let t_load = Timer::start();
    let sig: SignatureBundle = bundle::read(&sig_path)?;
    let proof: ProofBundle = bundle::read(&proof_path)?;
    let load_secs = t_load.secs();
    if sig.kind != SignatureBundle::KIND {
        return Err(format!("{sig_path} is not a {}", SignatureBundle::KIND));
    }
    if proof.kind != ProofBundle::KIND {
        return Err(format!("{proof_path} is not a {}", ProofBundle::KIND));
    }

    let mut checks = Vec::new();

    // The two bundles must describe the same run. A mismatch is a reportable
    // failure, not a crash.
    let t_agree = Timer::start();
    let params_agree = sig.bit_depth == proof.bit_depth && sig.batch_size == proof.batch_size;
    // The length is half the signed statement, so a proof about a
    // different N is about a different statement even at the same h_orig.
    let lengths_agree = sig.samples == proof.samples;
    let consistent = sig.h_orig == proof.h_orig && params_agree && lengths_agree;
    checks.push(Check {
        name: "bundles agree",
        ok: consistent,
        detail: if consistent {
            format!(
                "same h_orig, bd={}, B={}, N={}",
                proof.bit_depth, proof.batch_size, proof.samples
            )
        } else if !lengths_agree && sig.h_orig == proof.h_orig && params_agree {
            format!(
                "same h_orig but different lengths: signature says {} samples, proof \
                 says {}. The length is signed with the commitment, so these are \
                 different statements.",
                sig.samples, proof.samples
            )
        } else if !params_agree {
            format!(
                "different partitions: signature bd={}/B={}, proof bd={}/B={}. h_orig \
                 is a fold over a partition, so these commit to different values.",
                sig.bit_depth, sig.batch_size, proof.bit_depth, proof.batch_size
            )
        } else {
            // Same partition, different h_orig. By far the most common cause
            // is a proof over a prefix of the signed stream, which the
            // current scheme cannot bind to the full commitment.
            let step = proof.batch_size * proof.comp_size;
            let sig_folds = sig.samples / step;
            let proof_folds = proof.iterations;
            if sig_folds != proof_folds {
                format!(
                    "signature covers {sig_folds} steps ({} samples), proof covers \
                     {proof_folds} ({} samples). A proof over part of a signed stream \
                     is not bound to the full commitment.",
                    sig.samples, proof.samples
                )
            } else {
                "same partition and step count, but different h_orig: the proof is \
                 about different audio than the signature."
                    .to_string()
            }
        },
        secs: load_secs + t_agree.secs(),
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
    println!("deriving the verifier key from {}...", proof.circuit_name);
    let t = Timer::start();
    let vk = circuit::derive_vk(&r1cs).map_err(|e| e.to_string())?;
    let vk_secs = t.secs();
    println!("  done in {}", fmt_secs(vk_secs));

    let proof_json = serde_json::to_vec(&proof.proof).map_err(|e| e.to_string())?;
    let t = Timer::start();
    let terminal = circuit::verify_compressed(&proof_json, &vk, proof.iterations, &proof.z0);
    let proof_secs = t.secs();

    let terminal = match terminal {
        Ok(state) => {
            checks.push(Check {
                name: "proof",
                ok: true,
                detail: format!("verified, {} steps", proof.iterations),
                secs: proof_secs,
            });
            Some(state)
        }
        Err(e) => {
            checks.push(Check {
                name: "proof",
                ok: false,
                detail: e.to_string(),
                secs: proof_secs,
            });
            None
        }
    };

    // The proof's own terminal state must match the commitments the bundle
    // claims; otherwise the bundle's h_edit is unrelated to what was proven.
    if let Some(state) = &terminal {
        let t = Timer::start();
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
            secs: t.secs(),
        });
    }

    // Recompute the edited stream's commitment from the audio on disk.
    let samples_per_step = proof.batch_size * proof.comp_size;
    let t_audio = Timer::start();
    let (samples, _bits) = audio_parser::read_samples_signed(&audio_path);
    // Length first: the commitment is over the padded stream, so a file
    // trimmed inside the last fold recommits to the same value. Only the
    // signed N rules that out.
    let length_ok = samples.len() == proof.samples;
    let recomputed = if !length_ok || samples.is_empty() {
        None
    } else {
        Some(audio_parser::hex_stream_commitment::<Fq>(
            &samples,
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
            Some(_) if audio_ok => format!(
                "recomputed h_edit matches ({} samples, padded to {})",
                proof.samples, proof.padded_samples
            ),
            Some(h) => format!("recomputed {h}, expected {}", proof.h_edit),
            None if !length_ok => format!(
                "the file has {} samples, the proof is about {}. The commitment \
                 covers the padded stream, so the length is signed separately.",
                samples.len(),
                proof.samples
            ),
            None => "the file has no samples".to_string(),
        },
        secs: t_audio.secs(),
    });

    // Signature over the committed original, with the key from the bundle.
    let t_sig = Timer::start();
    let sig_ok = match p256::ecdsa::VerifyingKey::from_public_key_pem(&sig.public_key_pem) {
        Ok(vk) => match hex::decode(&sig.signature_der_hex)
            .ok()
            .and_then(|d| p256::ecdsa::Signature::from_der(&d).ok())
        {
            Some(s) => signer::verify_hash(
                &protocol::signed_digest(&sig.h_orig, sig.samples),
                &s,
                &vk,
            ),
            None => false,
        },
        Err(_) => false,
    };
    checks.push(Check {
        name: "signature",
        ok: sig_ok,
        detail: if sig_ok {
            format!(
                "valid over (h_orig, N={}), using the key in the bundle",
                sig.samples
            )
        } else {
            "does not verify".to_string()
        },
        secs: t_sig.secs(),
    });

    println!();
    for c in &checks {
        println!(
            "{:<20} {}  {:>7}   {}",
            c.name,
            if c.ok { "OK  " } else { "FAIL" },
            fmt_secs(c.secs),
            c.detail
        );
    }

    // The key derivation is not a check - it produces what the proof check
    // consumes - but it dominates the wall clock, so leaving it out of the
    // total would understate what verifying costs.
    let checks_secs: f64 = checks.iter().map(|c| c.secs).sum();
    println!("{:<20} {:4}  {:>7}   {}", "verifier key", "", fmt_secs(vk_secs), "derived from the circuit");
    println!("{:<20} {:4}  {:>7}", "total", "", fmt_secs(checks_secs + vk_secs));

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
