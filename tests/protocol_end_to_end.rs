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

/// The signed digest is over the commitment text, so it is reproducible by
/// anyone holding the bundle - which is what lets a verifier check the
/// signature without the audio.
#[test]
fn signed_digest_is_reproducible_from_the_commitment() {
    let (sk, vk) = keys();
    let h = "0x1f763f0eb61ff73353eb2ddb3971717a79f59d33d866d338a95d773ab7eaa6c1";

    let sig = signer::sign_hash(&protocol::signed_digest(h), &sk);
    assert!(signer::verify_hash(&protocol::signed_digest(h), &sig, &vk));

    let other = "0x0e50bb10fb8866b01b7026f8f0b00dc24847398053b05164a3daed15c709071f";
    assert!(!signer::verify_hash(&protocol::signed_digest(other), &sig, &vk));
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
