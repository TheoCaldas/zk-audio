#!/usr/bin/env python3
"""Summarise a fusion sweep: Option A (fused) against Option B (chain).

Reads the CSV `compare_chain --csv` writes, one row per side per case, and
prints the saving per N as a markdown table plus the fitted cost model.
Draws a figure when matplotlib is available.

The model each side obeys:

    total_prove = fixed + per_fold * folds

`fixed` is the setup and compression toll, paid once per proof, so a chain
of n stages pays it n times. `per_fold` tracks constraints per step. The
two together say where fusion's saving comes from at a given audio length,
which is the result the comparison exists to produce.

Usage:
    python3 scripts/analyze_fusion.py logs/fusion/fusion_b100_bd8.csv
    python3 scripts/analyze_fusion.py <csv> --plot logs/fusion/fusion.png
"""
import argparse
import collections
import csv
import os
import sys

SR = 44100
CHANNELS = 2


# `stage_constraints` was added to compare_chain mid-sweep, so a CSV can
# hold rows both with and without it under one header. Rows are parsed
# positionally against the widest known layout rather than by the header
# line, and the field is optional.
COLUMNS_15 = [
    "label", "option", "stages", "bit_depth", "batch_size",
    "requested_samples", "padded_samples", "folds", "proofs",
    "constraints_per_step", "public_params_ms", "fold_ms", "compress_ms",
    "total_prove_ms", "proof_size_bytes",
]
COLUMNS_16 = COLUMNS_15[:10] + ["stage_constraints"] + COLUMNS_15[10:]


def load(path):
    rows = []
    with open(path) as f:
        for parts in csv.reader(f):
            if not parts or parts[0] == "label":
                continue
            cols = COLUMNS_16 if len(parts) == len(COLUMNS_16) else COLUMNS_15
            if len(parts) != len(cols):
                continue
            rows.append(dict(zip(cols, parts)))
    for r in rows:
        for k in (
            "bit_depth",
            "batch_size",
            "requested_samples",
            "padded_samples",
            "folds",
            "proofs",
            "constraints_per_step",
            "proof_size_bytes",
        ):
            r[k] = int(r[k])
        for k in (
            "public_params_ms",
            "fold_ms",
            "compress_ms",
            "total_prove_ms",
        ):
            r[k] = float(r[k])
    return rows


def fit(points):
    """Least-squares fit of total = fixed + per_fold * folds."""
    n = len(points)
    if n < 2:
        return None, None
    sx = sum(f for f, _ in points)
    sy = sum(t for _, t in points)
    sxx = sum(f * f for f, _ in points)
    sxy = sum(f * t for f, t in points)
    denom = n * sxx - sx * sx
    if denom == 0:
        return None, None
    slope = (n * sxy - sx * sy) / denom
    intercept = (sy - slope * sx) / n
    return intercept, slope


def secs(n_samples):
    return n_samples / (SR * CHANNELS)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("csv")
    p.add_argument("--plot", help="write a figure to this path")
    args = p.parse_args()

    rows = load(args.csv)
    if not rows:
        sys.exit("no rows")

    # Group by chain spec, then by N, so each table covers one composite.
    by_spec = collections.defaultdict(lambda: collections.defaultdict(dict))
    for r in rows:
        by_spec[r["stages"]][r["padded_samples"]][r["option"]] = r

    for spec, by_n in sorted(by_spec.items()):
        stages = spec.split("+")
        bd = next(iter(by_n.values()))["B"]["bit_depth"]
        B = next(iter(by_n.values()))["B"]["batch_size"]

        print(f"## {' -> '.join(stages)}  (bd {bd}, B {B})")
        print()
        print(
            "| audio | N | folds | B total | A total | saving | B fold | A fold "
            "| B compress | A compress |"
        )
        print("|---|---|---|---|---|---|---|---|---|---|")

        for n in sorted(by_n):
            pair = by_n[n]
            if "A" not in pair or "B" not in pair:
                continue
            a, b = pair["A"], pair["B"]
            save = 100 * (1 - a["total_prove_ms"] / b["total_prove_ms"])
            print(
                f"| {secs(n):.2f}s | {n} | {b['folds']} "
                f"| {b['total_prove_ms'] / 1000:.1f}s "
                f"| {a['total_prove_ms'] / 1000:.1f}s "
                f"| **{save:.1f}%** "
                f"| {b['fold_ms'] / 1000:.1f}s | {a['fold_ms'] / 1000:.1f}s "
                f"| {b['compress_ms'] / 1000:.1f}s | {a['compress_ms'] / 1000:.1f}s |"
            )
        print()

        # Constraints are a property of the circuit, not of N.
        any_pair = next(iter(by_n.values()))
        if "A" in any_pair and "B" in any_pair:
            cb = any_pair["B"]["constraints_per_step"]
            ca = any_pair["A"]["constraints_per_step"]
            print(
                f"Constraints/step: chain {cb:,} (sum of {any_pair['B']['proofs']} "
                f"stages), fused {ca:,} - {100 * (1 - ca / cb):.1f}%."
            )
            print()

        for opt, name in (("B", "chain"), ("A", "fused")):
            pts = [
                (r[opt]["folds"], r[opt]["total_prove_ms"])
                for r in by_n.values()
                if opt in r
            ]
            fixed, per_fold = fit(pts)
            if fixed is not None:
                print(
                    f"{name:6} total_prove_ms ~ {fixed / 1000:.1f}s "
                    f"+ {per_fold:.1f} ms/fold"
                )
        print()

        # Spartan/IPA pads the constraint count to the next power of two,
        # so the compression toll tracks the PADDED size, not the raw
        # count. That makes fusion's compression saving the ratio of
        # padded sizes: fusing two circuits that each sit just under a
        # boundary into one that crosses it buys nothing here.
        if "A" in any_pair and "B" in any_pair:
            import math

            ca = any_pair["A"]["constraints_per_step"]
            ea = math.ceil(math.log2(ca))
            pa = 1 << ea
            print(
                f"Fused pads to 2^{ea} = {pa:,} ({100 * (pa - ca) / pa:.0f}% slack)."
            )

            # With the per-stage counts, the chain's padded total is the
            # sum of each stage's own power-of-two, which is what its
            # compression actually costs.
            per_stage = any_pair["B"].get("stage_constraints")
            if per_stage:
                stage_counts = [int(x) for x in per_stage.split()]
                padded_chain = sum(1 << math.ceil(math.log2(c)) for c in stage_counts)
                exps = "+".join(f"2^{math.ceil(math.log2(c))}" for c in stage_counts)
                predicted = 100 * (1 - pa / padded_chain)
                measured = 100 * (
                    1
                    - sum(r["A"]["compress_ms"] for r in by_n.values() if "A" in r)
                    / sum(r["B"]["compress_ms"] for r in by_n.values() if "B" in r)
                )
                print(
                    f"Chain pads to {exps} = {padded_chain:,}. Compression cost "
                    f"tracks padded size, so the predicted compression saving is "
                    f"{predicted:.0f}%; measured {measured:.0f}%."
                )
                if padded_chain == pa:
                    print(
                        "  Here fusion crosses a power-of-two boundary and the two "
                        "padded totals are equal, so compression saves nothing by "
                        "construction - the whole saving comes from folding."
                    )
            print()

        # Compression is a property of the circuit, so its cost should not
        # vary with N. A run where it does has been perturbed by something
        # outside the measurement - on a laptop, usually thermal
        # throttling - and the affected points' SAVINGS stay meaningful
        # (both sides are hit together) while their absolute times do not.
        for opt, name in (("B", "chain"), ("A", "fused")):
            cs = [
                (r[opt]["folds"], r[opt]["compress_ms"])
                for r in by_n.values()
                if opt in r
            ]
            if len(cs) >= 3:
                vals = [c for _, c in cs]
                lo, hi = min(vals), max(vals)
                if lo > 0 and hi / lo > 1.15:
                    worst = max(cs, key=lambda t: t[1])
                    print(
                        f"WARNING {name}: compression ms spans {lo:.0f}-{hi:.0f} "
                        f"({100 * (hi / lo - 1):.0f}%) although the circuit is fixed; "
                        f"worst at {worst[0]} folds. Treat absolute times from the "
                        "affected points as throttled, not as a size effect."
                    )
        print()

        # The crossover question: fixed cost favours A on short audio, the
        # per-fold term carries the saving on long audio. Report both, so a
        # claim that the saving decays can be checked rather than assumed.
        pts_a = [(r["A"]["folds"], r["A"]["total_prove_ms"]) for r in by_n.values() if "A" in r]
        pts_b = [(r["B"]["folds"], r["B"]["total_prove_ms"]) for r in by_n.values() if "B" in r]
        fa, pa = fit(pts_a)
        fb, pb = fit(pts_b)
        if fa is not None and fb is not None and pb:
            print(
                f"Least-squares slope ratio: {100 * (1 - pa / pb):.1f}%. Fitted "
                f"fixed-cost saving at 1 fold: "
                f"{100 * (1 - (fa + pa) / (fb + pb)):.1f}%."
            )

        # Per-fold cost is not constant: Nova's fixed per-step overhead
        # amortises, so ms/fold falls as folds grow (at B=100 it drops
        # about 15% from 3 folds to 570). A straight-line fit through that
        # curvature overstates the asymptotic slope, so the asymptote is
        # read off the LARGEST case's folding ratio, which is measured
        # rather than extrapolated. Folding is the bulk of the total there,
        # so this is also the limit the total saving approaches.
        biggest = max(by_n)
        pair = by_n[biggest]
        if "A" in pair and "B" in pair and pair["B"]["fold_ms"] > 0:
            fold_ratio = 100 * (1 - pair["A"]["fold_ms"] / pair["B"]["fold_ms"])
            share = 100 * pair["B"]["fold_ms"] / pair["B"]["total_prove_ms"]
            print(
                f"Measured at the largest case ({pair['B']['folds']} folds): folding "
                f"saving {fold_ratio:.1f}%, and folding is {share:.0f}% of the chain's "
                f"total - so the asymptotic total saving is ~{fold_ratio:.0f}%."
            )
        print()

    if args.plot:
        plot(by_spec, args.plot)


def plot(by_spec, out):
    try:
        import matplotlib

        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
    except ImportError:
        print("matplotlib unavailable; skipping the figure", file=sys.stderr)
        return

    specs = sorted(by_spec)
    fig, axes = plt.subplots(2, len(specs), figsize=(7 * len(specs), 9), squeeze=False)

    for col, spec in enumerate(specs):
        by_n = by_spec[spec]
        ns = sorted(n for n in by_n if "A" in by_n[n] and "B" in by_n[n])
        if not ns:
            continue
        x = [secs(n) for n in ns]
        tb = [by_n[n]["B"]["total_prove_ms"] / 1000 for n in ns]
        ta = [by_n[n]["A"]["total_prove_ms"] / 1000 for n in ns]
        save = [100 * (1 - by_n[n]["A"]["total_prove_ms"] / by_n[n]["B"]["total_prove_ms"]) for n in ns]

        ax = axes[0][col]
        ax.plot(x, tb, "o-", label="B: sequential chain")
        ax.plot(x, ta, "s-", label="A: fused")
        ax.set_xscale("log")
        ax.set_yscale("log")
        ax.set_xlabel("audio seconds (44.1 kHz stereo)")
        ax.set_ylabel("total prove time (s)")
        ax.set_title(" -> ".join(spec.split("+")))
        ax.grid(True, which="both", alpha=0.3)
        ax.legend()

        ax = axes[1][col]
        ax.plot(x, save, "d-", color="tab:green")
        ax.set_xscale("log")
        ax.set_ylim(0, 100)
        ax.set_xlabel("audio seconds")
        ax.set_ylabel("prove-time saving (%)")
        ax.grid(True, which="both", alpha=0.3)
        ax.axhline(50, ls="--", c="gray", lw=0.8)

    fig.tight_layout()
    os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
    fig.savefig(out, dpi=150)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
