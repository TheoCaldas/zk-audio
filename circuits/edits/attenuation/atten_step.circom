pragma circom 2.1.6;


include "hasher.circom";
include "hasher_sponge.circom";
include "decompressor.circom";
include "circom/bitify.circom";

// Volume reduction by a power of two: proves `edit = floor(orig / 2^m)`
// for m >= 1, i.e. k = 1/2, 1/4, 1/8 (-6dB per step).
//
// Needs no clamp, unlike gain_step.circom: |x/2^m| <= |x| never leaves the
// representable range. Fields have no division, so this is a shift -
// decompose and drop the low m bits, which is floor division, and the
// remainder bits fall out of the decomposition for free.
//
// Rounding is FLOOR, matching an arithmetic right shift, not Rust's `/`
// which truncates toward zero (-7/4 is -1 truncated, -2 floored). The Rust
// reference uses `>>`; atten_floor_matches_circuit_exhaustively pins the
// agreement at bd 8 and 16. Floor biases every sample down by up to one
// LSB.
//
// Samples arrive normalized (`orig_n = orig_signed + half`). Shifting
// orig_n directly is wrong - it disagrees for every input. Instead, from
//
//     floor(s / 2^m) = floor((s + half*2^m) / 2^m) - half
//
// take v = orig_n + half*(2^m - 1) = s + half*2^m >= 0, giving
// edit_n = v >> m. The offset cancels exactly, so the circuit decomposes v
// and reads off the high bits. v needs bit_depth + m bits.

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
        // Integer division (\), not field division (/): at bit_depth=24
    // 232/24 is not exact, and `/` would compute a modular inverse
    // instead of 9, which circom then rejects as an array size.
    var comp_size = (256 - bit_depth) \ bit_depth;

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
    signal input step_in[2]; // (prev_orig_hash, prev_edit_hash)

    signal output step_out[2]; // (next_orig_hash, next_edit_hash)

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

    component check = BatchCheckAtten(batch_size, bit_depth, m);
    check.orig <== orig;
    check.edit <== edit;
}
