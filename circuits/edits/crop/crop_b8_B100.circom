pragma circom 2.1.6;

include "crop_step.circom";

component main { public [step_in] } = Crop(100, 8);
