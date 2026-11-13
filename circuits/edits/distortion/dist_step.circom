pragma circom 2.1.6;


include "hasher.circom";
include "hasher_sponge.circom";
include "decompressor.circom";
include "circom/bitify.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";

// DISTORTION (full-wave rectifier fold, blended with the dry signal),
// after source_outputs/"dist 1.c":
//
//     output = d * 2*(|x| - 1/2) + (1 - d) * x          (x in [-1, 1])
//
// In integer form at bit depth b, with x in [-half, half-1] and the blend
// amount d as D = round(d * 2^16):
//
//     edit = clamp( ( D*(2|x| - half) + (2^16 - D)*x ) >> 16 )
//
// The most expensive edit here: it needs a sign test for |x| (a comparator
// plus a mux per sample), a clamp - at d = 1 the rectified term reaches
// +half, one above the representable maximum - and a wide decomposition
// per sample.
//
// D is at scale 2^16, so the rescale is a right shift. Floor rounding; the
// Rust reference must use `>>`, not `/`. The integer form is not
// bit-identical to the float reference: maximum deviation is 0.75 LSB at
// d = 0.25, from the single floor at the end rather than float rounding at
// every step. Report that figure.
//
// Samples arrive as orig_n = x + half. With a = |x| the circuit evaluates
//
//     v = D*(2a - half) + (2^16 - D)*(orig_n - half) + 2^16*half
//
// non-negative for every (x, D) in range, from which (v >> 16) - half is
// the signed pre-clamp result exactly. Verified over the full domain at
// b in {8,16} across D's range.

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
        // Integer division (\), not field division (/): at bit_depth=24
    // 232/24 is not exact, and `/` would compute a modular inverse
    // instead of 9, which circom then rejects as an array size.
    var comp_size = (256 - bit_depth) \ bit_depth;
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
    signal input step_in[2];   // (prev_orig_hash, prev_edit_hash)

    signal output step_out[2];

    signal input orig[batch_size];
    signal input edit[batch_size];

    component orig_batch_hasher = SpongeBatchHasher(batch_size, 4);
    component edit_batch_hasher = SpongeBatchHasher(batch_size, 4);
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
    component check = BatchCheckDistort(batch_size, bit_depth, blend);
    check.orig <== orig;
    check.edit <== edit;
}
