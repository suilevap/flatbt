"""Charts from `flatbt-compare scaling` output.

    cargo run --release -- scaling > charts/scaling.tsv
    python3 charts/plot.py charts/scaling.tsv charts/

Writes scaling.png (time per agent-tick against population size, one panel per
scenario) and memory.png (bytes per agent). Needs matplotlib.
"""

import csv
import math
import sys
from collections import defaultdict

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from matplotlib.lines import Line2D  # noqa: E402
from matplotlib.ticker import FixedLocator, NullLocator  # noqa: E402

# Validated with the dataviz palette checker, all pairs, light surface: every
# pair clears the CVD and normal-vision floors; aqua and yellow sit below 3:1
# contrast, so every line also carries a marker shape and a direct label.
SURFACE = "#fcfcfb"
INK = "#0b0b0b"
INK_2 = "#52514e"
MUTED = "#898781"
GRID = "#e1e0d9"
AXIS = "#c3c2b7"
LIBS = [
    # name, color, marker
    ("flatbt", "#2a78d6", "o"),
    ("bhv", "#4a3aa7", "s"),
    ("behavior-tree-lite", "#1baf7a", "^"),
    ("bonsai-bt", "#eda100", "D"),
    ("behavior-tree", "#008300", "v"),
]
SCENARIOS = [
    ("select8", "select8 · 25 nodes"),
    ("patrol", "patrol · 5 nodes"),
    ("guard", "guard · 13 nodes"),
    ("soldier", "soldier · 68 nodes"),
    ("villager", "villager · 51 nodes, node catalog"),
]
AGENTS = [1, 10, 100, 1_000, 10_000, 100_000, 1_000_000]
AGENT_LABELS = ["1", "10", "100", "1k", "10k", "100k", "1M"]

plt.rcParams.update(
    {
        "font.family": "DejaVu Sans",
        "font.size": 9,
        "axes.edgecolor": AXIS,
        "axes.labelcolor": INK_2,
        "xtick.color": MUTED,
        "ytick.color": MUTED,
        "axes.titlecolor": INK,
        "figure.facecolor": SURFACE,
        "axes.facecolor": SURFACE,
        "savefig.facecolor": SURFACE,
    }
)


def load(path):
    data = defaultdict(dict)  # (scenario, lib) -> {agents: (ns, bytes)}
    with open(path) as f:
        for row in csv.DictReader(f, delimiter="\t"):
            data[(row["scenario"], row["lib"])][int(row["agents"])] = (
                float(row["ns_per_agent_tick"]),
                float(row["bytes_per_agent"]),
            )
    return data


def style(ax):
    for side in ("top", "right"):
        ax.spines[side].set_visible(False)
    ax.grid(True, which="major", color=GRID, linewidth=0.6)
    ax.set_axisbelow(True)
    ax.tick_params(length=0)


def declutter(ys, gap):
    """Spreads label positions (log10 units) at least `gap` apart."""
    order = sorted(range(len(ys)), key=lambda i: ys[i])
    placed = list(ys)
    for a, b in zip(order, order[1:]):
        if placed[b] - placed[a] < gap:
            placed[b] = placed[a] + gap
    return placed


def scaling_chart(data, out):
    fig, axes = plt.subplots(2, 3, figsize=(11, 7.2), dpi=150)
    axes = axes.ravel()
    y_ticks = [3, 10, 30, 100, 300, 1000, 3000]
    for ax, (scenario, title) in zip(axes, SCENARIOS):
        style(ax)
        ax.set_xscale("log")
        ax.set_yscale("log")
        ax.set_xlim(0.6, 5e6)
        ax.set_ylim(2.5, 6000)
        ax.xaxis.set_major_locator(FixedLocator(AGENTS))
        ax.xaxis.set_minor_locator(NullLocator())
        ax.set_xticklabels(AGENT_LABELS)
        ax.yaxis.set_major_locator(FixedLocator(y_ticks))
        ax.yaxis.set_minor_locator(NullLocator())
        ax.set_yticklabels([str(t) for t in y_ticks])
        ax.set_title(title, loc="left", fontsize=10, fontweight="bold", pad=8)
        ends = []
        for lib, color, marker in LIBS:
            points = data.get((scenario, lib))
            if not points:
                continue
            xs = sorted(points)
            ys = [points[x][0] for x in xs]
            ax.plot(
                xs,
                ys,
                color=color,
                linewidth=1.3,
                marker=marker,
                markersize=4.5,
                markeredgecolor=SURFACE,
                markeredgewidth=0.8,
                solid_capstyle="round",
                zorder=3 if lib == "flatbt" else 2,
            )
            ends.append((lib, xs[-1], ys[-1]))
        # Direct labels at each line's last point, spread so they never overlap.
        placed = declutter([math.log10(y) for _, _, y in ends], 0.19)
        for (lib, x, y), ly in zip(ends, placed):
            ax.annotate(
                f"{lib} {y:.0f}" if y >= 10 else f"{lib} {y:.1f}",
                xy=(x, y),
                xytext=(x * 1.35, 10**ly),
                fontsize=7,
                color=INK_2,
                va="center",
                arrowprops=dict(arrowstyle="-", color=AXIS, linewidth=0.5, shrinkA=0, shrinkB=2)
                if abs(ly - math.log10(y)) > 0.02
                else None,
            )
        ax.set_xlabel("agents", fontsize=8)
    for ax in axes[3::3]:
        ax.set_ylabel("ns per agent-tick (log)", fontsize=8)
    axes[0].set_ylabel("ns per agent-tick (log)", fontsize=8)

    # The sixth cell holds the legend and notes.
    legend = axes[5]
    legend.axis("off")
    handles = [
        Line2D([], [], color=c, marker=m, linewidth=1.3, markersize=5.5,
               markeredgecolor=SURFACE, markeredgewidth=0.8, label=lib)
        for lib, c, m in LIBS
    ]
    legend.legend(handles=handles, loc="upper left", frameon=False, fontsize=9,
                  labelcolor=INK, handlelength=2.4, borderaxespad=0.5)
    legend.text(
        0.02, 0.36,
        "Lower is better. Each agent owns its world\n"
        "state (264 bytes), included in every time.\n"
        "bonsai-bt and behavior-tree cannot express\n"
        "the villager's custom nodes.\n"
        "Populations whose trees would hold over\n"
        "3 GB are skipped.",
        transform=legend.transAxes, va="top", fontsize=8, color=INK_2, linespacing=1.5,
    )
    fig.suptitle("Time per agent-tick as the population grows", x=0.06, y=0.985, ha="left",
                 fontsize=13, fontweight="bold", color=INK)
    fig.text(0.06, 0.925,
             "FlatBT main 1818c17 · Rust 1.98.1 · 4-vCPU Xeon @ 2.10 GHz · median of 5 samples",
             fontsize=8.5, color=INK_2)
    fig.tight_layout(rect=(0.02, 0.0, 1.0, 0.91), h_pad=2.2, w_pad=1.5)
    fig.savefig(out)


def memory_chart(data, out):
    fig, ax = plt.subplots(figsize=(9, 3.9), dpi=150)
    style(ax)
    ax.grid(True, axis="x", color=GRID, linewidth=0.6)
    ax.grid(False, axis="y")
    ax.set_xscale("log")
    ticks = [1, 10, 100, 1_000, 10_000, 100_000]
    ax.xaxis.set_major_locator(FixedLocator(ticks))
    ax.xaxis.set_minor_locator(NullLocator())
    ax.set_xticklabels(["1 B", "10 B", "100 B", "1 KB", "10 KB", "100 KB"])
    ax.set_xlim(0.7, 1e5)
    rows = list(reversed(SCENARIOS))
    ax.set_yticks(range(len(rows)))
    ax.set_yticklabels([name for name, _ in rows], color=INK_2, fontsize=9)
    ax.set_ylim(-0.6, len(rows) - 0.4)
    offsets = {lib: (i - 2) * 0.09 for i, (lib, _, _) in enumerate(LIBS)}
    for row, (scenario, _) in enumerate(rows):
        for lib, color, marker in LIBS:
            points = data.get((scenario, lib))
            if not points:
                continue
            value = points[max(points)][1]
            ax.plot(value, row + offsets[lib], marker=marker, color=color, markersize=6.5,
                    markeredgecolor=SURFACE, markeredgewidth=0.8, linestyle="none")
            if lib == "flatbt":
                ax.annotate(f"{value:.0f} B", (value, row + offsets[lib]), xytext=(7, 0),
                            textcoords="offset points", fontsize=7.5, color=INK_2, va="center")
    handles = [Line2D([], [], color=c, marker=m, linestyle="none", markersize=6.5,
                      markeredgecolor=SURFACE, label=lib) for lib, c, m in LIBS]
    ax.legend(handles=handles, loc="upper center", bbox_to_anchor=(0.5, -0.13), ncol=5,
              frameon=False, fontsize=8.5, labelcolor=INK)
    ax.set_title("Memory each agent keeps for its tree (log scale, world state excluded)",
                 loc="left", fontsize=11, fontweight="bold", pad=10)
    fig.tight_layout()
    fig.savefig(out)


def main():
    source, out_dir = sys.argv[1], sys.argv[2].rstrip("/")
    data = load(source)
    scaling_chart(data, f"{out_dir}/scaling.png")
    memory_chart(data, f"{out_dir}/memory.png")


if __name__ == "__main__":
    main()
