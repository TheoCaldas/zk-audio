#!/bin/bash

set -e

if [ -z "$1" ]; then
    echo "Fail! Usage: $0 {CIRCUIT_PATH} {OUTPUT_PATH:-artifacts}"
    exit 1
fi

OUTPUT_PATH=artifacts
CIRCUIT_PATH=$1

if [ -n "$2" ]; then
    OUTPUT_PATH=$2
fi

if [ ! -d "$OUTPUT_PATH" ]; then
    mkdir $OUTPUT_PATH
fi

# -l circuits: resolves bare includes ("hasher.circom",
# "circom/poseidon.circom") from the circuits/ root regardless of which
# subdirectory the circuit being compiled lives in. Without it, circom
# resolves includes relative to the including file, so a step circuit in
# circuits/edits/<family>/ cannot find the shared primitives at
# circuits/.
# --prime vesta: NOT optional, and not interchangeable with the default.
# circuit.rs folds with PallasEngine as the primary curve, and a Nova step
# circuit is expressed over the primary curve's SCALAR field - which is the
# Vesta base field, 0x40...0994a8dd8c46eb21...0001. Compiling under the
# default BN254 prime produces a circuit Nova cannot load.
#
# The prime also fixes the Poseidon digest. circomlib's round constants are
# literal hex that circom reinterprets modulo whichever prime it is given,
# so the SAME circuit source hashes differently under a different --prime.
# The Rust reference hash in src/poseidon must therefore be instantiated at
# this same field or a recomputed commitment will not match the proof's
# terminal state. That mismatch is exactly what
# README.md "Poseidon parity" documents; tests/poseidon_parity.rs
# pins it.
circom $CIRCUIT_PATH -o $OUTPUT_PATH -l circuits --r1cs --sym --wasm --prime vesta
# circom $CIRCUIT_PATH -o $OUTPUT_PATH -l circuits --r1cs --sym --wasm