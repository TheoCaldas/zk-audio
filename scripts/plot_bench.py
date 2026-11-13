#!/usr/bin/env python3
"""Plot benchmark scaling curves from `logs/bench/run-*/results.jsonl`.

Input format is **JSONL**, not CSV: `results.csv` drops every `peak_rss_*`
column, so it cannot answer the memory question at all. The JSONL rows are the
full `ProofMetrics` record, and `docs/benchmark_usage.md` already calls them
"the real data".

Produces four figures, each plotted against N (total audio samples):

  1. time_vs_n      — prove/verify wall-clock, the headline scaling curve
  2. memory_vs_n    — peak RSS, whole proof (plus per-phase breakdown)
  3. proof_size_vs_n— CompressedSNARK size in bytes
  4. us_per_sample  — prove-side microseconds per audio sample

Each figure's header carries the edit, bit depth and repeat count, as the
benchmark's own `summary.md` does.

Aggregation matches `src/bench/report.rs` exactly so these plots agree with
`summary.md`: the point is the **median** over repetitions, and the error bar
is the **half-range** (max-min)/2, not a standard deviation. `us_per_sample`
is recomputed as `recursive_prove_ms * 1000 / total_samples`, mirroring
`ProofMetrics::us_per_sample` in `src/circuit.rs`.

Only `status == "ok"` rows are plotted; `skipped` (n_too_small) and `failed`
(e.g. UnSat) rows carry no timings and are reported to stderr instead.

Rendering: uses matplotlib when the interpreter has it, otherwise falls back to
a dependency-free SVG writer, so the script works on a bare `python3`. The
default Homebrew python3.13 here has no matplotlib; `/usr/bin/python3` and
`python3.11` do.

Usage
-----
    # one run, all four figures, into logs/bench/run-*/plots/
    python3 scripts/plot_bench.py logs/bench/_baseline/run-1792898036

    # compare several runs on the same axes (one series per edit)
    python3 scripts/plot_bench.py logs/bench/_baseline/run-17928* --series edit

    # every run found, grouped by batch size
    python3 scripts/plot_bench.py --all --series batch_size -o /tmp/plots

    # just the memory figure, linear axes, also dump the aggregated table
    python3 scripts/plot_bench.py logs/bench/_baseline/run-1792898036 \
        --only memory --linear --print-table

When a multi-curve figure is grouped by a field that varies, only the headline
curve is drawn (otherwise 5 timing curves x 4 batch sizes = an unreadable
legend); pass --no-auto-focus for the full breakdown.
"""

import argparse
import glob
import json
import math
import os
import sys

# --------------------------------------------------------------------------
# metric definitions
# --------------------------------------------------------------------------
# Each metric: how to pull a value out of one ok-row, plus labelling. A metric
# returning None for a row means "not measured here" (older runs have no RSS).

MB = 1024.0 * 1024.0


def _rss(key):
    return lambda r: (r[key] / MB) if r.get(key) is not None else None


METRICS = {
    # 1. time x N
    "time": {
        "title": "Proving time vs audio length",
        "ylabel": "time (s)",
        "series": [
            ("RecursiveSNARK prove", lambda r: _ms_s(r.get("recursive_prove_ms"))),
            ("CompressedSNARK prove", lambda r: _ms_s(r.get("compressed_prove_ms"))),
            ("CompressedSNARK setup", lambda r: _ms_s(r.get("compressed_setup_ms"))),
            ("RecursiveSNARK verify", lambda r: _ms_s(r.get("recursive_verify_ms"))),
            ("CompressedSNARK verify", lambda r: _ms_s(r.get("compressed_verify_ms"))),
        ],
        "logy": True,
        "note": "compressed_prove excludes CompressedSNARK::setup (plotted separately)",
    },
    # 2. memory — whole proof first, that is the figure that matters
    "memory": {
        "title": "Peak memory vs audio length",
        "ylabel": "peak RSS (MB)",
        "series": [
            ("Whole proof (peak RSS)", _rss("peak_rss_bytes")),
            ("during folding", _rss("peak_rss_recursive_prove_bytes")),
            ("during compression", _rss("peak_rss_compressed_prove_bytes")),
            ("during public params", _rss("peak_rss_public_params_bytes")),
        ],
        "logy": False,
        "note": "process-wide, sampled every 50ms -> each figure is a LOWER BOUND on the true peak",
    },
    # 3. proof size
    "proof_size": {
        "title": "Proof size vs audio length",
        "ylabel": "proof size (bytes)",
        "series": [("CompressedSNARK", lambda r: _num(r.get("proof_size_bytes")))],
        "logy": False,
        # The values are near-constant by construction (succinctness), so a
        # 0-based axis collapses them onto one flat line. Fit the axis to the
        # data range instead -- but see `annotate_spread`: a zoomed axis can
        # make a trivial delta look dramatic, so the figure states the real
        # spread in bytes and percent.
        "zero_base": False,
        "annotate_spread": True,
        "note": "CompressedSNARK (Spartan+IPA-PC); succinct, so expected flat in N",
    },
    # 4. us per sample
    "us_per_sample": {
        "title": "Prove cost per audio sample",
        "ylabel": "µs per sample",
        "series": [("RecursiveSNARK prove", lambda r: _us_per_sample(r))],
        "logy": False,
        "note": "recursive_prove_ms * 1000 / total_samples (matches ProofMetrics::us_per_sample)",
    },
}

# The one series per metric that is the "focus" — drawn solid/emphasised, and
# the only one kept when --focus is passed. For memory this is the whole-proof
# peak, which is what the request asks to foreground.
FOCUS_INDEX = 0


def yrange(fig_data, ys_all):
    """(lo, hi) for a linear y axis.

    Zero-based by default, since for times and memory the distance from zero
    is meaningful. Metrics flagged `zero_base: False` instead fit the data
    range with 12% padding, so a near-constant series still shows its shape.
    """
    hi_v, lo_v = max(ys_all), min(ys_all)
    if fig_data["meta"].get("zero_base", True):
        return 0.0, (hi_v * 1.06 or 1.0)
    span = hi_v - lo_v
    if span <= 0:                      # every point identical: centre the line
        pad = abs(hi_v) * 0.02 or 1.0
    else:
        pad = span * 0.12
    return lo_v - pad, hi_v + pad


def spread_label(groups):
    """'spread: 80 B (0.26%)' across all plotted points, or None if flat."""
    vs = [p[1] for _, pts in groups for p in pts]
    if len(vs) < 2:
        return None
    lo, hi = min(vs), max(vs)
    if hi - lo <= 0:
        return "all points identical: %.0f B" % hi
    pct = (hi - lo) / lo * 100.0 if lo else 0.0
    return "spread: %.0f B (%.2f%% of min) \u2014 axis is zoomed, not 0-based" % (
        hi - lo, pct)


def _num(v):
    return float(v) if isinstance(v, (int, float)) else None


def _ms_s(v):
    return float(v) / 1000.0 if isinstance(v, (int, float)) else None


def _us_per_sample(r):
    n = r.get("total_samples") or 0
    ms = r.get("recursive_prove_ms")
    if not n or not isinstance(ms, (int, float)):
        return None
    return ms * 1000.0 / n


# --------------------------------------------------------------------------
# loading
# --------------------------------------------------------------------------


def load_run(run_dir):
    """Return (rows, manifest, skipped_notes) for one run directory."""
    jsonl = os.path.join(run_dir, "results.jsonl")
    if not os.path.isfile(jsonl):
        return [], {}, ["%s: no results.jsonl" % run_dir]

    manifest = {}
    mpath = os.path.join(run_dir, "manifest.json")
    if os.path.isfile(mpath):
        try:
            with open(mpath) as fh:
                manifest = json.load(fh)
        except (OSError, ValueError) as exc:
            manifest = {}
            print("warn: %s unreadable (%s)" % (mpath, exc), file=sys.stderr)

    rows, notes = [], []
    run = os.path.basename(os.path.normpath(run_dir))
    with open(jsonl) as fh:
        for lineno, line in enumerate(fh, 1):
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except ValueError as exc:
                notes.append("%s:%d malformed JSON (%s)" % (run, lineno, exc))
                continue
            status = rec.get("status")
            if status == "ok":
                rec["_run"] = run
                # Older schema rows predate `edit`; label them so they still group.
                rec.setdefault("edit", rec.get("circuit_name", "?"))
                rows.append(rec)
            elif status == "skipped":
                notes.append(
                    "%s: skipped %s N=%s (%s)"
                    % (run, rec.get("circuit_name"), rec.get("total_samples"),
                       rec.get("reason"))
                )
            else:
                notes.append(
                    "%s: FAILED %s N=%s rep=%s (%s)"
                    % (run, rec.get("circuit_name"), rec.get("total_samples"),
                       rec.get("repetition"), rec.get("error"))
                )
    return rows, manifest, notes


def discover(patterns, bench_root):
    """Expand CLI args into run directories."""
    dirs = []
    for pat in patterns:
        hits = sorted(glob.glob(pat))
        if not hits and os.path.isdir(pat):
            hits = [pat]
        for h in hits:
            if os.path.isdir(h):
                dirs.append(h)
            elif os.path.basename(h) in ("results.jsonl", "results.csv"):
                dirs.append(os.path.dirname(h))
    if not dirs and bench_root:
        # run dirs are nested under logs/bench/_baseline/ and
        # logs/bench/_experimental/ since the reorganisation, and some sit
        # one level deeper still (e.g. _baseline/rc/run-*).
        dirs = sorted(d for pat in ("run-*", "*/run-*", "*/*/run-*")
                      for d in glob.glob(os.path.join(bench_root, pat))
                      if os.path.isdir(d))
    # de-dup, keep order
    seen, out = set(), []
    for d in dirs:
        k = os.path.realpath(d)
        if k not in seen:
            seen.add(k)
            out.append(d)
    return out


# --------------------------------------------------------------------------
# aggregation (median + half-range, per src/bench/report.rs)
# --------------------------------------------------------------------------


def median(vals):
    s = sorted(vals)
    n = len(s)
    if n == 0:
        return None
    mid = n // 2
    return s[mid] if n % 2 else (s[mid - 1] + s[mid]) / 2.0


def aggregate(rows, getter, series_key):
    """-> {series_label: [(N, median, half_range, nreps), ...]} sorted by N."""
    buckets = {}
    for r in rows:
        v = getter(r)
        if v is None:
            continue
        n = r.get("total_samples")
        if not isinstance(n, (int, float)) or n <= 0:
            continue
        buckets.setdefault(series_key(r), {}).setdefault(float(n), []).append(float(v))

    out = {}
    for label, by_n in buckets.items():
        pts = []
        for n in sorted(by_n):
            vs = by_n[n]
            med = median(vs)
            half = (max(vs) - min(vs)) / 2.0 if len(vs) > 1 else 0.0
            pts.append((n, med, half, len(vs)))
        out[label] = pts
    return out


def series_keyfunc(name):
    if name == "none":
        return lambda r: ""
    if name == "auto":
        return None
    return lambda r: "%s=%s" % (name, r.get(name))


def pick_auto_series(rows):
    """Group by whichever dimension actually varies (keeps legends honest)."""
    for dim in ("edit", "batch_size", "bit_depth", "_run"):
        if len({str(r.get(dim)) for r in rows}) > 1:
            return dim
    return "none"


# --------------------------------------------------------------------------
# header / provenance
# --------------------------------------------------------------------------


def build_header(rows, manifests):
    """edit, bit_depth, repeats — the header the request asks for."""

    def uniq(key):
        return sorted({str(r.get(key)) for r in rows if r.get(key) is not None})

    def fmt(vals, label):
        if not vals:
            return None
        return "%s: %s" % (label, ", ".join(vals) if len(vals) <= 4
                           else "%d values" % len(vals))

    # repeats: trust the observed rep count per cell over manifest `repeat`,
    # since a run can be interrupted mid-sweep.
    cells = {}
    for r in rows:
        cells.setdefault(
            (r.get("edit"), r.get("bit_depth"), r.get("batch_size"),
             r.get("total_samples")), set()
        ).add(r.get("repetition"))
    reps = sorted({len(v) for v in cells.values()})
    declared = sorted({str(m.get("config", {}).get("repeat"))
                       for m in manifests if m.get("config", {}).get("repeat")})

    if not reps:
        rep_s = "repeats: -"
    elif len(reps) == 1:
        rep_s = "repeats: %d" % reps[0]
    else:
        rep_s = "repeats: %d-%d" % (reps[0], reps[-1])
    if declared and (len(reps) != 1 or str(reps[0]) not in declared):
        rep_s += " (declared %s)" % "/".join(declared)

    parts = [
        fmt(uniq("edit"), "edit"),
        fmt(uniq("bit_depth"), "bit_depth"),
        fmt(uniq("batch_size"), "B"),
        rep_s,
    ]
    line = "  |  ".join(p for p in parts if p)

    env = next((m.get("environment", {}) for m in manifests if m.get("environment")), {})
    sub = []
    if env.get("cpu"):
        sub.append(env["cpu"])
    if env.get("cores"):
        sub.append("%s cores" % env["cores"])
    if env.get("debug_build") is True:
        sub.append("DEBUG BUILD")
    runs = sorted({r["_run"] for r in rows})
    sub.append(runs[0] if len(runs) == 1 else "%d runs" % len(runs))
    return line, "  |  ".join(sub)


# --------------------------------------------------------------------------
# matplotlib backend
# --------------------------------------------------------------------------

PALETTE = ["#4269d0", "#efb118", "#ff725c", "#6cc5b0", "#3ca951",
           "#a463f2", "#97bbf5", "#9c6b4e"]


def plot_mpl(fig_data, outpath, logx, dpi):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from matplotlib.ticker import FuncFormatter

    meta = fig_data["meta"]
    groups = fig_data["groups"]  # [(label, pts)]

    fig, ax = plt.subplots(figsize=(9.0, 5.6))

    for i, (label, pts) in enumerate(groups):
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        es = [p[2] for p in pts]
        color = PALETTE[i % len(PALETTE)]
        focus = fig_data["focus_labels"] and label in fig_data["focus_labels"]
        ax.errorbar(
            xs, ys, yerr=es if any(es) else None,
            marker="o", markersize=5.5 if focus else 4.0,
            linewidth=2.4 if focus else 1.3,
            linestyle="-" if focus else "--",
            color=color, capsize=3, elinewidth=1.0,
            label=label, zorder=3 if focus else 2,
            alpha=1.0 if focus else 0.75,
        )

    ax.set_xscale("log" if logx else "linear")
    if meta["logy"]:
        ax.set_yscale("log")
    else:
        ys_all = [p[1] + p[2] for _, pts in groups for p in pts] + \
                 [p[1] - p[2] for _, pts in groups for p in pts]
        lo, hi = yrange(fig_data, ys_all)
        ax.set_ylim(bottom=lo, top=hi)

    ax.set_xlabel("N (total audio samples)")
    ax.set_ylabel(meta["ylabel"])
    ax.grid(True, which="major", alpha=0.25, linewidth=0.7)
    ax.grid(True, which="minor", alpha=0.10, linewidth=0.5)
    ax.set_axisbelow(True)

    def human(v, _pos=None):
        v = float(v)
        for div, suf in ((1e9, "G"), (1e6, "M"), (1e3, "k")):
            if abs(v) >= div:
                s = "%.1f" % (v / div)
                return s.rstrip("0").rstrip(".") + suf
        return "%g" % v

    ax.xaxis.set_major_formatter(FuncFormatter(human))
    if not meta["logy"]:
        # A zoomed axis can span less than one "k" step, where `human` would
        # print the same rounded label on every tick (30.9k, 30.9k, ...).
        # Fall back to plain integers whenever the span is too small for the
        # abbreviated form to distinguish the ticks.
        lo_t, hi_t = ax.get_ylim()
        if hi_t - lo_t >= 2000:
            ax.yaxis.set_major_formatter(FuncFormatter(human))
        else:
            ax.yaxis.set_major_formatter(
                FuncFormatter(lambda v, _p: ("%.0f" if abs(v) >= 100
                                             else "%g") % v))

    fig.suptitle(meta["title"], x=0.015, ha="left", fontsize=13.5,
                 fontweight="bold", y=0.985)
    legend_rows = 0
    if len(groups) > 1 or groups and groups[0][0]:
        ncol_probe = 4 if len(groups) > 6 else (3 if len(groups) > 2 else len(groups))
        legend_rows = -(-len(groups) // max(1, ncol_probe))
    ax.set_title(meta["header"] + "\n" + meta["subheader"],
                 loc="left", fontsize=8.8, color="#444",
                 pad=9 + 15 * legend_rows)

    if len(groups) > 1 or groups and groups[0][0]:
        # Outside the axes, under the header: `loc="best"` places the legend
        # inside the data area, where on a dense multi-series figure it sits
        # on top of the curves it is describing.
        ncol = 4 if len(groups) > 6 else (3 if len(groups) > 2 else len(groups))
        ax.legend(frameon=False, fontsize=8.6, ncol=max(1, ncol),
                  loc="lower left", bbox_to_anchor=(0, 1.005),
                  borderaxespad=0, columnspacing=1.6, handlelength=1.8)

    if meta.get("annotate_spread"):
        lbl = spread_label(groups)
        if lbl:
            # Figure coords, bottom-right: `loc="best"` can park the legend in
            # any corner of the axes, so an in-axes badge risks overlapping it.
            fig.text(0.985, 0.012, lbl, fontsize=8.2, color="#555",
                     ha="right", va="bottom")

    if meta.get("note"):
        fig.text(0.015, 0.012, "note: " + meta["note"], fontsize=7.4,
                 color="#666", ha="left")

    fig.tight_layout(rect=(0, 0.035, 1, 0.955))
    fig.savefig(outpath, dpi=dpi)
    plt.close(fig)


# --------------------------------------------------------------------------
# stdlib SVG fallback (no third-party deps)
# --------------------------------------------------------------------------


def _esc(s):
    return (str(s).replace("&", "&amp;").replace("<", "&lt;")
            .replace(">", "&gt;"))


def _ticks(lo, hi, want=6):
    if hi <= lo:
        hi = lo + 1.0
    raw = (hi - lo) / want
    mag = 10 ** math.floor(math.log10(raw)) if raw > 0 else 1.0
    for m in (1, 2, 2.5, 5, 10):
        if mag * m >= raw:
            step = mag * m
            break
    else:
        step = mag * 10
    t, out = math.floor(lo / step) * step, []
    while t <= hi + step * 0.5:
        if t >= lo - step * 0.001:
            out.append(t)
        t += step
    return out


def _human(v):
    v = float(v)
    for div, suf in ((1e9, "G"), (1e6, "M"), (1e3, "k")):
        if abs(v) >= div:
            return ("%.1f" % (v / div)).rstrip("0").rstrip(".") + suf
    if abs(v) < 1 and v != 0:
        return "%.3g" % v
    return "%g" % v


def plot_svg(fig_data, outpath, logx):
    meta, groups = fig_data["meta"], fig_data["groups"]
    # Reserve legend width from the actual longest label (~5.3px/char at 9.6px
    # font) instead of a fixed margin, so long series names are not clipped.
    longest = max((len(l or meta["ylabel"]) for l, _ in groups), default=10)
    R = max(150, min(430, int(longest * 5.3) + 62))
    H = 560
    W = 720 + R
    L, T, B = 86, 92, 64
    pw, ph = W - L - R, H - T - B

    xs_all = [p[0] for _, pts in groups for p in pts]
    # Do NOT clamp the lower bound to 0 here: on a zoomed (non-zero-based)
    # axis that would pull the range back down to zero and undo the zoom.
    ys_all = [p[1] + p[2] for _, pts in groups for p in pts] + \
             [p[1] - p[2] for _, pts in groups for p in pts]
    if not xs_all or not ys_all:
        return False

    logy = meta["logy"] and min(y for y in ys_all if y > 0) > 0

    def fx(v):
        if logx:
            lo, hi = math.log10(min(xs_all)), math.log10(max(xs_all))
            if hi == lo:
                return L + pw / 2.0
            return L + (math.log10(v) - lo) / (hi - lo) * pw
        lo, hi = min(xs_all), max(xs_all)
        if hi == lo:
            return L + pw / 2.0
        return L + (v - lo) / (hi - lo) * pw

    if logy:
        ylo = min(y for y in ys_all if y > 0) / 1.6
        yhi = max(ys_all) * 1.06 or 1.0
    else:
        ylo, yhi = yrange(fig_data, ys_all)

    def fy(v):
        if logy:
            lo, hi = math.log10(ylo), math.log10(yhi)
            v = max(v, ylo)
            return T + ph - (math.log10(v) - lo) / (hi - lo) * ph
        if yhi == ylo:
            return T + ph / 2.0
        return T + ph - (v - ylo) / (yhi - ylo) * ph

    o = ['<?xml version="1.0" encoding="UTF-8"?>',
         '<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" '
         'viewBox="0 0 %d %d" font-family="Helvetica,Arial,sans-serif">'
         % (W, H, W, H),
         '<rect width="%d" height="%d" fill="#ffffff"/>' % (W, H)]

    o.append('<text x="14" y="28" font-size="16" font-weight="bold" '
             'fill="#111">%s</text>' % _esc(meta["title"]))
    o.append('<text x="14" y="48" font-size="10.5" fill="#444">%s</text>'
             % _esc(meta["header"]))
    o.append('<text x="14" y="64" font-size="10" fill="#666">%s</text>'
             % _esc(meta["subheader"]))

    # y grid
    if logy:
        lo, hi = math.floor(math.log10(ylo)), math.ceil(math.log10(yhi))
        yt = [10 ** e for e in range(int(lo), int(hi) + 1)]
    else:
        yt = _ticks(ylo, yhi)
    for t in yt:
        if not (ylo - 1e-12 <= t <= yhi * 1.001):
            continue
        y = fy(t)
        o.append('<line x1="%d" y1="%.1f" x2="%.1f" y2="%.1f" stroke="#e6e6e6" '
                 'stroke-width="1"/>' % (L, y, L + pw, y))
        lab = _human(t) if (yhi - ylo) >= 2000 else (
            "%.0f" % t if abs(t) >= 100 else "%g" % t)
        o.append('<text x="%d" y="%.1f" font-size="9.5" fill="#555" '
                 'text-anchor="end">%s</text>' % (L - 8, y + 3.4, _esc(lab)))

    # x ticks: the measured N values, which are what the sweep chose
    for t in sorted(set(xs_all)):
        x = fx(t)
        o.append('<line x1="%.1f" y1="%d" x2="%.1f" y2="%.1f" stroke="#e6e6e6" '
                 'stroke-width="1"/>' % (x, T, x, T + ph))
        o.append('<text x="%.1f" y="%.1f" font-size="9.5" fill="#555" '
                 'text-anchor="middle">%s</text>' % (x, T + ph + 18, _esc(_human(t))))

    o.append('<rect x="%d" y="%d" width="%d" height="%d" fill="none" '
             'stroke="#bbb" stroke-width="1"/>' % (L, T, pw, ph))
    o.append('<text x="%.1f" y="%.1f" font-size="11" fill="#333" '
             'text-anchor="middle">N (total audio samples)</text>'
             % (L + pw / 2.0, H - 26))
    o.append('<text x="18" y="%.1f" font-size="11" fill="#333" '
             'text-anchor="middle" transform="rotate(-90 18 %.1f)">%s</text>'
             % (T + ph / 2.0, T + ph / 2.0, _esc(meta["ylabel"])))

    for i, (label, pts) in enumerate(groups):
        c = PALETTE[i % len(PALETTE)]
        focus = fig_data["focus_labels"] and label in fig_data["focus_labels"]
        wdt = 2.6 if focus else 1.4
        dash = "" if focus else ' stroke-dasharray="5,3"'
        d = " ".join("%s %.1f %.1f" % ("M" if j == 0 else "L", fx(p[0]), fy(p[1]))
                     for j, p in enumerate(pts))
        o.append('<path d="%s" fill="none" stroke="%s" stroke-width="%.1f"%s/>'
                 % (d, c, wdt, dash))
        for p in pts:
            x, y = fx(p[0]), fy(p[1])
            if p[2] > 0:
                o.append('<line x1="%.1f" y1="%.1f" x2="%.1f" y2="%.1f" '
                         'stroke="%s" stroke-width="1"/>'
                         % (x, fy(p[1] + p[2]), x, fy(max(ylo, p[1] - p[2])), c))
            o.append('<circle cx="%.1f" cy="%.1f" r="%.1f" fill="%s"/>'
                     % (x, y, 4.0 if focus else 3.0, c))
        ly = T + 6 + i * 17
        o.append('<line x1="%d" y1="%.1f" x2="%d" y2="%.1f" stroke="%s" '
                 'stroke-width="%.1f"%s/>'
                 % (L + pw + 14, ly, L + pw + 40, ly, c, wdt, dash))
        o.append('<circle cx="%.1f" cy="%.1f" r="3.2" fill="%s"/>'
                 % (L + pw + 27, ly, c))
        o.append('<text x="%d" y="%.1f" font-size="9.6" fill="#333">%s</text>'
                 % (L + pw + 46, ly + 3.4, _esc(label or meta["ylabel"])))

    if meta.get("annotate_spread"):
        lbl = spread_label(groups)
        if lbl:
            o.append('<text x="%.1f" y="%.1f" font-size="8.6" fill="#555" '
                     'text-anchor="end">%s</text>'
                     % (L + pw - 8, T + ph - 10, _esc(lbl)))

    if meta.get("note"):
        o.append('<text x="14" y="%d" font-size="8.6" fill="#777">note: %s</text>'
                 % (H - 8, _esc(meta["note"])))
    o.append("</svg>")

    with open(outpath, "w") as fh:
        fh.write("\n".join(o))
    return True


# --------------------------------------------------------------------------
# table dump
# --------------------------------------------------------------------------


def print_table(name, meta, groups):
    print("\n## %s — %s" % (name, meta["title"]))
    print("%-30s %12s %14s %12s %5s" % ("series", "N", "median", "±half", "reps"))
    for label, pts in groups:
        for n, med, half, k in pts:
            print("%-30s %12d %14.3f %12.3f %5d"
                  % ((label or meta["ylabel"])[:30], int(n), med, half, k))


# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Plot zk-audio benchmark scaling from results.jsonl",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="Input is results.jsonl (results.csv has no peak_rss_* columns).",
    )
    ap.add_argument("runs", nargs="*",
                    help="run dirs or globs, e.g. logs/bench/_baseline/run-1792898036")
    ap.add_argument("--all", action="store_true",
                    help="use every logs/bench/run-* directory")
    ap.add_argument("--bench-root", default=None,
                    help="logs/bench location (default: auto-detect from script path)")
    ap.add_argument("-o", "--outdir", default=None,
                    help="output dir (default: <run>/plots, or ./plots when "
                         "several runs are combined)")
    ap.add_argument("--only", action="append", choices=sorted(METRICS),
                    help="plot only these figures (repeatable)")
    ap.add_argument("--series", default="auto",
                    choices=["auto", "none", "edit", "batch_size", "bit_depth", "_run"],
                    help="group curves by this field (default: auto = whichever varies)")
    ap.add_argument("--focus", action="store_true",
                    help="drop the breakdown curves, keep only the headline one "
                         "(for memory: whole-proof peak RSS)")
    ap.add_argument("--no-auto-focus", action="store_true",
                    help="keep every breakdown curve even when grouping by a "
                         "field that varies (legend may get crowded)")
    ap.add_argument("--linear", action="store_true", help="linear x axis (default log)")
    ap.add_argument("--format", default="auto", choices=["auto", "png", "svg"],
                    help="auto = png via matplotlib if importable, else svg")
    ap.add_argument("--dpi", type=int, default=160)
    ap.add_argument("--print-table", action="store_true",
                    help="also print the aggregated medians to stdout")
    args = ap.parse_args(argv)

    here = os.path.dirname(os.path.abspath(__file__))
    bench_root = args.bench_root or os.path.join(os.path.dirname(here), "logs", "bench")

    run_dirs = discover(args.runs, bench_root if (args.all or not args.runs) else None)
    if not run_dirs:
        ap.error("no run directories found (looked in %s)" % bench_root)

    rows, manifests, notes = [], [], []
    for d in run_dirs:
        rr, mm, nn = load_run(d)
        rows.extend(rr)
        if mm:
            manifests.append(mm)
        notes.extend(nn)

    for n in notes:
        print("skip: " + n, file=sys.stderr)
    if not rows:
        print("error: no status=ok rows in %d run(s)" % len(run_dirs), file=sys.stderr)
        return 1

    # resolve series grouping
    sname = args.series
    if sname == "auto":
        sname = pick_auto_series(rows)
    keyf = series_keyfunc(sname)

    header, subheader = build_header(rows, manifests)
    nseries = len({keyf(r) for r in rows})

    # output dir
    if args.outdir:
        outdir = args.outdir
    elif len(run_dirs) == 1:
        outdir = os.path.join(run_dirs[0], "plots")
    else:
        outdir = os.path.join(os.getcwd(), "plots")
    os.makedirs(outdir, exist_ok=True)

    # backend
    use_mpl = False
    if args.format in ("auto", "png"):
        try:
            import matplotlib  # noqa: F401
            use_mpl = True
        except ImportError:
            if args.format == "png":
                print("error: --format png needs matplotlib; this interpreter "
                      "lacks it.\n  try: /usr/bin/python3 %s ... (has matplotlib)\n"
                      "  or:  --format svg (no dependencies)"
                      % os.path.relpath(__file__), file=sys.stderr)
                return 1
            print("note: matplotlib not importable -> writing SVG. For PNG try "
                  "/usr/bin/python3 or python3.11.", file=sys.stderr)
    ext = "png" if use_mpl else "svg"

    wanted = args.only or list(METRICS)
    written, empty = [], []

    for name in wanted:
        meta = dict(METRICS[name])
        defs = meta["series"]
        # A multi-series metric crossed with a grouping field explodes the
        # legend (5 timing curves x 4 batch sizes = 20 lines, unreadable), so
        # once the grouping field actually varies, keep only the headline
        # curve unless --no-auto-focus says otherwise.
        auto = (len(defs) > 1 and nseries > 1 and not args.no_auto_focus)
        if args.focus or auto:
            defs = [defs[FOCUS_INDEX]]
            if auto:
                meta["note"] = ("showing %s only (%d %s groups); "
                                "--no-auto-focus for the full breakdown"
                                % (defs[0][0], nseries, sname))

        # Build curves. With one metric-series, the legend is the grouping
        # field; with several, it is the metric-series (x grouping) pair.
        groups, focus_labels = [], set()
        multi = len(defs) > 1
        for si, (slabel, getter) in enumerate(defs):
            agg = aggregate(rows, getter, keyf)
            for gkey in sorted(agg):
                if multi and gkey:
                    label = "%s [%s]" % (slabel, gkey)
                elif multi:
                    label = slabel
                else:
                    label = gkey or slabel
                groups.append((label, agg[gkey]))
                if si == FOCUS_INDEX:
                    focus_labels.add(label)

        groups = [(l, p) for l, p in groups if p]
        if not groups:
            empty.append(name)
            continue

        meta["header"] = header
        meta["subheader"] = subheader
        fig = {"meta": meta, "groups": groups,
               "focus_labels": focus_labels if len(groups) > 1 else set()}

        out = os.path.join(outdir, "%s_vs_n.%s" % (name, ext))
        if use_mpl:
            plot_mpl(fig, out, logx=not args.linear, dpi=args.dpi)
        elif not plot_svg(fig, out, logx=not args.linear):
            empty.append(name)
            continue
        written.append(out)

        if args.print_table:
            print_table(name, meta, groups)

    if empty:
        print("note: no data for %s (older runs lack peak_rss_* fields)"
              % ", ".join(empty), file=sys.stderr)

    print("\n%s  |  %s" % (header, subheader))
    print("%d ok rows from %d run(s) -> %s"
          % (len(rows), len(run_dirs), os.path.relpath(outdir)))
    for w in written:
        print("  " + os.path.relpath(w))
    return 0 if written else 1


if __name__ == "__main__":
    sys.exit(main())
