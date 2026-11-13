#!/bin/bash
#
# One-shot setup for a fresh clone.
#
# Almost everything the demo needs is gitignored - circomlib, the WAV
# fixtures, the signing keys, the compiled circuits - so a clone cannot run
# anything until these are built. This script builds all of it, and is
# idempotent: re-running it skips whatever is already in place.
#
#   ./setup.sh                 # everything
#   ./setup.sh --force         # rebuild even what already exists
#   ./setup.sh --circuits-only # just compile the default circuits
#
# After this, `cargo run --release` should print PROTOCOL OK.

set -euo pipefail

cd "$(dirname "$0")"

FORCE=0
CIRCUITS_ONLY=0
for arg in "$@"; do
    case "$arg" in
        --force)         FORCE=1 ;;
        --circuits-only) CIRCUITS_ONLY=1 ;;
        -h|--help)
            sed -n '3,17p' "$0" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) echo "unknown argument: $arg (try --help)" >&2; exit 1 ;;
    esac
done

# The circuits main.rs and the end-to-end tests prove against. Keep this in
# step with the defaults in src/main.rs and tests/protocol_end_to_end.rs.
CIRCUITS=(
    "circuits/edits/gain/gain_b16_k2_B100.circom"   # src/main.rs default
    "circuits/edits/gain/gain_b8_k2_B10.circom"     # tests/protocol_end_to_end.rs
)

step()  { printf '\n\033[1m==> %s\033[0m\n' "$1"; }
have()  { command -v "$1" >/dev/null 2>&1; }
skip()  { printf '    (already present, skipping - use --force to rebuild)\n'; }

# --- 0. Tool check, up front -------------------------------------------
# Failing here is far clearer than failing three steps in with a cryptic
# error from whichever tool is missing.

step "Checking prerequisites"
missing=0
for tool in cargo circom node python3 openssl; do
    if have "$tool"; then
        printf '    %-8s %s\n' "$tool" "$(command -v "$tool")"
    else
        printf '    %-8s \033[31mMISSING\033[0m\n' "$tool"
        missing=1
    fi
done
# npm or pnpm, either is fine - only used to fetch circomlib.
if have npm; then
    NPM=npm
elif have pnpm; then
    NPM=pnpm
else
    printf '    %-8s \033[31mMISSING (need npm or pnpm)\033[0m\n' "npm"
    missing=1
fi
if [ "$missing" -ne 0 ]; then
    echo
    echo "Install the missing tools and re-run. See README.md > Prerequisites." >&2
    exit 1
fi

# --- 1. circomlib -------------------------------------------------------
# The circuits do `include "circom/poseidon.circom"`, resolved from the
# circuits/ root via `-l circuits`. That directory is gitignored, so it has
# to be populated from the npm package. It is ALSO the source that
# scripts/gen_poseidon_constants.py reads, so the Rust round constants and
# the circuits' constants provably come from one place.

if [ "$CIRCUITS_ONLY" -eq 0 ]; then
    step "circomlib -> circuits/circom/"
    if [ -f circuits/circom/poseidon_constants.circom ] && [ "$FORCE" -eq 0 ]; then
        skip
    else
        ( cd circuits && $NPM install )
        rm -rf circuits/circom
        cp -r circuits/node_modules/circomlib/circuits circuits/circom
        installed="$(python3 -c "import json;print(json.load(open('circuits/node_modules/circomlib/package.json'))['version'])")"
        printf '    circomlib %s\n' "$installed"

        # The version matters. circomlib 2.0.2 ships a DIFFERENT Poseidon
        # (decimal constants, no POSEIDON_S, t=2 present) whose digests do
        # not match 2.0.5's, and gen_poseidon_constants.py cannot parse it
        # at all. Getting this silently wrong would change every
        # commitment, so fail loudly instead.
        if [ "$installed" != "2.0.5" ]; then
            printf '\033[31m    ERROR: expected circomlib 2.0.5, got %s.\033[0m\n' "$installed" >&2
            printf '    The Poseidon constants differ between circomlib versions, so\n' >&2
            printf '    this would change every commitment. Fix the pin in\n' >&2
            printf '    circuits/package.json and re-run.\n' >&2
            exit 1
        fi
    fi

    # --- 2. Audio fixtures ---------------------------------------------
    # Amplitude is not cosmetic here. `gain2` is x*2 with NO clamping, so it
    # needs headroom; the clamping edits (gain_k*) want near-full-scale
    # audio or saturation never gets exercised. See scripts/README.md.

    step "Audio fixtures -> raw_audios/"
    if [ -f raw_audios/sweep_120s_2c_44sr_16bd.wav ] && [ "$FORCE" -eq 0 ]; then
        skip
    else
        python3 scripts/gen_bench_fixture.py
        python3 scripts/gen_16bit_clamp_fixture.py
        python3 scripts/gen_8bit_bench_fixture.py
    fi

    # --- 3. Signing keys -----------------------------------------------
    # generate.keys.sh names keys by today's date, so the id is discovered
    # rather than assumed - a clone run on any other day gets a different
    # one, and main.rs has to be told which.

    step "Signing keypair -> signers/"
    existing_key="$(ls signers/private_p256_*.pem 2>/dev/null | head -1 || true)"
    if [ -n "$existing_key" ] && [ "$FORCE" -eq 0 ]; then
        skip
    else
        bash ./generate.keys.sh
        existing_key="$(ls signers/private_p256_*.pem 2>/dev/null | head -1 || true)"
    fi
    KEY_ID="$(basename "${existing_key:-}" .pem | sed 's/^private_//')"
fi

# --- 4. Compile the circuits -------------------------------------------
# Circom needs concrete sizes, so every (edit, bit_depth, batch_size) is a
# separate compilation. These two cover the demo and the e2e tests.

step "Compiling circuits -> artifacts/"
for circuit in "${CIRCUITS[@]}"; do
    name="$(basename "$circuit" .circom)"
    if [ -f "artifacts/${name}.r1cs" ] && [ "$FORCE" -eq 0 ]; then
        printf '    %-24s already compiled\n' "$name"
        continue
    fi
    printf '    %-24s compiling...\n' "$name"
    bash ./compile.circuit.sh "$circuit" >/dev/null
done

# --- 5. Regenerate the derived Rust sources ----------------------------
# Both are committed, so this is a consistency check rather than a
# necessity: if it changes anything, the generated files had drifted from
# the circuits they mirror.

step "Checking generated sources"
python3 scripts/gen_poseidon_constants.py
python3 scripts/gen_lfo_table.py >/dev/null 2>&1 || true
if have git && git rev-parse --git-dir >/dev/null 2>&1; then
    if ! git diff --quiet -- src/poseidon/constants.rs src/lfo_table.rs 2>/dev/null; then
        printf '\033[33m    WARNING: regenerating changed a committed file.\033[0m\n'
        printf '    The Rust reference had drifted from the circuits. Review:\n'
        printf '      git diff src/poseidon/constants.rs src/lfo_table.rs\n'
    else
        printf '    generated sources match the circuits\n'
    fi
fi

# --- Done ---------------------------------------------------------------

step "Setup complete"
if [ "$CIRCUITS_ONLY" -eq 0 ] && [ -n "${KEY_ID:-}" ]; then
    main_key="$(grep -o 'let key_id = "[^"]*"' src/main.rs | sed 's/.*"\(.*\)"/\1/')"
    if [ "$KEY_ID" != "$main_key" ]; then
        printf '\033[33m    Your keypair is "%s" but src/main.rs expects "%s".\033[0m\n' \
            "$KEY_ID" "$main_key"
        printf '    Update key_id in src/main.rs, or run:\n'
        printf "      sed -i '' 's/%s/%s/' src/main.rs\n" "$main_key" "$KEY_ID"
        echo
    fi
fi
echo "    Next:  cargo run --release            # the protocol demo"
echo "           cargo test --release           # the test suite"
echo
