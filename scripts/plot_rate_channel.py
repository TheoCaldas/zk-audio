#!/usr/bin/env python3
"""Figure for the rate/channel decomposition experiment (logs/bench/_baseline/rc/).

Two panels, because the two arms answer different questions and want
different forms:

  LEFT  - Arm 1: four (ch, f_s, T) routes to the SAME N. The job is "these
          are indistinguishable", so it is a dot plot with every repetition
          drawn, not a line chart: the reader should see the four clusters
          overlap. A line would imply a trend across the x categories, and
          there is deliberately no trend.

  RIGHT - Arm 2: halving one factor at a time. The job is "measured matches
          predicted", so bars (magnitude) with the prediction drawn as a
          separate marker on the same axis. NOT a dual axis - both are the
          same quantity, a ratio to the baseline.

Emphasis colouring: one accent hue carries the measurement, grey carries
context/prediction. Palette validated with the dataviz validator for both
light and dark surfaces.

Run:  /usr/bin/python3 scripts/plot_rate_channel.py
(needs matplotlib; the default Homebrew python3 here does not have it)
"""
import json
import glob
import collections
import os
import sys

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.lines import Line2D

# Validated palette (see dataviz validate_palette.js, light + dark PASS).
ACCENT = "#2a78d6"   # categorical slot 1 - the measurement
PREDICT = "#eb6834"  # categorical slot 2 - the prediction
INK = "#0b0b0b"
INK2 = "#52514e"
GRID = "#dedddb"

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)


def load(rc_dir):
    g = collections.defaultdict(list)
    for d in sorted(glob.glob(os.path.join(rc_dir, "run-*"))):
        mpath, rpath = os.path.join(d, "manifest.json"), os.path.join(d, "results.jsonl")
        if not (os.path.isfile(mpath) and os.path.isfile(rpath)):
            continue
        m = json.load(open(mpath))
        a = m["audio"]
        durs = m["config"].get("durations_sec") or []
        if not durs:
            continue
        key = (a["channels"], a["sample_rate"], round(durs[0], 6))
        for line in open(rpath):
            line = line.strip()
            if not line:
                continue
            r = json.loads(line)
            if r.get("status") == "ok":
                g[key].append(r)
    return g


def med(v):
    v = sorted(v)
    return v[len(v) // 2]


def main():
    rc = os.path.join(ROOT, "logs", "bench", "_baseline", "rc")
    g = load(rc)
    if not g:
        print(f"no results under {rc}", file=sys.stderr)
        return 1

    T0 = 1.124717
    arm1 = [((2, 44100, T0),        "2ch\n44.1kHz\n1.12s"),
            ((1, 44100, 2.249433),  "1ch\n44.1kHz\n2.25s"),
            ((2, 22050, 2.249433),  "2ch\n22.05kHz\n2.25s"),
            ((1, 22050, 4.498866),  "1ch\n22.05kHz\n4.50s")]
    arm2 = [((2, 44100, T0), "baseline\n2ch / 44.1kHz", 1.00),
            ((1, 44100, T0), "mono\n1ch / 44.1kHz",     0.50),
            ((2, 22050, T0), "half-rate\n2ch / 22.05kHz", 0.50),
            ((1, 22050, T0), "both\n1ch / 22.05kHz",    0.25)]
    missing = [k for k, *_ in arm1 + arm2 if k not in g]
    if missing:
        print(f"missing configs: {missing}", file=sys.stderr)
        return 1

    base = med([x["recursive_prove_ms"] / 1000 for x in g[(2, 44100, T0)]])

    fig, (axL, axR) = plt.subplots(1, 2, figsize=(13.2, 5.5),
                                   gridspec_kw={"width_ratios": [1, 1]})

    # ---------------- LEFT: Arm 1, four routes to one N ----------------
    xs = range(len(arm1))
    for i, (k, _lab) in enumerate(arm1):
        reps = [x["recursive_prove_ms"] / 1000 for x in g[k]]
        # every repetition, jittered, so the spread is visible as data
        for j, v in enumerate(reps):
            axL.plot(i + (j - 1) * 0.055, v, "o", ms=7, color=ACCENT,
                     alpha=.55, mew=1.4, mec="#fcfcfb", zorder=3)
        axL.plot([i - .17, i + .17], [med(reps)] * 2, "-", lw=2.6,
                 color=ACCENT, zorder=4, solid_capstyle="round")

    allv = [x["recursive_prove_ms"] / 1000 for k, _ in arm1 for x in g[k]]
    meds = [med([x["recursive_prove_ms"] / 1000 for x in g[k]]) for k, _ in arm1]
    # Two DIFFERENT spreads, and conflating them would overstate the result:
    #   - across route medians: the quantity the claim is about
    #   - within a route, across reps: the noise floor it must beat
    spread_routes = (max(meds) - min(meds)) / min(meds) * 100
    spread_reps = max((max(v) - min(v)) / min(v) * 100 for v in
                      ([x["recursive_prove_ms"] / 1000 for x in g[k]] for k, _ in arm1))
    axL.axhline(med(meds), color=INK2, lw=1, ls=":", zorder=1)

    axL.set_xticks(list(xs))
    axL.set_xticklabels([l for _, l in arm1], fontsize=9)
    axL.set_ylabel("RecursiveSNARK prove (s)")
    # NOT zero-based: the whole point is resolving a ~3% difference, which a
    # 0-based axis flattens into one line. The zoom is stated on the panel so
    # the tiny spread cannot be mistaken for a big one.
    lo, hi = min(allv), max(allv)
    pad = (hi - lo) * .55
    axL.set_ylim(lo - pad, hi + pad * 1.7)
    axL.set_xlim(-.5, 3.5)
    axL.grid(axis="y", color=GRID, lw=.8)
    axL.set_axisbelow(True)
    for s in ("top", "right"):
        axL.spines[s].set_visible(False)
    axL.set_title("Same N, four different routes   (y-axis zoomed, not 0-based)\n"
                  "N = 99,200 · 32 folds · 234,318 constraints — all identical",
                  loc="left", fontsize=10.5, color=INK, pad=26)
    axL.text(.5, .99,
             f"medians span {spread_routes:.1f}%  —  below the {spread_reps:.1f}% "
             f"rep-to-rep noise within a single route",
             transform=axL.transAxes, ha="center", va="top", fontsize=8.8,
             color=INK2,
             bbox=dict(boxstyle="round,pad=0.4", fc="#f2f1ef", ec="#dedddb", lw=.8))
    axL.legend(handles=[Line2D([], [], marker="o", ls="", color=ACCENT,
                               alpha=.55, mec="#fcfcfb", mew=1.4, ms=7,
                               label="individual repetition"),
                        Line2D([], [], color=ACCENT, lw=2.6, label="median")],
               frameon=False, fontsize=8.6, ncol=2,
               # Outside the axes: inside, the sample dot and median bar sit
               # in the data area and read as plotted points.
               loc="lower left", bbox_to_anchor=(0, 1.005))

    # ---------------- RIGHT: Arm 2, halving ----------------
    w = .5
    for i, (k, _lab, pred) in enumerate(arm2):
        r = med([x["recursive_prove_ms"] / 1000 for x in g[k]]) / base
        axR.bar(i, r, w, color=ACCENT, zorder=3)
        axR.plot([i - w / 2 - .06, i + w / 2 + .06], [pred] * 2, "-", lw=2.4,
                 color=PREDICT, zorder=5, solid_capstyle="round")
        axR.text(i, r + .028, f"{r:.3f}", ha="center", fontsize=9.6,
                 color=INK, fontweight="bold")
        n = g[k][0]["total_samples"]
        axR.text(i, .035, f"N={n:,}\n{g[k][0]['iterations']} folds",
                 ha="center", fontsize=8.2, color="#fcfcfb", zorder=6)

    axR.set_xticks(range(len(arm2)))
    axR.set_xticklabels([l for _, l, _ in arm2], fontsize=9)
    axR.set_ylabel("prove time ÷ baseline")
    axR.set_ylim(0, 1.17)
    axR.grid(axis="y", color=GRID, lw=.8)
    axR.set_axisbelow(True)
    for s in ("top", "right"):
        axR.spines[s].set_visible(False)
    axR.set_title("Same duration, halving one factor at a time\n"
                  "T = 1.1247s fixed — measured vs predicted",
                  loc="left", fontsize=10.5, color=INK, pad=10)
    axR.legend(handles=[plt.Rectangle((0, 0), 1, 1, color=ACCENT, label="measured"),
                        Line2D([], [], color=PREDICT, lw=2.4,
                               label="predicted by N = ch × f_s × T")],
               frameon=False, fontsize=8.6, loc="upper right")

    fig.suptitle("Proving cost depends only on N — not on how N is reached",
                 x=.008, ha="left", fontsize=14, fontweight="bold", y=.985)
    fig.text(.008, .017,
             "gain_k2 · bit_depth 8 · B=100 · 3 repetitions per configuration · "
             "Apple M2 Pro · logs/bench/_baseline/rc/",
             fontsize=8.2, color=INK2, ha="left")

    fig.tight_layout(rect=(0, .042, 1, .945))
    out = os.path.join(rc, "rate_channel_decomposition.png")
    fig.savefig(out, dpi=170)
    print("wrote", os.path.relpath(out, ROOT))
    return 0


if __name__ == "__main__":
    sys.exit(main())
