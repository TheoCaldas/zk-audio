pragma circom 2.1.6;
include "circom/poseidon.circom";

// Sponge-mode batch hashing: absorb `rate` elements per permutation,
// ceil(width/rate) calls in total.
//
// Capacity-1 construction: each permutation takes
// (running_state, m_1..m_rate) and outputs the new state, the capacity
// element being the chaining value. The final partial group is
// zero-padded; width and rate are compile-time constants, so the pad
// length is fixed, not prover-chosen.
//
// This is Merkle-Damgard over circomlib's fixed-length Poseidon, not the
// spec sponge, and its digest differs from BatchHasher's. See README.md,
// "Benchmarks", for the cost comparison and the security note.

template SpongeBatchHasher(width, rate) {
    signal input batch[width];
    signal output hash;

    var nperm = (width + rate - 1) \ rate;

    component h[nperm];
    signal state[nperm + 1];
    state[0] <== 0;                       // initial capacity

    for (var i = 0; i < nperm; i++) {
        h[i] = Poseidon(rate + 1);
        h[i].inputs[0] <== state[i];      // chaining value
        for (var j = 0; j < rate; j++) {
            var idx = i * rate + j;
            h[i].inputs[j + 1] <== idx < width ? batch[idx] : 0;
        }
        state[i + 1] <== h[i].out;
    }

    hash <== state[nperm];
}
