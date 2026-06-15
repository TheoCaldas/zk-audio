pragma circom 2.1.6;
include "circom/poseidon.circom";

template Hasher(inputSize) {
    signal input values[inputSize];
    signal output hash;

    component hasher = Poseidon(inputSize);
    for (var i = 0; i < inputSize; i++) {
        hasher.inputs[i] <== values[i];
    }
    hash <== hasher.out;
}

template BatchHasher (width) {
    signal input batch[width];
    signal output hash;

    component hasher[width-1];

    for(var i=0; i < width-1; i++) {
        hasher[i] = Hasher(2);
        hasher[i].values[0] <== i == 0 ? batch[0] : hasher[i-1].hash;
        hasher[i].values[1] <== batch[i+1];
    }
    hash <== hasher[width-2].hash;
}