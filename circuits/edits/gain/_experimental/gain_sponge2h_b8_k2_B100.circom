pragma circom 2.1.6;

include "hasher.circom";
include "hasher_sponge.circom";
include "decompressor.circom";
include "circom/comparators.circom";
include "circom/mux1.circom";
include "edits/gain/gain_step.circom";

// `Gain` with BOTH running commitments retained, and each batch digest
// computed with a rate-`rate` sponge instead of the arity-2 Poseidon
// chain. This is the SOUND optimisation: the IVC state stays
// (h_orig, h_edit), so a verifier can still recompute h_edit from the
// published edited stream and check it against the proof's terminal
// state. Only the way each batch is absorbed changes.
//
// Contrast with gain_step_sponge4.circom, which also drops the edited
// commitment: that breaks the binding between the proof and the published
// artefact (a prover can publish any audio and the proof still verifies),
// so it is NOT a valid optimisation of this protocol.
//
// Digest differs from BatchHasher's, so the Rust reference hash must be
// migrated in lockstep; the IVC *shape* is unchanged.

template GainSponge2H(batch_size, bit_depth, k, rate) {
    signal input step_in[2];
    signal output step_out[2];
    signal input orig[batch_size];
    signal input edit[batch_size];

    component obh = SpongeBatchHasher(batch_size, rate);
    component ebh = SpongeBatchHasher(batch_size, rate);
    component ofold = Hasher(2);
    component efold = Hasher(2);

    obh.batch <== orig;
    ofold.values[0] <== step_in[0];
    ofold.values[1] <== obh.hash;
    step_out[0] <== ofold.hash;

    ebh.batch <== edit;
    efold.values[0] <== step_in[1];
    efold.values[1] <== ebh.hash;
    step_out[1] <== efold.hash;

    component check = BatchCheckGain(batch_size, bit_depth, k);
    check.orig <== orig;
    check.edit <== edit;
}

component main { public [step_in] } = GainSponge2H(100, 8, 2, 4);
