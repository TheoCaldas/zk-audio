# Benchmark Summary

- schema_version: 2
- edit: `gain_k2_sponge2h`
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
| 2480000 | 249.135 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 2480000 | 210.358 |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 2480000 | 14.293 (±4%) |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 2480000 | 193.741 (±7%) |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 2480000 | 27652.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 2480000 | 100.458 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 2480000 | 311.419 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 2480000 | 1108.141 (±10%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 2480000 | 1108.141 (±15%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 2480000 | 987.844 (±10%) |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 2480000 | 160.281 (±140%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 173705 | 167499 | 56.0 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 2480000 | 100 | 0x07e9b0dcb432… | 0x139d73451b9a… | yes |

