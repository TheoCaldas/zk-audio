pragma circom 2.1.6;

// ============================================================================
// UNSOUND - BENCHMARK ONLY. DO NOT USE FOR PROVENANCE PROOFS.
// ============================================================================
// This circuit commits to the ORIGINAL stream only. The edited-stream hash
// is dropped, which severs the binding between the proof and the artefact a
// verifier actually holds:
//
//   a prover runs this honestly over (alpha, beta), obtains a valid proof
//   and terminal state (h_alpha), then publishes ANY beta' it likes. The
//   verifier checks the proof (valid) and the signature over h_alpha
//   (valid) and accepts. Nothing in that chain touches beta'.
//
// `beta` is mathematically determined by alpha and k, but the verifier
// cannot recompute it without alpha - which is exactly what the proof
// hides. Determinacy is not checkability.
//
// Kept only to quantify what the second commitment costs. The sound
// variant is gain_sponge2h_b8_k2_B*.circom, which retains both hashes.
// ============================================================================

include "hasher.circom";
include "decompressor.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";
include "edits/gain/gain_step.circom";

// `Gain` committing to the ORIGINAL stream only.
//
// The existing circuit maintains two running Poseidon hashes, one per
// stream, costing 2 x BatchHasher(B) per fold. The edit-side digest is
// redundant: the circuit already PROVES `edit = clamp(k*orig)` for every
// sample, so a verifier holding the original commitment and the publicly
// agreed relation can recompute the edited stream, and with it any
// commitment over it. Hashing it inside the circuit pays for information
// the proof already establishes.
//
// What stays sound:
//   - the edit relation is still enforced per sample (BatchCheckGain is
//     untouched);
//   - `orig` is still bound to a running commitment, so the proof is still
//     about one specific recording;
//   - `edit` is bound *through* the relation: given the committed original
//     and the public k, the edited stream is uniquely determined.
//
// What changes for a verifier: they no longer receive an edited-stream
// digest for free. If the protocol needs one (e.g. to hand a consumer a
// commitment to the published audio without re-deriving it), it has to be
// computed outside the circuit - cheap off-circuit, expensive in-circuit.
// That is a protocol-level trade, not a soundness one.
//
// IVC state narrows from 2 to 1. `state_width` must be 1 in the registry.

template GainOneHash(batch_size, bit_depth, k) {
    //=== Previous hash: original stream only ===//
    signal input step_in[1];

    //=== Next hash ===//
    signal output step_out[1];

    //=== Current Batches ===//
    signal input orig[batch_size];
    signal input edit[batch_size];

    //=== Hash Components: ONE stream, not two ===//
    component orig_batch_hasher = BatchHasher(batch_size);
    component orig_hasher = Hasher(2);

    orig_batch_hasher.batch <== orig;
    orig_hasher.values[0] <== step_in[0];
    orig_hasher.values[1] <== orig_batch_hasher.hash;
    step_out[0] <== orig_hasher.hash;

    //=== Verifier Component: unchanged ===//
    component check = BatchCheckGain(batch_size, bit_depth, k);
    check.orig <== orig;
    check.edit <== edit;
}

component main { public [step_in] } = GainOneHash(100, 8, 2);
