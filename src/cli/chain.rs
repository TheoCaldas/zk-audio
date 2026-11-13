//! Options D and E: prove a chain of edits, verify a chain bundle.
//!
//! A chain is proven one stage at a time and linked by commitment
//! equality: stage `k`'s edited-stream commitment is stage `k+1`'s
//! original-stream commitment. Each stage writes an ordinary
//! `ProofBundle`, so a chain adds no proof format; the `ChainBundle` names
//! them in order. See docs/edit_chains.md.

use nova_snark::traits::Engine;

use crate::audio_parser;
use crate::bench::config::{circuit_name_for, comp_size_for, registry, EditCtx, EditSpec};
use crate::circuit::{self, CaseMeta, RunConfig};
use crate::cli::bundle::{self, ChainBundle, ProofBundle, SignatureBundle};
use crate::cli::prompt;
use crate::cli::verify::folds_for;
use crate::cli::{fmt_secs, stem, Timer};
use crate::poseidon::BatchHashKind;
use crate::protocol;
use crate::signer;

type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

/// Edits offerable as a chain stage.
///
/// The same exclusions as the single-edit step - ablation variants and the
/// pre-sponge `multiplier` baseline - plus `concat`, which cannot be a
/// stage at all: its gated chains commit to either side of the join
/// separately, so a preceding stage's whole-stream commitment matches
/// neither.
fn chainable_edits() -> Vec<EditSpec> {
    registry()
        .into_iter()
        .filter(|e| e.circuit_prefix != "multiplier")
        .filter(|e| e.name != "concat")
        .filter(|e| {
            !["_nohash", "_wide", "_wide4", "_wide8", "_onehash", "_sponge4", "_sponge8", "_sponge2h"]
                .iter()
                .any(|suffix| e.name.ends_with(suffix))
        })
        .collect()
}

/// True when an edit may only be the last stage, because it changes the
/// stream length and the link is a commitment over a padded stream.
fn terminal_only(name: &str) -> bool {
    name == "crop"
}

fn describe(e: &EditSpec) -> String {
    let what = match e.circuit_prefix {
        "gain" => format!("clamp({}x), amplify", e.factor.unwrap_or(0)),
        "atten" => format!("x >> {}, attenuate", e.factor.unwrap_or(0)),
        "dist" => format!("rectifier fold at {}% wet", e.factor.unwrap_or(0)),
        "tremolo" => format!("{} Hz amplitude modulation", e.factor.unwrap_or(0)),
        "crop" => "verbatim contiguous window (last stage only)".to_string(),
        "merge" => "floor((a+b)/2), mix two sources (last stage only)".to_string(),
        other => other.to_string(),
    };
    format!("{:<12} {:<44} depths {:?}", e.name, what, e.bit_depths)
}

/// One stage, as the prompts collected it.
struct PickedStage {
    spec: EditSpec,
    circuit_name: String,
    r1cs: String,
    wasm: String,
    /// Source B's path, for a two-input terminal stage.
    source_b: Option<String>,
    period: usize,
}

pub fn prove() -> Result<(), String> {
    println!();
    println!("--- D) prove a chain of edits ---");
    println!("Each stage proves its own edit; the stages are linked by");
    println!("requiring one stage's output commitment to be the next one's input.");

    let edits = chainable_edits();
    let labels: Vec<String> = edits.iter().map(describe).collect();

    // Depth and batch size are fixed for the whole chain: the link is a
    // commitment, and a commitment is a fold over a partition, so two
    // stages under different partitions could not link at all.
    let bit_depth = prompt::usize_or("bit depth (all stages)", 16).ok_or("cancelled")?;
    let batch_size = prompt::usize_or("batch size (all stages)", 100).ok_or("cancelled")?;
    let comp_size = comp_size_for(bit_depth);
    let samples_per_step = batch_size * comp_size;

    let n_stages = loop {
        let n = prompt::usize_or("how many stages", 2).ok_or("cancelled")?;
        if (1..=8).contains(&n) {
            break n;
        }
        println!("  enter 1-8");
    };

    let mut stages: Vec<PickedStage> = Vec::with_capacity(n_stages);
    for i in 0..n_stages {
        println!();
        println!("stage {} of {n_stages}:", i + 1);
        let last = i + 1 == n_stages;

        let spec = loop {
            let idx = prompt::pick("operation:", &labels).ok_or("cancelled")?;
            let e = &edits[idx];
            if !e.bit_depths.contains(&bit_depth) {
                println!("  {} has circuits at {:?} only", e.name, e.bit_depths);
                continue;
            }
            // Enforced here rather than after proving: a chain whose
            // interior stage changes the stream length cannot link, and
            // discovering that after minutes of folding is pointless.
            if !last && (terminal_only(e.name) || e.apply2.is_some()) {
                println!(
                    "  {} may only be the LAST stage: it {}. Pick it for stage {n_stages}, \
                     or choose another edit here.",
                    e.name,
                    if e.apply2.is_some() {
                        "takes a second source"
                    } else {
                        "changes the stream length"
                    }
                );
                continue;
            }
            break registry()
                .into_iter()
                .find(|r| r.name == e.name)
                .ok_or("edit vanished from the registry")?;
        };

        let circuit_name = circuit_name_for(spec.circuit_prefix, spec.factor, bit_depth, batch_size);
        let r1cs = format!("artifacts/{circuit_name}.r1cs");
        let wasm = format!("artifacts/{circuit_name}_js/{circuit_name}.wasm");
        for p in [&r1cs, &wasm] {
            if !std::path::Path::new(p).exists() {
                return Err(format!(
                    "no compiled circuit for {circuit_name} ({p} missing). Compile it \
                     before proving a chain that uses it."
                ));
            }
        }
        println!("  using circuit {circuit_name}");

        let period = if spec.circuit_prefix == "tremolo" {
            prompt::usize_or("  LFO period in samples (8820 = 5Hz at 44.1kHz)", 8820)
                .ok_or("cancelled")?
        } else {
            8820
        };

        let source_b = match spec.apply2.is_some() {
            true => Some(prompt::existing_path("  source B path: ").ok_or("cancelled")?),
            false => None,
        };

        stages.push(PickedStage { spec, circuit_name, r1cs, wasm, source_b, period });
    }

    println!();
    let audio = prompt::existing_path("audio source path: ").ok_or("cancelled")?;
    let out_dir = prompt::text_or("output directory", ".").ok_or("cancelled")?;
    let out_dir = out_dir.trim_end_matches('/').to_string();

    let (samples, file_bits) = audio_parser::read_samples_signed(&audio);
    let (lo, hi) = crate::audio_editor::sample_limits(bit_depth as u32);
    if let Some((i, bad)) = samples
        .iter()
        .enumerate()
        .find_map(|(i, &v)| (v < lo || v > hi).then_some((i, v)))
    {
        return Err(format!(
            "{audio} is {file_bits}-bit and cannot be proven at {bit_depth}-bit: sample \
             {bad} at index {i} is outside {lo}..={hi}."
        ));
    }

    // A two-input terminal stage folds both streams in lockstep, so the
    // usable length is bounded by stream B too.
    let mut available = samples.len();
    let mut b_samples: Option<Vec<i32>> = None;
    if let Some(path) = stages.last().and_then(|s| s.source_b.as_ref()) {
        let (b, b_bits) = audio_parser::read_samples_signed(path);
        if let Some((i, bad)) = b
            .iter()
            .enumerate()
            .find_map(|(i, &v)| (v < lo || v > hi).then_some((i, v)))
        {
            return Err(format!(
                "{path} is {b_bits}-bit and cannot be proven at {bit_depth}-bit: sample \
                 {bad} at index {i} is outside {lo}..={hi}."
            ));
        }
        available = available.min(b.len());
        b_samples = Some(b);
    }

    let max_folds = folds_for(available, samples_per_step);
    println!(
        "{available} samples available = {max_folds} steps of {samples_per_step} samples \
         once padded"
    );
    let n_samples = loop {
        let n = prompt::usize_or("how many samples to prove", available).ok_or("cancelled")?;
        if n >= 1 && n <= available {
            break n;
        }
        println!("  enter 1-{available}");
    };

    let padded_samples = audio_parser::padded_len(n_samples, samples_per_step);
    let iterations = padded_samples / samples_per_step;
    if padded_samples > n_samples {
        println!(
            "padding {} samples of silence to reach {iterations} steps",
            padded_samples - n_samples
        );
    }

    println!();
    println!(
        "Ready to prove {} stages over {n_samples} samples ({iterations} steps each).",
        stages.len()
    );
    println!(
        "Cost is the SUM of the stages, including one proof compression each; see \
         docs/edit_chains.md."
    );
    for (i, s) in stages.iter().enumerate() {
        println!("  stage {} : {}", i + 1, s.spec.name);
    }
    if !prompt::confirm("start proving?").ok_or("cancelled")? {
        println!("not proving.");
        return Ok(());
    }

    // Each stage runs on the PADDED stream and hands the padded result on,
    // so a stage sees exactly the samples the preceding stage committed
    // to. The relation has to hold on the pad too.
    let mut current: Vec<i32> = samples[..n_samples].to_vec();
    current.resize(padded_samples, 0);

    let chain_stem = stem(&audio);
    let mut stage_paths: Vec<String> = Vec::new();
    let mut stage_edits: Vec<String> = Vec::new();
    let mut h_orig_first: Option<String> = None;
    let mut prev_h_edit: Option<String> = None;
    let mut published: Vec<i32> = Vec::new();
    let mut published_n = n_samples;

    let t_all = Timer::start();
    for (i, s) in stages.iter().enumerate() {
        println!();
        println!("=== stage {} of {}: {} ===", i + 1, stages.len(), s.spec.name);

        let ctx = EditCtx {
            bit_depth,
            batch_size,
            start_index: 0,
            comp_size,
            period: s.period,
        };

        // Stream B is padded to the same partition, so both sides of a
        // two-input relation fold equal-length streams.
        let stream_b: Option<Vec<i32>> = match (&s.source_b, &b_samples) {
            (Some(_), Some(b)) => {
                let mut v: Vec<i32> = b[..n_samples].to_vec();
                v.resize(padded_samples, 0);
                Some(v)
            }
            _ => None,
        };

        let next: Vec<i32> = match (&stream_b, s.spec.apply2) {
            (Some(b), Some(f)) => f(&current, b, &ctx),
            _ => (s.spec.apply)(&current, &ctx),
        };

        let json_input = format!("serialized/_chain_{chain_stem}_s{i}_{}.json", s.circuit_name);
        match &stream_b {
            Some(b) => audio_parser::serialize_merge_at(
                &current,
                b,
                &next,
                samples_per_step,
                bit_depth as u32,
                &json_input,
            ),
            None => audio_parser::serialize_samples_at(
                &current,
                &next,
                samples_per_step,
                bit_depth as u32,
                &json_input,
            ),
        }

        let cfg = RunConfig {
            circuit_filepath: &s.r1cs,
            witness_gen_filepath: &s.wasm,
            json_input_filepath: &json_input,
            skip_compressed: false,
            verbose: false,
            progress: true,
            multi_input: s.spec.multi_input,
            initial_state: Vec::new(),
            state_width: s.spec.state_width,
        };
        let meta = CaseMeta {
            circuit_name: s.circuit_name.clone(),
            edit: s.spec.name.to_string(),
            audio_input: audio.clone(),
            bit_depth,
            batch_size,
            comp_size,
            total_samples: padded_samples,
            repetition: 0,
        };

        let t = Timer::start();
        let (metrics, artifacts) =
            circuit::run_proof_keep(&cfg, &meta).map_err(|e| format!("stage {}: {e}", i + 1))?;
        let _ = std::fs::remove_file(&json_input);
        let artifacts = artifacts.ok_or("no compressed proof was produced")?;

        // Cross-check against the circuit before writing anything: a
        // mismatch here is a Rust/circuit bug, not a forgery, and must not
        // be reported to the user as a failed chain.
        let kind = BatchHashKind::for_circuit(&s.circuit_name);
        let recomputed = audio_parser::hex_stream_commitment::<Fq>(
            &current,
            bit_depth as u32,
            samples_per_step,
            kind,
        );
        if recomputed != metrics.final_hash_original {
            return Err(format!(
                "stage {}: commitment mismatch: circuit {} vs recomputed {recomputed}",
                i + 1,
                metrics.final_hash_original
            ));
        }

        // The link, checked as it is formed rather than only at verify
        // time, so a break is attributed to the stage that caused it.
        if let Some(prev) = &prev_h_edit {
            if prev != &metrics.final_hash_original {
                return Err(format!(
                    "stage {}: the link is broken: stage {} produced {prev}, this stage \
                     consumed {}",
                    i + 1,
                    i,
                    metrics.final_hash_original
                ));
            }
            println!("  link to stage {} OK", i);
        }

        let h_b = match (&stream_b, metrics.final_hash_b.as_deref()) {
            (Some(b), Some(circuit_b)) => {
                let r = audio_parser::hex_stream_commitment::<Fq>(
                    b,
                    bit_depth as u32,
                    samples_per_step,
                    kind,
                );
                if r != circuit_b {
                    return Err(format!(
                        "stage {}: stream-B commitment mismatch: circuit {circuit_b} vs \
                         recomputed {r}",
                        i + 1
                    ));
                }
                Some(r)
            }
            (Some(_), None) => {
                return Err(format!(
                    "stage {}: {} folds two streams but the proof carries no stream-B \
                     commitment",
                    i + 1,
                    s.spec.name
                ))
            }
            _ => None,
        };

        let proof_path = format!(
            "{out_dir}/{chain_stem}.chain.s{}_{}.proof.json",
            i + 1,
            s.spec.name
        );
        bundle::write(
            &proof_path,
            &ProofBundle {
                kind: ProofBundle::KIND.to_string(),
                circuit_name: s.circuit_name.clone(),
                edit: s.spec.name.to_string(),
                bit_depth,
                batch_size,
                comp_size,
                // Every stage folds the same padded partition, so each
                // stage's own N is the chain's N. A terminal crop is the
                // identity on its window here, the window being the whole
                // proven stream.
                samples: n_samples,
                padded_samples,
                iterations: metrics.iterations,
                z0: vec![0u64; s.spec.state_width],
                h_orig: metrics.final_hash_original.clone(),
                h_edit: metrics.final_hash_edited.clone(),
                h_b,
                len_a: None,
                proof: serde_json::from_slice(&artifacts.proof_json)
                    .map_err(|e| e.to_string())?,
            },
        )?;
        println!(
            "  stage proof written to {proof_path} ({} KB) in {}",
            artifacts.proof_json.len() / 1024,
            t.elapsed()
        );

        if h_orig_first.is_none() {
            h_orig_first = Some(metrics.final_hash_original.clone());
        }
        prev_h_edit = Some(metrics.final_hash_edited.clone());
        stage_paths.push(proof_path);
        stage_edits.push(s.spec.name.to_string());

        published_n = n_samples;
        published = next.clone();
        current = next;
    }

    let (rate, channels) = audio_parser::read_rate_and_channels(&audio);
    let edited_path = format!("{out_dir}/{chain_stem}_chain.wav");
    audio_parser::write_file_at(
        &edited_path,
        &published[..published_n],
        rate,
        bit_depth as u32,
        if channels == 1 {
            audio_parser::WaveType::Mono
        } else {
            audio_parser::WaveType::Stereo
        },
    );

    let chain_path = format!("{out_dir}/{chain_stem}.chain.json");
    bundle::write(
        &chain_path,
        &ChainBundle {
            kind: ChainBundle::KIND.to_string(),
            stages: stage_paths.clone(),
            edits: stage_edits.clone(),
            h_orig: h_orig_first.clone().ok_or("no stages were proven")?,
            h_edit: prev_h_edit.clone().ok_or("no stages were proven")?,
            samples: n_samples,
            samples_out: published_n,
            source_file: audio.clone(),
        },
    )?;

    println!();
    println!("chain proved in {} ({} stages)", t_all.elapsed(), stages.len());
    println!("edited audio written to {edited_path}");
    println!("chain bundle written to {chain_path}");
    println!();
    println!("Send the edited audio, the chain bundle, its stage proofs, and the");
    println!("signature from option A over the SOURCE, to whoever verifies (option E).");
    Ok(())
}

/// One checked property of a chain.
struct Check {
    name: String,
    ok: bool,
    detail: String,
    secs: f64,
}

pub fn verify() -> Result<(), String> {
    println!();
    println!("--- E) verify a chain bundle ---");

    let chain_path = prompt::existing_path("chain bundle path: ").ok_or("cancelled")?;
    let chain: ChainBundle = bundle::read(&chain_path)?;
    if chain.kind != ChainBundle::KIND {
        return Err(format!("{chain_path} is not a {}", ChainBundle::KIND));
    }
    let sig_path = prompt::existing_path("source signature path: ").ok_or("cancelled")?;
    let sig: SignatureBundle = bundle::read(&sig_path)?;
    if sig.kind != SignatureBundle::KIND {
        return Err(format!("{sig_path} is not a {}", SignatureBundle::KIND));
    }
    let audio_path = prompt::existing_path("edited audio path: ").ok_or("cancelled")?;

    println!();
    println!("chain of {} stages: {}", chain.edits.len(), chain.edits.join(" -> "));

    // Stage paths are resolved relative to the chain bundle, so a moved
    // bundle directory still verifies.
    let base = std::path::Path::new(&chain_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let resolve = |p: &str| -> String {
        let path = std::path::Path::new(p);
        if path.is_absolute() || path.exists() {
            p.to_string()
        } else {
            base.join(path).to_string_lossy().to_string()
        }
    };

    let mut stages: Vec<ProofBundle> = Vec::new();
    for p in &chain.stages {
        let rp = resolve(p);
        let b: ProofBundle = bundle::read(&rp)?;
        if b.kind != ProofBundle::KIND {
            return Err(format!("{rp} is not a {}", ProofBundle::KIND));
        }
        stages.push(b);
    }
    if stages.is_empty() {
        return Err("the chain bundle names no stages".to_string());
    }

    let mut checks: Vec<Check> = Vec::new();

    // Every stage must fold the same partition. A commitment is a fold
    // over a partition, so stages under different ones could not link
    // even if the hex happened to match.
    let t = Timer::start();
    let bd = stages[0].bit_depth;
    let bs = stages[0].batch_size;
    let uniform = stages.iter().all(|s| s.bit_depth == bd && s.batch_size == bs);
    checks.push(Check {
        name: "one partition".to_string(),
        ok: uniform,
        detail: if uniform {
            format!("every stage folds bd={bd}/B={bs}")
        } else {
            "the stages do not all use the same bit depth and batch size, so their \
             commitments are not comparable"
                .to_string()
        },
        secs: t.secs(),
    });

    // The manifest's endpoints must be the stages', or the bundle claims
    // a chain other than the one its proofs show.
    let t = Timer::start();
    let ends_ok = chain.h_orig == stages[0].h_orig
        && chain.h_edit == stages[stages.len() - 1].h_edit;
    checks.push(Check {
        name: "manifest ends".to_string(),
        ok: ends_ok,
        detail: if ends_ok {
            "the bundle's h_orig and h_edit are the first and last stages'".to_string()
        } else {
            "the bundle's endpoints are not its stages' endpoints".to_string()
        },
        secs: t.secs(),
    });

    // THE chain check. Without it the stages are unrelated proofs about
    // streams nobody has claimed are connected.
    let t = Timer::start();
    let mut broken: Option<usize> = None;
    for i in 0..stages.len().saturating_sub(1) {
        if stages[i].h_edit != stages[i + 1].h_orig {
            broken = Some(i);
            break;
        }
    }
    checks.push(Check {
        name: "stage links".to_string(),
        ok: broken.is_none(),
        detail: match broken {
            None if stages.len() == 1 => "a single stage has no links to check".to_string(),
            None => format!("{} links hold: each stage consumed what the previous produced", stages.len() - 1),
            Some(i) => format!(
                "broken between stage {} and stage {}: stage {} produced {}, stage {} \
                 consumed {}",
                i + 1,
                i + 2,
                i + 1,
                stages[i].h_edit,
                i + 2,
                stages[i + 1].h_orig
            ),
        },
        secs: t.secs(),
    });

    // The signature covers the stream entering stage 0.
    let t = Timer::start();
    let sig_matches = sig.h_orig == chain.h_orig;
    let step = bs * stages[0].comp_size;
    checks.push(Check {
        name: "source bound".to_string(),
        ok: sig_matches && sig.samples == chain.samples,
        detail: if sig_matches && sig.samples == chain.samples {
            format!("the signed source is the chain's input (N={})", chain.samples)
        } else if sig_matches {
            format!(
                "same h_orig but different lengths: signature says {} samples, chain \
                 says {}",
                sig.samples, chain.samples
            )
        } else {
            format!(
                "the signature covers {} ({} steps), the chain starts from {} ({} \
                 steps). Only the FIRST stage's input can match a signature over the \
                 source.",
                sig.h_orig,
                folds_for(sig.samples, step),
                chain.h_orig,
                folds_for(chain.samples, step)
            )
        },
        secs: t.secs(),
    });

    // Each stage's proof, verified against a key derived from its own
    // circuit. A vk shipped with a proof proves nothing.
    let mut vk_secs = 0.0;
    for (i, s) in stages.iter().enumerate() {
        let r1cs = format!("artifacts/{}.r1cs", s.circuit_name);
        if !std::path::Path::new(&r1cs).exists() {
            return Err(format!(
                "no compiled circuit for {} ({r1cs} missing). A verifier must hold the \
                 circuit, since the circuit is the statement.",
                s.circuit_name
            ));
        }
        println!();
        println!(
            "stage {}/{}: deriving the verifier key from {}...",
            i + 1,
            stages.len(),
            s.circuit_name
        );
        let t = Timer::start();
        let vk = circuit::derive_vk(&r1cs).map_err(|e| e.to_string())?;
        vk_secs += t.secs();
        println!("  done in {}", t.elapsed());

        let proof_json = serde_json::to_vec(&s.proof).map_err(|e| e.to_string())?;
        let t = Timer::start();
        let terminal = circuit::verify_compressed(&proof_json, &vk, s.iterations, &s.z0);
        let secs = t.secs();

        match terminal {
            Ok(state) => {
                // The proof verifying is not enough: its terminal state
                // must be the commitments the bundle names, or the stage's
                // h_edit - the value the next link rests on - is unrelated
                // to what was proven.
                let claimed = state.first().map(|v| v == &s.h_orig).unwrap_or(false)
                    && state.get(1).map(|v| v == &s.h_edit).unwrap_or(false);
                checks.push(Check {
                    name: format!("stage {} {}", i + 1, s.edit),
                    ok: claimed,
                    detail: if claimed {
                        format!("verified, {} steps, commitments match", s.iterations)
                    } else {
                        "verified, but the bundle's commitments are not the proof's \
                         terminal state"
                            .to_string()
                    },
                    secs,
                });
            }
            Err(e) => checks.push(Check {
                name: format!("stage {} {}", i + 1, s.edit),
                ok: false,
                detail: e.to_string(),
                secs,
            }),
        }
    }

    // The published audio must be what the last stage produced.
    let t = Timer::start();
    let (samples, _bits) = audio_parser::read_samples_signed(&audio_path);
    let length_ok = samples.len() == chain.samples_out;
    let recomputed = if !length_ok || samples.is_empty() {
        None
    } else {
        Some(audio_parser::hex_stream_commitment::<Fq>(
            &samples,
            bd as u32,
            step,
            BatchHashKind::for_circuit(&stages[stages.len() - 1].circuit_name),
        ))
    };
    let audio_ok = recomputed.as_deref() == Some(chain.h_edit.as_str());
    checks.push(Check {
        name: "published audio".to_string(),
        ok: audio_ok,
        detail: match &recomputed {
            Some(_) if audio_ok => format!(
                "recomputed h_edit matches the last stage ({} samples)",
                chain.samples_out
            ),
            Some(h) => format!("recomputed {h}, expected {}", chain.h_edit),
            None if !length_ok => format!(
                "the file has {} samples, the chain's output is {}",
                samples.len(),
                chain.samples_out
            ),
            None => "the file has no samples".to_string(),
        },
        secs: t.secs(),
    });

    // The signature itself, over the chain's input commitment.
    use p256::pkcs8::DecodePublicKey;
    let t = Timer::start();
    let sig_ok = match p256::ecdsa::VerifyingKey::from_public_key_pem(&sig.public_key_pem) {
        Ok(vk) => match hex::decode(&sig.signature_der_hex)
            .ok()
            .and_then(|d| p256::ecdsa::Signature::from_der(&d).ok())
        {
            Some(s) => signer::verify_hash(
                &protocol::signed_digest(&sig.h_orig, sig.samples),
                &s,
                &vk,
            ),
            None => false,
        },
        Err(_) => false,
    };
    checks.push(Check {
        name: "signature".to_string(),
        ok: sig_ok,
        detail: if sig_ok {
            format!("valid over (h_orig, N={}), using the key in the bundle", sig.samples)
        } else {
            "does not verify".to_string()
        },
        secs: t.secs(),
    });

    println!();
    for c in &checks {
        println!(
            "{:<24} {}  {:>7}   {}",
            c.name,
            if c.ok { "OK  " } else { "FAIL" },
            fmt_secs(c.secs),
            c.detail
        );
    }
    let checks_secs: f64 = checks.iter().map(|c| c.secs).sum();
    println!(
        "{:<24} {:4}  {:>7}   {}",
        "verifier keys", "", fmt_secs(vk_secs), "derived from the circuits"
    );
    println!("{:<24} {:4}  {:>7}", "total", "", fmt_secs(checks_secs + vk_secs));

    let all = checks.iter().all(|c| c.ok);
    println!();
    if all {
        println!(
            "VERDICT: the published audio is provably the result of applying {} to an",
            chain.edits.join(", then ")
        );
        println!("original committed as h_orig and signed by the key in this bundle.");
        println!();
        println!("Each stage is proven separately and the stages are bound by their");
        println!("commitments; no intermediate stream was revealed.");
        println!();
        println!("That key is NOT a trust anchor: a forger ships their own key and");
        println!("their own signature and every check above still passes. Real");
        println!("verification compares the key against an out-of-band source.");
        if chain.edits.iter().any(|e| e == "crop") {
            println!("Note: a crop stage does not bind its window to a larger original.");
        }
        if chain.edits.iter().any(|e| e == "merge") {
            println!("Note: the merge stage's source B is not bound to a signed original here.");
        }
    } else {
        println!("VERDICT: FAILED - see the failing check above.");
    }
    Ok(())
}
