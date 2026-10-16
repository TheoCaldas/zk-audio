//! Structured output: manifest, append-only JSONL, CSV, and a summary table.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bench::config::{Case, SkipReason, SuiteConfig};
use crate::circuit::ProofMetrics;

/// Bumped when the meaning of a column changes. v2: `compressed_setup_ms` is
/// measured separately, so `compressed_prove_ms` no longer includes setup —
/// unlike the pre-suite logs, where the two were summed.
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResultRow {
    Ok(Box<ProofMetrics>),
    Skipped {
        circuit_name: String,
        bit_depth: usize,
        batch_size: usize,
        total_samples: usize,
        #[serde(flatten)]
        reason: SkipReason,
    },
    Failed {
        circuit_name: String,
        bit_depth: usize,
        batch_size: usize,
        total_samples: usize,
        repetition: usize,
        error: String,
    },
}

pub struct Reporter {
    pub dir: PathBuf,
    jsonl: BufWriter<File>,
    csv: BufWriter<File>,
    csv_header_written: bool,
    rows: Vec<ResultRow>,
}

impl Reporter {
    pub fn new(out_root: &str, run_id: &str) -> std::io::Result<Self> {
        let dir = Path::new(out_root).join(run_id);
        fs::create_dir_all(&dir)?;

        let jsonl = BufWriter::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("results.jsonl"))?,
        );
        let csv = BufWriter::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("results.csv"))?,
        );

        Ok(Reporter {
            dir,
            jsonl,
            csv,
            csv_header_written: false,
            rows: Vec::new(),
        })
    }

    pub fn write_manifest(
        &self,
        cfg: &SuiteConfig,
        env: &EnvInfo,
        cases_total: usize,
        cases_runnable: usize,
    ) -> std::io::Result<()> {
        let manifest = json!({
            "schema_version": SCHEMA_VERSION,
            "notes": "compressed_prove_ms EXCLUDES CompressedSNARK::setup \
                      (measured as compressed_setup_ms). Logs predating \
                      schema_version 2 reported setup+prove summed, so those \
                      numbers are not directly comparable.",
            "config": {
                "max_n": cfg.max_n,
                "batch_sizes": cfg.batch_sizes,
                "bit_depth": cfg.bit_depth,
                "comp_size": crate::bench::config::comp_size_for(cfg.bit_depth),
                "audio_input": cfg.audio_input,
                "edit": cfg.edit,
                "repeat": cfg.repeat,
                "skip_compressed": cfg.skip_compressed,
                "cache_inputs": cfg.cache_inputs,
            },
            "audio": {
                "path": cfg.audio_input,
                "total_samples": env.audio_samples,
                "channels": env.audio_channels,
                "sample_rate": env.audio_sample_rate,
                "note": "samples are interleaved channels as a flat vector",
            },
            "environment": {
                "git_sha": env.git_sha,
                "git_dirty": env.git_dirty,
                "rustc": env.rustc,
                "circom": env.circom,
                "os": env.os,
                "arch": env.arch,
                "cpu": env.cpu,
                "cores": env.cores,
                "debug_build": cfg!(debug_assertions),
            },
            "cases": { "total": cases_total, "runnable": cases_runnable },
            "started_at": env.started_at,
        });

        fs::write(
            self.dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
    }

    /// Appends and flushes immediately — a crash mid-sweep must not lose
    /// completed cases.
    pub fn record(&mut self, row: ResultRow) -> std::io::Result<()> {
        writeln!(self.jsonl, "{}", serde_json::to_string(&row).unwrap())?;
        self.jsonl.flush()?;

        if !self.csv_header_written {
            writeln!(self.csv, "{}", csv_header())?;
            self.csv_header_written = true;
        }
        writeln!(self.csv, "{}", csv_row(&row))?;
        self.csv.flush()?;

        self.rows.push(row);
        Ok(())
    }

    pub fn metrics(&self) -> Vec<&ProofMetrics> {
        self.rows
            .iter()
            .filter_map(|r| match r {
                ResultRow::Ok(m) => Some(m.as_ref()),
                _ => None,
            })
            .collect()
    }

    pub fn write_summary(&self, cfg: &SuiteConfig) -> std::io::Result<()> {
        fs::write(self.dir.join("summary.md"), self.render_summary(cfg))
    }

    fn render_summary(&self, cfg: &SuiteConfig) -> String {
        let mut s = String::new();
        s.push_str("# Benchmark Summary\n\n");
        s.push_str(&format!(
            "- schema_version: {}\n- edit: `{}`\n- audio: `{}`\n- bit_depth: {}\n- repeat: {}\n\n",
            SCHEMA_VERSION, cfg.edit, cfg.audio_input, cfg.bit_depth, cfg.repeat
        ));
        s.push_str(
            "`compressed_prove_ms` excludes `CompressedSNARK::setup`; see manifest.json.\n\n",
        );

        let metrics = self.metrics();
        if metrics.is_empty() {
            s.push_str("No successful runs.\n");
            return s;
        }

        // Median across repetitions, keyed by (N, B).
        let mut grouped: BTreeMap<(usize, usize), Vec<&ProofMetrics>> = BTreeMap::new();
        for m in &metrics {
            grouped
                .entry((m.meta.total_samples, m.meta.batch_size))
                .or_default()
                .push(m);
        }

        let batch_sizes: Vec<usize> = {
            let mut v: Vec<usize> = grouped.keys().map(|k| k.1).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let ns: Vec<usize> = {
            let mut v: Vec<usize> = grouped.keys().map(|k| k.0).collect();
            v.sort_unstable();
            v.dedup();
            v
        };

        let tables: Vec<(&str, Box<dyn Fn(&[&ProofMetrics]) -> String>)> = vec![
            (
                "RecursiveSNARK prove (s)",
                Box::new(|g: &[&ProofMetrics]| {
                    fmt_med(g.iter().map(|m| m.recursive_prove_ms / 1000.0).collect())
                }),
            ),
            (
                "RecursiveSNARK verify (ms)",
                Box::new(|g: &[&ProofMetrics]| {
                    fmt_med(g.iter().map(|m| m.recursive_verify_ms).collect())
                }),
            ),
            (
                "CompressedSNARK prove (s)",
                Box::new(|g: &[&ProofMetrics]| {
                    fmt_med(g.iter().map(|m| m.compressed_prove_ms / 1000.0).collect())
                }),
            ),
            (
                "CompressedSNARK verify (ms)",
                Box::new(|g: &[&ProofMetrics]| {
                    fmt_med(g.iter().map(|m| m.compressed_verify_ms).collect())
                }),
            ),
            (
                "Proof size (bytes)",
                Box::new(|g: &[&ProofMetrics]| {
                    fmt_med(g.iter().map(|m| m.proof_size_bytes as f64).collect())
                }),
            ),
            (
                "us per sample (prove)",
                Box::new(|g: &[&ProofMetrics]| {
                    fmt_med(g.iter().map(|m| m.us_per_sample()).collect())
                }),
            ),
            (
                "ms per iteration (prove)",
                Box::new(|g: &[&ProofMetrics]| {
                    fmt_med(g.iter().map(|m| m.ms_per_iteration()).collect())
                }),
            ),
        ];

        for (title, f) in &tables {
            s.push_str(&format!("## {}\n\n", title));
            s.push_str("| N |");
            for b in &batch_sizes {
                s.push_str(&format!(" B={} |", b));
            }
            s.push_str("\n|---|");
            for _ in &batch_sizes {
                s.push_str("---|");
            }
            s.push('\n');

            for n in &ns {
                s.push_str(&format!("| {} |", n));
                for b in &batch_sizes {
                    match grouped.get(&(*n, *b)) {
                        Some(g) => s.push_str(&format!(" {} |", f(g))),
                        None => s.push_str(" — |"),
                    }
                }
                s.push('\n');
            }
            s.push('\n');
        }

        // Constraints are a property of B alone.
        s.push_str("## Circuit size (per step, primary)\n\n| B | samples/step | constraints | variables | constraints/sample |\n|---|---|---|---|---|\n");
        for b in &batch_sizes {
            if let Some(m) = metrics.iter().find(|m| m.meta.batch_size == *b) {
                s.push_str(&format!(
                    "| {} | {} | {} | {} | {:.1} |\n",
                    b,
                    m.meta.batch_size * m.meta.comp_size,
                    m.constraints_primary,
                    m.variables_primary,
                    m.constraints_per_sample()
                ));
            }
        }
        s.push('\n');

        s.push_str(&self.render_hash_crosscheck());
        s.push_str(&self.render_skips());
        s
    }

    /// Commitments are reported per (B, N) so they can be diffed against
    /// earlier runs.
    ///
    /// They are NOT comparable across different B at the same N: `BatchHasher`
    /// folds B elements into one digest per step and `Hasher(2)` chains that
    /// into the running state, so B determines the hash-tree shape. Two runs
    /// over identical samples at different B therefore commit to different
    /// values by construction. The invariant that does hold is per-cell
    /// reproducibility: a given (B, N) must always yield the same pair, across
    /// repetitions and across circuit versions proving the same relation.
    fn render_hash_crosscheck(&self) -> String {
        let mut s = String::from("## Commitments\n\n");
        s.push_str(
            "Per (B, N). Not comparable across B — the batch size sets the \
             hash-tree shape. Within a cell they must be identical across \
             repetitions; diff against an earlier run's table to check a \
             circuit change preserved the relation.\n\n",
        );

        let mut by_cell: BTreeMap<(usize, usize), Vec<(String, String)>> = BTreeMap::new();
        for m in self.metrics() {
            by_cell
                .entry((m.meta.total_samples, m.meta.batch_size))
                .or_default()
                .push((m.final_hash_original.clone(), m.final_hash_edited.clone()));
        }

        s.push_str("| N | B | original hash | edited hash | reps agree |\n|---|---|---|---|---|\n");
        for ((n, b), entries) in by_cell {
            let mut uniq: Vec<&(String, String)> = entries.iter().collect();
            uniq.sort();
            uniq.dedup();

            let verdict = if entries.len() < 2 {
                "single rep".to_string()
            } else if uniq.len() == 1 {
                "yes".to_string()
            } else {
                format!("**NO — {} distinct**", uniq.len())
            };

            s.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                n,
                b,
                short(&entries[0].0),
                short(&entries[0].1),
                verdict
            ));
        }
        s.push('\n');
        s
    }

    fn render_skips(&self) -> String {
        let skipped: Vec<&ResultRow> = self
            .rows
            .iter()
            .filter(|r| !matches!(r, ResultRow::Ok(_)))
            .collect();
        if skipped.is_empty() {
            return String::new();
        }

        let mut s = String::from("## Skipped / failed cases\n\n| B | N | reason |\n|---|---|---|\n");
        for r in skipped {
            match r {
                ResultRow::Skipped {
                    batch_size,
                    total_samples,
                    reason,
                    ..
                } => s.push_str(&format!(
                    "| {} | {} | {} |\n",
                    batch_size, total_samples, reason
                )),
                ResultRow::Failed {
                    batch_size,
                    total_samples,
                    error,
                    ..
                } => s.push_str(&format!(
                    "| {} | {} | FAILED: {} |\n",
                    batch_size, total_samples, error
                )),
                ResultRow::Ok(_) => {}
            }
        }
        s.push('\n');
        s
    }
}

fn short(h: &str) -> String {
    if h.len() > 14 {
        format!("{}…", &h[..14])
    } else {
        h.to_string()
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = v.len() / 2;
    if v.len() % 2 == 0 {
        (v[mid - 1] + v[mid]) / 2.0
    } else {
        v[mid]
    }
}

/// Median, with spread shown when repetitions disagree enough to matter.
fn fmt_med(v: Vec<f64>) -> String {
    let m = median(v.clone());
    if v.len() < 2 {
        return format!("{:.3}", m);
    }
    let lo = v.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let spread = if m.abs() > f64::EPSILON {
        (hi - lo) / m * 100.0
    } else {
        0.0
    };
    if spread >= 5.0 {
        format!("{:.3} (±{:.0}%)", m, spread / 2.0)
    } else {
        format!("{:.3}", m)
    }
}

pub fn csv_header() -> &'static str {
    "status,circuit_name,edit,audio_input,bit_depth,batch_size,comp_size,total_samples,\
repetition,iterations,constraints_primary,constraints_secondary,variables_primary,\
variables_secondary,public_params_ms,recursive_prove_ms,recursive_verify_ms,\
compressed_setup_ms,compressed_prove_ms,compressed_verify_ms,proof_size_bytes,\
us_per_sample,ms_per_iteration,final_hash_original,final_hash_edited,note"
}

pub fn csv_row(row: &ResultRow) -> String {
    match row {
        ResultRow::Ok(m) => format!(
            "ok,{},{},{},{},{},{},{},{},{},{},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{:.6},{:.3},{},{},",
            m.meta.circuit_name,
            m.meta.edit,
            m.meta.audio_input,
            m.meta.bit_depth,
            m.meta.batch_size,
            m.meta.comp_size,
            m.meta.total_samples,
            m.meta.repetition,
            m.iterations,
            m.constraints_primary,
            m.constraints_secondary,
            m.variables_primary,
            m.variables_secondary,
            m.public_params_ms,
            m.recursive_prove_ms,
            m.recursive_verify_ms,
            m.compressed_setup_ms,
            m.compressed_prove_ms,
            m.compressed_verify_ms,
            m.proof_size_bytes,
            m.us_per_sample(),
            m.ms_per_iteration(),
            m.final_hash_original,
            m.final_hash_edited,
        ),
        ResultRow::Skipped {
            circuit_name,
            bit_depth,
            batch_size,
            total_samples,
            reason,
        } => format!(
            "skipped,{},,,{},{},,{},,,,,,,,,,,,,,,,,,\"{}\"",
            circuit_name, bit_depth, batch_size, total_samples, reason
        ),
        ResultRow::Failed {
            circuit_name,
            bit_depth,
            batch_size,
            total_samples,
            repetition,
            error,
        } => format!(
            "failed,{},,,{},{},,{},{},,,,,,,,,,,,,,,,,\"{}\"",
            circuit_name,
            bit_depth,
            batch_size,
            total_samples,
            repetition,
            error.replace('"', "'")
        ),
    }
}

pub struct EnvInfo {
    pub git_sha: String,
    pub git_dirty: bool,
    pub rustc: String,
    pub circom: String,
    pub os: String,
    pub arch: String,
    pub cpu: String,
    pub cores: String,
    pub audio_samples: usize,
    pub audio_channels: u16,
    pub audio_sample_rate: u32,
    pub started_at: String,
}

/// Prints the resolved matrix, with skip reasons, before anything is proven.
pub fn print_matrix(cases: &[Case]) {
    println!("\nCase matrix ({} cells):\n", cases.len());
    println!(
        "{:>8} {:>5} {:>10} {:>8}  {}",
        "N", "B", "samples/st", "iters", "status"
    );
    println!("{}", "-".repeat(72));
    for c in cases {
        let status = match &c.status {
            Ok(()) => "run".to_string(),
            Err(r) => format!("skip: {}", r),
        };
        println!(
            "{:>8} {:>5} {:>10} {:>8}  {}",
            c.total_samples,
            c.batch_size,
            c.samples_per_step(),
            c.iterations,
            status
        );
    }
    let runnable = cases.iter().filter(|c| c.is_runnable()).count();
    println!(
        "\n{} runnable, {} skipped.\n",
        runnable,
        cases.len() - runnable
    );

    for c in cases {
        if let Err(SkipReason::MissingArtifact { compile_with, .. }) = &c.status {
            println!("  compile: {}", compile_with);
        }
    }
}
