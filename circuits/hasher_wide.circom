pragma circom 2.1.6;
include "circom/poseidon.circom";
include "hasher.circom";

// Wide-arity batch hashing.
//
// `BatchHasher` in hasher.circom folds a batch with a chain of arity-2
// Poseidon calls: B-1 permutations for B elements. circomlib's Poseidon
// accepts up to 16 inputs (its N_ROUNDS_P table is indexed t-2 for
// t = nInputs+1, with 16 entries), so absorbing several elements per
// permutation does the same job with far fewer calls.
//
// MEASURED circomlib cost (compiled, not estimated):
//
//     Poseidon(2)   517 constraints   258.5 per input
//     Poseidon(4)   736               184.0
//     Poseidon(8)  1171               146.4
//     Poseidon(16) 2092               130.8
//
// Note the per-input cost falls only ~2x from arity 2 to 16, not with the
// call count: the S-box work per permutation grows slowly with t, but the
// MDS mixing is t^2, which claws back much of the saving. The win comes
// from needing 8 calls instead of 99, not from each call being cheap.
//
// A TREE, not a chain. Hashing all B elements in one shot is impossible
// past 16, and a chain of arity-a calls would serialise. Successive layers
// of arity-`a` nodes give ceil(log_a(B)) layers and ceil(B/a)+... calls.
//
// DOMAIN SEPARATION / SOUNDNESS. This changes the commitment value: a
// digest from this template does NOT equal `BatchHasher`'s for the same
// input, so the Rust-side reference hash has to be updated in lockstep or
// every proof fails verification. The tree is also only collision
// resistant if the arity is FIXED for a given batch_size - which it is,
// since batch_size is a compile-time parameter. A final partial group is
// zero-padded; because `a` and `batch_size` are both fixed at compile
// time, the padding length is constant and cannot be manipulated by a
// prover, so no length-extension ambiguity arises.

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
