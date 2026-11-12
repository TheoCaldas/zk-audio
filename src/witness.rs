//! In-process Circom witness calculation.
//!
//! `nova-scotia` computes each fold's witness by spawning a `node`
//! subprocess: it writes `circom_input.json`, runs the circuit's
//! `generate_witness.js` against the wasm module, writes a `.wtns` file,
//! then reads it back. At B=100 that is ~228ms per fold of which ~61ms is
//! the spawn and the file round-trip, and the ~7MB witness file is written
//! and re-read once per fold (853 times for a 30s clip).
//!
//! This module runs the same wasm module in-process with wasmer, keeping
//! it instantiated across folds, and reads the witness straight out of the
//! module's memory. The wasm executed and the values produced are
//! identical - only the plumbing changes, so there is no effect on what is
//! proven.
//!
//! The protocol below mirrors `witness_calculator.js` from circom's `_js`
//! output: signals are addressed by a 64-bit FNV-1a hash of their name,
//! and field elements move through a shared memory window four 32-bit
//! limbs at a time.

use std::collections::HashMap;
use std::path::Path;

use ff::{Field, PrimeField};
use num_bigint::BigInt;
use num_traits::Num;
use serde_json::Value;
use wasmer::{imports, Function, Instance, Module, Store, Value as WVal};

pub struct WitnessCalculator {
    store: Store,
    instance: Instance,
    n32: u32,
    /// Byte offset of the shared read/write window inside the wasm memory.
    /// Resolving it once lets us move field elements with slice copies
    /// instead of one host->wasm call per 32-bit limb; at 218k witness
    /// entries the per-call overhead otherwise dominates by ~6x.
    shared_ptr: usize,
}

/// FNV-1a over the signal name, exactly as `witness_calculator.js` does it.
fn fnv_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for b in s.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

impl WitnessCalculator {
    pub fn new(wasm_path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(wasm_path)
            .map_err(|e| format!("read {}: {}", wasm_path.display(), e))?;
        let mut store = Store::default();
        let module =
            Module::new(&store, bytes).map_err(|e| format!("wasm compile: {}", e))?;

        // circom's wasm imports a handful of logging/error hooks. They are
        // only called on assertion failure, so empty bodies are fine - but
        // `exceptionHandler` must not be a no-op silently swallowing a
        // failed constraint, so it panics loudly.
        let f0 = Function::new_typed(&mut store, || {});
        let f1 = Function::new_typed(&mut store, |_: i32| {});
        let f2 = Function::new_typed(&mut store, |_: i32, _: i32| {});
        let f4 = Function::new_typed(&mut store, |_: i32, _: i32, _: i32, _: i32| {});
        let err = Function::new_typed(
            &mut store,
            |c: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> () {
                panic!("circom witness generation failed (code {c})");
            },
        );
        let imports = imports! {
            "runtime" => {
                "error" => err,
                "exceptionHandler" => f1.clone(),
                "printErrorMessage" => f0.clone(),
                "writeBufferMessage" => f0.clone(),
                "showSharedRWMemory" => f0.clone(),
                "log" => f1.clone(),
                "logSetSignal" => f2.clone(),
                "logGetSignal" => f2.clone(),
                "logStartComponent" => f1.clone(),
                "logFinishComponent" => f1.clone(),
                "printDebug" => f1.clone(),
                "copyFr" => f4.clone(),
            }
        };
        let instance = Instance::new(&mut store, &module, &imports)
            .map_err(|e| format!("wasm instantiate: {}", e))?;

        let n32 = instance
            .exports
            .get_function("getFieldNumLen32")
            .or_else(|_| instance.exports.get_function("getFieldNumLen"))
            .map_err(|e| format!("getFieldNumLen: {}", e))?
            .call(&mut store, &[])
            .map_err(|e| format!("getFieldNumLen call: {}", e))?[0]
            .unwrap_i32() as u32;

        // `getSharedRWMemoryStart` would be ideal but circom does not
        // export it; instead write a sentinel through the documented API
        // and find where it landed in linear memory.
        let mut me = Self { store, instance, n32, shared_ptr: 0 };
        me.shared_ptr = me.locate_shared()?;
        Ok(me)
    }

    /// Find the shared window by writing a recognisable pattern into it
    /// and scanning linear memory for the result.
    fn locate_shared(&mut self) -> Result<usize, String> {
        let pat: Vec<u32> = (0..self.n32).map(|i| 0xDEAD_0000u32 ^ (i * 0x9E37)).collect();
        for (i, v) in pat.iter().enumerate() {
            self.call(
                "writeSharedRWMemory",
                &[WVal::I32(i as i32), WVal::I32(*v as i32)],
            )?;
        }
        let mem = self
            .instance
            .exports
            .get_memory("memory")
            .map_err(|e| format!("memory export: {}", e))?
            .view(&self.store);
        let mut buf = vec![0u8; mem.data_size() as usize];
        mem.read(0, &mut buf).map_err(|e| format!("mem read: {}", e))?;
        let needle: Vec<u8> = pat.iter().flat_map(|v| v.to_le_bytes()).collect();
        buf.windows(needle.len())
            .position(|w| w == needle)
            .ok_or_else(|| "shared RW window not found in wasm memory".to_string())
    }

    fn read_shared_fast(&self, idx: u32) -> Result<Vec<u32>, String> {
        let mem = self
            .instance
            .exports
            .get_memory("memory")
            .map_err(|e| format!("memory export: {}", e))?
            .view(&self.store);
        let mut raw = vec![0u8; (self.n32 * 4) as usize];
        mem.read((self.shared_ptr + (idx as usize) * 0) as u64, &mut raw)
            .map_err(|e| format!("mem read: {}", e))?;
        Ok(raw
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect())
    }

    fn call(&mut self, name: &str, args: &[WVal]) -> Result<Vec<WVal>, String> {
        let f = self
            .instance
            .exports
            .get_function(name)
            .map_err(|e| format!("export {}: {}", name, e))?
            .clone();
        f.call(&mut self.store, args)
            .map_err(|e| format!("{}: {}", name, e))
            .map(|b| b.to_vec())
    }

    /// Push one field element into the shared window, little-endian limbs.
    fn write_shared(&mut self, v: &BigInt) -> Result<(), String> {
        let mask = BigInt::from(u32::MAX);
        for i in 0..self.n32 {
            let limb = (v >> (32 * i)) & &mask;
            let limb: u32 = limb.try_into().unwrap_or(0);
            self.call(
                "writeSharedRWMemory",
                &[WVal::I32(i as i32), WVal::I32(limb as i32)],
            )?;
        }
        Ok(())
    }

    /// Compute the witness for one step. `inputs` maps signal name to its
    /// values, matching the JSON nova-scotia would otherwise have written.
    pub fn calculate<F: PrimeField>(
        &mut self,
        inputs: &HashMap<String, Vec<BigInt>>,
    ) -> Result<Vec<F>, String> {
        self.call("init", &[WVal::I32(0)])?;

        for (name, values) in inputs {
            let h = fnv_hash(name);
            let msb = (h >> 32) as i32;
            let lsb = (h & 0xFFFF_FFFF) as i32;
            for (i, v) in values.iter().enumerate() {
                self.write_shared(v)?;
                self.call(
                    "setInputSignal",
                    &[WVal::I32(msb), WVal::I32(lsb), WVal::I32(i as i32)],
                )?;
            }
        }

        let size = self.call("getWitnessSize", &[])?[0].unwrap_i32() as u32;

        // Resolve the typed export and the memory view ONCE. The dynamic
        // `call()` path boxes every argument and return value, which at
        // 218k witness entries costs more than the witness math itself.
        let get_witness = self
            .instance
            .exports
            .get_typed_function::<i32, ()>(&self.store, "getWitness")
            .map_err(|e| format!("getWitness typed: {}", e))?;
        let memory = self
            .instance
            .exports
            .get_memory("memory")
            .map_err(|e| format!("memory export: {}", e))?
            .clone();

        let n32 = self.n32 as usize;
        let mut raw = vec![0u8; n32 * 4];
        let mut out = Vec::with_capacity(size as usize);
        let mut le = vec![0u8; n32 * 4];

        for i in 0..size {
            get_witness
                .call(&mut self.store, i as i32)
                .map_err(|e| format!("getWitness({}): {}", i, e))?;
            memory
                .view(&self.store)
                .read(self.shared_ptr as u64, &mut raw)
                .map_err(|e| format!("mem read: {}", e))?;
            // `raw` is already little-endian limbs, which is exactly the
            // byte order PrimeField::from_repr wants - no hex round-trip.
            le.copy_from_slice(&raw);
            let mut repr = F::Repr::default();
            let rb = repr.as_mut();
            let n = rb.len().min(le.len());
            rb[..n].copy_from_slice(&le[..n]);
            let v = Option::<F>::from(F::from_repr(repr))
                .ok_or_else(|| format!("witness {} not a field element", i))?;
            out.push(v);
        }
        Ok(out)
    }
}

/// Convert nova-scotia's per-step private input map into the shape
/// `calculate` wants, adding `step_in` from the current public state.
pub fn build_inputs(
    step_in: &[BigInt],
    private: &HashMap<String, Value>,
) -> Result<HashMap<String, Vec<BigInt>>, String> {
    let mut m = HashMap::new();
    m.insert("step_in".to_string(), step_in.to_vec());
    for (k, v) in private {
        let arr = v
            .as_array()
            .ok_or_else(|| format!("private input {} is not an array", k))?;
        let mut vals = Vec::with_capacity(arr.len());
        for x in arr {
            let s = x
                .as_str()
                .ok_or_else(|| format!("private input {} holds a non-string", k))?;
            let s = s.strip_prefix("0x").unwrap_or(s);
            let radix = if s == x.as_str().unwrap_or("") { 10 } else { 16 };
            vals.push(
                BigInt::from_str_radix(s, radix)
                    .map_err(|e| format!("private input {}: {}", k, e))?,
            );
        }
        m.insert(k.clone(), vals);
    }
    Ok(m)
}


/// Fold loop against nova-snark 0.76.
///
/// Two API changes from 0.23, both visible here:
///   * `Group` became `Engine`;
///   * the secondary circuit and its `z0` are gone - `RecursiveSNARK` is
///     parameterised by the primary circuit alone and manages the
///     secondary internally.
///
/// The witness for each step is computed in-process (see
/// `WitnessCalculator`) rather than by spawning `node`.
pub fn create_recursive_circuit_inproc<E1, E2>(
    wasm_path: &Path,
    r1cs: crate::circom_bridge::circuit::R1CS<<E1 as nova_snark::traits::Engine>::Scalar>,
    private_inputs: Vec<HashMap<String, Value>>,
    start_public_input: Vec<<E1 as nova_snark::traits::Engine>::Scalar>,
    pp: &nova_snark::nova::PublicParams<
        E1,
        E2,
        crate::circom_bridge::circuit::CircomCircuit<<E1 as nova_snark::traits::Engine>::Scalar>,
    >,
) -> Result<
    nova_snark::nova::RecursiveSNARK<
        E1,
        E2,
        crate::circom_bridge::circuit::CircomCircuit<<E1 as nova_snark::traits::Engine>::Scalar>,
    >,
    String,
>
where
    E1: nova_snark::traits::Engine<Base = <E2 as nova_snark::traits::Engine>::Scalar>,
    E2: nova_snark::traits::Engine<Base = <E1 as nova_snark::traits::Engine>::Scalar>,
{
    use crate::circom_bridge::circuit::CircomCircuit;

    let mut wc = WitnessCalculator::new(wasm_path)?;
    let iteration_count = private_inputs.len();

    let to_bigint = |x: &<E1 as nova_snark::traits::Engine>::Scalar| -> BigInt {
        let h = format!("{:?}", x);
        BigInt::from_str_radix(h.strip_prefix("0x").unwrap_or(&h), 16).unwrap()
    };
    let mut current: Vec<BigInt> = start_public_input.iter().map(to_bigint).collect();

    let w0 = wc.calculate(&build_inputs(&current, &private_inputs[0])?)?;
    let circuit_0 = CircomCircuit { r1cs: r1cs.clone(), witness: Some(w0) };
    let mut recursive_snark =
        nova_snark::nova::RecursiveSNARK::new(pp, &circuit_0, &start_public_input)
            .map_err(|e| format!("RecursiveSNARK::new: {:?}", e))?;

    for i in 0..iteration_count {
        let w = wc.calculate(&build_inputs(&current, &private_inputs[i])?)?;
        let circuit = CircomCircuit { r1cs: r1cs.clone(), witness: Some(w) };
        current = circuit.get_public_outputs().iter().map(to_bigint).collect();
        recursive_snark
            .prove_step(pp, &circuit)
            .map_err(|e| format!("prove_step {}: {:?}", i, e))?;
    }
    Ok(recursive_snark)
}
