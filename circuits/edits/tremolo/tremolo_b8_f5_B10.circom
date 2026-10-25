pragma circom 2.1.6;

include "tremolo_step.circom";

// period = 8820 samples = 5Hz at 44100Hz (stereo-interleaved: see docs)
component main { public [step_in] } = Tremolo(10, 8, 8820, 256);
