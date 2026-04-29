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

circom $CIRCUIT_PATH -o $OUTPUT_PATH --r1cs --sym --wasm --prime vesta