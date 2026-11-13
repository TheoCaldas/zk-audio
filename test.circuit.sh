#!/bin/bash

set -e

if [ -z "$1" ]; then
    echo "Fail! Usage: $0 {CIRCUIT_NAME} {INPUT_JSON_PATH:-input.json}"
    exit 1
fi

CIRCUIT_NAME=$1
INPUT_JSON_PATH=input.json

if [ -n "$2" ]; then
    INPUT_JSON_PATH=$2
fi

if [ ! -f "$INPUT_JSON_PATH" ]; then
    echo "Fail! Input JSON file not found: $INPUT_JSON_PATH"
    exit 1
fi

# if not /test directory, create it
if [ ! -d "artifacts/${CIRCUIT_NAME}_js/test" ]; then
    mkdir -p artifacts/${CIRCUIT_NAME}_js/test
fi

node artifacts/${CIRCUIT_NAME}_js/generate_witness.js \
    artifacts/${CIRCUIT_NAME}_js/${CIRCUIT_NAME}.wasm \
    ${INPUT_JSON_PATH} \
    artifacts/${CIRCUIT_NAME}_js/test/witness.wtns

echo "Witness generated successfully: artifacts/${CIRCUIT_NAME}_js/test/witness.wtns"

snarkjs wtns export json artifacts/${CIRCUIT_NAME}_js/test/witness.wtns artifacts/${CIRCUIT_NAME}_js/test/witness.json 

echo "Witness exported to JSON successfully: artifacts/${CIRCUIT_NAME}_js/test/witness.json"

cat artifacts/${CIRCUIT_NAME}_js/test/witness.json | jq '.'