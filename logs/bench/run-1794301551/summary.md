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

| N | B=100 |
|---|---|
| 139500 | 26.077 |
| 418500 | 78.334 |
| 837000 | 155.573 |
| 1674000 | 317.654 |
| 2650500 | 493.306 (±3%) |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 139500 | 192.398 (±3%) |
| 418500 | 195.151 (±3%) |
| 837000 | 200.397 (±5%) |
| 1674000 | 195.054 (±5%) |
| 2650500 | 198.508 (±6%) |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 139500 | 5.595 |
| 418500 | 5.517 |
| 837000 | 5.569 (±4%) |
| 1674000 | 5.418 |
| 2650500 | 5.670 (±7%) |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 139500 | 187.667 |
| 418500 | 181.920 (±4%) |
| 837000 | 195.303 (±5%) |
| 1674000 | 184.261 (±9%) |
| 2650500 | 192.147 (±7%) |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 139500 | 30036.000 |
| 418500 | 30082.000 |
| 837000 | 30018.000 |
| 1674000 | 30042.000 |
| 2650500 | 30028.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 139500 | 186.931 |
| 418500 | 187.177 |
| 837000 | 185.869 |
| 1674000 | 189.757 |
| 2650500 | 186.118 (±3%) |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 139500 | 579.485 |
| 418500 | 580.249 |
| 837000 | 576.195 |
| 1674000 | 588.247 |
| 2650500 | 576.966 (±3%) |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 139500 | 1128.203 (±6%) |
| 418500 | 1169.797 (±3%) |
| 837000 | 1180.016 |
| 1674000 | 1229.641 (±7%) |
| 2650500 | 1197.812 (±7%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 139500 | 1032.547 (±11%) |
| 418500 | 1124.562 (±10%) |
| 837000 | 1061.141 (±7%) |
| 1674000 | 1150.094 (±5%) |
| 2650500 | 1115.875 (±3%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 139500 | 1009.359 (±8%) |
| 418500 | 1018.047 (±5%) |
| 837000 | 1120.141 (±8%) |
| 1674000 | 1056.891 (±5%) |
| 2650500 | 1035.031 (±10%) |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 139500 | 290.781 (±93%) |
| 418500 | 223.281 (±31%) |
| 837000 | 262.766 (±25%) |
| 1674000 | 223.281 (±22%) |
| 2650500 | 317.422 (±15%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 234318 | 228112 | 75.6 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 139500 | 100 | 0x14dcc5a894f5… | 0x02ee621f9185… | yes |
| 418500 | 100 | 0x22fb82fe59a4… | 0x1ceae8bbbf09… | yes |
| 837000 | 100 | 0x37ba0dce79c3… | 0x346059c9807b… | yes |
| 1674000 | 100 | 0x3137051cab22… | 0x216d5449998b… | yes |
| 2650500 | 100 | 0x37a7db2e9aff… | 0x35540d1c9aac… | yes |

