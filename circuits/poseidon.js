import * as circomlib from "circomlibjs";

const poseidon = await circomlib.buildPoseidon();

const h = poseidon([1n, 2n]);

console.log("dec:", poseidon.F.toString(h));
console.log("hex:", "0x" + BigInt(poseidon.F.toString(h)).toString(16));