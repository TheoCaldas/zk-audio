use std::{collections::HashMap, env::current_dir, fmt, path::Path, time::Instant};

use nova_scotia::{
    circom::reader::load_r1cs, create_public_params, create_recursive_circuit, FileLocation, F, S,
};
use nova_snark::{CompressedSNARK, PublicParams};

use crate::bench::memory::{self, PeakSampler};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
#[derive(Serialize)]
pub struct MultiplierInput {
    pub original: Vec<Vec<String>>,
    pub edited: Vec<Vec<String>>,
    /// Second input stream, for multi-input edits such as merge. Absent
    /// for the single-input edits, so their serialized JSON is unchanged
    /// and previously generated files stay loadable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_b: Option<Vec<Vec<String>>>,
}

/// Identity of a benchmark case, carried into the metrics so a results row is
/// self-describing without needing the filename parsed back.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaseMeta {
    pub circuit_name: String,
    pub edit: String,
    pub audio_input: String,
    pub bit_depth: usize,
    pub batch_size: usize,
    pub comp_size: usize,
    pub total_samples: usize,
    pub repetition: usize,
}

/// Timings and circuit sizes for one proof run.
///
/// Note: `compressed_setup_ms` is measured separately from
/// `compressed_prove_ms`. Logs produced before this split reported the two
/// summed under "CompressedSNARK::prove", so those older numbers are not
/// directly comparable to `compressed_prove_ms` here.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofMetrics {
    #[serde(flatten)]
    pub meta: CaseMeta,

    pub iterations: usize,

    pub constraints_primary: usize,
    pub constraints_secondary: usize,
    pub variables_primary: usize,
    pub variables_secondary: usize,

    pub public_params_ms: f64,
    pub recursive_prove_ms: f64,
    pub recursive_verify_ms: f64,
    pub compressed_setup_ms: f64,
    pub compressed_prove_ms: f64,
    pub compressed_verify_ms: f64,

    pub proof_size_bytes: usize,
    pub final_hash_original: String,
    pub final_hash_edited: String,
    /// Final value of the proven sample counter, for time-varying edits
    /// (tremolo, crop). `None` for memoryless edits, and `None` for merge,
    /// whose third state element is a hash rather than a counter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_counter: Option<String>,
    /// Final running commitment of the SECOND input stream, for
    /// multi-input edits. The three-wide IVC state is shared with
    /// tremolo/crop but means something different here, so the two are
    /// recorded in separate fields rather than one ambiguous slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_hash_b: Option<String>,

    /// Peak process RSS, in bytes, sampled during each phase and overall.
    ///
    /// `None` where the platform offers no RSS query, so a missing figure is
    /// distinguishable from a measured zero. These are PROCESS-WIDE and
    /// SAMPLED: they include the wasm witness generator's allocations, and a
    /// spike shorter than the sampling interval can be missed, making each
    /// value a lower bound. See `bench::memory`.
    pub peak_rss_bytes: Option<u64>,
    pub rss_baseline_bytes: Option<u64>,
    pub peak_rss_public_params_bytes: Option<u64>,
    pub peak_rss_recursive_prove_bytes: Option<u64>,
    pub peak_rss_compressed_prove_bytes: Option<u64>,
}

impl ProofMetrics {
    /// Prove-side microseconds per audio sample: the scaling figure.
    pub fn us_per_sample(&self) -> f64 {
        if self.meta.total_samples == 0 {
            return 0.0;
        }
        self.recursive_prove_ms * 1000.0 / self.meta.total_samples as f64
    }

    /// Milliseconds per folding step. Should stay roughly flat in N at a fixed
    /// B — that flatness is the IVC property.
    pub fn ms_per_iteration(&self) -> f64 {
        if self.iterations == 0 {
            return 0.0;
        }
        self.recursive_prove_ms / self.iterations as f64
    }

    /// Peak RSS in MB, if measured.
    pub fn peak_rss_mb(&self) -> Option<f64> {
        self.peak_rss_bytes.map(crate::bench::memory::to_mb)
    }

    /// Peak RSS above the pre-proof baseline, in MB.
    ///
    /// **Treat this as indicative only.** The baseline is whatever was
    /// resident when this case started, and the allocator does not return
    /// freed pages to the OS between cases, so a case that runs later in a
    /// sweep sees an inflated baseline and reports a smaller delta for
    /// identical work. Measured: the same (N=6000, B=100) cell reported a
    /// 35.9MB baseline when run alone and 344.7MB as the third case in a
    /// sweep, while its absolute peak differed by only 2.6%.
    ///
    /// Use [`peak_rss_mb`](Self::peak_rss_mb) for comparisons; it is the
    /// figure that reproduces.
    pub fn peak_rss_delta_mb(&self) -> Option<f64> {
        match (self.peak_rss_bytes, self.rss_baseline_bytes) {
            (Some(peak), Some(base)) => {
                Some(crate::bench::memory::to_mb(peak.saturating_sub(base)))
            }
            _ => None,
        }
    }

    /// Peak RSS divided by the fold count, in MB.
    ///
    /// Inherits the baseline caveat from
    /// [`peak_rss_delta_mb`](Self::peak_rss_delta_mb) and is NOT a per-step
    /// working-set figure: Nova reuses one step's allocation across folds,
    /// so the true per-step cost does not scale with the fold count at all.
    /// Dividing by `iterations` is therefore expected to FALL as N grows -
    /// that decline is the flatness, not a trend in memory.
    pub fn peak_rss_per_iteration_mb(&self) -> Option<f64> {
        if self.iterations == 0 {
            return None;
        }
        self.peak_rss_delta_mb().map(|mb| mb / self.iterations as f64)
    }

    pub fn constraints_per_sample(&self) -> f64 {
        let samples_per_step = self.meta.batch_size * self.meta.comp_size;
        if samples_per_step == 0 {
            return 0.0;
        }
        self.constraints_primary as f64 / samples_per_step as f64
    }
}

#[derive(Debug)]
pub enum ProofError {
    MissingFile(String),
    InputRead(String),
    InputShape(String),
    Prove(String),
    RecursiveVerify(String),
    CompressedSetup(String),
    CompressedProve(String),
    CompressedVerify(String),
}

impl fmt::Display for ProofError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProofError::MissingFile(s) => write!(f, "missing file: {}", s),
            ProofError::InputRead(s) => write!(f, "failed to read input: {}", s),
            ProofError::InputShape(s) => write!(f, "bad input shape: {}", s),
            ProofError::Prove(s) => write!(f, "RecursiveSNARK creation failed: {}", s),
            ProofError::RecursiveVerify(s) => write!(f, "RecursiveSNARK::verify failed: {}", s),
            ProofError::CompressedSetup(s) => write!(f, "CompressedSNARK::setup failed: {}", s),
            ProofError::CompressedProve(s) => write!(f, "CompressedSNARK::prove failed: {}", s),
            ProofError::CompressedVerify(s) => write!(f, "CompressedSNARK::verify failed: {}", s),
        }
    }
}

impl std::error::Error for ProofError {}

pub struct RunConfig<'a> {
    pub circuit_filepath: &'a str,
    pub witness_gen_filepath: &'a str,
    pub json_input_filepath: &'a str,
    /// Skip the CompressedSNARK phase (setup/prove/verify) for fast sweeps.
    pub skip_compressed: bool,
    pub verbose: bool,
    /// True when the third IVC state element is a second stream's hash
    /// rather than a sample counter, so the metrics record it correctly.
    pub multi_input: bool,
    /// Non-zero initial values for the public IVC state, as
    /// (index, value) pairs. Every element defaults to zero - both
    /// running hashes start empty and the sample counter starts at 0 -
    /// but concat carries its split point in the state, and a zero there
    /// would make the phase selector claim the entire stream belongs to
    /// stream B while the divisibility check passed trivially. So it has
    /// to be set explicitly rather than assumed.
    pub initial_state: Vec<(usize, u64)>,
    /// Width of the primary circuit's public IVC state. Memoryless edits
    /// use 2 (the two running hashes); a time-varying edit such as
    /// tremolo uses 3, adding a proven sample counter. This MUST match
    /// the compiled circuit's `step_in` width or Nova rejects the
    /// witness, so it comes from the edit registry rather than being
    /// assumed.
    pub state_width: usize,
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Runs one proof and returns its metrics. Errors are returned rather than
/// panicking so a sweep can record a failed cell and continue.
pub fn run_proof(cfg: &RunConfig, meta: &CaseMeta) -> Result<ProofMetrics, ProofError> {
    type G1 = pasta_curves::pallas::Point;
    type G2 = pasta_curves::vesta::Point;

    for p in [cfg.circuit_filepath, cfg.witness_gen_filepath, cfg.json_input_filepath] {
        if !Path::new(p).exists() {
            return Err(ProofError::MissingFile(p.to_string()));
        }
    }

    if cfg.verbose {
        println!(
            "Running test with witness generator: {} and group: {}",
            cfg.witness_gen_filepath,
            std::any::type_name::<G1>()
        );
    }

    let root = current_dir().unwrap();
    let circuit_file = root.join(cfg.circuit_filepath);
    let r1cs = load_r1cs::<G1, G2>(&FileLocation::PathBuf(circuit_file));
    let witness_generator_file = root.join(cfg.witness_gen_filepath);

    if cfg.verbose {
        println!();
        println!("Loading JSON input from \"{}\"... ", cfg.json_input_filepath);
    }

    let raw = std::fs::read_to_string(cfg.json_input_filepath)
        .map_err(|e| ProofError::InputRead(e.to_string()))?;
    let input_data: MultiplierInput =
        serde_json::from_str(&raw).map_err(|e| ProofError::InputRead(e.to_string()))?;

    if input_data.original.len() != input_data.edited.len() {
        return Err(ProofError::InputShape(format!(
            "original has {} batches, edited has {}",
            input_data.original.len(),
            input_data.edited.len()
        )));
    }
    if input_data.original.is_empty() {
        return Err(ProofError::InputShape("no batches".to_string()));
    }
    if let Some(b) = &input_data.stream_b {
        if b.len() != input_data.original.len() {
            return Err(ProofError::InputShape(format!(
                "stream_b has {} batches, original has {}",
                b.len(),
                input_data.original.len()
            )));
        }
    }

    let iteration_count = input_data.original.len();
    let elements_per_batch = input_data.original[0].len();

    // The witness generator is compiled for exactly `batch_size` elements; a
    // mismatch here is the failure mode that mislabelled inputs produce.
    if elements_per_batch != meta.batch_size {
        return Err(ProofError::InputShape(format!(
            "input has {} elements/batch but circuit expects {}",
            elements_per_batch, meta.batch_size
        )));
    }
    if let Some(bad) = input_data.original.iter().position(|b| b.len() != meta.batch_size) {
        return Err(ProofError::InputShape(format!(
            "batch {} has {} elements, expected {}",
            bad,
            input_data.original[bad].len(),
            meta.batch_size
        )));
    }

    if cfg.verbose {
        println!("Number of iterations (batches): {}", iteration_count);
        println!("Batch size: {}", elements_per_batch);
        println!();
    }

    // Signal names differ for multi-input edits: merge's step circuit
    // takes (a, b, out) rather than (orig, edit), because neither input
    // is "the original" in the single-stream sense.
    let mut private_inputs = Vec::new();
    for i in 0..iteration_count {
        let mut private_input = HashMap::new();
        match &input_data.stream_b {
            Some(b) => {
                private_input.insert("a".to_string(), json!(input_data.original[i]));
                private_input.insert("b".to_string(), json!(b[i]));
                private_input.insert("out".to_string(), json!(input_data.edited[i]));
            }
            None => {
                private_input.insert("orig".to_string(), json!(input_data.original[i]));
                private_input.insert("edit".to_string(), json!(input_data.edited[i]));
            }
        }
        private_inputs.push(private_input);
    }

    // All state elements start at zero: both running hashes, and the
    // sample counter for time-varying edits (the stream starts at index
    // 0). Sized from the registry so a 3-wide circuit is not handed a
    // 2-wide input.
    let mut start_public_input: Vec<F<G1>> = vec![F::<G1>::from(0); cfg.state_width];
    for &(i, v) in &cfg.initial_state {
        if i >= cfg.state_width {
            return Err(ProofError::InputShape(format!(
                "initial_state index {} is outside a {}-wide state",
                i, cfg.state_width
            )));
        }
        start_public_input[i] = F::<G1>::from(v);
    }

    // Baseline taken after inputs are parsed and before any proving work,
    // so the per-phase deltas attribute memory to proving rather than to
    // the ~100MB of parsed JSON the suite already holds.
    let rss_baseline_bytes = memory::current_rss_bytes();

    let sampler = PeakSampler::start();
    let start = Instant::now();
    let pp: PublicParams<G1, G2, _, _> = create_public_params(r1cs.clone());
    let public_params_ms = ms(start.elapsed());
    let peak_rss_public_params_bytes = sampler.stop();

    if cfg.verbose {
        println!(
            "Number of constraints per step (primary circuit): {}",
            pp.num_constraints().0
        );
        println!(
            "Number of constraints per step (secondary circuit): {}",
            pp.num_constraints().1
        );
        println!(
            "Number of variables per step (primary circuit): {}",
            pp.num_variables().0
        );
        println!(
            "Number of variables per step (secondary circuit): {}",
            pp.num_variables().1
        );
        println!("Creating a RecursiveSNARK...");
    }

    // The folding loop is where the peak almost always lands, and it is
    // entirely inside this one call - including the wasm witness generator
    // it drives per step - so sampling is the only way to see it.
    let sampler = PeakSampler::start();
    let start = Instant::now();
    let recursive_snark = create_recursive_circuit(
        FileLocation::PathBuf(witness_generator_file),
        r1cs,
        private_inputs,
        start_public_input.clone(),
        &pp,
    )
    .map_err(|e| ProofError::Prove(format!("{:?}", e)))?;
    let recursive_prove_ms = ms(start.elapsed());
    let peak_rss_recursive_prove_bytes = sampler.stop();

    if cfg.verbose {
        println!("RecursiveSNARK creation took {:.3}s", recursive_prove_ms / 1000.0);
        if let Some(p) = peak_rss_recursive_prove_bytes {
            println!("Peak RSS during folding: {:.1} MB", memory::to_mb(p));
        }
    }

    let z0_secondary = [F::<G2>::from(0)];

    if cfg.verbose {
        println!("Verifying a RecursiveSNARK...");
    }
    let start = Instant::now();
    let res = recursive_snark.verify(&pp, iteration_count, &start_public_input, &z0_secondary);
    let recursive_verify_ms = ms(start.elapsed());

    let res = res.map_err(|e| ProofError::RecursiveVerify(format!("{:?}", e)))?;
    let final_primary_state = res.0;
    let final_hash_original = format!("{:?}", &final_primary_state[0]);
    let final_hash_edited = format!("{:?}", &final_primary_state[1]);
    // For a time-varying edit the third state element is the proven
    // sample counter; the verifier checks it equals N, which is what
    // binds the edit to stream positions rather than letting the prover
    // choose a flattering phase.
    let third = final_primary_state.get(2).map(|c| format!("{:?}", c));
    let (final_counter, final_hash_b) = if cfg.multi_input {
        (None, third)
    } else {
        (third, None)
    };

    if cfg.verbose {
        println!("RecursiveSNARK::verify took {:?}ms", recursive_verify_ms);
        println!("Final original hash: {}", final_hash_original);
        println!("Final edited hash: {}", final_hash_edited);
    }

    let mut compressed_setup_ms = 0.0;
    let mut compressed_prove_ms = 0.0;
    let mut compressed_verify_ms = 0.0;
    let mut proof_size_bytes = 0;
    let mut peak_rss_compressed_prove_bytes = None;

    if !cfg.skip_compressed {
        if cfg.verbose {
            println!("Generating a CompressedSNARK using Spartan with IPA-PC...");
        }

        // Sampled across setup AND prove: `setup` materializes the prover
        // key, which stays resident through `prove`, so splitting the two
        // would attribute that allocation to whichever ran first rather
        // than reporting the real high-water mark of the phase.
        let sampler = PeakSampler::start();

        let start = Instant::now();
        let (pk, vk) = CompressedSNARK::<_, _, _, _, S<G1>, S<G2>>::setup(&pp)
            .map_err(|e| ProofError::CompressedSetup(format!("{:?}", e)))?;
        compressed_setup_ms = ms(start.elapsed());

        let start = Instant::now();
        let compressed_snark =
            CompressedSNARK::<_, _, _, _, S<G1>, S<G2>>::prove(&pp, &pk, &recursive_snark)
                .map_err(|e| ProofError::CompressedProve(format!("{:?}", e)))?;
        compressed_prove_ms = ms(start.elapsed());
        peak_rss_compressed_prove_bytes = sampler.stop();

        proof_size_bytes = serde_json::to_vec(&compressed_snark).map(|v| v.len()).unwrap_or(0);

        if cfg.verbose {
            println!("CompressedSNARK::prove took {:.3}s", compressed_prove_ms / 1000.0);
            println!("Verifying a CompressedSNARK...");
        }

        let start = Instant::now();
        compressed_snark
            .verify(
                &vk,
                iteration_count,
                start_public_input.clone(),
                z0_secondary.to_vec(),
            )
            .map_err(|e| ProofError::CompressedVerify(format!("{:?}", e)))?;
        compressed_verify_ms = ms(start.elapsed());

        if cfg.verbose {
            println!("CompressedSNARK::verify took {:?}ms", compressed_verify_ms);
            if let Some(p) = peak_rss_compressed_prove_bytes {
                println!("Peak RSS during compression: {:.1} MB", memory::to_mb(p));
            }
        }
    }

    Ok(ProofMetrics {
        meta: meta.clone(),
        iterations: iteration_count,
        constraints_primary: pp.num_constraints().0,
        constraints_secondary: pp.num_constraints().1,
        variables_primary: pp.num_variables().0,
        variables_secondary: pp.num_variables().1,
        public_params_ms,
        recursive_prove_ms,
        recursive_verify_ms,
        compressed_setup_ms,
        compressed_prove_ms,
        compressed_verify_ms,
        proof_size_bytes,
        final_hash_original,
        final_hash_edited,
        final_counter,
        final_hash_b,
        peak_rss_bytes: [
            peak_rss_public_params_bytes,
            peak_rss_recursive_prove_bytes,
            peak_rss_compressed_prove_bytes,
        ]
        .into_iter()
        .flatten()
        .max(),
        rss_baseline_bytes,
        peak_rss_public_params_bytes,
        peak_rss_recursive_prove_bytes,
        peak_rss_compressed_prove_bytes,
    })
}

/// Single-run entry point kept for the `main.rs` demo. Panics on failure, as
/// before.
pub fn run_test(circuit_filepath: &str, witness_gen_filepath: &str, json_input_filepath: &str) {
    // Infer the batch size from the input itself so this wrapper keeps its
    // original three-argument signature.
    let batch_size = std::fs::read_to_string(json_input_filepath)
        .ok()
        .and_then(|s| serde_json::from_str::<MultiplierInput>(&s).ok())
        .and_then(|d| d.original.first().map(|b| b.len()))
        .unwrap_or(0);

    let meta = CaseMeta {
        circuit_name: circuit_filepath.to_string(),
        edit: "unknown".to_string(),
        audio_input: "unknown".to_string(),
        bit_depth: 16,
        batch_size,
        comp_size: 15,
        total_samples: 0,
        repetition: 0,
    };

    let cfg = RunConfig {
        circuit_filepath,
        witness_gen_filepath,
        json_input_filepath,
        skip_compressed: false,
        verbose: true,
        multi_input: false,
        initial_state: Vec::new(),
        // The demo wrapper only drives memoryless edits; a time-varying
        // one must go through the bench suite, which knows its width.
        state_width: 2,
    };

    match run_proof(&cfg, &meta) {
        Ok(_) => {}
        Err(e) => panic!("proof run failed: {}", e),
    }
}
