//! The on-disk bundle formats, and the proof-persistence API the CLI's
//! verify step depends on.

use zk_audio::cli::bundle::{self, ProofBundle, SignatureBundle};

#[test]
fn signature_bundle_round_trips() {
    let path = "/tmp/_zk_sig_test.json";
    let b = SignatureBundle {
        kind: SignatureBundle::KIND.to_string(),
        h_orig: "0x1234".to_string(),
        signature_der_hex: "abcd".to_string(),
        public_key_pem: "-----BEGIN PUBLIC KEY-----\n-----END PUBLIC KEY-----\n".to_string(),
        bit_depth: 8,
        batch_size: 10,
        samples: 620,
        padded_samples: 620,
        source_file: "x.wav".to_string(),
    };
    bundle::write(path, &b).expect("write");
    let back: SignatureBundle = bundle::read(path).expect("read");
    assert_eq!(back.kind, SignatureBundle::KIND);
    assert_eq!(back.h_orig, "0x1234");
    // bit_depth and batch_size must survive: h_orig is a fold over a
    // specific partition, so a verifier cannot recompute without them.
    assert_eq!(back.bit_depth, 8);
    assert_eq!(back.batch_size, 10);
    let _ = std::fs::remove_file(path);
}

#[test]
fn proof_bundle_round_trips() {
    let path = "/tmp/_zk_proof_test.json";
    let b = ProofBundle {
        kind: ProofBundle::KIND.to_string(),
        circuit_name: "gain_b8_k2_B10".to_string(),
        edit: "gain_k2".to_string(),
        bit_depth: 8,
        batch_size: 10,
        comp_size: 31,
        samples: 620,
        padded_samples: 620,
        iterations: 2,
        z0: vec![0, 0],
        h_orig: "0xaa".to_string(),
        h_edit: "0xbb".to_string(),
        h_b: None,
        len_a: None,
        proof: serde_json::json!({"stub": true}),
    };
    bundle::write(path, &b).expect("write");
    let back: ProofBundle = bundle::read(path).expect("read");
    // circuit_name is what lets the verifier re-derive a key without being
    // told which circuit to use.
    assert_eq!(back.circuit_name, "gain_b8_k2_B10");
    assert_eq!(back.iterations, 2);
    assert_eq!(back.z0, vec![0, 0]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn reading_a_malformed_bundle_is_an_error_not_a_panic() {
    let path = "/tmp/_zk_bad_test.json";
    std::fs::write(path, "{ not json").unwrap();
    let r: Result<ProofBundle, _> = bundle::read(path);
    assert!(r.is_err());
    let _ = std::fs::remove_file(path);
}

/// The proof must survive a round trip through disk and verify against a
/// verifier key derived independently from the circuit - the path option C
/// takes, where no proving run is available to supply the key.
#[test]
fn proof_verifies_against_an_independently_derived_key() {
    use zk_audio::{
        audio_parser,
        bench::config::{circuit_name_for, comp_size_for, find_edit, EditCtx},
        circuit::{self, CaseMeta, RunConfig},
    };

    let (bd, b) = (8usize, 10usize);
    let comp = comp_size_for(bd);
    let sps = b * comp;
    let n = sps * 2;
    let edit = find_edit("gain_k2").expect("edit");
    let name = circuit_name_for(edit.circuit_prefix, edit.factor, bd, b);
    let r1cs = format!("artifacts/{name}.r1cs");
    let wasm = format!("artifacts/{name}_js/{name}.wasm");
    let fixture = "raw_audios/sweep_31s_2c_44sr_8bd.wav";
    for p in [r1cs.as_str(), wasm.as_str(), fixture] {
        if !std::path::Path::new(p).exists() {
            eprintln!("SKIP: {p} not found");
            return;
        }
    }

    let (samples, _) = audio_parser::read_samples_signed(fixture);
    let original: Vec<i32> = samples[..n].to_vec();
    let ctx = EditCtx { bit_depth: bd, batch_size: b, start_index: 0, comp_size: comp, period: 8820 };
    let edited: Vec<i32> = (edit.apply)(&original, &ctx).iter().map(|&s| s as i32).collect();

    let json = format!("/tmp/_zk_cli_vk_{n}.json");
    audio_parser::serialize_samples_at(&original, &edited, sps, bd as u32, &json);

    let cfg = RunConfig {
        circuit_filepath: &r1cs,
        witness_gen_filepath: &wasm,
        json_input_filepath: &json,
        skip_compressed: false,
        verbose: false,
        progress: false,
        multi_input: false,
        initial_state: Vec::new(),
        state_width: edit.state_width,
    };
    let meta = CaseMeta {
        circuit_name: name.clone(),
        edit: "gain_k2".to_string(),
        audio_input: fixture.to_string(),
        bit_depth: bd,
        batch_size: b,
        comp_size: comp,
        total_samples: n,
        repetition: 0,
    };

    let (metrics, artifacts) = circuit::run_proof_keep(&cfg, &meta).expect("prove");
    let artifacts = artifacts.expect("compressed proof kept");
    let _ = std::fs::remove_file(&json);

    let vk = circuit::derive_vk(&r1cs).expect("derive vk from the circuit alone");
    let state = circuit::verify_compressed(&artifacts.proof_json, &vk, metrics.iterations, &[0, 0])
        .expect("verify with the derived key");
    assert_eq!(state[0], metrics.final_hash_original);
    assert_eq!(state[1], metrics.final_hash_edited);

    // A corrupted proof must be rejected rather than silently accepted.
    let mut bad = artifacts.proof_json.clone();
    let i = bad.len() / 2;
    bad[i] = if bad[i] == b'1' { b'2' } else { b'1' };
    assert!(circuit::verify_compressed(&bad, &vk, metrics.iterations, &[0, 0]).is_err());
}

/// The padded length is what the proof folds and the true length is what
/// the signature binds, so a bundle that lost either could not be checked.
#[test]
fn bundles_carry_both_lengths() {
    let path = "/tmp/_zk_padded_test.json";
    let b = ProofBundle {
        kind: ProofBundle::KIND.to_string(),
        circuit_name: "gain_b8_k2_B10".to_string(),
        edit: "gain_k2".to_string(),
        bit_depth: 8,
        batch_size: 10,
        comp_size: 31,
        samples: 317,
        padded_samples: 620,
        iterations: 2,
        z0: vec![0, 0],
        h_orig: "0xaa".to_string(),
        h_edit: "0xbb".to_string(),
        h_b: None,
        len_a: None,
        proof: serde_json::json!({"stub": true}),
    };
    bundle::write(path, &b).expect("write");
    let back: ProofBundle = bundle::read(path).expect("read");
    assert_eq!(back.samples, 317, "the true length must survive");
    assert_eq!(back.padded_samples, 620);
    assert_eq!(
        back.iterations,
        back.padded_samples / (back.batch_size * back.comp_size),
        "iterations count padded steps, not requested samples"
    );
    let _ = std::fs::remove_file(path);
}

/// A v1 bundle predates the length binding, so it must not deserialize
/// silently into a v2 one with a zero length.
#[test]
fn a_bundle_without_a_padded_length_is_rejected() {
    let path = "/tmp/_zk_v1_test.json";
    std::fs::write(
        path,
        r#"{"kind":"zk-audio-proof/1","circuit_name":"x","edit":"gain_k2",
            "bit_depth":8,"batch_size":10,"comp_size":31,"samples":620,
            "iterations":2,"z0":[0,0],"h_orig":"0xaa","h_edit":"0xbb",
            "proof":{}}"#,
    )
    .unwrap();
    let r: Result<ProofBundle, _> = bundle::read(path);
    assert!(r.is_err(), "a v1 bundle must not load as v2");
    let _ = std::fs::remove_file(path);
}

/// A merge bundle names two sources, so both commitments must survive a
/// round trip; a verifier checks each against its own signature.
#[test]
fn a_two_input_bundle_round_trips_both_sources() {
    let path = "/tmp/_zk_merge_bundle.json";
    let b = ProofBundle {
        kind: ProofBundle::KIND.to_string(),
        circuit_name: "merge_b8_avg_B10".to_string(),
        edit: "merge".to_string(),
        bit_depth: 8,
        batch_size: 10,
        comp_size: 31,
        samples: 3100,
        padded_samples: 3100,
        iterations: 10,
        z0: vec![0, 0, 0],
        h_orig: "0xaa".to_string(),
        h_edit: "0xbb".to_string(),
        h_b: Some("0xcc".to_string()),
        len_a: None,
        proof: serde_json::json!({"stub": true}),
    };
    bundle::write(path, &b).expect("write");
    let back: ProofBundle = bundle::read(path).expect("read");
    assert_eq!(back.h_b.as_deref(), Some("0xcc"));
    assert_eq!(back.len_a, None, "merge has no join point");
    let _ = std::fs::remove_file(path);
}

/// Concat's join point is part of the statement: the circuit carries it
/// through the fold chain, so a bundle that lost it could not be checked.
#[test]
fn a_concat_bundle_carries_its_join_point() {
    let path = "/tmp/_zk_concat_bundle.json";
    let b = ProofBundle {
        kind: ProofBundle::KIND.to_string(),
        circuit_name: "concat_b8_B10".to_string(),
        edit: "concat".to_string(),
        bit_depth: 8,
        batch_size: 10,
        comp_size: 31,
        samples: 6200,
        padded_samples: 6200,
        iterations: 20,
        z0: vec![0, 0, 0, 0, 3100],
        h_orig: "0xaa".to_string(),
        h_edit: "0xbb".to_string(),
        h_b: Some("0xcc".to_string()),
        len_a: Some(3100),
        proof: serde_json::json!({"stub": true}),
    };
    bundle::write(path, &b).expect("write");
    let back: ProofBundle = bundle::read(path).expect("read");
    assert_eq!(back.len_a, Some(3100));
    // The join must also be the value z0 hands the circuit, or the proof
    // would be verified against a different boundary than the bundle names.
    assert_eq!(back.z0.get(4).copied(), Some(3100));
    assert_eq!(back.len_a.unwrap() % (back.batch_size * back.comp_size), 0);
    let _ = std::fs::remove_file(path);
}

/// A single-input bundle omits both fields rather than writing nulls, so
/// `h_b.is_some()` is what tells the verifier to ask for a second signature.
#[test]
fn a_single_input_bundle_omits_the_two_input_fields() {
    let path = "/tmp/_zk_single_bundle.json";
    let b = ProofBundle {
        kind: ProofBundle::KIND.to_string(),
        circuit_name: "gain_b8_k2_B10".to_string(),
        edit: "gain_k2".to_string(),
        bit_depth: 8,
        batch_size: 10,
        comp_size: 31,
        samples: 620,
        padded_samples: 620,
        iterations: 2,
        z0: vec![0, 0],
        h_orig: "0xaa".to_string(),
        h_edit: "0xbb".to_string(),
        h_b: None,
        len_a: None,
        proof: serde_json::json!({"stub": true}),
    };
    bundle::write(path, &b).expect("write");
    let raw = std::fs::read_to_string(path).expect("read raw");
    assert!(!raw.contains("h_b"), "h_b must be omitted, not null");
    assert!(!raw.contains("len_a"), "len_a must be omitted, not null");
    let _ = std::fs::remove_file(path);
}
