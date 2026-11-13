#!/usr/bin/env python3
"""Per-step circuit size by edit family, at bit_depth=8 and B=100.

Scope is deliberately narrow: one depth, one batch size, `gain2` excluded
(it is the legacy non-clamping relation and not comparable to the others).
That leaves exactly one circuit per family, so the comparison is apples to
apples - every bar covers the same 3,100 audio samples per folding step.

Form: a horizontal bar chart, sorted. The job is "compare magnitude across
named categories", the names are long, and there is no time axis - so bars,
not lines, and horizontal so the family names read straight.

Constraints are a STATIC property of the compiled circuit, not a
measurement: every run of a family reports the identical number. The script
asserts that rather than averaging, because disagreement would mean two
different circuits were being compared under one name.

Run:  /usr/bin/python3 scripts/plot_edit_constraints.py
"""
import json
import glob
import os
import sys
import collections

import matplotlib
matplotlib.use("Agg")
import matplotlib.ticker
import matplotlib.pyplot as plt

ACCENT = "#2a78d6"
INK = "#0b0b0b"
INK2 = "#52514e"
GRID = "#dedddb"

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

BIT_DEPTH, BATCH = 8, 100
EXCLUDE = {"gain2"}
# "_baseline" = pre-optimisation (arity-2 chain), "_experimental" = post.
GENERATION = os.environ.get("ZK_AUDIO_BENCH_GENERATION", "_baseline")          # legacy relation, no clamping - not comparable

# What each family's step circuit has to do beyond the shared hash+unpack,
# for the annotation column. Kept short; the point is why the bars differ.
WORK = {
    "crop":       "pass-through + counter",
    "concat":     "select between two streams",
    "atten_m1":   "shift right (floor)",
    "gain_k2":    "multiply + saturate",
    "merge":      "average two streams",
    "tremolo_f5": "LFO table lookup + multiply",
    "dist_d100":  "rectify + blend (wet/dry)",
}


def load():
    fams = collections.defaultdict(set)
    nvals = collections.defaultdict(set)
    # Scope to one generation. Since the sponge became the default, the same
    # circuit_name denotes two different circuits depending on when the run
    # happened, and mixing them trips the consistency check below (which is
    # the check doing its job, not a bug).
    root = os.path.join(ROOT, "logs", "bench", GENERATION)
    for f in glob.glob(os.path.join(root, "**", "results.jsonl"), recursive=True):
        for line in open(f):
            line = line.strip()
            if not line:
                continue
            r = json.loads(line)
            if r.get("status") != "ok":
                continue
            if r.get("bit_depth") != BIT_DEPTH or r.get("batch_size") != BATCH:
                continue
            e = r.get("edit")
            if not e or e in EXCLUDE:
                continue
            fams[e].add((r["constraints_primary"], r.get("variables_primary"),
                         r.get("comp_size")))
            nvals[e].add(r["total_samples"])
    return fams, nvals


def main():
    fams, nvals = load()
    if not fams:
        print("no bd=8 B=100 rows found", file=sys.stderr)
        return 1

    # A family reporting two different constraint counts would mean the
    # circuit changed between runs; averaging that would hide it.
    for e, s in sorted(fams.items()):
        if len(s) != 1:
            print(f"ERROR: {e} has inconsistent circuit sizes: {sorted(s)}",
                  file=sys.stderr)
            return 1

    data = []
    for e, s in fams.items():
        c, v, cs = next(iter(s))
        data.append((e, c, v, cs))
    data.sort(key=lambda t: t[1])

    comp = {d[3] for d in data}
    if len(comp) != 1:
        print(f"ERROR: mixed comp_size {comp}", file=sys.stderr)
        return 1
    spb = comp.pop() * BATCH          # audio samples per folding step

    fig, ax = plt.subplots(figsize=(11.6, 5.8))
    ys = range(len(data))
    names = [d[0] for d in data]
    cons = [d[1] for d in data]

    ax.barh(list(ys), cons, .62, color=ACCENT, zorder=3)
    lo = min(cons)
    for i, (e, c, v, _cs) in enumerate(data):
        ax.text(c + max(cons) * .012, i, f"{c:,}", va="center", ha="left",
                fontsize=10, color=INK, fontweight="bold")
        ax.text(c + max(cons) * .012, i - .265,
                f"{c/spb:.1f}/sample",
                va="center", ha="left", fontsize=7.8, color=INK2)

    ax.set_yticks(list(ys))
    ax.set_yticklabels(names, fontsize=10.5)
    ax.set_xlabel("constraints per folding step (primary circuit)")
    ax.set_xlim(0, max(cons) * 1.30)
    ax.xaxis.set_major_formatter(
        matplotlib.ticker.FuncFormatter(lambda v, _p: f"{v/1000:g}k" if v else "0"))
    ax.grid(axis="x", color=GRID, lw=.8)
    ax.set_axisbelow(True)
    for s in ("top", "right", "left"):
        ax.spines[s].set_visible(False)
    ax.tick_params(axis="y", length=0)

    fig.suptitle("Circuit cost per folding step, by edit", x=.008, ha="left",
                 fontsize=14, fontweight="bold", y=.975)
    ax.set_title(f"bit_depth {BIT_DEPTH} · B={BATCH} · {spb:,} audio samples per step · {GENERATION.lstrip(chr(95))} circuits",
                 loc="left", fontsize=9.6, color=INK2, pad=10)
    fig.text(.008, .018,
             "Constraints are a static property of the compiled circuit — identical "
             "across every run of a family, not a timing measurement.",
             fontsize=8.2, color=INK2, ha="left")

    fig.tight_layout(rect=(0, .045, 1, .935))
    out = os.path.join(ROOT, "logs", "bench", GENERATION, "_combined",
                       f"edit_constraints_b8_B100.png")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    fig.savefig(out, dpi=170)
    print("wrote", os.path.relpath(out, ROOT))

    print(f"\n{'edit':<12} {'constraints':>11} {'vars':>11} {'/sample':>9} "
          f"{'vs cheapest':>12} {'N values':>9}")
    for e, c, v, _cs in data:
        print(f"{e:<12} {c:>11,} {v:>11,} {c/spb:>9.1f} {c/lo:>11.2f}× "
              f"{len(nvals[e]):>8}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
