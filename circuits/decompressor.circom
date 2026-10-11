pragma circom 2.1.6;
include "circom/bitify.circom";


template Decompressor(bit_depth) {
    var nBits = 256 - bit_depth;
    var size = nBits / bit_depth;

    signal input in;
    signal output out[size];

    component toBits = Num2Bits(nBits);
    toBits.in <== in;

    component toNum[size];

    for (var i = 0; i < size; i++) {
        toNum[i] = Bits2Num(bit_depth);

        for (var b = 0; b < bit_depth; b++) {
            toNum[i].in[b] <== toBits.out[i * bit_depth + b];
        }

        out[i] <== toNum[i].out;
    }
}

// Inverse of Decompressor: packs `size` digits back into a single field
// element using the same fixed positional weights (2^(bit_depth*i)) that
// the off-circuit packer uses to build compressed samples.
//
// This is a single linear combination -> exactly one constraint. It adds
// NO new range constraints on `in`, because it's only ever used here on
// digits that were already produced by an in-circuit arithmetic
// computation over an already range-checked `Decompressor` output (see
// ExpectedEditMultiply in multiplier_step_b16.circom) - never on free
// witness values. Soundness relies on the same assumption the original
// circuit already made: that a correctly-computed transform output fits
// in `bit_depth` bits per sample, the same assumption baked into the
// old edit-side Decompressor(16). If a transform can legitimately
// overflow bit_depth bits, that has to be handled (e.g. via saturation)
// before this point, independent of this optimization.
template Recompressor(bit_depth) {
    var nBits = 256 - bit_depth;
    var size = nBits / bit_depth;

    signal input in[size];
    signal output out;

    var acc = 0;
    var weight = 1;

    for (var i = 0; i < size; i++) {
        acc += in[i] * weight;
        for (var b = 0; b < bit_depth; b++) {
            weight = weight + weight;
        }
    }

    out <== acc;
}

// component main { public [in] } = Decompressor(16);
