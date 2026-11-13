# Benchmark Summary

- schema_version: 2
- edit: `atten_m1`
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
| 3100 | 0.750 (±7%) |
| 31000 | 5.164 (±3%) |
| 310000 | 50.117 |
| 2644300 | 426.613 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 98.255 (±6%) |
| 31000 | 185.076 (±5%) |
| 310000 | 176.727 |
| 2644300 | 180.742 (±6%) |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 3100 | 5.857 (±6%) |
| 31000 | 5.532 |
| 310000 | 5.703 |
| 2644300 | 5.522 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 206.951 (±3%) |
| 31000 | 186.343 (±5%) |
| 310000 | 178.486 (±3%) |
| 2644300 | 177.921 (±4%) |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 3100 | 30008.000 |
| 31000 | 30009.000 |
| 310000 | 30063.000 |
| 2644300 | 30027.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 3100 | 242.079 (±7%) |
| 31000 | 166.581 (±3%) |
| 310000 | 161.667 |
| 2644300 | 161.333 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 3100 | 750.445 (±7%) |
| 31000 | 516.400 (±3%) |
| 310000 | 501.166 |
| 2644300 | 500.132 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 3100 | 1116.953 (±6%) |
| 31000 | 1200.766 (±9%) |
| 310000 | 1107.297 (±14%) |
| 2644300 | 1140.312 (±7%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 3100 | 1007.438 (±17%) |
| 31000 | 1100.453 (±8%) |
| 310000 | 1029.453 (±12%) |
| 2644300 | 1079.672 (±7%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 3100 | 1029.359 (±7%) |
| 31000 | 1200.766 (±7%) |
| 310000 | 914.641 (±7%) |
| 2644300 | 1001.375 |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 3100 | 300.016 (±105%) |
| 31000 | 148.938 (±22%) |
| 310000 | 157.625 (±25%) |
| 2644300 | 249.984 (±17%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 178518 | 175412 | 57.6 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 3100 | 100 | 0x097ea033f2f4… | 0x3cd427f70b9f… | yes |
| 31000 | 100 | 0x18f24faecb29… | 0x2fc171caca17… | yes |
| 310000 | 100 | 0x17dd4d3a3b1f… | 0x2ee9f4badeb1… | yes |
| 2644300 | 100 | 0x154c87c46c0b… | 0x13387a854082… | yes |

