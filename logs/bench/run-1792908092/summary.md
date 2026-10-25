# Benchmark Summary

- schema_version: 2
- edit: `concat`
- audio: `raw_audios/sweep_31s_2c_44sr_8bd.wav`
- bit_depth: 8
- repeat: 3

`compressed_prove_ms` excludes `CompressedSNARK::setup`; see manifest.json.

Peak RSS is **process-wide and sampled** (every 50ms): it includes the wasm
witness generator's allocations and anything else the binary holds, and a spike
shorter than the sampling interval can be missed — so each figure is a lower
bound on the true peak. Figures assume an otherwise-idle machine.

Compare the **absolute** peaks: they reproduce across runs (the same N=6000/B=100
cell measured 877.7MB alone and 854.9MB as the third case of a sweep, 2.6%
apart). The *over baseline* row does NOT reproduce — the allocator keeps freed
pages, so a case running later in a sweep starts from a higher baseline and
reports a smaller delta for identical work (35.9MB vs 344.7MB baseline for that
same cell). It is listed for completeness, not for comparison; to measure one
cell's own footprint, run it alone.

## RecursiveSNARK prove (s)

| N | B=100 |
|---|---|
| 3100 | 0.451 |
| 31000 | 3.533 |
| 310000 | 35.882 |
| 2644300 | 301.827 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 80.784 (±6%) |
| 31000 | 114.610 (±3%) |
| 310000 | 114.384 |
| 2644300 | 115.186 (±8%) |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 3100 | 2.889 (±22%) |
| 31000 | 2.937 (±8%) |
| 310000 | 2.923 |
| 2644300 | 2.893 (±3%) |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 102.997 (±9%) |
| 31000 | 97.148 (±5%) |
| 310000 | 99.081 (±3%) |
| 2644300 | 103.897 (±6%) |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 3100 | 29336.000 |
| 31000 | 29392.000 |
| 310000 | 29425.000 |
| 2644300 | 29393.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 3100 | 145.369 |
| 31000 | 113.955 |
| 310000 | 115.749 |
| 2644300 | 114.142 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 3100 | 450.643 |
| 31000 | 353.261 |
| 310000 | 358.823 |
| 2644300 | 353.841 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 3100 | 789.109 (±13%) |
| 31000 | 945.438 (±3%) |
| 310000 | 946.078 (±3%) |
| 2644300 | 1016.062 (±7%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 3100 | 746.672 (±22%) |
| 31000 | 901.359 |
| 310000 | 928.516 |
| 2644300 | 963.781 (±7%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 3100 | 784.859 (±8%) |
| 31000 | 887.750 (±3%) |
| 310000 | 884.844 (±3%) |
| 2644300 | 919.750 (±14%) |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 3100 | 103.578 (±255%) |
| 31000 | 90.156 (±14%) |
| 310000 | 94.609 (±47%) |
| 2644300 | 136.703 (±17%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 115446 | 115645 | 37.2 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 3100 | 100 | 0x000000000000… | 0x2392d97469eb… | yes |
| 31000 | 100 | 0x1b3113a32f36… | 0x35419df23529… | yes |
| 310000 | 100 | 0x2ce0251df6e8… | 0x3f333bc6731e… | yes |
| 2644300 | 100 | 0x06b69af5a668… | 0x325fa9154079… | yes |

