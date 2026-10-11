pragma circom 2.1.6;
include "circom/bitify.circom";


template Decompressor(bit_depth) {
    var nBits = 256 - bit_depth;
    var size = nBits / bit_depth;

    signal input in;
    signal output out[size];

    component toBits = Num2Bits(nBits);
    toBits.in <== in;

    component toNum[size];

    for (var i = 0; i < size; i++) {
        toNum[i] = Bits2Num(bit_depth);

        for (var b = 0; b < bit_depth; b++) {
            toNum[i].in[b] <== toBits.out[i * bit_depth + b];
        }

        out[i] <== toNum[i].out;
    }
}

// component main { public [in] } = Decompressor(16);