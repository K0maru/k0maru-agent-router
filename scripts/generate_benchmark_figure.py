#!/usr/bin/env python3
"""
Scientific Performance & Evaluation Benchmark Figure for K0maru-Agent-Router.
Generated according to figures4papers publication guidelines.
Empirically tested on NVIDIA A100-SXM4-80GB (Google Colab High-RAM) & Apple Silicon MPS.
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

    fig, axes = plt.subplots(1, 3, figsize=(16.8, 5.2), constrained_layout=True)

    # -------------------------------------------------------------
    # Panel (a): Routing Decision Latency (Log Scale)
    # -------------------------------------------------------------
    ax = axes[0]
    latencies = [1.64, 1.53, 18.23, 883.02, 1850.00]
    methods = [
        "Direct CWD\n(Multiplexer)",
        "Rule Engine\n(Regex/Prefix)",
        "ModernBERT\n(A100 GPU)",
        "Qwen2.5-Coder\n(7B A100 GPU)",
        "Autoregressive\nLLM (Cloud API)"
    ]
    colors = [
        PALETTE["teal"],
        PALETTE["blue_secondary"],
        PALETTE["blue_main"],
        PALETTE["red_2"],
        PALETTE["red_strong"]
    ]

    bars = ax.bar(methods, latencies, color=colors, width=0.56, edgecolor="black", linewidth=1.1)
    ax.set_yscale("log")
    ax.set_ylabel("Decision Latency (ms, log scale)")
    ax.set_title("(a) Empirical Decision Latency (A100 GPU)", pad=12)
    ax.grid(axis="y", linestyle="--", alpha=0.35, which="both")
    ax.set_ylim(0.2, 5500)

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
        2, 75, "48.4x - 100x Faster\nvs Autoregressive",
        ha="center", va="bottom", fontsize=8.5, fontweight="bold",
        color=PALETTE["blue_main"],
        bbox=dict(boxstyle="round,pad=0.35", fc="#EBF3FB", ec=PALETTE["blue_secondary"], lw=1)
    )

    # -------------------------------------------------------------
    # Panel (b): Memory Footprint & Role Decoupling
    # -------------------------------------------------------------
    ax = axes[1]
    memory_mb = [11.2, 14.8, 616.7, 15857.5]
    roles = [
        "Worker Node\n(Dispatch Core)",
        "Cockpit Core\n(Static Router)",
        "Cockpit Node\n(+ ModernBERT)",
        "Qwen2.5-Coder\n(7B on A100)"
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
    ax.set_title("(b) Memory Footprint & Invariant Gate", pad=12)
    ax.grid(axis="y", linestyle="--", alpha=0.35, which="both")
    ax.set_ylim(1, 45000)

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
        elif val < 1000:
            txt = f"{val:.1f} MB"
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
        else:
            txt = f"{val / 1024:.1f} GB"
            ax.text(
                bar.get_x() + bar.get_width() / 2.0,
                height * 1.35,
                txt,
                ha="center",
                va="bottom",
                fontsize=9.0,
                fontweight="bold",
                color=PALETTE["red_strong"]
            )

    # -------------------------------------------------------------
    # Panel (c): Safety Gate & Receipt Verification Performance
    # -------------------------------------------------------------
    ax = axes[2]
    metrics = [
        "Qwen2.5-Coder\nRouting Acc",
        "HITL Outbox\nQuarantine Rate",
        "Exit Code\nHard Gate",
        "SHA-256 Digest\nVerify Rate"
    ]
    rates = [95.0, 100.0, 100.0, 100.0]
    metric_colors = [
        PALETTE["teal"],
        PALETTE["blue_main"],
        PALETTE["green_3"],
        PALETTE["blue_secondary"]
    ]

    bars3 = ax.bar(metrics, rates, color=metric_colors, width=0.54, edgecolor="black", linewidth=1.1, zorder=3)
    ax.set_ylabel("Reliability & Success Rate (%)")
    ax.set_title("(c) Safety Quarantine & Receipt Gate", pad=12)
    ax.set_ylim(0, 118)
    ax.grid(axis="y", linestyle="--", alpha=0.35, zorder=1)

    for bar, rate in zip(bars3, rates):
        height = bar.get_height()
        ax.text(
            bar.get_x() + bar.get_width() / 2.0,
            height + 2.5,
            f"{rate:.1f}%",
            ha="center",
            va="bottom",
            fontsize=9.5,
            fontweight="bold",
            color=PALETTE["dark_text"],
            zorder=5
        )

    ax.text(
        1.5, 30, "Receipt Verification Latency: 2.05 ms\nZero-Token Hard Objective Oracle",
        ha="center", va="center", fontsize=8.8, fontweight="bold",
        color=PALETTE["blue_main"],
        bbox=dict(boxstyle="round,pad=0.4", fc="#F0F7FF", ec=PALETTE["blue_secondary"], lw=1),
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
