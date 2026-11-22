//! Option A vs Option B: prove the same composite edit both ways and
//! report the difference.
//!
//! Both halves run on the same input, the same bit depth and the same
//! batch size, so the only variable is the composition strategy:
//! sequential stages linked by commitment equality (`src/chain.rs`) or one
//! fused step circuit (`src/fused.rs`).
//!
//! cargo run --release --bin compare_chain -- \
//!     --stages gain_k2,atten_m1,dist_d100 --bd 8 --B 10 --n 1240

use nova_snark::traits::Engine;

use zk_audio::{
    audio_parser,
    bench::config::comp_size_for,
    chain::{self, ChainConfig, Stage},
    fused,
};

type Fq = <nova_snark::provider::PallasEngine as Engine>::Scalar;

const USAGE: &str = "\
Option A (fused) vs Option B (sequential chain)

USAGE:
    cargo run --release --bin compare_chain -- --stages <list> [OPTIONS]

REQUIRED:
    --stages <list>     Comma-separated edits in application order,
                        e.g. gain_k2,atten_m1,dist_d100. Every stage must
                        be fusable (pointwise): crop, concat and merge
                        change length or arity and cannot be fused.

OPTIONS:
    --bd <n>            Bit depth (8, 16, 24)            [default: 8]
    --B <n>             Batch size, packed elements/fold  [default: 10]
    --n <n>             Samples to prove. Rounded up to a fold boundary.
                        [default: one fold]
    --input <path>      WAV input. Omitted means a deterministic ramp,
                        so the comparison needs no fixture.
    --period <n>        LFO period in samples for a tremolo stage
                        [default: 8820]
    --skip-compressed   Fold only, no CompressedSNARK. Isolates the
                        folding difference from the compression toll.
    --only <a|b>        Run one side (its own number is still useful).
    --csv <path>        Append one row per side to this CSV, creating it
                        with a header. The durable artefact of a sweep.
    --label <text>      Free-text tag recorded in the CSV rows.
    -h, --help          Show this help

The fused side needs a compiled artefact per chain spec. If it is
missing, the run says which command generates and compiles it.
";

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

/// A deterministic ramp covering the representable range, so a comparison
/// run needs no WAV fixture. Full-scale on purpose: it exercises every
/// stage's clamp rather than a quiet middle band.
fn ramp(n: usize, bit_depth: usize) -> Vec<i32> {
    let half = 1i32 << (bit_depth - 1);
    (0..n)
        .map(|i| (i as i32 % (2 * half)) - half)
        .collect()
}

/// One row per side, so a sweep's rows can be grouped by (label, option).
const CSV_HEADER: &str = "label,option,stages,bit_depth,batch_size,\
requested_samples,padded_samples,folds,proofs,constraints_per_step,\
stage_constraints,public_params_ms,fold_ms,compress_ms,total_prove_ms,\
proof_size_bytes";

fn csv_append(path: &str, row: &str) {
    use std::io::Write;
    let exists = std::path::Path::new(path).exists();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap_or_else(|e| panic!("opening {path}: {e}"));
    if !exists {
        writeln!(f, "{CSV_HEADER}").expect("header");
    }
    writeln!(f, "{row}").expect("row");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return;
    }

    let stages: Vec<String> = match arg(&args, "--stages") {
        Some(s) => s.split(',').map(|s| s.trim().to_string()).collect(),
        None => {
            eprintln!("error: --stages is required\n");
            print!("{USAGE}");
            std::process::exit(2);
        }
    };

    let bit_depth: usize = arg(&args, "--bd").unwrap_or("8").parse().expect("--bd");
    let batch_size: usize = arg(&args, "--B").unwrap_or("10").parse().expect("--B");
    let period: usize = arg(&args, "--period").unwrap_or("8820").parse().expect("--period");
    let skip_compressed = args.iter().any(|a| a == "--skip-compressed");
    let only = arg(&args, "--only").map(|s| s.to_lowercase());
    let csv = arg(&args, "--csv").map(|s| s.to_string());
    let label = arg(&args, "--label").unwrap_or("").to_string();

    let comp_size = comp_size_for(bit_depth);
    let samples_per_step = batch_size * comp_size;

    let requested: usize = arg(&args, "--n")
        .map(|s| s.parse().expect("--n"))
        .unwrap_or(samples_per_step);

    let original = match arg(&args, "--input") {
        Some(path) => {
            // Signed at the file's own depth: 8-bit WAV stores samples
            // unsigned on disk, so the plain reader would be off by 128.
            let (samples, bits) = audio_parser::read_samples_signed(path);
            if bits as usize != bit_depth {
                eprintln!(
                    "error: {path} is {bits}-bit but --bd is {bit_depth}; the proving \
                     depth must match the file"
                );
                std::process::exit(2);
            }
            samples.into_iter().take(requested).collect::<Vec<i32>>()
        }
        None => ramp(requested, bit_depth),
    };

    // Both sides pad identically, so report the padded length: that is
    // what each one actually folds over.
    let padded_len = audio_parser::pad_to_step(&original, samples_per_step).len();
    let folds = padded_len / samples_per_step;

    println!("Option A (fused) vs Option B (chain)");
    println!("  stages        {}", stages.join(" -> "));
    println!("  bit depth     {bit_depth}");
    println!("  batch size    {batch_size}  ({samples_per_step} samples/fold)");
    println!(
        "  samples       {} requested, {padded_len} padded, {folds} folds",
        original.len()
    );
    if skip_compressed {
        println!("  compression   skipped (folding only)");
    }
    println!();

    // Reject an unfusable stage before paying for the chain side: a
    // comparison with only one half measured is not the deliverable.
    for s in &stages {
        if !fused::is_fusable(s) {
            eprintln!(
                "error: stage '{s}' cannot be fused, so there is nothing to compare.\n\
                 Fusable stages: {}",
                fused::FUSABLE.join(", ")
            );
            std::process::exit(2);
        }
    }

    // A fresh ephemeral key: the comparison measures proving, and a
    // persisted signer would add a setup step neither side needs.
    let sk = p256::ecdsa::SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
    let vk = p256::ecdsa::VerifyingKey::from(&sk);

    let cfg = ChainConfig {
        bit_depth,
        batch_size,
        period,
        serialized_dir: "serialized",
        tag: "cmp",
        skip_compressed,
        progress: false,
        audio_input: arg(&args, "--input").unwrap_or("(ramp)").to_string(),
    };

    let run_b = only.as_deref() != Some("a");
    let run_a = only.as_deref() != Some("b");

    let mut b_result = None;
    if run_b {
        println!("--- Option B: {} sequential proofs ---", stages.len());
        let stage_list: Vec<Stage> = stages.iter().map(|e| Stage::new(e)).collect();
        let prov = chain::prove_chain::<Fq>(&original, &stage_list, &cfg, &sk)
            .unwrap_or_else(|e| panic!("chain: {e}"));

        let edited = fused::apply_stages(
            &audio_parser::pad_to_step(&original, samples_per_step),
            &chain::validate(&stage_list, &cfg).expect("validate"),
            &zk_audio::bench::config::EditCtx {
                bit_depth,
                batch_size,
                start_index: 0,
                comp_size,
                period,
            },
        );
        chain::verify_chain::<Fq>(
            &edited,
            bit_depth as u32,
            samples_per_step,
            &prov,
            &vk,
        )
        .unwrap_or_else(|e| panic!("chain verify: {e}"));

        for (i, s) in prov.stages.iter().enumerate() {
            println!(
                "  stage {} {:12} {:>9} constraints  fold {:>9.0} ms  compress {:>8.0} ms",
                i + 1,
                s.edit,
                s.metrics.constraints_primary,
                s.metrics.recursive_prove_ms,
                s.metrics.compressed_setup_ms + s.metrics.compressed_prove_ms,
            );
        }
        println!("  verified: {} proofs, {} links", prov.stages.len(), prov.stages.len() - 1);
        println!();

        if let Some(path) = &csv {
            let pp: f64 = prov.stages.iter().map(|s| s.metrics.public_params_ms).sum();
            let size: usize = prov.stages.iter().map(|s| s.metrics.proof_size_bytes).sum();
            csv_append(
                path,
                &format!(
                    "{label},B,{},{bit_depth},{batch_size},{},{padded_len},{folds},{},{},\
{},{pp:.3},{:.3},{:.3},{:.3},{size}",
                    stages.join("+"),
                    original.len(),
                    prov.stages.len(),
                    prov.total_constraints(),
                    prov
                        .stages
                        .iter()
                        .map(|s| s.metrics.constraints_primary.to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                    prov.total_recursive_prove_ms(),
                    prov.total_compressed_prove_ms(),
                    prov.total_prove_ms(),
                ),
            );
        }
        b_result = Some(prov);
    }

    let mut a_result = None;
    if run_a {
        println!("--- Option A: 1 fused proof ---");
        match fused::prove_fused::<Fq>(&original, &stages, &cfg, &sk) {
            Ok(prov) => {
                let edited = fused::apply_stages(
                    &audio_parser::pad_to_step(&original, samples_per_step),
                    &fused::validate(&stages, &cfg).expect("validate").0,
                    &zk_audio::bench::config::EditCtx {
                        bit_depth,
                        batch_size,
                        start_index: 0,
                        comp_size,
                        period,
                    },
                );
                // What a creator publishes is the edited stream trimmed to
                // the input length; verify_fused restores the pad itself.
                let published: Vec<i32> = edited.iter().copied().take(original.len()).collect();
                fused::verify_fused::<Fq>(
                    &published,
                    bit_depth as u32,
                    samples_per_step,
                    &prov,
                    &vk,
                )
                .unwrap_or_else(|e| panic!("fused verify: {e}"));

                println!(
                    "  {:12} {:>9} constraints  fold {:>9.0} ms  compress {:>8.0} ms",
                    prov.circuit_name.split("_b").next().unwrap_or("fused"),
                    prov.metrics.constraints_primary,
                    prov.metrics.recursive_prove_ms,
                    prov.total_compressed_prove_ms(),
                );
                println!("  verified: 1 proof, 0 links");
                println!();

                if let Some(path) = &csv {
                    csv_append(
                        path,
                        &format!(
                            "{label},A,{},{bit_depth},{batch_size},{},{padded_len},{folds},1,{},\
{},{:.3},{:.3},{:.3},{:.3},{}",
                            stages.join("+"),
                            original.len(),
                            prov.total_constraints(),
                            prov.total_constraints(),
                            prov.metrics.public_params_ms,
                            prov.total_recursive_prove_ms(),
                            prov.total_compressed_prove_ms(),
                            prov.total_prove_ms(),
                            prov.metrics.proof_size_bytes,
                        ),
                    );
                }
                a_result = Some(prov);
            }
            Err(e) => {
                println!("  unavailable: {e}");
                println!();
            }
        }
    }

    if let (Some(b), Some(a)) = (&b_result, &a_result) {
        let pct = |x: f64, y: f64| {
            if y == 0.0 {
                0.0
            } else {
                100.0 * (1.0 - x / y)
            }
        };

        println!("--- A vs B ---");
        println!(
            "  {:22} {:>12} {:>12} {:>10}",
            "", "B (chain)", "A (fused)", "saving"
        );
        println!(
            "  {:22} {:>12} {:>12} {:>9.1}%",
            "constraints/step",
            b.total_constraints(),
            a.total_constraints(),
            pct(a.total_constraints() as f64, b.total_constraints() as f64),
        );
        println!(
            "  {:22} {:>12.0} {:>12.0} {:>9.1}%",
            "folding ms",
            b.total_recursive_prove_ms(),
            a.total_recursive_prove_ms(),
            pct(a.total_recursive_prove_ms(), b.total_recursive_prove_ms()),
        );
        println!(
            "  {:22} {:>12.0} {:>12.0} {:>9.1}%",
            "compression ms",
            b.total_compressed_prove_ms(),
            a.total_compressed_prove_ms(),
            pct(a.total_compressed_prove_ms(), b.total_compressed_prove_ms()),
        );
        println!(
            "  {:22} {:>12.0} {:>12.0} {:>9.1}%",
            "total prove ms",
            b.total_prove_ms(),
            a.total_prove_ms(),
            pct(a.total_prove_ms(), b.total_prove_ms()),
        );
        println!();
        println!(
            "  proofs to verify       {:>12} {:>12}",
            b.stages.len(),
            1
        );
        println!(
            "  intermediate commitments {:>10} {:>12}",
            b.stages.len().saturating_sub(1),
            0
        );
        println!();
        println!(
            "  A commits to the input and the final output only. The {} \
             intermediate\n  commitment(s) B publishes are what pay for its \
             stage-by-stage auditability.",
            b.stages.len().saturating_sub(1)
        );
    }
}
