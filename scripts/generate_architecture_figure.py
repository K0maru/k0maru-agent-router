#!/usr/bin/env python3
"""
Scientific Architecture Diagram for K0maru-Agent-Router.
Generated according to figures4papers publication styling (zero-mermaid, zero-ASCII).
"""
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.patches as patches

PALETTE = {
    "blue_main": "#0F4D92",
    "blue_secondary": "#3775BA",
    "green_bg": "#EDF7ED",
    "green_border": "#8BCF8B",
    "red_bg": "#FDF1F0",
    "red_border": "#B64342",
    "neutral_bg": "#F8F9FA",
    "neutral_border": "#CFCECE",
    "teal": "#42949E",
    "dark_text": "#1A1A1A",
    "white": "#FFFFFF",
}

def draw_box(ax, x, y, w, h, title, subtitle="", fc=PALETTE["white"], ec=PALETTE["blue_main"], lw=1.5, title_color=PALETTE["dark_text"]):
    box = patches.FancyBboxPatch(
        (x, y), w, h,
        boxstyle="round,pad=0.04,rounding_size=0.08",
        fc=fc, ec=ec, lw=lw, zorder=2
    )
    ax.add_patch(box)
    if subtitle:
        ax.text(x + w / 2, y + h * 0.62, title, ha="center", va="center", fontsize=10.5, fontweight="bold", color=title_color, zorder=3)
        ax.text(x + w / 2, y + h * 0.28, subtitle, ha="center", va="center", fontsize=8.5, color="#555555", zorder=3)
    else:
        ax.text(x + w / 2, y + h / 2, title, ha="center", va="center", fontsize=10.5, fontweight="bold", color=title_color, zorder=3)

def draw_arrow(ax, x1, y1, x2, y2, label="", label_pos=0.5, offset_y=0.03):
    arrow = patches.FancyArrowPatch(
        (x1, y1), (x2, y2),
        arrowstyle="-|>,head_width=5,head_length=7",
        connectionstyle="arc3,rad=0",
        fc=PALETTE["dark_text"], ec=PALETTE["dark_text"], lw=1.3, zorder=4
    )
    ax.add_patch(arrow)
    if label:
        mx = x1 + (x2 - x1) * label_pos
        my = y1 + (y2 - y1) * label_pos + offset_y
        ax.text(mx, my, label, ha="center", va="center", fontsize=8.5, fontweight="semibold", color="#333333", zorder=5,
                bbox=dict(boxstyle="round,pad=0.2", fc=PALETTE["white"], ec="#E0E0E0", lw=0.6))

def generate_architecture_figure(output_path: Path):
    fig, ax = plt.subplots(figsize=(14.5, 6.2), dpi=300)
    ax.set_xlim(-0.2, 14.2)
    ax.set_ylim(-0.2, 6.2)
    ax.axis("off")

    # 1. Consumer Workspace Group
    g1 = patches.FancyBboxPatch((0.2, 0.4), 3.2, 5.2, boxstyle="round,pad=0.1,rounding_size=0.15",
                                fc=PALETTE["neutral_bg"], ec=PALETTE["neutral_border"], lw=1.2, zorder=1)
    ax.add_patch(g1)
    ax.text(1.8, 5.25, "Consumer Workspace", ha="center", va="center", fontsize=11.5, fontweight="bold", color=PALETTE["blue_main"])

    draw_box(ax, 0.5, 3.2, 2.6, 1.4, "Active Coding Agent", "Worker / Consumer Subagent", fc=PALETTE["white"], ec=PALETTE["blue_secondary"])
    draw_box(ax, 0.5, 0.9, 2.6, 1.3, "Task Envelope", "agent-task-v1 contract", fc="#F0F4F8", ec=PALETTE["blue_main"])
    draw_arrow(ax, 1.8, 3.2, 1.8, 2.2, "encounter defect")

    # Arrow from Task Envelope to Router Core
    draw_arrow(ax, 3.1, 1.55, 4.4, 1.55, "CLI / MCP")

    # 2. Router Core Group
    g2 = patches.FancyBboxPatch((4.4, 0.4), 4.6, 5.2, boxstyle="round,pad=0.1,rounding_size=0.15",
                                fc="#F4F8FB", ec=PALETTE["blue_secondary"], lw=1.4, zorder=1)
    ax.add_patch(g2)
    ax.text(6.7, 5.25, "k0maru-agent-router Core", ha="center", va="center", fontsize=11.5, fontweight="bold", color=PALETTE["blue_main"])

    draw_box(ax, 4.8, 3.3, 3.8, 1.4, "Cascade Decision Engine", "Deterministic Rules ➔ Laya System-1", fc=PALETTE["white"], ec=PALETTE["blue_main"])
    draw_box(ax, 4.8, 0.9, 3.8, 1.4, "Boundary Policy & HITL Gate", "Anti-Corruption & Trust Evaluation", fc=PALETTE["white"], ec=PALETTE["blue_main"])
    draw_arrow(ax, 6.7, 3.3, 6.7, 2.3, "evaluate target")

    # Arrows from Router Core to Destination Groups
    draw_arrow(ax, 8.6, 4.0, 10.2, 4.2, "Trusted (≥85%)", label_pos=0.42, offset_y=0.22)
    draw_arrow(ax, 8.6, 1.4, 10.2, 1.4, "Untrusted (<85%)", label_pos=0.42, offset_y=0.22)

    # 3. Multiplexer Driver Group (Top Right)
    g3 = patches.FancyBboxPatch((10.1, 3.1), 3.8, 2.5, boxstyle="round,pad=0.1,rounding_size=0.15",
                                fc=PALETTE["green_bg"], ec=PALETTE["green_border"], lw=1.2, zorder=1)
    ax.add_patch(g3)
    ax.text(12.0, 5.25, "Multiplexer Driver (Herdr / Tmux)", ha="center", va="center", fontsize=10.5, fontweight="bold", color="#1B5E20")

    draw_box(ax, 10.4, 3.4, 3.2, 1.5, "Physical CWD Addresser", "HotSession (Idle) | ColdSpawn", fc=PALETTE["white"], ec=PALETTE["green_border"])

    # 4. HITL Outbox Gate Group (Bottom Right)
    g4 = patches.FancyBboxPatch((10.1, 0.4), 3.8, 2.4, boxstyle="round,pad=0.1,rounding_size=0.15",
                                fc=PALETTE["red_bg"], ec=PALETTE["red_border"], lw=1.2, zorder=1)
    ax.add_patch(g4)
    ax.text(12.0, 2.45, "HITL Outbox Quarantine", ha="center", va="center", fontsize=10.5, fontweight="bold", color=PALETTE["red_border"])

    draw_box(ax, 10.4, 0.7, 3.2, 1.4, "Human Authorization Gate", ".scratch/staged/ (inspect / approve)", fc=PALETTE["white"], ec=PALETTE["red_border"])

    output_path.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(output_path, bbox_inches="tight")
    svg_path = output_path.with_suffix(".svg")
    fig.savefig(svg_path, bbox_inches="tight")
    plt.close(fig)
    print(f"Generated architecture figure: {output_path} & {svg_path}")

if __name__ == "__main__":
    generate_architecture_figure(Path("assets/architecture_topology.png"))
