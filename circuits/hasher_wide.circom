pragma circom 2.1.6;
include "circom/poseidon.circom";
include "hasher.circom";

// Wide-arity batch hashing: a tree of Poseidon(arity) instead of
// BatchHasher's arity-2 chain.
//
// circomlib's Poseidon accepts up to 16 inputs, so absorbing several
// elements per permutation cuts the call count. Per-input cost falls only
// ~2x from arity 2 to 16, though: the S-box work per permutation grows
// slowly with t but the MDS mixing is t^2.
//
// Superseded by the rate-4 sponge in hasher_sponge.circom, which needs no
// second layer. Kept to reproduce the comparison in README.md,
// "Benchmarks".

template WideBatchHasher(width, arity) {
    signal input batch[width];
    signal output hash;

    // Layer sizes: width -> ceil(width/arity) -> ... -> 1
    var sizes[32];
    var depth = 0;
    var n = width;
    sizes[0] = width;
    while (n > 1) {
        n = (n + arity - 1) \ arity;
        depth++;
        sizes[depth] = n;
    }

    // Flatten every layer's values into one buffer, laid out layer by layer.
    var total = 0;
    for (var l = 0; l <= depth; l++) { total += sizes[l]; }

    signal buf[total];
    var offset[32];
    offset[0] = 0;
    for (var l = 1; l <= depth; l++) { offset[l] = offset[l-1] + sizes[l-1]; }

    for (var i = 0; i < width; i++) { buf[i] <== batch[i]; }

    // One Poseidon per node above the leaves.
    var nodes = total - width;
    component h[nodes];
    var nd = 0;
    for (var l = 1; l <= depth; l++) {
        for (var j = 0; j < sizes[l]; j++) {
            h[nd] = Poseidon(arity);
            for (var s = 0; s < arity; s++) {
                var src = j * arity + s;
                // Zero-pad the final partial group. The pad length is fixed
                // by the compile-time (width, arity), so it is not a
                // prover-chosen value.
                h[nd].inputs[s] <== src < sizes[l-1] ? buf[offset[l-1] + src] : 0;
            }
            buf[offset[l] + j] <== h[nd].out;
            nd++;
        }
    }

    hash <== buf[total - 1];
}
