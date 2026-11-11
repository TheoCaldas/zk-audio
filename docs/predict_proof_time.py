#!/usr/bin/env python3
"""Cost model for RecursiveSNARK prove time, fitted to logs/bench/.

    time = folds x ms_per_fold(bit_depth, batch_size, edit)
    folds = N / (comp_size(bit_depth) * batch_size)
    N     = channels * sample_rate * duration

The two factors are estimated very differently, and the difference matters:

  * `folds` is EXACT. It is arithmetic on the configuration, not a fit.
  * `ms_per_fold` is MEASURED per (edit, bit_depth, batch_size) where we have
    data, and only falls back to a power law in constraint count for cells we
    have never run.

That split is the whole point. Extrapolating in N is reliable because only
the exact factor grows: fitting on N <= 2.65M and predicting a 52.9M-sample
proof lands within 0.3%. Extrapolating to an unmeasured CIRCUIT is the weak
axis - leave-one-out CV on the power law is ~6.8% median but 28% worst case.

Usage
-----
    # predict one configuration
    python3 docs/predict_proof_time.py --edit gain_k2 --bit-depth 8 \\
        --batch-size 100 --duration 600 --channels 2 --sample-rate 44100

    # the matrix views
    python3 docs/predict_proof_time.py --matrix
"""
import argparse
import collections
import glob
import json
import math
import os
import statistics as st
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

MIN_FOLDS = 10          # below this, fixed setup dominates and ms/fold is inflated


def comp_size(bit_depth):
    """Samples packed per field element - integer division, as circom does."""
    return (256 - bit_depth) // bit_depth


def load(root=None):
    rows = []
    pat = os.path.join(root or ROOT, "logs", "bench", "**", "results.jsonl")
    for f in glob.glob(pat, recursive=True):
        for line in open(f):
            line = line.strip()
            if not line:
                continue
            r = json.loads(line)
            if r.get("status") != "ok" or not r.get("iterations"):
                continue
            if r.get("edit") == "gain2":        # legacy, non-clamping
                continue
            rows.append(r)
    return rows


def calibrate(rows):
    """-> (measured ms/fold per config, constraints per config, power-law fit)."""
    per = collections.defaultdict(list)
    cons = {}
    for r in rows:
        if r["iterations"] < MIN_FOLDS:
            continue
        k = (r["edit"], r["bit_depth"], r["batch_size"])
        per[k].append(r["recursive_prove_ms"] / r["iterations"])
        cons[k] = r["constraints_primary"]
    mpf = {k: st.median(v) for k, v in per.items()}

    xs = [math.log(cons[k]) for k in mpf]
    ys = [math.log(mpf[k]) for k in mpf]
    n = len(xs)
    mx, my = sum(xs) / n, sum(ys) / n
    b = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sum((x - mx) ** 2 for x in xs)
    a = math.exp(my - b * mx)
    return mpf, cons, (a, b)


def ms_per_fold(edit, bd, batch, mpf, cons, law):
    """Measured where known; power law in constraints otherwise."""
    k = (edit, bd, batch)
    if k in mpf:
        return mpf[k], "measured"
    # Same edit at another batch size -> scale its constraint count. Constraints
    # are very nearly affine in batch size at fixed depth, so this is a better
    # guess than the global law alone.
    same = [(kk, c) for kk, c in cons.items() if kk[0] == edit and kk[1] == bd]
    if same:
        kk, c = max(same, key=lambda t: t[1])
        est_c = c * batch / kk[2]
    else:
        est_c = None
    a, b = law
    if est_c:
        return a * est_c ** b, "extrapolated (circuit size)"
    return None, "unknown"


def predict(edit, bd, batch, n, mpf, cons, law):
    folds = n // (comp_size(bd) * batch)
    m, how = ms_per_fold(edit, bd, batch, mpf, cons, law)
    if m is None:
        return None
    return {"N": n, "folds": folds, "ms_per_fold": m,
            "seconds": folds * m / 1000.0, "basis": how,
            "exact_folds": n % (comp_size(bd) * batch) == 0}


def fmt_hms(s):
    if s < 90:
        return f"{s:.1f}s"
    if s < 5400:
        return f"{s/60:.1f}m"
    return f"{s/3600:.2f}h"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--edit", default="gain_k2")
    ap.add_argument("--bit-depth", type=int, default=8)
    ap.add_argument("--batch-size", type=int, default=100)
    ap.add_argument("--duration", type=float, help="seconds of audio")
    ap.add_argument("--channels", type=int, default=2)
    ap.add_argument("--sample-rate", type=int, default=44100)
    ap.add_argument("--n", type=int, help="total samples, instead of --duration")
    ap.add_argument("--matrix", action="store_true", help="print the matrix views")
    args = ap.parse_args(argv)

    rows = load()
    if not rows:
        print("no benchmark rows found", file=sys.stderr)
        return 1
    mpf, cons, law = calibrate(rows)

    if args.matrix:
        print_matrices(mpf, cons, law)
        return 0

    n = args.n
    if n is None:
        if args.duration is None:
            ap.error("pass --duration or --n")
        n = int(round(args.duration * args.channels * args.sample_rate))
    p = predict(args.edit, args.bit_depth, args.batch_size, n, mpf, cons, law)
    if p is None:
        print(f"no basis to predict {args.edit} at bd={args.bit_depth}", file=sys.stderr)
        return 1
    print(f"edit={args.edit}  bit_depth={args.bit_depth}  B={args.batch_size}")
    print(f"N = {n:,} samples"
          + (f"  ({args.duration}s x {args.channels}ch x {args.sample_rate}Hz)"
             if args.n is None else ""))
    print(f"folds        : {p['folds']:,}"
          + ("" if p["exact_folds"] else "   (N not a whole number of folds)"))
    print(f"ms per fold  : {p['ms_per_fold']:.1f}   [{p['basis']}]")
    print(f"PROVE TIME   : {fmt_hms(p['seconds'])}   ({p['seconds']:,.0f}s)")
    if p["basis"] != "measured":
        print("  note: circuit not benchmarked - power-law estimate, ~7% median "
              "error, up to ~28% worst case")
    return 0


def print_matrices(mpf, cons, law):
    """Matrix views: the model is a product, so show it as one."""
    edits = sorted({k[0] for k in mpf})
    bds = sorted({k[1] for k in mpf})
    batches = sorted({k[2] for k in mpf})

    print("=" * 78)
    print("MATRIX 1 - ms per fold (the only measured factor)")
    print("=" * 78)
    print("rows: edit @ bit_depth   cols: batch size   '·' = never run\n")
    print(f"{'edit':<12}{'bd':>4}  " + "".join(f"{('B='+str(b)):>12}" for b in batches))
    for e in edits:
        for bd in bds:
            if not any((e, bd, b) in mpf for b in batches):
                continue
            cells = "".join(
                (f"{mpf[(e,bd,b)]:>12.1f}" if (e, bd, b) in mpf else f"{'·':>12}")
                for b in batches)
            print(f"{e:<12}{bd:>4}  {cells}")

    print("\n" + "=" * 78)
    print("MATRIX 2 - folds, i.e. N decomposed  (EXACT arithmetic, no fitting)")
    print("=" * 78)
    print("folds = (channels x sample_rate x duration) / (comp_size(bd) x B)\n")
    durs = [10, 30, 60, 300, 600, 1800]
    for bd in bds:
        cs = comp_size(bd)
        print(f"  bit_depth {bd}  (comp_size {cs})   stereo 44.1kHz")
        print(f"    {'duration':>10} {'N':>14} " + "".join(f"{('B='+str(b)):>10}" for b in batches))
        for d in durs:
            n = int(d * 2 * 44100)
            print(f"    {str(d)+'s':>10} {n:>14,} "
                  + "".join(f"{n//(cs*b):>10,}" for b in batches))
        print()

    print("=" * 78)
    print("MATRIX 3 - predicted prove time, the product of the two above")
    print("=" * 78)
    print("edit=gain_k2, bit_depth=8, stereo 44.1kHz.  * = extrapolated beyond "
          "measured N\n")
    print(f"    {'duration':>10} {'N':>14} " + "".join(f"{('B='+str(b)):>11}" for b in batches))
    for d in durs:
        n = int(d * 2 * 44100)
        cells = ""
        for b in batches:
            p = predict("gain_k2", 8, b, n, mpf, cons, law)
            mark = "*" if n > 52_917_000 else " "
            cells += f"{fmt_hms(p['seconds'])+mark:>11}" if p else f"{'·':>11}"
        print(f"    {str(d)+'s':>10} {n:>14,} {cells}")

    print("\n" + "=" * 78)
    print("MATRIX 4 - N decomposed into its three factors")
    print("=" * 78)
    print("N = channels x sample_rate x duration.  Prove time is a function of N")
    print("alone, so any route to the same N costs the same (verified: 2.8% spread")
    print("across four routes, below the 7.1% rep noise).\n")
    print("gain_k2, bd=8, B=100, 60s of audio:\n")
    print(f"    {'channels':>9} {'sample rate':>12} {'N':>12} {'folds':>8} {'prove':>10}")
    for ch in (1, 2):
        for sr in (22050, 44100, 48000, 96000):
            n = int(60 * ch * sr)
            p = predict("gain_k2", 8, 100, n, mpf, cons, law)
            print(f"    {ch:>9} {sr:>12,} {n:>12,} {p['folds']:>8,} "
                  f"{fmt_hms(p['seconds']):>10}")


if __name__ == "__main__":
    sys.exit(main())
