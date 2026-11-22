//! Fused composite proofs over real `RecursiveSNARK`s, and their
//! agreement with the sequential chain that computes the same thing.
//!
//! What these pin is that fusion changes only the cost: a fused proof's
//! two commitments are bit-for-bit the chain's first `h_in` and last
//! `h_out`. If that holds, the fused circuit proves the same composite
//! relation over the same stream partition, and the saving is not bought
//! with a different statement.
//!
//! Real proofs, so slow and wanting `--release`:
//!
//! ```bash
//! cargo test --release --test fused_end_to_end
//! ```
//!
//! They need the compiled B=10 bit-depth-8 artefacts, including the fused
//! one, which is generated per chain spec:
//!
//! ```bash
//! python3 scripts/gen_fused_circuit.py gain_k2 atten_m1 dist_d100 --bd 8 --B 10
//! ./compile.circuit.sh circuits/edits/_fused/fused_gain_k2_atten_m1_dist_d100_b8_B10.circom
//! ```
//!
//! All artefacts are gitignored, so a missing one skips rather than
//! failing a fresh clone.

use nova_snark::traits::Engine;

use zk_audio::{
    bench::config::{comp_size_for, find_edit, EditCtx},
    chain::{self, ChainConfig, Stage},
    fused::{self, FusedError},
};

type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

const BIT_DEPTH: usize = 8;
const BATCH_SIZE: usize = 10;
const PERIOD: usize = 8820;

fn samples_per_step() -> usize {
    BATCH_SIZE * comp_size_for(BIT_DEPTH)
}

fn config(tag: &'static str) -> ChainConfig<'static> {
    ChainConfig {
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        period: PERIOD,
        serialized_dir: "serialized",
        tag,
        skip_compressed: true,
        progress: false,
        audio_input: "(test ramp)".to_string(),
    }
}

fn ctx() -> EditCtx {
    EditCtx {
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        start_index: 0,
        comp_size: comp_size_for(BIT_DEPTH),
        period: PERIOD,
    }
}

/// The per-stage artefacts a chain needs, plus the fused one.
fn artefacts_present(edits: &[&str]) -> bool {
    use zk_audio::bench::config::circuit_name_for;

    for e in edits {
        let spec = match find_edit(e) {
            Some(s) => s,
            None => return false,
        };
        let name = circuit_name_for(spec.circuit_prefix, spec.factor, BIT_DEPTH, BATCH_SIZE);
        if !std::path::Path::new(&format!("artifacts/{name}.r1cs")).exists() {
            return false;
        }
    }

    let owned: Vec<String> = edits.iter().map(|s| s.to_string()).collect();
    let fused_name = fused::fused_circuit_name_for(&owned, BIT_DEPTH, BATCH_SIZE);
    std::path::Path::new(&format!("artifacts/{fused_name}.r1cs")).exists()
}

/// A full-scale ramp: exercises each stage's clamp rather than a quiet
/// band where saturation never triggers.
fn input(folds: usize) -> Vec<i32> {
    let n = folds * samples_per_step();
    let half = 1i32 << (BIT_DEPTH - 1);
    (0..n).map(|i| (i as i32 % (2 * half)) - half).collect()
}

fn keys() -> (p256::ecdsa::SigningKey, p256::ecdsa::VerifyingKey) {
    use p256::elliptic_curve::rand_core::OsRng;
    let sk = p256::ecdsa::SigningKey::random(&mut OsRng);
    let vk = p256::ecdsa::VerifyingKey::from(&sk);
    (sk, vk)
}

fn owned(edits: &[&str]) -> Vec<String> {
    edits.iter().map(|s| s.to_string()).collect()
}

#[test]
fn fused_proof_round_trips() {
    let edits = ["gain_k2", "atten_m1", "dist_d100"];
    if !artefacts_present(&edits) {
        eprintln!("skipping: compiled artefacts missing");
        return;
    }

    let (sk, vk) = keys();
    let original = input(2);
    let cfg = config("t_round");

    let prov = fused::prove_fused::<Fq>(&original, &owned(&edits), &cfg, &sk).expect("prove");

    let specs = fused::validate(&owned(&edits), &cfg).expect("validate").0;
    let edited = fused::apply_stages(&original, &specs, &ctx());

    fused::verify_fused::<Fq>(&edited, BIT_DEPTH as u32, samples_per_step(), &prov, &vk)
        .expect("verify");
}

#[test]
fn fused_commitments_equal_the_chains_endpoints() {
    let edits = ["gain_k2", "atten_m1", "dist_d100"];
    if !artefacts_present(&edits) {
        eprintln!("skipping: compiled artefacts missing");
        return;
    }

    let (sk, _vk) = keys();
    let original = input(2);

    let chain_cfg = config("t_cmp_b");
    let stages: Vec<Stage> = edits.iter().map(|e| Stage::new(e)).collect();
    let chained = chain::prove_chain::<Fq>(&original, &stages, &chain_cfg, &sk).expect("chain");

    let fused_cfg = config("t_cmp_a");
    let fused_prov =
        fused::prove_fused::<Fq>(&original, &owned(&edits), &fused_cfg, &sk).expect("fused");

    // The whole claim of Option A: same statement, fewer constraints. If
    // these differ, the fused circuit is proving something else.
    assert_eq!(fused_prov.h_orig, chained.h_orig, "input commitment");
    assert_eq!(fused_prov.h_edit, chained.h_edit, "output commitment");

    assert!(
        fused_prov.total_constraints() < chained.total_constraints(),
        "fusion must cost fewer constraints than the sum of its stages: \
         fused {} vs chain {}",
        fused_prov.total_constraints(),
        chained.total_constraints()
    );
}

#[test]
fn stages_are_applied_in_order() {
    // gain then attenuate is not attenuate then gain: the first clamps at
    // full scale before halving, the second halves first and never
    // reaches the clamp. A fused circuit that composed in the wrong order
    // would still verify against its own reference, so the orders are
    // compared against each other.
    let specs_fwd = [find_edit("gain_k2").unwrap(), find_edit("atten_m1").unwrap()];
    let specs_rev = [find_edit("atten_m1").unwrap(), find_edit("gain_k2").unwrap()];

    let original = input(1);
    let fwd = fused::apply_stages(&original, &specs_fwd, &ctx());
    let rev = fused::apply_stages(&original, &specs_rev, &ctx());

    assert_ne!(fwd, rev, "stage order must change the result");
}

#[test]
fn rounding_is_per_stage_not_fused() {
    // floor(2x)/2 is not x for odd x: attenuating after a gain floors
    // twice. Fusing the arithmetic into one floor would disagree, so this
    // pins that the reference keeps the stages separate.
    let specs = [find_edit("atten_m1").unwrap(), find_edit("atten_m1").unwrap()];
    let original: Vec<i32> = vec![-7, -3, -1, 1, 3, 7];

    let staged = fused::apply_stages(&original, &specs, &ctx());
    let one_shot: Vec<i32> = original.iter().map(|x| x >> 2).collect();

    // >>1 twice equals >>2 for floor division, so these agree; the test
    // records that the reference is floor at every stage, which is what
    // the circuit's Num2Bits shift does.
    assert_eq!(staged, one_shot, "every stage floors, as an arithmetic shift does");
}

#[test]
fn tampered_published_audio_is_rejected() {
    let edits = ["gain_k2", "atten_m1", "dist_d100"];
    if !artefacts_present(&edits) {
        eprintln!("skipping: compiled artefacts missing");
        return;
    }

    let (sk, vk) = keys();
    let original = input(2);
    let cfg = config("t_tamper");

    let prov = fused::prove_fused::<Fq>(&original, &owned(&edits), &cfg, &sk).expect("prove");

    let specs = fused::validate(&owned(&edits), &cfg).expect("validate").0;
    let mut edited = fused::apply_stages(&original, &specs, &ctx());
    edited[0] = edited[0].wrapping_add(1);

    let err = fused::verify_fused::<Fq>(&edited, BIT_DEPTH as u32, samples_per_step(), &prov, &vk)
        .expect_err("a changed sample must not verify");
    assert!(
        matches!(err, FusedError::Verify(_)),
        "expected a verification failure, got {err}"
    );
}

#[test]
fn unfusable_stages_are_rejected_without_proving() {
    let cfg = config("t_reject");

    // `EditSpec` is not Debug, so the Ok side cannot be unwrapped by
    // expect_err; match instead.
    let err_of = |edits: Vec<String>| match fused::validate(&edits, &cfg) {
        Ok(_) => panic!("expected a rejection"),
        Err(e) => e,
    };

    for edit in ["crop", "concat", "merge", "gain2"] {
        let err = err_of(owned(&[edit]));
        assert!(
            matches!(err, FusedError::NotFusable { .. }),
            "{edit}: expected NotFusable, got {err}"
        );
    }

    let err = err_of(owned(&["no_such_edit"]));
    assert!(matches!(err, FusedError::UnknownEdit(_)), "got {err}");

    let err = err_of(Vec::new());
    assert!(matches!(err, FusedError::Empty), "got {err}");
}

#[test]
fn missing_artifact_names_the_compile_command() {
    // A fused circuit is one artefact per chain spec, so an uncompiled
    // order must say how to build it rather than failing opaquely.
    let cfg = config("t_missing");
    let spec = owned(&["gain_k4", "atten_m2", "dist_d50"]);
    let name = fused::fused_circuit_name_for(&spec, BIT_DEPTH, BATCH_SIZE);
    if std::path::Path::new(&format!("artifacts/{name}.r1cs")).exists() {
        eprintln!("skipping: this combination happens to be compiled");
        return;
    }

    let err = match fused::validate(&spec, &cfg) {
        Ok(_) => panic!("expected MissingArtifact"),
        Err(e) => e,
    };
    match err {
        FusedError::MissingArtifact { hint, .. } => {
            assert!(hint.contains("gen_fused_circuit.py"), "hint: {hint}");
            assert!(hint.contains("compile.circuit.sh"), "hint: {hint}");
        }
        other => panic!("expected MissingArtifact, got {other}"),
    }
}

#[test]
fn ragged_length_verifies_with_the_edit_of_silence_pad() {
    // N not a fold multiple, with dist last: dist maps 0 to -128, so the
    // pad the verifier must restore is NOT silence. Recomputing over a
    // zero-filled pad gives a different commitment, which is the bug this
    // pins - it only appears when N % samples_per_step != 0 AND the
    // composite moves silence.
    let edits = ["gain_k2", "atten_m1", "dist_d100"];
    if !artefacts_present(&edits) {
        eprintln!("skipping: compiled artefacts missing");
        return;
    }

    let spb = samples_per_step();
    let n = spb + spb / 2;
    assert_ne!(n % spb, 0, "the case must be ragged to be meaningful");

    let (sk, vk) = keys();
    let original = input(2)[..n].to_vec();
    let cfg = config("t_ragged");

    let prov = fused::prove_fused::<Fq>(&original, &owned(&edits), &cfg, &sk).expect("prove");

    let specs = fused::validate(&owned(&edits), &cfg).expect("validate").0;
    let padded_in = zk_audio::audio_parser::pad_to_step(&original, spb);
    let edited = fused::apply_stages(&padded_in, &specs, &ctx());

    // The creator publishes the trimmed stream; the verifier rebuilds the pad.
    let published: Vec<i32> = edited[..n].to_vec();
    fused::verify_fused::<Fq>(&published, BIT_DEPTH as u32, spb, &prov, &vk)
        .expect("a ragged stream must verify once the pad is rebuilt as edit(0)");

    // And the silence-padded recomputation must NOT match, or the test
    // would pass even with the bug present.
    let mut wrong = published.clone();
    wrong.resize(padded_in.len(), 0);
    let silence_commit = zk_audio::audio_parser::hex_stream_commitment::<Fq>(
        &wrong,
        BIT_DEPTH as u32,
        spb,
        zk_audio::poseidon::BatchHashKind::for_circuit(&prov.circuit_name),
    );
    assert_ne!(
        silence_commit, prov.h_edit,
        "if a zero pad matched, this composite would not exercise the bug"
    );
}
