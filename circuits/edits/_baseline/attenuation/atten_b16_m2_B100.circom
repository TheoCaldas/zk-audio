pragma circom 2.1.6;
// BASELINE (pre-optimisation) circuit: arity-2 Poseidon chain in
// BatchHasher. Preserved so the numbers in logs/bench/ recorded before
// the sponge change remain reproducible. The current default lives at
// circuits/edits/attenuation/ and uses a rate-4 sponge; its digest differs.

include "edits/_baseline/attenuation/atten_step.circom";

component main { public [step_in] } = Attenuate(100, 16, 2);
