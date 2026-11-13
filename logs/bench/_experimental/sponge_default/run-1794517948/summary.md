# Benchmark Summary

- schema_version: 2
- edit: `gain_k2`
- audio: `raw_audios/sweep_605s_2c_44sr_8bd.wav`
- bit_depth: 8
- repeat: 1

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
| 31000 | 3.270 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 31000 | 244.078 |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 31000 | 14.107 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 31000 | 191.127 |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 31000 | 27652.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 31000 | 105.470 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 31000 | 326.958 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 31000 | 1062.547 |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 31000 | 848.688 |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 31000 | 1062.547 |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 31000 | 817.828 |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 173705 | 167499 | 56.0 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 31000 | 100 | 0x10acb8187095… | 0x097210760ccf… | single rep |

