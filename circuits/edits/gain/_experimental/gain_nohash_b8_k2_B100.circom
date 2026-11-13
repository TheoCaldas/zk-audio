pragma circom 2.1.6;

include "decompressor.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";
include "edits/gain/gain_step.circom";

// ============================================================================
// ABLATION ONLY - NOT SOUND. DO NOT USE FOR PROVENANCE PROOFS.
// ============================================================================
// `Gain` with the Poseidon commitment chain removed, to measure what the
// hashing costs. It proves the gain relation over each batch and nothing
// else: a prover can supply any orig/edit pair satisfying the relation, a
// different pair at every step, and the verifier cannot tell. Nothing binds
// the proven samples to a recording, and nothing links consecutive steps.
//
// The constraint count is still a valid lower bound on the relation's cost.
// ============================================================================

template GainNoHash(batch_size, bit_depth, k) {
    //=== IVC state: carried through untouched (see header) ===//
    signal input step_in[2];
    signal output step_out[2];

    //=== Current Batches ===//
    signal input orig[batch_size];
    signal input edit[batch_size];

    step_out[0] <== step_in[0];
    step_out[1] <== step_in[1];

    //=== Verifier Component - the only real work left ===//
    component check = BatchCheckGain(batch_size, bit_depth, k);
    check.orig <== orig;
    check.edit <== edit;
}

component main { public [step_in] } = GainNoHash(100, 8, 2);
