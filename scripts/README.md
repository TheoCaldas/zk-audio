# Scripts

Helper scripts for the benchmark suite: generating the WAV fixtures and the
LFO table the circuits need, and turning `logs/bench/` into figures, tables
and predictions.

**Run everything from the repository root.** The fixture generators write to
paths relative to the working directory (`raw_audios/…`); the analysis
scripts locate the repo from their own location, so they work either way,
but the root is the consistent choice.

```bash
cd zk-audio
python3 scripts/<name>.py
```

## Python interpreter

The analysis scripts that draw figures need **matplotlib**, which the default
Homebrew `python3` (3.13) does not have and cannot easily get — it is
PEP-668 externally managed. Use `/usr/bin/python3` (3.9, matplotlib 3.8) or
`python3.11` for those:

```bash
/usr/bin/python3 scripts/plot_bench.py logs/bench/_baseline/run-1792898036
```

`plot_bench.py` falls back to a dependency-free SVG writer when matplotlib is
missing, so it never hard-fails. The other plotters require it. Everything
else is pure standard library.

---

## Fixture generators

Deterministic and seeded: re-running one reproduces the file bit-identically,
so a fixture never has to be committed to be reproducible. All write into
`raw_audios/`, which is gitignored.

| script | output | notes |
|---|---|---|
| `gen_bench_fixture.py` | `sweep_120s_2c_44sr_16bd.wav` | 2 min, 16-bit. Capped at ¼ full scale: the `gain2` relation is `x*2` with **no clamping**, so near-full-scale samples would overflow `i16` and stop matching the circuit. |
| `gen_16bit_clamp_fixture.py` | `sweep_31s_2c_44sr_16bd_fullscale.wav` | 31 s, 16-bit, **near full scale**. Use this for the clamping edits (`gain_k*`), where saturation is part of the relation under test. |
| `gen_8bit_bench_fixture.py` | `sweep_31s_2c_44sr_8bd.wav` | 31 s, 8-bit, near full scale. |
| `gen_8bit_long_fixture.py` | `sweep_605s_2c_44sr_8bd.wav` | 605 s (~53 MB), for runs past 30 s of audio. |
| `gen_24bit_bench_fixture.py` | `sweep_35s_2c_44sr_24bd.wav` | 35 s, 24-bit. |
| `gen_8bit_stream_b.py` | `sweep_31s_2c_44sr_8bd_streamB.wav` | Second stream for the two-input edits (`merge`, `concat`), passed as `--input-b`. |
| `gen_rate_channel_fixtures.py` | `rc_{1,2}c_{22050,44100}sr_8bd.wav` | Four files for the rate/channel experiment. The 22.05 kHz versions integrate phase from absolute time, so they are a true resampling of the same signal rather than a different waveform. |

**Which fixture matters.** Amplitude is not cosmetic: `sweep_120s_2c_44sr_16bd.wav`
saturates 0 % of samples under `gain_k2`, against ~49.5 % for the fullscale
files. Benchmarking a clamping edit on the quarter-scale fixture measures a
materially easier workload.

## Code generators

| script | output | notes |
|---|---|---|
| `gen_lfo_table.py` | `circuits/edits/tremolo/tremolo_table.circom` and `src/lfo_table.rs` | The tremolo LFO as a 256-entry lookup table, emitted for **both** sides so the circuit and the Rust reference cannot drift. Re-run after changing the table size or waveform. |
| `gen_poseidon_constants.py` | `src/poseidon/constants.rs` | circomlib's Poseidon round constants, transcribed from `circuits/circom/poseidon_constants.circom` so the Rust reference hash and the circuits cannot drift. Emitted as **hex strings, not limbs**: circom reinterprets each literal modulo whichever `--prime` it is given, and keeping them undecoded lets one table serve any field - which is what makes the Rust side match `--prime vesta`. Asserts every table length and exits non-zero on mismatch. Re-run after a circomlib upgrade. |
| `gen_circuit_index.py` | `circuits/CIRCUIT_INDEX.md` (stdout) | Maps every `circuit_name` in `logs/bench/` to the file that produced it (`circuits/edits/_baseline/…`) and the file carrying that name today. Needed because the sponge switch changed the commitment digest, so an old `circuit_name` no longer denotes the circuit at that path. |

```bash
python3 scripts/gen_circuit_index.py > circuits/CIRCUIT_INDEX.md
```

## Analysis and plotting

| script | what it does |
|---|---|
| `plot_bench.py` | The general plotter: time, memory, proof size and µs/sample against N, for one run or many. Reads `results.jsonl`, **not** `results.csv` — the CSV has no `peak_rss_*` columns. |
| `plot_edit_constraints.py` | Horizontal bar chart of per-step circuit size by edit family. |
| `plot_rate_channel.py` | Two-panel figure for the rate/channel experiment: equal-N routes, and the halving arms. |
| `predict_proof_time.py` | Cost model, `time = folds × ms_per_fold`. Predicts a configuration, or prints matrix views. |

### `plot_bench.py`

```bash
# one run -> <run>/plots/
/usr/bin/python3 scripts/plot_bench.py logs/bench/_baseline/run-1792898036

# compare several, one series per edit
/usr/bin/python3 scripts/plot_bench.py logs/bench/_baseline/run-17928* --series edit

# every run found, grouped by batch size
/usr/bin/python3 scripts/plot_bench.py --all --series batch_size -o /tmp/plots

# aggregated medians to stdout as well
/usr/bin/python3 scripts/plot_bench.py <run> --only memory --print-table
```

Aggregation matches `src/bench/report.rs`: points are **medians** over
repetitions and error bars are the **half-range**, so figures agree with the
`summary.md` each run writes. Rows with `status != "ok"` are excluded and
reported to stderr rather than silently dropped.

### `plot_edit_constraints.py`

```bash
/usr/bin/python3 scripts/plot_edit_constraints.py
ZK_AUDIO_BENCH_GENERATION=_experimental /usr/bin/python3 scripts/plot_edit_constraints.py
```

Scoped to one generation (`_baseline` by default) because the same
`circuit_name` denotes two different circuits either side of the sponge
switch. The script asserts each name has one constraint count and **errors
out** if a directory mixes generations — that check is deliberate, not a bug.

### `predict_proof_time.py`

```bash
# predict one configuration
python3 scripts/predict_proof_time.py --edit gain_k2 --bit-depth 8 \
    --batch-size 100 --duration 600 --channels 2 --sample-rate 44100

# matrix views
python3 scripts/predict_proof_time.py --matrix
```

`folds` is exact arithmetic; `ms_per_fold` is measured per
`(edit, bit_depth, batch_size)` where data exists and otherwise estimated
from a power law in constraint count. Output labels which was used.
Extrapolating in **N** is reliable (validated to +0.3 % at 20× beyond the
fitted range); extrapolating to an **unmeasured circuit** is ~7 % median and
up to ~28 % worst case.

---

## Layout these scripts assume

```
logs/bench/
├── _baseline/      pre-optimisation results (arity-2 Poseidon chain)
├── _experimental/  optimisation-study runs
└── _old/           historical, no results.jsonl

circuits/edits/
├── _baseline/      pre-optimisation circuits
└── <family>/       current default (rate-4 sponge)
    └── gain/_experimental/   hashing variants
```

Moving a run between `_baseline` and `_experimental` changes which
generation the plots cover, so file new runs deliberately — the suite writes
to `logs/bench/run-<timestamp>/` by default, outside both.
