use zk_audio::{audio_parser, signer, circuit};

fn main() {
    /* Test Circuit */
    let name = "dist_step";
    let circuit_filepath = &format!("artifacts/{}.r1cs", name);
    let witness_gen_filepath = &format!("artifacts/{}_js/{}.wasm", name, name);
    circuit::run_test(circuit_filepath, witness_gen_filepath);

    /* Test Audio Samples */
    let name = "test_12.16.01";
    audio_parser::print_specs(&format!("raw_audios/{}.wav", name));
    audio_parser::print_samples(&format!("raw_audios/{}.wav", name), 10).unwrap();

    /* Test Signature */
    let name = "260426";
    let key = signer::import_key(&format!("signers/private_p256_{}.pem", name));
    let text = "Hello, world!";
    let sig = dbg!(signer::sign_text(text, &key));
    
    // let hash = signer::hash_text(text);
    // dbg!(signer::sign_hash(hash, &key));

    let pub_key = signer::import_pub_key(&format!("signers/public_p256_{}.pem", name));
    let verified = signer::verify_text(text, &sig, &pub_key);
    println!("{}\n", format!("Text `{}` was verified: {}", text, verified));

    /* Sign Audio Samples */
    let name = "test_12.16.01";
    // let audio_hash = audio_parser::hash_samples_from(&format!("raw_audios/{}.wav", name));
    let samples = audio_parser::read_samples(&format!("raw_audios/{}.wav", name));
    let audio_hash = audio_parser::hash_samples(&samples);
    let sig = dbg!(signer::sign_hash(&audio_hash, &key));

    let verified = signer::verify_hash(&audio_hash, &sig, &pub_key);
    println!("{}\n", format!("Audio `raw_audios/{}.wav` was verified: {}", name, verified));
}
