pragma circom 2.1.6;

include "gain_step.circom";

component main { public [step_in] } = Gain(10, 16, 2);
