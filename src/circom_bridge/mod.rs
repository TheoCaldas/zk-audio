//! Circom bridge vendored from nova-scotia 0.5.0 and ported to
//! nova-snark 0.76.
//!
//! nova-scotia is pinned to nova-snark 0.23 and unmaintained, so the parts
//! we need - the `CircomCircuit` StepCircuit impl and the r1cs reader -
//! are vendored here. The node-subprocess witness generators are dropped:
//! `crate::witness` computes witnesses in-process instead.
pub mod circuit;
pub mod reader;
pub mod file;
