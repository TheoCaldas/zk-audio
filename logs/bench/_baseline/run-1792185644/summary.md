# Benchmark Summary

- schema_version: 2
- edit: `gain2`
- audio: `raw_audios/sweep_120s_2c_44sr_16bd.wav`
- bit_depth: 16
- repeat: 1

`compressed_prove_ms` excludes `CompressedSNARK::setup`; see manifest.json.

## RecursiveSNARK prove (s)

| N | B=100 |
|---|---|
| 882000 | 237.438 |
| 2646000 | 765.389 |
| 5292000 | 1534.865 |
| 10584000 | 2944.818 |

## RecursiveSNARK verify (ms)

| N | B=100 |
|---|---|
| 882000 | 145.558 |
| 2646000 | 133.120 |
| 5292000 | 160.664 |
| 10584000 | 240.394 |

## CompressedSNARK prove (s)

| N | B=100 |
|---|---|
| 882000 | 5.417 |
| 2646000 | 5.334 |
| 5292000 | 5.468 |
| 10584000 | 5.480 |

## CompressedSNARK verify (ms)

| N | B=100 |
|---|---|
| 882000 | 182.424 |
| 2646000 | 183.299 |
| 5292000 | 170.578 |
| 10584000 | 202.562 |

## Proof size (bytes)

| N | B=100 |
|---|---|
| 882000 | 30101.000 |
| 2646000 | 30055.000 |
| 5292000 | 30058.000 |
| 10584000 | 30082.000 |

## us per sample (prove)

| N | B=100 |
|---|---|
| 882000 | 269.204 |
| 2646000 | 289.263 |
| 5292000 | 290.035 |
| 10584000 | 278.233 |

## ms per iteration (prove)

| N | B=100 |
|---|---|
| 882000 | 403.806 |
| 2646000 | 433.894 |
| 5292000 | 435.053 |
| 10584000 | 417.349 |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 100 | 1500 | 140418 | 140412 | 93.6 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 882000 | 100 | 0x0e19cef40179… | 0x1a9a6f7e9b18… | single rep |
| 2646000 | 100 | 0x34fb46770284… | 0x2b141dec69f5… | single rep |
| 5292000 | 100 | 0x14e35d14320d… | 0x21911647be1f… | single rep |
| 10584000 | 100 | 0x2a699f3c32f3… | 0x2e0c2cf72fe9… | single rep |

