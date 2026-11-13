//! A Rust library for zero-knowledge proofs for audio editing. 
//! This library provides tools for parsing audio files, generating cryptographic signatures, and constructing zk-SNARK circuits for audio editing verification.

pub mod circuit;
pub mod poseidon;
pub mod protocol;
pub mod circom_bridge;
pub mod witness;
pub mod audio_parser;
pub mod audio_editor;
pub mod signer;
pub mod bench;
pub mod lfo_table;
