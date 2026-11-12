# Benchmark Summary

- schema_version: 2
- edit: `gain_k2_wide`
- audio: `raw_audios/sweep_605s_2c_44sr_8bd.wav`
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
| 310000 | 65.643 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 310000 | 142.072 |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 310000 | 5.480 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 310000 | 180.605 |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 310000 | 30049.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 310000 | 211.751 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 310000 | 656.428 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 310000 | 1104.969 (±8%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 310000 | 1026.188 (±7%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 310000 | 981.500 |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 310000 | 293.250 (±86%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 165424 | 159218 | 53.4 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 310000 | 100 | 0x36dc97683785… | 0x37547d8bb533… | yes |

