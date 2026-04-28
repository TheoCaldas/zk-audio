//! Utilities for WAVE audio parsing.

use std::{fs::File, io::BufWriter};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use sha2::{Sha256, Digest};

pub type WaveSampleFormat = SampleFormat;

/// Converts SampleFormat enum to string for printing.
/// # Examples
/// ```
/// use zk_audio::audio_parser::{fmt_to_str, WaveSampleFormat};
/// assert_eq!(fmt_to_str(&WaveSampleFormat::Int), "Int");
/// assert_eq!(fmt_to_str(&WaveSampleFormat::Float), "Float");
/// ```
pub fn fmt_to_str<'a>(fmt: &'a WaveSampleFormat) -> &'a str {
    return match fmt {
        WaveSampleFormat::Float => "Float",
        WaveSampleFormat::Int => "Int",
    }
}

/* --- READ/WRITE OPERATIONS --- */

/// Prints the specifications of a WAVE audio file from disk, such as sample rate, channels, bits per sample, etc.
pub fn print_specs(filepath: &str) {
    let reader = WavReader::open(filepath).unwrap();
    let spec = reader.spec();

    println!("Sample rate: {}", spec.sample_rate);
    println!("Channels: {}", spec.channels);
    println!("Bits per sample: {}", spec.bits_per_sample);
    println!("Sample format: {}", fmt_to_str(&spec.sample_format));
    println!("Number of Samples (Stereo/Mono): {}", reader.duration());
    println!("Duration: {} seconds", reader.duration() / spec.sample_rate);
}

/// Prints the first few samples of a WAVE audio file from disk.
pub fn print_samples(filepath: &str, first_few: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = WavReader::open(filepath)?;
    let samples = reader.samples::<i16>();
    for sample in samples.take(first_few) {
        println!("{}", sample?);
    }
    Ok(())
}

/// Reads samples of a WAVE file and returns allocated i16 vector.
/// * Use `reader.samples` iterator instead if audio too big.
pub fn read_samples(filepath: &str) -> Vec<i16> {
    let mut reader = WavReader::open(filepath).unwrap();
    reader.samples::<i16>().flatten().collect()
}

/// Writes WAVE file to disk with (Mono, 44100Hz, i-16bit) format
pub fn write_file_44100hz_16bits_mono(filepath: &str, samples: &Vec<i16>) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let writer = BufWriter::new(File::create(filepath)
        .expect("Failed creating WAVE file"));
    let mut wav_writer = WavWriter::new(writer, spec)
        .expect("Failed writing WAVE specs to file");

    for s in samples {
        wav_writer.write_sample(*s).expect("Failed writing sample to file");
    }
    wav_writer.finalize().expect("Failed updating WAVE file header");
}

/* --- HASH OPERATIONS --- */

/// Hashes the samples of a WAVE file from disk.
/// * Use this instead of `hash_samples` if audio too big to be allocated.
pub fn hash_samples_from(filepath: &str) -> Vec<u8> {
    let mut reader = WavReader::open(filepath).unwrap();
    let mut hasher = Sha256::new();
    for sample in reader.samples::<i16>() {
        let bytes = sample.unwrap().to_le_bytes();
        hasher.update(bytes);
    }
    hasher.finalize().to_vec()
}

/// Hashes allocated samples using SHA256 and returns byte vec.
pub fn hash_samples(samples: &Vec<i16>) -> Vec<u8> {
    let mut hasher = Sha256::new();
    for sample in samples {
        let bytes = sample.to_le_bytes();
        hasher.update(bytes);
    }
    hasher.finalize().to_vec()
}

/* --- UNIT TESTS --- */

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    const TEST_FILE: &str = "raw_audios/unit_test_audio.wav";

    static SETUP_INSTANCE: Once = Once::new();

    // Fixed test data
    const SAMPLE_INPUT: &[i16] = &[1, -2, 3, -4, 5];

    // Precomputed expected hash for SAMPLE_INPUT
    const EXPECTED_HASH: [u8; 32] = [
        13, 63, 160, 191,
        249, 176, 105, 133,
        195, 67, 180, 171,
        20, 49, 140, 174,
        134, 197, 102, 205,
        141, 65, 134, 125,
        53, 12, 187, 255,
        66, 245, 54, 146
    ];

    fn setup() {
        SETUP_INSTANCE.call_once(|| {
            if !std::path::Path::new(TEST_FILE).exists() {
                write_file_44100hz_16bits_mono(TEST_FILE, &SAMPLE_INPUT.to_vec());
            }
        });
    }

    #[test]
    fn test_read_samples() {
        setup();
        let samples = read_samples(TEST_FILE);
        assert_eq!(samples, SAMPLE_INPUT);
    }

    #[test]
    fn test_hash_samples_from() {
        setup();
        let hash = hash_samples_from(TEST_FILE);
        assert_eq!(hash.as_slice(), &EXPECTED_HASH);
    }

    #[test]
    fn test_hash_samples_matches() {
        setup();
        let samples = read_samples(TEST_FILE);
        let h1 = hash_samples(&samples);
        let h2 = hash_samples_from(TEST_FILE);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_print_samples_ok() {
        setup();
        let result = print_samples(TEST_FILE, 3);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_specs_no_panic() {
        setup();
        print_specs(TEST_FILE);
    }
}