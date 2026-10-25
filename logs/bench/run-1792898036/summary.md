# Benchmark Summary

- schema_version: 2
- edit: `tremolo_f5`
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
| 3100 | 0.998 (±3%) |
| 31000 | 7.185 |
| 310000 | 71.058 (±3%) |
| 2644300 | 648.714 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 130.541 (±8%) |
| 31000 | 226.753 (±6%) |
| 310000 | 241.990 (±24%) |
| 2644300 | 328.032 (±17%) |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 3100 | 11.290 |
| 31000 | 10.981 |
| 310000 | 11.414 (±24%) |
| 2644300 | 12.552 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 394.620 (±8%) |
| 31000 | 366.494 (±6%) |
| 310000 | 369.302 (±25%) |
| 2644300 | 385.079 |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 3100 | 30860.000 |
| 31000 | 30940.000 |
| 310000 | 30929.000 |
| 2644300 | 30911.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 3100 | 322.084 (±3%) |
| 31000 | 231.786 |
| 310000 | 229.218 (±3%) |
| 2644300 | 245.326 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 3100 | 998.459 (±3%) |
| 31000 | 718.537 |
| 310000 | 710.577 (±3%) |
| 2644300 | 760.509 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 3100 | 1302.500 (±11%) |
| 31000 | 1403.938 |
| 310000 | 1347.812 |
| 2644300 | 1527.781 (±13%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 3100 | 923.781 (±3%) |
| 31000 | 1043.516 (±3%) |
| 310000 | 1099.422 |
| 2644300 | 1143.281 (±7%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 3100 | 1302.500 (±11%) |
| 31000 | 1403.938 |
| 310000 | 1276.328 (±3%) |
| 2644300 | 1527.781 (±13%) |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 3100 | 744.625 (±42%) |
| 31000 | 577.844 (±6%) |
| 310000 | 512.422 |
| 2644300 | 760.000 (±5%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 288420 | 285016 | 93.0 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 3100 | 100 | 0x097ea033f2f4… | 0x253bffc1f8b3… | yes |
| 31000 | 100 | 0x18f24faecb29… | 0x32c4a650f95a… | yes |
| 310000 | 100 | 0x17dd4d3a3b1f… | 0x3744989b070d… | yes |
| 2644300 | 100 | 0x154c87c46c0b… | 0x35e52da2ca6c… | yes |

