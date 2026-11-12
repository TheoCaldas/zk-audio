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
| 2644300 | 358.392 |
| 5291700 | 700.670 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 2644300 | 256.902 |
| 5291700 | 253.836 |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 2644300 | 15.513 |
| 5291700 | 15.455 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 2644300 | 208.301 |
| 5291700 | 206.102 |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 2644300 | 27652.000 |
| 5291700 | 27652.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 2644300 | 135.534 |
| 5291700 | 132.409 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 2644300 | 420.155 |
| 5291700 | 410.469 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 2644300 | 1209.734 |
| 5291700 | 1180.984 |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 2644300 | 1209.734 |
| 5291700 | 1180.984 |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 2644300 | 987.531 |
| 5291700 | 1016.625 |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 2644300 | 886.469 |
| 5291700 | 291.719 |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 234321 | 228115 | 75.6 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 2644300 | 100 | 0x34e150afbc7c… | 0x0185ab3e0d07… | single rep |
| 5291700 | 100 | 0x07e337436c79… | 0x159e04c0c26d… | single rep |

