pragma circom 2.1.6;
// BASELINE (pre-optimisation) circuit: arity-2 Poseidon chain in
// BatchHasher. Preserved so the numbers in logs/bench/ recorded before
// the sponge change remain reproducible. The current default lives at
// circuits/edits/tremolo/ and uses a rate-4 sponge; its digest differs.

include "edits/_baseline/tremolo/tremolo_step.circom";

component main { public [step_in] } = Tremolo(100, 8, 8820, 256);
