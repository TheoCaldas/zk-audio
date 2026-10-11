pragma circom 2.1.6;

include "hasher.circom";
include "circom/comparators.circom";


template CheckMultiply() {
    signal input a;
    signal input b;

    component eq = IsEqual();
    eq.in[0] <== a * 2;
    eq.in[1] <== b;
    eq.out === 1;
}

template Multiply(){
    signal input step_in[2];
    // signal input prev_orig_hash;
    // signal input prev_edit_hash;
    signal output step_out[2];
    // signal output next_orig_hash;
    // signal output next_edit_hash;
    
    signal input orig;
    signal input edit;
    
    // Verify Edit 
    component check = CheckMultiply();
    check.a <== orig;
    check.b <== edit;

    // Next hash
    component orig_hasher = Hasher(2);
    orig_hasher.values[0] <== orig;
    orig_hasher.values[1] <== step_in[0];
    step_out[0] <== orig_hasher.hash;

    component edit_hasher = Hasher(2);
    edit_hasher.values[0] <== edit;
    edit_hasher.values[1] <== step_in[1];
    step_out[1] <== edit_hasher.hash;
}

component main { public [step_in] } = Multiply();



