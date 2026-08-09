//! Utilities for WAVE audio parsing.

use std::{collections::HashMap, fs, io::{BufWriter, Write}, path::Path};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::json;
use sha2::{Sha256, Digest};
use num_bigint::BigUint;

use ark_bn254::Fr;
use ark_ff::{PrimeField, BigInteger};
use light_poseidon::{Poseidon, PoseidonHasher, parameters::bn254_x5};

use crate::circuit::MultiplierInput;

pub type WaveSampleFormat = SampleFormat;
pub enum WaveType { Stereo, Mono }

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

/// Writes WAVE file to disk with 16bit bit depth
pub fn write_file_16bits(
    filepath: &str,
    samples: &Vec<i16>,
    sample_rate: u32,
    channels: WaveType
) {
    let spec = WavSpec {
        channels: match channels {
            WaveType::Stereo => 2,
            WaveType::Mono => 1,
        },
        sample_rate: sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    // Create parent directories if they do not exist
    if let Some(parent) = Path::new(filepath).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .expect("Failed creating parent directories");
        }
    }

    let writer = BufWriter::new(fs::File::create(filepath)
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

pub fn poseidon_hash_samples(samples: &[i16]) -> Fr {
    let mut h = Fr::from(0u64);
    let mut poseidon = Poseidon::<Fr>::new_circom(2).unwrap();

    for &sample in samples {
        h = poseidon
            .hash(&[
                h,
                Fr::from(u64::from(sample as u16)),
            ])
            .unwrap();
    }
    h
}

// pub fn poseidon_hash_chunks(chunks: &[BigUint]) -> Fr {
//     let mut poseidon = Poseidon::<Fr>::new_circom(2).unwrap();
//     let mut h = Fr::from(0u64);

//     for chunk in chunks {
//         // Convert the 240-bit integer into a BN254 field element.
//         let chunk_fr = Fr::from_le_bytes_mod_order(&chunk.to_bytes_le());
//         h = poseidon.hash(&[h, chunk_fr]).unwrap();
//     }
//     h
// }

pub fn poseidon_hash_chunks(chunks: &[BigUint]) -> Fr {
    assert!(!chunks.is_empty());
    let mut poseidon = Poseidon::<Fr>::new_circom(2).unwrap();
    let mut h = poseidon.hash(&[
        Fr::from_le_bytes_mod_order(&chunks[0].to_bytes_le()),
        Fr::from_le_bytes_mod_order(&chunks[1].to_bytes_le()),
    ]).unwrap();

    for chunk in &chunks[2..] {
        let chunk_fr = Fr::from_le_bytes_mod_order(&chunk.to_bytes_le());
        h = poseidon.hash(&[h, chunk_fr]).unwrap();
    }
    h
}

pub fn ex_poseidon_hash() -> () {
    let mut p = Poseidon::<Fr>::new(bn254_x5::get_poseidon_parameters(3).unwrap());
    // let mut p = Poseidon::<Fr>::new_circom(2).unwrap();
    let h = p.hash(&[
        Fr::from(1u64),
        Fr::from(2u64),
    ]).unwrap();
    println!("\nPoseidon(1, 2): 0x{:x}", BigUint::from_bytes_le(&h.into_bigint().to_bytes_le()));
}

pub fn hex_poseidon_hash_samples(samples: &[i16]) -> String {
    let hash = poseidon_hash_samples(samples);
    let s = format!("0x{:x}", BigUint::from_bytes_le(&hash.into_bigint().to_bytes_le()));
    s
}

pub fn hex_poseidon_hash_chunks(chunks: &[BigUint]) -> String {
    let hash = poseidon_hash_chunks(chunks);
    let s = format!("0x{:x}", BigUint::from_bytes_le(&hash.into_bigint().to_bytes_le()));
    s
}

/* --- SERIALIZE OPERATIONS --- */

pub fn compress_samples(samples: Vec<i16>) -> Vec<String> {
    let mut compressed = Vec::new();

    for chunk in samples.chunks(15) {
        let mut bytes = Vec::with_capacity(chunk.len() * 2);
        for &sample in chunk {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let value = BigUint::from_bytes_le(&bytes);
        compressed.push(format!("0x{}", value.to_str_radix(16)));
    }
    compressed
}

pub fn compress_samples_bigint(samples: Vec<i16>) -> Vec<BigUint> {
    let mut compressed = Vec::new();

    for chunk in samples.chunks(15) {
        let mut bytes = Vec::with_capacity(chunk.len() * 2);
        for &sample in chunk {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let value = BigUint::from_bytes_le(&bytes);
        compressed.push(value);
    }
    compressed
}

/// Normalizes i16 samples to non-negative strings for JSON serialization.
pub fn normalize_samples(samples: &[i16]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| ((s as i32) - (i16::MIN as i32)) as i16)
        .collect()
}

/// Serializes allocated samples to JSON file and write to disk.
pub fn serialize_samples(
    original: &Vec<i16>,
    edited: &Vec<i16>,
    batch_size: usize,
    filepath: &str
) {
    let mut batched_original = Vec::new();
    for batch in original.chunks(batch_size) {
        batched_original.push(compress_samples(normalize_samples(&batch)));
    }
    let mut batched_edited = Vec::new();
    for batch in edited.chunks(batch_size) {
        batched_edited.push(compress_samples(normalize_samples(&batch)));
    }
    let input_data = MultiplierInput {
        original: batched_original,
        edited: batched_edited,
    };
    let mut file = fs::File::create(filepath).unwrap();
    file.write_all(serde_json::to_string_pretty(&json!(input_data)).unwrap().as_bytes()).unwrap();
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
                write_file_16bits(
                    TEST_FILE, &SAMPLE_INPUT.to_vec(), 44100, WaveType::Mono
                );
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