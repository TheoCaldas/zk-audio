pragma circom 2.1.6;

include "hasher.circom";
include "decompressor.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";

// Generalized gain step circuit: proves `edit = clamp(k * orig)` for an
// arbitrary integer factor k > 1, at any supported bit depth, with
// saturation at the limits of the sample's bit representation.
//
// Relation to `multiplier_step_b16.circom` (the gain2 family): that
// circuit is the special case k=2, bit_depth=16, WITHOUT clamping. It is
// kept as-is, because its benchmark results depend on that exact
// relation. This file does not replace it.
//
// === The normalized-space trick ===
//
// Samples reach the circuit normalized: `orig_n = orig_signed + 2^(bd-1)`,
// so orig_n is in [0, 2^bd - 1] and never negative (field elements have
// no sign, and `LessThan` needs non-negative inputs).
//
// Multiplying normalized values directly would be wrong - k*orig_n is not
// the normalization of k*orig_signed. Instead we work in the *offset*
// space k*orig_n = k*orig_signed + k*2^(bd-1), where the signed bounds
// [-2^(bd-1), 2^(bd-1)-1] map to:
//
//     lo = k*2^(bd-1) - 2^(bd-1)      (signed MIN)
//     hi = k*2^(bd-1) + 2^(bd-1) - 1  (signed MAX)
//
// We clamp `raw = k*orig_n` into [lo, hi], then subtract the surplus
// offset (k-1)*2^(bd-1) to land back in normalized edit space. Every
// intermediate stays non-negative, so no signed comparison is needed.
// (Verified exhaustively against the Rust reference for bd in {8,16}.)

// Clamps one already-normalized sample and returns the normalized edit
// digit. `n_bits` must be wide enough to hold k*(2^bd - 1).
template ClampedGain(bit_depth, k, n_bits) {
    signal input orig;      // normalized: orig_signed + 2^(bd-1)
    signal output edit;     // normalized: clamp(k*orig_signed) + 2^(bd-1)

    var half = 1 << (bit_depth - 1);
    var lo = k * half - half;
    var hi = k * half + half - 1;

    signal raw;
    raw <== orig * k;

    // over = (raw > hi), under = (raw < lo)
    component is_over = GreaterThan(n_bits);
    is_over.in[0] <== raw;
    is_over.in[1] <== hi;

    component is_under = LessThan(n_bits);
    is_under.in[0] <== raw;
    is_under.in[1] <== lo;

    // Select hi when over, else (lo when under, else raw). Two muxes
    // rather than arithmetic blending, so each branch is one constraint.
    component pick_low = Mux1();
    pick_low.c[0] <== raw;
    pick_low.c[1] <== lo;
    pick_low.s <== is_under.out;

    component pick_high = Mux1();
    pick_high.c[0] <== pick_low.out;
    pick_high.c[1] <== hi;
    pick_high.s <== is_over.out;

    // Shift out the surplus offset introduced by multiplying the
    // already-offset value: k*(x + half) = k*x + k*half, but normalized
    // edit space wants k*x + half.
    edit <== pick_high.out - (k - 1) * half;
}

// Derives the digits `edit` must equal from orig's (already
// range-checked) digits, applying the clamped gain per digit.
template ExpectedEditGain(size, bit_depth, k, n_bits) {
    signal input orig[size];
    signal output edit[size];

    component g[size];
    for (var i = 0; i < size; i++) {
        g[i] = ClampedGain(bit_depth, k, n_bits);
        g[i].orig <== orig[i];
        edit[i] <== g[i].edit;
    }
}

template BatchCheckGain(batch_size, bit_depth, k) {
    var comp_size = (256 - bit_depth) / bit_depth;

    // Width needed to compare k*(2^bit_depth - 1) without wrapping.
    // Computed here rather than passed in, so a caller cannot silently
    // under-size the comparator and break soundness.
    var raw_max = k * ((1 << bit_depth) - 1);
    var n_bits = 0;
    var probe = raw_max;
    while (probe > 0) {
        n_bits++;
        probe = probe >> 1;
    }

    signal input orig[batch_size];
    signal input edit[batch_size];

    component decompressor_orig[batch_size];
    component expected[batch_size];
    component recompressor[batch_size];

    for (var j = 0; j < batch_size; j++) {
        // Decompress the original only. `edit` is checked against a
        // value we compute and repack ourselves - the same optimization
        // the gain2 circuit uses.
        decompressor_orig[j] = Decompressor(bit_depth);
        decompressor_orig[j].in <== orig[j];

        expected[j] = ExpectedEditGain(comp_size, bit_depth, k, n_bits);
        expected[j].orig <== decompressor_orig[j].out;

        // Safe to repack: ClampedGain's output is provably within
        // [0, 2^bit_depth - 1] because it was clamped to [lo, hi] and
        // then shifted by exactly (k-1)*half. Unlike the unclamped
        // gain2 circuit, this holds for EVERY input rather than relying
        // on the audio having headroom.
        recompressor[j] = Recompressor(bit_depth);
        recompressor[j].in <== expected[j].edit;

        edit[j] === recompressor[j].out;
    }
}

template Gain(batch_size, bit_depth, k) {
    //=== Previous Hashes ===//
    signal input step_in[2]; // (prev_orig_hash, prev_edit_hash)

    //=== Next Hashes ===//
    signal output step_out[2]; // (next_orig_hash, next_edit_hash)

    //=== Current Batches ===//
    signal input orig[batch_size];
    signal input edit[batch_size];

    //=== Hash Components ===//
    component orig_batch_hasher = BatchHasher(batch_size);
    component edit_batch_hasher = BatchHasher(batch_size);
    component orig_hasher = Hasher(2);
    component edit_hasher = Hasher(2);

    orig_batch_hasher.batch <== orig;
    orig_hasher.values[0] <== step_in[0];
    orig_hasher.values[1] <== orig_batch_hasher.hash;
    step_out[0] <== orig_hasher.hash;

    edit_batch_hasher.batch <== edit;
    edit_hasher.values[0] <== step_in[1];
    edit_hasher.values[1] <== edit_batch_hasher.hash;
    step_out[1] <== edit_hasher.hash;

    //=== Verifier Component ===//
    component check = BatchCheckGain(batch_size, bit_depth, k);
    check.orig <== orig;
    check.edit <== edit;
}
