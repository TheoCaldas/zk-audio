pragma circom 2.1.6;

include "hasher.circom";
include "decompressor.circom";
include "circom/bitify.circom";

// Volume REDUCTION by a power of two: proves `edit = floor(orig / 2^m)`
// for m >= 1, i.e. gain factors k = 1/2, 1/4, 1/8, ... (-6dB per step).
//
// Companion to `gain_step.circom` (k > 1, amplification). Kept separate
// because the two relations differ structurally, not just in a constant:
//
//   * Amplification can leave the representable range, so it needs a
//     clamp: two comparators and two muxes per sample.
//   * Attenuation can NEVER leave the range (|x/2^m| <= |x|), so it needs
//     no clamp at all.
//   * Fields have no division, so attenuation is a shift: decompose and
//     drop the low m bits. That IS floor division, and it is cheap here
//     because the remainder bits come out of the decomposition for free -
//     no separate division-with-remainder witness, and no range check on
//     a remainder.
//
// === Rounding convention ===
//
// FLOOR (toward negative infinity), matching an arithmetic right shift.
// This is NOT Rust's `/` operator, which truncates toward zero and
// disagrees on negatives: -7/4 is -1 truncated but -2 floored. The Rust
// reference uses `>>` to match, and
// `atten_floor_matches_circuit_exhaustively` pins the agreement over
// every input at bd in {8,16}.
//
// Floor is chosen over round-half-up because it falls straight out of the
// bit decomposition at zero extra cost. The DC-offset consequence is
// real but tiny: it biases every sample down by up to one LSB, which at
// 8-bit is -1/256 of full scale.
//
// === The normalized-space formulation ===
//
// Samples arrive normalized: `orig_n = orig_signed + half`, where
// half = 2^(bit_depth-1). Shifting orig_n directly is WRONG - it is not
// the normalization of the shifted signed value, and it disagrees for
// EVERY input (verified: 256/256 mismatches at bd=8). Instead:
//
//     floor(s / 2^m) = floor((s + half*2^m) / 2^m) - half
//
// so with  v = orig_n + half*(2^m - 1)  =  s + half*2^m  >= 0:
//
//     edit_signed = (v >> m) - half
//     edit_n      = edit_signed + half = v >> m
//
// The offset cancels exactly, so the circuit just decomposes v and reads
// off the high bits. v needs `bit_depth + m` bits (its max is
// half*(2^m - 1) + 2^bit_depth - 1).

// Attenuates one already-normalized sample by 2^m, returning the
// normalized edit digit. Output is provably in [0, 2^bit_depth - 1].
template AttenuatedGain(bit_depth, m) {
    signal input orig;      // normalized: orig_signed + 2^(bd-1)
    signal output edit;     // normalized: floor(orig_signed / 2^m) + 2^(bd-1)

    var half = 1 << (bit_depth - 1);

    // v = orig_n + half*(2^m - 1), always non-negative.
    signal v;
    v <== orig + half * ((1 << m) - 1);

    // Width must cover max(v) = half*(2^m - 1) + (2^bit_depth - 1).
    var v_max = half * ((1 << m) - 1) + (1 << bit_depth) - 1;
    var width = 0;
    var probe = v_max;
    while (probe > 0) {
        width++;
        probe = probe >> 1;
    }

    component bits = Num2Bits(width);
    bits.in <== v;

    // edit_n = v >> m: recombine the bits above position m. The low m
    // bits are the discarded remainder - no constraint needed on them,
    // because Num2Bits already proved they are bits and the recombination
    // below is what defines the output.
    component out = Bits2Num(width - m);
    for (var i = 0; i < width - m; i++) {
        out.in[i] <== bits.out[i + m];
    }

    edit <== out.out;
}

template ExpectedEditAtten(size, bit_depth, m) {
    signal input orig[size];
    signal output edit[size];

    component a[size];
    for (var i = 0; i < size; i++) {
        a[i] = AttenuatedGain(bit_depth, m);
        a[i].orig <== orig[i];
        edit[i] <== a[i].edit;
    }
}

template BatchCheckAtten(batch_size, bit_depth, m) {
    var comp_size = (256 - bit_depth) / bit_depth;

    signal input orig[batch_size];
    signal input edit[batch_size];

    component decompressor_orig[batch_size];
    component expected[batch_size];
    component recompressor[batch_size];

    for (var j = 0; j < batch_size; j++) {
        // Same optimization as the gain family: decompress only the
        // original, then check `edit` against a value we compute and
        // repack ourselves.
        decompressor_orig[j] = Decompressor(bit_depth);
        decompressor_orig[j].in <== orig[j];

        expected[j] = ExpectedEditAtten(comp_size, bit_depth, m);
        expected[j].orig <== decompressor_orig[j].out;

        // Safe to repack: AttenuatedGain's output is provably within
        // [0, 2^bit_depth - 1] for every input, since attenuation shrinks
        // magnitude. No headroom assumption, and no clamp required.
        recompressor[j] = Recompressor(bit_depth);
        recompressor[j].in <== expected[j].edit;

        edit[j] === recompressor[j].out;
    }
}

template Attenuate(batch_size, bit_depth, m) {
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
    component check = BatchCheckAtten(batch_size, bit_depth, m);
    check.orig <== orig;
    check.edit <== edit;
}
