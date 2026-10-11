pragma circom 2.1.6;

include "hasher.circom";
include "decompressor.circom";


// Derives the digits `edit` must equal, directly from orig's
// (already range-checked) digits, via the fixed public transform
// edit_i = 2*orig_i - 32768. Pure arithmetic - no decompression,
// no bit-level work at all.
template ExpectedEditMultiply(size) {
    signal input orig[size];
    signal output edit[size];

    for (var i = 0; i < size; i++) {
        edit[i] <== orig[i] * 2 - 32768;
    }
}

template BatchCheckMultiply(batch_size){
    var bit_depth = 16;
    var comp_size = 15;

    signal input orig[batch_size];
    signal input edit[batch_size];

    component decompressor_orig[batch_size];
    component expected[batch_size];
    component recompressor[batch_size];

    for (var j=0; j<batch_size; j++) {
        // Decompress the original values only. `edit` is never
        // decompressed: we check it against a value we compute and
        // repack ourselves, instead of comparing digit-by-digit
        // against a second decompression of `edit`.
        decompressor_orig[j] = Decompressor(bit_depth);
        decompressor_orig[j].in <== orig[j];

        // Compute the digits the packed `edit[j]` element must equal.
        expected[j] = ExpectedEditMultiply(comp_size);
        expected[j].orig <== decompressor_orig[j].out;

        // Repack those digits into one field element using the same
        // positional weights the off-circuit packer used.
        recompressor[j] = Recompressor(bit_depth);
        recompressor[j].in <== expected[j].edit;

        // One equality check per compressed element, instead of
        // comp_size (15) IsEqual checks plus a whole second
        // Decompressor(16) call on `edit`.
        edit[j] === recompressor[j].out;
    }

}

template Multiply(batch_size){
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
    component check = BatchCheckMultiply(batch_size);
    check.orig <== orig;
    check.edit <== edit;
}

// component main { public [step_in] } = Multiply(150);
