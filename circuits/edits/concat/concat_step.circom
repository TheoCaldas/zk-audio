pragma circom 2.1.6;

include "hasher.circom";
include "circom/bitify.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";

// CONCAT: the output is stream A played back followed by stream B.
//
// Unlike merge, which sums two streams sample-for-sample, concat sums
// them in DURATION: N_out = N_a + N_b, and no sample value changes.
//
// === Why a single hash chain would prove nothing ===
//
// The obvious design - fold the output while chaining "the source" -
// collapses: since out[j] === src[j] for every batch, the source chain
// and the output chain absorb identical values and are therefore always
// equal. Such a proof says only "the output is some stream", which is
// vacuous.
//
// What a verifier actually needs is that the output's chain equals A's
// chain *continued by* B's, with A and B independently committed. That
// requires TWO separate source chains, each a public output, covering
// disjoint regions of the playback order.
//
// === Gated chains ===
//
// A Poseidon permutation cannot be skipped, so both source chains are
// evaluated every step and a selector decides which one absorbs the
// batch while the other passes through unchanged:
//
//     phase = (idx >= len_a)                       0 while in A, 1 in B
//     hash_a' = phase ? hash_a : H(hash_a, hb)
//     hash_b' = phase ? H(hash_b, hb) : hash_b
//     hash_out' = H(hash_out, hb)                  always
//
// where hb is the batch hash. The selector costs ONE comparison per
// step - it tests the counter, not individual samples.
//
// === Two BatchHashers, not three ===
//
// Each source chain absorbs its own batch, so A and B are hashed
// separately. The output chain absorbs the SELECTED batch hash, which
// equals `out` by the per-element check, so no third BatchHasher is
// needed - the same number as every other edit, despite carrying an
// extra chain.
//
// === len_a is a runtime public input ===
//
// The split point travels in the IVC state rather than being compiled
// in, so one artifact serves any split. To keep the selector per-step
// rather than per-sample, the circuit requires the boundary to fall on
// a fold boundary:
//
//     len_a === q * (batch_size * comp_size)
//
// with q supplied by the prover. A straddling boundary would force a
// per-sample gate - roughly 3,100 comparisons per step instead of one -
// so this restriction is what keeps concat cheap. Valid splits are
// therefore any whole number of folds, which at bd=8/B=100 is any
// multiple of 3,100 samples (~35ms).
//
// === What this proves, and what it does not ===
//
// It proves: the output is exactly the samples committed in hash_a
// followed by those in hash_b, in that order, with the boundary at
// len_a. Swapping A and B gives a different hash_out, so order is part
// of the statement - unlike merge, where addition commutes.
//
// It does NOT bind either source to a larger original, for the same
// reason crop does not: a sequential chain cannot be opened at an
// interior window. See crop_step.circom for that discussion.

// Selects, per batch element, whichever source the phase dictates, and
// checks the output against it.
//
// Both source batches are supplied every step and the CIRCUIT picks -
// rather than the prover supplying only "the relevant one" - so the
// prover cannot choose which stream a given fold draws from. That
// choice is forced by the counter, which is the whole point of the
// construction.
template ConcatCheck(batch_size) {
    signal input a[batch_size];
    signal input b[batch_size];
    signal input out[batch_size];
    signal input phase;          // 0 => take from A, 1 => take from B

    component pick[batch_size];

    // Verbatim playback: values are unchanged, only their arrangement in
    // time is the edit. As in crop, comparing packed elements is sound
    // because the packing is injective over in-range digits, so no
    // decompression is needed.
    for (var j = 0; j < batch_size; j++) {
        pick[j] = Mux1();
        pick[j].c[0] <== a[j];
        pick[j].c[1] <== b[j];
        pick[j].s <== phase;
        out[j] === pick[j].out;
    }
}

template Concat(batch_size, bit_depth) {
    var comp_size = (256 - bit_depth) / bit_depth;
    var spb = batch_size * comp_size;

    // Width for the counter comparison. 40 bits covers far more samples
    // than any audio this proves.
    var idx_bits = 40;

    //=== IVC state ===//
    // [hash_a, hash_out, hash_b, idx, len_a]
    //
    // Five wide: three chains, the sample counter, and the split point.
    // len_a is carried through unchanged so it stays public and constant
    // across the fold chain - the verifier sets it in z0 and reads the
    // same value back, which is what makes the boundary non-negotiable.
    signal input step_in[5];
    signal output step_out[5];

    //=== Current Batches ===//
    // Both sources are supplied each step; the phase selects which one
    // the output must equal, and which chain absorbs it.
    signal input a[batch_size];
    signal input b[batch_size];
    signal input out[batch_size];

    signal hash_a;
    signal hash_out;
    signal hash_b;
    signal idx;
    signal len_a;
    hash_a <== step_in[0];
    hash_out <== step_in[1];
    hash_b <== step_in[2];
    idx <== step_in[3];
    len_a <== step_in[4];

    //=== The boundary must fall on a fold boundary ===//
    // Otherwise a step would straddle it and the gate would have to be
    // per-sample. The prover supplies the fold count; one constraint
    // pins it.
    signal q;
    q <-- len_a \ spb;
    len_a === q * spb;

    //=== Phase selector: one comparison per step ===//
    component in_b = GreaterEqThan(idx_bits);
    in_b.in[0] <== idx;
    in_b.in[1] <== len_a;
    signal phase;
    phase <== in_b.out;

    //=== Batch hashes ===//
    // Each source chain must absorb ITS OWN batch, so both are hashed.
    // The output chain absorbs whichever one the phase selected, which
    // equals `out` by the check below - so no third BatchHasher is
    // needed.
    component a_batch = BatchHasher(batch_size);
    a_batch.batch <== a;

    component b_batch = BatchHasher(batch_size);
    b_batch.batch <== b;

    component picked_batch = Mux1();
    picked_batch.c[0] <== a_batch.hash;
    picked_batch.c[1] <== b_batch.hash;
    picked_batch.s <== phase;

    //=== Gated source chains ===//
    component a_next = Hasher(2);
    a_next.values[0] <== hash_a;
    a_next.values[1] <== a_batch.hash;

    component b_next = Hasher(2);
    b_next.values[0] <== hash_b;
    b_next.values[1] <== b_batch.hash;

    // While in A, A absorbs and B holds; in B, the reverse.
    component pick_a = Mux1();
    pick_a.c[0] <== a_next.hash;   // phase 0: absorb
    pick_a.c[1] <== hash_a;        // phase 1: hold
    pick_a.s <== phase;

    component pick_b = Mux1();
    pick_b.c[0] <== hash_b;        // phase 0: hold
    pick_b.c[1] <== b_next.hash;   // phase 1: absorb
    pick_b.s <== phase;

    //=== Output chain always absorbs ===//
    component out_next = Hasher(2);
    out_next.values[0] <== hash_out;
    out_next.values[1] <== picked_batch.out;

    step_out[0] <== pick_a.out;
    step_out[1] <== out_next.hash;
    step_out[2] <== pick_b.out;
    step_out[3] <== idx + spb;
    step_out[4] <== len_a;         // carried unchanged

    //=== Verifier Component ===//
    component check = ConcatCheck(batch_size);
    check.a <== a;
    check.b <== b;
    check.out <== out;
    check.phase <== phase;
}
