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

/// Reads a WAV file as signed samples at its own bit depth, returning
/// `(samples, bits_per_sample)`.
///
/// 8-bit WAV stores samples unsigned on disk (0..=255, centered at 128)
/// while 16-bit stores them signed, but `hound` already normalizes that
/// away: it yields signed values in the range implied by the depth for
/// both, so no recentring is applied here. (Verified against an 8-bit
/// fixture: hound returns -109..=109 for data stored as 19..=237.)
pub fn read_samples_signed(filepath: &str) -> (Vec<i32>, u32) {
    let mut reader = WavReader::open(filepath).unwrap();
    let bits = reader.spec().bits_per_sample as u32;
    let samples: Vec<i32> = reader.samples::<i32>().flatten().collect();
    (samples, bits)
}

/// Writes a WAV file at an arbitrary supported bit depth, applying the
/// inverse of the 8-bit unsigned-storage convention handled by
/// [`read_samples_signed`].
pub fn write_file_at(
    filepath: &str,
    samples: &[i32],
    sample_rate: u32,
    bit_depth: u32,
    channels: WaveType,
) {
    let spec = WavSpec {
        channels: match channels {
            WaveType::Stereo => 2,
            WaveType::Mono => 1,
        },
        sample_rate,
        bits_per_sample: bit_depth as u16,
        sample_format: SampleFormat::Int,
    };

    if let Some(parent) = Path::new(filepath).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).expect("Failed creating parent directories");
        }
    }

    let writer = BufWriter::new(fs::File::create(filepath).expect("Failed creating WAVE file"));
    let mut wav_writer = WavWriter::new(writer, spec).expect("Failed writing WAVE specs to file");

    for &s in samples {
        // hound handles the 8-bit unsigned storage convention itself, so
        // signed samples go straight through at every depth.
        wav_writer
            .write_sample(s)
            .expect("Failed writing sample to file");
    }
    wav_writer.finalize().expect("Failed updating WAVE file header");
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

/// Samples packed into one field element at `bit_depth`, matching
/// `Decompressor`/`Recompressor` in circom: `(256 - bit_depth) / bit_depth`.
/// At bd=16 this is 15; at bd=8, 31.
pub fn comp_size_for(bit_depth: u32) -> usize {
    ((256 - bit_depth) / bit_depth) as usize
}

/// Packs already-normalized (non-negative) samples into field elements,
/// `comp_size_for(bit_depth)` samples per element, using the positional
/// weights `2^(bit_depth*i)` that `Recompressor` applies in-circuit.
///
/// Generic over bit depth: the old 16-bit-only version hardcoded both
/// the chunk size (15) and the 2-byte little-endian layout, neither of
/// which holds at bd=8.
pub fn compress_samples_at(samples: &[i32], bit_depth: u32) -> Vec<String> {
    compress_samples_bigint_at(samples, bit_depth)
        .into_iter()
        .map(|v| format!("0x{}", v.to_str_radix(16)))
        .collect()
}

/// Shared packer: shifts each normalized sample into its positional slot.
/// Bit-shifting rather than byte concatenation, so any bit depth works
/// (byte layout only lines up when bit_depth is a multiple of 8).
pub fn compress_samples_bigint_at(samples: &[i32], bit_depth: u32) -> Vec<BigUint> {
    let comp = comp_size_for(bit_depth);
    let limit = 1i64 << bit_depth;
    let mut compressed = Vec::new();

    for chunk in samples.chunks(comp) {
        let mut value = BigUint::from(0u32);
        for (i, &sample) in chunk.iter().enumerate() {
            assert!(
                sample >= 0 && (sample as i64) < limit,
                "sample {} is not a normalized {}-bit value (expected 0..{})",
                sample, bit_depth, limit
            );
            value |= BigUint::from(sample as u64) << (bit_depth as usize * i);
        }
        compressed.push(value);
    }
    compressed
}

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

/// Shifts signed samples into the non-negative range for `bit_depth` by
/// adding `2^(bit_depth-1)`, so they can be packed as unsigned digits.
/// The circuit undoes this offset internally (see `gain_step.circom`).
pub fn normalize_samples_at(samples: &[i32], bit_depth: u32) -> Vec<i32> {
    let half = 1i32 << (bit_depth - 1);
    samples.iter().map(|&s| s + half).collect()
}

/// Serializes a batched (a, b, out) triple for multi-input edits.
///
/// `a` lands in `original` and `out` in `edited`, matching the field
/// names every other edit uses, so existing tooling reads the same two
/// slots; `b` is carried in the optional `stream_b` field.
pub fn serialize_merge_at(
    a: &[i32],
    b: &[i32],
    out: &[i32],
    batch_size: usize,
    bit_depth: u32,
    filepath: &str,
) {
    let batch = |src: &[i32]| -> Vec<Vec<String>> {
        src.chunks(batch_size)
            .map(|c| compress_samples_at(&normalize_samples_at(c, bit_depth), bit_depth))
            .collect()
    };
    let input_data = MultiplierInput {
        original: batch(a),
        edited: batch(out),
        stream_b: Some(batch(b)),
    };
    let mut file = fs::File::create(filepath).unwrap();
    file.write_all(
        serde_json::to_string_pretty(&json!(input_data))
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
}

/// Serializes a batched (original, edited) pair at an arbitrary bit depth.
/// The generic counterpart to [`serialize_samples`], which is 16-bit only.
pub fn serialize_samples_at(
    original: &[i32],
    edited: &[i32],
    batch_size: usize,
    bit_depth: u32,
    filepath: &str,
) {
    let mut batched_original = Vec::new();
    for batch in original.chunks(batch_size) {
        batched_original.push(compress_samples_at(
            &normalize_samples_at(batch, bit_depth),
            bit_depth,
        ));
    }
    let mut batched_edited = Vec::new();
    for batch in edited.chunks(batch_size) {
        batched_edited.push(compress_samples_at(
            &normalize_samples_at(batch, bit_depth),
            bit_depth,
        ));
    }
    let input_data = MultiplierInput {
        original: batched_original,
        edited: batched_edited,
        stream_b: None,
    };
    let mut file = fs::File::create(filepath).unwrap();
    file.write_all(
        serde_json::to_string_pretty(&json!(input_data))
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
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
        stream_b: None,
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
#[cfg(test)]
mod packing_tests {
    use super::*;

    #[test]
    fn comp_size_matches_circom_formula() {
        assert_eq!(comp_size_for(16), 15);
        assert_eq!(comp_size_for(8), 31);
    }

    /// The generic bit-shift packer must agree byte-for-byte with the
    /// original 16-bit byte-concatenation packer, or every previously
    /// generated input file and commitment hash becomes incomparable.
    #[test]
    fn generic_packer_matches_legacy_16bit_packer() {
        let samples: Vec<i16> = (0..60).map(|i| (i * 1091) as i16).collect();
        let legacy = compress_samples(normalize_samples(&samples));

        let wide: Vec<i32> = samples.iter().map(|&s| s as i32).collect();
        let generic = compress_samples_at(&normalize_samples_at(&wide, 16), 16);

        assert_eq!(legacy, generic);
    }

    #[test]
    fn generic_packer_matches_legacy_on_extremes() {
        let samples: Vec<i16> = vec![i16::MIN, -1, 0, 1, i16::MAX, i16::MIN, 12345];
        let legacy = compress_samples(normalize_samples(&samples));
        let wide: Vec<i32> = samples.iter().map(|&s| s as i32).collect();
        let generic = compress_samples_at(&normalize_samples_at(&wide, 16), 16);
        assert_eq!(legacy, generic);
    }

    #[test]
    fn normalize_maps_signed_range_to_unsigned() {
        assert_eq!(normalize_samples_at(&[-32768, 0, 32767], 16), vec![0, 32768, 65535]);
        assert_eq!(normalize_samples_at(&[-128, 0, 127], 8), vec![0, 128, 255]);
    }

    /// Packing is the inverse of the in-circuit `Decompressor`: unpacking
    /// the field element with the same positional weights must return the
    /// original digits, at both depths.
    #[test]
    fn packing_round_trips_at_both_depths() {
        for bd in [8u32, 16] {
            let comp = comp_size_for(bd);
            let half = 1i32 << (bd - 1);
            let signed: Vec<i32> = (0..comp)
                .map(|i| -half + (i as i32 * 7) % (2 * half))
                .collect();
            let norm = normalize_samples_at(&signed, bd);
            let packed = &compress_samples_bigint_at(&norm, bd)[0];

            let mask = (BigUint::from(1u32) << bd as usize) - BigUint::from(1u32);
            for (i, &expected) in norm.iter().enumerate() {
                let digit = (packed >> (bd as usize * i)) & mask.clone();
                assert_eq!(
                    digit,
                    BigUint::from(expected as u64),
                    "bd={} digit {} mismatched",
                    bd, i
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "not a normalized")]
    fn packer_rejects_unnormalized_input() {
        // A negative value would silently corrupt the packed element.
        compress_samples_bigint_at(&[-1], 16);
    }

    #[test]
    #[should_panic(expected = "not a normalized")]
    fn packer_rejects_value_above_depth() {
        compress_samples_bigint_at(&[256], 8);
    }
}
