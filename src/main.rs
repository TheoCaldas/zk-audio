//! Single-run demo of the protocol, end to end:
//!
//!     hash -> sign -> prove -> verify proof -> recompute hash -> verify sig
//!
//! On success writes the edited stream to raw_audios/output/, the artefact
//! a creator publishes alongside (h_orig, sigma, pi).
//!
//! For sweeps across B and N: cargo run --release --bin bench

use nova_snark::traits::Engine;

use zk_audio::{
    audio_parser,
    bench::config::{circuit_name_for, comp_size_for, find_edit, EditCtx},
    circuit::{CaseMeta, RunConfig},
    poseidon::BatchHashKind,
    protocol, signer,
};

/// The field the circuits prove in: Nova's primary-curve scalar field.
type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

fn main() {
    /* Inputs */
    let bit_depth = 16;
    let batch_size = 100;
    let max_size = 600000;
    // clamp(2x), proven by circuits/edits/gain/gain_step.circom.
    let edit_name = "gain_k2";
    // Capped at 1/4 full scale, so gain_k2 never saturates on it. Use
    // sweep_31s_2c_44sr_16bd_fullscale to exercise clamping.
    let input_name = "sweep_120s_2c_44sr_16bd";
    let key_id = "p256_260426";

    // Naming convention: b = bit depth, B = batch size, N = total samples.
    let comp_size = comp_size_for(bit_depth);
    let edit = find_edit(edit_name).expect("unknown edit");

    // One batch covers batch_size * comp_size samples; N must be a whole
    // number of batches or the circuit rejects the short final one.
    let samples_per_step = batch_size * comp_size;
    assert_eq!(
        max_size % samples_per_step,
        0,
        "N={} is not a multiple of {} samples/step (B={})",
        max_size,
        samples_per_step,
        batch_size
    );

    // Each family encodes its parameter differently (gain_b16_k3_B100,
    // atten_b8_m1_B100), so use the shared namer.
    let circuit_name = &circuit_name_for(edit.circuit_prefix, edit.factor, bit_depth, batch_size);
    let circuit_filepath = &format!("artifacts/{}.r1cs", circuit_name);
    // The commitment must use the construction this circuit hashes with.
    let hash_kind = BatchHashKind::for_circuit(circuit_name);
    let witness_gen_filepath = &format!("artifacts/{}_js/{}.wasm", circuit_name, circuit_name);

    /* Read Audio Samples */
    // i32 so 24-bit samples survive.
    let input_filepath = &format!("raw_audios/{}.wav", input_name);
    let (original, _file_bits) = audio_parser::read_samples_signed(input_filepath);
    // Needed to write the edited stream back out in the input's own format.
    let (sample_rate, channels) = audio_parser::read_rate_and_channels(input_filepath);
    assert!(
        original.len() >= max_size,
        "audio has {} samples, need {}",
        original.len(),
        max_size
    );
    let original = &original[0..max_size];

    assert!(
        edit.bit_depths.contains(&bit_depth),
        "edit '{}' has no circuit at bit depth {}; available: {:?}",
        edit_name,
        bit_depth,
        edit.bit_depths
    );

    /* Apply Edit */
    let ctx = EditCtx {
        bit_depth,
        batch_size,
        start_index: 0,
        comp_size,
        period: 8820, // 5Hz at 44.1kHz, for time-varying edits
    };
    let edited = (edit.apply)(original, &ctx);

    /* Serialize Samples */
    let json_input_filepath = &format!("serialized/{}_N{}.json", circuit_name, max_size);
    let orig_wide: Vec<i32> = original.to_vec();
    let edit_wide: Vec<i32> = edited.iter().map(|&s| s as i32).collect();
    audio_parser::serialize_samples_at(
        &orig_wide,
        &edit_wide,
        samples_per_step,
        bit_depth as u32,
        json_input_filepath,
    );

    /* Load Signing Keys */
    // The creator signs; the verifier below holds only the public half.
    let signing_key = signer::import_key(&format!("signers/private_{}.pem", key_id));
    let verifying_key = signer::import_pub_key(&format!("signers/public_{}.pem", key_id));

    let cfg = RunConfig {
        circuit_filepath,
        witness_gen_filepath,
        json_input_filepath,
        skip_compressed: false,
        verbose: true,
        multi_input: edit.multi_input,
        initial_state: Vec::new(),
        state_width: edit.state_width,
    };
    let meta = CaseMeta {
        circuit_name: circuit_name.clone(),
        edit: edit_name.to_string(),
        audio_input: input_name.to_string(),
        bit_depth,
        batch_size,
        comp_size,
        total_samples: max_size,
        repetition: 0,
    };

    /* --- CREATOR: commit, sign, prove --- */
    println!();
    println!("=== Creator: commit -> sign -> prove ===");
    let prov = protocol::create_provenance::<Fq>(
        &orig_wide,
        &edit_wide,
        bit_depth as u32,
        samples_per_step,
        hash_kind,
        &signing_key,
        &cfg,
        &meta,
    )
    .expect("creating provenance failed");

    println!("h_orig (signed)   : {}", prov.h_orig);
    println!("h_edit (from proof): {}", prov.h_edit);

    /* --- VERIFIER: recompute the edited commitment, check the signature --- */
    // Gets the edited audio and the bundle, never `orig_wide`.
    println!();
    println!("=== Verifier: verify proof -> recompute hash -> verify signature ===");
    match protocol::verify_provenance::<Fq>(
        &edit_wide,
        bit_depth as u32,
        samples_per_step,
        hash_kind,
        &prov,
        &verifying_key,
    ) {
        Ok(()) => {
            println!("edited-stream commitment matches the proof's terminal state");
            println!("signature over the committed original verifies");

            // Write only after verification passes. The proof binds to
            // these exact samples, so re-encoding the file invalidates it.
            let output_filepath =
                &format!("raw_audios/output/{}_{}.wav", input_name, edit_name);
            audio_parser::write_file_at(
                output_filepath,
                &edit_wide,
                sample_rate,
                bit_depth as u32,
                match channels {
                    1 => audio_parser::WaveType::Mono,
                    _ => audio_parser::WaveType::Stereo,
                },
            );
            println!("wrote edited audio to {}", output_filepath);

            println!();
            println!("PROTOCOL OK: the published edit is provably {} of an original", edit_name);
            println!("signed by the key holder, and the original was never revealed.");
        }
        Err(e) => {
            println!("PROTOCOL FAILED: {}", e);
            std::process::exit(1);
        }
    }
}
