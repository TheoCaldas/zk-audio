use std::{collections::HashMap, env::current_dir, fmt, path::Path, time::Instant};

use nova_scotia::{
    circom::reader::load_r1cs, create_public_params, create_recursive_circuit, FileLocation, F, S,
};
use nova_snark::{CompressedSNARK, PublicParams};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
#[derive(Serialize)]
pub struct MultiplierInput {
    pub original: Vec<Vec<String>>,
    pub edited: Vec<Vec<String>>,
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

    let mut private_inputs = Vec::new();
    for i in 0..iteration_count {
        let mut private_input = HashMap::new();
        private_input.insert("orig".to_string(), json!(input_data.original[i]));
        private_input.insert("edit".to_string(), json!(input_data.edited[i]));
        private_inputs.push(private_input);
    }

    let start_public_input = [F::<G1>::from(0), F::<G1>::from(0)];

    let start = Instant::now();
    let pp: PublicParams<G1, G2, _, _> = create_public_params(r1cs.clone());
    let public_params_ms = ms(start.elapsed());

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

    let start = Instant::now();
    let recursive_snark = create_recursive_circuit(
        FileLocation::PathBuf(witness_generator_file),
        r1cs,
        private_inputs,
        start_public_input.to_vec(),
        &pp,
    )
    .map_err(|e| ProofError::Prove(format!("{:?}", e)))?;
    let recursive_prove_ms = ms(start.elapsed());

    if cfg.verbose {
        println!("RecursiveSNARK creation took {:.3}s", recursive_prove_ms / 1000.0);
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

    if cfg.verbose {
        println!("RecursiveSNARK::verify took {:?}ms", recursive_verify_ms);
        println!("Final original hash: {}", final_hash_original);
        println!("Final edited hash: {}", final_hash_edited);
    }

    let mut compressed_setup_ms = 0.0;
    let mut compressed_prove_ms = 0.0;
    let mut compressed_verify_ms = 0.0;
    let mut proof_size_bytes = 0;

    if !cfg.skip_compressed {
        if cfg.verbose {
            println!("Generating a CompressedSNARK using Spartan with IPA-PC...");
        }

        let start = Instant::now();
        let (pk, vk) = CompressedSNARK::<_, _, _, _, S<G1>, S<G2>>::setup(&pp)
            .map_err(|e| ProofError::CompressedSetup(format!("{:?}", e)))?;
        compressed_setup_ms = ms(start.elapsed());

        let start = Instant::now();
        let compressed_snark =
            CompressedSNARK::<_, _, _, _, S<G1>, S<G2>>::prove(&pp, &pk, &recursive_snark)
                .map_err(|e| ProofError::CompressedProve(format!("{:?}", e)))?;
        compressed_prove_ms = ms(start.elapsed());

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
                start_public_input.to_vec(),
                z0_secondary.to_vec(),
            )
            .map_err(|e| ProofError::CompressedVerify(format!("{:?}", e)))?;
        compressed_verify_ms = ms(start.elapsed());

        if cfg.verbose {
            println!("CompressedSNARK::verify took {:?}ms", compressed_verify_ms);
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
    };

    match run_proof(&cfg, &meta) {
        Ok(_) => {}
        Err(e) => panic!("proof run failed: {}", e),
    }
}
