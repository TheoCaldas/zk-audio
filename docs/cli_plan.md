# CLI implementation plan

**Status: implemented.** See README.md, "The CLI", for usage. This file is
the design record; where it and the code disagree, the code is right.

Two things changed during implementation:

- `CompressedSNARK::verify` turned out to take only `vk`, not `pp`, so
  verification is milliseconds and the ~30s in option C is purely key
  derivation. The measured vk is ~28 MB against a ~24 KB proof, which
  settles the question of shipping it.
- Both A and B gained a "folds to prove/commit" prompt. Without it A signed
  the whole file while B proved a prefix, so their commitments could never
  agree — found by running the two steps back to back.

An interactive CLI covering the three roles in the protocol: mock-sign a
source, prove an edit, verify a bundle. Built by `cargo build --release`.

Decisions taken (asked and confirmed): option C re-derives the verifier key
from the circuit rather than loading a shipped one; proving truncates to a
fold boundary and confirms before starting; option A signs the Poseidon
stream commitment; the CLI is interactive-only, in `src/cli/` behind its own
binary.

## What already exists, and what is missing

Most of this is wiring. Three things genuinely do not exist yet.

**1. A proof cannot currently be saved.** `circuit::run_proof` builds the
`CompressedSNARK`, serializes it only to measure `proof_size_bytes`, then
drops it along with the `VerifierKey`. Both types are
`Serialize + Deserialize` in nova-snark 0.76 (checked), so persistence is
possible but needs an API change.

**2. There is no verify-from-disk path.** Nothing loads a proof and checks
it. `run_proof` proves and verifies in one call.

**3. `S<E>` and `EE<E>` are private type aliases** in `circuit.rs`. A
standalone verifier needs them.

Everything else is reuse: `bench::config::{registry, find_edit,
circuit_name_for, comp_size_for}`, `audio_parser` for WAV and packing,
`poseidon::BatchHashKind`, `protocol::{create_provenance,
verify_provenance}`, `signer`, and `scripts/predict_proof_time.py`.

Deferred: `concat` and `merge`. Both need a second input stream, and concat
carries a 5-wide IVC state with a prover-supplied split point, so neither
fits the single-source prompt flow without a second pass.

## Step 0: extend `circuit.rs`

`run_proof` has five call sites (`protocol.rs`, `run_test`, `bench/mod.rs`,
two tests). Changing its signature churns all of them, so instead:

- Add `ProofArtifacts { proof_json: Vec<u8>, vk_json: Vec<u8> }`.
- Add `pub fn run_proof_keep(cfg, meta) -> Result<(ProofMetrics,
  Option<ProofArtifacts>), ProofError>` holding the current body.
- Reduce `run_proof` to `run_proof_keep(..).map(|(m, _)| m)`. Existing
  callers and the bench suite are untouched.
- `Option` because `skip_compressed` means there is no compressed proof to
  keep.
- Add `pub fn verify_compressed(proof_json, vk_json, iterations, z0) ->
  Result<Vec<String>, ProofError>`, deserializing both and calling
  `CompressedSNARK::verify`, returning the terminal IVC state as hex
  strings so the caller can compare commitments.
- Add `pub fn derive_vk(r1cs_path) -> Result<Vec<u8>, ProofError>` for
  option C, which has no proving run to take `vk` from.
- Make `S`/`EE` `pub type`.

`CompressedSNARK::verify(&vk, num_steps, z0)` takes **only** `vk` — not
`pp` — and returns the terminal IVC state (checked against the nova-snark
source). So verification itself is fast; the ~30s in option C is spent
re-deriving `vk` from the circuit via `create_public_params` +
`CompressedSNARK::setup`.

That re-derivation is a deliberate choice, not a necessity: `vk` is
`Serialize`, so B could persist it and C could load it in milliseconds. It
is rebuilt from the r1cs instead because **a shipped verifier key is not
trustworthy** — a forger ships their own `vk` alongside their own proof and
every check passes. Deriving `vk` from the circuit the verifier already
holds is what makes the proof mean anything. The cost buys that property.

## Step 1: the bundle format

Two files per run, both JSON for inspectability.

`<name>.sig.json` — written by A:

```json
{
  "kind": "zk-audio-signature/1",
  "h_orig": "0x...",
  "signature_der_hex": "...",
  "public_key_pem": "...",
  "bit_depth": 16,
  "batch_size": 100,
  "samples": 600000,
  "source_file": "sweep_120s_2c_44sr_16bd.wav"
}
```

`bit_depth`, `batch_size` and `samples` are not decoration: `h_orig` is a
fold over a specific partition, so the same audio under a different batch
size commits to a different value. A verifier that does not know them cannot
recompute anything. `public_key_pem` is embedded for convenience **and is
not a trust anchor** — see the mock-signing note below.

`<name>.proof.json` — written by B:

```json
{
  "kind": "zk-audio-proof/1",
  "circuit_name": "gain_b16_k2_B100",
  "edit": "gain_k2",
  "bit_depth": 16,
  "batch_size": 100,
  "comp_size": 15,
  "samples": 600000,
  "iterations": 400,
  "z0": ["0x00..", "0x00.."],
  "h_orig": "0x...",
  "h_edit": "0x...",
  "proof": { "...": "CompressedSNARK" }
}
```

`circuit_name` is what lets C re-derive the key without asking the user for
it. `iterations` and `z0` are required by `CompressedSNARK::verify`.

No `ProofMetrics`. Timings are not part of the statement, and a bundle that
carries them invites reading provenance data as benchmark data. The CLI
times its own phases in-process and logs them to the terminal instead; the
bench suite remains the place where timings are recorded to disk.

## Step 2: module layout

```
src/cli/
├── mod.rs        — the menu loop, dispatch, exit
├── prompt.rs     — read_line helpers: text, path, usize with default, y/n, pick-from-list
├── sign.rs       — option A
├── prove.rs      — option B
├── verify.rs     — option C
└── bundle.rs     — the two JSON formats, read + write
src/bin/cli.rs    — fn main() { zk_audio::cli::run() }
```

Logic separate from prompting so the three operations stay testable without
stdin.

`Cargo.toml` gains `[[bin]] name = "zk-audio-cli" path = "src/bin/cli.rs"`.
`default-run` stays on `zk-audio`, so `cargo run` still runs the demo and
the CLI is `cargo run --release --bin zk-audio-cli`.

Dependencies: none new. `base64` is avoidable by storing the proof as a hex
string or a nested JSON value; prefer a nested value, since
`serde_json::to_vec` already produces JSON.

## Step 3: the interaction

### Menu

```
zk-audio
  A) mock-sign a source
  B) prove an edit
  C) verify a bundle
  exit
>
```

Loops after each run. Invalid input re-prompts rather than exiting.

### A — mock-sign

1. Prompt: audio source path; key path (default: the newest
   `signers/private_p256_*.pem`, since `generate.keys.sh` names by date);
   output dir (default `.`).
2. Prompt bit depth (default 16) and batch size (default 100), because
   `h_orig` depends on both. Truncate to a fold boundary and say so.
3. Compute `h_orig` via `audio_parser::hex_stream_commitment`, sign
   `SHA256(h_orig)` via `signer`.
4. Write `<stem>.sig.json`. Print `h_orig` and the path.

Prints that this is a **mock** signing layer and that the real key would
come from an attested capture device.

### B — prove an edit

1. List the provable single-input edits from the registry, numbered. Excluded:
   the ablation variants (`*_nohash`, `*_wide*`, `*_onehash`, `*_sponge*`),
   which are benchmark-only and in two cases unsound; and the multi-input
   edits (`concat`, `merge`), which need a second audio stream and a 5-wide
   `z0` — deferred, with `edit.apply2.is_some()` as the filter so they
   appear automatically once supported.
2. Prompt bit depth (default 16, validated against `edit.bit_depths`) and
   batch size (default 100).
3. Prompt the edit's own parameters. These differ per family and come from
   `EditSpec.factor` / `EditCtx`:

   | edit | extra prompt |
   |---|---|
   | `gain_k*` | factor k — but k is baked into the registry entry, so offer the compiled k values as the choice |
   | `atten_m*` | shift m, likewise from the registry |
   | `dist_d*` | blend d, likewise |
   | `tremolo_f*` | LFO frequency; also `period` for `EditCtx` (default 8820 = 5 Hz at 44.1 kHz) |
   | `crop` | window start and length, fold-aligned |

   Note: k/m/d/f are **not free parameters** — each value is a separately
   compiled circuit, so the prompt is a choice among compiled variants, not
   a number. Present it that way rather than accepting a value that cannot
   be proven.
4. Check `artifacts/<circuit_name>.r1cs` and the `_js/*.wasm` exist. If not,
   print the exact `./compile.circuit.sh <path>` to run, and return to the
   menu.
5. Prompt audio source path and output dir (default `.`).
6. Read the WAV, truncate to a whole number of folds, report what was
   dropped. Apply the edit off-circuit via `edit.apply`. Write the edited
   WAV, inheriting the source's rate/channels.
7. Predict the proving time by shelling out to
   `scripts/predict_proof_time.py` with the chosen parameters, printing its
   output. If python3 is missing or the script fails, fall back to a note
   that no estimate is available — never block on it.
8. Confirm (`y/N`) before proving.
9. Serialize inputs, run `run_proof_keep` with `verbose: true` so Nova's own
   per-phase progress shows, write `<stem>.proof.json`, print `h_orig`,
   `h_edit`, proof size and elapsed time.

`crop` needs a non-zero `z0`: it carries the window start in state element
2. Build `initial_state` from the prompted parameters rather than
defaulting to zero.

### C — verify

1. Prompt: signature path, proof path, edited audio path.
2. Load both bundles. Cross-check their `bit_depth`/`batch_size`/`samples`
   agree and that `sig.h_orig == proof.h_orig`; a mismatch means they
   describe different runs, which is reported as a failure, not a crash.
3. Re-derive `pp` + `vk` from `artifacts/<circuit_name>.r1cs` (warn that
   this takes ~30s), then `verify_compressed`.
4. Recompute `h_edit'` from the edited audio on disk and compare against the
   proof's terminal state.
5. Verify the signature over `sig.h_orig` with the embedded public key.
6. Print a per-check table and an overall verdict, exit code 0/1 only if a
   `--once` style invocation is added later; interactively, return to the
   menu.

```
proof            OK   (CompressedSNARK verified, 400 folds)
edited audio     OK   (recomputed h_edit matches the proof)
signature        OK   (over h_orig, key from bundle)
VERDICT: the published audio is provably gain_k2 of a signed original
```

Each check reports independently, so a failure says which property broke.

## Honesty requirements

The CLI must not overstate what it proves.

- Option A prints that it is a **mock**. The signature means "whoever held
  this key signed this commitment"; the trust model assumes an attested
  capture device, which this is not.
- Option C prints that the embedded public key is **not** a trust anchor: a
  forger ships their own key and their own signature, and every check still
  passes. Real verification compares the key against an out-of-band source.
  The verdict wording must say "signed by the key in the bundle", not
  "authentic".
- `crop`'s verdict notes that the excerpt is not bound to a larger original.

## Testing

Logic is separated from prompting, so:

- `bundle.rs` round-trips both JSON formats.
- A small end-to-end test at `bd=8, B=10` (the cheap compiled circuit)
  runs sign → prove → verify through the library functions, asserting the
  verdict is positive and that flipping one sample in the edited WAV makes
  option C fail with the edited-commitment check, not a crash.
- A test that a missing circuit produces the compile hint rather than a
  panic.

The prompting layer itself is not unit-tested; it is thin by design.

## Open risks

- **C requires `artifacts/` to be populated**, since `vk` is re-derived
  from the r1cs. A verifier must therefore hold the circuit. That is
  inherent rather than incidental — the circuit *is* the statement being
  verified — but it means C cannot check a bundle naming a circuit the
  verifier does not have. Report that clearly instead of failing obscurely.
- **Proving is slow and not cancellable mid-fold.** The confirmation step
  and the prediction are the mitigation.
- Nova's `verbose` output goes to stdout and is not structured; the CLI's
  own progress lines should be distinguishable from it.
