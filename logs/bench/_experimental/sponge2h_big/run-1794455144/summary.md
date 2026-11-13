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

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 138.379 | 135.512 |

## RecursiveSNARK verify (ms)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 911.760 | 1137.258 (±4%) |

## CompressedSNARK prove (s)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 41.689 | 42.417 |

## CompressedSNARK verify (ms)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 1483.652 (±10%) | 1487.177 (±8%) |

## Proof size (bytes)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 32594.000 | 32545.000 |

## us per sample (prove)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 111.596 | 109.284 |

## ms per iteration (prove)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 2767.576 | 3387.791 |

## Peak RSS, whole proof (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 3929.859 (±4%) | 4328.156 (±4%) |

## Peak RSS during folding (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 3281.031 | 3715.000 (±6%) |

## Peak RSS during compression (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 3614.422 (±4%) | 3991.031 |

## Peak RSS over baseline (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 2310.438 (±30%) | 2827.984 (±6%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 800 | 24800 | 1313652 | 1264046 | 53.0 |
| 1000 | 31000 | 1639352 | 1577346 | 52.9 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 1240000 | 800 | 0x13fbac523b2d… | 0x2b0c5135e116… | yes |
| 1240000 | 1000 | 0x194d74f13d32… | 0x27a8af66dd82… | yes |

