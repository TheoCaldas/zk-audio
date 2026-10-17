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

## The two knobs

Everything revolves around two numbers:

| | Meaning |
|---|---|
| **B** (batch size) | Compressed field elements per Nova folding step. One element packs 15 audio samples, so a step covers `B × 15` samples. |
| **N** (total samples) | How many audio samples get proven in total. Number of folding steps is `N / (B × 15)`. |

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

### Not every (B, N) pair is valid

A case only runs if **N divides evenly into whole batches**, i.e.
`N % (B × 15) == 0`. Otherwise the last batch would be short and the circuit
rejects it.

So some cells get skipped automatically, and that's expected:

```
       N     B samples/st    iters  status
      60    10        150        0  skip: N < one batch (150 samples/step)
     600    10        150        4  run
     600   100       1500        0  skip: N < one batch (1500 samples/step)
    6000   100       1500        4  run
```

`--dry-run` shows you this table before committing to a long run. **Always
dry-run first.**

> The usable set is **B ∈ {10, 40, 100, 200}**.

---

## All the flags

| Flag | Default | What it does |
|---|---|---|
| `--max-n <N>` | **required\*** | Where the N ladder stops |
| `--durations <list>` | — | Audio lengths in seconds; replaces the ladder |
| `--sample-rate <hz>` | `44100` | Rate assumed by `--durations` |
| `--channels <n>` | `2` | Channels assumed by `--durations` |
| `--batch-sizes <list>` | `10,40,100,200` | Comma-separated B values |
| `--bit-depth <n>` | `16` | Only 16 is compiled today |
| `--input <path>` | `raw_audios/2c_44sr_16bd_01.wav` | WAV to prove over |
| `--edit <name>` | `gain2` | Which edit relation to prove |
| `--out <dir>` | `logs/bench` | Where results go |
| `--repeat <n>` | `3` | Repetitions per case (for timing spread) |
| `--skip-compressed` | off | Recursive phase only — much faster |
| `--no-cache-inputs` | off | Delete each serialized JSON after use |
| `--dry-run` | off | Print the matrix and exit |
| `-h`, `--help` | | Show usage |

\* Either `--max-n` or `--durations` is required, not both.

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
logs/bench/run-1791754350/
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
alone) and a **commitments** table.

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

path = "logs/bench/run-1791754350/results.jsonl"
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
df = pd.read_json("logs/bench/run-1791754350/results.jsonl", lines=True)
ok = df[df.status == "ok"]
ok.pivot_table(index="total_samples", columns="batch_size",
               values="recursive_prove_ms", aggfunc="median")
```

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
[docs/gen_bench_fixture.py](gen_bench_fixture.py).

Its amplitude is capped at ±8000 (about a quarter of full scale) **on purpose**:
`gain2` is `x * 2` with no clamping on either side, so samples near full scale
would overflow `i16` in Rust and stop matching what the circuit enforces. Any
fixture you add for `gain2` needs the same headroom.

### Different edit

```bash
./target/release/bench --max-n 6000 --edit gain2
```

`gain2` is the only edit available today. Adding one requires **both sides**:

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

`--bit-depth` accepts only 16 today. The plumbing is parameterized —
`comp_size` is derived as `(256 - bit_depth) / bit_depth`, not hardcoded — but
another depth needs a new step circuit, since the `-32768` offset is
`2^(bit_depth-1)`.

---

## Troubleshooting

**`error: --max-n is required`**
By design. Pick where the ladder stops: `--max-n 600` for a smoke test,
`--max-n 600000` for the full sweep.

**`no runnable cases; see skip reasons above`**
Every cell was skipped. Usually `--max-n` is below `B × 15` for all your B
values — e.g. `--max-n 600 --batch-sizes 200` needs 3000 samples per step.
Lower B or raise `--max-n`.

**`skip: missing artifact artifacts/multiplier_b16_B40.r1cs`**
That circuit isn't compiled. The dry-run output prints the exact command:
```bash
./compile.circuit.sh circuits/multiplier_b16_B40.circom
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
