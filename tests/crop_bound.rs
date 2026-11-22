//! `crop_bound` over a real proof: the excerpt is tied to a commitment
//! over the whole original.
//!
//! Plain `crop` folds only the kept window, so its `h_orig` commits to the
//! excerpt and cannot equal a signature over the file it came from. These
//! tests pin the property that makes `crop_bound` different: the original
//! chain absorbs every batch, so its terminal value is the same commitment
//! a signer would compute over the whole stream, while the edited chain
//! covers the window alone.
//!
//! Real `RecursiveSNARK`s, so these want `--release`:
//!
//! ```bash
//! cargo test --release --test crop_bound
//! ```
//!
//! Needs `artifacts/crop_bound_b8_B10` and the 8-bit fixture; both are
//! gitignored, so a missing input skips rather than fails.

use nova_snark::traits::Engine;

use zk_audio::{
    audio_parser,
    bench::config::comp_size_for,
    circuit::{self, CaseMeta, RunConfig},
    poseidon::BatchHashKind,
};

type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

const BIT_DEPTH: usize = 8;
const BATCH_SIZE: usize = 10;
const CIRCUIT: &str = "crop_bound_b8_B10";
const FIXTURE: &str = "raw_audios/sweep_31s_2c_44sr_8bd.wav";

/// Folds over the whole original, keeping `[start, start + len)`.
struct Proven {
    /// Terminal state element 0: the whole original's commitment.
    h_orig: String,
    /// Terminal state element 1: the window's commitment.
    h_window: String,
    original: Vec<i32>,
    samples_per_step: usize,
    json_path: String,
}

impl Drop for Proven {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.json_path);
    }
}

/// Proves one window, or `None` if the inputs are not on disk.
fn prove(tag: &str, steps: usize, start_step: usize, len_steps: usize) -> Option<Proven> {
    let comp_size = comp_size_for(BIT_DEPTH);
    let samples_per_step = BATCH_SIZE * comp_size;
    let r1cs = format!("artifacts/{CIRCUIT}.r1cs");
    let wasm = format!("artifacts/{CIRCUIT}_js/{CIRCUIT}.wasm");

    for needed in [r1cs.clone(), wasm.clone(), FIXTURE.to_string()] {
        if !std::path::Path::new(&needed).exists() {
            eprintln!("SKIP: {needed} not found - compile the circuit / generate the fixture");
            return None;
        }
    }

    let total = samples_per_step * steps;
    let (samples, _bits) = audio_parser::read_samples_signed(FIXTURE);
    assert!(samples.len() >= total, "fixture too short");
    let original: Vec<i32> = samples[..total].to_vec();

    // Crop is the identity on what it keeps, so the edited witness is the
    // original; outside the window the circuit does not constrain it.
    let json_path = format!("serialized/_test_{tag}_{CIRCUIT}_N{total}.json");
    audio_parser::serialize_samples_at(
        &original,
        &original,
        samples_per_step,
        BIT_DEPTH as u32,
        &json_path,
    );

    let window_start = start_step * samples_per_step;
    let window_len = len_steps * samples_per_step;

    let cfg = RunConfig {
        circuit_filepath: &r1cs,
        witness_gen_filepath: &wasm,
        json_input_filepath: &json_path,
        // The recursive chain is fully verified; compression is exercised
        // by the bench suite and only slows these down.
        skip_compressed: true,
        verbose: false,
        progress: false,
        multi_input: false,
        initial_state: vec![(3usize, window_start as u64), (4usize, window_len as u64)],
        state_width: 5,
    };
    let meta = CaseMeta {
        circuit_name: CIRCUIT.to_string(),
        edit: "crop_bound".to_string(),
        audio_input: FIXTURE.to_string(),
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        comp_size,
        total_samples: total,
        repetition: 0,
    };

    let metrics = circuit::run_proof(&cfg, &meta).expect("crop_bound proof");
    Some(Proven {
        h_orig: metrics.final_hash_original.clone(),
        h_window: metrics.final_hash_edited.clone(),
        original,
        samples_per_step,
        json_path,
    })
}

fn commit(samples: &[i32], samples_per_step: usize) -> String {
    audio_parser::hex_stream_commitment::<Fq>(
        samples,
        BIT_DEPTH as u32,
        samples_per_step,
        BatchHashKind::for_circuit(CIRCUIT),
    )
}

/// The property plain `crop` lacks: the proof's original-stream
/// commitment is a commitment to the WHOLE stream, so it equals what a
/// signer computes over the file without knowing which window was kept.
#[test]
fn h_orig_commits_to_the_whole_original_not_the_excerpt() {
    let Some(p) = prove("whole", 4, 1, 1) else { return };

    let signer_side = commit(&p.original, p.samples_per_step);
    assert_eq!(
        p.h_orig, signer_side,
        "h_orig must equal a commitment over the entire original"
    );

    // And it is specifically NOT the excerpt's commitment, which is what
    // plain crop would have produced here.
    let excerpt = &p.original[p.samples_per_step..2 * p.samples_per_step];
    assert_ne!(
        p.h_orig,
        commit(excerpt, p.samples_per_step),
        "h_orig must not be the excerpt's own commitment"
    );
}

/// The gated chain absorbs exactly the window, so the verifier can
/// recompute it from the published excerpt alone.
#[test]
fn h_window_commits_to_the_kept_samples() {
    let Some(p) = prove("window", 4, 2, 1) else { return };

    let kept = &p.original[2 * p.samples_per_step..3 * p.samples_per_step];
    assert_eq!(
        p.h_window,
        commit(kept, p.samples_per_step),
        "the edited chain must cover exactly the window"
    );
}

/// Two different windows of one file produce the same `h_orig`. This is
/// what lets several excerpts verify against a single signature.
#[test]
fn different_windows_share_one_original_commitment() {
    let Some(a) = prove("shared_a", 4, 0, 1) else { return };
    let Some(b) = prove("shared_b", 4, 3, 1) else { return };

    assert_eq!(
        a.h_orig, b.h_orig,
        "h_orig must not depend on which window was kept"
    );
    assert_ne!(
        a.h_window, b.h_window,
        "different windows must commit to different excerpts"
    );
}

/// A multi-step window folds the gate across several steps, so the
/// absorbed span is contiguous rather than only ever one batch.
#[test]
fn a_multi_step_window_absorbs_a_contiguous_span() {
    let Some(p) = prove("span", 5, 1, 3) else { return };

    let kept = &p.original[p.samples_per_step..4 * p.samples_per_step];
    assert_eq!(p.h_window, commit(kept, p.samples_per_step));
    assert_eq!(p.h_orig, commit(&p.original, p.samples_per_step));
}
