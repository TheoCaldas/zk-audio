# Benchmark Summary

- schema_version: 2
- edit: `gain_k2_sponge4`
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
| 1240000 | 121.416 | 119.348 |

## RecursiveSNARK verify (ms)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 793.045 (±5%) | 991.933 (±17%) |

## CompressedSNARK prove (s)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 41.096 | 42.067 |

## CompressedSNARK verify (ms)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 1317.634 (±39%) | 1361.243 (±6%) |

## Proof size (bytes)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 32440.000 | 32516.000 |

## us per sample (prove)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 97.916 | 96.249 |

## ms per iteration (prove)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 2428.310 | 2983.705 |

## Peak RSS, whole proof (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 4048.938 (±7%) | 3837.438 (±4%) |

## Peak RSS during folding (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 3209.203 (±3%) | 3595.094 |

## Peak RSS during compression (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 3511.641 (±3%) | 3614.609 (±3%) |

## Peak RSS over baseline (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 2785.328 (±27%) | 2317.734 (±10%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 800 | 24800 | 1146133 | 1096525 | 46.2 |
| 1000 | 31000 | 1430083 | 1368075 | 46.1 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 1240000 | 800 | 0x13fbac523b2d… | n/a (single-co… | yes |
| 1240000 | 1000 | 0x194d74f13d32… | n/a (single-co… | yes |

