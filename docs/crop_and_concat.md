# Crop and concat: edits that change the sample count

Gain, attenuation, tremolo and distortion are one-in-one-out: `N_edit ==
N_orig`, and the step circuit folds both streams in lockstep under
`edit[i] = f(orig[i])`. Crop and concat break that invariant, so they need
a third IVC state element.

## Crop

Proves: *the edited stream is a verbatim, contiguous run of samples drawn
from a stream whose window commitment is `step_out[0]`, beginning at
absolute index `step_in[2]` and ending at `step_out[2]`.*

The output is shorter than the input, so there is no per-sample
correspondence across the whole file. The circuit folds **only the kept
window**: the prover never presents the discarded samples, so every step
does have a one-to-one correspondence and the relation collapses to the
identity. What makes it a crop rather than a no-op is the IVC counter —
the window's start and end are public, so the statement says *which*
contiguous run was kept.

The bounds cost nothing extra. No additional public signals and no
per-step range checks are needed, because the counter carries them:

```
step_in[2]  at the first fold  == start   (verifier supplies z0)
step_out[2] at the last fold   == end     (verifier reads it)
```

Each step advances the counter by exactly one batch, so a prover cannot
claim a different window without changing values the verifier already
sees.

**No decompression.** Unlike every other edit, crop does not call
`Decompressor`. Comparing packed field elements directly is sound: the
packing is a positional base-`2^bit_depth` encoding, injective over
in-range digits, so `orig[j] == edit[j]` as field elements implies the
underlying samples are equal digit for digit. Decomposing to compare
per-sample would add thousands of constraints and prove nothing extra.
This makes crop the cheapest edit in the project — essentially the
Poseidon hashing every edit pays, plus `batch_size` field equalities.

### Limitation: the excerpt is not bound to a larger original

Crop proves the excerpt is unmodified and contiguous, and pins its
position relative to the counter's origin. It does **not** bind the
excerpt to a specific larger file: the outer file's hash never enters the
statement, because a sequential Merkle–Damgård chain cannot be opened at
an interior window.

Proving "this excerpt is positions 1000..2499 of the file with root R"
would need the original committed as a Merkle tree, which changes the
commitment scheme for every edit and invalidates existing commitment
tables. Left as future work. **State this limitation wherever the crop
edit is reported.**

### Composition caveat

The running hash depends on the entire prefix, so crop does not commute
with the pointwise edits: crop-then-gain and gain-then-crop yield
different commitments. The other edits commute with each other; crop is
the first where order is part of the statement.

## Concat

The output is stream A followed by stream B: `N_out = N_a + N_b`, with no
sample value changed. IVC state is five wide —
`(hash_a, hash_out, hash_b, idx, len_a)`.

### Why a single hash chain would prove nothing

Folding the output while chaining "the source" collapses: since
`out[j] === src[j]` for every batch, the source chain and the output chain
absorb identical values and are always equal. Such a proof says only "the
output is some stream", which is vacuous.

A verifier needs the output's chain to equal A's chain *continued by*
B's, with A and B independently committed. That requires two separate
source chains, each a public output, covering disjoint regions of the
playback order.

### Gated chains

A Poseidon permutation cannot be skipped, so both source chains are
evaluated every step and a selector decides which one absorbs the batch
while the other passes through unchanged:

```
phase     = (idx >= len_a)                  0 while in A, 1 in B
hash_a'   = phase ? hash_a : H(hash_a, hb)
hash_b'   = phase ? H(hash_b, hb) : hash_b
hash_out' = H(hash_out, hb)                 always
```

where `hb` is the batch hash. The selector costs one comparison per step:
it tests the counter, not individual samples. Only two `BatchHasher`s are
needed, since the output chain absorbs the selected batch hash, which
equals `out` by the per-element check.

Both source batches are supplied every step and the circuit picks, rather
than the prover supplying only "the relevant one", so the prover cannot
choose which stream a fold draws from.

### The boundary must be fold-aligned

`len_a` travels in the IVC state rather than being compiled in, so one
artifact serves any split. To keep the selector per-step rather than
per-sample, the boundary must fall on a fold boundary:

```
len_a === q * (batch_size * comp_size)
```

with `q` supplied by the prover. A straddling boundary would force a
per-sample gate — roughly 3,100 comparisons per step instead of one. At
bd=8/B=100 valid splits are any multiple of 3,100 samples (~35 ms).

### What it proves

The output is exactly the samples committed in `hash_a` followed by those
in `hash_b`, in that order, with the boundary at `len_a`. Swapping A and B
gives a different `hash_out`, so order is part of the statement — unlike
merge, where addition commutes.

It does not bind either source to a larger original, for the same reason
crop does not.
