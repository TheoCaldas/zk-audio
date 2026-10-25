pragma circom 2.1.6;

include "dist_step.circom";

// blend = 65536 = d 1.0 (fully wet), the reference's KNOB_1 = 1.
component main { public [step_in] } = Distort(10, 8, 65536);
