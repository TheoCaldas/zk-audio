pragma circom 2.1.6;

include "concat_step.circom";

component main { public [step_in] } = Concat(10, 8);
