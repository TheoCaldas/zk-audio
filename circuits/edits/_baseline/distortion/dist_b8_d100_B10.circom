pragma circom 2.1.6;
// BASELINE (pre-optimisation) circuit: arity-2 Poseidon chain in
// BatchHasher. Preserved so the numbers in logs/bench/ recorded before
// the sponge change remain reproducible. The current default lives at
// circuits/edits/distortion/ and uses a rate-4 sponge; its digest differs.

include "edits/_baseline/distortion/dist_step.circom";

// blend = 65536 = d 1.0 (fully wet), the reference's KNOB_1 = 1.
component main { public [step_in] } = Distort(10, 8, 65536);
