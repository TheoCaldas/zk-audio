//! Sequential edit chains over real proofs: one `RecursiveSNARK` per
//! stage, linked by commitment equality.
//!
//! What these pin is the link itself - that stage `k`'s in-circuit
//! edited-stream commitment is bit-for-bit stage `k+1`'s in-circuit
//! original-stream commitment, with no off-circuit fixup in between. That
//! equality is the whole of Option B's soundness: break it and the stages
//! are proofs about unrelated streams.
//!
//! Real proofs, so slow and wanting `--release`:
//!
//! ```bash
//! cargo test --release --test chain_end_to_end
//! ```
//!
//! They need the compiled B=10 artefacts at bit depth 8 and the 8-bit
//! fixture; both are gitignored, so a missing one skips rather than
//! failing a fresh clone. See `scripts/README.md`.

use nova_snark::traits::Engine;

use zk_audio::{
    audio_parser,
    bench::config::{circuit_name_for, comp_size_for, find_edit},
    chain::{self, ChainConfig, ChainError, Stage},
};

type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

const BIT_DEPTH: usize = 8;
const BATCH_SIZE: usize = 10;
const FIXTURE: &str = "raw_audios/sweep_31s_2c_44sr_8bd.wav";
const FIXTURE_B: &str = "raw_audios/sweep_31s_2c_44sr_8bd_streamB.wav";

fn samples_per_step() -> usize {
    BATCH_SIZE * comp_size_for(BIT_DEPTH)
}

fn config(tag: &'static str) -> ChainConfig<'static> {
    ChainConfig {
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        period: 8820, // 5Hz at 44.1kHz
        serialized_dir: "serialized",
        tag,
        // The recursive chain is still fully verified; skipping
        // compression keeps these tests to seconds without weakening
        // what they check.
        skip_compressed: true,
        progress: false,
        audio_input: FIXTURE.to_string(),
    }
}

/// Returns `None` with a printed reason when an artefact or fixture is
/// missing, so a fresh clone stays green and the skip cannot pass
/// silently.
fn artefacts_present(edits: &[&str]) -> bool {
    for &e in edits {
        let spec = find_edit(e).expect("known edit");
        let name = circuit_name_for(spec.circuit_prefix, spec.factor, BIT_DEPTH, BATCH_SIZE);
        for needed in [
            format!("artifacts/{name}.r1cs"),
            format!("artifacts/{name}_js/{name}.wasm"),
        ] {
            if !std::path::Path::new(&needed).exists() {
                eprintln!("SKIP: {needed} not found - compile the circuit");
                return false;
            }
        }
    }
    for f in [FIXTURE] {
        if !std::path::Path::new(f).exists() {
            eprintln!("SKIP: {f} not found - generate the fixture");
            return false;
        }
    }
    true
}

fn input(folds: usize) -> Vec<i32> {
    let (samples, _bits) = audio_parser::read_samples_signed(FIXTURE);
    let total = samples_per_step() * folds;
    assert!(samples.len() >= total, "fixture too short");
    samples[..total].to_vec()
}

fn keys() -> (p256::ecdsa::SigningKey, p256::ecdsa::VerifyingKey) {
    use p256::elliptic_curve::rand_core::OsRng;
    let sk = p256::ecdsa::SigningKey::random(&mut OsRng);
    let vk = p256::ecdsa::VerifyingKey::from(&sk);
    (sk, vk)
}

/// Applies the chain's stages off-circuit, the way a creator would, to
/// get the stream a verifier is handed.
fn apply_stages(original: &[i32], edits: &[&str]) -> Vec<i32> {
    use zk_audio::bench::config::EditCtx;
    let comp_size = comp_size_for(BIT_DEPTH);
    let ctx = EditCtx {
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        start_index: 0,
        comp_size,
        period: 8820,
    };
    let mut cur = audio_parser::pad_to_step(original, samples_per_step());
    for &e in edits {
        let spec = find_edit(e).expect("known edit");
        cur = (spec.apply)(&cur, &ctx);
    }
    cur
}

/// **The test this module exists for.** A two-stage chain's middle
/// commitment must agree across the seam: what stage 0's circuit folded
/// as its edited stream is what stage 1's circuit folded as its original.
#[test]
fn middle_commitment_links_the_two_stages() {
    let edits = ["gain_k2", "atten_m1"];
    if !artefacts_present(&edits) {
        return;
    }
    let original = input(2);
    let stages: Vec<Stage> = edits.iter().map(|e| Stage::new(e)).collect();
    let (sk, _vk) = keys();

    let prov = chain::prove_chain::<Fq>(&original, &stages, &config("link"), &sk)
        .expect("chain proving");

    assert_eq!(prov.stages.len(), 2);
    assert_eq!(
        prov.stages[0].h_out, prov.stages[1].h_in,
        "the chain's middle commitment disagrees across the seam: stage 0 proved it \
         produced {}, stage 1 proved it consumed {}",
        prov.stages[0].h_out, prov.stages[1].h_in
    );
    // The seam is a real value, not two copies of a default.
    assert_ne!(prov.stages[0].h_out, prov.stages[0].h_in);
}

/// The happy path: prove a chain, verify it as a verifier holding only
/// the published audio and the bundle.
#[test]
fn three_stage_chain_round_trip() {
    let edits = ["gain_k2", "atten_m1", "dist_d100"];
    if !artefacts_present(&edits) {
        return;
    }
    let original = input(2);
    let stages: Vec<Stage> = edits.iter().map(|e| Stage::new(e)).collect();
    let (sk, vk) = keys();

    let prov = chain::prove_chain::<Fq>(&original, &stages, &config("rt3"), &sk)
        .expect("chain proving");

    let published = apply_stages(&original, &edits);

    chain::verify_chain::<Fq>(&published, BIT_DEPTH as u32, samples_per_step(), &prov, &vk)
        .expect("chain verification");

    assert_eq!(prov.stages.len(), 3);
    assert_eq!(prov.h_orig, prov.stages[0].h_in);
    assert_eq!(prov.h_edit, prov.stages[2].h_out);
}

/// A chain of one is the single-proof protocol, and must verify the same.
#[test]
fn single_stage_chain_matches_the_plain_protocol() {
    let edits = ["gain_k2"];
    if !artefacts_present(&edits) {
        return;
    }
    let original = input(2);
    let stages = vec![Stage::new("gain_k2")];
    let (sk, vk) = keys();

    let prov = chain::prove_chain::<Fq>(&original, &stages, &config("one"), &sk)
        .expect("chain proving");
    let published = apply_stages(&original, &edits);

    chain::verify_chain::<Fq>(&published, BIT_DEPTH as u32, samples_per_step(), &prov, &vk)
        .expect("chain verification");

    // The same commitment the single-proof protocol would compute.
    let direct = audio_parser::hex_stream_commitment::<Fq>(
        &audio_parser::pad_to_step(&original, samples_per_step()),
        BIT_DEPTH as u32,
        samples_per_step(),
        zk_audio::poseidon::BatchHashKind::PRODUCTION,
    );
    assert_eq!(prov.h_orig, direct);
}

/// Swapping the published audio after proving must be caught, exactly as
/// it is for a single proof.
#[test]
fn tampered_published_audio_is_rejected() {
    let edits = ["gain_k2", "atten_m1"];
    if !artefacts_present(&edits) {
        return;
    }
    let original = input(2);
    let stages: Vec<Stage> = edits.iter().map(|e| Stage::new(e)).collect();
    let (sk, vk) = keys();

    let prov = chain::prove_chain::<Fq>(&original, &stages, &config("tamper"), &sk)
        .expect("chain proving");

    let mut published = apply_stages(&original, &edits);
    published[0] = published[0].wrapping_add(1);

    let err = chain::verify_chain::<Fq>(
        &published,
        BIT_DEPTH as u32,
        samples_per_step(),
        &prov,
        &vk,
    )
    .expect_err("a tampered stream must not verify");
    assert!(
        matches!(err, ChainError::Stage { .. }),
        "expected a stage verification failure, got {err}"
    );
}

/// A forged seam must be caught: re-pointing a stage's input commitment
/// breaks the link even though every individual proof still verifies.
///
/// This is the attack the link check exists for - two valid proofs about
/// two unrelated streams, presented as a chain.
#[test]
fn broken_link_is_rejected() {
    let edits = ["gain_k2", "atten_m1"];
    if !artefacts_present(&edits) {
        return;
    }
    let original = input(2);
    let stages: Vec<Stage> = edits.iter().map(|e| Stage::new(e)).collect();
    let (sk, vk) = keys();

    let mut prov = chain::prove_chain::<Fq>(&original, &stages, &config("broken"), &sk)
        .expect("chain proving");
    let published = apply_stages(&original, &edits);

    // Stage 1 now claims to have consumed something stage 0 did not
    // produce.
    prov.stages[1].h_in = prov.stages[0].h_in.clone();

    let err = chain::verify_chain::<Fq>(
        &published,
        BIT_DEPTH as u32,
        samples_per_step(),
        &prov,
        &vk,
    )
    .expect_err("a broken link must not verify");
    assert!(
        matches!(err, ChainError::BrokenLink { at: 0, .. }),
        "expected a broken link at stage 0, got {err}"
    );
}

/// A time-varying stage composes: tremolo's proven counter is independent
/// of the stages around it, so it may sit anywhere in a chain.
#[test]
fn time_varying_stage_composes() {
    let edits = ["gain_k2", "tremolo_f5"];
    if !artefacts_present(&edits) {
        return;
    }
    let original = input(2);
    let stages: Vec<Stage> = edits.iter().map(|e| Stage::new(e)).collect();
    let (sk, vk) = keys();

    let prov = chain::prove_chain::<Fq>(&original, &stages, &config("trem"), &sk)
        .expect("chain proving");
    let published = apply_stages(&original, &edits);

    chain::verify_chain::<Fq>(&published, BIT_DEPTH as u32, samples_per_step(), &prov, &vk)
        .expect("chain verification");

    // The tremolo stage proves a counter; the gain stage has none.
    assert!(prov.stages[0].counter.is_none());
    assert!(prov.stages[1].counter.is_some());
}

/// Crop as a terminal stage: it changes the stream length, so the bundle
/// carries the input and output lengths separately.
#[test]
fn crop_as_terminal_stage() {
    let edits = ["gain_k2", "crop"];
    if !artefacts_present(&edits) {
        return;
    }
    let original = input(2);
    let stages: Vec<Stage> = edits.iter().map(|e| Stage::new(e)).collect();
    let (sk, vk) = keys();

    let prov = chain::prove_chain::<Fq>(&original, &stages, &config("crop"), &sk)
        .expect("chain proving");
    let published = apply_stages(&original, &edits);

    chain::verify_chain::<Fq>(&published, BIT_DEPTH as u32, samples_per_step(), &prov, &vk)
        .expect("chain verification");
}

/// Merge as a terminal stage: a two-input relation ends the chain, so a
/// verifier holding a signature over stem B can check the mix contains
/// precisely that stem.
#[test]
fn merge_as_terminal_stage() {
    let edits = ["gain_k2", "merge"];
    if !artefacts_present(&edits) || !std::path::Path::new(FIXTURE_B).exists() {
        eprintln!("SKIP: merge stage needs {FIXTURE_B}");
        return;
    }
    let original = input(2);
    let (b_samples, _bits) = audio_parser::read_samples_signed(FIXTURE_B);
    let b = b_samples[..original.len()].to_vec();

    let stages = vec![Stage::new("gain_k2"), Stage::with_stream_b("merge", b.clone())];
    let (sk, vk) = keys();

    let prov = chain::prove_chain::<Fq>(&original, &stages, &config("merge"), &sk)
        .expect("chain proving");

    // Stage 1's output is merge(gain(orig), b), computed the way the
    // creator would.
    let mid = apply_stages(&original, &["gain_k2"]);
    let spec = find_edit("merge").unwrap();
    let ctx = zk_audio::bench::config::EditCtx {
        bit_depth: BIT_DEPTH,
        batch_size: BATCH_SIZE,
        start_index: 0,
        comp_size: comp_size_for(BIT_DEPTH),
        period: 8820,
    };
    let published = (spec.apply2.unwrap())(
        &mid,
        &audio_parser::pad_to_step(&b, samples_per_step()),
        &ctx,
    );

    chain::verify_chain::<Fq>(&published, BIT_DEPTH as u32, samples_per_step(), &prov, &vk)
        .expect("chain verification");

    // The second stem's commitment is proven, so it can be checked
    // against a signature over stem B.
    assert!(prov.stages[1].h_b.is_some());
}

/// Validation runs before any proving, so a bad chain costs nothing.
#[test]
fn invalid_chains_are_rejected_without_proving() {
    let cfg = config("validate");

    assert!(matches!(chain::validate(&[], &cfg), Err(ChainError::Empty)));

    assert!(matches!(
        chain::validate(&[Stage::new("no_such_edit")], &cfg),
        Err(ChainError::UnknownEdit(_))
    ));

    // Concat commits to the two sides of the join separately, so no
    // single input commitment links a preceding stage to it.
    assert!(matches!(
        chain::validate(&[Stage::new("gain_k2"), Stage::new("concat")], &cfg),
        Err(ChainError::ConcatUnsupported)
    ));

    // Crop changes the stream length, so it may only come last.
    assert!(matches!(
        chain::validate(&[Stage::new("crop"), Stage::new("gain_k2")], &cfg),
        Err(ChainError::NonTerminalLengthChange { position: 0, .. })
    ));

    // Merge needs a second stream.
    assert!(matches!(
        chain::validate(&[Stage::new("merge")], &cfg),
        Err(ChainError::StreamBMismatch { expected: true, .. })
    ));

    // Gain takes only one.
    assert!(matches!(
        chain::validate(&[Stage::with_stream_b("gain_k2", vec![0; 310])], &cfg),
        Err(ChainError::StreamBMismatch { expected: false, .. })
    ));
}
