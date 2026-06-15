use std::{collections::HashMap, env::current_dir, fs::File, io::Write, time::Instant};

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

pub fn run_test(circuit_filepath: &str, witness_gen_filepath: &str, json_input_filepath: &str) {
    type G1 = pasta_curves::pallas::Point;
    type G2 = pasta_curves::vesta::Point;

    
    println!(
        "Running test with witness generator: {} and group: {}",
        witness_gen_filepath,
        std::any::type_name::<G1>()
    );
    let root = current_dir().unwrap();
    
    let circuit_file = root.join(circuit_filepath);
    let r1cs = load_r1cs::<G1, G2>(&FileLocation::PathBuf(circuit_file));
    let witness_generator_file = root.join(witness_gen_filepath);
    
    println!();
    println!("Loading JSON input from \"{}\"... ", json_input_filepath);
    let input_data: MultiplierInput = serde_json::from_str(
        &std::fs::read_to_string(json_input_filepath).expect("Failed to read JSON file"))
        .expect("Deserialization failed");
    assert_eq!(input_data.original.len(), input_data.edited.len(), "Original and edited batches must have the same length");
    let iteration_count = input_data.original.len();
    println!("Number of iterations (batches): {}", iteration_count);
    println!("Batch size: {}", input_data.original[0].len());
    println!();

    let mut private_inputs = Vec::new();
    for i in 0..iteration_count {
        let mut private_input = HashMap::new();
        private_input.insert("orig".to_string(), json!(input_data.original[i]));
        private_input.insert("edit".to_string(), json!(input_data.edited[i]));
        private_inputs.push(private_input);
    }
    
    // private_inputs = dbg!(private_inputs);

    let start_public_input = [F::<G1>::from(0), F::<G1>::from(0)];

    let pp: PublicParams<G1, G2, _, _> = create_public_params(r1cs.clone());

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
    let start = Instant::now();
    let recursive_snark = create_recursive_circuit(
        FileLocation::PathBuf(witness_generator_file),
        r1cs,
        private_inputs,
        start_public_input.to_vec(),
        &pp,
    )
    .unwrap();
    println!("RecursiveSNARK creation took {:?}", start.elapsed());

    // println!("{}", json!(recursive_snark));
    let mut file = File::create("recursive_snark.json").unwrap();
    file.write_all(
        serde_json::to_string_pretty(&json!(recursive_snark)).unwrap().as_bytes()
    ).unwrap();

    // TODO: empty?
    let z0_secondary = [F::<G2>::from(0)];

    // verify the recursive SNARK
    println!("Verifying a RecursiveSNARK...");
    let start = Instant::now();
    let res = recursive_snark.verify(&pp, iteration_count, &start_public_input, &z0_secondary);
    println!(
        "RecursiveSNARK::verify: {:?}, took {:?}",
        res,
        start.elapsed()
    );
    assert!(res.is_ok());

    // produce a compressed SNARK
    println!("Generating a CompressedSNARK using Spartan with IPA-PC...");
    let start = Instant::now();

    let (pk, vk) = CompressedSNARK::<_, _, _, _, S<G1>, S<G2>>::setup(&pp).unwrap();
    let res = CompressedSNARK::<_, _, _, _, S<G1>, S<G2>>::prove(&pp, &pk, &recursive_snark);
    println!(
        "CompressedSNARK::prove: {:?}, took {:?}",
        res.is_ok(),
        start.elapsed()
    );
    assert!(res.is_ok());
    let compressed_snark = res.unwrap();
    // println!("{}", json!(compressed_snark));

    // verify the compressed SNARK
    println!("Verifying a CompressedSNARK...");
    let start = Instant::now();
    let res = compressed_snark.verify(
        &vk,
        iteration_count,
        start_public_input.to_vec(),
        z0_secondary.to_vec(),
    );
    println!(
        "CompressedSNARK::verify: {:?}, took {:?}",
        res.is_ok(),
        start.elapsed()
    );
    assert!(res.is_ok());
}
