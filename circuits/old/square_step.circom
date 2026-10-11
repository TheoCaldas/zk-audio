pragma circom 2.0.3;

template Square () {
    signal input step_in[2];

    signal output step_out[2];

    signal input x;

    step_out[0] <== x;
    step_out[1] <== x * x;
}

component main { public [step_in] } = Square();
