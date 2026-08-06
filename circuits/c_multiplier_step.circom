pragma circom 2.1.6;

include "hasher.circom";
include "decompressor.circom";
include "circom/comparators.circom";


template DecompCheckMultiply(size) {
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

template BatchCheckMultiply(batch_size){
    var bit_depth = 16;
    var comp_size = 15;

    signal input orig[batch_size];
    signal input edit[batch_size];
    
    component decompressor_orig[batch_size];
    component decompressor_edit[batch_size];

    component check[batch_size];

    for (var j=0; j<batch_size; j++) {
        // Decompress the original values
        decompressor_orig[j] = Decompressor(bit_depth);
        decompressor_orig[j].in <== orig[j];

        // Decompress the edited values
        decompressor_edit[j] = Decompressor(bit_depth);
        decompressor_edit[j].in <== edit[j];
        
        // Check the decompressed values
        check[j] = DecompCheckMultiply(comp_size);
        check[j].orig <== decompressor_orig[j].out;
        check[j].edit <== decompressor_edit[j].out;
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



