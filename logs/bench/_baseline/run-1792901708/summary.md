# Benchmark Summary

- schema_version: 2
- edit: `dist_d100`
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
| 3100 | 1.141 (±4%) |
| 31000 | 8.484 |
| 310000 | 82.247 (±3%) |
| 2644300 | 700.458 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 134.280 (±3%) |
| 31000 | 241.076 |
| 310000 | 245.552 (±8%) |
| 2644300 | 251.031 (±3%) |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 3100 | 10.463 (±7%) |
| 31000 | 10.626 (±3%) |
| 310000 | 10.404 (±3%) |
| 2644300 | 10.389 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 3100 | 360.310 |
| 31000 | 344.185 |
| 310000 | 352.207 (±3%) |
| 2644300 | 350.322 (±4%) |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 3100 | 30846.000 |
| 31000 | 30899.000 |
| 310000 | 30878.000 |
| 2644300 | 30864.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 3100 | 367.909 (±4%) |
| 31000 | 273.689 |
| 310000 | 265.314 (±3%) |
| 2644300 | 264.894 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 3100 | 1140.518 (±4%) |
| 31000 | 848.437 |
| 310000 | 822.475 (±3%) |
| 2644300 | 821.170 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 3100 | 1677.344 (±4%) |
| 31000 | 1738.453 (±5%) |
| 310000 | 1712.141 (±5%) |
| 2644300 | 1595.125 (±3%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 3100 | 1082.422 (±9%) |
| 31000 | 1185.062 (±6%) |
| 310000 | 1235.172 (±3%) |
| 2644300 | 1209.688 (±3%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 3100 | 1623.922 |
| 31000 | 1687.266 |
| 310000 | 1598.375 (±6%) |
| 2644300 | 1516.344 (±3%) |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 3100 | 826.797 (±49%) |
| 31000 | 756.484 (±5%) |
| 310000 | 687.469 (±4%) |
| 2644300 | 628.031 (±6%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 364518 | 352112 | 117.6 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 3100 | 100 | 0x097ea033f2f4… | 0x0c765db83eec… | yes |
| 31000 | 100 | 0x18f24faecb29… | 0x1fc30308ad76… | yes |
| 310000 | 100 | 0x17dd4d3a3b1f… | 0x0d06e68ff14f… | yes |
| 2644300 | 100 | 0x154c87c46c0b… | 0x027a0361a01c… | yes |

