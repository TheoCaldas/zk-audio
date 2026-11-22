pragma circom 2.1.6;

include "crop_bound_step.circom";

component main { public [step_in] } = CropBound(100, 8);
