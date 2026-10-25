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

/// Applies a tremolo (LFO amplitude modulation) to `original`:
///
/// ```text
/// edit_i = floor( orig_i * L(i) / 2^16 )
/// ```
///
/// where `L(i)` is a unipolar sine LFO in `[0, 2^16]` sampled at the
/// sample's absolute position. `period` is the LFO period in samples
/// (e.g. 8820 for 5 Hz at 44.1 kHz); `start_index` is the position of
/// `original[0]` in the whole stream, so a caller processing in chunks
/// keeps the phase continuous.
///
/// **This is the mirror of `circuits/edits/tremolo/tremolo_step.circom`
/// and must match it exactly**, which drives three choices that would
/// otherwise look arbitrary:
///
/// - The LFO is held constant over blocks of `comp_size` samples, because
///   the circuit does one table lookup per packed field element rather
///   than per sample.
/// - Rounding is floor (`>>`), not truncation (`/`).
/// - LFO values come from [`crate::lfo_table::LFO_TABLE`], generated by
///   `docs/gen_lfo_table.py` alongside the circom table.
///
/// No clamping: `L <= 2^16` means the result never exceeds the source
/// magnitude, so it always fits `bit_depth`.
pub fn tremolo(
    original: &[i32],
    period: usize,
    bit_depth: u32,
    start_index: usize,
    comp_size: usize,
) -> Result<Vec<i32>, String> {
    use crate::lfo_table::{LFO_TABLE, LFO_TABLE_N};

    if period == 0 {
        return Err("tremolo period must be > 0 samples".to_string());
    }
    if comp_size == 0 {
        return Err("comp_size must be > 0".to_string());
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
        // Piecewise-constant: the LFO is looked up for the first sample of
        // each comp_size-sized block and held across it, exactly as the
        // circuit does per packed element.
        let abs = start_index + i;
        let block_start = (abs / comp_size) * comp_size;
        let idx = ((block_start * LFO_TABLE_N) / period) % LFO_TABLE_N;
        let l = LFO_TABLE[idx];
        // Floor via arithmetic shift, matching the circuit's bit truncation.
        edited.push(((sample as i64 * l) >> 16) as i32);
    }
    Ok(edited)
}

/// Concatenates two streams: the output is `a` followed by `b`.
///
/// Mirror of `circuits/edits/concat/concat_step.circom`. Sample values
/// are unchanged — the edit is in the arrangement in time, so
/// `N_out = N_a + N_b`. This is the duration counterpart to [`merge`],
/// which combines two streams in value instead.
///
/// `samples_per_step` is the fold width (`batch_size * comp_size`).
/// `a.len()` must be a whole multiple of it, because the circuit's phase
/// selector tests the sample counter once per fold rather than once per
/// sample; a boundary falling mid-fold would need a per-sample gate and
/// cost roughly 3,100 comparisons per step instead of one.
///
/// Unlike merge, concat does **not** commute: swapping the arguments
/// yields a different output and a different commitment, so the order is
/// part of the proven statement.
///
/// # Examples
/// ```
/// use zk_audio::audio_editor::concat;
/// let a = vec![1, 2, 3, 4];
/// let b = vec![5, 6];
/// assert_eq!(concat(&a, &b, 2).unwrap(), vec![1, 2, 3, 4, 5, 6]);
/// // Order matters.
/// assert_ne!(concat(&a, &b, 2).unwrap(), concat(&b, &a, 2).unwrap());
/// ```
pub fn concat(a: &[i32], b: &[i32], samples_per_step: usize) -> Result<Vec<i32>, String> {
    if samples_per_step == 0 {
        return Err("samples_per_step must be > 0".to_string());
    }
    if a.is_empty() || b.is_empty() {
        return Err("concat needs two non-empty streams".to_string());
    }
    if a.len() % samples_per_step != 0 {
        return Err(format!(
            "stream A has {} samples, which is not a whole number of {}-sample folds;              the concat boundary must fall on a fold boundary",
            a.len(),
            samples_per_step
        ));
    }
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend_from_slice(a);
    out.extend_from_slice(b);
    Ok(out)
}

/// Mixes two streams by averaging: `out = floor((a + b) / 2)`.
///
/// Mirror of `circuits/edits/merge/merge_step.circom` with
/// `average = 1`. This is the DEFAULT merge: it is 20% cheaper in
/// constraints than the clamped sum (248,400 vs 310,400 at bd=8/B=100)
/// because halving the sum of two in-range samples is always in range,
/// so no saturation logic is needed at all.
///
/// It is also what mixing desks do — summing two full-scale signals and
/// clamping sounds like hard distortion, whereas averaging is a clean
/// -6 dB mix of each source.
///
/// **Rounding is floor**, matching the circuit's bit truncation, and NOT
/// Rust's `/`, which truncates toward zero. The two disagree for every
/// odd negative sum, which is common in real audio: a witness built with
/// `/` instead of `>>` is rejected by the circuit (verified).
///
/// # Examples
/// ```
/// use zk_audio::audio_editor::merge;
/// assert_eq!(merge(&[10, -10], &[4, -4], 8).unwrap(), vec![7, -7]);
/// // Full scale cannot overflow, so nothing is clamped.
/// assert_eq!(merge(&[127], &[127], 8).unwrap(), vec![127]);
/// assert_eq!(merge(&[-128], &[-128], 8).unwrap(), vec![-128]);
/// // Floor, not truncation: (-7 + 0)/2 floors to -4.
/// assert_eq!(merge(&[-7], &[0], 8).unwrap(), vec![-4]);
/// ```
pub fn merge(a: &[i32], b: &[i32], bit_depth: u32) -> Result<Vec<i32>, String> {
    if a.len() != b.len() {
        return Err(format!(
            "merge needs equal-length streams, got {} and {}",
            a.len(),
            b.len()
        ));
    }
    let (min, max) = sample_limits(bit_depth);
    let mut out = Vec::with_capacity(a.len());
    for (i, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
        for (which, v) in [("a", x), ("b", y)] {
            if v < min || v > max {
                return Err(format!(
                    "stream {} sample {} at index {} does not fit in {} bits (range {}..={})",
                    which, v, i, bit_depth, min, max
                ));
            }
        }
        // Arithmetic shift == floor division by 2, including negatives.
        // No clamp: |(x+y)/2| <= max(|x|,|y|), so the result always fits.
        out.push(((x as i64 + y as i64) >> 1) as i32);
    }
    Ok(out)
}

/// Mixes two streams by summing with saturation: `out = clamp(a + b)`.
///
/// Mirror of the `average = 0` variant. Kept as the literal
/// interpretation of "a + b", and as the cost control that shows what
/// the clamp costs: 310,400 constraints against 248,400 for [`merge`]
/// at bd=8/B=100, a 20% premium for two comparators and two
/// multiplexers per sample.
///
/// Prefer [`merge`] unless the literal sum is specifically wanted:
/// clamping fires on roughly a quarter of samples for loud material and
/// sounds like hard distortion.
///
/// Mirror of `circuits/edits/merge/merge_step.circom`. Unlike every other
/// edit here this is a function of TWO inputs, so the provenance claim it
/// supports is different in kind: an output committed alongside two
/// source commitments attests that the mix contains exactly those two
/// streams.
///
/// **Clamping is the common path, not an edge case.** Two full-scale
/// signals sum well outside the representable range in both directions
/// (at 8-bit, -256..254 against -128..127), so saturation fires often on
/// loud material. Real mixers usually attenuate before summing for this
/// reason; this function implements the literal sum that was specified.
///
/// Both streams must have equal length and fit `bit_depth`.
///
/// # Examples
/// ```
/// use zk_audio::audio_editor::merge;
/// assert_eq!(merge(&[10, -10], &[5, -5], 8).unwrap(), vec![15, -15]);
/// // Saturates in both directions.
/// assert_eq!(merge(&[100], &[100], 8).unwrap(), vec![127]);
/// assert_eq!(merge(&[-100], &[-100], 8).unwrap(), vec![-128]);
/// ```
pub fn merge_clamped(a: &[i32], b: &[i32], bit_depth: u32) -> Result<Vec<i32>, String> {
    if a.len() != b.len() {
        return Err(format!(
            "merge needs equal-length streams, got {} and {}",
            a.len(),
            b.len()
        ));
    }
    let (min, max) = sample_limits(bit_depth);
    let mut out = Vec::with_capacity(a.len());
    for (i, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
        for (which, v) in [("a", x), ("b", y)] {
            if v < min || v > max {
                return Err(format!(
                    "stream {} sample {} at index {} does not fit in {} bits (range {}..={})",
                    which, v, i, bit_depth, min, max
                ));
            }
        }
        out.push(clamp_to_depth(x as i64 + y as i64, bit_depth));
    }
    Ok(out)
}

/// Crops a contiguous window from `original`: returns the samples in
/// `[start, start + len)` unchanged.
///
/// Mirror of `circuits/edits/crop/crop_step.circom`. The values pass
/// through verbatim, so there is no arithmetic to agree on — what the
/// circuit proves instead is *which* window was kept, via the IVC
/// counter, whose initial value is the window start and whose final
/// value is the window end.
///
/// Two constraints the caller must respect, both of which the suite
/// checks:
///
/// - **`start` must be even for interleaved stereo**, or the channels
///   swap for the remainder of the stream. The circuit cannot detect
///   this (it sees a flat sample vector), so it is enforced here.
/// - **`len` must be a whole number of `B * comp_size`** samples, or the
///   final batch is short and the circuit rejects it.
///
/// # Examples
/// ```
/// use zk_audio::audio_editor::crop;
/// let samples: Vec<i32> = (0..10).collect();
/// assert_eq!(crop(&samples, 2, 4, 1).unwrap(), vec![2, 3, 4, 5]);
/// ```
pub fn crop(
    original: &[i32],
    start: usize,
    len: usize,
    channels: usize,
) -> Result<Vec<i32>, String> {
    if channels == 0 {
        return Err("channels must be >= 1".to_string());
    }
    if len == 0 {
        return Err("crop length must be > 0".to_string());
    }
    if start % channels != 0 {
        return Err(format!(
            "crop start {} is not a multiple of the channel count {};              the channels would swap for the rest of the stream",
            start, channels
        ));
    }
    if len % channels != 0 {
        return Err(format!(
            "crop length {} is not a multiple of the channel count {};              the window would end mid-frame",
            len, channels
        ));
    }
    if start + len > original.len() {
        return Err(format!(
            "crop window [{}, {}) exceeds the {} available samples",
            start,
            start + len,
            original.len()
        ));
    }
    Ok(original[start..start + len].to_vec())
}

/// Applies the `dist 1.c` distortion: a full-wave rectifier fold blended
/// with the dry signal.
///
/// ```text
/// output = d * 2*(|x| - 1/2) + (1 - d) * x        (x in [-1, 1])
/// ```
///
/// `blend` is `d` at fixed-point scale 2^16, so 65536 is fully wet and 0
/// is a passthrough. In integer form:
///
/// ```text
/// edit = clamp( ( blend*(2|x| - half) + (2^16 - blend)*x ) >> 16 )
/// ```
///
/// Mirror of `circuits/edits/distortion/dist_step.circom`. Three
/// properties matter for the agreement:
///
/// - Rounding is floor (`>>`), as everywhere else here.
/// - **Clamping is required**: at `blend = 2^16` the rectified term
///   reaches `+half`, one above the representable maximum.
/// - The integer form deviates from the float reference by at most 0.75
///   LSB (worst at d = 0.25), because it floors once at the end rather
///   than rounding at every step.
///
/// # Examples
/// ```
/// use zk_audio::audio_editor::distort;
/// // Fully dry is a passthrough.
/// assert_eq!(distort(&[50, -50], 0, 8).unwrap(), vec![50, -50]);
/// // Fully wet rectifies and folds: |x| doubled, offset by half.
/// assert_eq!(distort(&[0], 65536, 8).unwrap(), vec![-128]);
/// ```
pub fn distort(original: &[i32], blend: i64, bit_depth: u32) -> Result<Vec<i32>, String> {
    let scale = 1i64 << 16;
    if blend < 0 || blend > scale {
        return Err(format!("blend must be in 0..={}, got {}", scale, blend));
    }
    let (min, max) = sample_limits(bit_depth);
    let half = 1i64 << (bit_depth - 1);
    let mut edited = Vec::with_capacity(original.len());

    for (i, &sample) in original.iter().enumerate() {
        if sample < min || sample > max {
            return Err(format!(
                "sample {} at index {} does not fit in {} bits (range {}..={})",
                sample, i, bit_depth, min, max
            ));
        }
        let x = sample as i64;
        let a = x.abs();
        let num = blend * (2 * a - half) + (scale - blend) * x;
        // Floor via shift, then saturate: the upper bound IS reachable.
        edited.push(clamp_to_depth(num >> 16, bit_depth));
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

    /// The in-circuit tremolo, transcribed from `ModulateSample` in
    /// `circuits/edits/tremolo/tremolo_step.circom`, in NORMALIZED space.
    fn circuit_tremolo_normalized(orig_n: i64, l: i64, bit_depth: u32) -> i64 {
        let half = 1i64 << (bit_depth - 1);
        let scale = 1i64 << 16;
        // u = orig_n*L + (scale - L)*half = orig*L + half*scale >= 0
        let u = orig_n * l + (scale - l) * half;
        assert!(u >= 0, "u must be non-negative, got {}", u);
        u >> 16
    }

    #[test]
    fn tremolo_matches_circuit_exhaustively() {
        use crate::lfo_table::LFO_TABLE;
        // Every sample value against a spread of LFO values including both
        // extremes, at both depths.
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            let step = if bd == 8 { 1 } else { 137 };
            let ls = [
                0i64,
                1,
                LFO_TABLE[0],
                LFO_TABLE[64],
                LFO_TABLE[192],
                (1 << 16) - 1,
                1 << 16,
            ];
            let mut orig = -half;
            while orig < half {
                for &l in &ls {
                    let want = (orig * l) >> 16;
                    let got = circuit_tremolo_normalized(orig + half, l, bd) - half;
                    assert_eq!(
                        want, got,
                        "bd={} orig={} L={}: want {} got {}",
                        bd, orig, l, want, got
                    );
                }
                orig += step;
            }
        }
    }

    #[test]
    fn tremolo_output_always_fits_the_depth() {
        // Why no clamp is needed: L <= 2^16 so the result never exceeds
        // the source magnitude. This is what keeps Recompressor sound.
        use crate::lfo_table::LFO_TABLE;
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            let step = if bd == 8 { 1 } else { 211 };
            let mut orig = -half;
            while orig < half {
                for &l in LFO_TABLE.iter() {
                    let out = circuit_tremolo_normalized(orig + half, l, bd);
                    assert!(
                        out >= 0 && out < (1i64 << bd),
                        "bd={} orig={} L={} produced {} outside [0,2^{})",
                        bd, orig, l, out, bd
                    );
                }
                orig += step;
            }
        }
    }

    #[test]
    fn tremolo_rust_reference_matches_circuit_over_a_stream() {
        // End-to-end: the Rust helper's piecewise-constant blocking must
        // line up with the circuit's per-element lookup.
        use crate::lfo_table::{LFO_TABLE, LFO_TABLE_N};
        let bd = 8u32;
        let comp = 31usize;
        let period = 8820usize;
        let samples: Vec<i32> = (0..comp * 20).map(|i| ((i * 7) % 255) as i32 - 128).collect();
        let out = tremolo(&samples, period, bd, 0, comp).unwrap();
        for (i, (&s, &o)) in samples.iter().zip(out.iter()).enumerate() {
            let block_start = (i / comp) * comp;
            let idx = ((block_start * LFO_TABLE_N) / period) % LFO_TABLE_N;
            let l = LFO_TABLE[idx];
            let expected = circuit_tremolo_normalized(s as i64 + 128, l, bd) - 128;
            assert_eq!(o as i64, expected, "mismatch at sample {}", i);
        }
    }

    #[test]
    fn tremolo_phase_is_continuous_across_chunks() {
        // start_index exists so a chunked caller keeps LFO phase; proving
        // in batches must equal proving the whole stream at once.
        let comp = 31usize;
        let all: Vec<i32> = (0..comp * 10).map(|i| (i % 200) as i32 - 100).collect();
        let whole = tremolo(&all, 8820, 8, 0, comp).unwrap();
        let mut chunked = Vec::new();
        for (c, chunk) in all.chunks(comp * 3).enumerate() {
            let start = c * comp * 3;
            chunked.extend(tremolo(chunk, 8820, 8, start, comp).unwrap());
        }
        assert_eq!(whole, chunked);
    }

    #[test]
    fn tremolo_rejects_bad_parameters() {
        assert!(tremolo(&[0], 0, 8, 0, 31).is_err());
        assert!(tremolo(&[0], 8820, 8, 0, 0).is_err());
        assert!(tremolo(&[1000], 8820, 8, 0, 31).is_err());
    }

    #[test]
    fn lfo_table_is_unipolar_and_in_scale() {
        use crate::lfo_table::{LFO_SCALE, LFO_TABLE, LFO_TABLE_N};
        assert_eq!(LFO_TABLE.len(), LFO_TABLE_N);
        assert!(LFO_TABLE.iter().all(|&v| v >= 0 && v <= LFO_SCALE));
        // A negative entry would break the normalized formulation.
        assert_eq!(*LFO_TABLE.iter().min().unwrap(), 0);
        assert_eq!(*LFO_TABLE.iter().max().unwrap(), LFO_SCALE);
    }

    /// The in-circuit distortion, transcribed from `DistortSample` in
    /// `circuits/edits/distortion/dist_step.circom`, in NORMALIZED space.
    fn circuit_distort_normalized(orig_n: i64, blend: i64, bit_depth: u32) -> i64 {
        let half = 1i64 << (bit_depth - 1);
        let scale = 1i64 << 16;
        // AbsValue: orig_n < half means x < 0.
        let a = if orig_n < half { half - orig_n } else { orig_n - half };
        let v = blend * (2 * a - half) + (scale - blend) * (orig_n - half) + scale * half;
        assert!(v >= 0, "v must be non-negative, got {}", v);
        let shifted = v >> 16;
        // Clamp in normalized space.
        let hi = (1i64 << bit_depth) - 1;
        if shifted > hi {
            hi
        } else if shifted < 0 {
            0
        } else {
            shifted
        }
    }

    #[test]
    fn distort_matches_circuit_exhaustively() {
        let scale = 1i64 << 16;
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            let step = if bd == 8 { 1 } else { 131 };
            for &blend in &[0i64, 1, scale / 4, scale / 2, 3 * scale / 4, scale - 1, scale] {
                let mut x = -half;
                while x < half {
                    let rust = distort(&[x as i32], blend, bd).unwrap()[0] as i64 + half;
                    let circuit = circuit_distort_normalized(x + half, blend, bd);
                    assert_eq!(
                        rust, circuit,
                        "bd={} blend={} x={}: rust={} circuit={}",
                        bd, blend, x, rust, circuit
                    );
                    x += step;
                }
            }
        }
    }

    #[test]
    fn distort_clamping_is_actually_required() {
        // The property that distinguishes this edit from attenuation and
        // tremolo: at full wet the output exceeds the representable max,
        // so saturation is not optional. If this ever stops holding, the
        // clamp could be dropped for a cheaper circuit.
        let scale = 1i64 << 16;
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            let mut exceeded = false;
            let mut x = -half;
            while x < half {
                let a = x.abs();
                let num = scale * (2 * a - half);
                if (num >> 16) > half - 1 {
                    exceeded = true;
                }
                x += 1;
            }
            assert!(
                exceeded,
                "bd={}: expected the unclamped output to exceed the max",
                bd
            );
        }
    }

    #[test]
    fn distort_fully_dry_is_a_passthrough() {
        let xs: Vec<i32> = (-128..128).collect();
        assert_eq!(distort(&xs, 0, 8).unwrap(), xs);
    }

    #[test]
    fn distort_rectifies_when_fully_wet() {
        // Fully wet: sign is discarded, so +x and -x map to the same value.
        for x in [1i32, 7, 50, 127] {
            let pos = distort(&[x], 65536, 8).unwrap()[0];
            let neg = distort(&[-x], 65536, 8).unwrap()[0];
            assert_eq!(pos, neg, "rectifier should be sign-blind at x={}", x);
        }
    }

    #[test]
    fn distort_integer_form_tracks_the_float_reference() {
        // Bounds the deviation introduced by flooring once at the end
        // rather than rounding at each float step. Claimed as <= 0.75 LSB
        // in the circuit comment; verified here.
        let scale = 1i64 << 16;
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            let step = if bd == 8 { 1 } else { 97 };
            for &(blend, d) in &[(0i64, 0.0f64), (scale / 4, 0.25), (scale / 2, 0.5), (scale, 1.0)]
            {
                let mut worst = 0.0f64;
                let mut x = -half;
                while x < half {
                    let got = distort(&[x as i32], blend, bd).unwrap()[0] as f64;
                    let xf = x as f64 / half as f64;
                    let want = (d * 2.0 * (xf.abs() - 0.5) + (1.0 - d) * xf) * half as f64;
                    let want = want.max(-(half as f64)).min((half - 1) as f64);
                    worst = worst.max((got - want).abs());
                    x += step;
                }
                assert!(
                    worst <= 0.75,
                    "bd={} d={}: deviation {} exceeds 0.75 LSB",
                    bd, d, worst
                );
            }
        }
    }

    #[test]
    fn distort_rejects_blend_out_of_range() {
        assert!(distort(&[0], -1, 8).is_err());
        assert!(distort(&[0], (1 << 16) + 1, 8).is_err());
        assert!(distort(&[1000], 0, 8).is_err());
    }

    /// The in-circuit merge, transcribed from `ClampSum` in
    /// `circuits/edits/merge/merge_step.circom`, in NORMALIZED space.
    fn circuit_merge_normalized(a_n: i64, b_n: i64, bit_depth: u32) -> i64 {
        let half = 1i64 << (bit_depth - 1);
        let maxn = (1i64 << bit_depth) - 1;
        let s = a_n + b_n;
        // Comparisons happen in s-space because t = s - half may be
        // negative and the field has no sign.
        if s < half {
            0
        } else if s > maxn + half {
            maxn
        } else {
            s - half
        }
    }

    /// The in-circuit averaging merge, from `AverageSum`.
    fn circuit_merge_avg_normalized(a_n: i64, b_n: i64) -> i64 {
        (a_n + b_n) >> 1
    }

    #[test]
    fn concat_appends_in_order() {
        let a: Vec<i32> = (0..20).collect();
        let b: Vec<i32> = (100..130).collect();
        let out = concat(&a, &b, 10).unwrap();
        assert_eq!(out.len(), 50);
        assert_eq!(&out[..20], &a[..]);
        assert_eq!(&out[20..], &b[..]);
    }

    #[test]
    fn concat_does_not_commute() {
        // The property that distinguishes it from merge: addition
        // commutes, playback order does not.
        let a: Vec<i32> = (0..20).collect();
        let b: Vec<i32> = (100..120).collect();
        assert_ne!(
            concat(&a, &b, 10).unwrap(),
            concat(&b, &a, 10).unwrap()
        );
    }

    #[test]
    fn concat_requires_a_fold_aligned_boundary() {
        // A mid-fold boundary would force per-sample gating in the
        // circuit, ~3100x more expensive than the per-step selector.
        let a: Vec<i32> = (0..25).collect();
        let b: Vec<i32> = (0..10).collect();
        assert!(concat(&a, &b, 10).is_err());
        let a2: Vec<i32> = (0..30).collect();
        assert!(concat(&a2, &b, 10).is_ok());
    }

    #[test]
    fn concat_leaves_b_unaligned_on_purpose() {
        // Only A's length must be fold-aligned: B is the tail, so its
        // length is free (the suite's N divisibility still applies to
        // the total, but that is a separate constraint).
        let a: Vec<i32> = (0..20).collect();
        let b: Vec<i32> = (0..7).collect();
        assert!(concat(&a, &b, 10).is_ok());
    }

    #[test]
    fn concat_rejects_degenerate_input() {
        let a: Vec<i32> = (0..10).collect();
        assert!(concat(&a, &[], 10).is_err());
        assert!(concat(&[], &a, 10).is_err());
        assert!(concat(&a, &a, 0).is_err());
    }

    #[test]
    fn merge_avg_matches_circuit_exhaustively() {
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            let step = if bd == 8 { 1 } else { 1021 };
            let mut x = -half;
            while x < half {
                let mut y = -half;
                while y < half {
                    let rust = merge(&[x as i32], &[y as i32], bd).unwrap()[0] as i64 + half;
                    let circuit = circuit_merge_avg_normalized(x + half, y + half);
                    assert_eq!(
                        rust, circuit,
                        "bd={} a={} b={}: rust={} circuit={}",
                        bd, x, y, rust, circuit
                    );
                    y += step;
                }
                x += step;
            }
        }
    }

    #[test]
    fn merge_avg_never_needs_a_clamp() {
        // The property that makes averaging 20% cheaper: the result is
        // always representable, so no saturation logic is required.
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            let step = if bd == 8 { 1 } else { 1021 };
            let mut x = -half;
            while x < half {
                let mut y = -half;
                while y < half {
                    let o = circuit_merge_avg_normalized(x + half, y + half);
                    assert!(
                        o >= 0 && o < (1i64 << bd),
                        "bd={} a={} b={} gave {} outside [0,2^{})",
                        bd, x, y, o, bd
                    );
                    y += step;
                }
                x += step;
            }
        }
    }

    #[test]
    fn merge_avg_is_floor_not_truncation() {
        // These disagree for every odd negative sum, which is common in
        // real audio; a witness built with `/` is rejected by the circuit.
        assert_eq!(merge(&[-7], &[0], 8).unwrap(), vec![-4]); // floor
        assert_ne!((-7i32 + 0) / 2, -4); // Rust's `/` gives -3
        assert_eq!(merge(&[-1], &[0], 8).unwrap(), vec![-1]);
        assert_eq!(merge(&[1], &[0], 8).unwrap(), vec![0]);
    }

    #[test]
    fn merge_clamped_matches_circuit_exhaustively() {
        for bd in [8u32, 16] {
            let half = 1i64 << (bd - 1);
            let step = if bd == 8 { 1 } else { 1021 };
            let mut x = -half;
            while x < half {
                let mut y = -half;
                while y < half {
                    let rust =
                        merge_clamped(&[x as i32], &[y as i32], bd).unwrap()[0] as i64 + half;
                    let circuit = circuit_merge_normalized(x + half, y + half, bd);
                    assert_eq!(
                        rust, circuit,
                        "bd={} a={} b={}: rust={} circuit={}",
                        bd, x, y, rust, circuit
                    );
                    y += step;
                }
                x += step;
            }
        }
    }

    #[test]
    fn merge_clamped_clamps_in_both_directions() {
        // Both bounds are load-bearing for the summing variant, unlike
        // distortion where only the upper one was reachable.
        assert_eq!(merge_clamped(&[127], &[127], 8).unwrap(), vec![127]);
        assert_eq!(merge_clamped(&[-128], &[-128], 8).unwrap(), vec![-128]);
        assert_eq!(merge_clamped(&[32767], &[32767], 16).unwrap(), vec![32767]);
        assert_eq!(merge_clamped(&[-32768], &[-32768], 16).unwrap(), vec![-32768]);
    }

    #[test]
    fn merge_avg_is_cheaper_because_it_cannot_overflow() {
        // Averaging full-scale inputs stays in range; summing them does
        // not. This is the whole reason for the 20% constraint saving.
        assert_eq!(merge(&[127], &[127], 8).unwrap(), vec![127]);
        assert_eq!(merge_clamped(&[127], &[127], 8).unwrap(), vec![127]);
        // The sum WANTED 254 and had to be clamped; the average did not.
        assert_eq!(127i32 + 127, 254);
        assert_eq!((127i32 + 127) >> 1, 127);
    }

    #[test]
    fn merge_is_commutative() {
        // Addition commutes, so the RELATION does; the hash slots do not,
        // which is why the circuit keeps A and B in fixed state positions.
        let a: Vec<i32> = (-128..128).collect();
        let b: Vec<i32> = (-128..128).rev().collect();
        assert_eq!(merge(&a, &b, 8).unwrap(), merge(&b, &a, 8).unwrap());
        assert_eq!(
            merge_clamped(&a, &b, 8).unwrap(),
            merge_clamped(&b, &a, 8).unwrap()
        );
    }

    #[test]
    fn merge_with_silence_halves_the_signal() {
        // Averaging with silence is a -6dB fade, not a passthrough: the
        // semantic difference from the summing variant.
        let a: Vec<i32> = (-128..128).collect();
        let silence = vec![0i32; a.len()];
        let avg = merge(&a, &silence, 8).unwrap();
        assert_eq!(avg, a.iter().map(|&x| x >> 1).collect::<Vec<i32>>());
        // The summing variant IS a passthrough.
        assert_eq!(merge_clamped(&a, &silence, 8).unwrap(), a);
    }

    #[test]
    fn merge_rejects_mismatched_lengths_and_ranges() {
        assert!(merge(&[1, 2], &[1], 8).is_err());
        assert!(merge(&[1000], &[0], 8).is_err());
        assert!(merge(&[0], &[1000], 8).is_err());
        assert!(merge_clamped(&[1, 2], &[1], 8).is_err());
    }

    #[test]
    fn crop_returns_the_window_verbatim() {
        let samples: Vec<i32> = (0..100).collect();
        let out = crop(&samples, 10, 20, 1).unwrap();
        assert_eq!(out.len(), 20);
        assert_eq!(out, (10..30).collect::<Vec<i32>>());
        // Values are untouched - that is the whole relation.
        assert!(out.iter().zip(10..30).all(|(&a, b)| a == b));
    }

    #[test]
    fn crop_rejects_odd_boundaries_on_stereo() {
        // An odd start on interleaved stereo swaps L/R for the rest of
        // the stream. The circuit cannot see this, so Rust must refuse.
        let samples: Vec<i32> = (0..100).collect();
        assert!(crop(&samples, 3, 20, 2).is_err());
        assert!(crop(&samples, 4, 21, 2).is_err());
        assert!(crop(&samples, 4, 20, 2).is_ok());
    }

    #[test]
    fn crop_rejects_windows_past_the_end() {
        let samples: Vec<i32> = (0..10).collect();
        assert!(crop(&samples, 8, 4, 1).is_err());
        assert!(crop(&samples, 0, 11, 1).is_err());
        assert!(crop(&samples, 6, 4, 1).is_ok());
    }

    #[test]
    fn crop_rejects_degenerate_parameters() {
        let samples: Vec<i32> = (0..10).collect();
        assert!(crop(&samples, 0, 0, 1).is_err());
        assert!(crop(&samples, 0, 4, 0).is_err());
    }

    #[test]
    fn crop_at_offset_differs_from_crop_at_zero() {
        // The position is the content of the statement: two windows of
        // equal length over different offsets are different outputs, and
        // the circuit distinguishes them through the counter.
        let samples: Vec<i32> = (0..100).map(|i| i * 3).collect();
        let a = crop(&samples, 0, 10, 1).unwrap();
        let b = crop(&samples, 10, 10, 1).unwrap();
        assert_ne!(a, b);
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
