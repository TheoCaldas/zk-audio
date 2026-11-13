//! The full protocol over a real proof:
//!
//!     hash -> sign -> prove -> verify proof -> recompute hash -> verify sig
//!
//! `poseidon_parity.rs` pins the hash against circom witnesses; this file
//! checks the thing that actually matters - that a commitment recomputed
//! in Rust equals the one Nova's step circuit computed in-circuit, so a
//! verifier can bind a proof to the audio it holds.
//!
//! These tests run a real `RecursiveSNARK`, so they are slow and want
//! `--release`:
//!
//! ```bash
//! cargo test --release --test protocol_end_to_end
//! ```
//!
//! They need the compiled artefacts for `gain_b8_k2_B10` and the 8-bit
//! fixture; both are gitignored, so they are regenerated/skipped rather
//! than failing the suite on a fresh clone. See `scripts/README.md`.

use nova_snark::traits::Engine;

use zk_audio::{
    audio_parser,
    bench::config::{circuit_name_for, comp_size_for, find_edit, EditCtx},
    circuit::{CaseMeta, RunConfig},
    poseidon::BatchHashKind,
    protocol::{self, VerifyError},
    signer,
};

type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

const BIT_DEPTH: usize = 8;
const BATCH_SIZE: usize = 10;
const FIXTURE: &str = "raw_audios/sweep_31s_2c_44sr_8bd.wav";
const EDIT: &str = "gain_k2";

/// One small case: two Nova folds over real audio.
struct Case {
    circuit_name: String,
    /// The batch-digest construction this circuit hashes with.
    hash_kind: BatchHashKind,
    original: Vec<i32>,
    edited: Vec<i32>,
    samples_per_step: usize,
    json_path: String,
}

/// Builds the case, or returns `None` if its inputs are not on disk.
///
/// Returning `None` rather than panicking keeps a fresh clone's
/// `cargo test` green: the artefacts and WAV fixtures are generated
/// locally and gitignored. The skip is printed so it cannot pass silently.
fn setup(tag: &str) -> Option<Case> {
    let comp_size = comp_size_for(BIT_DEPTH);
    let samples_per_step = BATCH_SIZE * comp_size;
    // Two folds: enough to exercise the chaining, cheap enough to prove.
    let total = samples_per_step * 2;

    let edit = find_edit(EDIT).expect("gain_k2 is a known edit");
    let circuit_name = circuit_name_for(edit.circuit_prefix, edit.factor, BIT_DEPTH, BATCH_SIZE);

    for needed in [
        format!("artifacts/{circuit_name}.r1cs"),
        format!("artifacts/{circuit_name}_js/{circuit_name}.wasm"),
        FIXTURE.to_string(),
    ] {
        if !std::path::Path::new(&needed).exists() {
            eprintln!("SKIP: {needed} not found - compile the circuit / generate the fixture");
            return None;
        }
    }

    let (samples, _bits) = audio_parser::read_samples_signed(FIXTURE);
    assert!(samples.len() >= total, "fixture too short");
    let original: Vec<i32> = samples[..total].to_vec();

    let ctx = EditCtx {
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        start_index: 0,
        comp_size,
        period: 8820,
    };
    let edited: Vec<i32> = (edit.apply)(&original, &ctx)
        .iter()
        .map(|&s| s as i32)
        .collect();

    // Per-test path: these run in parallel and must not share a file.
    let json_path = format!("serialized/_test_{tag}_{circuit_name}_N{total}.json");
    audio_parser::serialize_samples_at(
        &original,
        &edited,
        samples_per_step,
        BIT_DEPTH as u32,
        &json_path,
    );

    let hash_kind = BatchHashKind::for_circuit(&circuit_name);
    Some(Case { circuit_name, hash_kind, original, edited, samples_per_step, json_path })
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.json_path);
    }
}

fn config<'a>(case: &'a Case, r1cs: &'a str, wasm: &'a str) -> (RunConfig<'a>, CaseMeta) {
    let edit = find_edit(EDIT).unwrap();
    let cfg = RunConfig {
        circuit_filepath: r1cs,
        witness_gen_filepath: wasm,
        json_input_filepath: &case.json_path,
        // The compressed SNARK is verified by the bench suite; skipping it
        // here keeps these tests to a few seconds without weakening what
        // they check, since the recursive chain is still fully verified.
        skip_compressed: true,
        verbose: false,
        progress: false,
        multi_input: edit.multi_input,
        initial_state: Vec::new(),
        state_width: edit.state_width,
    };
    let meta = CaseMeta {
        circuit_name: case.circuit_name.clone(),
        edit: EDIT.to_string(),
        audio_input: FIXTURE.to_string(),
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        comp_size: comp_size_for(BIT_DEPTH),
        total_samples: case.original.len(),
        repetition: 0,
    };
    (cfg, meta)
}

fn keys() -> (p256::ecdsa::SigningKey, p256::ecdsa::VerifyingKey) {
    use p256::elliptic_curve::rand_core::OsRng;
    let sk = p256::ecdsa::SigningKey::random(&mut OsRng);
    let vk = p256::ecdsa::VerifyingKey::from(&sk);
    (sk, vk)
}

/// **The test this whole exercise was for.** A commitment recomputed
/// off-circuit in Rust must equal the one the step circuit folded
/// in-circuit, for both streams, bit for bit.
///
/// Before the Poseidon port these differed - the Rust side was on BN254 -
/// which made the verifier's recompute step impossible and left the proof
/// unbindable to any published artefact.
#[test]
fn rust_commitments_match_the_proofs_terminal_state() {
    let Some(case) = setup("match") else { return };
    let r1cs = format!("artifacts/{}.r1cs", case.circuit_name);
    let wasm = format!(
        "artifacts/{}_js/{}.wasm",
        case.circuit_name, case.circuit_name
    );
    let (cfg, meta) = config(&case, &r1cs, &wasm);

    let metrics = zk_audio::circuit::run_proof(&cfg, &meta).expect("proof run");

    let h_orig =
        audio_parser::hex_stream_commitment::<Fq>(&case.original, BIT_DEPTH as u32, case.samples_per_step, case.hash_kind);
    let h_edit =
        audio_parser::hex_stream_commitment::<Fq>(&case.edited, BIT_DEPTH as u32, case.samples_per_step, case.hash_kind);

    assert_eq!(
        h_orig, metrics.final_hash_original,
        "Rust and circuit disagree on the ORIGINAL stream commitment"
    );
    assert_eq!(
        h_edit, metrics.final_hash_edited,
        "Rust and circuit disagree on the EDITED stream commitment"
    );
}

/// The happy path, all six steps.
#[test]
fn full_protocol_round_trip() {
    let Some(case) = setup("roundtrip") else { return };
    let r1cs = format!("artifacts/{}.r1cs", case.circuit_name);
    let wasm = format!(
        "artifacts/{}_js/{}.wasm",
        case.circuit_name, case.circuit_name
    );
    let (cfg, meta) = config(&case, &r1cs, &wasm);
    let (sk, vk) = keys();

    let prov = protocol::create_provenance::<Fq>(
        &case.original,
        &case.edited,
        BIT_DEPTH as u32,
        case.samples_per_step,
        case.hash_kind,
        &sk,
        &cfg,
        &meta,
    )
    .expect("creating provenance");

    // The verifier sees only the edited audio and the bundle - never
    // `case.original`.
    protocol::verify_provenance::<Fq>(
        &case.edited,
        BIT_DEPTH as u32,
        case.samples_per_step,
        case.hash_kind,
        &prov,
        &vk,
    )
    .expect("verification should succeed on an honest bundle");
}

/// Publishing audio other than what was proven must be caught.
///
/// This is the attack the edited-stream commitment exists to stop: the
/// proof still verifies (it is about the original pair), but the recomputed
/// commitment no longer matches the terminal state.
#[test]
fn tampered_audio_is_rejected() {
    let Some(case) = setup("tamper") else { return };
    let r1cs = format!("artifacts/{}.r1cs", case.circuit_name);
    let wasm = format!(
        "artifacts/{}_js/{}.wasm",
        case.circuit_name, case.circuit_name
    );
    let (cfg, meta) = config(&case, &r1cs, &wasm);
    let (sk, vk) = keys();

    let prov = protocol::create_provenance::<Fq>(
        &case.original,
        &case.edited,
        BIT_DEPTH as u32,
        case.samples_per_step,
        case.hash_kind,
        &sk,
        &cfg,
        &meta,
    )
    .expect("creating provenance");

    // One sample altered, in the middle of the second fold.
    let mut tampered = case.edited.clone();
    let i = tampered.len() - 7;
    tampered[i] = if tampered[i] == 0 { 1 } else { tampered[i] - 1 };

    let err = protocol::verify_provenance::<Fq>(
        &tampered,
        BIT_DEPTH as u32,
        case.samples_per_step,
        case.hash_kind,
        &prov,
        &vk,
    )
    .expect_err("a single altered sample must be rejected");

    assert!(
        matches!(err, VerifyError::EditedCommitmentMismatch { .. }),
        "expected a commitment mismatch, got {err}"
    );
}

/// A bundle signed by one key must not verify under another.
#[test]
fn signature_from_the_wrong_key_is_rejected() {
    let Some(case) = setup("wrongkey") else { return };
    let r1cs = format!("artifacts/{}.r1cs", case.circuit_name);
    let wasm = format!(
        "artifacts/{}_js/{}.wasm",
        case.circuit_name, case.circuit_name
    );
    let (cfg, meta) = config(&case, &r1cs, &wasm);
    let (sk, _vk) = keys();
    let (_other_sk, other_vk) = keys();

    let prov = protocol::create_provenance::<Fq>(
        &case.original,
        &case.edited,
        BIT_DEPTH as u32,
        case.samples_per_step,
        case.hash_kind,
        &sk,
        &cfg,
        &meta,
    )
    .expect("creating provenance");

    let err = protocol::verify_provenance::<Fq>(
        &case.edited,
        BIT_DEPTH as u32,
        case.samples_per_step,
        case.hash_kind,
        &prov,
        &other_vk,
    )
    .expect_err("a foreign key must not verify");

    assert!(
        matches!(err, VerifyError::BadSignature),
        "expected a bad signature, got {err}"
    );
}

/// Swapping the committed original invalidates the signature, so a
/// prover cannot re-point a signed bundle at a different original.
#[test]
fn substituted_commitment_breaks_the_signature() {
    let Some(case) = setup("substitute") else { return };
    let r1cs = format!("artifacts/{}.r1cs", case.circuit_name);
    let wasm = format!(
        "artifacts/{}_js/{}.wasm",
        case.circuit_name, case.circuit_name
    );
    let (cfg, meta) = config(&case, &r1cs, &wasm);
    let (sk, vk) = keys();

    let mut prov = protocol::create_provenance::<Fq>(
        &case.original,
        &case.edited,
        BIT_DEPTH as u32,
        case.samples_per_step,
        case.hash_kind,
        &sk,
        &cfg,
        &meta,
    )
    .expect("creating provenance");

    // Claim the edited stream's commitment as the original's. The
    // signature was made over the real h_orig, so it no longer applies.
    prov.h_orig = prov.h_edit.clone();

    let err = protocol::verify_provenance::<Fq>(
        &case.edited,
        BIT_DEPTH as u32,
        case.samples_per_step,
        case.hash_kind,
        &prov,
        &vk,
    )
    .expect_err("a substituted commitment must be rejected");

    assert!(
        matches!(err, VerifyError::BadSignature),
        "expected a bad signature, got {err}"
    );
}

/// A full protocol round trip at an N that is NOT a whole number of
/// folds, which is the case the fixed-width step circuit cannot fold
/// directly. The stream is silence-padded to a boundary, the edit runs on
/// the padded stream, and the true N rides in the signed statement.
///
/// Runs a real `RecursiveSNARK`, so a disagreement between the padding
/// Rust applies and the batches the circuit folds shows up as a
/// commitment mismatch rather than passing quietly.
#[test]
fn protocol_round_trips_at_a_length_that_is_not_a_whole_fold() {
    let comp_size = comp_size_for(BIT_DEPTH);
    let samples_per_step = BATCH_SIZE * comp_size;
    // One full fold plus a deliberately ragged tail.
    let n_samples = samples_per_step + 7;
    let padded = n_samples.div_ceil(samples_per_step) * samples_per_step;
    assert_ne!(n_samples % samples_per_step, 0, "the case must be ragged");

    let edit = find_edit(EDIT).expect("gain_k2 is a known edit");
    let circuit_name = circuit_name_for(edit.circuit_prefix, edit.factor, BIT_DEPTH, BATCH_SIZE);
    let r1cs = format!("artifacts/{circuit_name}.r1cs");
    let wasm = format!("artifacts/{circuit_name}_js/{circuit_name}.wasm");
    for needed in [r1cs.as_str(), wasm.as_str(), FIXTURE] {
        if !std::path::Path::new(needed).exists() {
            eprintln!("SKIP: {needed} not found");
            return;
        }
    }

    let (samples, _bits) = audio_parser::read_samples_signed(FIXTURE);
    let unpadded: Vec<i32> = samples[..n_samples].to_vec();
    let padded_original = audio_parser::pad_to_step(&unpadded, samples_per_step);
    assert_eq!(padded_original.len(), padded);

    let ctx = EditCtx {
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        start_index: 0,
        comp_size,
        period: 8820,
    };
    // The edit runs on the PADDED stream: the relation has to hold on the
    // pad too, and `edit(0)` is not 0 for every edit.
    let padded_edited: Vec<i32> = (edit.apply)(&padded_original, &ctx)
        .iter()
        .map(|&s| s as i32)
        .collect();

    let json_path = format!("serialized/_test_ragged_{circuit_name}_N{padded}.json");
    audio_parser::serialize_samples_at(
        &padded_original,
        &padded_edited,
        samples_per_step,
        BIT_DEPTH as u32,
        &json_path,
    );

    let hash_kind = BatchHashKind::for_circuit(&circuit_name);
    let cfg = RunConfig {
        circuit_filepath: &r1cs,
        witness_gen_filepath: &wasm,
        json_input_filepath: &json_path,
        skip_compressed: true,
        verbose: false,
        progress: false,
        multi_input: edit.multi_input,
        initial_state: Vec::new(),
        state_width: edit.state_width,
    };
    let meta = CaseMeta {
        circuit_name: circuit_name.clone(),
        edit: EDIT.to_string(),
        audio_input: FIXTURE.to_string(),
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        comp_size,
        total_samples: padded,
        repetition: 0,
    };

    let (sk, vk) = keys();
    // Unpadded in, so the signed statement carries the true N.
    let prov = protocol::create_provenance::<Fq>(
        &unpadded,
        &padded_edited[..n_samples],
        BIT_DEPTH as u32,
        samples_per_step,
        hash_kind,
        &sk,
        &cfg,
        &meta,
    )
    .expect("creating provenance over a ragged stream");
    let _ = std::fs::remove_file(&json_path);

    assert_eq!(prov.n_samples, n_samples, "the statement must carry the true N");
    assert_eq!(prov.metrics.iterations, padded / samples_per_step);

    protocol::verify_provenance::<Fq>(
        &padded_edited[..n_samples],
        BIT_DEPTH as u32,
        samples_per_step,
        hash_kind,
        &prov,
        &vk,
    )
    .expect("a ragged stream must verify");
}

/// Trimming the published audio inside the final fold leaves every
/// commitment unchanged, because the commitment is over the padded
/// stream. Only the signed length rules it out.
#[test]
fn truncating_inside_the_pad_is_caught_by_the_length() {
    let comp_size = comp_size_for(BIT_DEPTH);
    let samples_per_step = BATCH_SIZE * comp_size;
    let n_samples = samples_per_step + 7;

    let edit = find_edit(EDIT).expect("gain_k2 is a known edit");
    let circuit_name = circuit_name_for(edit.circuit_prefix, edit.factor, BIT_DEPTH, BATCH_SIZE);
    let r1cs = format!("artifacts/{circuit_name}.r1cs");
    let wasm = format!("artifacts/{circuit_name}_js/{circuit_name}.wasm");
    for needed in [r1cs.as_str(), wasm.as_str(), FIXTURE] {
        if !std::path::Path::new(needed).exists() {
            eprintln!("SKIP: {needed} not found");
            return;
        }
    }

    let (samples, _bits) = audio_parser::read_samples_signed(FIXTURE);
    // A silent tail, so trimming it changes neither commitment.
    let mut unpadded: Vec<i32> = samples[..n_samples].to_vec();
    for s in unpadded.iter_mut().skip(samples_per_step) {
        *s = 0;
    }
    let padded_original = audio_parser::pad_to_step(&unpadded, samples_per_step);
    let padded = padded_original.len();

    let ctx = EditCtx {
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        start_index: 0,
        comp_size,
        period: 8820,
    };
    let padded_edited: Vec<i32> = (edit.apply)(&padded_original, &ctx)
        .iter()
        .map(|&s| s as i32)
        .collect();

    let json_path = format!("serialized/_test_trunc_{circuit_name}_N{padded}.json");
    audio_parser::serialize_samples_at(
        &padded_original,
        &padded_edited,
        samples_per_step,
        BIT_DEPTH as u32,
        &json_path,
    );

    let hash_kind = BatchHashKind::for_circuit(&circuit_name);
    let cfg = RunConfig {
        circuit_filepath: &r1cs,
        witness_gen_filepath: &wasm,
        json_input_filepath: &json_path,
        skip_compressed: true,
        verbose: false,
        progress: false,
        multi_input: edit.multi_input,
        initial_state: Vec::new(),
        state_width: edit.state_width,
    };
    let meta = CaseMeta {
        circuit_name: circuit_name.clone(),
        edit: EDIT.to_string(),
        audio_input: FIXTURE.to_string(),
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        comp_size,
        total_samples: padded,
        repetition: 0,
    };

    let (sk, vk) = keys();
    let prov = protocol::create_provenance::<Fq>(
        &unpadded,
        &padded_edited[..n_samples],
        BIT_DEPTH as u32,
        samples_per_step,
        hash_kind,
        &sk,
        &cfg,
        &meta,
    )
    .expect("creating provenance");
    let _ = std::fs::remove_file(&json_path);

    // The attack: publish the stream one sample short. The commitment is
    // identical, so only the length catches it.
    let trimmed = &padded_edited[..n_samples - 1];
    assert_eq!(
        audio_parser::hex_stream_commitment::<Fq>(
            trimmed,
            BIT_DEPTH as u32,
            samples_per_step,
            hash_kind
        ),
        prov.h_edit,
        "the trimmed stream must still recommit to h_edit, or this tests nothing"
    );

    let err = protocol::verify_provenance::<Fq>(
        trimmed,
        BIT_DEPTH as u32,
        samples_per_step,
        hash_kind,
        &prov,
        &vk,
    )
    .expect_err("a trimmed stream must be rejected");
    assert!(
        matches!(
            err,
            VerifyError::LengthMismatch { signed, published }
                if signed == n_samples && published == n_samples - 1
        ),
        "expected a length mismatch, got {err}"
    );
}

/// Re-signing the same commitment under a different length must not yield
/// a signature that verifies, or the padding would be prover-chosen.
#[test]
fn the_length_is_bound_into_the_signature() {
    let (sk, vk) = keys();
    let h = "0x1f763f0eb61ff73353eb2ddb3971717a79f59d33d866d338a95d773ab7eaa6c1";

    let sig = signer::sign_hash(&protocol::signed_digest(h, 620), &sk);
    for n in [619usize, 621, 0, 1240] {
        assert!(
            !signer::verify_hash(&protocol::signed_digest(h, n), &sig, &vk),
            "a signature over N=620 must not verify at N={n}"
        );
    }
}

/// Field separation: no `(h, n)` pair may be readable as a different one.
#[test]
fn the_commitment_statement_is_unambiguous() {
    assert_ne!(
        protocol::commitment_statement("0xab", 12),
        protocol::commitment_statement("0xab1", 2),
    );
    assert_ne!(
        protocol::commitment_statement("0xab|1", 2),
        protocol::commitment_statement("0xab", 12),
    );
}

/// The signed digest is over the commitment text, so it is reproducible by
/// anyone holding the bundle - which is what lets a verifier check the
/// signature without the audio.
#[test]
fn signed_digest_is_reproducible_from_the_commitment() {
    let (sk, vk) = keys();
    let h = "0x1f763f0eb61ff73353eb2ddb3971717a79f59d33d866d338a95d773ab7eaa6c1";

    let sig = signer::sign_hash(&protocol::signed_digest(h, 620), &sk);
    assert!(signer::verify_hash(&protocol::signed_digest(h, 620), &sig, &vk));

    let other = "0x0e50bb10fb8866b01b7026f8f0b00dc24847398053b05164a3daed15c709071f";
    assert!(!signer::verify_hash(&protocol::signed_digest(other, 620), &sig, &vk));
    // The length is half the statement, so changing it alone breaks it.
    assert!(!signer::verify_hash(&protocol::signed_digest(h, 621), &sig, &vk));
}

/// The same differential check against the **legacy chain-hashed** family.
///
/// `gain2` compiles to `multiplier_b16_B*`, which predates the sponge
/// switch and still absorbs each batch with the arity-2 `BatchHasher`
/// chain. Computing its commitment with the sponge yields a wrong hash -
/// that is a real bug this suite caught in the `main.rs` demo after the
/// sponge was wired in, so both constructions are now pinned against live
/// proofs rather than only the current one.
#[test]
fn legacy_chain_hashed_circuit_also_matches() {
    const LEGACY_BIT_DEPTH: usize = 16;
    const LEGACY_BATCH: usize = 10;

    let comp_size = comp_size_for(LEGACY_BIT_DEPTH);
    let samples_per_step = LEGACY_BATCH * comp_size;
    let total = samples_per_step * 2;

    let edit = find_edit("gain2").expect("gain2 is a known edit");
    let circuit_name =
        circuit_name_for(edit.circuit_prefix, edit.factor, LEGACY_BIT_DEPTH, LEGACY_BATCH);
    let r1cs = format!("artifacts/{circuit_name}.r1cs");
    let wasm = format!("artifacts/{circuit_name}_js/{circuit_name}.wasm");
    let fixture = "raw_audios/sweep_120s_2c_44sr_16bd.wav";

    for needed in [r1cs.as_str(), wasm.as_str(), fixture] {
        if !std::path::Path::new(needed).exists() {
            eprintln!("SKIP: {needed} not found");
            return;
        }
    }

    // This family's relation is x*2 with NO clamping, so it needs the
    // quarter-scale fixture: near-full-scale samples would overflow and
    // stop matching the circuit. See scripts/README.md.
    let (samples, _bits) = audio_parser::read_samples_signed(fixture);
    let original: Vec<i32> = samples[..total].to_vec();
    let ctx = EditCtx {
        bit_depth: LEGACY_BIT_DEPTH,
        batch_size: LEGACY_BATCH,
        start_index: 0,
        comp_size,
        period: 8820,
    };
    let edited: Vec<i32> = (edit.apply)(&original, &ctx)
        .iter()
        .map(|&s| s as i32)
        .collect();

    let json_path = format!("serialized/_test_legacy_{circuit_name}_N{total}.json");
    audio_parser::serialize_samples_at(
        &original,
        &edited,
        samples_per_step,
        LEGACY_BIT_DEPTH as u32,
        &json_path,
    );

    let kind = BatchHashKind::for_circuit(&circuit_name);
    assert_eq!(kind, BatchHashKind::Chain, "gain2 must route to the chain");

    let cfg = RunConfig {
        circuit_filepath: &r1cs,
        witness_gen_filepath: &wasm,
        json_input_filepath: &json_path,
        skip_compressed: true,
        verbose: false,
        progress: false,
        multi_input: edit.multi_input,
        initial_state: Vec::new(),
        state_width: edit.state_width,
    };
    let meta = CaseMeta {
        circuit_name: circuit_name.clone(),
        edit: "gain2".to_string(),
        audio_input: fixture.to_string(),
        bit_depth: LEGACY_BIT_DEPTH,
        batch_size: LEGACY_BATCH,
        comp_size,
        total_samples: total,
        repetition: 0,
    };

    let metrics = zk_audio::circuit::run_proof(&cfg, &meta).expect("proof run");
    let _ = std::fs::remove_file(&json_path);

    let h_orig = audio_parser::hex_stream_commitment::<Fq>(
        &original,
        LEGACY_BIT_DEPTH as u32,
        samples_per_step,
        kind,
    );
    let h_edit = audio_parser::hex_stream_commitment::<Fq>(
        &edited,
        LEGACY_BIT_DEPTH as u32,
        samples_per_step,
        kind,
    );

    assert_eq!(h_orig, metrics.final_hash_original, "legacy ORIGINAL mismatch");
    assert_eq!(h_edit, metrics.final_hash_edited, "legacy EDITED mismatch");

    // And the sponge would have been wrong here - the bug being pinned.
    let wrong = audio_parser::hex_stream_commitment::<Fq>(
        &original,
        LEGACY_BIT_DEPTH as u32,
        samples_per_step,
        BatchHashKind::PRODUCTION,
    );
    assert_ne!(
        wrong, metrics.final_hash_original,
        "sponge and chain digests coincided; the distinction would be untested"
    );
}

/// Every single-input family must fold a ragged N, not just gain.
///
/// Distortion is the case that justifies padding the ORIGINAL and then
/// applying the edit, rather than padding both streams with silence:
/// `distort(0)` is `-2^(bd-1)/2`, so a zero-padded edited stream would
/// violate the relation in the pad and the circuit would reject it.
///
/// Crop carries a sample counter in its IVC state, so its z0 is set from
/// the registry's state width rather than left at zero.
#[test]
fn every_single_input_family_folds_a_ragged_length() {
    const RAGGED_BATCH: usize = 10;
    const RAGGED_DEPTH: usize = 8;

    let comp_size = comp_size_for(RAGGED_DEPTH);
    let samples_per_step = RAGGED_BATCH * comp_size;
    let n_samples = samples_per_step + 13;
    let padded = n_samples.div_ceil(samples_per_step) * samples_per_step;

    let (samples, _bits) = match std::path::Path::new(FIXTURE).exists() {
        true => audio_parser::read_samples_signed(FIXTURE),
        false => {
            eprintln!("SKIP: {FIXTURE} not found");
            return;
        }
    };

    let mut checked = 0;
    for name in ["gain_k2", "atten_m1", "dist_d100", "tremolo_f5", "crop"] {
        let edit = find_edit(name).unwrap_or_else(|| panic!("{name} is a known edit"));
        let circuit_name =
            circuit_name_for(edit.circuit_prefix, edit.factor, RAGGED_DEPTH, RAGGED_BATCH);
        let r1cs = format!("artifacts/{circuit_name}.r1cs");
        let wasm = format!("artifacts/{circuit_name}_js/{circuit_name}.wasm");
        if !std::path::Path::new(&r1cs).exists() || !std::path::Path::new(&wasm).exists() {
            eprintln!("SKIP {name}: {circuit_name} not compiled");
            continue;
        }
        // A `_baseline` build compiles to the same stem as the current
        // circuit but absorbs with the arity-2 chain, so an artefact left
        // over from one commits differently than `for_circuit` assumes.
        // Skipping rather than asserting keeps this test about padding.
        if !hashes_with_the_current_construction(&r1cs, &wasm, &circuit_name, &edit, comp_size) {
            eprintln!(
                "SKIP {name}: artifacts/{circuit_name} does not hash with the sponge; \
                 recompile it from circuits/edits/ (a stale _baseline build)"
            );
            continue;
        }

        let unpadded: Vec<i32> = samples[..n_samples].to_vec();
        let padded_original = audio_parser::pad_to_step(&unpadded, samples_per_step);
        let ctx = EditCtx {
            bit_depth: RAGGED_DEPTH,
            batch_size: RAGGED_BATCH,
            start_index: 0,
            comp_size,
            period: 8820,
        };
        let padded_edited: Vec<i32> = (edit.apply)(&padded_original, &ctx)
            .iter()
            .map(|&s| s as i32)
            .collect();

        let json_path = format!("serialized/_test_fam_{circuit_name}_N{padded}.json");
        audio_parser::serialize_samples_at(
            &padded_original,
            &padded_edited,
            samples_per_step,
            RAGGED_DEPTH as u32,
            &json_path,
        );

        let hash_kind = BatchHashKind::for_circuit(&circuit_name);
        let cfg = RunConfig {
            circuit_filepath: &r1cs,
            witness_gen_filepath: &wasm,
            json_input_filepath: &json_path,
            skip_compressed: true,
            verbose: false,
            progress: false,
            multi_input: edit.multi_input,
            initial_state: Vec::new(),
            state_width: edit.state_width,
        };
        let meta = CaseMeta {
            circuit_name: circuit_name.clone(),
            edit: name.to_string(),
            audio_input: FIXTURE.to_string(),
            bit_depth: RAGGED_DEPTH,
            batch_size: RAGGED_BATCH,
            comp_size,
            total_samples: padded,
            repetition: 0,
        };

        let metrics = zk_audio::circuit::run_proof(&cfg, &meta)
            .unwrap_or_else(|e| panic!("{name} failed to prove a ragged N: {e}"));
        let _ = std::fs::remove_file(&json_path);

        assert_eq!(
            metrics.iterations,
            padded / samples_per_step,
            "{name} folded the wrong number of steps"
        );
        assert_eq!(
            audio_parser::hex_stream_commitment::<Fq>(
                &unpadded,
                RAGGED_DEPTH as u32,
                samples_per_step,
                hash_kind
            ),
            metrics.final_hash_original,
            "{name}: Rust and circuit disagree on the padded ORIGINAL"
        );
        assert_eq!(
            audio_parser::hex_stream_commitment::<Fq>(
                &padded_edited,
                RAGGED_DEPTH as u32,
                samples_per_step,
                hash_kind
            ),
            metrics.final_hash_edited,
            "{name}: Rust and circuit disagree on the padded EDITED stream"
        );
        checked += 1;
    }

    assert!(checked > 0, "no family was compiled; the test checked nothing");
    eprintln!("ragged N verified against {checked} families");
}

/// Whether a compiled artefact absorbs batches the way
/// `BatchHashKind::for_circuit` says it does.
///
/// `_baseline` circuits compile to the same stems as the current ones but
/// hash with the arity-2 chain, so a stale artefact on disk silently
/// commits to a different value (README.md, "Poseidon parity"). One fold
/// over constant samples is enough to tell them apart.
fn hashes_with_the_current_construction(
    r1cs: &str,
    wasm: &str,
    circuit_name: &str,
    edit: &zk_audio::bench::config::EditSpec,
    comp_size: usize,
) -> bool {
    const PROBE_BATCH: usize = 10;
    let samples_per_step = PROBE_BATCH * comp_size;
    let original = vec![1i32; samples_per_step];
    let ctx = EditCtx {
        bit_depth: 8,
        batch_size: PROBE_BATCH,
        start_index: 0,
        comp_size,
        period: 8820,
    };
    let edited: Vec<i32> = (edit.apply)(&original, &ctx).iter().map(|&s| s as i32).collect();

    let json_path = format!("serialized/_test_probe_{circuit_name}.json");
    audio_parser::serialize_samples_at(&original, &edited, samples_per_step, 8, &json_path);
    let cfg = RunConfig {
        circuit_filepath: r1cs,
        witness_gen_filepath: wasm,
        json_input_filepath: &json_path,
        skip_compressed: true,
        verbose: false,
        progress: false,
        multi_input: edit.multi_input,
        initial_state: Vec::new(),
        state_width: edit.state_width,
    };
    let meta = CaseMeta {
        circuit_name: circuit_name.to_string(),
        edit: edit.name.to_string(),
        audio_input: "probe".to_string(),
        bit_depth: 8,
        batch_size: PROBE_BATCH,
        comp_size,
        total_samples: samples_per_step,
        repetition: 0,
    };
    let metrics = zk_audio::circuit::run_proof(&cfg, &meta);
    let _ = std::fs::remove_file(&json_path);
    let Ok(metrics) = metrics else { return false };

    let expected = audio_parser::hex_stream_commitment::<Fq>(
        &original,
        8,
        samples_per_step,
        BatchHashKind::for_circuit(circuit_name),
    );
    expected == metrics.final_hash_original
}

/// Merge and concat at a ragged N.
///
/// Both take two source streams, so padding has to reach all three: the
/// two sources and the output. Concat additionally carries its split
/// point in z0, and that point must stay fold-aligned after padding --
/// padding extends the stream at the end, so a boundary computed from the
/// padded length still lands on a fold.
#[test]
fn multi_input_families_fold_a_ragged_length() {
    const RAGGED_BATCH: usize = 10;
    const RAGGED_DEPTH: usize = 8;
    const FIXTURE_B: &str = "raw_audios/sweep_31s_2c_44sr_8bd.wav";

    let comp_size = comp_size_for(RAGGED_DEPTH);
    let samples_per_step = RAGGED_BATCH * comp_size;
    // Two full folds plus a tail, so concat's split point has room.
    let n_samples = samples_per_step * 2 + 11;
    let padded = n_samples.div_ceil(samples_per_step) * samples_per_step;

    if !std::path::Path::new(FIXTURE).exists() {
        eprintln!("SKIP: {FIXTURE} not found");
        return;
    }
    let (samples, _bits) = audio_parser::read_samples_signed(FIXTURE);
    let (samples_b, _) = audio_parser::read_samples_signed(FIXTURE_B);

    let mut checked = 0;
    for name in ["merge", "concat"] {
        let edit = find_edit(name).unwrap_or_else(|| panic!("{name} is a known edit"));
        let circuit_name =
            circuit_name_for(edit.circuit_prefix, edit.factor, RAGGED_DEPTH, RAGGED_BATCH);
        let r1cs = format!("artifacts/{circuit_name}.r1cs");
        let wasm = format!("artifacts/{circuit_name}_js/{circuit_name}.wasm");
        if !std::path::Path::new(&r1cs).exists() || !std::path::Path::new(&wasm).exists() {
            eprintln!("SKIP {name}: {circuit_name} not compiled");
            continue;
        }

        // Pad both sources before the edit, so the relation holds on the
        // pad for whichever source the output draws from there.
        let a = audio_parser::pad_to_step(&samples[..n_samples], samples_per_step);
        let b = audio_parser::pad_to_step(
            &samples_b[samples_per_step..samples_per_step + n_samples],
            samples_per_step,
        );
        assert_eq!(a.len(), padded);
        assert_eq!(b.len(), padded);

        let ctx = EditCtx {
            bit_depth: RAGGED_DEPTH,
            batch_size: RAGGED_BATCH,
            start_index: 0,
            comp_size,
            period: 8820,
        };
        let apply2 = edit.apply2.expect("a multi-input edit has apply2");
        let out: Vec<i32> = apply2(&a, &b, &ctx);
        assert_eq!(out.len(), padded);

        let json_path = format!("serialized/_test_multi_{circuit_name}_N{padded}.json");
        audio_parser::serialize_merge_at(
            &a,
            &b,
            &out,
            samples_per_step,
            RAGGED_DEPTH as u32,
            &json_path,
        );

        // Concat's split point is derived from the padded length, so it
        // stays on a fold boundary.
        let initial_state = if name == "concat" {
            let aligned = ((padded / 2) / samples_per_step) * samples_per_step;
            vec![(4usize, aligned as u64)]
        } else {
            Vec::new()
        };

        let cfg = RunConfig {
            circuit_filepath: &r1cs,
            witness_gen_filepath: &wasm,
            json_input_filepath: &json_path,
            skip_compressed: true,
            verbose: false,
            progress: false,
            multi_input: edit.multi_input,
            initial_state,
            state_width: edit.state_width,
        };
        let meta = CaseMeta {
            circuit_name: circuit_name.clone(),
            edit: name.to_string(),
            audio_input: FIXTURE.to_string(),
            bit_depth: RAGGED_DEPTH,
            batch_size: RAGGED_BATCH,
            comp_size,
            total_samples: padded,
            repetition: 0,
        };

        let metrics = zk_audio::circuit::run_proof(&cfg, &meta)
            .unwrap_or_else(|e| panic!("{name} failed to prove a ragged N: {e}"));
        let _ = std::fs::remove_file(&json_path);

        assert_eq!(
            metrics.iterations,
            padded / samples_per_step,
            "{name} folded the wrong number of steps"
        );
        // Concat gates its source chains: A absorbs only while the
        // counter is below the split, so hash_a covers len_a samples
        // rather than the whole stream and is not a commitment to `a`.
        // Merge absorbs every step, so its chain is.
        let hash_kind = BatchHashKind::for_circuit(&circuit_name);
        if name == "merge" {
            assert_eq!(
                audio_parser::hex_stream_commitment::<Fq>(
                    &a,
                    RAGGED_DEPTH as u32,
                    samples_per_step,
                    hash_kind
                ),
                metrics.final_hash_original,
                "{name}: Rust and circuit disagree on the padded source A"
            );
        } else {
            let aligned = ((padded / 2) / samples_per_step) * samples_per_step;
            assert_eq!(
                audio_parser::hex_stream_commitment::<Fq>(
                    &a[..aligned],
                    RAGGED_DEPTH as u32,
                    samples_per_step,
                    hash_kind
                ),
                metrics.final_hash_original,
                "{name}: hash_a must cover exactly the pre-split prefix"
            );
        }
        checked += 1;
    }

    assert!(checked > 0, "no multi-input family was compiled");
    eprintln!("ragged N verified against {checked} multi-input families");
}
