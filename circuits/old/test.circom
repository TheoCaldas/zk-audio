pragma circom 2.1.6;
include "node_modules/circomlib/circuits/poseidon.circom";

template Test() {
    signal input step_in[2];
    signal output step_out[2];

    component h = Poseidon(2);
    h.inputs[0] <== 1;
    h.inputs[1] <== 2;

    log(h.out);
    log(h.inputs[0]);
    log(h.inputs[1]);
    step_out[0] <== h.out;
    step_out[1] <== step_in[1];
}

component main { public [step_in] } = Test();