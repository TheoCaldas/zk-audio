//! Option B: prove an edit.

use nova_snark::traits::Engine;

use crate::audio_parser;
use crate::bench::config::{circuit_name_for, comp_size_for, registry, EditCtx, EditSpec};
use crate::circuit::{self, CaseMeta, RunConfig};
use crate::cli::bundle::{self, ProofBundle};
use crate::cli::prompt;
use crate::cli::{stem, Timer};
use crate::poseidon::BatchHashKind;

type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

/// Edits this step can drive: single-input, and not a benchmark ablation.
///
/// `apply2.is_some()` filters out concat and merge, which need a second
/// stream; they appear here automatically once supported. The ablation
/// variants are excluded by name because several are deliberately unsound
/// and must never be offered as a provenance proof.
fn provable_edits() -> Vec<EditSpec> {
    registry()
        .into_iter()
        .filter(|e| e.apply2.is_none())
        .filter(|e| {
            !["_nohash", "_wide", "_wide4", "_wide8", "_onehash", "_sponge4", "_sponge8", "_sponge2h"]
                .iter()
                .any(|suffix| e.name.ends_with(suffix))
        })
        .collect()
}

/// A one-line description of what each family's parameter means, so the
/// pick-list is readable without consulting the registry.
fn describe(e: &EditSpec) -> String {
    let what = match e.circuit_prefix {
        "gain" => format!("clamp({}x), amplify", e.factor.unwrap_or(0)),
        "atten" => format!("x >> {}, attenuate", e.factor.unwrap_or(0)),
        "dist" => format!("rectifier fold at {}% wet", e.factor.unwrap_or(0)),
        "tremolo" => format!("{} Hz amplitude modulation", e.factor.unwrap_or(0)),
        "crop" => "verbatim contiguous window".to_string(),
        "multiplier" => "2x, NO clamping (legacy baseline)".to_string(),
        other => other.to_string(),
    };
    format!("{:<12} {:<34} depths {:?}", e.name, what, e.bit_depths)
}

pub fn run() -> Result<(), String> {
    println!();
    println!("--- B) prove an edit ---");

    let edits = provable_edits();
    let labels: Vec<String> = edits.iter().map(describe).collect();
    let idx = prompt::pick("operation:", &labels).ok_or("cancelled")?;
    let edit = &edits[idx];

    // Each k/m/d/f value is a separately compiled circuit, so the choice
    // was already made by picking the edit. Only depth and batch size are
    // free, and only among compiled combinations.
    let bit_depth = loop {
        let bd = prompt::usize_or("bit depth", 16).ok_or("cancelled")?;
        if edit.bit_depths.contains(&bd) {
            break bd;
        }
        println!("  {} has circuits at {:?} only", edit.name, edit.bit_depths);
    };
    let batch_size = prompt::usize_or("batch size", 100).ok_or("cancelled")?;

    let comp_size = comp_size_for(bit_depth);
    let samples_per_step = batch_size * comp_size;

    let circuit_name = circuit_name_for(edit.circuit_prefix, edit.factor, bit_depth, batch_size);
    let r1cs = format!("artifacts/{circuit_name}.r1cs");
    let wasm = format!("artifacts/{circuit_name}_js/{circuit_name}.wasm");
    for p in [&r1cs, &wasm] {
        if !std::path::Path::new(p).exists() {
            println!();
            println!("no compiled circuit for {circuit_name} ({p} missing).");
            println!("compile it with:");
            println!("  ./compile.circuit.sh circuits/edits/{}/{circuit_name}.circom", family(edit));
            return Err("circuit not available".to_string());
        }
    }
    println!("circuit   {circuit_name}");

    // The LFO period is a genuine runtime parameter: unlike k/m/d/f it is
    // not compiled in.
    let period = if edit.circuit_prefix == "tremolo" {
        prompt::usize_or("LFO period in samples (8820 = 5Hz at 44.1kHz)", 8820)
            .ok_or("cancelled")?
    } else {
        8820
    };

    let audio = prompt::existing_path("audio source path: ").ok_or("cancelled")?;
    let out_dir = prompt::text_or("output directory", ".").ok_or("cancelled")?;
    let out_dir = out_dir.trim_end_matches('/').to_string();

    let (samples, file_bits) = audio_parser::read_samples_signed(&audio);
    if file_bits as usize != bit_depth {
        println!(
            "note: {audio} is {file_bits}-bit, proving at {bit_depth}-bit"
        );
    }

    // crop carries its window start in the IVC state, so z0 is not zero.
    let (window_start, initial_state) = if edit.circuit_prefix == "crop" {
        let s = prompt::usize_or("window start (samples, fold-aligned)", 0).ok_or("cancelled")?;
        if s % samples_per_step != 0 {
            return Err(format!(
                "window start {s} is not a multiple of {samples_per_step} samples/step"
            ));
        }
        (s, vec![(2usize, s as u64)])
    } else {
        (0, Vec::new())
    };

    // Offer a cap: the whole file is often far more proving than wanted,
    // and a fold is the smallest unit that can be proven.
    let available = samples.len().saturating_sub(window_start);
    let max_folds = available / samples_per_step;
    if max_folds == 0 {
        return Err(format!(
            "{available} samples from offset {window_start} is less than one step of \
             {samples_per_step}"
        ));
    }
    println!(
        "{available} samples available from offset {window_start} = {max_folds} folds of \
         {samples_per_step}"
    );
    let folds = loop {
        let f = prompt::usize_or("folds to prove", max_folds).ok_or("cancelled")?;
        if f >= 1 && f <= max_folds {
            break f;
        }
        println!("  enter 1-{max_folds}");
    };
    let available = folds * samples_per_step;
    let usable = available;
    let original: Vec<i32> = samples[window_start..window_start + usable].to_vec();
    let iterations = usable / samples_per_step;

    let ctx = EditCtx {
        bit_depth,
        batch_size,
        start_index: window_start,
        comp_size,
        period,
    };
    let t = Timer::start();
    let edited: Vec<i32> = (edit.apply)(&original, &ctx).iter().map(|&s| s as i32).collect();
    println!("edit applied off-circuit in {}", t.elapsed());

    let (rate, channels) = audio_parser::read_rate_and_channels(&audio);
    let edited_path = format!("{out_dir}/{}_{}.wav", stem(&audio), edit.name);
    audio_parser::write_file_at(
        &edited_path,
        &edited,
        rate,
        bit_depth as u32,
        if channels == 1 {
            audio_parser::WaveType::Mono
        } else {
            audio_parser::WaveType::Stereo
        },
    );
    println!("edited audio -> {edited_path}");

    predict(edit.name, bit_depth, batch_size, usable);

    println!();
    println!("{iterations} folds over {usable} samples at B={batch_size}, bd={bit_depth}.");
    if !prompt::confirm("start proving?").ok_or("cancelled")? {
        println!("not proving; the edited audio was still written.");
        return Ok(());
    }

    let json_input = format!("serialized/{circuit_name}_N{usable}.json");
    audio_parser::serialize_samples_at(
        &original,
        &edited,
        samples_per_step,
        bit_depth as u32,
        &json_input,
    );

    let cfg = RunConfig {
        circuit_filepath: &r1cs,
        witness_gen_filepath: &wasm,
        json_input_filepath: &json_input,
        skip_compressed: false,
        verbose: true,
        multi_input: edit.multi_input,
        initial_state: initial_state.clone(),
        state_width: edit.state_width,
    };
    let meta = CaseMeta {
        circuit_name: circuit_name.clone(),
        edit: edit.name.to_string(),
        audio_input: audio.clone(),
        bit_depth,
        batch_size,
        comp_size,
        total_samples: usable,
        repetition: 0,
    };

    println!();
    let t = Timer::start();
    let (metrics, artifacts) = circuit::run_proof_keep(&cfg, &meta).map_err(|e| e.to_string())?;
    let total = t.elapsed();
    let artifacts = artifacts.ok_or("no compressed proof was produced")?;

    // Cross-check the Rust commitment against the circuit's before writing
    // a bundle: a mismatch here is a bug, not a forgery.
    let h_orig = audio_parser::hex_stream_commitment::<Fq>(
        &original,
        bit_depth as u32,
        samples_per_step,
        BatchHashKind::for_circuit(&circuit_name),
    );
    if h_orig != metrics.final_hash_original {
        return Err(format!(
            "commitment mismatch: circuit {} vs recomputed {}",
            metrics.final_hash_original, h_orig
        ));
    }

    let mut z0 = vec![0u64; edit.state_width];
    for (i, v) in &initial_state {
        z0[*i] = *v;
    }

    let proof_path = format!("{out_dir}/{}_{}.proof.json", stem(&audio), edit.name);
    bundle::write(
        &proof_path,
        &ProofBundle {
            kind: ProofBundle::KIND.to_string(),
            circuit_name,
            edit: edit.name.to_string(),
            bit_depth,
            batch_size,
            comp_size,
            samples: usable,
            iterations: metrics.iterations,
            z0,
            h_orig: metrics.final_hash_original.clone(),
            h_edit: metrics.final_hash_edited.clone(),
            proof: serde_json::from_slice(&artifacts.proof_json).map_err(|e| e.to_string())?,
        },
    )?;

    println!();
    println!("proving took {total}");
    println!("  fold chain   {:.1}s", metrics.recursive_prove_ms / 1000.0);
    println!("  compression  {:.1}s", metrics.compressed_prove_ms / 1000.0);
    println!("constraints/step {}", metrics.constraints_primary);
    println!("h_orig    {}", metrics.final_hash_original);
    println!("h_edit    {}", metrics.final_hash_edited);
    println!("proof     {proof_path} ({} KB)", artifacts.proof_json.len() / 1024);
    Ok(())
}

/// Which `circuits/edits/` directory a family lives in, for the compile hint.
fn family(e: &EditSpec) -> &'static str {
    match e.circuit_prefix {
        "gain" | "multiplier" => "gain",
        "atten" => "attenuation",
        "dist" => "distortion",
        other => other,
    }
}

/// Logs the predicted proving time from the measured cost model. Advisory
/// only: a missing python3 or script must not block proving.
fn predict(edit: &str, bit_depth: usize, batch_size: usize, samples: usize) {
    let out = std::process::Command::new("python3")
        .args([
            "scripts/predict_proof_time.py",
            "--edit", edit,
            "--bit-depth", &bit_depth.to_string(),
            "--batch-size", &batch_size.to_string(),
            "--n", &samples.to_string(),
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            for l in text.lines().filter(|l| l.contains("PROVE TIME") || l.contains("ms per fold")) {
                println!("predicted: {}", l.trim());
            }
        }
        _ => println!("(no time estimate available)"),
    }
}
