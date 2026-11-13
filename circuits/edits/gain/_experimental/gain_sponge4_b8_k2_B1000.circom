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
include "hasher_sponge.circom";
include "decompressor.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";
include "edits/gain/gain_step.circom";

// Both commitment optimisations together: ONE stream hashed (the edited
// digest is recomputable from the committed original plus the proven
// relation) and that stream hashed with a rate-4 sponge instead of the
// arity-2 chain. Digest differs from every other variant; state_width 1.
template GainSponge4(batch_size, bit_depth, k) {
    signal input step_in[1];
    signal output step_out[1];
    signal input orig[batch_size];
    signal input edit[batch_size];

    component bh = SpongeBatchHasher(batch_size, 4);
    component fold = Hasher(2);
    bh.batch <== orig;
    fold.values[0] <== step_in[0];
    fold.values[1] <== bh.hash;
    step_out[0] <== fold.hash;

    component check = BatchCheckGain(batch_size, bit_depth, k);
    check.orig <== orig;
    check.edit <== edit;
}
component main { public [step_in] } = GainSponge4(1000, 8, 2);
