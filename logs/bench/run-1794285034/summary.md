# Benchmark Summary

- schema_version: 2
- edit: `gain_k2`
- audio: `raw_audios/sweep_35s_2c_44sr_24bd.wav`
- bit_depth: 24
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
| 139500 | 80.999 |
| 418500 | 244.290 |
| 837000 | 485.970 |
| 1674000 | 985.055 |
| 2650500 | 1533.247 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 139500 | 188.910 (±6%) |
| 418500 | 183.539 (±11%) |
| 837000 | 189.789 (±7%) |
| 1674000 | 187.502 (±4%) |
| 2650500 | 179.494 (±11%) |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 139500 | 5.774 |
| 418500 | 5.839 (±3%) |
| 837000 | 5.462 (±3%) |
| 1674000 | 5.539 (±6%) |
| 2650500 | 5.511 (±8%) |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 139500 | 189.155 (±5%) |
| 418500 | 186.211 (±4%) |
| 837000 | 190.481 |
| 1674000 | 188.066 (±4%) |
| 2650500 | 170.234 (±9%) |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 139500 | 30062.000 |
| 418500 | 30008.000 |
| 837000 | 30025.000 |
| 1674000 | 30047.000 |
| 2650500 | 30046.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 139500 | 580.635 |
| 418500 | 583.729 |
| 837000 | 580.609 |
| 1674000 | 588.444 |
| 2650500 | 578.475 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 139500 | 522.572 |
| 418500 | 525.356 |
| 837000 | 522.548 |
| 1674000 | 529.600 |
| 2650500 | 520.627 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 139500 | 962.281 (±3%) |
| 418500 | 1008.594 (±6%) |
| 837000 | 1052.266 (±3%) |
| 1674000 | 1092.000 (±3%) |
| 2650500 | 1137.672 (±6%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 139500 | 883.047 (±10%) |
| 418500 | 916.578 (±7%) |
| 837000 | 1008.406 |
| 1674000 | 945.375 (±4%) |
| 2650500 | 1039.797 (±5%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 139500 | 868.531 (±4%) |
| 418500 | 878.547 (±4%) |
| 837000 | 853.359 (±4%) |
| 1674000 | 871.969 (±6%) |
| 2650500 | 944.391 (±6%) |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 139500 | 211.156 (±159%) |
| 418500 | 196.391 (±33%) |
| 837000 | 236.531 (±21%) |
| 1674000 | 181.984 (±8%) |
| 2650500 | 235.688 (±43%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 900 | 191718 | 189912 | 213.0 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 139500 | 100 | 0x0252ddaf12d1… | 0x0bac30271b15… | yes |
| 418500 | 100 | 0x0e4604fbc44f… | 0x22a296771cb6… | yes |
| 837000 | 100 | 0x22312496bc26… | 0x1c204052d69f… | yes |
| 1674000 | 100 | 0x0d42cec4109b… | 0x1ea6ae4661b2… | yes |
| 2650500 | 100 | 0x1968ca11d2c0… | 0x2023f31ea64e… | yes |

