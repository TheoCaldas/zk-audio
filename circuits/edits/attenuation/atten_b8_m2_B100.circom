pragma circom 2.1.6;

include "atten_step.circom";

component main { public [step_in] } = Attenuate(100, 8, 2);
