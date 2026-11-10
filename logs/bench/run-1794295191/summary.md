# Benchmark Summary

- schema_version: 2
- edit: `gain_k2`
- audio: `raw_audios/sweep_31s_2c_44sr_16bd_fullscale.wav`
- bit_depth: 16
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
| 139500 | 51.004 |
| 418500 | 150.805 |
| 837000 | 303.343 |
| 1674000 | 601.015 |
| 2650500 | 956.103 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 139500 | 186.330 |
| 418500 | 186.269 |
| 837000 | 187.085 |
| 1674000 | 187.210 |
| 2650500 | 208.896 (±9%) |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 139500 | 5.577 |
| 418500 | 5.473 (±4%) |
| 837000 | 5.517 |
| 1674000 | 5.456 (±4%) |
| 2650500 | 5.967 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 139500 | 176.923 (±10%) |
| 418500 | 187.694 (±6%) |
| 837000 | 184.228 (±3%) |
| 1674000 | 192.845 (±5%) |
| 2650500 | 180.618 (±8%) |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 139500 | 30012.000 |
| 418500 | 30068.000 |
| 837000 | 30053.000 |
| 1674000 | 30047.000 |
| 2650500 | 30038.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 139500 | 365.621 |
| 418500 | 360.347 |
| 837000 | 362.417 |
| 1674000 | 359.029 |
| 2650500 | 360.726 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 139500 | 548.431 |
| 418500 | 540.521 |
| 837000 | 543.625 |
| 1674000 | 538.544 |
| 2650500 | 541.088 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 139500 | 1055.438 (±9%) |
| 418500 | 1171.094 (±7%) |
| 837000 | 1204.766 (±5%) |
| 1674000 | 1190.531 (±4%) |
| 2650500 | 1222.297 (±3%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 139500 | 862.594 (±12%) |
| 418500 | 1095.812 (±7%) |
| 837000 | 1097.047 (±4%) |
| 1674000 | 1036.828 (±8%) |
| 2650500 | 1201.844 (±3%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 139500 | 961.359 (±4%) |
| 418500 | 1040.984 |
| 837000 | 1027.328 (±7%) |
| 1674000 | 962.047 (±5%) |
| 2650500 | 895.875 (±5%) |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 139500 | 270.734 (±124%) |
| 418500 | 201.328 (±26%) |
| 837000 | 269.172 (±20%) |
| 1674000 | 133.906 (±47%) |
| 2650500 | 323.703 (±13%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 1500 | 207918 | 204912 | 138.6 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 139500 | 100 | 0x234ad5459101… | 0x14ad2b764466… | yes |
| 418500 | 100 | 0x123f3afe639f… | 0x2ef9271ef445… | yes |
| 837000 | 100 | 0x29ef06cc2d37… | 0x1029a72ab965… | yes |
| 1674000 | 100 | 0x0a5ac0484f1f… | 0x0bf08c5cc6e3… | yes |
| 2650500 | 100 | 0x2ff74ff70fcd… | 0x07457fe0b403… | yes |

