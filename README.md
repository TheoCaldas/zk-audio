# zk-audio

Proving that an audio edit was correctly derived from an original recording
**without revealing the original samples**, using recursive zk-SNARKs
(Nova/IVC).

The relation proven is, for a publicly agreed effect `f`:

> for every sample of the original, the corresponding sample of the edited
> stream equals `f(original)`

while the only things ever published are a succinct proof and a running
**Poseidon commitment** to each stream — never the audio itself. A separate
ECDSA-P256 layer signs the commitment to the original, so a verifier learns
both "this edit is correct" and "the original came from the key holder".

---

## Table of contents

- [How it works](#how-it-works)
- [Quick start](#quick-start)
- [The CLI](#the-cli)
- [What `main.rs` does](#what-mainrs-does)
- [Repository layout](#repository-layout)
- [Poseidon parity: the Rust and Circom hashes MUST agree](#poseidon-parity-the-rust-and-circom-hashes-must-agree)
  - [The proving field is Vesta, not BN254](#1-the-proving-field-is-vesta-not-bn254)
  - [There are two batch-digest constructions](#2-there-are-two-batch-digest-constructions-not-one)
  - [Rules for changing any of this](#3-rules-for-changing-any-of-this)
- [Edits](#edits)
- [Benchmarks](#benchmarks)
- [Testing](#testing)
- [Design notes](#design-notes)
- [Lengths that are not a whole number of folds](#lengths-that-are-not-a-whole-number-of-folds)
- [Trust model](#trust-model)
- [Current state and gaps](#current-state-and-gaps)

---

## How it works

Samples are packed off-circuit (many per field element), fed to a Circom
step circuit one batch per Nova fold, and unpacked in-circuit. Each fold
checks the edit relation over its batch and folds a Poseidon digest of the
batch into a running commitment carried as the public IVC state. After the
last fold the recursive proof is compressed once into a succinct
`CompressedSNARK`.

The full protocol:

```
  CREATOR                                  VERIFIER
  1. commit    h_orig = Poseidon(original)
  2. sign      sigma  = ECDSA(SHA256(h_orig))
  3. prove     pi, (h_orig*, h_edit*) = Nova(original, edited)
                       -- publishes (edited, pi, h_orig, sigma) -->
  4. verify proof                         check pi
  5. recommit                             h_edit' = Poseidon(edited)
  6. verify sig                           check sigma over h_orig
```

Steps 5–6 are what make the proof mean anything. The proof alone says "some
pair of streams stands in the gain relation"; binding it to the *published*
artefact requires the verifier to recompute `h_edit'` from the audio it
actually holds and check it against the proof's terminal IVC state. That
comparison is only possible because the Rust and Circom Poseidons agree —
see [Poseidon parity](#poseidon-parity-the-rust-and-circom-hashes-must-agree),
which is the most important section in this file.

`src/protocol.rs` implements this: `create_provenance` does 1–3,
`verify_provenance` does 5–6, and the proof is verified inside `run_proof`.

**What it establishes:** the original was signed by the holder of the
signing key, the edit is exactly the proven relation, and the edited audio
the verifier holds is the one the proof was about.

How much that first clause is worth depends on where the public key came
from — see [Trust model](#trust-model). The signing layer here is a mock;
the ZK edit-correctness half is the real contribution.

---

## Quick start

### Prerequisites

| tool | why |
|---|---|
| Rust (edition 2024) | the prover/verifier |
| [circom](https://docs.circom.io) 2.2+ | compiles the step circuits |
| Node.js | runs the generated wasm witness generator |
| Python 3 | fixture and code generators |
| `snarkjs` *(optional)* | only to inspect witnesses by hand |
| `openssl` | generating signing keys |

### Setup

**Almost everything the demo needs is gitignored** — `circuits/circom/`
(vendored circomlib), `raw_audios/`, `signers/`, `artifacts/` and
`serialized/` are all excluded from git, so a fresh clone cannot run
anything until they are built.

One script does all of it:

```bash
./setup.sh
```

It is idempotent — re-running skips whatever is already in place — and it:

1. **Checks the prerequisites** up front, so a missing tool fails with a
   clear message instead of a cryptic error three steps later.
2. **Fetches circomlib** into `circuits/circom/`. The circuits
   `include "circom/poseidon.circom"`, and this is *also* the source
   `scripts/gen_poseidon_constants.py` reads, so the Rust round constants
   and the circuits' constants provably come from one place.
   **Pinned to 2.0.5 and verified** — see the warning below.
3. **Generates the WAV fixtures** (`scripts/gen_*_fixture.py`).
4. **Generates a signing keypair** via `generate.keys.sh`.
5. **Compiles the circuits** `main.rs` and the end-to-end tests need.
6. **Re-runs the code generators** and warns if that changes a committed
   file, which would mean the Rust reference had drifted from the circuits.

```bash
./setup.sh --force           # rebuild even what already exists
./setup.sh --circuits-only   # just recompile the default circuits
./setup.sh --help
```

> **One manual step.** `generate.keys.sh` names keys by *today's* date, so
> your keypair will be `p256_<DDMMYY>` while `src/main.rs` has a fixed
> `key_id`. `setup.sh` detects the mismatch and prints the one-line `sed`
> to fix it.

> ⚠️ **circomlib must be 2.0.5.** Version 2.0.2 ships a *different*
> Poseidon — decimal constants, no `POSEIDON_S`, a `t=2` case — whose
> digests do not match, and which `gen_poseidon_constants.py` cannot parse
> at all. `circuits/package.json` pins 2.0.5 and `setup.sh` hard-fails on
> anything else, because getting this silently wrong would change every
> commitment in the system.

### Run

```bash
cargo run --release
```

**Use `--release`.** The default configuration proves `gain_k2` over
600,000 samples = **400 Nova folds** against the production circuit
`gain_b16_k2_B100` (144,299 constraints per step); debug builds are
impractically slow. Measured on an M-series laptop:

| phase | time |
|---|---|
| `RecursiveSNARK` creation (400 folds) | ~111 s |
| `RecursiveSNARK::verify` | ~0.22 s |
| `CompressedSNARK::prove` | ~15 s |
| `CompressedSNARK::verify` | ~0.20 s |

Peak RSS ~760 MB.

```
Final original hash: 0x3876afb0795b382c632229380985d61f43a3e6173ce30618ee1f42b069fc41cd
Final edited hash:   0x037f8cf88f0040e02a08c635061de26baad15b735c03d077f7d787e14965287d

=== Verifier: verify proof -> recompute hash -> verify signature ===
edited-stream commitment matches the proof's terminal state
signature over the committed original verifies

PROTOCOL OK: the published edit is provably gain_k2 of an original
signed by the key holder, and the original was never revealed.
```

Exits non-zero if verification fails, so it is usable from a script.

---

## The CLI

An interactive tool covering the roles in the protocol:

```bash
cargo build --release
./target/release/zk-audio-cli
```

```
  A) mock-sign a source
  B) prove an edit
  C) verify a bundle
  D) prove a chain of edits
  E) verify a chain bundle
  exit
```

**A — mock-sign.** Computes the Poseidon commitment `h_orig` to a source
stream and signs it. Prompts for bit depth and batch size, because
`h_orig` is a fold over the partition those fix, and the signature is only
recomputable by a verifier that knows them. Also prompts for a sample
count, defaulting to the whole file: the length is part of the signed
statement, so signing exactly what will be proven is what makes a short
proof verifiable. Writes `<stem>.sig.json`.

**B — prove an edit.** Pick an operation from the provable edits, choose bit
depth and batch size, pick how many samples to prove, and confirm after
seeing the predicted time. Writes the edited WAV and `<stem>.proof.json`
(~24 KB). The two-input relations (`merge`, `concat`) prompt for a second
source and fold both streams in lockstep, capped at the shorter one.

**C — verify a bundle.** Takes a proof, a signature per source and the
edited audio, then reports each property separately:

```
bundles agree        OK        0ms   same h_orig, bd=8, B=10, N=110250
proof                OK       3.1s   verified, 356 steps
bundle commitments   OK        0ms   h_orig and h_edit match the proof's terminal state
edited audio         OK       52ms   recomputed h_edit matches (110250 samples, padded to 110360)
signature            OK        0ms   valid over (h_orig, N=110250), using the key in the bundle
verifier key                  2.2s   derived from the circuit
total                         5.3s
```

A merge adds a check per source, since the statement names two of them:

```
bundle commitments   OK        0ms   h_a, h_edit and h_b match the proof's terminal state
signature A          OK        0ms   valid over (h_orig, N=110250), using the key in the bundle
signature B          OK        0ms   valid over (h_orig, N=110250), using the key in the bundle
source B bound       OK        0ms   the signed source B is the stream the proof folded
```

Every step is timed. Nearly all of the wall clock is the proof check plus
deriving the verifier key, which scales with circuit size; recomputing the
commitment stays in the low hundreds of milliseconds even over 150,000
samples.

Alter one byte of the edited audio and `edited audio` fails while `proof`
still passes — the proof is about the original pair; the commitment is what
binds it to the published file.

Two things to know:

- **A proof over part of a signed stream does not verify.** When the signed
  and proven lengths differ, C fails the `bundles agree` check and reports
  both fold counts. This is a real limitation of the scheme, not of the CLI:
  binding a prefix proof to a full commitment would need the original's fold
  carried past the proven window. Answer A's sample-count prompt with the
  number you intend to prove.
- **C re-derives the verifier key from the circuit**, so `artifacts/` must
  hold the circuit named in the bundle. That is inherent — the circuit *is*
  the statement. A verifier key shipped inside a bundle would prove nothing,
  since a forger supplies both; it is also ~28 MB against a 24 KB proof.

- **A concat proof does not bind its sources to signed originals.** Concat
  gates its source chains, so `h_a` commits to A's samples below the join
  and `h_b` to B's from it on — spans, not whole streams. A signature over
  a whole source therefore cannot equal either, by construction, so C
  reports `bundles agree` as *not applicable* and says in the verdict that
  the signatures establish only that those keys signed *some* stream. What
  a concat proof does establish is the join itself: the output is exactly
  the samples in `h_a` followed by those in `h_b`, boundary included, since
  `len_a` travels in the IVC state and the verifier reads it back.

All eleven production edits are offered: the nine single-input ones at 8,
16 and 24 bits, plus `merge` and `concat` at 8 bits. Not listed: `gain2`,
the pre-sponge baseline, and the ablation variants, several of which are
deliberately unsound.

**D — prove a chain of edits.** More than one operation on the same input:
each stage proves its own edit, and the stages are linked by requiring one
stage's output commitment to be the next one's input. Bit depth and batch
size are chosen once for the whole chain, because a commitment is a fold
over a partition and stages under different partitions could not link.
Writes one `<stem>.chain.sN_<edit>.proof.json` per stage plus a
`<stem>.chain.json` manifest naming them in order.

`crop` and `merge` are offered only as the **last** stage and `concat` not
at all; the prompt refuses the choice rather than discovering it after
minutes of folding. See [docs/edit_chains.md](docs/edit_chains.md).

**E — verify a chain bundle.** Takes the manifest, a signature over the
source, and the published audio:

```
one partition            OK    every stage folds bd=8/B=10
manifest ends            OK    the bundle's h_orig and h_edit are the first and last stages'
stage links              OK    1 links hold: each stage consumed what the previous produced
source bound             OK    the signed source is the chain's input (N=620)
stage 1 gain_k2          OK    verified, 2 steps, commitments match
stage 2 atten_m1         OK    verified, 2 steps, commitments match
published audio          OK    recomputed h_edit matches the last stage (620 samples)
signature                OK    valid over (h_orig, N=620), using the key in the bundle
```

`stage links` is the check that makes a chain a chain. Reorder the stages
and it fails — along with `source bound` and `published audio` — while
both stage proofs still verify individually, which is exactly the attack
it exists for: valid proofs about unrelated streams presented as a chain.

The manifest stores **no links**; they are recomputed from the stage
proofs, since a manifest asserting a link its proofs do not show is the
one thing a verifier must not trust. Cost is the sum of the stages, one
verifier key and one proof check each.

## What `main.rs` does

A single-run demo of the whole protocol: one audio file, one configuration,
both sides of the transaction in one process. (For sweeps, use
[`--bin bench`](#benchmarks).)

Configuration is hardcoded at the top rather than parsed — this is a demo,
not a CLI:

```rust
bit_depth = 16;  batch_size = 100;  max_size = 600000;
edit_name = "gain_k2";  input_name = "sweep_120s_2c_44sr_16bd";
```

`gain_k2` is the **production** relation — `clamp(2x)`, proven by
`circuits/edits/gain/gain_step.circom`, committing to both streams with the
rate-4 sponge. The legacy `gain2` (in `circuits/_old`, unclamped, arity-2
chain) is kept only as the benchmark baseline.

**Setup:**

1. **Derive the circuit name** from the edit registry — `gain_k2` →
   `gain_b16_k2_B100` — locating the `.r1cs` and wasm witness generator in
   `artifacts/`. Names come from `circuit_name_for`, not an inline
   `format!`, because each family encodes its parameter differently
   (`gain_b16_k3_B100`, `atten_b8_m1_B100`).
2. **Pick the hash construction** via `BatchHashKind::for_circuit`. This
   line is load-bearing: the current families use the rate-4 sponge, but
   the legacy `multiplier_*` stems use the arity-2 chain. Getting it wrong
   yields a valid-looking but wrong commitment.
3. **Read and slice** the audio to exactly `max_size` samples, then pad to
   a whole number of steps. One step covers `batch_size × comp_size` =
   100 × 15 = 1,500 samples, so 600,000 samples is **400 folds**; an
   `max_size` that is not a multiple is silence-padded up to one (see
   "Lengths that are not a whole number of folds").
4. **Apply the edit** through the registry, to the *padded* stream.
5. **Serialize** both streams to `serialized/<circuit>_N<n>.json` as packed
   hex field elements.
6. **Load the keypair** from `signers/`.

**Then the protocol**, split into two halves deliberately:

- **Creator** — `create_provenance`: commit, sign `SHA256(h_orig, N)`, prove
  (400 folds + `CompressedSNARK`), and cross-check its own commitment
  against the circuit's terminal state.
- **Verifier** — `verify_provenance` receives the edited audio, the bundle
  and the **public** key, and never the original samples. That argument
  list *is* the privacy claim, expressed in types.

On success it writes the edited stream to
`raw_audios/output/<input>_<edit>.wav`, inheriting the input's sample rate,
channel count and bit depth. **Written only after verification passes**, so
what lands on disk is always audio whose provenance checked out — this is
the artefact a creator would publish alongside `(h_orig, sigma, pi)`. The
proof binds to these exact samples via `h_edit`, so re-encoding or trimming
the file invalidates it.

### Changing what it proves

Edit the constants and make sure a matching compiled circuit exists
(`./setup.sh --circuits-only` compiles the defaults; `./compile.circuit.sh
<path>` does one). Switching between a current family and the legacy
`gain2` also switches the hash construction; `hash_kind` follows
automatically, which is why it is derived rather than hardcoded.

**Fixture amplitude is not cosmetic.** The default
`sweep_120s_2c_44sr_16bd` is capped at ¼ full scale, so under `gain_k2`
**0 % of its samples saturate** — the clamping branch is never taken. That
is the right pairing for the unclamped `gain2` baseline, where full-scale
audio would overflow `i16` and stop matching the circuit, but it means the
default run does not exercise clamping.

To exercise it, switch to the fullscale fixture, where **33.4 % of samples
saturate** under ×2:

```rust
let input_name = "sweep_31s_2c_44sr_16bd_fullscale";
```

Both are generated by `setup.sh`. See
[`scripts/README.md`](scripts/README.md) for the full fixture matrix.

---

## Repository layout

```
├── src/
│   ├── main.rs            — the single-run protocol demo
│   ├── protocol.rs        — commit/sign/prove/verify, end to end
│   ├── poseidon/          — circomlib Poseidon generic over the field
│   │   └── constants.rs   — GENERATED by scripts/gen_poseidon_constants.py
│   ├── circuit.rs         — Nova driver: RecursiveSNARK + CompressedSNARK
│   ├── audio_parser.rs    — WAV I/O, packing/serialization, stream_commitment
│   ├── audio_editor.rs    — off-circuit reference implementations of the edits
│   ├── signer.rs          — ECDSA P-256 sign/verify (PKCS#8 PEM)
│   ├── witness.rs         — in-process wasm witness generation
│   ├── circom_bridge/     — r1cs loading + Nova circuit shim
│   ├── bench/             — benchmark suite: edit registry, config, reporting
│   └── lfo_table.rs       — GENERATED by scripts/gen_lfo_table.py
├── circuits/
│   ├── edits/<family>/    — CURRENT step circuits, one dir per edit family
│   ├── edits/_baseline/   — pre-sponge versions, kept for comparison
│   ├── hasher.circom      — Hasher, BatchHasher (arity-2 chain; legacy)
│   ├── hasher_sponge.circom — SpongeBatchHasher (rate-4; what current circuits use)
│   ├── hasher_wide.circom — WideBatchHasher, arity-N tree (benchmark variant)
│   ├── decompressor.circom — unpacks one field element into N sub-samples
│   ├── circom/            — vendored circomlib (gitignored; see Setup)
│   └── _old/              — superseded. NOTE multiplier_step_b16.circom here is
│                            still live as the `gain2` relation.
├── setup.sh               — one-shot setup for a fresh clone
├── compile.circuit.sh     — compile one circuit (--prime vesta; see Poseidon parity)
├── generate.keys.sh       — ECDSA P-256 keypair
├── scripts/               — fixture + code generators, plotting
├── tests/                 — poseidon_parity.rs, protocol_end_to_end.rs, ...
├── artifacts/             — compiled .r1cs / .sym / _js  (gitignored)
├── serialized/            — serialized circuit inputs    (gitignored)
├── raw_audios/            — WAV fixtures, and output/ for edited streams (gitignored)
├── signers/               — PEM keypairs                 (gitignored)
└── logs/bench/            — benchmark results
```

---

## Poseidon parity: the Rust and Circom hashes MUST agree

**Read this before touching anything hash-related.** A verifier recomputes a
stream commitment off-circuit and compares it against the proof's terminal
IVC state, so a Rust/Circom disagreement is indistinguishable from a
forgery. Two things have to be right, and both have bitten this codebase.

### 1. The proving field is Vesta, not BN254

`compile.circuit.sh` passes **`--prime vesta`, and that is not optional.**
`circuit.rs` folds with `PallasEngine` as the primary curve, and a Nova step
circuit is expressed over the primary curve's *scalar* field — which is
`halo2curves::pasta::fq::Fq`, modulus:

```
0x40000000000000000000000000000000224698fc0994a8dd8c46eb2100000001
```

That modulus *is* the Vesta prime. The field is forced by the curve cycle:
Nova's IVC verifies each step inside a circuit over the other curve's field,
which requires a **2-cycle** of curves. BN254 alone is not a cycle (its
partner is Grumpkin); Pasta is Nova's default cycle. Compiling under the
default BN254 prime produces a circuit Nova cannot load.

**The prime also fixes the Poseidon digest.** circomlib's round constants
are literal hex that circom reinterprets modulo whichever prime it is given,
so the *same circuit source* hashes differently per `--prime`.
`Poseidon(2)([1,2])` is:

| field | digest |
|---|---|
| **Vesta** (what the circuits use) | `10148246943864975455840209516398831844995242484352636702637979101131422116154` |
| BN254 | `7853200120776062878684798364095072458815029376092732009249414926327459813530` |

This caused a long-standing "the two sides give different hashes" problem:
the Rust helpers used `light-poseidon` + `ark_bn254::Fr`, i.e. the wrong
field. **Neither library was buggy.** `ark-bn254`, `ark-ff` and
`light-poseidon` have been removed from `Cargo.toml`; do not reintroduce
them for hashing.

**`src/poseidon/` is the fix:** a port of circomlib's `PoseidonEx` generic
over `ff::PrimeField`. Round constants live in `src/poseidon/constants.rs`,
generated by `scripts/gen_poseidon_constants.py` from
`circuits/circom/poseidon_constants.circom` (circomlib 2.0.5), and are
stored as **big-endian hex strings, not limbs**. That is the whole
mechanism: the same string decodes to a different field element per field,
exactly as circom reinterprets the literal. One implementation therefore
reproduces *both* fields' reference digests — verified in
`tests/poseidon_parity.rs`, where Vesta matches a circom witness and BN254
matches `light-poseidon`.

Instantiate commitments at `<PallasEngine as Engine>::Scalar`.

### 2. There are TWO batch-digest constructions, not one

Their digests differ, and a commitment is only meaningful alongside the one
that produced it:

| construction | circom | used by |
|---|---|---|
| rate-4 sponge | `SpongeBatchHasher(width, 4)` | every current `circuits/edits/<family>/*_step.circom` |
| arity-2 chain | `BatchHasher(width)` | legacy `multiplier_*` stems — which is what **`gain2`** compiles to — and everything under `circuits/edits/_baseline/` |

The chain seeds with `batch[0]` rather than a zero capacity, so a
one-element batch hashes to itself; that is *why* the two differ even at
rate 1.

**Use `poseidon::BatchHashKind::for_circuit(circuit_name)`** rather than
defaulting to the sponge; it routes `multiplier*` to `Chain` and everything
else to the sponge. Caveat: `_baseline` circuits compile to the *same stems*
as current ones, so proving against `_baseline` artefacts requires passing
`BatchHashKind::Chain` explicitly.

This is not hypothetical — assuming "the current default" produced a real
`OriginalCommitmentMismatch` when the protocol was first wired up against
`gain2`.

### 3. Rules for changing any of this

- **Expected hash vectors must come from a real circom witness** — compile,
  `node generate_witness.js`, `snarkjs wtns export json` — and **never**
  from what Rust currently prints. A vector "fixed" by pasting Rust's output
  defeats the entire test.
- `tests/poseidon_parity.rs` pins the digests, including
  `proving_field_is_vesta`: if that fails, the curve cycle changed and every
  other vector is meaningless.
- `tests/protocol_end_to_end.rs` pins the thing that matters: a
  Rust-recomputed commitment equals the circuit's in-circuit fold, bit for
  bit, for **both** constructions.
- Changing a circuit's hashing changes its digest, so the Rust reference
  must move in lockstep. Re-run `scripts/gen_poseidon_constants.py` after a
  circomlib upgrade.

---

## Edits

Each family has an off-circuit reference in `audio_editor.rs`, a step
circuit in `circuits/edits/<family>/`, and a registry entry in
`bench/config.rs` tying them together. **Every edit floors rather than
truncates, and the Rust reference must mirror the circuit exactly** —
several tests check this exhaustively at 8-bit.

The nine production edits are compiled at every depth for `B=100`, which
is the matrix the CLI offers; other batch sizes exist only where a
benchmark needed them.

| edit | relation | depths (at B=100) |
|---|---|---|
| `gain_k2`, `gain_k3`, `gain_k4` | `clamp(k * x)`, saturating | 8, 16, 24 |
| `atten_m1`, `atten_m2` | `x >> m`, i.e. `k = 1/2^m` | 8, 16, 24 |
| `dist_d50`, `dist_d100` | rectifying distortion at a wet blend | 8, 16, 24 |
| `tremolo_f5` | LFO amplitude modulation (table-driven) | 8, 16, 24 |
| `crop` | a window, with a proven sample counter | 8, 16, 24 |
| `concat`, `merge` | two input streams | 8 |
| `gain2` | `x * 2`, **no clamping** (the pre-sponge baseline) | 16 |

`gain2` is kept for benchmark comparison and is **not offered by the
CLI**: it absorbs with the arity-2 chain rather than the rate-4 sponge
(see [Poseidon parity](#poseidon-parity)), so its commitments are not
interchangeable with the production circuits'. `concat` and `merge` are
offered; both prompt for a second source, and a concat proof commits to
spans of its sources rather than to whole streams.

The `gain_k2_{nohash,wide,wide4,wide8,onehash,sponge4,sponge8,sponge2h}`
entries are **hashing ablations for the optimisation study**, not usable
relations. Several are deliberately unsound — `gain_nohash` drops the
commitment entirely, and `gain_sponge4`/`gain_onehash` commit to only one
stream, which severs the binding between the proof and the published
artefact. Each such circuit says so in its own header. The sound sponge
variant is `gain_sponge2h`.

### Chaining several edits

`src/chain.rs` composes edits sequentially — one proof per stage, linked
by requiring stage `k`'s edited-stream commitment to equal stage `k+1`'s
original-stream commitment. Both are computed in-circuit, so nothing
off-circuit sits in the seam and `stream_commitment` needs no change.

`gain`, `atten`, `dist` and `tremolo` compose anywhere; `crop` and
`merge` are allowed only as the last stage, and `concat` is not supported
as a stage at all. Cost is the sum of the stages plus one
`CompressedSNARK` per stage, which on short audio is over half the wall
time — the baseline a fused composite circuit has to beat. See
[docs/edit_chains.md](docs/edit_chains.md) for the measured figures and
the reasons for the per-edit restrictions.

---

## Benchmarks

```bash
cargo run --release --bin bench -- --max-n 6000 --batch-sizes 10,100
cargo run --release --bin bench -- --help
```

Writes `results.jsonl`, `results.csv` and a `summary.md` per run under
`logs/bench/`. Plot with `scripts/plot_bench.py` (needs matplotlib — see
[`scripts/README.md`](scripts/README.md) for which interpreter).

Headline results from the optimisation study:

- **Sample packing** — 15× 16-bit samples per field element, unpacked
  in-circuit. One fold covers 1,500 samples while marshalling only 100 field
  elements through the witness generator, which matters because JSON
  (de)serialization and per-step witness generation are real bottlenecks at
  scale, not just constraint count.
- **Rate-4 sponge over the arity-2 chain** — `ceil(B/4)` permutations
  instead of `B-1`: −59 % hashing constraints at B=100, −13.8 % end-to-end
  prove time. A tree of arity `a` still needs ~`B/(a-1)` calls because the
  lower layers must themselves be hashed; a sponge carries the state
  forward and needs no second layer. Measured at B=100:

  | construction | Poseidon calls | constraints |
  |---|---|---|
  | `BatchHasher`, arity-2 chain | 99 | 51,183 |
  | tree, arity 4 | 35 | 25,760 |
  | sponge, rate 4 | 25 | 18,400 |
  | sponge, rate 8 | 13 | 15,223 |

  **Security note.** circomlib's `Poseidon(n)` is a fixed-length hash with
  a standard-conformant domain tag. Chaining it as `SpongeBatchHasher`
  does is Merkle–Damgård over that compression function, not the sponge
  from the Poseidon paper, which has a defined capacity and padding rule.
  It is collision resistant on the same assumption `BatchHasher` already
  relies on — that `Poseidon(t)` is a collision-resistant compression
  function — and the input length is fixed at compile time, so there is no
  padding ambiguity. It is not interchangeable with a spec Poseidon
  sponge.
- **IVC folding** — proving cost never materialises the whole file's
  constraint system; arbitrary-length audio just means more folds.
- **Final compression** — `CompressedSNARK` (Spartan + IPA-PC) cost is
  driven by per-step circuit size, not chain length, which is exactly the
  property IVC should give you.

Written analyses live in `docs/analyses/` (gitignored).

---

## Testing

```bash
cargo test --release
```

`--release` matters: `tests/protocol_end_to_end.rs` and
`tests/chain_end_to_end.rs` run real `RecursiveSNARK` proofs.

| suite | what it covers |
|---|---|
| `tests/poseidon_parity.rs` | Rust/Circom hash agreement, pinned to circom witnesses |
| `tests/protocol_end_to_end.rs` | the full protocol over real proofs, plus tamper cases |
| `tests/chain_end_to_end.rs` | multi-stage chains over real proofs: the seam, broken links, terminal crop/merge |
| `src/audio_editor.rs` unit tests | each edit against its circuit, exhaustively at 8-bit |
| `src/signer.rs` unit tests | sign/verify round trips |

The end-to-end tests **skip with a printed `SKIP:` line** if the gitignored
artefacts or fixtures are missing, so a fresh clone stays green without
passing silently — which also means a green run proves nothing until
`./setup.sh` has built `gain_b8_k2_B10` and the 8-bit fixture. Check for
`SKIP:` in the output if in doubt.

---

## Design notes

- **Normalized sample space.** Samples reach the circuits as
  `x + 2^(bd-1)`, so they are never negative — field elements have no sign,
  and the comparators need non-negative inputs. The clamping edits then work
  in an *offset* space and subtract the surplus; multiplying normalized
  values directly would be wrong. `gain_step.circom` explains the
  derivation.
- **Poseidon, not SHA256, inside the circuit** — algebraic and far cheaper
  in constraints. SHA256 is kept only for the off-circuit signing layer,
  where circuit-friendliness is irrelevant.
- **Generated code has one source of truth.** `src/poseidon/constants.rs`
  and `src/lfo_table.rs` are generated, as is
  `circuits/edits/tremolo/tremolo_table.circom`. Edit the generator, not the
  output.
- **Separation of concerns.** Signature authenticity and ZK edit-correctness
  are independent layers; conflating them inside the SNARK would be far more
  expensive. The cost is the gap below.

---

## Lengths that are not a whole number of folds

A compiled circuit folds a fixed-width step: `batch_size × comp_size`
samples, 1,500 at bd=16/B=100. Nova folds that one shape repeatedly, so a
stream whose length is not a multiple of it has no witness for its final,
short batch.

The stream is **silence-padded** up to the next fold boundary, and the true
sample count `N` is signed alongside the commitment.

**Why the edit runs on the padded stream, not the other way round.** The
relation is enforced on every sample the circuit folds, the pad included,
so the padded original is edited and the result is what the proof is about.
Padding both streams with zeros instead would be wrong for any edit whose
`edit(0)` is not `0` — `distort(0)` is `-2^(bd-1)/2`, so a zero-padded
edited stream violates the relation and the circuit rejects it.

**Why `N` is signed.** The commitment covers the padded stream, so every
length in `(N_padded - samples_per_step, N_padded]` shares a commitment.
Trimming the published file inside that window changes nothing a verifier
could recompute. The signature is therefore over
`"zk-audio/1|" + h_orig + "|" + N` (`protocol::commitment_statement`), and
the verifier checks the published length against it before anything else.
`tests/protocol_end_to_end.rs` pins exactly that attack:
`truncating_inside_the_pad_is_caught_by_the_length` asserts the trimmed
stream still recomputes to `h_edit`, and that verification rejects it
anyway.

**What the published artefact holds.** The WAV carries `N` samples; the
padding exists only inside the commitment and the witness. A stereo stream
must still end on a frame, so `N` has to be a multiple of the channel
count — `write_file_at` asserts it.

**The protocol establishes**: "these `N` samples, silence-extended to a
fold boundary, are the sanctioned edit of a stream committed as `h_orig`."

## Trust model

**The signature layer is mock-only, and deliberately so.** `signer.rs`
generates its own keypair with `openssl` and signs off-circuit. That is a
stand-in for the real thing, not an approximation of it.

The protocol assumes the verifier obtains the public key through a **secure
out-of-band disclosure of the source** — an attested capture device
(a camera, a recorder, a signing DAW) whose key is published by the
manufacturer or a trusted registry. Under that assumption there is no
in-circuit signature check to add: the key identifies the device, the
device signs `h_orig` at capture time, and a verifier who trusts the
attestation chain and sees a valid signature over `h_orig` knows the
original came from that device. The proof then shows the published audio
is a sanctioned edit of exactly that committed original.

What this repository demonstrates is the **ZK edit-correctness half** — the
part that is novel and expensive. The signing half is a placeholder for an
attested-capture system that is out of scope here; swapping `signer.rs` for
a real device key changes nothing about the circuits or the commitments.

Consequently, do not read `./generate.keys.sh` as part of the security
story. It exists so the demo runs end to end.

## Current state and gaps

- **Each `(edit, bit_depth, batch_size)` needs its own compiled circuit**,
  since Circom requires concrete sizes. `artifacts/` therefore holds many
  near-identical circuits and `circuit_name_for` is the single namer.
- **circomlib's Poseidon constants were generated for BN254.** Using them
  over Vesta is structurally sound and matches the circuit exactly — which
  is all the protocol needs, since both sides agree — but they are not
  constants generated *for* this field, so they lack the parameter-
  generation provenance a cryptographic review would want. Inherited from
  circomlib-on-Nova generally. Relatedly, `hasher_sponge.circom` notes that
  its construction is Merkle–Damgård over circomlib's fixed-length Poseidon
  rather than the spec sponge.
