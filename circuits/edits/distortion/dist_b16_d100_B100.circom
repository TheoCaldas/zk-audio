pragma circom 2.1.6;

include "dist_step.circom";

component main { public [step_in] } = Distort(100, 16, 65536);
