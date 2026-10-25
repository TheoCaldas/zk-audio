pragma circom 2.1.6;

include "hasher.circom";
include "decompressor.circom";
include "circom/bitify.circom";
include "circom/comparators.circom";
include "circom/multiplexer.circom";
include "tremolo_table.circom";

// TREMOLO: amplitude modulation by a low-frequency oscillator.
//
//     edit_i = floor( orig_i * L(i) / 2^16 )
//
// where L(i) in [0, 2^16] is a unipolar sine LFO sampled at position i.
//
// === Naming ===
//
// The reference implementation (source_outputs/"pitch shifting 2.c") is
// titled "pitch shifting" but its own comments describe, and its code
// computes, a TREMOLO: `output = input * lfo`, which modulates amplitude,
// not pitch. True pitch shifting requires resampling or a phase vocoder,
// neither of which is expressible at reasonable cost here. This circuit
// is named for what it actually proves.
//
// === Why this is the first edit needing a notion of time ===
//
// Gain and attenuation are memoryless: edit_i depends only on orig_i. A
// tremolo depends on the sample's absolute POSITION in the stream, so the
// circuit must know where each batch sits. The IVC state is therefore
// extended with a sample counter:
//
//     step_in  = [orig_hash, edit_hash, sample_index]
//     step_out = [orig_hash', edit_hash', sample_index + batch_size*comp_size]
//
// The counter is proven, not supplied: each step advances it by exactly
// one batch's worth of samples, and the verifier sees the final value,
// which must equal N. A prover cannot shift the LFO phase to flatter the
// signal without breaking the chain.
//
// === Fixed point ===
//
// L is an integer in [0, 2^16] representing a gain in [0, 1] at a scale
// of 2^16. The post-multiply rescale is therefore a right shift by 16,
// not a general division - the same trick the attenuation circuit uses,
// and the reason the scale is a power of two.
//
// Floor rounding, as in `atten_step.circom`, because bit truncation
// computes it natively. The off-circuit reference must use `>>`, not `/`.
//
// === No clamping needed ===
//
// Because L <= 2^16, the product satisfies |orig*L/2^16| <= |orig|, so
// the result is always representable at the source depth. Like
// attenuation and unlike gain, this circuit needs no saturation logic -
// verified exhaustively against the Rust reference.
//
// === The normalized-space formulation ===
//
// Samples arrive normalized as orig_n = orig + half. The product orig*L
// is signed, which the field cannot represent directly, so the circuit
// works with
//
//     u = orig_n * L + (2^16 - L) * half   =   orig*L + half*2^16   >=  0
//
// from which floor(u / 2^16) = floor(orig*L / 2^16) + half = edit_n
// exactly, with no correction term. (Verified for every input at
// bit_depth in {8,16} across the full range of L.)
//
// === Piecewise-constant LFO ===
//
// L is looked up ONCE PER PACKED ELEMENT (comp_size samples), not per
// sample. A 256-entry lookup costs ~256 constraints, so a per-sample
// lookup would make this circuit ~3x the cost of gain; amortizing it over
// comp_size samples brings it back in line.
//
// The approximation is inaudible: at 44.1kHz with comp_size=15 the LFO
// updates at 2940Hz against a 5Hz modulation (588x oversampled), and the
// worst amplitude step between adjacent blocks is 0.046dB - well under
// the ~1dB difference limen. At comp_size=31 (bit_depth=8) it is 0.095dB.
// Both are reported in the thesis rather than claimed to be exact.

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
    var comp_size = (256 - bit_depth) / bit_depth;
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
    var comp_size = (256 - bit_depth) / bit_depth;

    //=== IVC state: two hashes PLUS the sample counter ===//
    signal input step_in[3];   // (prev_orig_hash, prev_edit_hash, sample_index)
    signal output step_out[3];

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

    // The counter advances by exactly one batch of samples. This is what
    // binds the LFO phase: the prover cannot choose where in the stream
    // this batch sits, and the verifier checks the final counter == N.
    step_out[2] <== step_in[2] + batch_size * comp_size;

    //=== Verifier Component ===//
    component check = BatchCheckTremolo(batch_size, bit_depth, period, table_n);
    check.orig <== orig;
    check.edit <== edit;
    check.start_idx <== step_in[2];
}
