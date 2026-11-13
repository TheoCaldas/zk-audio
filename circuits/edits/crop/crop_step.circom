pragma circom 2.1.6;


include "hasher.circom";
include "hasher_sponge.circom";

// CROP: a contiguous sub-selection of the samples, with no value change.
//
// Proves the edited stream is a verbatim, contiguous run drawn from the
// stream committed in step_out[0], from index step_in[2] to step_out[2].
// Only the kept window is folded, so the relation is the identity; the
// counter is what makes it a crop rather than a no-op.
//
// Does NOT bind the excerpt to a larger original. See
// docs/crop_and_concat.md, which states the limitation to report.

template CropCheck(batch_size) {
    signal input orig[batch_size];
    signal input edit[batch_size];

    // Verbatim: the kept samples pass through untouched. One constraint
    // per packed element, covering comp_size samples each.
    for (var j = 0; j < batch_size; j++) {
        edit[j] === orig[j];
    }
}

template Crop(batch_size, bit_depth) {
        // Integer division (\), not field division (/): at bit_depth=24
    // 232/24 is not exact, and `/` would compute a modular inverse
    // instead of 9, which circom then rejects as an array size.
    var comp_size = (256 - bit_depth) \ bit_depth;

    // Third state element: the sample counter.
    // The counter is what distinguishes a crop from a no-op: it makes
    // the window's start and end public.
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

    // Advance by exactly one batch. Combined with the verifier's chosen
    // z0 counter and the final counter it reads back, this pins the
    // window to [z0[2], z0[2] + N).
    step_out[2] <== step_in[2] + batch_size * comp_size;

    component check = CropCheck(batch_size);
    check.orig <== orig;
    check.edit <== edit;
}
