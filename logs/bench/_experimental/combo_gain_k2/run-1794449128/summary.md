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

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 183.793 | 184.562 |

## RecursiveSNARK verify (ms)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 1335.778 (±5%) | 1711.612 |

## CompressedSNARK prove (s)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 44.467 (±9%) | 82.093 |

## CompressedSNARK verify (ms)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 1368.836 (±11%) | 2460.148 |

## Proof size (bytes)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 32545.000 | 33371.000 |

## us per sample (prove)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 148.220 | 148.840 |

## ms per iteration (prove)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 3675.857 | 4614.046 |

## Peak RSS, whole proof (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 3794.141 (±14%) | 4749.109 |

## Peak RSS during folding (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 3538.172 (±15%) | 4631.266 (±4%) |

## Peak RSS during compression (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 3794.141 (±7%) | 4747.562 (±3%) |

## Peak RSS over baseline (MB)

| N | B=800 | B=1000 |
|---|---|---|
| 1240000 | 2778.312 (±9%) | 3404.953 (±6%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 800 | 24800 | 1805818 | 1756212 | 72.8 |
| 1000 | 31000 | 2254818 | 2192812 | 72.7 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 1240000 | 800 | 0x06096ebc1500… | 0x0e243d5b638e… | yes |
| 1240000 | 1000 | 0x2fa0c39e26da… | 0x2438e6f5e8d4… | yes |

