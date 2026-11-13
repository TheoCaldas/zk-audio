pragma circom 2.1.6;

include "decompressor.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";
include "edits/gain/gain_step.circom";

// ============================================================================
// ABLATION ONLY - NOT SOUND, NOT FOR PRODUCTION USE
// ============================================================================
//
// `Gain` with the Poseidon commitment chain removed, to measure what the
// hashing costs. It proves the gain relation over the batch and NOTHING
// else.
//
// What this breaks, and why the result is still useful:
//
//   The real circuit folds a running Poseidon hash of each stream into the
//   IVC state, so the final proof commits to *which* audio was processed.
//   Without it, a prover can supply any `orig`/`edit` pair satisfying
//   `edit = clamp(k*orig)` at each step - including a different pair at
//   every step - and the verifier cannot tell. There is no binding between
//   the proven samples and any particular recording, and no link between
//   consecutive steps. As an audio-provenance proof this is worthless.
//
//   It is still a valid measurement of the *arithmetic* half of the step
//   circuit, which is the point: subtracting this from the real circuit
//   gives the price of the commitment.
//
// `step_in`/`step_out` are kept and passed through unchanged. Nova needs a
// non-empty IVC state, and a pass-through keeps the folding machinery and
// the public-input shape identical to the real circuit, so the comparison
// isolates the hashing rather than also changing the IVC interface.

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
