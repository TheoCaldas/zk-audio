# Benchmark Summary

- schema_version: 2
- edit: `atten_m1`
- audio: `raw_audios/sweep_31s_2c_44sr_16bd_fullscale.wav`
- bit_depth: 16
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
| 30000 | 5.264 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 30000 | 161.765 |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 30000 | 7.906 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 30000 | 127.403 |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 30000 | 26935.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 30000 | 175.454 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 30000 | 263.180 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 30000 | 574.641 |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 30000 | 523.734 |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 30000 | 574.641 |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 30000 | 535.094 |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 1500 | 108305 | 106799 | 72.2 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 30000 | 100 | 0x1b265a0db625… | 0x3d0d088fe8a5… | single rep |

