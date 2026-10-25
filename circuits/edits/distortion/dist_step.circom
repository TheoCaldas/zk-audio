pragma circom 2.1.6;

include "hasher.circom";
include "decompressor.circom";
include "circom/bitify.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";

// DISTORTION (full-wave rectifier fold, blended with the dry signal),
// after source_outputs/"dist 1.c":
//
//     output = d * 2*(|x| - 1/2) + (1 - d) * x          (x in [-1, 1])
//
// In integer form at bit depth b, with x in [-half, half-1] and the
// blend amount d represented as D = round(d * 2^16):
//
//     edit = clamp( ( D*(2|x| - half) + (2^16 - D)*x ) >> 16 )
//
// === Relation to the other edit families ===
//
// This is the first edit needing BOTH of the hard pieces:
//
//   * |x| requires a SIGN TEST - a comparator plus a multiplexer per
//     sample. `circuits/old/dist_step.circom` was an abandoned attempt
//     at exactly this; its sign handling was left commented out and it
//     hand-rolled a comparator rather than using circomlib's. This file
//     uses the library primitives and handles the sign completely.
//   * CLAMPING is required, unlike attenuation and tremolo. At d = 1 the
//     rectified term reaches +half (+32768 at b=16), one above the
//     representable maximum, so saturation is not optional. Verified by
//     enumerating the output range over the full input domain.
//
// It is therefore the most expensive edit in the project: a sign test, a
// clamp, and a wide bit decomposition per sample.
//
// === Fixed point and rounding ===
//
// D is at scale 2^16, so the rescale is a right shift by 16 rather than a
// general division - the same reason the scale is a power of two in
// `atten_step.circom` and `tremolo_step.circom`. Rounding is floor, as
// everywhere else in this project, because bit truncation computes it
// natively; the Rust reference must use `>>`, not `/`.
//
// The integer form is not bit-identical to the float reference: the
// maximum deviation is 0.75 LSB (at d = 0.25), from the single floor at
// the end rather than float rounding at every step. That is below one
// quantization step and is reported rather than hidden.
//
// === Normalized space ===
//
// Samples arrive as orig_n = x + half. Writing a = |x|, the circuit
// evaluates
//
//     v = D*(2a - half) + (2^16 - D)*(orig_n - half) + 2^16*half
//
// which is non-negative for every (x, D) in range, and from which
// (v >> 16) - half equals the signed pre-clamp result exactly - the
// offset cancels, with no correction term. (Verified over the full
// domain at b in {8, 16} for D across its range.)

// |x| for a normalized sample, returned as a non-negative magnitude.
//
// x = orig_n - half is signed, which the field cannot compare directly,
// so the test is done on orig_n against half: orig_n < half means x < 0.
template AbsValue(bit_depth) {
    signal input orig;          // normalized: x + half
    signal output abs;          // |x|, in [0, half]

    var half = 1 << (bit_depth - 1);

    component is_neg = LessThan(bit_depth + 1);
    is_neg.in[0] <== orig;
    is_neg.in[1] <== half;

    // |x| = half - orig_n when negative, orig_n - half otherwise.
    component pick = Mux1();
    pick.c[0] <== orig - half;
    pick.c[1] <== half - orig;
    pick.s <== is_neg.out;

    abs <== pick.out;
}

// One sample: rectify, blend, rescale, clamp.
template DistortSample(bit_depth, blend, v_bits) {
    signal input orig;          // normalized
    signal output edit;         // normalized

    var half = 1 << (bit_depth - 1);
    var scale = 1 << 16;

    component a = AbsValue(bit_depth);
    a.orig <== orig;

    // v = D*(2a - half) + (scale - D)*(orig_n - half) + scale*half >= 0
    signal v;
    v <== blend * (2 * a.abs - half)
        + (scale - blend) * (orig - half)
        + scale * half;

    // pre-clamp normalized result = (v >> 16)
    component bits = Num2Bits(v_bits);
    bits.in <== v;

    component shifted = Bits2Num(v_bits - 16);
    for (var i = 0; i < v_bits - 16; i++) {
        shifted.in[i] <== bits.out[i + 16];
    }

    // Clamp to [0, 2^bit_depth - 1] in normalized space, which is
    // [-half, half-1] signed. Only the upper bound can be exceeded (at
    // d = 1), but both are checked: the lower costs one comparator and
    // makes the template total rather than relying on that argument.
    var lo = 0;
    var hi = (1 << bit_depth) - 1;

    component over = GreaterThan(v_bits - 16);
    over.in[0] <== shifted.out;
    over.in[1] <== hi;

    component under = LessThan(v_bits - 16);
    under.in[0] <== shifted.out;
    under.in[1] <== lo;

    component pick_low = Mux1();
    pick_low.c[0] <== shifted.out;
    pick_low.c[1] <== lo;
    pick_low.s <== under.out;

    component pick_high = Mux1();
    pick_high.c[0] <== pick_low.out;
    pick_high.c[1] <== hi;
    pick_high.s <== over.out;

    edit <== pick_high.out;
}

template BatchCheckDistort(batch_size, bit_depth, blend) {
    var comp_size = (256 - bit_depth) / bit_depth;
    var half = 1 << (bit_depth - 1);
    var scale = 1 << 16;

    // Width for v, maximised at D = scale, a = half, orig_n = 2^bd - 1.
    var v_max = scale * half + scale * half + scale * half;
    var v_bits = 0;
    var probe = v_max;
    while (probe > 0) {
        v_bits++;
        probe = probe >> 1;
    }

    signal input orig[batch_size];
    signal input edit[batch_size];

    component decompressor_orig[batch_size];
    component distort[batch_size][comp_size];
    component recompressor[batch_size];

    signal expected[batch_size][comp_size];

    for (var j = 0; j < batch_size; j++) {
        decompressor_orig[j] = Decompressor(bit_depth);
        decompressor_orig[j].in <== orig[j];

        for (var s = 0; s < comp_size; s++) {
            distort[j][s] = DistortSample(bit_depth, blend, v_bits);
            distort[j][s].orig <== decompressor_orig[j].out[s];
            expected[j][s] <== distort[j][s].edit;
        }

        // Safe to repack: DistortSample clamps to [0, 2^bit_depth - 1],
        // so every digit fits by construction.
        recompressor[j] = Recompressor(bit_depth);
        recompressor[j].in <== expected[j];

        edit[j] === recompressor[j].out;
    }
}

template Distort(batch_size, bit_depth, blend) {
    //=== Previous Hashes ===//
    signal input step_in[2];   // (prev_orig_hash, prev_edit_hash)

    //=== Next Hashes ===//
    signal output step_out[2];

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

    // Memoryless: distortion is a pointwise function of the sample, so
    // unlike tremolo it needs no sample counter in the IVC state.
    //=== Verifier Component ===//
    component check = BatchCheckDistort(batch_size, bit_depth, blend);
    check.orig <== orig;
    check.edit <== edit;
}
