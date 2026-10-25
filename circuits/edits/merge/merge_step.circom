pragma circom 2.1.6;

include "hasher.circom";
include "decompressor.circom";
include "circom/bitify.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";

// MERGE (mix two streams): out_i = clamp(a_i + b_i)
//
// === Why this one is structurally different ===
//
// Every other edit in this project proves a relation of the form
// edit = f(orig): ONE input stream, ONE output stream, and an IVC state
// carrying two running hashes. Merge proves out = f(a, b), which changes
// the shape of the statement rather than just the arithmetic:
//
//   * THREE streams are folded per step, not two.
//   * The IVC state carries THREE hashes - one per stream - where the
//     other three-wide edits (tremolo, crop) use the third element for a
//     sample counter instead. The widths coincide; the meanings do not.
//   * The provenance claim becomes "this output derives from exactly
//     these two committed sources", which is the most expressive
//     statement the project proves. A verifier holding signatures over
//     stem A and stem B can check that a mix contains precisely those
//     two stems and nothing else.
//
// === Clamping is mandatory on BOTH sides ===
//
// Distortion could only exceed the upper bound. A sum of two full-scale
// signals breaks both: at bit_depth 8 the pre-clamp signed sum spans
// -256..254, against a representable -128..127. So saturation here is
// not an edge case but the common path for loud material, and both
// comparators are load-bearing. (Verified by enumerating the sum over
// the full input domain at bit_depth in {8,16}.)
//
// Real mixers usually attenuate before summing rather than clamp,
// because a clamped sum sounds like hard distortion. This circuit
// implements the literal `a + b` that was asked for; an averaging
// variant (which cannot overflow and therefore needs no clamp at all)
// would be a cheaper separate instantiation.
//
// === Normalized space ===
//
// Streams arrive normalized: a_n = a + half, b_n = b + half. Then
//
//     s = a_n + b_n = (a + b) + 2*half   >=  0
//
// so the signed sum is s - 2*half and the normalized result before
// clamping is t = s - half. Both are computed without any signed
// intermediate; t may still fall outside [0, 2^bit_depth - 1] in either
// direction, which is what the clamp fixes.
//
// No bit decomposition is needed for the arithmetic itself - the sum is
// linear - but the comparators need one internally, and t's width must
// cover s.

// Clamps one normalized sum into [0, 2^bit_depth - 1].
//
// `t_bits` must cover s = a_n + b_n, i.e. bit_depth + 1 bits. The
// comparison is done on s against shifted bounds rather than on t
// directly, because t can be negative and the field has no sign.
template ClampSum(bit_depth, t_bits) {
    signal input s;             // a_n + b_n, in [0, 2^(bd+1) - 2]
    signal output edit;         // clamped normalized result

    var half = 1 << (bit_depth - 1);
    var maxn = (1 << bit_depth) - 1;

    // t = s - half is the normalized result pre-clamp. Comparing in
    // s-space keeps every operand non-negative:
    //   t < 0      <=>  s < half
    //   t > maxn   <=>  s > maxn + half
    component under = LessThan(t_bits);
    under.in[0] <== s;
    under.in[1] <== half;

    component over = GreaterThan(t_bits);
    over.in[0] <== s;
    over.in[1] <== maxn + half;

    component pick_low = Mux1();
    pick_low.c[0] <== s - half;
    pick_low.c[1] <== 0;
    pick_low.s <== under.out;

    component pick_high = Mux1();
    pick_high.c[0] <== pick_low.out;
    pick_high.c[1] <== maxn;
    pick_high.s <== over.out;

    edit <== pick_high.out;
}

// Averages one normalized sum: out_n = (a_n + b_n) >> 1.
//
// This is the `(a+b)/2` mix, and it needs NO CLAMP: halving the sum of
// two in-range samples is always in range (verified over the full input
// domain at bit_depth in {8,16}), so both comparators and both
// multiplexers that ClampSum requires disappear.
//
// The normalization offset cancels exactly, which is what makes it this
// cheap. With s = a_n + b_n = (a+b) + 2*half:
//
//     floor((a+b)/2) = floor(s/2) - half
//     out_n          = floor(s/2) - half + half = floor(s/2)
//
// so the circuit just drops the low bit of s. Rounding is floor, as
// everywhere else here; the Rust reference must use `>>`, not `/`.
template AverageSum(bit_depth, t_bits) {
    signal input s;             // a_n + b_n
    signal output edit;         // (a_n + b_n) >> 1

    component bits = Num2Bits(t_bits);
    bits.in <== s;

    component out = Bits2Num(t_bits - 1);
    for (var i = 0; i < t_bits - 1; i++) {
        out.in[i] <== bits.out[i + 1];
    }

    edit <== out.out;
}

// `average`: 0 proves out = clamp(a + b), 1 proves out = floor((a+b)/2).
// Both share the hashing and decompression; only the per-sample
// combiner differs, so the two variants are directly comparable.
template BatchCheckMerge(batch_size, bit_depth, average) {
    var comp_size = (256 - bit_depth) / bit_depth;

    // s = a_n + b_n needs bit_depth + 1 bits (max 2*(2^bd - 1)).
    // ClampSum's comparators want one extra for safety; AverageSum needs
    // exactly bit_depth + 1 so that dropping the low bit leaves
    // bit_depth bits.
    var t_bits = average == 1 ? bit_depth + 1 : bit_depth + 2;

    signal input a[batch_size];
    signal input b[batch_size];
    signal input out[batch_size];

    component dec_a[batch_size];
    component dec_b[batch_size];
    component clamp[batch_size][comp_size];
    component avg[batch_size][comp_size];
    component recompressor[batch_size];

    signal expected[batch_size][comp_size];

    for (var j = 0; j < batch_size; j++) {
        // BOTH inputs must be decompressed: unlike the single-stream
        // edits there is no way to derive one from the other, and the
        // output is checked against a value computed from both.
        dec_a[j] = Decompressor(bit_depth);
        dec_a[j].in <== a[j];

        dec_b[j] = Decompressor(bit_depth);
        dec_b[j].in <== b[j];

        for (var s = 0; s < comp_size; s++) {
            if (average == 1) {
                avg[j][s] = AverageSum(bit_depth, t_bits);
                avg[j][s].s <== dec_a[j].out[s] + dec_b[j].out[s];
                expected[j][s] <== avg[j][s].edit;
            } else {
                clamp[j][s] = ClampSum(bit_depth, t_bits);
                clamp[j][s].s <== dec_a[j].out[s] + dec_b[j].out[s];
                expected[j][s] <== clamp[j][s].edit;
            }
        }

        // Safe to repack: both combiners output within
        // [0, 2^bit_depth - 1] by construction - ClampSum because it
        // saturates, AverageSum because halving cannot overflow.
        recompressor[j] = Recompressor(bit_depth);
        recompressor[j].in <== expected[j];

        out[j] === recompressor[j].out;
    }
}

template Merge(batch_size, bit_depth, average) {
    //=== IVC state: THREE hashes, one per stream ===//
    // step_in[2] is stream B's running hash, NOT a sample counter as in
    // tremolo_step.circom and crop_step.circom. The state widths match
    // but the semantics differ, so a results row must record which.
    signal input step_in[3];   // (hash_a, hash_out, hash_b)
    signal output step_out[3];

    //=== Current Batches ===//
    signal input a[batch_size];
    signal input b[batch_size];
    signal input out[batch_size];

    //=== Hash Components ===//
    component a_batch_hasher = BatchHasher(batch_size);
    component b_batch_hasher = BatchHasher(batch_size);
    component out_batch_hasher = BatchHasher(batch_size);
    component a_hasher = Hasher(2);
    component b_hasher = Hasher(2);
    component out_hasher = Hasher(2);

    // Stream A keeps slot 0 and the output keeps slot 1, matching the
    // (original, edited) convention of every other edit, so existing
    // tooling reads the same two fields. B is appended in slot 2.
    a_batch_hasher.batch <== a;
    a_hasher.values[0] <== step_in[0];
    a_hasher.values[1] <== a_batch_hasher.hash;
    step_out[0] <== a_hasher.hash;

    out_batch_hasher.batch <== out;
    out_hasher.values[0] <== step_in[1];
    out_hasher.values[1] <== out_batch_hasher.hash;
    step_out[1] <== out_hasher.hash;

    b_batch_hasher.batch <== b;
    b_hasher.values[0] <== step_in[2];
    b_hasher.values[1] <== b_batch_hasher.hash;
    step_out[2] <== b_hasher.hash;

    //=== Verifier Component ===//
    component check = BatchCheckMerge(batch_size, bit_depth, average);
    check.a <== a;
    check.b <== b;
    check.out <== out;
}
