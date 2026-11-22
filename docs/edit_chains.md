# Edit chains

More than one operation per input. `src/chain.rs` composes edits
sequentially: one proof per stage, linked by commitment equality.

```
  orig --[gain]--> s1 --[atten]--> s2 --[dist]--> edited
        pi_1            pi_2            pi_3
```

The verifier checks `n` proofs, `n-1` link equalities, one recomputed
commitment and one signature. This is the baseline against which a fused
composite circuit is measured, and it is deliberately not optimised: its
cost is the sum of its stages.

## Driving it from the CLI

Options **D** (prove a chain) and **E** (verify a chain bundle), alongside
the single-edit **B** and **C**.

D asks for one bit depth and batch size for the whole chain — the link is
a commitment, and a commitment is a fold over a partition, so two stages
under different partitions could not link at all — then the stage list,
then the source. It writes one `ProofBundle` per stage plus a
`ChainBundle` naming them in order.

E loads the manifest and its stages and reports each check separately:

```
one partition            OK    every stage folds bd=8/B=10, padded to 620
manifest ends            OK    the bundle's h_orig and h_edit are the first and last stages'
stage links              OK    1 links hold: each stage consumed what the previous produced
source bound             OK    the signed source is the chain's input (N=620)
stage 1 gain_k2          OK    verified, 2 steps, commitments match
stage 2 atten_m1         OK    verified, 2 steps, commitments match
published audio          OK    recomputed h_edit matches the last stage (620 samples)
signature                OK    valid over (h_orig, N=620), using the key in the bundle
```

The signature must cover the chain's *input*, at the same sample count the
chain was proven over. Option A takes a sample count for that reason: the
length is part of the signed statement, so a signature over more samples
than were proven cannot be bound to the commitment.

A reordered chain is rejected on three checks at once — broken link,
unbound source, mismatched audio — while both stage proofs still verify
individually. That is the attack the link check exists for.

The `ChainBundle` deliberately stores **no links**. They are recomputed
from the stage proofs, because a manifest asserting a link its proofs do
not show is the one thing a verifier must never take on trust.

## What links the stages

Stage `k`'s edited-stream commitment is stage `k+1`'s original-stream
commitment. Both are computed *in-circuit*, by the same sponge-4
absorption over the same partition, so the equality is a string
comparison on values neither side can choose:

```
stages[k].h_out == stages[k+1].h_in
```

This works with no change to `stream_commitment`, because a commitment is
a pure function of a stream: whatever the verifier would recompute for an
intermediate is what both adjacent circuits already folded. Nothing
off-circuit sits in the seam.

Only the chain's input is signed, and only its output is recomputed from
published audio. The intermediate streams are never published and get no
signature — a stage's `h_out` is meaningful because the next stage's
proof consumed it, not because anyone vouched for it.

`broken_link_is_rejected` in `tests/chain_end_to_end.rs` pins the attack
this prevents: two individually valid proofs about unrelated streams,
presented as a chain.

## Cost: the measured baseline

Three stages, bd 8, B 10, N 1240 (4 folds per stage), compression on:

| stage | constraints/step | pp | fold | c-setup | c-prove |
|---|---|---|---|---|---|
| gain_k2 | 27,975 | 2,175 | 480 | 68 | 2,648 |
| atten_m1 | 22,395 | 2,007 | 466 | 65 | 2,606 |
| dist_d100 | 40,995 | 2,947 | 530 | 87 | 4,494 |
| **total** | **91,365** | 7,129 | **1,475** | 220 | **9,748** |

Total prove wall: 18,573 ms, of which the compression toll is 9,968 ms —
**54%**, paid three times for what is logically one edit. On short audio
that toll dominates; on long audio the fold time does, and chaining then
costs close to the true sum of stages.

Two separable sources of waste, which set up the two optimisations:

- **Per-stage compression.** `CompressedSNARK` cost tracks per-step
  circuit size, not chain length, so it is a fixed toll per *proof*. A
  single proof covering all stages pays it once.
- **Intermediate decompression and rehashing.** Each interior stream is
  decomposed twice (as one stage's output, as the next stage's input) and
  hashed twice. `crop` — identity, no decompression — is 52,608
  constraints/step at B=100 against gain's 173,705, so the `Num2Bits`
  decomposition, not the relation, is most of a stage's cost.

A fused step circuit avoids both: decompress once, apply the stages in
registers, recompress once, hash only the endpoints.

## Which edits can be stages

`gain`, `atten`, `dist`, `tremolo` compose anywhere. Tremolo's proven
counter is per-stage and independent of its neighbours.

`crop` and `merge` are supported **only as the last stage**, enforced by
`chain::validate` before any proving happens:

- **crop** changes the stream length. A chain links stages by a
  commitment over a stream padded to a fold boundary, and a length change
  moves that boundary, so an interior crop would hand the next stage a
  stream whose padding the preceding commitment does not cover.
- **merge** takes a second stream. As a terminal stage it is exactly the
  useful case: a verifier holding a signature over stem B can check the
  published mix contains precisely that stem, since `h_b` is proven.

`concat` is **not supported as a stage**. Its gated source chains commit
to either side of the join separately (`h_a` covers A, `h_b` covers B),
so a preceding stage's single whole-stream `h_out` matches neither. A
preceding stage would have to be proven twice, once per side, against the
two spans — which is a different construction, not a link. Future work.

## Padding across stages

Each stage runs on the stream padded to a fold boundary and hands the
padded result to the next, so a stage sees exactly the samples the
preceding stage's commitment covers. The relation therefore has to hold
on the pad too, which is why the edit is applied to the padded stream
rather than trimmed between stages — `edit(0)` is not 0 for every edit.

`n_samples` (the chain's input length, signed) and `n_samples_out` (the
published length) are carried separately, because a terminal crop makes
them differ. The bundle also carries `padded_samples`, the boundary every
stage folds to: the published WAV is trimmed to `n_samples_out`, so a
verifier has to restore the pad before recomputing `h_edit`, and the pad
it restores is the **last stage's edit applied to silence**, not silence.
`audio_parser::pad_edited_to_step` does that; see README.md, "How a
verifier rebuilds the pad".

## Floor rounding does not fuse

Stage-by-stage, each edit floors its own result. `f2(f1(x))` with two
floors is not `(f2 . f1)(x)` with one. Any fused circuit must apply the
stages in sequence with a floor at each, matching what the chain proves
and what the Rust references compute — fusing the arithmetic and flooring
once gives a different, unprovable stream.

## Fusion: one proof for the whole composite

`src/fused.rs` proves `edit = f3(f2(f1(orig)))` in a single step circuit.
The input is decompressed once, each stage's relation template is applied
digit-wise in signals, the result is recompressed once, and only the
endpoints are hashed. The chain remains the control; both are built and
both verify.

The circuits are generated, not hand-written, because the artefact set is
a product of family, bit depth, parameters and stage order:

```bash
python3 scripts/gen_fused_circuit.py gain_k2 atten_m1 dist_d100 --bd 8 --B 10
./compile.circuit.sh circuits/edits/_fused/fused_gain_k2_atten_m1_dist_d100_b8_B10.circom
```

The generator reuses `ClampedGain`, `AttenuatedGain`, `DistortSample` and
`ModulateSample` from the family step circuits unmodified, so a fused
circuit cannot drift from the stages it fuses. A stage list containing
`crop`, `concat`, `merge` or `gain2` is rejected with the reason.

### Measured: A against B

`cargo run --release --bin compare_chain -- --stages <list> --bd 8 --B 100 --n <N>`
proves the same composite both ways on one input and reports the delta;
`--csv` appends a row per side and `scripts/analyze_fusion.py` turns the
file into tables, a fitted cost model and a figure. The full sweep is
`logs/fusion/` (summary.md, the CSV, the figure).

Two chains at bd 8, B 100, 0.1 s to 30 s of 44.1 kHz stereo:

| audio | folds | gain->atten->dist | gain->tremolo |
|---|---|---|---|
| 0.11 s | 3 | 44.3% | 12.5% |
| 1.02 s | 29 | 41.0% | 8.3% |
| 5.03 s | 143 | 39.3% | 20.1% |
| 10.02 s | 285 | 38.0% | 23.2% |
| 30.02 s | 854 | **34.4%** | **25.0%** |

At 30 s the three-stage chain falls from 989 s to 649 s (16.5 min to
10.8 min) and the tremolo pair from 654 s to 491 s.

**The saving is a curve between two computable ratios, not a constant,
and the two chains approach it from opposite ends.** Each interpolates
from its one-fold saving to its folding-saving as folds accumulate:

| | one fold | 30 s | direction |
|---|---|---|---|
| gain->atten->dist | 45% | 34.4% | falls |
| gain->tremolo | 5% | 25.0% | rises |

What decides which end a chain starts from is **power-of-two padding**,
not the constraint counts. Spartan pads a proof's constraint count to the
next power of two, so the compression toll tracks the padded size and
fusion's compression saving is the ratio of padded totals:

| chain | chain padded | fused padded | predicted | measured |
|---|---|---|---|---|
| gain->atten->dist | 2^18+2^17+2^19 = 917,504 | 2^19 = 524,288 | 43% | 43.3% |
| gain->tremolo | 2^18+2^18 = 524,288 | 2^19 = 524,288 | 0% | 9.7% |

The tremolo pair fuses two circuits that each fit under 2^18 into one
that crosses into 2^19, so its padded total does not shrink and fusion
buys nothing up front - its whole saving comes from folding a 20% smaller
circuit. The 9.7% residual is per-proof overhead that one Spartan
invocation avoids regardless of padding. A chain whose fused circuit
lands just under a power of two therefore gains on both terms, which
makes batch size a tuning knob for fusion and not only for the fold
count.

Asymptotically the saving is the folding ratio, measured at the largest
case rather than extrapolated: 33.6% for the three-stage chain and 26.3%
for the tremolo pair, where folding is over 90% of the total. A
least-squares fit through all eight points agrees (33.4% and 26.9%), but
only once the large-N points are in: per-fold cost falls about 15% from 3
folds to 854 as Nova's per-step overhead amortises, and a fit over short
cases alone overstates the slope.

Compression time is flat in N for a fixed circuit, as IVC predicts -
54-62 s for the chain and 30-36 s for the fused circuit across a 285x
range in N. The variation within that is thermal throttling on a laptop
under sustained load, not a size effect; `analyze_fusion.py` warns when
it exceeds 15% so an affected point is not read as one. Savings ratios
survive it because both sides are measured minutes apart under the same
conditions, but absolute times from late points in a long sweep should
not be quoted against early ones.

### What fusion gives up

The intermediate streams get no commitment. Nothing can later be proven
about the half-edited audio, and there is no stage-by-stage audit trail:
a verifier checks one proof instead of n proofs and n-1 links. When the
intermediates must be referable — composing proofs made by different
parties at different times — the state has to widen to carry their
running commitments, which costs the sponge absorptions but still skips
the decompression.

### The range-check the composition rests on

A single stage decompresses its input, so every digit it reads was
range-checked by `Num2Bits`. A fused stage after the first reads a digit
produced by the preceding stage's arithmetic, which no `Decompressor`
checks. The next stage's comparators and the final `Recompressor` both
rely on that digit fitting in `bit_depth` bits, so every stage must bound
its own output: gain's clamp, atten's shift and tremolo's `L <= 2^16` all
do, and `gen_fused_circuit.py` refuses to place an unbounded stage
anywhere but last.
