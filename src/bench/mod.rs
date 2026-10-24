//! Benchmark suite: sweeps one circuit family across B and N, recording
//! structured results.

pub mod config;
pub mod memory;
pub mod report;

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::audio_parser;
use crate::bench::config::{comp_size_for, generate_cases, Case, SuiteConfig, SUPPORTED_BIT_DEPTHS};
use crate::bench::report::{print_matrix, EnvInfo, Reporter, ResultRow};
use crate::circuit::{self, CaseMeta, MultiplierInput, RunConfig};

pub fn run_suite(cfg: &SuiteConfig) -> Result<(), String> {
    if cfg.max_n == 0 && cfg.durations.is_empty() && cfg.n_list.is_empty() {
        return Err(
            "--max-n, --durations or --n-list is required (they drive total runtime)".to_string(),
        );
    }
    if !SUPPORTED_BIT_DEPTHS.contains(&cfg.bit_depth) {
        return Err(format!(
            "bit_depth {} has no compiled step circuit; supported: {:?}",
            cfg.bit_depth, SUPPORTED_BIT_DEPTHS
        ));
    }
    if cfg.repeat == 0 {
        return Err("--repeat must be at least 1".to_string());
    }

    let edit = config::find_edit(&cfg.edit).ok_or_else(|| {
        format!(
            "unknown edit '{}'; available: {:?}",
            cfg.edit,
            config::registry().iter().map(|e| e.name).collect::<Vec<_>>()
        )
    })?;

    if !edit.bit_depths.contains(&cfg.bit_depth) {
        return Err(format!(
            "edit '{}' has no circuit at bit_depth {}; available: {:?}",
            cfg.edit, cfg.bit_depth, edit.bit_depths
        ));
    }

    if !Path::new(&cfg.audio_input).exists() {
        return Err(format!("audio input not found: {}", cfg.audio_input));
    }

    println!("Reading audio: {}", cfg.audio_input);
    // Depth-aware read: 8-bit WAV stores samples unsigned, so reading it
    // as i16 would yield 0..=255 instead of -128..=127.
    let (original_wide, file_bits) = audio_parser::read_samples_signed(&cfg.audio_input);
    if file_bits as usize != cfg.bit_depth {
        return Err(format!(
            "--bit-depth {} but {} is a {}-bit file; pass --bit-depth {} or a matching file",
            cfg.bit_depth, cfg.audio_input, file_bits, file_bits
        ));
    }
    let original: Vec<i16> = original_wide.iter().map(|&s| s as i16).collect();
    let available = original.len();
    println!("Available samples: {}", available);

    if !cfg.durations.is_empty() && cfg.n_list.is_empty() {
        let (ch, sr) = audio_meta(&cfg.audio_input);
        if ch != 0 && (ch as usize != cfg.channels || sr as usize != cfg.sample_rate) {
            // A duration is only meaningful against the real format; silently
            // proving the wrong number of samples would misreport the x-axis.
            return Err(format!(
                "--durations assumes {} Hz / {} ch but {} is {} Hz / {} ch; \
                 pass --sample-rate {} --channels {} or a matching file",
                cfg.sample_rate, cfg.channels, cfg.audio_input, sr, ch, sr, ch
            ));
        }
    }

    let comp = comp_size_for(cfg.bit_depth);
    let cases = generate_cases(cfg, available, edit.circuit_prefix, edit.factor);
    print_matrix(&cases);

    if cfg.dry_run {
        println!("Dry run — nothing proven.");
        return Ok(());
    }

    let runnable = cases.iter().filter(|c| c.is_runnable()).count();
    if runnable == 0 {
        return Err("no runnable cases; see skip reasons above".to_string());
    }

    let run_id = run_id();
    let mut reporter =
        Reporter::new(&cfg.out_dir, &run_id).map_err(|e| format!("cannot open output: {}", e))?;

    let (channels, sample_rate) = audio_meta(&cfg.audio_input);
    let env = EnvInfo {
        git_sha: sh("git", &["rev-parse", "HEAD"]),
        git_dirty: !sh("git", &["status", "--porcelain"]).is_empty(),
        rustc: sh("rustc", &["-V"]),
        circom: sh("circom", &["--version"]),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu: sh("sysctl", &["-n", "machdep.cpu.brand_string"]),
        cores: sh("sysctl", &["-n", "hw.ncpu"]),
        audio_samples: available,
        audio_channels: channels,
        audio_sample_rate: sample_rate,
        started_at: run_id.clone(),
    };
    reporter
        .write_manifest(cfg, &env, cases.len(), runnable)
        .map_err(|e| format!("cannot write manifest: {}", e))?;

    println!("Results: {}", reporter.dir.display());
    if cfg!(debug_assertions) {
        println!("WARNING: debug build — timings are not representative.");
    }

    // Record skips first so the results file describes the whole matrix.
    for c in &cases {
        if let Err(reason) = &c.status {
            reporter
                .record(ResultRow::Skipped {
                    circuit_name: c.circuit_name.clone(),
                    bit_depth: c.bit_depth,
                    batch_size: c.batch_size,
                    total_samples: c.total_samples,
                    reason: reason.clone(),
                })
                .map_err(|e| format!("cannot write result: {}", e))?;
        }
    }

    let total_runs = runnable * cfg.repeat;
    let mut done = 0;

    for c in cases.iter().filter(|c| c.is_runnable()) {
        let slice = &original[0..c.total_samples];
        let edited = (edit.apply)(slice, cfg.bit_depth);

        if let Err(e) = ensure_input(c, slice, &edited, comp) {
            done += cfg.repeat;
            eprintln!("  input preparation failed: {}", e);
            reporter
                .record(ResultRow::Failed {
                    circuit_name: c.circuit_name.clone(),
                    bit_depth: c.bit_depth,
                    batch_size: c.batch_size,
                    total_samples: c.total_samples,
                    repetition: 0,
                    error: e,
                })
                .map_err(|e| format!("cannot write result: {}", e))?;
            continue;
        }

        for rep in 1..=cfg.repeat {
            done += 1;
            println!(
                "[{}/{}] B={} N={} iters={} rep={}/{}",
                done, total_runs, c.batch_size, c.total_samples, c.iterations, rep, cfg.repeat
            );

            let meta = CaseMeta {
                circuit_name: c.circuit_name.clone(),
                edit: cfg.edit.clone(),
                audio_input: cfg.audio_input.clone(),
                bit_depth: c.bit_depth,
                batch_size: c.batch_size,
                comp_size: c.comp_size,
                total_samples: c.total_samples,
                repetition: rep,
            };

            let run_cfg = RunConfig {
                circuit_filepath: &c.r1cs_path,
                witness_gen_filepath: &c.wasm_path,
                json_input_filepath: &c.input_path,
                skip_compressed: cfg.skip_compressed,
                verbose: false,
            };

            match circuit::run_proof(&run_cfg, &meta) {
                Ok(m) => {
                    println!(
                        "      prove {:.2}s  verify {:.0}ms  compressed {:.2}s  proof {} B{}",
                        m.recursive_prove_ms / 1000.0,
                        m.recursive_verify_ms,
                        m.compressed_prove_ms / 1000.0,
                        m.proof_size_bytes,
                        match m.peak_rss_mb() {
                            Some(mb) => format!("  peak {:.0} MB", mb),
                            None => String::new(),
                        }
                    );
                    reporter
                        .record(ResultRow::Ok(Box::new(m)))
                        .map_err(|e| format!("cannot write result: {}", e))?;
                }
                Err(e) => {
                    eprintln!("      FAILED: {}", e);
                    reporter
                        .record(ResultRow::Failed {
                            circuit_name: c.circuit_name.clone(),
                            bit_depth: c.bit_depth,
                            batch_size: c.batch_size,
                            total_samples: c.total_samples,
                            repetition: rep,
                            error: e.to_string(),
                        })
                        .map_err(|e| format!("cannot write result: {}", e))?;
                }
            }
        }

        if !cfg.cache_inputs {
            let _ = std::fs::remove_file(&c.input_path);
        }
    }

    reporter
        .write_summary(cfg)
        .map_err(|e| format!("cannot write summary: {}", e))?;

    println!("\nDone. Summary: {}", reporter.dir.join("summary.md").display());
    Ok(())
}

/// Generates the serialized input if absent, reuses it when its shape already
/// matches. The post-write check is what catches a file whose name disagrees
/// with its contents.
fn ensure_input(c: &Case, original: &[i16], edited: &[i16], comp: usize) -> Result<(), String> {
    if Path::new(&c.input_path).exists() && validate_shape(c, comp).is_ok() {
        return Ok(());
    }

    if let Some(parent) = Path::new(&c.input_path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    // Depth-generic packer. At bit_depth=16 it is byte-identical to the
    // legacy `serialize_samples` (pinned by a test in audio_parser), so
    // existing serialized inputs and commitment hashes stay comparable.
    let orig_wide: Vec<i32> = original.iter().map(|&s| s as i32).collect();
    let edit_wide: Vec<i32> = edited.iter().map(|&s| s as i32).collect();
    audio_parser::serialize_samples_at(
        &orig_wide,
        &edit_wide,
        c.samples_per_step(),
        c.bit_depth as u32,
        &c.input_path,
    );

    validate_shape(c, comp)
}

fn validate_shape(c: &Case, comp: usize) -> Result<(), String> {
    let raw = std::fs::read_to_string(&c.input_path).map_err(|e| e.to_string())?;
    let data: MultiplierInput = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    if data.original.len() != data.edited.len() {
        return Err(format!(
            "{}: original/edited batch count differs ({} vs {})",
            c.input_path,
            data.original.len(),
            data.edited.len()
        ));
    }
    if data.original.len() != c.iterations {
        return Err(format!(
            "{}: has {} batches, expected {}",
            c.input_path,
            data.original.len(),
            c.iterations
        ));
    }
    for (i, b) in data.original.iter().enumerate() {
        if b.len() != c.batch_size {
            return Err(format!(
                "{}: batch {} has {} elements, expected B={}",
                c.input_path,
                i,
                b.len(),
                c.batch_size
            ));
        }
    }
    let total = data.original.len() * c.batch_size * comp;
    if total != c.total_samples {
        return Err(format!(
            "{}: covers {} samples, expected N={}",
            c.input_path, total, c.total_samples
        ));
    }
    Ok(())
}

fn run_id() -> String {
    // Avoids a chrono dependency: seconds since epoch is enough to order runs,
    // and the manifest carries the human-readable context.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("run-{}", secs)
}

fn sh(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn audio_meta(path: &str) -> (u16, u32) {
    match hound::WavReader::open(path) {
        Ok(r) => {
            let s = r.spec();
            (s.channels, s.sample_rate)
        }
        Err(_) => (0, 0),
    }
}
