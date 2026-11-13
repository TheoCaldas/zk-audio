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
| 1500 | 0.776 (±6%) |
| 15000 | 5.611 |
| 150000 | 55.067 |
| 2646000 | 969.574 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 1500 | 102.656 (±4%) |
| 15000 | 205.062 |
| 150000 | 208.745 |
| 2646000 | 187.614 |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 1500 | 5.993 (±5%) |
| 15000 | 6.235 (±3%) |
| 150000 | 5.929 |
| 2646000 | 5.588 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 1500 | 218.254 (±6%) |
| 15000 | 203.517 (±12%) |
| 150000 | 202.666 (±5%) |
| 2646000 | 186.552 (±7%) |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 1500 | 29956.000 |
| 15000 | 30057.000 |
| 150000 | 30052.000 |
| 2646000 | 29974.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 1500 | 517.011 (±6%) |
| 15000 | 374.096 |
| 150000 | 367.112 |
| 2646000 | 366.430 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 1500 | 775.516 (±6%) |
| 15000 | 561.144 |
| 150000 | 550.668 |
| 2646000 | 549.645 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 1500 | 867.625 (±12%) |
| 15000 | 940.281 (±5%) |
| 150000 | 958.453 (±4%) |
| 2646000 | 1225.375 (±26%) |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 1500 | 683.875 (±17%) |
| 15000 | 882.516 (±3%) |
| 150000 | 937.047 (±6%) |
| 2646000 | 1073.188 (±25%) |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 1500 | 867.625 (±7%) |
| 15000 | 888.391 (±3%) |
| 150000 | 851.594 |
| 2646000 | 1111.328 (±13%) |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 1500 | 453.000 (±45%) |
| 15000 | 362.406 (±21%) |
| 150000 | 99.922 (±75%) |
| 2646000 | 265.938 (±33%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 1500 | 207918 | 204912 | 138.6 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 1500 | 100 | 0x2ae577530ef0… | 0x1a0b45cc65b1… | yes |
| 15000 | 100 | 0x3a8697dd64c2… | 0x17ff86130cf5… | yes |
| 150000 | 100 | 0x3f56c8465ac9… | 0x0039e3ae676d… | yes |
| 2646000 | 100 | 0x118f1680e819… | 0x16c9d7c6bfd7… | yes |

