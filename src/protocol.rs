//! The end-to-end audio-provenance protocol.
//!
//! ```text
//!   CREATOR                                  VERIFIER
//!   1. commit    h_orig = Poseidon(pad(original))
//!   2. sign      sigma = ECDSA(SHA256(h_orig, N))
//!   3. prove     pi, (h_orig*, h_edit*) = Nova(original, edited)
//!                     -- publishes (edited, pi, h_orig, N, sigma) -->
//!   4. verify proof                         check pi
//!   5. recommit                             h_edit' = Poseidon(pad(edited))
//!   6. verify sig                           check sigma over (h_orig, N)
//! ```
//!
//! Step 5 is what binds the proof to the published artefact: the verifier
//! recomputes `h_edit'` from the audio it holds and checks it against the
//! proof's terminal IVC state.
//!
//! A stream that is not a whole number of folds is silence-padded to one,
//! so `N` is signed alongside the commitment; see `commitment_statement`.
//!
//! `signer.rs` is a mock of the signing layer, which assumes the public key
//! arrives by out-of-band disclosure from an attested capture device. See
//! README.md, "Trust model".

use ff::PrimeField;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

use crate::audio_parser;
use crate::circuit::{self, CaseMeta, ProofMetrics, RunConfig};
use crate::poseidon::{self, BatchHashKind};
use crate::signer;

/// Everything the creator publishes alongside the edited audio.
///
/// Carries no original samples: `h_orig` is a commitment, not the audio.
pub struct Provenance {
    /// Commitment to the original stream, `0x` + 64 hex digits.
    pub h_orig: String,
    /// Commitment to the edited stream.
    pub h_edit: String,
    /// Samples before padding. The commitments cover the stream extended
    /// to a fold boundary, so this is what says where the audio ends.
    pub n_samples: usize,
    /// ECDSA-P256 signature over `SHA256(commitment_statement(..))`.
    pub signature: Signature,
    /// Timings and circuit sizes from the proving run.
    pub metrics: ProofMetrics,
}

/// Why a verification attempt failed. One variant per attack caught.
#[derive(Debug)]
pub enum VerifyError {
    /// The published audio is not what was proven.
    EditedCommitmentMismatch { expected: String, recomputed: String },
    /// The committed original disagrees with the proof's terminal state.
    OriginalCommitmentMismatch { expected: String, recomputed: String },
    /// The published audio is a different length than was signed.
    LengthMismatch { signed: usize, published: usize },
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
            Self::LengthMismatch { signed, published } => write!(
                f,
                "length mismatch: the statement is about {signed} samples, the published \
                 audio has {published}"
            ),
            Self::BadSignature => write!(f, "signature does not verify over the committed original"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// The prover's half: commit, sign, prove.
///
/// Takes signed samples and `samples_per_step = batch_size * comp_size`,
/// the same arguments as `serialize_samples_at`, so the commitment folds
/// over the partition the circuit folds over. `kind` must match how the
/// target circuit hashes each batch.
///
/// `original` and `edited` are the unpadded streams; both are silence-
/// padded to a fold boundary to commit, and the signed statement carries
/// `original.len()` so a verifier can tell the audio from the padding.
/// The caller must have applied the edit to the PADDED original, since
/// the relation has to hold on the pad too and `edit(0)` is not always 0.
///
/// Instantiate `F` at the circuit's proving field:
/// `<PallasEngine as Engine>::Scalar`.
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
    // Commit first: this is the value that gets signed.
    let h_orig = audio_parser::hex_stream_commitment::<F>(original, bit_depth, samples_per_step, kind);
    let n_samples = original.len();

    // Sign the commitment, not the samples, so the signature stays
    // verifiable by someone who never sees the original. The length rides
    // along because the commitment is over the padded stream.
    let signature = signer::sign_hash(&signed_digest(&h_orig, n_samples), key);

    // Nova's terminal state carries both commitments, computed in-circuit.
    let metrics = circuit::run_proof(cfg, meta)?;

    // Catch a Rust/circuit disagreement here rather than letting the
    // verifier report it as a forgery.
    if metrics.final_hash_original != h_orig {
        return Err(Box::new(VerifyError::OriginalCommitmentMismatch {
            expected: metrics.final_hash_original.clone(),
            recomputed: h_orig,
        }));
    }

    let h_edit = metrics.final_hash_edited.clone();
    Ok(Provenance { h_orig, h_edit, n_samples, signature, metrics })
}

/// The verifier's half: recompute the edited commitment, check the
/// signature.
///
/// Takes the published audio and the bundle, never the original samples.
/// The proof is verified in `run_proof`, so what remains here is binding it
/// to the artefact.
///
/// `edited` is the unpadded published stream. Its length is checked against
/// the signed statement before anything else: the commitment is over the
/// padded stream, so a truncated or extended file within the same fold
/// would otherwise recommit to the same value.
pub fn verify_provenance<F: PrimeField>(
    edited: &[i32],
    bit_depth: u32,
    samples_per_step: usize,
    kind: BatchHashKind,
    prov: &Provenance,
    pubkey: &VerifyingKey,
) -> Result<(), VerifyError> {
    // Trimming the file inside the last fold leaves every commitment
    // unchanged, so the length is checked rather than recomputed.
    if edited.len() != prov.n_samples {
        return Err(VerifyError::LengthMismatch {
            signed: prov.n_samples,
            published: edited.len(),
        });
    }

    // Swapping the audio after proving shows up here.
    let recomputed = audio_parser::hex_stream_commitment::<F>(edited, bit_depth, samples_per_step, kind);
    if recomputed != prov.h_edit {
        return Err(VerifyError::EditedCommitmentMismatch {
            expected: prov.h_edit.clone(),
            recomputed,
        });
    }

    if !signer::verify_hash(
        &signed_digest(&prov.h_orig, prov.n_samples),
        &prov.signature,
        pubkey,
    ) {
        return Err(VerifyError::BadSignature);
    }

    Ok(())
}

/// The statement the signature is over: a commitment and the sample count
/// it is a commitment to.
///
/// The commitment covers the stream padded to a fold boundary, so `h_orig`
/// alone does not pin where the audio ends: every length in
/// `(n - samples_per_step, n]` that pads to the same boundary shares it.
/// Binding `n` here is what makes the padding non-negotiable.
///
/// Field-separated so no `(h, n)` pair can be re-read as another; `h` is
/// fixed-width hex and `n` decimal, with a separator that occurs in
/// neither.
pub fn commitment_statement(h: &str, n_samples: usize) -> String {
    format!("zk-audio/1|{h}|{n_samples}")
}

/// The digest the signature is over.
pub fn signed_digest(h_orig: &str, n_samples: usize) -> Vec<u8> {
    signer::hash_text(&commitment_statement(h_orig, n_samples))
}

/// Renders a field element as the protocol transports commitments.
pub fn commitment_hex<F: PrimeField>(f: &F) -> String {
    poseidon::to_hex(f)
}
