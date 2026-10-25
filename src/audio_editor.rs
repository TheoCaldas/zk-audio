//! Utilities for WAVE audio editing.

/// Sample-value limits for a given bit depth, as signed values.
/// At bd=16 this is [-32768, 32767]; at bd=8, [-128, 127].
///
/// Returns `(min, max)`. Panics on a depth that cannot be represented in
/// i32, which is every depth this project supports.
pub fn sample_limits(bit_depth: u32) -> (i32, i32) {
    assert!(
        (2..=24).contains(&bit_depth),
        "bit_depth {} out of supported range 2..=24",
        bit_depth
    );
    let half = 1i32 << (bit_depth - 1);
    (-half, half - 1)
}

/// Clamps a value to the signed range of `bit_depth`, saturating at the
/// limits of the representation rather than wrapping.
///
/// This is the off-circuit mirror of `ClampedGain` in
/// `circuits/edits/gain/gain_step.circom`; the two must agree for every input, or
/// the suite proves a relation other than the one it reports.
pub fn clamp_to_depth(value: i64, bit_depth: u32) -> i32 {
    let (min, max) = sample_limits(bit_depth);
    if value > max as i64 {
        max
    } else if value < min as i64 {
        min
    } else {
        value as i32
    }
}

/// Applies a gain of integer factor `k` to `original`, saturating at the
/// limits of `bit_depth` instead of overflowing.
///
/// `k` must be > 1 (a factor of 1 is a no-op and 0 silences the signal;
/// neither is a gain edit, and the circuit family is built for k > 1).
/// Samples are signed values that must already fit in `bit_depth`.
///
/// # Examples
/// ```
/// use zk_audio::audio_editor::gain;
/// // 16-bit: 20000*2 saturates at i16::MAX rather than wrapping.
/// let edited = gain(&[1000, -1000, 20000, -20000], 2, 16).unwrap();
/// assert_eq!(edited, vec![2000, -2000, 32767, -32768]);
///
/// // 8-bit: the limits are much closer, so clamping bites sooner.
/// let edited = gain(&[10, -10, 100, -100], 3, 8).unwrap();
/// assert_eq!(edited, vec![30, -30, 127, -128]);
/// ```
pub fn gain(original: &[i32], k: u32, bit_depth: u32) -> Result<Vec<i32>, String> {
    if k <= 1 {
        return Err(format!("gain factor must be > 1, got {}", k));
    }
    let (min, max) = sample_limits(bit_depth);
    let mut edited = Vec::with_capacity(original.len());
    for (i, &sample) in original.iter().enumerate() {
        if sample < min || sample > max {
            return Err(format!(
                "sample {} at index {} does not fit in {} bits (range {}..={})",
                sample, i, bit_depth, min, max
            ));
        }
        edited.push(clamp_to_depth(sample as i64 * k as i64, bit_depth));
    }
    Ok(edited)
}

/// Applies a gain multiplier to the original audio samples.
///
/// Kept for the existing 16-bit call sites and `tests/`; new code should
/// use [`gain`], which is generic over the factor and the bit depth.
/// This is `gain(..., k, 16)` with i16 in and out.
pub fn gain_multiplier(original: &[i16], amount: i8) -> Result<Vec<i16>, &str> {
    if amount.is_negative() {
        return Err("Amount value should be positive!");
    }
    let mut edited = Vec::<i16>::new();
    for sample in original {
        let sample = *sample as i32; 
        let amount = amount as i32;
        let c = clamp_i16(sample * amount);
        edited.push(c);
    }
    Ok(edited)
}

/// Attenuates `original` by a factor of `2^m` (m >= 1), i.e. gain
/// k = 1/2, 1/4, 1/8, ... — about -6dB per step of m.
///
/// Rounding is **floor** (toward negative infinity), via an arithmetic
/// right shift. This deliberately does NOT match Rust's `/`, which
/// truncates toward zero: `-7 / 4 == -1` but `-7 >> 2 == -2`. Floor is
/// what the circuit computes, because it falls out of the bit
/// decomposition for free, and the two sides must agree exactly.
///
/// No clamping is needed or applied: attenuation shrinks magnitude, so
/// the result always fits the depth it came from.
///
/// # Examples
/// ```
/// use zk_audio::audio_editor::attenuate;
/// // Halving, 8-bit.
/// assert_eq!(attenuate(&[100, -100, 1, 0], 1, 8).unwrap(), vec![50, -50, 0, 0]);
/// // Floor, not truncation: -7/4 floors to -2.
/// assert_eq!(attenuate(&[-7], 2, 8).unwrap(), vec![-2]);
/// // The extremes stay in range.
/// assert_eq!(attenuate(&[-128, 127], 1, 8).unwrap(), vec![-64, 63]);
/// ```
pub fn attenuate(original: &[i32], m: u32, bit_depth: u32) -> Result<Vec<i32>, String> {
    if m == 0 {
        return Err("attenuation shift m must be >= 1 (m=0 is a no-op)".to_string());
    }
    if m >= bit_depth {
        return Err(format!(
            "attenuation shift m={} would zero every {}-bit sample",
            m, bit_depth
        ));
    }
    let (min, max) = sample_limits(bit_depth);
    let mut edited = Vec::with_capacity(original.len());
    for (i, &sample) in original.iter().enumerate() {
        if sample < min || sample > max {
            return Err(format!(
                "sample {} at index {} does not fit in {} bits (range {}..={})",
                sample, i, bit_depth, min, max
            ));
        }
        // Arithmetic shift == floor division by 2^m, including negatives.
        edited.push(sample >> m);
    }
    Ok(edited)
}

/// Applies a distortion effect to the original audio samples.
pub fn distortion(original: &[i16]) -> Vec<i16> {
    let mut edited = Vec::with_capacity(original.len());
    for &sample in original {
        let sample = sample as i32;
        let mid_value = (i16::MAX / 2) as i32;
        let y = 2 * (sample.abs() - mid_value);
        let c = clamp_i16(y);
        edited.push(c);
    }
    edited
}

/// Applies a distortion effect to the original audio samples.
pub fn pitch_shifter(original: &[i16], knob: f32, sample_rate: f32) -> Vec<i16> {
    let mut edited = Vec::with_capacity(original.len());
    for (i, &sample) in original.iter().enumerate() {
        edited.push(lfo_float(sample, knob, (i as f32) / sample_rate));
    }
    edited
}

/// Applies a low-frequency oscillator (LFO) effect to the original audio samples.
fn lfo_float(sample: i16, knob: f32, time: f32) -> i16 {
    let freq = knob * 1000.0;
    let lfo = (std::f32::consts::PI * freq * time).sin();
    let output = sample as f32 * lfo;
    clamp_i16(output as i32)
}

/// Clamps an i32/i64 value to the range of i16 to prevent overflow.
fn clamp_i16<T>(value: T) -> i16
where
    T: Into<i64> + Copy,
{
    let value = value.into();
    if value > i16::MAX as i64 {
        i16::MAX
    } else if value < i16::MIN as i64 {
        i16::MIN
    } else {
        value as i16
    }
}
/* --- UNIT TESTS --- */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_match_bit_depth() {
        assert_eq!(sample_limits(16), (-32768, 32767));
        assert_eq!(sample_limits(8), (-128, 127));
        assert_eq!(sample_limits(12), (-2048, 2047));
    }

    #[test]
    fn gain_saturates_instead_of_overflowing() {
        // 16-bit: the whole point of requirement #2.
        assert_eq!(gain(&[20000], 2, 16).unwrap(), vec![32767]);
        assert_eq!(gain(&[-20000], 2, 16).unwrap(), vec![-32768]);
        // 8-bit: clamping bites far sooner.
        assert_eq!(gain(&[100], 3, 8).unwrap(), vec![127]);
        assert_eq!(gain(&[-100], 3, 8).unwrap(), vec![-128]);
    }

    #[test]
    fn gain_passes_through_when_in_range() {
        assert_eq!(gain(&[1000, -1000], 2, 16).unwrap(), vec![2000, -2000]);
        assert_eq!(gain(&[10, -10], 4, 8).unwrap(), vec![40, -40]);
    }

    #[test]
    fn gain_rejects_factors_of_one_or_zero() {
        assert!(gain(&[1], 1, 16).is_err());
        assert!(gain(&[1], 0, 16).is_err());
    }

    #[test]
    fn gain_rejects_samples_outside_the_depth() {
        // A 16-bit sample handed to an 8-bit gain is a caller bug, not
        // something to silently clamp: the circuit would range-check it
        // and fail, so fail loudly here instead.
        assert!(gain(&[1000], 2, 8).is_err());
        assert!(gain(&[-1000], 2, 8).is_err());
    }

    /// The in-circuit computation, transcribed from `ClampedGain` in
    /// `circuits/edits/gain/gain_step.circom`, operating in NORMALIZED space.
    ///
    /// This is the contract test for the whole scheme: if the Rust
    /// reference and the circuit disagree on any input, the benchmark
    /// suite proves a different relation than it reports.
    fn circuit_clamped_gain_normalized(orig_n: i64, k: u32, bit_depth: u32) -> i64 {
        let half = 1i64 << (bit_depth - 1);
        let k = k as i64;
        let raw = orig_n * k;
        let lo = k * half - half;
        let hi = k * half + half - 1;
        let picked = if raw < lo {
            lo
        } else if raw > hi {
            hi
        } else {
            raw
        };
        picked - (k - 1) * half
    }

    #[test]
    fn rust_and_circuit_agree_exhaustively_8bit() {
        let half = 1i64 << 7;
        for k in [2u32, 3, 4, 5, 8, 17] {
            for orig in -half..half {
                let rust = gain(&[orig as i32], k, 8).unwrap()[0] as i64 + half;
                let circuit = circuit_clamped_gain_normalized(orig + half, k, 8);
                assert_eq!(
                    rust, circuit,
                    "disagreement at bd=8 k={} orig={}: rust={} circuit={}",
                    k, orig, rust, circuit
                );
            }
        }
    }

    #[test]
    fn rust_and_circuit_agree_exhaustively_16bit() {
        let half = 1i64 << 15;
        for k in [2u32, 3, 7] {
            for orig in -half..half {
                let rust = gain(&[orig as i32], k, 16).unwrap()[0] as i64 + half;
                let circuit = circuit_clamped_gain_normalized(orig + half, k, 16);
                assert_eq!(
                    rust, circuit,
                    "disagreement at bd=16 k={} orig={}: rust={} circuit={}",
                    k, orig, rust, circuit
                );
            }
        }
    }

    /// The in-circuit attenuation, transcribed from `AttenuatedGain` in
    /// `circuits/edits/attenuation/atten_step.circom`, operating in NORMALIZED space.
    fn circuit_atten_normalized(orig_n: i64, m: u32, bit_depth: u32) -> i64 {
        let half = 1i64 << (bit_depth - 1);
        // v = orig_n + half*(2^m - 1), always non-negative.
        let v = orig_n + half * ((1i64 << m) - 1);
        // edit_n = v >> m; the offset cancels exactly.
        v >> m
    }

    #[test]
    fn atten_floor_matches_circuit_exhaustively() {
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            for m in 1..4u32 {
                for orig in -half..half {
                    let rust = attenuate(&[orig as i32], m, bd).unwrap()[0] as i64 + half;
                    let circuit = circuit_atten_normalized(orig + half, m, bd);
                    assert_eq!(
                        rust, circuit,
                        "disagreement at bd={} m={} orig={}: rust={} circuit={}",
                        bd, m, orig, rust, circuit
                    );
                }
            }
        }
    }

    #[test]
    fn atten_is_floor_not_truncation() {
        // The distinction that would silently break Rust/circom agreement.
        assert_eq!(attenuate(&[-7], 2, 8).unwrap(), vec![-2]); // floor
        assert_ne!(-7i32 / 4, -2); // Rust's `/` gives -1
        assert_eq!(attenuate(&[-1], 1, 8).unwrap(), vec![-1]); // floor(-0.5) = -1
        assert_eq!(attenuate(&[1], 1, 8).unwrap(), vec![0]);
    }

    #[test]
    fn atten_output_always_fits_and_never_clamps() {
        // Attenuation shrinks magnitude, so Recompressor stays sound with
        // no clamp - the property the circuit relies on.
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            let step = if bd == 8 { 1 } else { 97 };
            for m in 1..4u32 {
                let mut orig = -half;
                while orig < half {
                    let out = circuit_atten_normalized(orig + half, m, bd);
                    assert!(
                        out >= 0 && out < (1i64 << bd),
                        "bd={} m={} orig={} produced {} outside [0, 2^{})",
                        bd, m, orig, out, bd
                    );
                    orig += step;
                }
            }
        }
    }

    #[test]
    fn atten_rejects_degenerate_shifts() {
        assert!(attenuate(&[1], 0, 8).is_err());
        assert!(attenuate(&[1], 8, 8).is_err());
        assert!(attenuate(&[1000], 1, 8).is_err()); // out of depth
    }

    /// Shifting the normalized value directly is the obvious-looking
    /// mistake, and it is wrong for EVERY input - not just negatives.
    /// This pins why the offset formulation exists.
    #[test]
    fn naive_shift_of_normalized_value_is_wrong() {
        let bd = 8u32;
        let half = 1i64 << (bd - 1);
        let mut mismatches = 0;
        for orig in -half..half {
            let naive = ((orig + half) >> 1) - half;
            if naive != (orig >> 1) {
                mismatches += 1;
            }
        }
        assert_eq!(mismatches, 2 * half, "expected every input to mismatch");
    }

    #[test]
    fn circuit_output_always_fits_the_depth() {
        // Recompressor soundness depends on this: every clamped digit
        // must land inside [0, 2^bd - 1] in normalized space, for every
        // input, with no headroom assumption.
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            let step = if bd == 8 { 1 } else { 37 };
            for k in [2u32, 3, 4, 255] {
                let mut orig = -half;
                while orig < half {
                    let out = circuit_clamped_gain_normalized(orig + half, k, bd);
                    assert!(
                        out >= 0 && out < (1i64 << bd),
                        "bd={} k={} orig={} produced {} outside [0, 2^{})",
                        bd, k, orig, out, bd
                    );
                    orig += step;
                }
            }
        }
    }
}
