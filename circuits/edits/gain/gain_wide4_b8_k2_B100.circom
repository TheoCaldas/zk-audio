pragma circom 2.1.6;

include "hasher.circom";
include "hasher_wide.circom";
include "decompressor.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";
include "gain_step.circom";

// `Gain` with the arity-2 Poseidon CHAIN in BatchHasher replaced by an
// arity-16 TREE (see hasher_wide.circom). The proven relation is
// unchanged - same clamped gain, same IVC state shape, same per-step
// commitment structure - only the batch digest is computed differently.
//
// Measured at B=100, bit_depth=8:
//     BatchHasher (arity-2 chain)   51,183 constraints per stream
//     WideBatchHasher(100, 16)      16,736 constraints per stream
//
// NOT INTERCHANGEABLE WITH THE EXISTING CIRCUIT. The digest differs, so
// the running commitment differs: proofs produced here will not verify
// against hashes from `gain_step.circom`, and the Rust-side reference
// (audio_parser::poseidon_hash_chunks) computes the arity-2 chain, so the
// suite's expected final hashes do not match either. This circuit is for
// measuring the optimisation; adopting it means migrating the Rust hash
// to the same tree and regenerating any stored commitments.

template GainWide(batch_size, bit_depth, k, arity) {
    //=== Previous Hashes ===//
    signal input step_in[2];

    //=== Next Hashes ===//
    signal output step_out[2];

    //=== Current Batches ===//
    signal input orig[batch_size];
    signal input edit[batch_size];

    //=== Hash Components ===//
    component orig_batch_hasher = WideBatchHasher(batch_size, arity);
    component edit_batch_hasher = WideBatchHasher(batch_size, arity);
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
    component check = BatchCheckGain(batch_size, bit_depth, k);
    check.orig <== orig;
    check.edit <== edit;
}

component main { public [step_in] } = GainWide(100, 8, 2, 4);
