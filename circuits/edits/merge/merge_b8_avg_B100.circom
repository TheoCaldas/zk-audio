pragma circom 2.1.6;

include "merge_step.circom";

component main { public [step_in] } = Merge(100, 8, 1);
