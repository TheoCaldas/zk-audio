//! The two on-disk formats: a signature over a source commitment, and a
//! proof of an edit.
//!
//! JSON so a bundle stays inspectable. Neither carries timings - those are
//! not part of the statement, and the CLI logs them to the terminal.

use serde::{Deserialize, Serialize};

/// Written by the mock-sign step.
///
/// `bit_depth` and `batch_size` are not decoration: `h_orig` is a fold over
/// a specific partition, so the same audio under a different batch size
/// commits to a different value. A verifier that does not know them cannot
/// recompute anything.
#[derive(Serialize, Deserialize)]
pub struct SignatureBundle {
    pub kind: String,
    pub h_orig: String,
    pub signature_der_hex: String,
    /// Carried for convenience. NOT a trust anchor: a forger ships their
    /// own key and their own signature, and every check still passes. Real
    /// verification compares this against an out-of-band source.
    pub public_key_pem: String,
    pub bit_depth: usize,
    pub batch_size: usize,
    /// True sample count, before padding. Part of the signed statement:
    /// `h_orig` is over the stream padded to a fold boundary, so the
    /// commitment alone does not say where the audio ends.
    pub samples: usize,
    /// What `h_orig` actually folds over: `samples` rounded up to a whole
    /// number of steps. Derivable from `samples` and the partition, and
    /// written out so a bundle can be read without recomputing it.
    pub padded_samples: usize,
    pub source_file: String,
}

impl SignatureBundle {
    pub const KIND: &'static str = "zk-audio-signature/2";
}

/// Written by the prove step. `circuit_name` is what lets the verify step
/// re-derive a verifier key without being told which circuit to use.
#[derive(Serialize, Deserialize)]
pub struct ProofBundle {
    pub kind: String,
    pub circuit_name: String,
    pub edit: String,
    pub bit_depth: usize,
    pub batch_size: usize,
    pub comp_size: usize,
    /// True sample count, before padding.
    pub samples: usize,
    /// What the proof folds over: `samples` padded to a fold boundary.
    /// `iterations` counts these, not `samples`.
    pub padded_samples: usize,
    /// Fold count, required by `CompressedSNARK::verify`.
    pub iterations: usize,
    /// Initial IVC state, required by `verify` and not always zero: crop
    /// carries its window start here.
    pub z0: Vec<u64>,
    pub h_orig: String,
    pub h_edit: String,
    /// Stream B's commitment, for the two-input relations. `h_orig` is
    /// stream A's, so a merge or concat bundle names both sources and the
    /// verifier checks each against its own signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h_b: Option<String>,
    /// Concat's join point, in samples. Part of the statement: the output
    /// is A's samples below it followed by B's from it on, and the circuit
    /// carries it through the fold chain unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub len_a: Option<usize>,
    /// The serialized `CompressedSNARK`, nested rather than encoded so the
    /// bundle is one JSON document.
    pub proof: serde_json::Value,
}

impl ProofBundle {
    pub const KIND: &'static str = "zk-audio-proof/2";
}

pub fn write<T: Serialize>(path: &str, value: &T) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("{path}: {e}"))
}

pub fn read<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("{path}: {e}"))
}
