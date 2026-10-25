pragma circom 2.1.6;

include "hasher.circom";

// CROP: a contiguous sub-selection of the samples, with no value change.
//
// Proves: "the edited stream is a verbatim, contiguous run of samples
// drawn from a stream whose window commitment is step_out[0], beginning
// at absolute index step_in[2] and ending at step_out[2]."
//
// === Why crop is structurally unlike the other edits ===
//
// Gain, attenuation, tremolo and distortion are all one-in-one-out:
// N_edit == N_orig, and the step circuit folds both streams in lockstep
// under the relation edit[i] = f(orig[i]). Crop breaks that invariant -
// the output is SHORTER than the input - so there is no per-sample
// correspondence across the whole file.
//
// This implementation (design "Option A") resolves that by folding ONLY
// the kept window. The prover never presents the discarded samples, so
// every step does have a one-to-one correspondence, and the relation
// collapses to the identity. What makes it a crop rather than a no-op is
// the IVC counter: the window's start and end are public, so the
// statement says WHICH contiguous run was kept, not merely that some run
// was.
//
// === The bounds are free ===
//
// No extra public signals and no per-step range checks are needed,
// because the counter already carries them:
//
//     step_in[2]  at the first fold  == start   (verifier supplies z0)
//     step_out[2] at the last fold   == end     (verifier reads it)
//
// and each step advances the counter by exactly one batch. A prover
// cannot claim a different window without changing values the verifier
// already sees.
//
// === No decompression needed ===
//
// Unlike every other edit, this circuit does NOT call Decompressor.
// Comparing the packed field elements directly is sound: the packing is
// injective over in-range digits (it is a positional base-2^bit_depth
// encoding), so orig[j] == edit[j] as field elements implies the
// underlying samples are equal digit for digit. Decomposing to compare
// per-sample would add thousands of constraints and prove nothing extra.
//
// Consequently this is the cheapest edit in the project: its cost is
// essentially the Poseidon hashing that every edit pays, plus
// batch_size field equalities.
//
// === What this does and does not prove ===
//
// It DOES prove the excerpt is unmodified and contiguous, and it pins
// the excerpt's position relative to the counter's origin.
//
// It does NOT bind the excerpt to a specific larger original: the outer
// file's hash never enters the statement, because a sequential
// Merkle-Damgard chain cannot be opened at an interior window. Proving
// "this excerpt is positions 1000..2499 of the file with root R" needs
// the original committed as a Merkle tree instead (design "Option B"),
// which would change the commitment scheme for every edit and
// invalidate existing commitment tables. That is left as future work and
// should be stated as a limitation wherever this edit is reported.
//
// === Composition caveat ===
//
// Because the running hash depends on the entire prefix, crop does NOT
// commute with the pointwise edits: crop-then-gain and gain-then-crop
// yield different commitments. All the other edits commute with each
// other; crop is the first where the order is part of the statement.

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
    var comp_size = (256 - bit_depth) / bit_depth;

    //=== IVC state: two hashes PLUS the sample counter ===//
    // The counter is what distinguishes a crop from a no-op: it makes
    // the window's start and end public.
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

    // Advance by exactly one batch. Combined with the verifier's chosen
    // z0 counter and the final counter it reads back, this pins the
    // window to [z0[2], z0[2] + N).
    step_out[2] <== step_in[2] + batch_size * comp_size;

    //=== Verifier Component ===//
    component check = CropCheck(batch_size);
    check.orig <== orig;
    check.edit <== edit;
}
