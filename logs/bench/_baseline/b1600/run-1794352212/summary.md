# Benchmark Summary

- schema_version: 2
- edit: `gain_k2`
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

| N | B=1600 |
|---|---|
| 1488000 | 219.472 |

## RecursiveSNARK verify (ms)

| N | B=1600 |
|---|---|
| 1488000 | 2697.031 |

## CompressedSNARK prove (s)

| N | B=1600 |
|---|---|
| 1488000 | 90.264 (±10%) |

## CompressedSNARK verify (ms)

| N | B=1600 |
|---|---|
| 1488000 | 2590.383 (±3%) |

## Proof size (bytes)

| N | B=1600 |
|---|---|
| 1488000 | 33413.000 |

## us per sample (prove)

| N | B=1600 |
|---|---|
| 1488000 | 147.494 |

## ms per iteration (prove)

| N | B=1600 |
|---|---|
| 1488000 | 7315.726 |

## Peak RSS, whole proof (MB)

| N | B=1600 |
|---|---|
| 1488000 | 6601.141 (±3%) |

## Peak RSS during folding (MB)

| N | B=1600 |
|---|---|
| 1488000 | 6563.328 (±5%) |

## Peak RSS during compression (MB)

| N | B=1600 |
|---|---|
| 1488000 | 6174.578 (±8%) |

## Peak RSS over baseline (MB)

| N | B=1600 |
|---|---|
| 1488000 | 5313.578 (±7%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 1600 | 49600 | 3601818 | 3502612 | 72.6 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 1488000 | 1600 | 0x370b700b334c… | 0x317728e55c9b… | yes |

