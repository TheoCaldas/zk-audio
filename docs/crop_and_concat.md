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
statement, because only the window is folded, so `step_out[0]` is a
commitment to the excerpt rather than to the file.

A verifier sees this as a `bundles agree` failure: the signature covers
all 471 steps of the source, the proof covers the 20 the window needed,
and nothing relates the two commitments. **State this limitation wherever
the `crop` edit is reported.** `crop_bound` below removes it.

## Crop bound to the original

`crop_bound` proves the same relation as crop and additionally ties the
excerpt to the full original: *the edited stream is samples
`[win_start, win_start + win_len)` of the stream committed in
`step_out[0]`, which is a commitment to the whole original.*

### Why a Merkle tree is not needed

Opening a sequential chain at an interior window looks like it requires a
Merkle commitment. It does not, because the prover is not trying to
*avoid* touching the discarded samples — it only has to avoid revealing
them. The Merkle–Damgård state the sponge already produces is a resumable
chaining value, so the circuit can fold **every** batch of the original
into one chain and gate which batches reach the edited chain:

```
hash_orig' = H(hash_orig, hb)                     always
hash_edit' = in_window ? H(hash_edit, hb) : hash_edit
in_window  = (idx >= win_start) && (idx < win_start + win_len)
```

The discarded batches enter the hash and nothing else: they are private
witness, never output, and the relation says nothing about their values.
`hash_orig` therefore terminates at exactly the value a signer computes
over the file, so the existing commitment scheme is unchanged and every
other edit's commitment tables stay valid — which is what the Merkle-tree
approach would have cost.

IVC state is five wide — `(hash_orig, hash_edit, idx, win_start, win_len)`
— with both bounds carried through unchanged so the verifier sets them in
`z0` and reads the same values back. As with concat's join, both bounds
must be fold-aligned, since the gate tests the counter once per step
rather than per sample.

### What it costs

One fold per batch of the **original**, not of the excerpt: proving a
0.7-second window of a 16-second file still folds the whole 16 seconds.
The step count is the entire price — the step itself is *cheaper* than
plain crop's, 8,770 non-linear constraints against 16,686 at bd=16/B=100.
Crop sponges both batches, while here only the original's is absorbed and
the edited chain reuses that digest, which more than pays for the extra
Poseidon(2), the two counter comparisons and the `Mux1`.

That price is intrinsic rather than an artefact of this construction: a
commitment to the whole file cannot be produced without reading the whole
file. A prover unwilling to pay it wants plain `crop` and its weaker
statement.

### What it proves

The published audio is a verbatim contiguous run of the signed original,
at a named position. Several excerpts of one file share a single
`h_orig`, so they all verify against one signature.

It still does not establish that the prover *held* the signed original
rather than signing their own audio — that needs the signature checked
in-circuit, which is unattempted. See README.md, "Trust model".

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
