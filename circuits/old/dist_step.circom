pragma circom 2.1.6;

// template LessThan(n) {
//     assert(n <= 252);
//     signal input in[2];
//     signal output out;

//     component n2b = Num2Bits(n+1);

//     n2b.in <== in[0]+ (1<<n) - in[1];

//     out <== 1-n2b.out[n];
// }

template GreaterThan(nBits) {
    signal input x;
    signal input t;
    signal output out;

    signal diff;
    signal diff_bits[nBits];

    diff <== x - t;

    var i;
    for (i = 0; i < nBits; i++) {
        diff_bits[i] <-- (diff >> i) & 1;
        diff_bits[i] * (diff_bits[i] - 1) === 0;
    }

    signal reconstructed;

    var acc = 0;
    var i = 0;

    for (; i < nBits; i++) {
        acc += diff_bits[i] * (1 << i);
    }

    reconstructed <== acc;

    signal isZero;
    isZero <== 1;
    for (i = 0; i < nBits; i++) {
        isZero <== isZero * (1 - diff_bits[i]);
    }

    out <== 1 - isZero;
}

template Dist () {
    signal input step_in[2];

    signal output step_out[2];

    signal input x;

    var HALF = 255 / 2;
    var TWO  = 2;
    var ONE  = 255;
    var KNOB = ONE;         
    var INV_KNOB = ONE - KNOB;  

    // component isNeg = LessThan(252);
    // isNeg.in[0] <== x;
    // isNeg.in[1] <== 0;

    // signal negFlag;
    // negFlag <== isNeg.out;

    signal a <== x;
    signal b <== HALF;
    signal output result;

    component gt = GreaterThan(32);
    gt.x <== a;
    gt.t <== b;
    result <== gt.out;

    signal two_x;
    two_x <== TWO * x;

    // signal correction;
    // correction <== two_x * negFlag; 

    signal absx;
    absx <== x - (two_x * result);    

    signal shaped;
    shaped <== TWO * (absx - HALF);  

    signal part1;
    signal part2;

    part1 <== shaped * KNOB ;
    part2 <== x * INV_KNOB;

    step_out[0] <== part1;
    step_out[1] <== part1 + part2;
}

component main { public [step_in] } = Dist();
