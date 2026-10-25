pragma circom 2.1.6;

include "tremolo_step.circom";

component main { public [step_in] } = Tremolo(100, 8, 8820, 256);
