#!/bin/bash

set -euo pipefail

OUT_DIR="signers"
OPENSSL_BIN="$(command -v openssl)"
ID="$(date +%d%m%y)"

mkdir -p "$OUT_DIR"

"$OPENSSL_BIN" genpkey -algorithm EC \
  -pkeyopt ec_paramgen_curve:P-256 \
  -out "$OUT_DIR/private_p256_${ID}.pem"

"$OPENSSL_BIN" pkey -in "$OUT_DIR/private_p256_${ID}.pem" -pubout \
  -out "$OUT_DIR/public_p256_${ID}.pem"

echo "Keys generated:"
echo "  Private: $OUT_DIR/private_p256_${ID}.pem"
echo "  Public:  $OUT_DIR/public_p256_${ID}.pem"