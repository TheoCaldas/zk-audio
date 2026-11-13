//! The end-to-end audio-provenance protocol.
//!
//! Ties the three previously independent pieces - the Poseidon commitment,
//! the ECDSA signature layer and the Nova proof - into one runnable
//! sequence:
//!
//! ```text
//!   CREATOR                                  VERIFIER
//!   1. commit    h_orig = Poseidon(original)
//!   2. sign      sigma = ECDSA(SHA256(h_orig))
//!   3. prove     pi, (h_orig*, h_edit*) = Nova(original, edited)
//!                        -- publishes (edited, pi, h_orig, sigma) -->
//!   4. verify proof                         check pi
//!   5. recommit                             h_edit' = Poseidon(edited)
//!   6. verify sig                           check sigma over h_orig
//! ```
//!
//! Steps 4-6 are what makes the proof mean anything. The proof alone says
//! "some pair of streams stands in the gain relation"; binding it to the
//! *published* artefact needs the verifier to recompute `h_edit'` from the
//! audio it actually holds and check it against the proof's terminal IVC
//! state. That comparison is only possible because the Rust and Circom
//! Poseidons now agree - see README.md, "Poseidon parity".
//!
//! # What this does and does not establish
//!
//! Establishes: the original was signed by the key holder (step 6), the
//! edit is exactly `k * x` clamped (step 4), and the edited audio the
//! verifier holds is the one the proof was about (step 5).
//!
//! The signature and the proof are bound by `h_orig` being the same value
//! in both, which `verify_provenance` checks. What that is worth depends
//! entirely on where the public key came from: the protocol assumes it is
//! obtained through a secure out-of-band disclosure of the source - an
//! attested capture device whose key is published by the manufacturer or a
//! trusted registry. Under that assumption a valid signature over `h_orig`
//! identifies the device that captured the original, and no in-circuit
//! signature check is needed.
//!
//! `signer.rs` is a **mock** of that layer: it generates its own keypair
//! and signs off-circuit, so the demo runs end to end. Swapping it for a
//! real device key changes nothing about the circuits or the commitments.
//! See README.md, "Trust model".

use ff::PrimeField;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

use crate::audio_parser;
use crate::circuit::{self, CaseMeta, ProofMetrics, RunConfig};
use crate::poseidon::{self, BatchHashKind};
use crate::signer;

/// Everything the creator publishes alongside the edited audio.
///
/// Deliberately does **not** include the original samples: that is the
/// point of the exercise. `h_orig` is a commitment, not the audio.
pub struct Provenance {
    /// Running Poseidon commitment to the original stream, as the circuit
    /// renders it (`0x` + 64 hex digits).
    pub h_orig: String,
    /// Running Poseidon commitment to the edited stream.
    pub h_edit: String,
    /// ECDSA-P256 signature over `SHA256(h_orig)`.
    pub signature: Signature,
    /// Timings and circuit sizes from the proving run.
    pub metrics: ProofMetrics,
}

/// Why a verification attempt failed.
///
/// Each variant is a distinct attack being caught, so they are kept apart
/// rather than collapsed into one error string.
#[derive(Debug)]
pub enum VerifyError {
    /// The recomputed commitment to the edited stream disagrees with the
    /// proof's terminal IVC state: the published audio is not what was
    /// proven.
    EditedCommitmentMismatch { expected: String, recomputed: String },
    /// The recomputed commitment to the original disagrees with the
    /// proof's terminal state.
    OriginalCommitmentMismatch { expected: String, recomputed: String },
    /// The signature does not verify over the committed original.
    BadSignature,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EditedCommitmentMismatch { expected, recomputed } => write!(
                f,
                "edited-stream commitment mismatch: proof terminal state is {expected}, \
                 recomputing from the published audio gives {recomputed}"
            ),
            Self::OriginalCommitmentMismatch { expected, recomputed } => write!(
                f,
                "original-stream commitment mismatch: proof terminal state is {expected}, \
                 recomputed {recomputed}"
            ),
            Self::BadSignature => write!(f, "signature does not verify over the committed original"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// The prover's half: commit, sign, prove.
///
/// `original` and `edited` are **signed** samples and `samples_per_step`
/// is `batch_size * comp_size` - the same three things
/// `serialize_samples_at` was given, since the commitment has to fold over
/// exactly the partition the circuit folds over.
///
/// `kind` must match how the target circuit hashes each batch; see
/// [`BatchHashKind`].
///
/// Generic over the proving field so the commitment is computed in the
/// field the circuit proves in; pass `<PallasEngine as Engine>::Scalar`
/// for this pipeline.
pub fn create_provenance<F: PrimeField>(
    original: &[i32],
    edited: &[i32],
    bit_depth: u32,
    samples_per_step: usize,
    kind: BatchHashKind,
    key: &SigningKey,
    cfg: &RunConfig,
    meta: &CaseMeta,
) -> Result<Provenance, Box<dyn std::error::Error>> {
    // 1. Commit to the original. This is the value that gets signed, so it
    //    is computed before anything else touches the audio.
    let h_orig = audio_parser::hex_stream_commitment::<F>(original, bit_depth, samples_per_step, kind);

    // 2. Sign SHA256(h_orig), not the samples: the signature then travels
    //    with a commitment rather than the audio, and stays verifiable by
    //    someone who never sees the original.
    let signature = signer::sign_hash(&signer::hash_text(&h_orig), key);

    // 3. Prove the edit relation. Nova's terminal state carries both
    //    commitments, computed in-circuit.
    let metrics = circuit::run_proof(cfg, meta)?;

    // The prover's own commitment must agree with the circuit's, or the
    // two sides have drifted and nothing downstream is meaningful. Catch
    // that here rather than letting the verifier report it as a forgery.
    if metrics.final_hash_original != h_orig {
        return Err(Box::new(VerifyError::OriginalCommitmentMismatch {
            expected: metrics.final_hash_original.clone(),
            recomputed: h_orig,
        }));
    }

    let h_edit = metrics.final_hash_edited.clone();
    Ok(Provenance { h_orig, h_edit, signature, metrics })
}

/// The verifier's half: recompute the edited commitment and check the
/// signature.
///
/// Takes the **published edited audio** and the provenance bundle. Note
/// what is absent: the original samples. The verifier never sees them, and
/// still learns that the edit was computed correctly from an original the
/// key holder signed.
///
/// The proof itself is verified inside `run_proof` (both the recursive
/// chain and, unless skipped, the compressed SNARK), so by the time a
/// `Provenance` exists its proof has already checked out. What remains -
/// and what this function does - is the binding to the artefact.
pub fn verify_provenance<F: PrimeField>(
    edited: &[i32],
    bit_depth: u32,
    samples_per_step: usize,
    kind: BatchHashKind,
    prov: &Provenance,
    pubkey: &VerifyingKey,
) -> Result<(), VerifyError> {
    // 5. Recompute the edited-stream commitment from the audio actually
    //    published. If a prover swapped the audio after proving, this is
    //    where it shows up.
    let recomputed = audio_parser::hex_stream_commitment::<F>(edited, bit_depth, samples_per_step, kind);
    if recomputed != prov.h_edit {
        return Err(VerifyError::EditedCommitmentMismatch {
            expected: prov.h_edit.clone(),
            recomputed,
        });
    }

    // 6. Check the signature over the committed original.
    if !signer::verify_hash(&signer::hash_text(&prov.h_orig), &prov.signature, pubkey) {
        return Err(VerifyError::BadSignature);
    }

    Ok(())
}

/// Convenience wrapper: the digest the signature is actually over.
///
/// Exposed so a caller can re-derive it without assuming how
/// `create_provenance` composed the two layers.
pub fn signed_digest(h_orig: &str) -> Vec<u8> {
    signer::hash_text(h_orig)
}

/// Renders a field element as the protocol transports commitments.
pub fn commitment_hex<F: PrimeField>(f: &F) -> String {
    poseidon::to_hex(f)
}
