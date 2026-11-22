#!/usr/bin/env python3
"""Generate a fused composite step circuit from a chain spec.

A sequential chain (src/chain.rs) proves one stage per proof: each stage
decompresses its input, applies its relation, recompresses, and hashes two
streams. A fused circuit proves the whole composite relation in one step
circuit: decompress once, apply every stage's relation digit-wise in
signals, recompress once, hash only the input and the final output.

Generated rather than hand-written because the artefact set is a product:
7 edit families x 3 bit depths x per-family parameters x chain order. The
per-stage relation templates (`ExpectedEditGain`, `AttenuatedGain`,
`DistortSample`, `ModulateSample`) are reused unmodified from the family
step circuits, so a fused circuit cannot drift from the stage it fuses.

Only pointwise stages can fuse. crop/concat/merge change stream length or
arity, so they have no sample-level `f(x)` to compose and must stay in a
sequential chain.

Usage:
    python3 scripts/gen_fused_circuit.py gain_k2 atten_m1 dist_d100 \\
        --bit-depth 8 --batch-size 100

    python3 scripts/gen_fused_circuit.py gain_k2 atten_m1 --bd 8 --B 10 \\
        --print    # to stdout instead of writing

Writes circuits/edits/_fused/<stem>.circom, where the stem is
    fused_<stage>_<stage>..._b<bd>_B<batch>
matching `bench::config::fused_circuit_name_for`.
"""
import argparse
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DIR = os.path.join(ROOT, "circuits", "edits", "_fused")

# Pointwise stages, keyed by the registry name used in src/bench/config.rs.
#
# `emit` renders the circom that maps one normalized digit to the next
# stage's normalized digit. Each takes the input signal expression and a
# unique suffix, and returns (lines, output_expression).
#
# `bounded` records that the stage's output is provably in
# [0, 2^bd - 1]. Fusion makes this load-bearing in a way a single stage
# does not: an intermediate digit is never re-range-checked by a
# Decompressor, so the next stage's comparators and the final
# Recompressor both rely on the preceding stage's own bound. A stage
# without one cannot be an interior stage.
STAGES = {}


def _gain(k):
    def emit(src, sfx, bd):
        return (
            [
                f"    component g_{sfx} = ClampedGain({bd}, {k}, GAIN_BITS_{sfx});",
                f"    g_{sfx}.orig <== {src};",
            ],
            f"g_{sfx}.edit",
        )

    def consts(sfx, bd):
        # Width must cover k*(2^bd - 1) without wrapping, computed the
        # same way BatchCheckGain does so a fused circuit cannot
        # under-size the comparator where the stage circuit would not.
        return [
            f"    var GAIN_RAW_MAX_{sfx} = {k} * ((1 << {bd}) - 1);",
            f"    var GAIN_BITS_{sfx} = 0;",
            f"    var GAIN_PROBE_{sfx} = GAIN_RAW_MAX_{sfx};",
            f"    while (GAIN_PROBE_{sfx} > 0) {{",
            f"        GAIN_BITS_{sfx}++;",
            f"        GAIN_PROBE_{sfx} = GAIN_PROBE_{sfx} >> 1;",
            "    }",
        ]

    return {
        "include": "edits/gain/gain_step.circom",
        "emit": emit,
        "consts": consts,
        "bounded": True,  # ClampedGain clamps to [lo, hi] then shifts
        "bit_depths": (8, 16, 24),
    }


def _atten(m):
    def emit(src, sfx, bd):
        return (
            [
                f"    component a_{sfx} = AttenuatedGain({bd}, {m});",
                f"    a_{sfx}.orig <== {src};",
            ],
            f"a_{sfx}.edit",
        )

    return {
        "include": "edits/attenuation/atten_step.circom",
        "emit": emit,
        "consts": lambda sfx, bd: [],
        "bounded": True,  # |x/2^m| <= |x|, never leaves the range
        "bit_depths": (8, 16, 24),
    }


def _dist(blend):
    def emit(src, sfx, bd):
        return (
            [
                f"    component d_{sfx} = DistortSample({bd}, {blend}, DIST_VBITS_{sfx});",
                f"    d_{sfx}.orig <== {src};",
            ],
            f"d_{sfx}.edit",
        )

    def consts(sfx, bd):
        # v is maximised at D = scale, a = half, orig_n = 2^bd - 1;
        # mirrors BatchCheckDistort.
        return [
            f"    var DIST_HALF_{sfx} = 1 << ({bd} - 1);",
            f"    var DIST_SCALE_{sfx} = 1 << 16;",
            f"    var DIST_VMAX_{sfx} = 3 * DIST_SCALE_{sfx} * DIST_HALF_{sfx};",
            f"    var DIST_VBITS_{sfx} = 0;",
            f"    var DIST_PROBE_{sfx} = DIST_VMAX_{sfx};",
            f"    while (DIST_PROBE_{sfx} > 0) {{",
            f"        DIST_VBITS_{sfx}++;",
            f"        DIST_PROBE_{sfx} = DIST_PROBE_{sfx} >> 1;",
            "    }",
        ]

    return {
        "include": "edits/distortion/dist_step.circom",
        "emit": emit,
        "consts": consts,
        "bounded": True,  # DistortSample clamps to [0, 2^bd - 1]
        "bit_depths": (8, 16, 24),
    }


def _tremolo(freq):
    # Tremolo is time-varying: its LFO is a function of absolute sample
    # index, held constant per packed element. That makes it the one
    # pointwise stage whose emitter needs the element index and a
    # per-element `lfo` signal, and it widens the IVC state to carry the
    # proven sample counter.
    def emit(src, sfx, bd):
        return (
            [
                f"    component t_{sfx} = ModulateSample({bd}, TREM_UBITS_{sfx});",
                f"    t_{sfx}.orig <== {src};",
                f"    t_{sfx}.lfo <== lfo_in;",
            ],
            f"t_{sfx}.edit",
        )

    def consts(sfx, bd):
        # u = L*(orig_n - half) + scale*half, L <= 2^16.
        return [
            f"    var TREM_HALF_{sfx} = 1 << ({bd} - 1);",
            f"    var TREM_UMAX_{sfx} = (1 << 16) * ((1 << {bd}) - 1) + (1 << 16) * TREM_HALF_{sfx};",
            f"    var TREM_UBITS_{sfx} = 0;",
            f"    var TREM_PROBE_{sfx} = TREM_UMAX_{sfx};",
            f"    while (TREM_PROBE_{sfx} > 0) {{",
            f"        TREM_UBITS_{sfx}++;",
            f"        TREM_PROBE_{sfx} = TREM_PROBE_{sfx} >> 1;",
            "    }",
        ]

    return {
        "include": "edits/tremolo/tremolo_step.circom",
        "emit": emit,
        "consts": consts,
        "bounded": True,  # L <= 2^16 bounds the rescaled result
        "bit_depths": (8, 16, 24),
        "time_varying": True,
        "freq": freq,
    }


STAGES = {
    "gain_k2": _gain(2),
    "gain_k3": _gain(3),
    "gain_k4": _gain(4),
    "atten_m1": _atten(1),
    "atten_m2": _atten(2),
    "dist_d100": _dist(1 << 16),
    "dist_d50": _dist(1 << 15),
    "tremolo_f5": _tremolo(5),
}

# Stages that cannot fuse, with the reason, so the error names it rather
# than reporting "unknown stage".
NON_POINTWISE = {
    "crop": "changes stream length; it has no sample-level f(x)",
    "concat": "changes stream length and commits to two source spans",
    "merge": "takes two input streams, so it is not a unary map",
    "gain2": "legacy arity-2-chain hashing; fuse the generalized gain family instead",
}


def comp_size_for(bit_depth):
    """Digits per packed field element, as Decompressor computes it."""
    return (256 - bit_depth) // bit_depth


def fused_stem(stages, bit_depth, batch_size):
    return "fused_" + "_".join(stages) + f"_b{bit_depth}_B{batch_size}"


def validate(stages, bit_depth):
    if not stages:
        sys.exit("error: a fused circuit needs at least one stage")

    for i, name in enumerate(stages):
        if name in NON_POINTWISE:
            sys.exit(
                f"error: '{name}' cannot be fused: {NON_POINTWISE[name]}.\n"
                "       Keep it as a sequential chain stage (src/chain.rs)."
            )
        if name not in STAGES:
            sys.exit(
                f"error: unknown stage '{name}'. Known: {', '.join(sorted(STAGES))}"
            )
        if bit_depth not in STAGES[name]["bit_depths"]:
            sys.exit(f"error: stage '{name}' has no relation at bit depth {bit_depth}")

        # An interior stage's output feeds the next stage's comparators and,
        # at the end, the Recompressor -- neither of which re-range-checks
        # it. Only the final stage could tolerate an unbounded output, and
        # none of the current families is unbounded, so this is a guard
        # against a future one being added without a bound.
        if not STAGES[name]["bounded"] and i + 1 < len(stages):
            sys.exit(
                f"error: stage '{name}' does not provably bound its output to "
                f"[0, 2^{bit_depth} - 1], so it cannot be an interior fused stage: "
                "the next stage would read a digit no Decompressor range-checked."
            )


def render(stages, bit_depth, batch_size, period):
    comp = comp_size_for(bit_depth)
    specs = [STAGES[s] for s in stages]
    time_varying = [s for s in specs if s.get("time_varying")]
    state_width = 3 if time_varying else 2
    stem = fused_stem(stages, bit_depth, batch_size)
    arrow = " -> ".join(stages)

    includes = ["hasher.circom", "hasher_sponge.circom", "decompressor.circom"]
    for s in specs:
        if s["include"] not in includes:
            includes.append(s["include"])
    if time_varying:
        includes.append("edits/tremolo/tremolo_table.circom")

    L = []
    L.append("pragma circom 2.1.6;")
    L.append("")
    L.append("// GENERATED by scripts/gen_fused_circuit.py -- do not edit by hand.")
    L.append("//")
    L.append(f"// Fused composite step: {arrow}")
    L.append("//")
    # Outermost application is the LAST stage: f_n(...f_1(orig)...).
    nest = "".join(f"f{i}(" for i in range(len(stages), 0, -1))
    L.append(
        "// Proves edit = " + nest + "orig" + ")" * len(stages) + " over every sample"
    )
    L.append(
        "// in the batch, decompressing the input once and recompressing the result"
    )
    L.append(
        "// once. The intermediate streams are never packed, never hashed and get"
    )
    L.append(
        "// no commitment, so nothing can be proven about them later; that is the"
    )
    L.append("// trade for the saving. See docs/edit_chains.md, \"Fusion\".")
    L.append("//")
    L.append(
        "// Each stage's output is consumed as a digit that no Decompressor"
    )
    L.append(
        "// range-checks, so the composition relies on every stage bounding its own"
    )
    L.append(
        "// output to [0, 2^bit_depth - 1]. The generator enforces that for every"
    )
    L.append("// interior stage.")
    L.append("//")
    L.append(
        "// Rounding is applied per stage, not fused: floor(f2(floor(f1(x)))) is not"
    )
    L.append(
        "// floor(f2(f1(x))). The Rust reference must apply the stages in sequence"
    )
    L.append("// for the same reason.")
    L.append("")
    for inc in includes:
        L.append(f'include "{inc}";')
    L.append("")

    # The composite map over one digit.
    L.append("// One digit through every stage, in order.")
    if time_varying:
        L.append(
            "// `lfo_in` is the LFO value for the packed element this digit belongs"
        )
        L.append("// to, held constant across the element as tremolo_step does.")
    L.append(f"template FusedSample{batch_size}_{bit_depth}() {{")
    L.append("    signal input orig;")
    if time_varying:
        L.append("    signal input lfo_in;")
    L.append("    signal output edit;")
    L.append("")

    for i, (name, spec) in enumerate(zip(stages, specs)):
        sfx = f"s{i}"
        consts = spec["consts"](sfx, bit_depth)
        if consts:
            L.extend(consts)

    L.append("")
    cur = "orig"
    for i, (name, spec) in enumerate(zip(stages, specs)):
        sfx = f"s{i}"
        L.append(f"    // stage {i + 1}: {name}")
        lines, out = spec["emit"](cur, sfx, bit_depth)
        L.extend(lines)
        cur = out
        L.append("")
    L.append(f"    edit <== {cur};")
    L.append("}")
    L.append("")

    # Batch check: one decompress, composite map, one recompress.
    if time_varying:
        L.append("template FusedBatchCheck(batch_size, bit_depth, period, table_n) {")
    else:
        L.append("template FusedBatchCheck(batch_size, bit_depth) {")
    L.append("    // Integer division (\\), not field division: at bit_depth=24")
    L.append("    // 232/24 is not exact, and `/` would compute a modular inverse.")
    L.append("    var comp_size = (256 - bit_depth) \\ bit_depth;")
    L.append("")
    L.append("    signal input orig[batch_size];")
    L.append("    signal input edit[batch_size];")
    if time_varying:
        L.append("    signal input start_idx;   // absolute index of orig[0]'s first sample")
    L.append("")
    if time_varying:
        L.append("    // Width for the index division, as BatchCheckTremolo sizes it:")
        L.append("    // 40 bits covers far more samples than any audio this proves.")
        L.append("    var idx_bits = 40;")
        L.append("")
    L.append("    component decompressor_orig[batch_size];")
    L.append("    component fused[batch_size][comp_size];")
    L.append("    component recompressor[batch_size];")
    if time_varying:
        L.append("    component lfo[batch_size];")
    L.append("    signal expected[batch_size][comp_size];")
    L.append("")
    L.append("    for (var j = 0; j < batch_size; j++) {")
    L.append("        // The input is decompressed once for the whole composite,")
    L.append("        // which is the saving a sequential chain does not get: there")
    L.append("        // each stage pays its own Num2Bits.")
    L.append("        decompressor_orig[j] = Decompressor(bit_depth);")
    L.append("        decompressor_orig[j].in <== orig[j];")
    L.append("")
    if time_varying:
        L.append("        // ONE lookup per packed element, shared by its comp_size")
        L.append("        // samples, matching BatchCheckTremolo.")
        L.append("        lfo[j] = LfoSelect(period, table_n, idx_bits);")
        L.append("        lfo[j].idx <== start_idx + j * comp_size;")
        L.append("")
    L.append("        for (var s = 0; s < comp_size; s++) {")
    L.append(f"            fused[j][s] = FusedSample{batch_size}_{bit_depth}();")
    L.append("            fused[j][s].orig <== decompressor_orig[j].out[s];")
    if time_varying:
        L.append("            fused[j][s].lfo_in <== lfo[j].lfo;")
    L.append("            expected[j][s] <== fused[j][s].edit;")
    L.append("        }")
    L.append("")
    L.append("        // Safe to repack: the last stage bounds every digit to")
    L.append("        // [0, 2^bit_depth - 1].")
    L.append("        recompressor[j] = Recompressor(bit_depth);")
    L.append("        recompressor[j].in <== expected[j];")
    L.append("")
    L.append("        edit[j] === recompressor[j].out;")
    L.append("    }")
    L.append("}")
    L.append("")

    # Top-level step circuit.
    if state_width == 3:
        L.append("template Fused(batch_size, bit_depth, period, table_n) {")
        L.append("    var comp_size = (256 - bit_depth) \\ bit_depth;")
        L.append("")
    else:
        L.append("template Fused(batch_size, bit_depth) {")
    if state_width == 3:
        L.append("    // (prev_orig_hash, prev_edit_hash, sample_index)")
        L.append("    signal input step_in[3];")
        L.append("    signal output step_out[3];")
    else:
        L.append("    // (prev_orig_hash, prev_edit_hash)")
        L.append("    signal input step_in[2];")
        L.append("    signal output step_out[2];")
    L.append("")
    L.append("    signal input orig[batch_size];")
    L.append("    signal input edit[batch_size];")
    L.append("")
    L.append("    // Only the input and the final output are committed to.")
    L.append("    component orig_batch_hasher = SpongeBatchHasher(batch_size, 4);")
    L.append("    component edit_batch_hasher = SpongeBatchHasher(batch_size, 4);")
    L.append("    component orig_hasher = Hasher(2);")
    L.append("    component edit_hasher = Hasher(2);")
    L.append("")
    L.append("    orig_batch_hasher.batch <== orig;")
    L.append("    orig_hasher.values[0] <== step_in[0];")
    L.append("    orig_hasher.values[1] <== orig_batch_hasher.hash;")
    L.append("    step_out[0] <== orig_hasher.hash;")
    L.append("")
    L.append("    edit_batch_hasher.batch <== edit;")
    L.append("    edit_hasher.values[0] <== step_in[1];")
    L.append("    edit_hasher.values[1] <== edit_batch_hasher.hash;")
    L.append("    step_out[1] <== edit_hasher.hash;")
    L.append("")

    if state_width == 3:
        L.append("    // The counter advances by exactly one batch of samples, which is")
        L.append("    // what binds the LFO phase: the prover cannot choose where in the")
        L.append("    // stream this batch sits, and the verifier checks the final")
        L.append("    // counter == N.")
        L.append("    step_out[2] <== step_in[2] + batch_size * comp_size;")
        L.append("")
        L.append(
            "    component check = FusedBatchCheck(batch_size, bit_depth, period, table_n);"
        )
        L.append("    check.orig <== orig;")
        L.append("    check.edit <== edit;")
        L.append("    check.start_idx <== step_in[2];")
    else:
        L.append("    component check = FusedBatchCheck(batch_size, bit_depth);")
        L.append("    check.orig <== orig;")
        L.append("    check.edit <== edit;")
    L.append("}")
    L.append("")

    if state_width == 3:
        L.append("// A fused chain containing tremolo inherits its LFO period and")
        L.append("// table size, and with them the third state element. The period is")
        L.append("// in SAMPLES, so it depends on the sample rate the suite proves at.")
        L.append(
            f"component main {{ public [step_in] }} = Fused({batch_size}, {bit_depth}, {period}, 256);"
        )
    else:
        L.append(
            f"component main {{ public [step_in] }} = Fused({batch_size}, {bit_depth});"
        )
    L.append("")

    return stem, "\n".join(L)


def main():
    p = argparse.ArgumentParser(
        description="Generate a fused composite step circuit from a chain spec.",
        epilog="Stages: " + ", ".join(sorted(STAGES)),
    )
    p.add_argument("stages", nargs="+", help="registry edit names, in application order")
    p.add_argument("--bit-depth", "--bd", type=int, default=8, dest="bit_depth")
    p.add_argument("--batch-size", "--B", type=int, default=100, dest="batch_size")
    p.add_argument(
        "--period",
        type=int,
        default=8820,
        help="LFO period in samples for a tremolo stage (default 8820 = 5Hz at 44.1kHz)",
    )
    p.add_argument(
        "--print", action="store_true", dest="to_stdout", help="write to stdout"
    )
    args = p.parse_args()

    validate(args.stages, args.bit_depth)
    stem, src = render(args.stages, args.bit_depth, args.batch_size, args.period)

    if args.to_stdout:
        print(src)
        return

    os.makedirs(OUT_DIR, exist_ok=True)
    path = os.path.join(OUT_DIR, f"{stem}.circom")
    with open(path, "w") as f:
        f.write(src)

    rel = os.path.relpath(path, ROOT)
    comp = comp_size_for(args.bit_depth)
    print(f"wrote {rel}")
    print(
        f"  {len(args.stages)} stages, bd={args.bit_depth}, B={args.batch_size}, "
        f"{args.batch_size * comp} samples/step"
    )
    print(f"  compile:  ./compile.circuit.sh {rel}")


if __name__ == "__main__":
    main()
