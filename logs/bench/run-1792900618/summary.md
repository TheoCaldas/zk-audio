# Benchmark Summary

- schema_version: 2
- edit: `crop`
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
| 3100 | 0.444 (±3%) |
| 31000 | 3.591 |
| 310000 | 35.341 |
| 2644300 | 300.717 (±3%) |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 107.547 (±24%) |
| 31000 | 129.045 (±12%) |
| 310000 | 125.794 (±15%) |
| 2644300 | 115.566 (±9%) |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 3100 | 3.681 (±3%) |
| 31000 | 3.426 |
| 310000 | 3.414 |
| 2644300 | 3.052 (±8%) |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 114.388 (±3%) |
| 31000 | 117.438 (±16%) |
| 310000 | 118.265 (±16%) |
| 2644300 | 105.075 (±4%) |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 3100 | 29214.000 |
| 31000 | 29294.000 |
| 310000 | 29286.000 |
| 2644300 | 29280.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 3100 | 143.346 (±3%) |
| 31000 | 115.848 |
| 310000 | 114.005 |
| 2644300 | 113.723 (±3%) |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 3100 | 444.373 (±3%) |
| 31000 | 359.128 |
| 310000 | 353.415 |
| 2644300 | 352.541 (±3%) |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 3100 | 608.750 (±11%) |
| 31000 | 764.094 (±3%) |
| 310000 | 851.562 |
| 2644300 | 1020.062 (±11%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 3100 | 542.484 (±20%) |
| 31000 | 708.578 |
| 310000 | 808.344 |
| 2644300 | 1020.062 (±11%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 3100 | 587.797 (±11%) |
| 31000 | 698.938 (±5%) |
| 310000 | 798.078 (±4%) |
| 2644300 | 987.500 (±11%) |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 3100 | 84.000 (±239%) |
| 31000 | 79.453 |
| 310000 | 78.547 |
| 2644300 | 102.188 (±41%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 113221 | 113317 | 36.5 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 3100 | 100 | 0x097ea033f2f4… | 0x097ea033f2f4… | yes |
| 31000 | 100 | 0x18f24faecb29… | 0x18f24faecb29… | yes |
| 310000 | 100 | 0x17dd4d3a3b1f… | 0x17dd4d3a3b1f… | yes |
| 2644300 | 100 | 0x154c87c46c0b… | 0x154c87c46c0b… | yes |

