# Running the Benchmark Suite

How to run `bench`, what the flags do, and how to read the results.

---

## TL;DR

```bash
# 1. Build once (always --release: a debug build is ~50x slower)
cargo build --release --bin bench

# 2. See what would run, without proving anything
./target/release/bench --max-n 600000 --dry-run

# 3. Quick real run (~2 seconds)
./target/release/bench --max-n 600 --batch-sizes 10,40

# 4. The full sweep (~70 min)
./target/release/bench --max-n 600000
```

Results land in `logs/bench/run-<timestamp>/`. Start with `summary.md`.

---

## Before comparing against older runs

Two changes split the benchmark history. Numbers from either side of them
are not directly comparable.

**The batch hasher changed.** Every edit now absorbs each batch with a
rate-4 Poseidon sponge (`hasher_sponge.circom`) instead of the arity-2 chain
it used before — same relation, same `(h_orig, h_edit)` IVC state, ~30-60%
fewer constraints, **different commitment digest**. The pre-change circuits
are preserved verbatim under `circuits/edits/_baseline/`, so any older run
can still be reproduced; `circuits/CIRCUIT_INDEX.md` maps each logged
`circuit_name` to both the file that produced it and the file carrying that
name today.

**The proving stack changed.** `nova-snark` moved 0.23 → 0.76 and witness
generation moved in-process (`src/witness.rs`) instead of spawning `node`
per fold. Folding is ~28% faster; `CompressedSNARK::prove` is slower but
flat in N, and proofs are ~8% smaller.

Existing results are filed by which stack produced them:

```
logs/bench/
├── _baseline/      pre-optimisation results (arity-2 chain, nova 0.23)
├── _experimental/  optimisation-study runs
└── _old/           historical, no results.jsonl
```

New runs land in `logs/bench/run-<timestamp>/`, outside both — file them
deliberately, or pass `--out logs/bench/_experimental`.

---

## The two knobs

Everything revolves around two numbers:

| | Meaning |
|---|---|
| **B** (batch size) | Compressed field elements per Nova folding step. One element packs `comp_size` audio samples, so a step covers `B × comp_size` samples. |
| **N** (total samples) | How many audio samples get proven in total. Number of folding steps is `N / (B × comp_size)`. |

`comp_size` — how many samples fit in one field element — is fixed by the bit
depth, **not** by B: it is `15` at the default bd=16, but `31` at bd=8 and `9`
at bd=24 (see [Different bit depth](#different-bit-depth)). Every
`B × comp_size` below therefore means `B × 15` only at the default depth. The
code calls the product `samples_per_step`, and `--dry-run` prints it per cell
as `samples/st`.

The suite sweeps every valid combination of the two.

### Two ways to choose N

**By sample count** (`--max-n`) — the ladder, below.

**By audio duration** (`--durations`) — when the x-axis you want is seconds of
audio rather than abstract sample counts:

```bash
./target/release/bench --durations 10,30,60,120 \
    --input raw_audios/sweep_120s_2c_44sr_16bd.wav \
    --batch-sizes 100 --repeat 1
```

N is computed as `seconds × sample-rate × channels` and **replaces the ladder
entirely**. The two flags are mutually exclusive.

`--sample-rate` (default 44100) and `--channels` (default 2) define the
conversion. Both count toward N, because the circuit folds over the
**interleaved** stream — one second of 44.1 kHz stereo is 88,200 samples, not
44,100. The suite refuses to run if the WAV's real format disagrees with the
assumed one, rather than quietly proving a different length than it reports.

| Duration | N (44.1 kHz stereo) | iters at B=100 |
|---|---|---|
| 10 s | 882,000 | 588 |
| 30 s | 2,646,000 | 1,764 |
| 1 min | 5,292,000 | 3,528 |
| 2 min | 10,584,000 | 7,056 |

All four divide cleanly by every usable B, so none of them skip. Durations are
sorted ascending, so cheap cases still run first.

### N follows a fixed ladder

N always steps `60 → 600 → 6000 → 60000 → …`. You only choose where it stops:

```bash
--max-n 6000     # runs N = 60, 600, 6000
--max-n 600000   # runs N = 60, 600, 6000, 60000, 600000
```

`--max-n` is **required** — it's the single biggest driver of runtime, so there's
no default.

### N is rounded up to whole batches

A fold is a fixed-width step, so a case folds
`ceil(N / (B × comp_size))` batches: an N that does not divide evenly is
silence-padded up to the next boundary rather than skipped. `total_samples`
in the results is what was folded and `requested_samples` is what was asked
for, so a padded cell's cost is reported against the audio it actually
covers.

The matrix prints both, so a padded cell says what it actually measures:

```
   asked   folded     B samples/st    iters  status
      60      310    10        310        1  run
      60     3100   100       3100        1  run
     600      620    10        310        2  run
    6000     6200    10        310       20  run
```

An N below one batch pads up to a single fold and runs; what remains a
skip is a missing artefact or an input too short to cover the padded N.

`--dry-run` shows you this table before committing to a long run. **Always
dry-run first.**

> Compiled batch sizes depend on the edit. `gain_k2` at bd=8 has
> **B ∈ {10, 50, 100, 200, 400, 800, 1000, 1600}**; most other edits have
> **{10, 100}**. `ls artifacts/*.r1cs` shows what exists; a missing one is
> reported as a skip with the `compile.circuit.sh` line to create it.

---

## All the flags

| Flag | Default | What it does |
|---|---|---|
| `--max-n <N>` | **required\*** | Where the N ladder stops |
| `--durations <list>` | — | Audio lengths in seconds; replaces the ladder |
| `--n-list <list>` | — | Explicit sample counts; overrides ladder and durations |
| `--sample-rate <hz>` | `44100` | Rate assumed by `--durations` |
| `--channels <n>` | `2` | Channels assumed by `--durations` |
| `--batch-sizes <list>` | `10,40,100,200` | Comma-separated B values |
| `--bit-depth <n>` | `16` | 8, 16 or 24 |
| `--input <path>` | `raw_audios/2c_44sr_16bd_01.wav` | WAV to prove over |
| `--input-b <path>` | — | Second WAV; required by `merge` and `concat` |
| `--edit <name>` | `gain2` | Which edit relation to prove |
| `--out <dir>` | `logs/bench` | Where results go |
| `--repeat <n>` | `3` | Repetitions per case (for timing spread) |
| `--skip-compressed` | off | Recursive phase only — much faster |
| `--no-cache-inputs` | off | Delete each serialized JSON after use |
| `--dry-run` | off | Print the matrix and exit |
| `-h`, `--help` | | Show usage |

\* Exactly one of `--max-n`, `--durations` or `--n-list` is required.

---

## Recipes

**Check the plan before a long run**
```bash
./target/release/bench --max-n 600000 --dry-run
```

**Fastest sanity check** (~2 s) — proves the pipeline works end to end
```bash
./target/release/bench --max-n 600 --batch-sizes 10,40 --repeat 1
```

**Compare batch sizes cheaply** (~2 min) — skips the slow compression phase
```bash
./target/release/bench --max-n 60000 --skip-compressed
```

**One specific cell**, with full detail
```bash
./target/release/bench --max-n 6000 --batch-sizes 100 --repeat 1
```

**The full thesis sweep** (~70 min)
```bash
./target/release/bench --max-n 600000
```

**Duration scaling, 10 s to 2 min** (~5.5 h) — the real-audio-length axis
```bash
./target/release/bench --durations 10,30,60,120 \
    --input raw_audios/sweep_120s_2c_44sr_16bd.wav \
    --batch-sizes 100 --repeat 1 --no-cache-inputs
```
`--no-cache-inputs` matters here: the N=10,584,000 serialized JSON is ~100 MB,
and keeping all four costs ~180 MB of disk.

**Disk-constrained** — the N=600000 inputs are ~5.8 MB each
```bash
./target/release/bench --max-n 600000 --no-cache-inputs
```

---

## How long does it take?

Measured on an M2 Pro, release build. Per-cell times, recursive prove plus
compressed prove:

| N | B=10 | B=40 | B=100 | B=200 |
|---|---|---|---|---|
| 600 | ~2 s | ~2 s | — | — |
| 6 000 | ~7 s | ~5 s | ~7 s | ~13 s |
| 60 000 | ~62 s | ~32 s | ~23 s | ~29 s |
| 600 000 | ~10 min | ~5 min | ~3 min | ~3 min |

Whole-sweep totals over all four B values:

| `--max-n` | `--repeat 1` | `--repeat 3` (default) |
|---|---|---|
| 600 | ~5 s | ~15 s |
| 6 000 | ~40 s | ~2 min |
| 60 000 | ~3 min | ~9 min |
| 600 000 | ~25 min | ~70 min |

Duration-based cells at B=100, extrapolated from the ~274 µs/sample measured
above (`--repeat 1`):

| Duration | N | Expected prove |
|---|---|---|
| 10 s | 882,000 | ~4 min |
| 30 s | 2,646,000 | ~12 min |
| 1 min | 5,292,000 | ~24 min |
| 2 min | 10,584,000 | ~48 min |

Roughly 1.5 h for all four, plus serialization overhead — budget ~5 h, since
writing and witness-generating a 100 MB input is itself significant at this
scale.

`--skip-compressed` cuts a large share of this at small N, where the fixed
compression cost dominates.

Two things to know:

- **Smaller B is slower per sample at the same N.** B=10 needs 4000 folding
  steps for N=600000; B=200 needs only 200. More steps, more overhead.
- **Cases run cheapest-first**, so if something is broken you find out in
  seconds rather than an hour in.

---

## Reading the results

Each run creates a timestamped directory:

```
logs/bench/_baseline/run-1791754350/
├── summary.md       ← start here: pivot tables, human-readable
├── results.jsonl    ← one JSON object per case (the real data)
├── results.csv      ← same rows, flat, for spreadsheets
└── manifest.json    ← config + machine + git SHA (reproducibility)
```

### `summary.md`

Tables with N down the rows and B across the columns, one per metric:
recursive prove/verify, compressed prove/verify, proof size, plus two derived
columns that are the actually-comparable numbers:

- **`us per sample`** — proving cost normalized by audio length. This is the
  scaling claim.
- **`ms per iteration`** — should stay roughly flat as N grows at fixed B.
  That flatness *is* the IVC property, visible directly in the table.

It also carries a **circuit size** table (constraints per step, a property of B
alone), four **peak RSS** tables, and a **commitments** table.

### Memory tables — what they mean

Proving is memory-hungry, and the figure that matters is a *peak*, not an
end-of-phase reading: Nova's folding loop allocates and frees per step, so the
high-water mark happens inside `create_recursive_circuit` and is gone by the
time it returns. A background thread therefore samples process RSS every 50ms
and keeps the maximum per phase:

| Table | Phase sampled |
|---|---|
| Peak RSS, whole proof | max across all phases |
| Peak RSS during folding | `create_recursive_circuit` (includes the wasm witness generator) |
| Peak RSS during compression | `CompressedSNARK::setup` + `prove` together |
| Peak RSS over baseline | peak minus the RSS measured just before proving |

Measured at B=100, N=6000: ~706 MB during folding, ~817 MB during compression —
**the compression phase is the memory peak**, not the folding loop, and peak
scales with **B** (batch size), not with N. At B=10 the whole proof peaks around
280–330 MB regardless of N.

**Three caveats, all of which belong next to any number you quote:**

1. **Process-wide.** The figure covers the whole benchmark binary — the wasm
   witness generator's allocations and the parsed JSON input included — not the
   prover's working set in isolation.
2. **Sampled**, so a spike shorter than 50ms is missed. Every value is a
   *lower bound* on the true peak.
3. **"Over baseline" does not reproduce.** The allocator keeps freed pages, so a
   case that runs later in a sweep starts from a higher baseline and reports a
   smaller delta for identical work. The same (N=6000, B=100) cell measured a
   **35.9 MB** baseline run alone and **344.7 MB** as the third case of a sweep —
   while its absolute peak differed by only 2.6% (877.7 vs 854.9 MB).

   **So compare absolute peaks, not deltas.** To measure one cell's own
   footprint, run that cell alone.

RSS is resident pages, which the OS may reclaim under pressure — these are
measurements of a particular run, not a minimum spec. `manifest.json` records
whether sampling was supported and at what interval.

### Commitments table — what it does and doesn't prove

```
| N | B | original hash | edited hash | reps agree |
| 600 | 10 | 0x01fc62a89680… | 0x2b43c24221f1… | yes |
| 600 | 40 | 0x0778b7c642d8… | 0x1b4b7cff2ade… | yes |
```

**Hashes differ across B, and that is correct.** `BatchHasher` folds B elements
into one digest per step, so B determines the shape of the hash tree — identical
audio at a different B legitimately commits to a different value. Do not read
the two rows above as a disagreement.

What you *should* check:

- **`reps agree` = yes** — the same cell is reproducible across repetitions.
- **Cross-run, same (B, N)** — diff this table between two runs to confirm a
  circuit change preserved the relation. This is exactly how the optimized
  circuits were validated against the originals.

### `results.jsonl`

One object per line, appended and flushed as each case finishes — so a crash
three hours in doesn't lose the first three hours.

Plain Python, no dependencies:

```python
import json, statistics, collections

path = "logs/bench/_baseline/run-1791754350/results.jsonl"
rows = [json.loads(l) for l in open(path)]
ok = [r for r in rows if r["status"] == "ok"]

g = collections.defaultdict(list)
for r in ok:
    g[(r["total_samples"], r["batch_size"])].append(r["recursive_prove_ms"])

for k in sorted(g):
    print("N=%-7d B=%-4d median %8.1f ms" % (k[0], k[1], statistics.median(g[k])))
```

Or with pandas, if you have it:

```python
import pandas as pd
df = pd.read_json("logs/bench/_baseline/run-1791754350/results.jsonl", lines=True)
ok = df[df.status == "ok"]
ok.pivot_table(index="total_samples", columns="batch_size",
               values="recursive_prove_ms", aggfunc="median")
```

### Or use the scripts

`scripts/` has plotters and a cost model that already apply the same
median-and-half-range aggregation the suite's own `summary.md` uses, so
figures agree with it. See [scripts/README.md](../scripts/README.md).

```bash
# figures for one run -> <run>/plots/
/usr/bin/python3 scripts/plot_bench.py logs/bench/_baseline/run-1792898036

# predict a configuration without running it
python3 scripts/predict_proof_time.py --edit gain_k2 --bit-depth 8 \
    --batch-size 100 --duration 600
```

The plotters need matplotlib, which the default Homebrew `python3` does not
have; `/usr/bin/python3` does.

Each row also carries `peak_rss_bytes`, `rss_baseline_bytes`,
`peak_rss_public_params_bytes`, `peak_rss_recursive_prove_bytes` and
`peak_rss_compressed_prove_bytes`. These are `null` (not `0`) where the platform
offers no RSS query, so a missing measurement is distinguishable from a measured
zero. Supported on macOS and Linux.

Every row carries its own identity (`batch_size`, `total_samples`, `edit`,
`audio_input`, `bit_depth`, `repetition`), so rows stay meaningful out of
context. Skipped and failed cases appear too, with a `reason` or `error`, so a
gap in a table always has an explanation attached.

> **Timing caveat for comparisons with older logs:** `compressed_prove_ms`
> **excludes** `CompressedSNARK::setup`, which is now timed separately as
> `compressed_setup_ms`. The hand-written logs in `logs/26.*` reported the two
> summed, so those numbers are not directly comparable. `manifest.json` records
> `schema_version: 2` to mark this.

---

## Changing the audio or the edit

### Different audio

```bash
./target/release/bench --max-n 6000 --input raw_audios/test_48.16.01.wav
```

Must be 16-bit WAV. Cells needing more samples than the file has are skipped as
`InsufficientAudio` rather than crashing.

| Fixture | Format | Samples | Duration |
|---|---|---|---|
| `2c_44sr_16bd_01.wav` (default) | 44.1 kHz stereo | 705,600 | 8 s |
| `sweep_120s_2c_44sr_16bd.wav` | 44.1 kHz stereo | 11,025,000 | 125 s |
| `test_48.16.01.wav` | 48 kHz stereo | 384,000 | 4 s |
| `test_12.16.01.wav` | 12 kHz stereo | 96,000 | 4 s |

The default fixture covers N up to 600,000 but **not** any of the duration
targets — the shortest, 10 s, already needs 882,000 samples. Use
`sweep_120s_2c_44sr_16bd.wav` for duration runs; it is a deterministic
80 Hz→4 kHz log sweep plus dither, regenerable bit-identically with
[scripts/gen_bench_fixture.py](../scripts/gen_bench_fixture.py).

Its amplitude is capped at ±8000 (about a quarter of full scale) **on purpose**:
`gain2` is `x * 2` with no clamping on either side, so samples near full scale
would overflow `i16` in Rust and stop matching what the circuit enforces. Any
fixture you add for `gain2` needs the same headroom.

### Different edit

```bash
./target/release/bench --max-n 6000 --edit gain2
```

### Where the circuits live

The sources are organised by edit family; shared primitives stay at the root:

```
circuits/
├── hasher.circom          — Poseidon wrappers (shared)
├── hasher_sponge.circom   — SpongeBatchHasher: the DEFAULT batch hasher
├── hasher_wide.circom     — WideBatchHasher (tree); benchmark variant only
├── decompressor.circom    — Decompressor + Recompressor (shared)
├── circom/                — vendored circomlib
├── CIRCUIT_INDEX.md       — maps logged circuit_name -> source file
├── edits/
│   ├── gain/              — gain_step.circom + gain_b{8,16,24}_k*_B*.circom
│   │   └── _experimental/ — hashing variants from the optimisation study
│   ├── attenuation/ concat/ crop/ distortion/ merge/ tremolo/
│   └── _baseline/         — pre-optimisation copies of every circuit above,
│                            using the arity-2 Poseidon chain
└── _old/                  — pre-reorganisation circuits, incl. the
                             `multiplier` (gain2) baseline still in use
```

`compile.circuit.sh` passes `-l circuits`, so a circuit in any
subdirectory can `include "hasher.circom"` by bare name. **Compiling without
that flag fails** with `error[P1014]: The file hasher.circom to be included has
not been found`, because circom otherwise resolves includes relative to the
including file.

One trap to know about: circom prefers the *including file's own directory*
over the `-l` path, so a stale copy of a shared primitive sitting beside a
circuit will shadow the real one. A duplicate `decompressor.circom` in `old/`
(missing `Recompressor`) did exactly this and broke the baseline build until it
was removed. If you copy circuits between directories, don't bring the shared
primitives along.

### The edit registry today

| Edit | Relation | Clamps? | Bit depths | Circuit artifacts |
|---|---|---|---|---|
| `gain2` | `x*2`, no clamp | **no** | 16 | `multiplier_b16_B{10,40,100,200}` |
| `gain_k2` | `clamp(2x)` | yes | 8, 16, 24 | `gain_b{8,16,24}_k2_B*` |
| `gain_k3` | `clamp(3x)` | yes | 16 | `gain_b16_k3_B100` |
| `gain_k4` | `clamp(4x)` | yes | 8, 16 | `gain_b{8,16}_k4_B100` |
| `atten_m1` | `floor(x/2)` | no | 8, 16 | `atten_b{8,16}_m1_B100` |
| `atten_m2` | `floor(x/4)` | no | 8, 16 | `atten_b{8,16}_m2_B100` |
| `crop` | verbatim window + counter | no | 8 | `crop_b8_B{10,100}` |
| `concat` | `A[..k] ++ B[k..]` | no | 8 | `concat_b8_B{10,100}` |
| `merge` | `floor((a+b)/2)` | no | 8 | `merge_b8_avg_B{10,100}` |
| `tremolo_f5` | LFO multiply, 5 Hz | no | 8 | `tremolo_b8_f5_B{10,100}` |
| `dist_d100` | rectifier fold, 100% wet | yes | 8 | `dist_b8_d100_B{10,100}` |
| `dist_d50` | rectifier fold, 50% wet | yes | 8 | `dist_b8_d50_B100` |

`merge` and `concat` take two streams: pass `--input-b <wav>`. The registry
also holds benchmark-only `gain_k2_{nohash,onehash,wide*,sponge*}` variants
from the hashing study — `gain_k2_nohash`, `gain_k2_onehash` and
`gain_k2_sponge{4,8}` are **unsound** (no binding to the published audio) and
exist only to price the commitment. Their circuits live in
`circuits/edits/gain/_experimental/` with warnings in the source.

`gain2` is the **original unclamped baseline**, kept unchanged so the
duration-scaling results in
[analyses/duration_scaling_conclusions.md](analyses/duration_scaling_conclusions.md) remain
valid and comparable. It requires audio with headroom, since `x * 2` must not
overflow.

The `gain_*` family is the generalized relation: **any integer k > 1,
saturating at the limits of the bit depth, at 8 or 16 bits**. Use it for new
work; it is safe on full-scale audio precisely because it clamps.

To add another factor, write the instantiation and compile it:

```bash
cat > circuits/edits/gain/gain_b16_k5_B100.circom <<'EOF'
pragma circom 2.1.6;
include "gain_step.circom";
component main { public [step_in] } = Gain(100, 16, 5);
EOF
./compile.circuit.sh circuits/edits/gain/gain_b16_k5_B100.circom
```

then add a matching `EditSpec` with `factor: Some(5)`.

### Adding a different kind of edit

Adding one requires **both sides**:

1. **Circom** — a step circuit enforcing the relation, plus a
   `component main = Multiply(B)` instantiation per B, compiled into
   `artifacts/`.
2. **Rust** — one entry in the registry in [src/bench/config.rs](../src/bench/config.rs):

```rust
EditSpec {
    name: "myedit",
    circuit_prefix: "myedit",     // -> artifacts/myedit_b16_B100.r1cs
    apply: |samples, _bit_depth| { /* same relation as the circuit */ },
}
```

The registry is the only place the Rust↔Circom correspondence is recorded. If
the two implementations disagree, the suite proves the wrong thing — and
`gain2` is a live example of why this needs care: the circuit enforces
`2*orig - 32768` on *normalized* samples with no clamping, which is **not** the
same function as `audio_editor::gain_multiplier` (which clamps on signed
samples). The registry deliberately uses a plain `x * 2` closure instead.

`distortion` and `pitch_shifter` have no sound circuit counterpart yet, so they
are not in the registry.

### Different bit depth

`--bit-depth` accepts **8, 16 or 24**. `comp_size` is derived as
`(256 - bit_depth) \ bit_depth` — **integer** division — so the packing
density changes with the depth:

| Bit depth | Samples per field element | Samples/step at B=10 | B=100 |
|---|---|---|---|
| 8 | 31 | 310 | 3,100 |
| 16 | 15 | 150 | 1,500 |
| 24 | 9 | 90 | 900 |

24 is the one depth where the division is **not** exact: 256 − 24 = 232, and
232 = 9×24 + 16, so a packed element uses only 216 of its 232 available bits.
Two consequences, both already handled: `Decompressor` decomposes
`size * bit_depth` bits rather than all of `nBits` (otherwise 2^16 distinct
field elements would unpack to the same samples), and every circom site
computing `comp_size` uses `\` rather than `/`, because field division would
yield a modular inverse instead of 9.

**The N ladder is ragged at 8 bits.** A B=10 step covers 310 samples, and no
`60 × 10^k` value is a whole number of 310s, so every cell is padded up and
measures slightly more audio than its label says. Use `--n-list` with
explicit multiples when the reported N has to be exact:

```bash
./target/release/bench --n-list 3100,31000 --bit-depth 8 \
    --edit gain_k2 --batch-sizes 10 --input <an 8-bit wav>
```

The suite also refuses to run when `--bit-depth` disagrees with the WAV's actual
`bits_per_sample`, rather than proving a misreported relation. Note `hound`
returns signed samples at both depths — it normalizes away the 8-bit unsigned
on-disk storage convention itself — so no recentring is applied on read.

Per-sample cost rises with depth even though the per-fold cost falls, because
`comp_size` shrinks faster: bd=24 costs ~3.1× bd=8 at equal N, bd=16 ~1.9×.

A further depth needs a new set of compiled instantiations; the circom
templates are depth-generic.

---

## Troubleshooting

**`error: --max-n is required`**
By design. Pick where the ladder stops: `--max-n 600` for a smoke test,
`--max-n 600000` for the full sweep.

**`no runnable cases; see skip reasons above`**
Every cell was skipped. Usually `--max-n` is below `B × comp_size` for all
your B values — e.g. at bd=16 (`comp_size` 15), `--max-n 600 --batch-sizes 200`
needs 3000 samples per step. Lower B or raise `--max-n`. At bd=8 the ladder
cannot divide at all, whatever B — use `--n-list` instead.

**`skip: missing artifact artifacts/multiplier_b16_B40.r1cs`**
That circuit isn't compiled. The dry-run output prints the exact command:
```bash
./compile.circuit.sh circuits/_old/multiplier_b16_B40.circom
```

**`WARNING: debug build — timings are not representative`**
You forgot `--release`. Numbers from a debug build are meaningless.

**`bad input shape: input has 10 elements/batch but circuit expects 150`**
A serialized JSON whose filename disagrees with its contents. Delete the file
and let the suite regenerate it; it validates shape after writing.

**Everything is slower than the table above**
Check `manifest.json` → `environment.debug_build` is `false`, and that nothing
else heavy is running. Proving is CPU-bound and single-run-sensitive.

---

## Single-run mode

For one proof with verbose per-phase output rather than a sweep:

```bash
cargo run --release
```

Edit the constants at the top of [src/main.rs](../src/main.rs) (`bit_depth`,
`batch_size`, `max_size`, `edit_name`). It asserts N is a whole number of
batches instead of silently truncating, so a bad combination fails immediately
with a clear message.
