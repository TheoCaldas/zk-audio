# Benchmark Summary

- schema_version: 2
- edit: `merge`
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
| 3100 | 1.086 |
| 31000 | 7.940 |
| 310000 | 80.910 |
| 2644300 | 678.931 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 129.311 (±4%) |
| 31000 | 218.881 (±5%) |
| 310000 | 223.807 |
| 2644300 | 238.049 (±33%) |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 3100 | 5.712 |
| 31000 | 5.374 (±11%) |
| 310000 | 5.318 (±12%) |
| 2644300 | 5.402 (±5%) |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 178.746 (±37%) |
| 31000 | 178.353 |
| 310000 | 184.296 |
| 2644300 | 177.368 |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 3100 | 30104.000 |
| 31000 | 30105.000 |
| 310000 | 30097.000 |
| 2644300 | 30100.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 3100 | 350.407 |
| 31000 | 256.143 |
| 310000 | 261.000 |
| 2644300 | 256.753 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 3100 | 1086.263 |
| 31000 | 794.042 |
| 310000 | 809.099 |
| 2644300 | 795.934 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 3100 | 1186.922 (±12%) |
| 31000 | 1349.453 |
| 310000 | 1403.844 (±3%) |
| 2644300 | 1503.156 (±4%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 3100 | 966.047 (±20%) |
| 31000 | 1203.562 |
| 310000 | 1330.391 (±8%) |
| 2644300 | 1412.531 (±4%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 3100 | 1138.562 (±8%) |
| 31000 | 1257.531 |
| 310000 | 1328.094 (±5%) |
| 2644300 | 1349.016 |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 3100 | 272.906 (±131%) |
| 31000 | 245.422 (±25%) |
| 310000 | 191.781 (±26%) |
| 2644300 | 285.562 (±16%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 258220 | 255116 | 83.3 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 3100 | 100 | 0x097ea033f2f4… | 0x3f58374a53db… | yes |
| 31000 | 100 | 0x18f24faecb29… | 0x3b6cb4163346… | yes |
| 310000 | 100 | 0x17dd4d3a3b1f… | 0x18ed6fcfe37b… | yes |
| 2644300 | 100 | 0x154c87c46c0b… | 0x2775b7f6887d… | yes |

