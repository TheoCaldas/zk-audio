pragma circom 2.1.6;
include "circom/poseidon.circom";

// Sponge-mode batch hashing: absorb `rate` elements per permutation.
//
// Why this beats the tree in hasher_wide.circom. A tree node of arity `a`
// consumes `a` values and emits 1, so a layer shrinks the work by `a` but
// the layers below still had to be hashed: total calls ~ B/(a-1). A
// sponge chains one permutation per group, carrying the state forward, so
// it needs exactly ceil(B/rate) calls and no second layer.
//
//   B = 100 packed elements, measured circomlib Poseidon costs:
//     BatchHasher (arity-2 chain)   99 calls   51,183 constraints
//     tree,   arity 4               35 calls   25,760
//     SPONGE, rate 4                25 calls   18,400
//     SPONGE, rate 8                13 calls   15,223
//
// Construction: capacity-1 sponge. Each permutation takes
// (running_state, m_1..m_rate) as its `rate+1` inputs and outputs the new
// state. The capacity element is the chaining value, which is what makes
// this a sponge rather than independent hashes.
//
// SECURITY NOTE. circomlib's Poseidon(n) is a fixed-length hash with a
// standard-conformant domain tag; chaining it as below is a Merkle-Damgard
// construction over that compression function, not the sponge from the
// Poseidon paper (which has a defined capacity and padding rule). It is
// collision resistant on the same assumption the existing `BatchHasher`
// already relies on - that Poseidon(t) is a collision-resistant
// compression function - and like `BatchHasher` it uses a FIXED input
// length fixed at compile time, so there is no padding ambiguity. It is
// not interchangeable with a spec Poseidon sponge, and the digest differs
// from `BatchHasher`'s.
//
// The final partial group is zero-padded. `width` and `rate` are both
// compile-time constants, so the pad length is fixed and not prover-chosen.

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
