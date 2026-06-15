pragma circom 2.1.6;

include "hasher.circom";
include "circom/comparators.circom";


template BatchCheckMultiply(size) {
    signal input orig[size];
    signal input edit[size];

    component eq[size];

    for (var i = 0; i < size; i++) {   
        eq[i] = IsEqual();
        eq[i].in[0] <== orig[i] * 2 - 32768;
        eq[i].in[1] <== edit[i];
        eq[i].out === 1;
    }
}

template Multiply(size){
    //=== Previous Hashes ===//
    signal input step_in[2];
    // signal input prev_orig_hash;
    // signal input prev_edit_hash;

    //=== Next Hashes ===//
    signal output step_out[2];
    // signal output next_orig_hash;
    // signal output next_edit_hash;
    
    //=== Current Batches ===//
    signal input orig[size];
    signal input edit[size];

    //=== Hash Components ===//
    component orig_batch_hasher = BatchHasher(size);
    component edit_batch_hasher = BatchHasher(size);
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
    component check = BatchCheckMultiply(size);
    check.orig <== orig;
    check.edit <== edit;
}

// component main { public [step_in] } = Multiply(100);



