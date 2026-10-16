# Benchmark Summary

- schema_version: 2
- edit: `gain2`
- audio: `raw_audios/2c_44sr_16bd_01.wav`
- bit_depth: 16
- repeat: 3

`compressed_prove_ms` excludes `CompressedSNARK::setup`; see manifest.json.

## RecursiveSNARK prove (s)

| N | B=10 | B=40 |
|---|---|---|
| 600 | 0.610 (±11%) | 0.317 (±3%) |

## RecursiveSNARK verify (ms)

| N | B=10 | B=40 |
|---|---|---|
| 600 | 38.632 (±5%) | 45.007 (±4%) |

## CompressedSNARK prove (s)

| N | B=10 | B=40 |
|---|---|---|
| 600 | 0.973 | 1.681 (±8%) |

## CompressedSNARK verify (ms)

| N | B=10 | B=40 |
|---|---|---|
| 600 | 41.818 (±3%) | 61.407 (±5%) |

## Proof size (bytes)

| N | B=10 | B=40 |
|---|---|---|
| 600 | 27591.000 | 28347.000 |

## us per sample (prove)

| N | B=10 | B=40 |
|---|---|---|
| 600 | 1017.043 (±11%) | 527.776 (±3%) |

## ms per iteration (prove)

| N | B=10 | B=40 |
|---|---|---|
| 600 | 152.556 (±11%) | 316.665 (±3%) |

## Circuit size (per step, primary)

| B | samples/step | constraints | variables | constraints/sample |
|---|---|---|---|---|
| 10 | 150 | 22878 | 22872 | 152.5 |
| 40 | 600 | 62058 | 62052 | 103.4 |

## Commitments

Per (B, N). Not comparable across B — the batch size sets the hash-tree shape. Within a cell they must be identical across repetitions; diff against an earlier run's table to check a circuit change preserved the relation.

| N | B | original hash | edited hash | reps agree |
|---|---|---|---|---|
| 600 | 10 | 0x01fc62a89680… | 0x2b43c24221f1… | yes |
| 600 | 40 | 0x0778b7c642d8… | 0x1b4b7cff2ade… | yes |

## Skipped / failed cases

| B | N | reason |
|---|---|---|
| 10 | 60 | N < one batch (150 samples/step) |
| 40 | 60 | N < one batch (600 samples/step) |

