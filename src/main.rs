//! Single-run demo of the proving pipeline.
//! For sweeps across B and N, use the benchmark suite:
//!     cargo run --release --bin bench -- --max-n 6000

use zk_audio::{
    audio_parser,
    bench::config::{circuit_name_for, comp_size_for, find_edit, EditCtx},
    circuit,
};

fn main() {
    /* Inputs */
    let bit_depth = 16;
    let batch_size = 100;
    let max_size = 600000;
    let edit_name = "gain2";
    let input_name = "2c_44sr_16bd_01";

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

    // Use the shared namer, not an inline format!: the gain and atten
    // families encode their factor in the stem (gain_b16_k3_B100,
    // atten_b8_m1_B100), so a hand-rolled "{prefix}_b{bd}_B{B}" only ever
    // resolves for the gain2 baseline.
    let circuit_name = &circuit_name_for(edit.circuit_prefix, edit.factor, bit_depth, batch_size);
    let circuit_filepath = &format!("artifacts/{}.r1cs", circuit_name);
    let witness_gen_filepath = &format!("artifacts/{}_js/{}.wasm", circuit_name, circuit_name);

    /* Read Audio Samples */
    let original = audio_parser::read_samples(&format!("raw_audios/{}.wav", input_name));
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
    // Depth-generic serializer; byte-identical to the legacy 16-bit one at
    // bit_depth=16, and the only correct choice at 8.
    let orig_wide: Vec<i32> = original.iter().map(|&s| s as i32).collect();
    let edit_wide: Vec<i32> = edited.iter().map(|&s| s as i32).collect();
    audio_parser::serialize_samples_at(
        &orig_wide,
        &edit_wide,
        samples_per_step,
        bit_depth as u32,
        json_input_filepath,
    );

    /* Test Circuit */
    circuit::run_test(circuit_filepath, witness_gen_filepath, json_input_filepath);
}
