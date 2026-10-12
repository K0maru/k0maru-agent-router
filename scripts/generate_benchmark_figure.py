#!/usr/bin/env python3
"""
Scientific Performance & Evaluation Benchmark Figure for K0maru-Agent-Router.
Generated according to figures4papers publication guidelines.
"""
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

# House style semantic palette
PALETTE = {
    "blue_main": "#0F4D92",
    "blue_secondary": "#3775BA",
    "green_1": "#DDF3DE",
    "green_2": "#AADCA9",
    "green_3": "#8BCF8B",
    "red_1": "#F6CFCB",
    "red_2": "#E9A6A1",
    "red_strong": "#B64342",
    "neutral": "#CFCECE",
    "neutral_dark": "#767676",
    "highlight": "#FFD700",
    "teal": "#42949E",
    "violet": "#9A4D8E",
    "dark_text": "#1A1A1A",
}

def apply_publication_style():
    plt.rcParams.update({
        "font.family": "sans-serif",
        "font.sans-serif": ["Arial", "Helvetica", "DejaVu Sans"],
        "font.size": 10.5,
        "axes.titlesize": 12.5,
        "axes.titleweight": "bold",
        "axes.labelsize": 10.5,
        "axes.labelweight": "medium",
        "xtick.labelsize": 9.5,
        "ytick.labelsize": 9.5,
        "axes.linewidth": 1.4,
        "axes.spines.top": False,
        "axes.spines.right": False,
        "svg.fonttype": "none",
        "figure.dpi": 300,
    })

def generate_benchmark_figure(output_dir: Path):
    output_dir.mkdir(parents=True, exist_ok=True)
    apply_publication_style()

    fig, axes = plt.subplots(1, 3, figsize=(16.5, 5.0), constrained_layout=True)

    # -------------------------------------------------------------
    # Panel (a): Routing Decision Latency (Log Scale)
    # -------------------------------------------------------------
    ax = axes[0]
    latencies = [0.85, 1.10, 18.40, 42.10, 1850.00]
    methods = [
        "Direct CWD\n(Multiplexer)",
        "Rule Engine\n(Regex/Prefix)",
        "Laya MPS\n(Apple Silicon)",
        "Laya CPU\n(8-Thread)",
        "Autoregressive\nLLM (API)"
    ]
    colors = [
        PALETTE["teal"],
        PALETTE["blue_secondary"],
        PALETTE["blue_main"],
        PALETTE["neutral_dark"],
        PALETTE["red_strong"]
    ]

    bars = ax.bar(methods, latencies, color=colors, width=0.56, edgecolor="black", linewidth=1.1)
    ax.set_yscale("log")
    ax.set_ylabel("Decision Latency (ms, log scale)")
    ax.set_title("(a) Decision Engine Latency", pad=12)
    ax.grid(axis="y", linestyle="--", alpha=0.35, which="both")
    ax.set_ylim(0.1, 5500)

    for bar, val in zip(bars, latencies):
        height = bar.get_height()
        if val < 10:
            txt = f"{val:.2f} ms"
        elif val < 100:
            txt = f"{val:.1f} ms"
        else:
            txt = f"{val:.0f} ms"
        ax.text(
            bar.get_x() + bar.get_width() / 2.0,
            height * 1.35,
            txt,
            ha="center",
            va="bottom",
            fontsize=9.0,
            fontweight="semibold",
            color=PALETTE["dark_text"]
        )

    ax.text(
        2, 75, "100x Faster\nvs Autoregressive",
        ha="center", va="bottom", fontsize=8.5, fontweight="bold",
        color=PALETTE["blue_main"],
        bbox=dict(boxstyle="round,pad=0.35", fc="#EBF3FB", ec=PALETTE["blue_secondary"], lw=1)
    )

    # -------------------------------------------------------------
    # Panel (b): Memory Footprint & Architecture Invariant
    # -------------------------------------------------------------
    ax = axes[1]
    memory_mb = [11.2, 14.8, 1150.0, 8500.0]
    roles = [
        "Worker Node\n(Dispatch Core)",
        "Cockpit Node\n(Router Core)",
        "Cockpit Node\n(+ Laya ModernBERT)",
        "Local 7B LLM\n(Autoregressive)"
    ]
    mem_colors = [
        PALETTE["green_3"],
        PALETTE["teal"],
        PALETTE["blue_main"],
        PALETTE["red_2"]
    ]

    bars2 = ax.bar(roles, memory_mb, color=mem_colors, width=0.54, edgecolor="black", linewidth=1.1)
    ax.set_yscale("log")
    ax.set_ylabel("Memory / VRAM Footprint (MB, log scale)")
    ax.set_title("(b) Memory Footprint & Role Decoupling", pad=12)
    ax.grid(axis="y", linestyle="--", alpha=0.35, which="both")
    ax.set_ylim(1, 35000)

    # Threshold horizontal line for Worker invariant (<15MB)
    ax.axhline(15, color="#2E7D32", linestyle="--", linewidth=1.8, alpha=0.95)
    ax.text(
        0.5, 24, "Worker <15MB Zero-Model Invariant",
        color="#1B5E20", fontsize=8.8, fontweight="bold", ha="center",
        bbox=dict(boxstyle="round,pad=0.25", fc="#E8F5E9", ec="#81C784", lw=0.9)
    )

    for i, (bar, val) in enumerate(zip(bars2, memory_mb)):
        height = bar.get_height()
        if val < 20:
            txt = f"{val:.1f} MB"
            ax.text(
                bar.get_x() + bar.get_width() / 2.0,
                3.8,
                txt,
                ha="center",
                va="center",
                fontsize=9.0,
                fontweight="bold",
                color="white"
            )
        else:
            txt = f"{val / 1024:.2f} GB"
            ax.text(
                bar.get_x() + bar.get_width() / 2.0,
                height * 1.35,
                txt,
                ha="center",
                va="bottom",
                fontsize=9.0,
                fontweight="semibold",
                color=PALETTE["dark_text"]
            )

    # -------------------------------------------------------------
    # Panel (c): Confidence Calibration & HITL Gating
    # -------------------------------------------------------------
    ax = axes[2]
    scenarios = [
        "UI Defect\n(TerraButton)",
        "Webhook Bug\n(Stripe HMAC)",
        "Ambiguous Chore\n(Refactor)",
        "Noise Task\n(Text anomaly)"
    ]
    confidences = [95.26, 91.87, 70.12, 32.40]
    outcomes = ["Act", "Act", "Quarantined", "Quarantined"]
    bar_colors = [
        PALETTE["blue_main"],
        PALETTE["blue_secondary"],
        PALETTE["red_strong"],
        PALETTE["red_strong"]
    ]

    # Shaded quarantine zone below 85%
    ax.axhspan(0, 85, color="#FDF1F0", alpha=0.75, zorder=0)

    bars3 = ax.bar(scenarios, confidences, color=bar_colors, width=0.54, edgecolor="black", linewidth=1.1, zorder=3)
    ax.set_ylabel("Decision Confidence Score (%)")
    ax.set_title("(c) Confidence Calibration & HITL Gate", pad=12)
    ax.set_ylim(0, 112)
    ax.grid(axis="y", linestyle="--", alpha=0.35, zorder=1)

    # Threshold horizontal line at 85%
    ax.axhline(85, color=PALETTE["red_strong"], linestyle="--", linewidth=1.8, zorder=4)
    ax.text(
        3.1, 86.5, "Action Gate (85.0%)",
        color=PALETTE["red_strong"], fontsize=8.8, fontweight="bold", ha="right", zorder=5,
        bbox=dict(boxstyle="round,pad=0.2", fc="#FFF0F0", ec=PALETTE["red_strong"], lw=0.8)
    )
    ax.text(
        3.0, 58, "HITL Outbox Quarantine Zone (<85%)",
        color="#8B2524", fontsize=8.8, fontweight="semibold", ha="center", zorder=5,
        bbox=dict(boxstyle="round,pad=0.3", fc="#FAECEB", ec="#D48B8A", lw=0.8)
    )

    for bar, conf, outcome in zip(bars3, confidences, outcomes):
        height = bar.get_height()
        ax.text(
            bar.get_x() + bar.get_width() / 2.0,
            height + 2.2,
            f"{conf:.1f}%\n[{outcome}]",
            ha="center",
            va="bottom",
            fontsize=8.5,
            fontweight="bold",
            color=PALETTE["dark_text"],
            zorder=5
        )

    png_path = output_dir / "benchmark_decision_performance.png"
    svg_path = output_dir / "benchmark_decision_performance.svg"
    pdf_path = output_dir / "benchmark_decision_performance.pdf"

    fig.savefig(png_path, dpi=300, bbox_inches="tight")
    fig.savefig(svg_path, bbox_inches="tight")
    fig.savefig(pdf_path, bbox_inches="tight")
    plt.close(fig)

    print(f"Generated benchmark figures successfully:\n- {png_path}\n- {svg_path}\n- {pdf_path}")

if __name__ == "__main__":
    generate_benchmark_figure(Path("assets"))
