# Benchmark Summary

- schema_version: 2
- edit: `gain_k2`
- audio: `raw_audios/sweep_31s_2c_44sr_8bd.wav`
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
| 3100 | 0.738 (±6%) |
| 31000 | 5.711 |
| 310000 | 56.489 |
| 2644300 | 498.205 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 98.248 (±3%) |
| 31000 | 196.870 (±5%) |
| 310000 | 204.152 (±9%) |
| 2644300 | 208.823 (±8%) |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 3100 | 5.435 (±6%) |
| 31000 | 5.495 (±5%) |
| 310000 | 5.527 (±4%) |
| 2644300 | 5.732 (±7%) |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 177.878 |
| 31000 | 177.692 (±3%) |
| 310000 | 173.246 (±3%) |
| 2644300 | 200.858 (±56%) |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 3100 | 29977.000 |
| 31000 | 30037.000 |
| 310000 | 30031.000 |
| 2644300 | 30021.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 3100 | 238.050 (±6%) |
| 31000 | 184.225 |
| 310000 | 182.224 |
| 2644300 | 188.407 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 3100 | 737.955 (±6%) |
| 31000 | 571.099 |
| 310000 | 564.894 |
| 2644300 | 584.062 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 3100 | 1086.234 (±8%) |
| 31000 | 1451.828 (±5%) |
| 310000 | 1303.203 (±3%) |
| 2644300 | 1134.094 (±14%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 3100 | 895.516 (±17%) |
| 31000 | 1253.281 (±7%) |
| 310000 | 1239.734 |
| 2644300 | 1134.094 (±13%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 3100 | 1059.125 (±4%) |
| 31000 | 1299.203 (±11%) |
| 310000 | 1132.031 |
| 2644300 | 1000.516 |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 3100 | 255.031 (±145%) |
| 31000 | 324.812 (±23%) |
| 310000 | 296.906 (±5%) |
| 2644300 | 328.562 (±22%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 234318 | 228112 | 75.6 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 3100 | 100 | 0x097ea033f2f4… | 0x2a2b71b1b662… | yes |
| 31000 | 100 | 0x18f24faecb29… | 0x2edda898340e… | yes |
| 310000 | 100 | 0x17dd4d3a3b1f… | 0x2ed7b4faac3d… | yes |
| 2644300 | 100 | 0x154c87c46c0b… | 0x1b14fc7271bd… | yes |

