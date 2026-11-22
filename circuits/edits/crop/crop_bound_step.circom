pragma circom 2.1.6;


include "hasher.circom";
include "hasher_sponge.circom";
include "circom/bitify.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";

// CROP_BOUND: a contiguous window, bound to the commitment of the whole
// stream it was drawn from.
//
// Proves: the edited stream is a verbatim contiguous run of the stream
// committed in step_out[0], which is a commitment to the WHOLE original,
// not to the excerpt. The window is [win_start, win_start + win_len).
//
// Unlike `crop_step.circom`, which folds only the kept window and so
// commits to the excerpt alone, this folds every batch of the original
// into one chain and gates which batches reach the edited chain. The
// Merkle-Damgard state is a resumable chaining value, so the discarded
// samples can be absorbed without being revealed by the relation: they
// enter the hash and nothing else. That is what ties the excerpt to a
// signature over the full file. See docs/crop_and_concat.md.

// Verbatim check, gated on the window.
//
// Inside the window the kept samples pass through untouched; outside it
// `edit` is unconstrained, because there is no corresponding output
// sample. Comparing packed elements directly is sound here for the same
// reason as in crop: the packing is injective over in-range digits.
template CropBoundCheck(batch_size) {
    signal input orig[batch_size];
    signal input edit[batch_size];
    signal input in_window;      // 1 => this batch is kept

    for (var j = 0; j < batch_size; j++) {
        in_window * (edit[j] - orig[j]) === 0;
    }
}

template CropBound(batch_size, bit_depth) {
    // Integer division (\), not field division (/): at bit_depth=24
    // 232/24 is not exact, and `/` would compute a modular inverse
    // instead of 9, which circom then rejects as an array size.
    var comp_size = (256 - bit_depth) \ bit_depth;
    var spb = batch_size * comp_size;

    // Width for the counter comparisons. 40 bits covers far more samples
    // than any audio this proves.
    var idx_bits = 40;

    // [hash_orig, hash_edit, idx, win_start, win_len]
    //
    // hash_orig covers the whole original; hash_edit covers the window.
    // win_start and win_len are carried through unchanged so they stay
    // public and constant across the fold chain - the verifier sets them
    // in z0 and reads the same values back, which is what makes the
    // window non-negotiable.
    signal input step_in[5];
    signal output step_out[5];

    // The original's batch is supplied every step, in or out of the
    // window. `edit` is supplied every step too, because a compiled
    // circuit has a fixed witness shape; outside the window its value is
    // unconstrained and never absorbed.
    signal input orig[batch_size];
    signal input edit[batch_size];

    signal hash_orig;
    signal hash_edit;
    signal idx;
    signal win_start;
    signal win_len;
    hash_orig <== step_in[0];
    hash_edit <== step_in[1];
    idx <== step_in[2];
    win_start <== step_in[3];
    win_len <== step_in[4];

    // Both bounds must fall on fold boundaries, otherwise a step would
    // straddle one and the gate would have to be per-sample. The prover
    // supplies the fold counts; one constraint each pins them.
    signal qs;
    qs <-- win_start \ spb;
    win_start === qs * spb;

    signal ql;
    ql <-- win_len \ spb;
    win_len === ql * spb;

    // Window selector: two comparisons per step, on the counter, not on
    // individual samples.
    component at_or_after = GreaterEqThan(idx_bits);
    at_or_after.in[0] <== idx;
    at_or_after.in[1] <== win_start;

    component before_end = LessThan(idx_bits);
    before_end.in[0] <== idx;
    before_end.in[1] <== win_start + win_len;

    signal in_window;
    in_window <== at_or_after.out * before_end.out;

    // The original chain absorbs EVERY batch. This is the whole point:
    // its terminal value is a commitment to the full stream, so it can
    // equal a signature over the full stream.
    component orig_batch = SpongeBatchHasher(batch_size, 4);
    orig_batch.batch <== orig;

    component orig_next = Hasher(2);
    orig_next.values[0] <== hash_orig;
    orig_next.values[1] <== orig_batch.hash;
    step_out[0] <== orig_next.hash;

    // The edited chain absorbs only inside the window, and holds outside
    // it. A Poseidon permutation cannot be skipped, so it is evaluated
    // every step and a selector decides whether its result is taken -
    // the same gating concat uses. The batch hashed here is the
    // ORIGINAL's, which equals the edit's inside the window by the check
    // below, so no third hasher is needed.
    component edit_next = Hasher(2);
    edit_next.values[0] <== hash_edit;
    edit_next.values[1] <== orig_batch.hash;

    component pick_edit = Mux1();
    pick_edit.c[0] <== hash_edit;        // outside: hold
    pick_edit.c[1] <== edit_next.hash;   // inside: absorb
    pick_edit.s <== in_window;
    step_out[1] <== pick_edit.out;

    step_out[2] <== idx + spb;
    step_out[3] <== win_start;           // carried unchanged
    step_out[4] <== win_len;             // carried unchanged

    component check = CropBoundCheck(batch_size);
    check.orig <== orig;
    check.edit <== edit;
    check.in_window <== in_window;
}
