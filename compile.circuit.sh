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
circom $CIRCUIT_PATH -o $OUTPUT_PATH -l circuits --r1cs --sym --wasm --prime vesta
# circom $CIRCUIT_PATH -o $OUTPUT_PATH -l circuits --r1cs --sym --wasm