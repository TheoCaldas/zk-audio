use zk_audio::{audio_editor, audio_parser::{self, WaveType}, circuit, signer};

fn main() {
    /* Test Circuit */
    let name = "multiplier";
    let circuit_filepath = &format!("artifacts/{}.r1cs", name);
    let witness_gen_filepath = &format!("artifacts/{}_js/{}.wasm", name, name);
    circuit::run_test(circuit_filepath, witness_gen_filepath);

    // /* Apply Transformations */
    // let input_name = "2c_44sr_16bd_01";
    // let original = audio_parser::read_samples(&format!("raw_audios/{}.wav", input_name));
    // let sample_rate = 44100;
    
    // // gain
    // let output_name = &format!("output/{}_gain", input_name);
    // let amount = 127;
    // let edited = audio_editor::gain_multiplier(&original, amount).unwrap();
    // audio_parser::write_file_16bits(
    //     &format!("raw_audios/{}.wav", output_name),
    //     &edited,
    //     sample_rate,
    //     WaveType::Stereo,
    // );

    // // distortion
    // let output_name = &format!("output/{}_dist", input_name);
    // let edited = audio_editor::distortion(&original);
    // audio_parser::write_file_16bits(
    //     &format!("raw_audios/{}.wav", output_name),
    //     &edited,
    //     sample_rate,
    //     WaveType::Stereo,
    // );

    // // pitch shifter
    // let output_name = &format!("output/{}_pshift", input_name);
    // let edited = audio_editor::pitch_shifter(&original, 0.9, sample_rate as f32);
    // audio_parser::write_file_16bits(
    //     &format!("raw_audios/{}.wav", output_name),
    //     &edited,
    //     sample_rate,
    //     WaveType::Stereo,
    // );
    
}
