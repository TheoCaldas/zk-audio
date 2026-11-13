# Benchmark Summary

- schema_version: 2
- edit: `gain_k2`
- audio: `raw_audios/sweep_605s_2c_44sr_8bd.wav`
- bit_depth: 8
- repeat: 1

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
| 880400 | 164.190 |
| 2644300 | 497.236 |
| 5291700 | 995.596 |
| 10583400 | 1941.819 |
| 26458500 | 4940.551 |
| 52917000 | 10010.112 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 880400 | 198.582 |
| 2644300 | 194.387 |
| 5291700 | 195.535 |
| 10583400 | 192.177 |
| 26458500 | 189.930 |
| 52917000 | 223.199 |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 880400 | 5.562 |
| 2644300 | 5.334 |
| 5291700 | 5.347 |
| 10583400 | 5.519 |
| 26458500 | 5.491 |
| 52917000 | 6.309 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 880400 | 182.377 |
| 2644300 | 190.262 |
| 5291700 | 189.197 |
| 10583400 | 184.232 |
| 26458500 | 180.045 |
| 52917000 | 255.103 |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 880400 | 30057.000 |
| 2644300 | 30049.000 |
| 5291700 | 30020.000 |
| 10583400 | 30066.000 |
| 26458500 | 30022.000 |
| 52917000 | 30034.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 880400 | 186.494 |
| 2644300 | 188.041 |
| 5291700 | 188.143 |
| 10583400 | 183.478 |
| 26458500 | 186.728 |
| 52917000 | 189.166 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 880400 | 578.133 |
| 2644300 | 582.926 |
| 5291700 | 583.243 |
| 10583400 | 568.781 |
| 26458500 | 578.858 |
| 52917000 | 586.415 |

## Peak RSS, whole proof (MB)

| N | B=100 |
|---|---|
| 880400 | 1015.156 |
| 2644300 | 1080.016 |
| 5291700 | 1240.469 |
| 10583400 | 1281.797 |
| 26458500 | 1526.688 |
| 52917000 | 2014.141 |

## Peak RSS during folding (MB)

| N | B=100 |
|---|---|
| 880400 | 1015.156 |
| 2644300 | 1026.703 |
| 5291700 | 1116.953 |
| 10583400 | 1281.797 |
| 26458500 | 1326.078 |
| 52917000 | 1396.812 |

## Peak RSS during compression (MB)

| N | B=100 |
|---|---|
| 880400 | 919.453 |
| 2644300 | 1017.844 |
| 5291700 | 1050.812 |
| 10583400 | 1039.688 |
| 26458500 | 1221.641 |
| 52917000 | 1078.219 |

## Peak RSS over baseline (MB)

| N | B=100 |
|---|---|
| 880400 | 628.359 |
| 2644300 | 162.922 |
| 5291700 | 326.359 |
| 10583400 | 328.656 |
| 26458500 | 238.562 |
| 52917000 | 331.594 |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 3100 | 234318 | 228112 | 75.6 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 880400 | 100 | 0x1c0494c2f406… | 0x3085262506c9… | single rep |
| 2644300 | 100 | 0x34e150afbc7c… | 0x0185ab3e0d07… | single rep |
| 5291700 | 100 | 0x07e337436c79… | 0x159e04c0c26d… | single rep |
| 10583400 | 100 | 0x2971e723b5da… | 0x1a26c63d4ac5… | single rep |
| 26458500 | 100 | 0x27a2d76877cd… | 0x30b79a7c3f59… | single rep |
| 52917000 | 100 | 0x1ec55a795488… | 0x3eef074b1d60… | single rep |

