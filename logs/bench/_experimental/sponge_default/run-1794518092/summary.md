# Benchmark Summary

- schema_version: 2
- edit: `gain_k2`
- audio: `raw_audios/sweep_35s_2c_44sr_24bd.wav`
- bit_depth: 24
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
| 27000 | 7.871 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 27000 | 165.487 |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 27000 | 14.047 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 27000 | 204.127 |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 27000 | 27652.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 27000 | 291.502 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 27000 | 262.351 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 27000 | 814.922 |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 27000 | 611.484 |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 27000 | 814.922 |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 27000 | 770.344 |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 900 | 131105 | 129299 | 145.7 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 27000 | 100 | 0x16fe603b3ecd… | 0x38ec8fea7306… | single rep |

