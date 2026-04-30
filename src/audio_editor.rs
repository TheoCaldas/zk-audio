//! Utilities for WAVE audio editing.

/// Applies a gain multiplier to the original audio samples.
/// The `amount` parameter should be a positive integer.
/// Returns an error if the amount is negative.
/// # Examples
/// ```
/// use zk_audio::audio_editor::gain_multiplier;
/// let original = vec![1000, -1000, 500, -500];
/// let amount = 2;
/// let edited = gain_multiplier(&original, amount).unwrap();
/// assert_eq!(edited, vec![2000, -2000, 1000, -1000]);
/// ```
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