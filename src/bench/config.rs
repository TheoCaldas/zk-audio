//! Suite configuration, the N ladder, the edit registry, and case generation.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Samples packed into one field element, derived from the bit depth exactly as
/// `Decompressor`/`Recompressor` do it in circom. At b=16 this is 15.
pub fn comp_size_for(bit_depth: usize) -> usize {
    (256 - bit_depth) / bit_depth
}

/// The in-circuit normalization offset, `2^(bit_depth-1)`.
pub fn offset_for(bit_depth: usize) -> i64 {
    1i64 << (bit_depth - 1)
}

/// Bit depths with a compiled step circuit. Only 16 for now; the rest of the
/// pipeline is parameterized so adding one is a config change plus a circuit.
pub const SUPPORTED_BIT_DEPTHS: &[usize] = &[8, 16];

/// N ladder: 60, 600, 6000, ... capped at `max_n`.
pub fn n_ladder(max_n: usize) -> Vec<usize> {
    let mut v = Vec::new();
    let mut n = 60usize;
    while n <= max_n {
        v.push(n);
        match n.checked_mul(10) {
            Some(next) => n = next,
            None => break,
        }
    }
    v
}

/// Total samples in `seconds` of audio at `sample_rate` Hz across `channels`
/// interleaved channels. This is the whole stream, both channels included,
/// because that is what the circuit folds over: `read_samples` returns the
/// interleaved frames, not per-channel ones.
pub fn samples_for_duration(seconds: f64, sample_rate: usize, channels: usize) -> usize {
    (seconds * sample_rate as f64 * channels as f64).round() as usize
}

/// The N values the suite will sweep. `--durations` takes precedence over the
/// ladder: asking for wall-clock audio lengths means the 60*10^k steps are not
/// what you want. Values are sorted ascending so cheap cases still run first.
pub fn n_targets(cfg: &SuiteConfig) -> Vec<usize> {
    // An explicit list wins over everything: at bit depths whose
    // comp_size does not divide the 60*10^k ladder (bd=8 packs 31
    // samples per element, so a B=10 step covers 310), the ladder
    // produces no runnable cases at all.
    if !cfg.n_list.is_empty() {
        let mut v = cfg.n_list.clone();
        v.sort_unstable();
        v.dedup();
        return v;
    }
    if cfg.durations.is_empty() {
        return n_ladder(cfg.max_n);
    }
    let mut v: Vec<usize> = cfg
        .durations
        .iter()
        .map(|&d| samples_for_duration(d, cfg.sample_rate, cfg.channels))
        .filter(|&n| n > 0)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Pairs a Rust-side edit with the circuit family that enforces it. Both sides
/// must implement the same relation; this struct is the only place that
/// correspondence is recorded.
///
/// `clamps` records whether the circuit family enforces saturation. The
/// `gain2` family does NOT (it is pure arithmetic and relies on the audio
/// having headroom), while the generalized `gain` family does. The suite
/// uses this to decide whether full-scale audio is safe to prove.
/// Everything an edit needs beyond the samples themselves. Memoryless
/// edits (gain, attenuation) ignore all of it; a time-varying edit like
/// tremolo needs the stream position and the packing width, because its
/// LFO is a function of absolute sample index and is held constant per
/// packed element.
#[derive(Clone, Copy, Debug)]
pub struct EditCtx {
    pub bit_depth: usize,
    /// Packed elements per fold. Needed by edits whose boundary must be
    /// fold-aligned (concat).
    pub batch_size: usize,
    /// Index of this slice's first sample within the whole stream. The
    /// suite always proves a prefix, so this is 0 today, but the edit
    /// functions take it so chunked callers stay correct.
    pub start_index: usize,
    /// Samples per packed field element.
    pub comp_size: usize,
    /// LFO period in samples, for edits that have one.
    pub period: usize,
}

pub struct EditSpec {
    pub name: &'static str,
    pub circuit_prefix: &'static str,
    pub apply: fn(&[i16], &EditCtx) -> Vec<i16>,
    /// For multi-input edits, the two-stream relation. When present the
    /// suite reads a second WAV and `apply` is ignored: a function of two
    /// streams cannot be expressed by the single-stream signature, and
    /// faking it (e.g. by closing over stream B) would hide the second
    /// input from the manifest.
    pub apply2: Option<fn(&[i16], &[i16], &EditCtx) -> Vec<i16>>,
    /// Gain factor k, for the generalized family. `None` for edits whose
    /// circuit does not take a factor.
    pub factor: Option<u32>,
    pub clamps: bool,
    /// Bit depths this edit has compiled circuits for.
    pub bit_depths: &'static [usize],
    /// True when the third IVC state element is a second stream's hash
    /// rather than a sample counter.
    pub multi_input: bool,
    /// Width of the Nova public IVC state. Memoryless edits carry the two
    /// running hashes; a time-varying edit adds a proven sample counter,
    /// so `start_public_input` must be sized to match the circuit or
    /// Nova rejects the witness.
    pub state_width: usize,
}

/// `gain2`: the relation `multiplier_step_b16.circom` proves is
/// `edit = 2*orig - 2^(bit_depth-1)` over NORMALIZED samples. On raw signed
/// samples that is just `x * 2`.
///
/// Note this is deliberately not `audio_editor::gain_multiplier`, which clamps
/// to i16 range on signed samples — the circuit has no clamping, so the two are
/// different functions. Wiring the clamping version here would prove the wrong
/// relation.
fn gain2(original: &[i16], _ctx: &EditCtx) -> Vec<i16> {
    original.iter().map(|x| x * 2).collect()
}

/// The generalized, clamping gain. Unlike `gain2` this IS
/// `audio_editor::gain` — the circuit saturates too, so the clamping
/// reference is now the correct mirror rather than a mismatch.
fn clamped_gain(original: &[i16], bit_depth: usize, k: u32) -> Vec<i16> {
    let wide: Vec<i32> = original.iter().map(|&s| s as i32).collect();
    crate::audio_editor::gain(&wide, k, bit_depth as u32)
        .expect("gain factor and sample range validated by the suite")
        .into_iter()
        .map(|s| s as i16)
        .collect()
}

fn gain_k2(original: &[i16], ctx: &EditCtx) -> Vec<i16> {
    clamped_gain(original, ctx.bit_depth, 2)
}
fn gain_k3(original: &[i16], ctx: &EditCtx) -> Vec<i16> {
    clamped_gain(original, ctx.bit_depth, 3)
}
fn gain_k4(original: &[i16], ctx: &EditCtx) -> Vec<i16> {
    clamped_gain(original, ctx.bit_depth, 4)
}

/// Power-of-two attenuation: k = 1/2^m. Floor rounding, no clamping
/// needed (attenuation shrinks magnitude).
fn attenuated(original: &[i16], bit_depth: usize, m: u32) -> Vec<i16> {
    let wide: Vec<i32> = original.iter().map(|&s| s as i32).collect();
    crate::audio_editor::attenuate(&wide, m, bit_depth as u32)
        .expect("shift and sample range validated by the suite")
        .into_iter()
        .map(|s| s as i16)
        .collect()
}

fn atten_m1(original: &[i16], ctx: &EditCtx) -> Vec<i16> {
    attenuated(original, ctx.bit_depth, 1)
}
fn atten_m2(original: &[i16], ctx: &EditCtx) -> Vec<i16> {
    attenuated(original, ctx.bit_depth, 2)
}

/// Concat: out = A then B. Two-input like merge, but combining duration
/// rather than value, so the suite proves N_a + N_b samples total.
///
/// The suite splits its single N in half: the first half comes from
/// stream A, the second from stream B, which keeps the boundary
/// fold-aligned whenever N is (and N always is, by the divisibility
/// the case matrix already enforces).
fn concat_streams(a: &[i16], b: &[i16], ctx: &EditCtx) -> Vec<i16> {
    // The caller hands equal-length slices; the split is at the
    // fold-aligned midpoint, so the output is A's first half followed by
    // B's second half.
    //
    // B is taken from the SAME absolute offset, not restarted at 0. The
    // circuit compares out[j] against a[j] or b[j] at the same batch
    // index, so after the boundary batch j must hold b[j*spb..], not
    // b[(j-q)*spb..]. Restarting B from zero verifies at one fold (there
    // is no fold past the boundary) and fails at every larger N - which
    // is exactly the failure this once produced.
    let spb = ctx.comp_size * ctx.batch_size;
    let aligned = ((a.len() / 2) / spb) * spb;
    let mut out = Vec::with_capacity(a.len());
    out.extend_from_slice(&a[..aligned]);
    out.extend_from_slice(&b[aligned..]);
    out
}

/// Merge: out = floor((a + b)/2). The only two-input relation, so the
/// only edit using `apply2`.
///
/// Averaging rather than clamped summing: it cannot overflow, so the
/// circuit needs no saturation logic and costs 20% fewer constraints
/// (248,400 vs 310,400 at bd=8/B=100).
fn merge_streams(a: &[i16], b: &[i16], ctx: &EditCtx) -> Vec<i16> {
    let wa: Vec<i32> = a.iter().map(|&s| s as i32).collect();
    let wb: Vec<i32> = b.iter().map(|&s| s as i32).collect();
    crate::audio_editor::merge(&wa, &wb, ctx.bit_depth as u32)
        .expect("stream lengths and ranges validated by the suite")
        .into_iter()
        .map(|s| s as i16)
        .collect()
}

/// Crop: a verbatim contiguous window. The suite proves a prefix, so the
/// window starts at `ctx.start_index` (0) and runs for the whole slice -
/// the interesting variable is the COUNTER the circuit proves, not the
/// values, which pass through unchanged.
fn crop_window(original: &[i16], _ctx: &EditCtx) -> Vec<i16> {
    original.to_vec()
}

/// Distortion per `dist 1.c`: rectifier fold blended with the dry
/// signal. `blend` is the wet amount at scale 2^16.
fn distorted(original: &[i16], bit_depth: usize, blend: i64) -> Vec<i16> {
    let wide: Vec<i32> = original.iter().map(|&s| s as i32).collect();
    crate::audio_editor::distort(&wide, blend, bit_depth as u32)
        .expect("blend and sample range validated by the suite")
        .into_iter()
        .map(|s| s as i16)
        .collect()
}

fn dist_d100(original: &[i16], ctx: &EditCtx) -> Vec<i16> {
    distorted(original, ctx.bit_depth, 1 << 16)
}
fn dist_d50(original: &[i16], ctx: &EditCtx) -> Vec<i16> {
    distorted(original, ctx.bit_depth, 1 << 15)
}

/// Tremolo: the first time-varying edit, so the first that uses more of
/// `EditCtx` than the bit depth.
fn tremolo_f5(original: &[i16], ctx: &EditCtx) -> Vec<i16> {
    let wide: Vec<i32> = original.iter().map(|&s| s as i32).collect();
    crate::audio_editor::tremolo(
        &wide,
        ctx.period,
        ctx.bit_depth as u32,
        ctx.start_index,
        ctx.comp_size,
    )
    .expect("tremolo parameters and sample range validated by the suite")
    .into_iter()
    .map(|s| s as i16)
    .collect()
}

pub fn registry() -> Vec<EditSpec> {
    vec![
        // The original relation, kept unchanged as the benchmark baseline.
        // No clamping: `x * 2` must not overflow, so the audio needs
        // headroom. See docs/duration_scaling_conclusions.md.
        EditSpec {
            name: "gain2",
            circuit_prefix: "multiplier",
            apply: gain2,
            apply2: None,
            factor: Some(2),
            clamps: false,
            bit_depths: &[16],
            multi_input: false,
            state_width: 2,
        },
        // Generalized family: any integer k > 1, saturating at the limits
        // of the bit depth, at 8 or 16 bits.
        EditSpec {
            name: "gain_k2",
            circuit_prefix: "gain",
            apply: gain_k2,
            apply2: None,
            factor: Some(2),
            clamps: true,
            bit_depths: &[8, 16],
            multi_input: false,
            state_width: 2,
        },
        EditSpec {
            name: "gain_k3",
            circuit_prefix: "gain",
            apply: gain_k3,
            apply2: None,
            factor: Some(3),
            clamps: true,
            bit_depths: &[16],
            multi_input: false,
            state_width: 2,
        },
        EditSpec {
            name: "gain_k4",
            circuit_prefix: "gain",
            apply: gain_k4,
            apply2: None,
            factor: Some(4),
            clamps: true,
            bit_depths: &[8, 16],
            multi_input: false,
            state_width: 2,
        },
        // Volume REDUCTION, k = 1/2^m. A separate circuit family: the
        // relation is a shift (floor division), not a multiply, and needs
        // no clamp because attenuation cannot leave the range.
        EditSpec {
            name: "atten_m1",
            circuit_prefix: "atten",
            apply: atten_m1,
            apply2: None,
            factor: Some(1), // m, not k — see circuit_name_for
            clamps: false,
            bit_depths: &[8, 16],
            multi_input: false,
            state_width: 2,
        },
        EditSpec {
            name: "atten_m2",
            circuit_prefix: "atten",
            apply: atten_m2,
            apply2: None,
            factor: Some(2),
            clamps: false,
            bit_depths: &[8, 16],
            multi_input: false,
            state_width: 2,
        },
        // Time-varying: the LFO depends on absolute sample position, so
        // the IVC state carries a proven counter alongside the hashes.
        // `factor` here is the LFO frequency in Hz, used for the artifact
        // stem (tremolo_b8_f5_B100).
        // Merge: the only edit that is a function of TWO input streams.
        // Its three-wide state is three hashes, not two hashes plus a
        // counter, which is why `multi_input` exists.
        // Concat: the duration counterpart to merge. Cheapest edit in
        // the project - one shared BatchHasher and a per-step selector.
        EditSpec {
            name: "concat",
            circuit_prefix: "concat",
            apply: |_, _| unreachable!("concat uses apply2"),
            apply2: Some(concat_streams),
            factor: None,
            clamps: false,
            bit_depths: &[8],
            multi_input: true,
            state_width: 5,
        },
        EditSpec {
            name: "merge",
            circuit_prefix: "merge",
            apply: |_, _| unreachable!("merge uses apply2"),
            apply2: Some(merge_streams),
            factor: None,
            // Averaging cannot overflow, so no saturation is enforced.
            clamps: false,
            bit_depths: &[8],
            multi_input: true,
            state_width: 3,
        },
        // Crop: the only edit that is not one-in-one-out over the whole
        // file. Folding just the kept window makes each step one-to-one
        // again, and the proven counter is what pins which window it was.
        // Cheapest edit in the project: no decompression, no arithmetic.
        EditSpec {
            name: "crop",
            circuit_prefix: "crop",
            apply: crop_window,
            apply2: None,
            factor: None,
            clamps: false,
            bit_depths: &[8],
            multi_input: false,
            state_width: 3,
        },
        // Distortion: memoryless like gain, but the first edit needing a
        // sign test (for |x|) as well as a clamp.
        EditSpec {
            name: "dist_d100",
            circuit_prefix: "dist",
            apply: dist_d100,
            apply2: None,
            factor: Some(100), // wet percent, for the artifact stem
            clamps: true,
            bit_depths: &[8],
            multi_input: false,
            state_width: 2,
        },
        EditSpec {
            name: "dist_d50",
            circuit_prefix: "dist",
            apply: dist_d50,
            apply2: None,
            factor: Some(50),
            clamps: true,
            bit_depths: &[8],
            multi_input: false,
            state_width: 2,
        },
        EditSpec {
            name: "tremolo_f5",
            circuit_prefix: "tremolo",
            apply: tremolo_f5,
            apply2: None,
            factor: Some(5),
            clamps: false,
            bit_depths: &[8],
            multi_input: false,
            state_width: 3,
        },
    ]
    // Not yet available: `distortion` (dist_step.circom unfinished, sign
    // handling commented out) and `pitch_shifter` (floating point, needs a
    // fixed-point redesign before it maps to a field relation).
}

pub fn find_edit(name: &str) -> Option<EditSpec> {
    registry().into_iter().find(|e| e.name == name)
}

#[derive(Clone, Debug)]
pub struct SuiteConfig {
    pub max_n: usize,
    /// Audio durations in seconds to prove. Non-empty overrides the N ladder.
    pub durations: Vec<f64>,
    /// Explicit sample counts. Non-empty overrides both the ladder and
    /// `durations` — the escape hatch for depths whose samples-per-step
    /// does not divide the ladder.
    pub n_list: Vec<usize>,
    /// Sample rate and channel count used to turn a duration into a sample
    /// count. Defaults match the project's 44.1 kHz stereo fixtures.
    pub sample_rate: usize,
    pub channels: usize,
    pub batch_sizes: Vec<usize>,
    pub bit_depth: usize,
    pub audio_input: String,
    pub edit: String,
    pub out_dir: String,
    pub repeat: usize,
    pub skip_compressed: bool,
    pub dry_run: bool,
    pub cache_inputs: bool,
    /// Second input WAV, for multi-input edits. Empty means "derive a
    /// default": the suite refuses rather than silently inventing one.
    pub audio_input_b: String,
    /// LFO period in samples for time-varying edits. Default 8820 =
    /// 5Hz at 44.1kHz, matching the compiled tremolo circuits.
    pub lfo_period: usize,
    pub artifacts_dir: String,
    pub serialized_dir: String,
}

impl Default for SuiteConfig {
    fn default() -> Self {
        SuiteConfig {
            max_n: 0, // required: no sensible default, the cap drives runtime
            durations: Vec::new(),
            n_list: Vec::new(),
            sample_rate: 44100,
            channels: 2,
            batch_sizes: vec![10, 40, 100, 200],
            bit_depth: 16,
            audio_input: "raw_audios/2c_44sr_16bd_01.wav".to_string(),
            edit: "gain2".to_string(),
            out_dir: "logs/bench".to_string(),
            repeat: 3,
            skip_compressed: false,
            dry_run: false,
            cache_inputs: true,
            audio_input_b: String::new(),
            lfo_period: 8820,
            artifacts_dir: "artifacts".to_string(),
            serialized_dir: "serialized".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SkipReason {
    /// N smaller than one batch: `chunks()` would emit a single short batch.
    NTooSmall { samples_per_step: usize },
    /// N not a multiple of one batch: a short final batch the circuit rejects.
    NotDivisible { samples_per_step: usize, remainder: usize },
    MissingArtifact { path: String, compile_with: String },
    InsufficientAudio { available: usize, required: usize },
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::NTooSmall { samples_per_step } => {
                write!(f, "N < one batch ({} samples/step)", samples_per_step)
            }
            SkipReason::NotDivisible { samples_per_step, remainder } => write!(
                f,
                "N not divisible by {} samples/step (remainder {})",
                samples_per_step, remainder
            ),
            SkipReason::MissingArtifact { path, .. } => write!(f, "missing artifact {}", path),
            SkipReason::InsufficientAudio { available, required } => {
                write!(f, "audio has {} samples, needs {}", available, required)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Case {
    pub bit_depth: usize,
    pub batch_size: usize,
    pub total_samples: usize,
    pub comp_size: usize,
    pub iterations: usize,
    pub circuit_name: String,
    pub r1cs_path: String,
    pub wasm_path: String,
    pub input_path: String,
    pub status: Result<(), SkipReason>,
}

impl Case {
    pub fn is_runnable(&self) -> bool {
        self.status.is_ok()
    }

    pub fn samples_per_step(&self) -> usize {
        self.batch_size * self.comp_size
    }
}

/// Builds the full case matrix with skip reasons resolved up front, so a long
/// sweep can be inspected before it starts.
/// Builds the compiled-artifact stem for an edit at a given depth and batch
/// size. The two families name their artifacts differently, because the
/// generalized one is parameterized by the gain factor as well:
///
///   gain2 (baseline):  multiplier_b16_B100
///   gain  (general):   gain_b16_k3_B100
pub fn circuit_name_for(
    prefix: &str,
    factor: Option<u32>,
    bit_depth: usize,
    batch_size: usize,
) -> String {
    match (prefix, factor) {
        ("gain", Some(k)) => format!("gain_b{}_k{}_B{}", bit_depth, k, batch_size),
        // For the atten family the parameter is the shift m, so the stem
        // reads m1/m2 (k = 1/2^m) rather than a multiplier.
        ("atten", Some(m)) => format!("atten_b{}_m{}_B{}", bit_depth, m, batch_size),
        // For tremolo the parameter is the LFO frequency in Hz.
        ("tremolo", Some(f)) => format!("tremolo_b{}_f{}_B{}", bit_depth, f, batch_size),
        // For distortion the parameter is the wet blend as a percentage.
        ("dist", Some(d)) => format!("dist_b{}_d{}_B{}", bit_depth, d, batch_size),
        // Crop takes no parameter: the window is carried by the counter.
        ("crop", None) => format!("crop_b{}_B{}", bit_depth, batch_size),
        // The default merge is the averaging variant; the clamped-sum
        // artifacts keep the unsuffixed name as the cost control.
        ("merge", None) => format!("merge_b{}_avg_B{}", bit_depth, batch_size),
        ("concat", None) => format!("concat_b{}_B{}", bit_depth, batch_size),
        _ => format!("{}_b{}_B{}", prefix, bit_depth, batch_size),
    }
}

/// Where an edit family's `.circom` sources live, relative to the repo
/// root. The circuits tree is organised by family, so the recompile hint a
/// skipped case prints has to point at the right subdirectory or it sends
/// the reader to a path that does not exist.
pub fn circuit_source_dir(prefix: &str) -> &'static str {
    match prefix {
        "gain" => "circuits/edits/gain",
        "atten" => "circuits/edits/attenuation",
        "tremolo" => "circuits/edits/tremolo",
        "dist" => "circuits/edits/distortion",
        "crop" => "circuits/edits/crop",
        "merge" => "circuits/edits/merge",
        "concat" => "circuits/edits/concat",
        // The gain2 baseline still lives in the pre-reorganisation tree.
        _ => "circuits/old",
    }
}

pub fn generate_cases(
    cfg: &SuiteConfig,
    available_samples: usize,
    prefix: &str,
    factor: Option<u32>,
) -> Vec<Case> {
    let comp = comp_size_for(cfg.bit_depth);
    let mut cases = Vec::new();

    let mut batch_sizes = cfg.batch_sizes.clone();
    batch_sizes.sort_unstable();

    // Ascending cost: N outer (dominates runtime), B inner. Cheap cells
    // validate the pipeline before expensive ones commit hours.
    for n in n_targets(cfg) {
        for &b in &batch_sizes {
            let spb = b * comp;
            let circuit_name = circuit_name_for(prefix, factor, cfg.bit_depth, b);
            let r1cs_path = format!("{}/{}.r1cs", cfg.artifacts_dir, circuit_name);
            let wasm_path = format!(
                "{}/{}_js/{}.wasm",
                cfg.artifacts_dir, circuit_name, circuit_name
            );
            let input_path = format!(
                "{}/{}_N{}.json",
                cfg.serialized_dir, circuit_name, n
            );

            let status = if n < spb {
                Err(SkipReason::NTooSmall { samples_per_step: spb })
            } else if n % spb != 0 {
                Err(SkipReason::NotDivisible {
                    samples_per_step: spb,
                    remainder: n % spb,
                })
            } else if available_samples < n {
                Err(SkipReason::InsufficientAudio {
                    available: available_samples,
                    required: n,
                })
            } else if !Path::new(&r1cs_path).exists() {
                Err(SkipReason::MissingArtifact {
                    path: r1cs_path.clone(),
                    compile_with: format!(
                        "./compile.circuit.sh {}/{}.circom",
                        circuit_source_dir(prefix),
                        circuit_name
                    ),
                })
            } else if !Path::new(&wasm_path).exists() {
                Err(SkipReason::MissingArtifact {
                    path: wasm_path.clone(),
                    compile_with: format!(
                        "./compile.circuit.sh {}/{}.circom",
                        circuit_source_dir(prefix),
                        circuit_name
                    ),
                })
            } else {
                Ok(())
            };

            cases.push(Case {
                bit_depth: cfg.bit_depth,
                batch_size: b,
                total_samples: n,
                comp_size: comp,
                iterations: if spb > 0 { n / spb } else { 0 },
                circuit_name,
                r1cs_path,
                wasm_path,
                input_path,
                status,
            });
        }
    }

    cases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_steps_by_ten_from_sixty() {
        assert_eq!(n_ladder(60), vec![60]);
        assert_eq!(n_ladder(6000), vec![60, 600, 6000]);
        assert_eq!(n_ladder(600000), vec![60, 600, 6000, 60000, 600000]);
    }

    #[test]
    fn ladder_cap_is_inclusive_and_excludes_above() {
        assert_eq!(n_ladder(5999), vec![60, 600]);
        assert!(n_ladder(0).is_empty());
        assert!(n_ladder(59).is_empty());
    }

    #[test]
    fn comp_size_matches_circom_formula() {
        assert_eq!(comp_size_for(16), 15);
        assert_eq!(offset_for(16), 32768);
    }

    #[test]
    fn b150_never_divides_the_ladder() {
        // 2250 = 150*15 carries a factor of 9 that 6*10^k never has.
        let comp = comp_size_for(16);
        for n in n_ladder(600000) {
            let spb = 150 * comp;
            assert!(n < spb || n % spb != 0, "N={} unexpectedly divides {}", n, spb);
        }
    }

    #[test]
    fn clean_batch_sizes_divide_every_applicable_n() {
        let comp = comp_size_for(16);
        for &b in &[10usize, 40, 100, 200] {
            for n in n_ladder(600000) {
                let spb = b * comp;
                if n >= spb {
                    assert_eq!(n % spb, 0, "B={} does not divide N={}", b, n);
                }
            }
        }
    }

    #[test]
    fn duration_to_samples_counts_every_channel() {
        // 16-bit 44.1 kHz stereo: 88200 samples per second of audio.
        assert_eq!(samples_for_duration(10.0, 44100, 2), 882_000);
        assert_eq!(samples_for_duration(30.0, 44100, 2), 2_646_000);
        assert_eq!(samples_for_duration(60.0, 44100, 2), 5_292_000);
        assert_eq!(samples_for_duration(120.0, 44100, 2), 10_584_000);
        assert_eq!(samples_for_duration(1.0, 44100, 1), 44_100);
    }

    #[test]
    fn durations_override_the_ladder() {
        let cfg = SuiteConfig {
            max_n: 600,
            durations: vec![10.0, 30.0],
            ..Default::default()
        };
        // The ladder cap is ignored entirely once durations are given.
        assert_eq!(n_targets(&cfg), vec![882_000, 2_646_000]);
    }

    #[test]
    fn durations_are_sorted_and_deduped() {
        let cfg = SuiteConfig {
            max_n: 0,
            durations: vec![30.0, 10.0, 30.0],
            ..Default::default()
        };
        assert_eq!(n_targets(&cfg), vec![882_000, 2_646_000]);
    }

    #[test]
    fn empty_durations_fall_back_to_ladder() {
        let cfg = SuiteConfig { max_n: 6000, ..Default::default() };
        assert_eq!(n_targets(&cfg), vec![60, 600, 6000]);
    }

    #[test]
    fn requested_durations_divide_the_usable_batch_sizes() {
        // Why these four durations are clean: every one is a whole number of
        // steps at every usable B, so none of them skip as NotDivisible.
        let comp = comp_size_for(16);
        for secs in [10.0, 30.0, 60.0, 120.0] {
            let n = samples_for_duration(secs, 44100, 2);
            for &b in &[10usize, 40, 100, 200] {
                assert_eq!(n % (b * comp), 0, "B={} does not divide {}s (N={})", b, secs, n);
            }
        }
    }

    #[test]
    fn recompile_hints_point_at_real_source_files() {
        // A hint naming a nonexistent path is worse than no hint: it sends
        // the reader to a file that was moved. Check every registry edit's
        // hint resolves to a file on disk.
        for e in registry() {
            for &bd in e.bit_depths {
                let name = circuit_name_for(e.circuit_prefix, e.factor, bd, 100);
                let path = format!("{}/{}.circom", circuit_source_dir(e.circuit_prefix), name);
                assert!(
                    std::path::Path::new(&path).exists(),
                    "edit '{}' hint points at missing source {}",
                    e.name, path
                );
            }
        }
    }

    #[test]
    fn atten_artifacts_are_named_by_shift() {
        assert_eq!(
            circuit_name_for("atten", Some(1), 8, 100),
            "atten_b8_m1_B100"
        );
        assert_eq!(
            circuit_name_for("gain", Some(2), 8, 100),
            "gain_b8_k2_B100"
        );
    }

    #[test]
    fn every_registry_edit_has_a_resolvable_name() {
        // A prefix/factor combination with no naming branch would build a
        // path that silently never matches an artifact.
        for e in registry() {
            for &bd in e.bit_depths {
                let n = circuit_name_for(e.circuit_prefix, e.factor, bd, 100);
                assert!(
                    n.contains(&format!("b{}", bd)) && n.ends_with("_B100"),
                    "edit '{}' produced an unexpected stem: {}",
                    e.name, n
                );
            }
        }
    }

    #[test]
    fn n_list_overrides_ladder_and_durations() {
        let cfg = SuiteConfig {
            max_n: 6000,
            durations: vec![10.0],
            n_list: vec![3100, 310, 620, 310],
            ..Default::default()
        };
        assert_eq!(n_targets(&cfg), vec![310, 620, 3100]);
    }

    #[test]
    fn eight_bit_step_never_divides_the_ladder() {
        // bd=8 packs 31 samples/element, so a B=10 step covers 310 and no
        // 60*10^k value is a whole number of steps. This is why --n-list
        // exists.
        let comp = comp_size_for(8);
        assert_eq!(comp, 31);
        for n in n_ladder(600000) {
            let spb = 10 * comp;
            assert!(n < spb || n % spb != 0, "N={} unexpectedly divides {}", n, spb);
        }
    }

    #[test]
    fn skips_are_classified() {
        let cfg = SuiteConfig {
            max_n: 600,
            batch_sizes: vec![10, 100],
            artifacts_dir: "/nonexistent".to_string(),
            ..Default::default()
        };
        let cases = generate_cases(&cfg, 705_600, "multiplier", Some(2));
        // Ladder is {60, 600}. Below one batch: B=10 at N=60 (150/step), and
        // B=100 at both N=60 and N=600 (1500/step).
        let too_small = cases
            .iter()
            .filter(|c| matches!(c.status, Err(SkipReason::NTooSmall { .. })))
            .count();
        assert_eq!(too_small, 3);
        // B=10 cells reach the artifact check and fail it.
        assert!(cases
            .iter()
            .any(|c| matches!(c.status, Err(SkipReason::MissingArtifact { .. }))));
    }
}
