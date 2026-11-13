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
one partition            OK    every stage folds bd=8/B=10
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
them differ.

## Floor rounding does not fuse

Stage-by-stage, each edit floors its own result. `f2(f1(x))` with two
floors is not `(f2 . f1)(x)` with one. Any fused circuit must apply the
stages in sequence with a floor at each, matching what the chain proves
and what the Rust references compute — fusing the arithmetic and flooring
once gives a different, unprovable stream.
