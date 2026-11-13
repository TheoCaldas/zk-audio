//! Sequential composition of edits: one proof per stage, linked by
//! commitment equality.
//!
//! ```text
//!   orig --[f1]--> s1 --[f2]--> s2 --[f3]--> edited
//!        pi_1          pi_2          pi_3
//! ```
//!
//! Stage `k`'s edited-stream commitment is stage `k+1`'s original-stream
//! commitment, so the verifier checks `n` proofs plus `n-1` string
//! equalities. Only the first stage's `h_orig` is signed, and only the
//! last stage's `h_edit` is recomputed from the published audio; the
//! intermediate streams are never published and get no signature.
//!
//! Prove cost is the sum of the stages, including one `CompressedSNARK`
//! per stage. That is the point: this is the control a fused composite
//! circuit has to beat. See docs/edit_chains.md.

use ff::PrimeField;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

use crate::audio_parser;
use crate::bench::config::{circuit_name_for, comp_size_for, find_edit, EditCtx, EditSpec};
use crate::circuit::{self, CaseMeta, ProofMetrics, RunConfig};
use crate::poseidon::BatchHashKind;
use crate::protocol::{self, VerifyError};
use crate::signer;

/// One stage of a chain: an edit from the registry, plus the second input
/// stream for the two-input edits.
pub struct Stage {
    /// Registry name, e.g. `gain_k2`.
    pub edit: String,
    /// Stream B, for an edit whose relation takes two inputs (merge).
    /// `None` for single-input edits.
    pub stream_b: Option<Vec<i32>>,
}

impl Stage {
    pub fn new(edit: &str) -> Self {
        Self { edit: edit.to_string(), stream_b: None }
    }

    pub fn with_stream_b(edit: &str, b: Vec<i32>) -> Self {
        Self { edit: edit.to_string(), stream_b: Some(b) }
    }
}

/// What one stage produced: its two commitments and the proof's metrics.
pub struct StageProof {
    pub edit: String,
    pub circuit_name: String,
    /// Commitment to this stage's input stream, from the proof's terminal
    /// IVC state.
    pub h_in: String,
    /// Commitment to this stage's output stream, from the terminal state.
    pub h_out: String,
    /// Commitment to stream B, for a two-input stage.
    pub h_b: Option<String>,
    /// The proven sample counter, for a time-varying or cropping stage.
    pub counter: Option<String>,
    pub metrics: ProofMetrics,
}

/// Everything the creator publishes for a chain.
///
/// Carries no samples from the original or from any intermediate stream:
/// every stage contributes commitments only.
pub struct ChainProvenance {
    /// One per stage, in application order.
    pub stages: Vec<StageProof>,
    /// Commitment to the stream entering stage 0. This is what is signed.
    pub h_orig: String,
    /// Commitment to the stream leaving the last stage. The verifier
    /// recomputes this from the published audio.
    pub h_edit: String,
    /// Samples before padding, for the stream entering stage 0.
    pub n_samples: usize,
    /// Samples before padding, for the published output. Differs from
    /// `n_samples` when a stage changes the stream length.
    pub n_samples_out: usize,
    /// ECDSA-P256 over `SHA256(commitment_statement(h_orig, n_samples))`.
    pub signature: Signature,
}

impl ChainProvenance {
    /// Prove-side wall time summed over the stages, in milliseconds.
    ///
    /// The figure a fused circuit is measured against.
    pub fn total_prove_ms(&self) -> f64 {
        self.stages
            .iter()
            .map(|s| {
                s.metrics.public_params_ms
                    + s.metrics.recursive_prove_ms
                    + s.metrics.compressed_setup_ms
                    + s.metrics.compressed_prove_ms
            })
            .sum()
    }

    /// Folding time summed over the stages.
    pub fn total_recursive_prove_ms(&self) -> f64 {
        self.stages.iter().map(|s| s.metrics.recursive_prove_ms).sum()
    }

    /// Compression time summed over the stages: the toll a chain pays
    /// once per stage and a fused circuit pays once.
    pub fn total_compressed_prove_ms(&self) -> f64 {
        self.stages
            .iter()
            .map(|s| s.metrics.compressed_setup_ms + s.metrics.compressed_prove_ms)
            .sum()
    }

    /// Constraints per step summed over the stages, the static cost that
    /// fusion reduces by sharing one decompression.
    pub fn total_constraints(&self) -> usize {
        self.stages.iter().map(|s| s.metrics.constraints_primary).sum()
    }
}

/// Why a chain failed to verify. The single-proof variants live in
/// [`VerifyError`]; these are the ones only a chain can hit.
#[derive(Debug)]
pub enum ChainError {
    /// No stages were given.
    Empty,
    /// An edit name is not in the registry.
    UnknownEdit(String),
    /// The edit has no compiled circuit at this bit depth.
    UnsupportedBitDepth { edit: String, bit_depth: usize, available: Vec<usize> },
    /// A two-input stage was given no stream B, or a single-input stage
    /// was given one.
    StreamBMismatch { edit: String, expected: bool },
    /// A stage that changes the stream length appeared before the last
    /// one. Only a terminal stage may do so: an interior one would make
    /// the next stage's input a different stream than the one this
    /// stage's commitment covers at a different padding.
    NonTerminalLengthChange { edit: String, position: usize },
    /// Concat is not supported: its gated source chains commit to either
    /// side of the join separately, so no single `h_in` exists to link a
    /// preceding stage to. See docs/edit_chains.md.
    ConcatUnsupported,
    /// Stage `k`'s output commitment is not stage `k+1`'s input
    /// commitment, so the stages are not about the same stream.
    BrokenLink { at: usize, produced: String, consumed: String },
    /// A stage's Rust-recomputed commitment disagrees with its proof.
    StageCommitmentMismatch { at: usize, expected: String, recomputed: String },
    /// The proven sample counter is not the stream length it should be.
    CounterMismatch { at: usize, expected: usize, proven: String },
    /// A single-proof check failed at this stage.
    Stage { at: usize, source: VerifyError },
    /// A stage's proof run failed.
    Prove { at: usize, source: String },
}

impl std::fmt::Display for ChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "a chain needs at least one stage"),
            Self::UnknownEdit(e) => write!(f, "unknown edit '{e}'"),
            Self::UnsupportedBitDepth { edit, bit_depth, available } => write!(
                f,
                "edit '{edit}' has no circuit at bit depth {bit_depth}; available: {available:?}"
            ),
            Self::StreamBMismatch { edit, expected: true } => {
                write!(f, "edit '{edit}' takes two input streams but no stream B was given")
            }
            Self::StreamBMismatch { edit, expected: false } => {
                write!(f, "edit '{edit}' takes one input stream but a stream B was given")
            }
            Self::NonTerminalLengthChange { edit, position } => write!(
                f,
                "edit '{edit}' at position {position} changes the stream length, which is \
                 only supported as the last stage of a chain"
            ),
            Self::ConcatUnsupported => write!(
                f,
                "concat cannot be a chain stage: it commits to the two sides of the join \
                 separately, so a preceding stage's commitment matches neither"
            ),
            Self::BrokenLink { at, produced, consumed } => write!(
                f,
                "chain broken between stage {} and stage {}: stage {} proved output \
                 commitment {produced}, stage {} proved input commitment {consumed}",
                at,
                at + 1,
                at,
                at + 1
            ),
            Self::StageCommitmentMismatch { at, expected, recomputed } => write!(
                f,
                "stage {at}: proof terminal state is {expected}, recomputing from the \
                 stream gives {recomputed}"
            ),
            Self::CounterMismatch { at, expected, proven } => write!(
                f,
                "stage {at}: proven sample counter is {proven}, expected {expected}"
            ),
            Self::Stage { at, source } => write!(f, "stage {at}: {source}"),
            Self::Prove { at, source } => write!(f, "stage {at}: proving failed: {source}"),
        }
    }
}

impl std::error::Error for ChainError {}

/// Whether an edit's output stream can differ in length from its input.
///
/// Such a stage may only be last: the chain links stages by a commitment
/// over a stream padded to a fold boundary, and a length change moves
/// that boundary, so an interior one would hand the next stage a stream
/// whose padding the preceding commitment does not cover.
fn changes_length(edit: &str) -> bool {
    matches!(edit, "crop" | "concat")
}

/// How a chain runs one stage. Mirrors the single-proof arguments in
/// `protocol::create_provenance`, which every stage ultimately uses.
pub struct ChainConfig<'a> {
    pub bit_depth: usize,
    pub batch_size: usize,
    /// LFO period in samples, for a time-varying stage.
    pub period: usize,
    /// Where the serialized per-stage JSON is written. One file per
    /// stage, since the witness generator reads from disk.
    pub serialized_dir: &'a str,
    /// Tag folded into each stage's JSON filename, so concurrent runs do
    /// not share a path.
    pub tag: &'a str,
    pub skip_compressed: bool,
    pub progress: bool,
    pub audio_input: String,
}

/// Validates a stage list against the registry before any proving.
///
/// Separate from [`prove_chain`] so a caller can reject a bad chain
/// without paying for the stages that precede the bad one.
pub fn validate(stages: &[Stage], cfg: &ChainConfig) -> Result<Vec<EditSpec>, ChainError> {
    if stages.is_empty() {
        return Err(ChainError::Empty);
    }

    let mut specs = Vec::with_capacity(stages.len());
    for (i, stage) in stages.iter().enumerate() {
        if stage.edit == "concat" {
            return Err(ChainError::ConcatUnsupported);
        }

        let spec = find_edit(&stage.edit).ok_or_else(|| ChainError::UnknownEdit(stage.edit.clone()))?;

        if !spec.bit_depths.contains(&cfg.bit_depth) {
            return Err(ChainError::UnsupportedBitDepth {
                edit: stage.edit.clone(),
                bit_depth: cfg.bit_depth,
                available: spec.bit_depths.to_vec(),
            });
        }

        let wants_b = spec.apply2.is_some();
        if wants_b != stage.stream_b.is_some() {
            return Err(ChainError::StreamBMismatch {
                edit: stage.edit.clone(),
                expected: wants_b,
            });
        }

        if changes_length(&stage.edit) && i + 1 != stages.len() {
            return Err(ChainError::NonTerminalLengthChange {
                edit: stage.edit.clone(),
                position: i,
            });
        }

        specs.push(spec);
    }

    Ok(specs)
}

/// The prover's half: apply and prove each stage in turn, then sign the
/// first stage's input commitment.
///
/// `original` is the unpadded input stream. Each stage runs on the stream
/// padded to a fold boundary, because the relation has to hold on the pad
/// too, and hands the padded result to the next stage - so a stage sees
/// exactly the samples the preceding stage's commitment covers.
///
/// Instantiate `F` at the circuit's proving field:
/// `<PallasEngine as Engine>::Scalar`.
pub fn prove_chain<F: PrimeField>(
    original: &[i32],
    stages: &[Stage],
    cfg: &ChainConfig,
    key: &SigningKey,
) -> Result<ChainProvenance, ChainError> {
    let specs = validate(stages, cfg)?;

    let comp_size = comp_size_for(cfg.bit_depth);
    let samples_per_step = cfg.batch_size * comp_size;

    let n_samples = original.len();
    // The input stream, padded: this is what stage 0's commitment covers
    // and what stage 0's circuit folds.
    let mut current = audio_parser::pad_to_step(original, samples_per_step);

    let mut proofs = Vec::with_capacity(stages.len());

    for (i, (stage, spec)) in stages.iter().zip(&specs).enumerate() {
        let ctx = EditCtx {
            bit_depth: cfg.bit_depth,
            batch_size: cfg.batch_size,
            start_index: 0,
            comp_size,
            period: cfg.period,
        };

        // Stream B is padded to the same partition as the primary input,
        // so both sides of a two-input relation fold over equal-length
        // streams.
        let stream_b = stage
            .stream_b
            .as_ref()
            .map(|b| audio_parser::pad_to_step(b, samples_per_step));

        let next = match (spec.apply2, &stream_b) {
            (Some(apply2), Some(b)) => apply2(&current, b, &ctx),
            _ => (spec.apply)(&current, &ctx),
        };

        let circuit_name =
            circuit_name_for(spec.circuit_prefix, spec.factor, cfg.bit_depth, cfg.batch_size);
        let hash_kind = BatchHashKind::for_circuit(&circuit_name);

        let json_path = format!(
            "{}/_chain_{}_s{}_{}_N{}.json",
            cfg.serialized_dir,
            cfg.tag,
            i,
            circuit_name,
            current.len()
        );

        match &stream_b {
            Some(b) => audio_parser::serialize_merge_at(
                &current,
                b,
                &next,
                samples_per_step,
                cfg.bit_depth as u32,
                &json_path,
            ),
            None => audio_parser::serialize_samples_at(
                &current,
                &next,
                samples_per_step,
                cfg.bit_depth as u32,
                &json_path,
            ),
        }

        let r1cs = format!("artifacts/{circuit_name}.r1cs");
        let wasm = format!("artifacts/{circuit_name}_js/{circuit_name}.wasm");

        let run_cfg = RunConfig {
            circuit_filepath: &r1cs,
            witness_gen_filepath: &wasm,
            json_input_filepath: &json_path,
            skip_compressed: cfg.skip_compressed,
            verbose: false,
            progress: cfg.progress,
            multi_input: spec.multi_input,
            initial_state: Vec::new(),
            state_width: spec.state_width,
        };
        let meta = CaseMeta {
            circuit_name: circuit_name.clone(),
            edit: stage.edit.clone(),
            audio_input: cfg.audio_input.clone(),
            bit_depth: cfg.bit_depth,
            batch_size: cfg.batch_size,
            comp_size,
            total_samples: current.len(),
            repetition: 0,
        };

        let metrics = circuit::run_proof(&run_cfg, &meta)
            .map_err(|e| ChainError::Prove { at: i, source: e.to_string() })?;

        // Catch a Rust/circuit disagreement at the stage that caused it,
        // rather than letting it surface as a broken link two stages on.
        let h_in = audio_parser::hex_stream_commitment::<F>(
            &current,
            cfg.bit_depth as u32,
            samples_per_step,
            hash_kind,
        );
        if h_in != metrics.final_hash_original {
            return Err(ChainError::StageCommitmentMismatch {
                at: i,
                expected: metrics.final_hash_original.clone(),
                recomputed: h_in,
            });
        }

        let _ = std::fs::remove_file(&json_path);

        proofs.push(StageProof {
            edit: stage.edit.clone(),
            circuit_name,
            h_in,
            h_out: metrics.final_hash_edited.clone(),
            h_b: metrics.final_hash_b.clone(),
            counter: metrics.final_counter.clone(),
            metrics,
        });

        current = next;
    }

    let h_orig = proofs[0].h_in.clone();
    let h_edit = proofs[proofs.len() - 1].h_out.clone();

    // Only the chain's input is signed. The intermediates are never
    // published, and the output needs no signature: the proofs tie it to
    // the signed input.
    let signature = signer::sign_hash(&protocol::signed_digest(&h_orig, n_samples), key);

    Ok(ChainProvenance {
        stages: proofs,
        h_orig,
        h_edit,
        n_samples,
        n_samples_out: current.len(),
        signature,
    })
}

/// The verifier's half: check the links, recompute the published stream's
/// commitment, check the signature.
///
/// Takes the published output audio and the bundle, never the original or
/// any intermediate stream. Each stage's proof is verified inside
/// `run_proof` at proving time; what remains here is binding the chain
/// together and to the artefact.
///
/// `edited` is the unpadded published stream.
pub fn verify_chain<F: PrimeField>(
    edited: &[i32],
    bit_depth: u32,
    samples_per_step: usize,
    prov: &ChainProvenance,
    pubkey: &VerifyingKey,
) -> Result<(), ChainError> {
    if prov.stages.is_empty() {
        return Err(ChainError::Empty);
    }

    // The links are what make a chain a chain: without them the stages
    // are unrelated proofs about streams nobody claims are connected.
    for i in 0..prov.stages.len() - 1 {
        let produced = &prov.stages[i].h_out;
        let consumed = &prov.stages[i + 1].h_in;
        if produced != consumed {
            return Err(ChainError::BrokenLink {
                at: i,
                produced: produced.clone(),
                consumed: consumed.clone(),
            });
        }
    }

    // Trimming the file inside the last fold leaves the commitment
    // unchanged, so the length is checked rather than recomputed.
    if edited.len() != prov.n_samples_out {
        return Err(ChainError::Stage {
            at: prov.stages.len() - 1,
            source: VerifyError::LengthMismatch {
                signed: prov.n_samples_out,
                published: edited.len(),
            },
        });
    }

    let last = &prov.stages[prov.stages.len() - 1];
    let hash_kind = BatchHashKind::for_circuit(&last.circuit_name);
    let recomputed =
        audio_parser::hex_stream_commitment::<F>(edited, bit_depth, samples_per_step, hash_kind);
    if recomputed != prov.h_edit {
        return Err(ChainError::Stage {
            at: prov.stages.len() - 1,
            source: VerifyError::EditedCommitmentMismatch {
                expected: prov.h_edit.clone(),
                recomputed,
            },
        });
    }

    // The first stage's input is the stream the signature is over.
    if prov.h_orig != prov.stages[0].h_in {
        return Err(ChainError::StageCommitmentMismatch {
            at: 0,
            expected: prov.stages[0].h_in.clone(),
            recomputed: prov.h_orig.clone(),
        });
    }

    if !signer::verify_hash(
        &protocol::signed_digest(&prov.h_orig, prov.n_samples),
        &prov.signature,
        pubkey,
    ) {
        return Err(ChainError::Stage { at: 0, source: VerifyError::BadSignature });
    }

    Ok(())
}
