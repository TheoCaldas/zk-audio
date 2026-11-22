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

/// Edits this step can drive: the production circuits under
/// `circuits/edits/<family>/`.
///
/// The ablation variants are excluded by name because several are
/// deliberately unsound and must never be offered as a provenance proof.
/// The `multiplier` prefix is the pre-sponge baseline kept for benchmark
/// comparison: it absorbs with the arity-2 chain, not the rate-4 sponge,
/// so offering it here would hand out commitments under a different
/// construction.
fn provable_edits() -> Vec<EditSpec> {
    registry()
        .into_iter()
        .filter(|e| e.circuit_prefix != "multiplier")
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
        "crop_bound" => "verbatim window, bound to the whole original".to_string(),
        "merge" => "floor((a+b)/2), mix two sources".to_string(),
        "concat" => "a then b, join two sources".to_string(),
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
    println!("using circuit {circuit_name}");

    // The LFO period is a genuine runtime parameter: unlike k/m/d/f it is
    // not compiled in.
    let period = if edit.circuit_prefix == "tremolo" {
        prompt::usize_or("LFO period in samples (8820 = 5Hz at 44.1kHz)", 8820)
            .ok_or("cancelled")?
    } else {
        8820
    };

    // A two-input relation needs both sources up front; the prompt says
    // which is which, because neither merge's commitment pair nor
    // concat's ordering is symmetric in the bundle.
    let two_input = edit.apply2.is_some();
    let audio = match two_input {
        true => prompt::existing_path("source A path: ").ok_or("cancelled")?,
        false => prompt::existing_path("audio source path: ").ok_or("cancelled")?,
    };
    let audio_b = match two_input {
        true => Some(prompt::existing_path("source B path: ").ok_or("cancelled")?),
        false => None,
    };
    let out_dir = prompt::text_or("output directory", ".").ok_or("cancelled")?;
    let out_dir = out_dir.trim_end_matches('/').to_string();

    let (samples, file_bits) = audio_parser::read_samples_signed(&audio);

    // Every circuit range-checks its samples to the chosen bit depth, so a
    // source carrying wider samples cannot be proven at all - the witness
    // is rejected rather than losing precision. Refuse here, where the fix
    // is still cheap, instead of panicking inside the edit.
    let (lo, hi) = crate::audio_editor::sample_limits(bit_depth as u32);
    let range_check = |path: &str, s: &[i32], bits: u32| -> Result<(), String> {
        let Some((i, bad)) = s
            .iter()
            .enumerate()
            .find_map(|(i, &v)| (v < lo || v > hi).then_some((i, v)))
        else {
            return Ok(());
        };
        let depths = edit.bit_depths;
        let hint = if depths.contains(&(bits as usize)) {
            format!("rerun and answer {bits} at the bit depth prompt")
        } else {
            format!(
                "{} has circuits at {depths:?} only, so this source needs one of those \
                 depths; convert it or pick another edit",
                edit.name
            )
        };
        Err(format!(
            "{path} is {bits}-bit and cannot be proven at {bit_depth}-bit: sample \
             {bad} at index {i} is outside {lo}..={hi}. The circuit range-checks every \
             sample to the proving depth, so this is unprovable, not merely lossy - {hint}."
        ))
    };
    range_check(&audio, &samples, file_bits)?;
    if file_bits as usize != bit_depth {
        println!("note: {audio} is {file_bits}-bit, proving at {bit_depth}-bit");
    }

    // Stream B is proven under the same circuit, so it gets the same
    // range check rather than failing later inside the witness generator.
    let samples_b = match &audio_b {
        Some(path) => {
            let (b, b_bits) = audio_parser::read_samples_signed(path);
            range_check(path, &b, b_bits)?;
            if b_bits as usize != bit_depth {
                println!("note: {path} is {b_bits}-bit, proving at {bit_depth}-bit");
            }
            Some(b)
        }
        None => None,
    };

    // Both crops carry a window in the IVC state, so z0 is not zero. They
    // differ in what gets folded: `crop` folds only the window, so the
    // offset is where its stream starts; `crop_bound` folds the whole
    // original from 0 and the window only gates the edited chain.
    let bound = edit.circuit_prefix == "crop_bound";
    let (window_start, initial_state) = if edit.circuit_prefix == "crop" || bound {
        let s = prompt::usize_or("window start (samples, fold-aligned)", 0).ok_or("cancelled")?;
        if s % samples_per_step != 0 {
            return Err(format!(
                "window start {s} is not a multiple of {samples_per_step} samples/step"
            ));
        }
        match bound {
            // win_len is not known until the length prompt; filled below.
            true => (s, vec![(3usize, s as u64)]),
            false => (s, vec![(2usize, s as u64)]),
        }
    } else {
        (0, Vec::new())
    };

    // Offer a cap: the whole file is often far more proving than wanted.
    // The choice is in samples; a tail below a step boundary is padded up
    // to one rather than refused.
    // A two-input relation folds both streams in lockstep, so the usable
    // length is whichever source runs out first.
    let available = match &samples_b {
        Some(b) => samples.len().min(b.len()).saturating_sub(window_start),
        None => samples.len().saturating_sub(window_start),
    };
    if available == 0 {
        return Err(format!("no samples at or after offset {window_start}"));
    }
    let max_folds = audio_parser::padded_len(available, samples_per_step) / samples_per_step;
    println!(
        "{available} samples available = {max_folds} steps of {samples_per_step} samples \
         once padded"
    );
    if bound {
        println!(
            "crop_bound folds the WHOLE original regardless, so this is the window \
             length, not the proving cost."
        );
    }
    let n_samples = loop {
        let n = prompt::usize_or(
            match bound {
                true => "how many samples to keep",
                false => "how many samples to prove",
            },
            available,
        )
        .ok_or("cancelled")?;
        if n >= 1 && n <= available {
            break n;
        }
        println!("  enter 1-{available}");
    };
    // `crop_bound` binds the excerpt to a commitment over the whole
    // original, so the folded stream is the original itself - every batch
    // of it, from sample 0 - and the window is a sub-range of that. The
    // window length must be fold-aligned too, since the gate is per-step.
    let window_len = match bound {
        true => audio_parser::padded_len(n_samples, samples_per_step),
        false => n_samples,
    };
    // The window may end inside the silence pad - the pad is folded and
    // committed like any other batch - but not past it, since there are
    // no further folds for the gate to select.
    if bound {
        let folded = audio_parser::padded_len(samples.len(), samples_per_step);
        if window_start + window_len > folded {
            return Err(format!(
                "the window [{window_start}, {}) runs past the {folded} samples the \
                 proof folds. The window length is rounded up to a fold boundary, so a \
                 window ending inside the last partial step needs a shorter one.",
                window_start + window_len
            ));
        }
    }
    // The edit is applied to the PADDED original, not padded afterwards:
    // the relation has to hold on the pad too, and `edit(0)` is not always
    // 0 (distortion maps silence to -2^(bd-1)/2).
    //
    // For `crop_bound` the proven stream is the WHOLE original, from
    // sample 0 to the end of the file. Stopping at the window's end would
    // commit h_orig to a prefix, and a prefix commitment cannot equal a
    // signature over the file - which is exactly the binding this circuit
    // exists to provide. The cost is one fold per batch of the original,
    // not per batch of the excerpt.
    let (proven_from, proven_n) = match bound {
        true => (0usize, samples.len()),
        false => (window_start, n_samples),
    };
    let padded_samples = audio_parser::padded_len(proven_n, samples_per_step);
    let mut original: Vec<i32> = samples[proven_from..proven_from + proven_n].to_vec();
    original.resize(padded_samples, 0);
    let usable = padded_samples;
    let iterations = padded_samples / samples_per_step;
    if padded_samples > proven_n {
        println!(
            "padding {} samples of silence to reach {iterations} steps",
            padded_samples - proven_n
        );
    }
    if bound {
        println!(
            "folding the whole original: {padded_samples} samples, {iterations} steps. \
             Window [{window_start}, {}) is what gets published.",
            window_start + window_len
        );
        println!(
            "binding the excerpt to the full commitment costs {iterations} folds rather \
             than {}; that is the price of h_orig covering the signed stream.",
            window_len / samples_per_step
        );
    }

    // Stream B is padded to the same boundary, so both chains fold the
    // same number of steps.
    let original_b: Option<Vec<i32>> = samples_b.as_ref().map(|b| {
        let mut v: Vec<i32> = b[window_start..window_start + n_samples].to_vec();
        v.resize(padded_samples, 0);
        v
    });

    let ctx = EditCtx {
        bit_depth,
        batch_size,
        // Where the folded slice begins in the file, which is 0 for
        // `crop_bound`: an index-dependent edit reads absolute positions
        // from here, so handing it the window start would shift its phase.
        start_index: proven_from,
        comp_size,
        period,
    };
    let t = Timer::start();
    let edited: Vec<i32> = match (&original_b, edit.apply2) {
        (Some(b), Some(f)) => f(&original, b, &ctx),
        _ => (edit.apply)(&original, &ctx),
    }
    .iter()
    .map(|&s| s as i32)
    .collect();
    let _ = t;

    // Concat's boundary is carried in the IVC state, and the circuit
    // constrains it to a whole number of folds. `concat_streams` splits at
    // the fold-aligned midpoint of what it is handed, so the CLI must
    // declare the same point or the proof would attest a different join.
    let concat_len_a = match edit.circuit_prefix == "concat" {
        true => Some(((padded_samples / 2) / samples_per_step) * samples_per_step),
        false => None,
    };
    let initial_state = match (concat_len_a, bound) {
        (Some(len_a), _) => vec![(4usize, len_a as u64)],
        // The window's length joins its start in the state: the circuit
        // gates on both, and carrying them through unchanged is what lets
        // the verifier read back the window it supplied.
        (None, true) => vec![(3usize, window_start as u64), (4usize, window_len as u64)],
        (None, false) => initial_state,
    };
    if let Some(len_a) = concat_len_a {
        println!(
            "join at {len_a} samples: A contributes [0, {len_a}), B contributes \
             [{len_a}, {padded_samples})"
        );
    }

    let (rate, channels) = audio_parser::read_rate_and_channels(&audio);
    let edited_path = format!("{out_dir}/{}_{}.wav", stem(&audio), edit.name);
    // Publish the audio, not the padding: the verifier re-pads from the
    // signed length, and a file carrying the pad would be N_padded long.
    //
    // `crop_bound` folds the original from 0 but publishes only the
    // window, so what it writes is the slice the gate selected.
    let published: &[i32] = match bound {
        true => &edited[window_start..window_start + n_samples],
        false => &edited[..n_samples],
    };
    audio_parser::write_file_at(
        &edited_path,
        published,
        rate,
        bit_depth as u32,
        if channels == 1 {
            audio_parser::WaveType::Mono
        } else {
            audio_parser::WaveType::Stereo
        },
    );
    println!("edited audio written to {edited_path}");

    predict(edit.name, bit_depth, batch_size, usable);

    println!();
    println!("Ready to prove {n_samples} samples ({iterations} steps, {usable} padded).");
    if !prompt::confirm("start proving?").ok_or("cancelled")? {
        println!("not proving; the edited audio was still written.");
        return Ok(());
    }

    let json_input = format!("serialized/{circuit_name}_N{usable}.json");
    // `serialized/` is gitignored, so a fresh checkout does not have it.
    std::fs::create_dir_all("serialized")
        .map_err(|e| format!("cannot create serialized/: {e}"))?;
    match &original_b {
        // A two-input circuit reads stream B from `stream_b`; the
        // single-input serializer has no slot for it.
        Some(b) => audio_parser::serialize_merge_at(
            &original,
            b,
            &edited,
            samples_per_step,
            bit_depth as u32,
            &json_input,
        ),
        None => audio_parser::serialize_samples_at(
            &original,
            &edited,
            samples_per_step,
            bit_depth as u32,
            &json_input,
        ),
    }

    let cfg = RunConfig {
        circuit_filepath: &r1cs,
        witness_gen_filepath: &wasm,
        json_input_filepath: &json_input,
        skip_compressed: false,
        verbose: false,
        progress: true,
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

    // Cross-check the Rust commitments against the circuit's before
    // writing a bundle: a mismatch here is a bug, not a forgery.
    //
    // Concat gates its source chains, so neither covers the whole padded
    // stream: A absorbs only below the join and B only from it on. The
    // recomputation has to fold the same spans the circuit did.
    let kind = BatchHashKind::for_circuit(&circuit_name);
    let commit = |s: &[i32]| {
        audio_parser::hex_stream_commitment::<Fq>(s, bit_depth as u32, samples_per_step, kind)
    };
    let (span_a, span_b) = match concat_len_a {
        Some(len_a) => (&original[..len_a], original_b.as_deref().map(|b| &b[len_a..])),
        None => (&original[..], original_b.as_deref()),
    };

    let h_orig = commit(span_a);
    if h_orig != metrics.final_hash_original {
        return Err(format!(
            "commitment mismatch on stream A: circuit {} vs recomputed {}",
            metrics.final_hash_original, h_orig
        ));
    }
    let h_b = match (span_b, metrics.final_hash_b.as_deref()) {
        (Some(b), Some(circuit_b)) => {
            let recomputed = commit(b);
            if recomputed != circuit_b {
                return Err(format!(
                    "commitment mismatch on stream B: circuit {circuit_b} vs recomputed \
                     {recomputed}"
                ));
            }
            Some(recomputed)
        }
        // A two-input proof whose circuit reported no third state element
        // would leave stream B uncommitted, so the bundle would claim more
        // than the proof shows.
        (Some(_), None) => {
            return Err(format!(
                "{} folds two streams but the proof carries no stream-B commitment",
                edit.name
            ))
        }
        _ => None,
    };

    // `crop_bound`'s two chains cover different spans, so the edited one
    // is checked too: h_orig covering the whole stream is the new claim,
    // and a gate that absorbed the wrong batches would still produce a
    // matching h_orig while committing the excerpt to something else.
    if bound {
        let h_window = commit(&original[window_start..window_start + window_len]);
        if h_window != metrics.final_hash_edited {
            return Err(format!(
                "commitment mismatch on the window: circuit {} vs recomputed {h_window}",
                metrics.final_hash_edited
            ));
        }
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
            // For `crop_bound`, `samples` describes the stream h_orig
            // covers - the original - so a signature over it can agree.
            // The window the verifier actually holds goes in samples_out.
            samples: match bound {
                true => proven_n,
                false => n_samples,
            },
            padded_samples,
            samples_out: bound.then_some(n_samples),
            iterations: metrics.iterations,
            z0,
            h_orig: metrics.final_hash_original.clone(),
            h_edit: metrics.final_hash_edited.clone(),
            h_b,
            len_a: concat_len_a,
            window: bound.then_some((window_start, window_len)),
            proof: serde_json::from_slice(&artifacts.proof_json).map_err(|e| e.to_string())?,
        },
    )?;

    println!();
    println!("proved in {total}");
    println!("proof written to {proof_path} ({} KB)", artifacts.proof_json.len() / 1024);
    println!();
    println!("Send the edited audio and the proof, with the signature from");
    println!("option A, to whoever verifies (option C).");
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
                println!("estimate: {}", l.trim());
            }
        }
        _ => println!("(no time estimate available)"),
    }
}
