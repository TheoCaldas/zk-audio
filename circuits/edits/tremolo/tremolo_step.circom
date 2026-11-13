pragma circom 2.1.6;


include "hasher.circom";
include "hasher_sponge.circom";
include "decompressor.circom";
include "circom/bitify.circom";
include "circom/comparators.circom";
include "circom/multiplexer.circom";
include "tremolo_table.circom";

// TREMOLO: amplitude modulation by a low-frequency oscillator.
//
//     edit_i = floor( orig_i * L(i) / 2^16 )
//
// where L(i) in [0, 2^16] is a unipolar sine LFO at scale 2^16, so the
// rescale is a right shift. Floor rounding; the Rust reference must use
// `>>`, not `/`.
//
// The first edit needing a notion of time: the IVC state carries a proven
// sample counter, advanced one batch per step, so the LFO phase cannot be
// shifted without breaking the chain.
//
// L <= 2^16 means |orig*L/2^16| <= |orig|, so no clamp is needed. L is
// looked up once per packed element, not per sample, which keeps the cost
// in line with gain at an inaudible approximation.
//
// See docs/tremolo.md for the normalized-space derivation and the
// quantified LFO-quantisation figures to report.

// Selects LFO value L for a block whose first sample is at absolute
// index `idx`, from a TABLE_N-entry table covering one LFO period of
// `period` samples.
//
// The table index is floor(idx * TABLE_N / period) mod TABLE_N, proven
// via division-with-remainder: the prover supplies the quotient and
// remainder and the circuit checks the identity and the remainder's
// range. `idx_bits` must cover idx * TABLE_N.
template LfoSelect(period, table_n, idx_bits) {
    signal input idx;             // absolute sample index of this block
    signal output lfo;            // L in [0, 2^16]

    // --- Division with remainder: idx * table_n = q*period + r, r < period
    signal scaled;
    scaled <== idx * table_n;

    signal q;
    signal r;
    q <-- scaled \ period;
    r <-- scaled % period;

    scaled === q * period + r;

    // r < period, so the decomposition is unique.
    component r_ok = LessThan(idx_bits);
    r_ok.in[0] <== r;
    r_ok.in[1] <== period;
    r_ok.out === 1;

    // --- q mod table_n, with table_n a power of two: the low bits of q.
    var tn_bits = 0;
    var probe = table_n - 1;
    while (probe > 0) {
        tn_bits++;
        probe = probe >> 1;
    }

    component q_bits = Num2Bits(idx_bits);
    q_bits.in <== q;

    component sel = Bits2Num(tn_bits);
    for (var b = 0; b < tn_bits; b++) {
        sel.in[b] <== q_bits.out[b];
    }

    // --- Table lookup by the proven index.
    component mux = Multiplexer(1, table_n);
    var tbl[table_n] = LfoTable();
    for (var i = 0; i < table_n; i++) {
        mux.inp[i][0] <== tbl[i];
    }
    mux.sel <== sel.out;

    lfo <== mux.out[0];
}

// Applies one LFO value to one normalized sample.
template ModulateSample(bit_depth, u_bits) {
    signal input orig;       // normalized: orig_signed + half
    signal input lfo;        // L in [0, 2^16]
    signal output edit;      // normalized result

    var half = 1 << (bit_depth - 1);
    var scale = 1 << 16;

    // u = orig_n*L + (scale - L)*half = orig*L + half*scale >= 0
    signal u;
    u <== orig * lfo + (scale - lfo) * half;

    // edit_n = u >> 16
    component bits = Num2Bits(u_bits);
    bits.in <== u;

    component out = Bits2Num(u_bits - 16);
    for (var b = 0; b < u_bits - 16; b++) {
        out.in[b] <== bits.out[b + 16];
    }

    edit <== out.out;
}

template BatchCheckTremolo(batch_size, bit_depth, period, table_n) {
        // Integer division (\), not field division (/): at bit_depth=24
    // 232/24 is not exact, and `/` would compute a modular inverse
    // instead of 9, which circom then rejects as an array size.
    var comp_size = (256 - bit_depth) \ bit_depth;
    var half = 1 << (bit_depth - 1);
    var scale = 1 << 16;

    // Width for u = orig_n*L + (scale-L)*half, max at orig_n = 2^bd - 1.
    var u_max = (2 * half - 1) * scale + scale * half;
    var u_bits = 0;
    var probe = u_max;
    while (probe > 0) {
        u_bits++;
        probe = probe >> 1;
    }

    // Width for the index division: idx * table_n, where idx can reach
    // the whole stream length. 40 bits covers 2^40/256 samples, far more
    // than any audio this proves.
    var idx_bits = 40;

    signal input orig[batch_size];
    signal input edit[batch_size];
    signal input start_idx;              // absolute index of orig[0]'s first sample

    component decompressor_orig[batch_size];
    component lfo[batch_size];
    component modulate[batch_size][comp_size];
    component recompressor[batch_size];

    // Declared up front: circom allows signal declarations only in the
    // initial scope, not inside a loop body.
    signal expected[batch_size][comp_size];

    for (var j = 0; j < batch_size; j++) {
        decompressor_orig[j] = Decompressor(bit_depth);
        decompressor_orig[j].in <== orig[j];

        // ONE lookup per packed element, shared by its comp_size samples.
        lfo[j] = LfoSelect(period, table_n, idx_bits);
        lfo[j].idx <== start_idx + j * comp_size;

        for (var s = 0; s < comp_size; s++) {
            modulate[j][s] = ModulateSample(bit_depth, u_bits);
            modulate[j][s].orig <== decompressor_orig[j].out[s];
            modulate[j][s].lfo <== lfo[j].lfo;
            expected[j][s] <== modulate[j][s].edit;
        }

        // Safe to repack: the result is provably in [0, 2^bit_depth - 1]
        // because L <= 2^16 means the modulated sample never exceeds the
        // source magnitude. No clamp, no headroom assumption.
        recompressor[j] = Recompressor(bit_depth);
        recompressor[j].in <== expected[j];

        edit[j] === recompressor[j].out;
    }
}

template Tremolo(batch_size, bit_depth, period, table_n) {
        // Integer division (\), not field division (/): at bit_depth=24
    // 232/24 is not exact, and `/` would compute a modular inverse
    // instead of 9, which circom then rejects as an array size.
    var comp_size = (256 - bit_depth) \ bit_depth;

    // Third state element: the sample counter.
    signal input step_in[3];   // (prev_orig_hash, prev_edit_hash, sample_index)
    signal output step_out[3];

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

    // The counter advances by exactly one batch of samples. This is what
    // binds the LFO phase: the prover cannot choose where in the stream
    // this batch sits, and the verifier checks the final counter == N.
    step_out[2] <== step_in[2] + batch_size * comp_size;

    component check = BatchCheckTremolo(batch_size, bit_depth, period, table_n);
    check.orig <== orig;
    check.edit <== edit;
    check.start_idx <== step_in[2];
}
