use zk_audio::{audio_parser};

#[test]
fn test_audio_header() {
    let name = "test_12.16.01";
    let filepath = &format!("raw_audios/{}.wav", name);
    audio_parser::print_specs(filepath);
    assert!(audio_parser::print_samples(filepath, 10).is_ok());
}