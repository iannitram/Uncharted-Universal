#!/usr/bin/env python3
"""Measure the Uncharted 3 readback cost across the 4-way A/B matrix (Issue #191).

Matrix cells:
  - off_off   (off/off): BB_TINY_READBACK=0, BB_READBACK_SMALL_BUFFERS=0
  - tiny_only (tiny=1):  BB_TINY_READBACK=1, BB_READBACK_SMALL_BUFFERS=0
  - small_only(small=1): BB_TINY_READBACK=0, BB_READBACK_SMALL_BUFFERS=1
  - both_on   (both=1):  BB_TINY_READBACK=1, BB_READBACK_SMALL_BUFFERS=1

Supports two workflows:
  1. `run`: execute a launch command across all 4 matrix cells with BB_BENCHMARK /
     BB_FRAME_STATS=1 / BB_VRAM_LOG=1, archive each cell's benchmark.json + run.log,
     and generate JSON + Markdown comparison reports.
  2. `report`: parse existing per-cell benchmark.json and/or run.log directories (or
     prior report JSONs) for one or more devices (e.g. Strix Halo RADV + AMD Handheld)
     and output combined Markdown + JSON comparison tables.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import sys
from typing import Any


@dataclass(frozen=True)
class MatrixCell:
    cell_id: str
    label: str
    tiny_readback: str
    readback_small_buffers: str


MATRIX: tuple[MatrixCell, ...] = (
    MatrixCell("off_off", "off/off", "0", "0"),
    MatrixCell("tiny_only", "tiny=1", "1", "0"),
    MatrixCell("small_only", "small=1", "0", "1"),
    MatrixCell("both_on", "both=1", "1", "1"),
)

CELLS_BY_ID: dict[str, MatrixCell] = {cell.cell_id: cell for cell in MATRIX}

DEVICE_PRESETS: dict[str, str] = {
    "strix-halo": "Strix Halo (RADV)",
    "handheld": "AMD Handheld",
    "desktop": "Desktop",
}

_GPU_RE = re.compile(
    r"^GPU:\s*(?P<name>.+?),\s*(?P<vram_mib>\d+)\s+MiB\s+VRAM,\s*driver\s+(?P<driver>.+)$"
)
_FRAME_STATS_RE = re.compile(
    r"^Frame stats:\s*(?P<fps>[0-9.]+)\s+FPS,\s*worst frame\s*(?P<worst_ms>[0-9.]+)\s+ms\s*"
    r"\(vblank\s+\d+\s+Hz\);\s*\d+\s+shader/pipeline compiles,\s*[0-9.]+\s+ms;\s*"
    r"\d+\s+recorder syncs;\s*"
    r"(?P<write_faults>[0-9.]+)\s+write faults/s\s*"
    r"\([^)]*?(?P<readbacks>[0-9.]+)/s\s+read back,\s*"
    r"(?P<side_queue>[0-9.]+)/s\s+on the side queue\s*"
    r"\((?P<fault_thread>[0-9.]+)/s\s+on the faulting thread\)[^)]*\),\s*"
    r"-?\d+\s+hot pages;\s*"
    r"GPU thread\s*(?P<us_per_draw>[0-9.]+)\s+us/draw,\s*"
    r"(?P<draws_per_frame>[0-9.]+)\s+draws/frame,\s*"
    r"idle\s*(?P<idle_pct>[0-9.]+)%;\s*"
    r"blocked:\s*recorder\s*[0-9.]+%,\s*host copies\s*(?P<host_copies_pct>[0-9.]+)%"
)
_FRAME_PACING_RE = re.compile(
    r"^Frame pacing:\s*median\s*(?P<p50_ms>[0-9.]+)\s+ms,\s*"
    r"stddev\s*(?P<stddev_ms>[0-9.]+)\s+ms,\s*"
    r"p99\s*(?P<p99_ms>[0-9.]+)\s+ms,\s*"
    r"(?P<spikes>\d+)\s+frames over 1\.5x median"
)
_QUEUE_WAITS_RE = re.compile(
    r"^Queue waits:\s*graphics\s*(?P<gfx_pct>[0-9.]+)%"
)
_STALL_RE = re.compile(r"^Stall:\s*(?P<ms>[0-9.]+)\s+ms frame;")
_VRAM_RE = re.compile(
    r"^VRAM[^:]*:\s*.*?process\s+(?P<process_mib>\d+)\s+MiB,\s*"
    r"driver budget\s+(?P<budget_mib>\d+)\s+of\s+(?P<total_mib>\d+)\s+MiB;\s*"
    r"VMA blocks\s+(?P<vma_blocks_mib>\d+)\s+MiB"
)


def device_label_for(device_id: str, explicit_label: str | None = None) -> str:
    if explicit_label:
        return explicit_label
    return DEVICE_PRESETS.get(device_id, device_id)


def build_cell_env(
    cell: MatrixCell,
    base_env: dict[str, str] | None = None,
    *,
    benchmark_seconds: int = 60,
    frame_stats: bool = True,
    vram_log: bool = True,
    user_dir: Path | str | None = None,
) -> dict[str, str]:
    """Assemble the child process environment for a single matrix cell."""
    env = dict(base_env) if base_env is not None else {}
    env["BB_TINY_READBACK"] = cell.tiny_readback
    env["BB_READBACK_SMALL_BUFFERS"] = cell.readback_small_buffers
    if benchmark_seconds > 0:
        env["BB_BENCHMARK"] = str(benchmark_seconds)
    else:
        env.pop("BB_BENCHMARK", None)
    if frame_stats:
        env["BB_FRAME_STATS"] = "1"
    if vram_log:
        env["BB_VRAM_LOG"] = "1"
    if user_dir is not None:
        user_str = str(user_dir)
        env["BB_USER_DIR"] = user_str
        env["BB_GPU_USER_DIR"] = user_str
    return env


def parse_benchmark_json(text: str) -> dict[str, Any]:
    """Parse one `benchmark.json` payload produced by `crates/runtime/src/benchmark.rs`."""
    data = json.loads(text)
    if not isinstance(data, dict):
        raise ValueError("benchmark.json root must be an object")
    frame_ms = data.get("frame_ms")
    if not isinstance(frame_ms, dict):
        raise ValueError("benchmark.json missing frame_ms object")
    for key in ("p50", "p95", "p99"):
        if not isinstance(frame_ms.get(key), (int, float)):
            raise ValueError(f"benchmark.json missing numeric frame_ms.{key}")
    if not isinstance(data.get("avg_fps"), (int, float)):
        raise ValueError("benchmark.json missing numeric avg_fps")
    if not isinstance(data.get("vram_peak_mib"), (int, float)):
        raise ValueError("benchmark.json missing numeric vram_peak_mib")
    if not isinstance(data.get("stalls_over_40ms"), (int, float)):
        raise ValueError("benchmark.json missing numeric stalls_over_40ms")
    return {
        "game": data.get("game"),
        "seconds": data.get("seconds"),
        "settings": data.get("settings") if isinstance(data.get("settings"), dict) else {},
        "avg_fps": float(data["avg_fps"]),
        "p50_ms": float(frame_ms["p50"]),
        "p95_ms": float(frame_ms["p95"]),
        "p99_ms": float(frame_ms["p99"]),
        "vram_peak_mib": int(data["vram_peak_mib"]),
        "stalls_over_40ms": int(data["stalls_over_40ms"]),
        "ended": str(data.get("ended", "unknown")),
    }


def parse_run_log(text: str) -> dict[str, Any]:
    """Parse `BB_FRAME_STATS=1`, `BB_VRAM_LOG=1`, and GPU header lines from a runtime log."""
    gpu_name: str | None = None
    gpu_total_vram_mib: int | None = None
    fps_samples: list[float] = []
    worst_ms_samples: list[float] = []
    p50_samples: list[float] = []
    p99_samples: list[float] = []
    stddev_samples: list[float] = []
    us_per_draw_samples: list[float] = []
    readbacks_samples: list[float] = []
    side_queue_samples: list[float] = []
    fault_thread_samples: list[float] = []
    host_copies_pct_samples: list[float] = []
    gfx_wait_pct_samples: list[float] = []
    vram_process_mib: list[int] = []
    vma_blocks_mib: list[int] = []
    stall_ms_list: list[float] = []

    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if gpu_name is None:
            m = _GPU_RE.match(line)
            if m:
                gpu_name = m.group("name").strip()
                gpu_total_vram_mib = int(m.group("vram_mib"))
                continue
        m = _FRAME_STATS_RE.match(line)
        if m:
            fps_samples.append(float(m.group("fps")))
            worst_ms_samples.append(float(m.group("worst_ms")))
            readbacks_samples.append(float(m.group("readbacks")))
            side_queue_samples.append(float(m.group("side_queue")))
            fault_thread_samples.append(float(m.group("fault_thread")))
            us_per_draw_samples.append(float(m.group("us_per_draw")))
            host_copies_pct_samples.append(float(m.group("host_copies_pct")))
            continue
        m = _FRAME_PACING_RE.match(line)
        if m:
            p50_samples.append(float(m.group("p50_ms")))
            stddev_samples.append(float(m.group("stddev_ms")))
            p99_samples.append(float(m.group("p99_ms")))
            continue
        m = _QUEUE_WAITS_RE.match(line)
        if m:
            gfx_wait_pct_samples.append(float(m.group("gfx_pct")))
            continue
        m = _STALL_RE.match(line)
        if m:
            stall_ms_list.append(float(m.group("ms")))
            continue
        m = _VRAM_RE.match(line)
        if m:
            vram_process_mib.append(int(m.group("process_mib")))
            vma_blocks_mib.append(int(m.group("vma_blocks_mib")))

    def mean_or_none(values: list[float]) -> float | None:
        return round(statistics.fmean(values), 3) if values else None

    has_stats = bool(fps_samples or p50_samples or p99_samples or stall_ms_list)
    return {
        "gpu": gpu_name,
        "gpu_total_vram_mib": gpu_total_vram_mib,
        "frame_stats_windows": len(fps_samples),
        "avg_fps": mean_or_none(fps_samples),
        "p50_ms": mean_or_none(p50_samples),
        "p95_ms": None,  # Frame pacing logs report median (p50) and p99, not p95.
        "p99_ms": mean_or_none(p99_samples),
        "stddev_ms": mean_or_none(stddev_samples),
        "worst_frame_ms": max(worst_ms_samples) if worst_ms_samples else (max(stall_ms_list) if stall_ms_list else None),
        "vram_peak_mib": max(vram_process_mib) if vram_process_mib else None,
        "vma_blocks_peak_mib": max(vma_blocks_mib) if vma_blocks_mib else None,
        "stalls_over_40ms": len(stall_ms_list) if has_stats else None,
        "gpu_us_per_draw": mean_or_none(us_per_draw_samples),
        "readbacks_per_s": mean_or_none(readbacks_samples),
        "side_queue_per_s": mean_or_none(side_queue_samples),
        "fault_thread_per_s": mean_or_none(fault_thread_samples),
        "host_copies_pct": mean_or_none(host_copies_pct_samples),
        "gfx_wait_pct": mean_or_none(gfx_wait_pct_samples),
    }


def combine_cell_metrics(
    cell: MatrixCell,
    benchmark_data: dict[str, Any] | None = None,
    log_data: dict[str, Any] | None = None,
    *,
    exit_code: int | None = None,
) -> dict[str, Any]:
    """Merge benchmark.json (authoritative) and run.log (diagnostics + fallback) for one cell."""
    has_bench = benchmark_data is not None
    has_log = log_data is not None and any(
        log_data.get(k) is not None
        for k in ("avg_fps", "p50_ms", "p99_ms", "vram_peak_mib", "gpu_us_per_draw")
    )
    if has_bench and has_log:
        source = "benchmark+log"
    elif has_bench:
        source = "benchmark"
    elif has_log:
        source = "log"
    else:
        source = "missing"

    b = benchmark_data or {}
    l = log_data or {}

    ended = b.get("ended")
    if ended is None:
        if source == "missing":
            ended = "missing"
        elif exit_code is not None and exit_code != 0:
            ended = f"exit:{exit_code}"
        else:
            ended = "log-only"

    return {
        "cell_id": cell.cell_id,
        "label": cell.label,
        "tiny_readback": cell.tiny_readback,
        "readback_small_buffers": cell.readback_small_buffers,
        "source": source,
        "ended": ended,
        "exit_code": exit_code,
        "seconds": b.get("seconds"),
        "avg_fps": b.get("avg_fps") if b.get("avg_fps") is not None else l.get("avg_fps"),
        "p50_ms": b.get("p50_ms") if b.get("p50_ms") is not None else l.get("p50_ms"),
        "p95_ms": b.get("p95_ms"),
        "p99_ms": b.get("p99_ms") if b.get("p99_ms") is not None else l.get("p99_ms"),
        "worst_frame_ms": l.get("worst_frame_ms"),
        "vram_peak_mib": (
            b.get("vram_peak_mib")
            if b.get("vram_peak_mib") is not None
            else l.get("vram_peak_mib")
        ),
        "stalls_over_40ms": (
            b.get("stalls_over_40ms")
            if b.get("stalls_over_40ms") is not None
            else l.get("stalls_over_40ms")
        ),
        "gpu": l.get("gpu"),
        "gpu_us_per_draw": l.get("gpu_us_per_draw"),
        "readbacks_per_s": l.get("readbacks_per_s"),
        "side_queue_per_s": l.get("side_queue_per_s"),
        "fault_thread_per_s": l.get("fault_thread_per_s"),
        "host_copies_pct": l.get("host_copies_pct"),
        "gfx_wait_pct": l.get("gfx_wait_pct"),
        "settings": b.get("settings", {}),
    }


def compute_deltas(
    cells_by_id: dict[str, dict[str, Any]],
    baseline_id: str = "off_off",
) -> None:
    """Annotate each cell dict in place with deltas relative to `baseline_id`."""
    base = cells_by_id.get(baseline_id)
    base_fps = base.get("avg_fps") if base else None
    base_p50 = base.get("p50_ms") if base else None
    base_p95 = base.get("p95_ms") if base else None
    base_p99 = base.get("p99_ms") if base else None
    base_vram = base.get("vram_peak_mib") if base else None

    for cell in cells_by_id.values():
        fps = cell.get("avg_fps")
        if fps is not None and base_fps is not None and base_fps > 0:
            diff = fps - base_fps
            cell["delta_fps"] = round(diff, 3)
            cell["delta_fps_pct"] = round((diff / base_fps) * 100.0, 2)
        else:
            cell["delta_fps"] = None
            cell["delta_fps_pct"] = None

        for metric, base_val, out_key in (
            ("p50_ms", base_p50, "delta_p50_ms"),
            ("p95_ms", base_p95, "delta_p95_ms"),
            ("p99_ms", base_p99, "delta_p99_ms"),
        ):
            val = cell.get(metric)
            cell[out_key] = round(val - base_val, 3) if (val is not None and base_val is not None) else None

        vram = cell.get("vram_peak_mib")
        cell["delta_vram_mib"] = (
            int(vram - base_vram) if (vram is not None and base_vram is not None) else None
        )


def load_device_dir(
    device_id: str,
    path: Path,
    *,
    label: str | None = None,
) -> dict[str, Any]:
    """Load matrix results from a device directory or an existing report JSON file."""
    if path.is_file():
        payload = json.loads(path.read_text(encoding="utf-8"))
        devices = payload.get("devices")
        if isinstance(devices, dict):
            if device_id in devices:
                entry = dict(devices[device_id])
                if label:
                    entry["label"] = label
                compute_deltas(entry["cells"])
                return entry
            if len(devices) == 1:
                only = dict(next(iter(devices.values())))
                only["label"] = device_label_for(device_id, label)
                compute_deltas(only["cells"])
                return only
        raise ValueError(f"{path} does not contain device '{device_id}'")

    if not path.is_dir():
        raise FileNotFoundError(f"input path does not exist: {path}")

    cells: dict[str, dict[str, Any]] = {}
    gpu_name: str | None = None
    for cell in MATRIX:
        bench_candidates = [
            path / cell.cell_id / "benchmark.json",
            path / f"{cell.cell_id}.json",
        ]
        log_candidates = [
            path / cell.cell_id / "run.log",
            path / f"{cell.cell_id}.log",
        ]
        bench_data = None
        for cand in bench_candidates:
            if cand.is_file():
                bench_data = parse_benchmark_json(cand.read_text(encoding="utf-8"))
                break
        log_data = None
        for cand in log_candidates:
            if cand.is_file():
                log_data = parse_run_log(cand.read_text(encoding="utf-8", errors="replace"))
                if gpu_name is None and log_data.get("gpu"):
                    gpu_name = log_data["gpu"]
                break
        cells[cell.cell_id] = combine_cell_metrics(cell, bench_data, log_data)

    compute_deltas(cells)
    return {
        "label": device_label_for(device_id, label),
        "gpu": gpu_name,
        "baseline_cell": "off_off",
        "cells": cells,
    }


def build_report(
    devices: dict[str, dict[str, Any]],
    *,
    game: str = "u3",
    generated_at: str | None = None,
) -> dict[str, Any]:
    """Assemble the canonical JSON report structure across one or more devices."""
    normalized_devices: dict[str, dict[str, Any]] = {}
    for dev_id, entry in devices.items():
        cells = {k: dict(v) for k, v in entry["cells"].items()}
        compute_deltas(cells, entry.get("baseline_cell", "off_off"))
        normalized_devices[dev_id] = {
            "label": entry.get("label") or device_label_for(dev_id),
            "gpu": entry.get("gpu"),
            "baseline_cell": entry.get("baseline_cell", "off_off"),
            "cells": cells,
        }
    return {
        "schema_version": 1,
        "issue": 191,
        "game": game,
        "generated_at": generated_at or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "devices": normalized_devices,
    }


def _fmt_num(val: float | int | None, digits: int = 1) -> str:
    if val is None:
        return "n/a"
    if isinstance(val, int) and digits == 0:
        return str(val)
    return f"{val:.{digits}f}"


def _fmt_delta_fps(cell: dict[str, Any]) -> str:
    if cell["cell_id"] == "off_off":
        return "baseline"
    dfps = cell.get("delta_fps")
    dpct = cell.get("delta_fps_pct")
    if dfps is None or dpct is None:
        return "n/a"
    return f"{dfps:+.1f} ({dpct:+.1f}%)"


def format_markdown(report: dict[str, Any]) -> str:
    """Render a Markdown comparison report from `build_report` output."""
    lines: list[str] = [
        "# Uncharted 3 Readback Cost Matrix (`BB_TINY_READBACK` & `BB_READBACK_SMALL_BUFFERS`)",
        "",
        f"- Issue: #{report.get('issue', 191)} | Game: `{report.get('game', 'u3')}` | Generated: `{report.get('generated_at', '')}`",
        "",
    ]

    devices: dict[str, dict[str, Any]] = report.get("devices", {})
    for dev_id, dev in devices.items():
        header = f"## {dev.get('label', dev_id)} (`{dev_id}`)"
        if dev.get("gpu"):
            header += f" — {dev['gpu']}"
        lines.append(header)
        lines.append("")
        lines.append(
            "| Cell | `BB_TINY_READBACK` | `BB_READBACK_SMALL_BUFFERS` | Avg FPS | Δ vs `off/off` | p50 (ms) | p95 (ms) | p99 (ms) | VRAM peak (MiB) | Stalls >40ms | Status |"
        )
        lines.append(
            "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|"
        )
        cells = dev.get("cells", {})
        has_diag = False
        for matrix_cell in MATRIX:
            cell = cells.get(matrix_cell.cell_id)
            if not cell:
                continue
            if any(
                cell.get(k) is not None
                for k in ("gpu_us_per_draw", "readbacks_per_s", "side_queue_per_s", "host_copies_pct", "gfx_wait_pct", "worst_frame_ms")
            ):
                has_diag = True
            vram_str = str(cell["vram_peak_mib"]) if cell.get("vram_peak_mib") is not None else "n/a"
            stalls_str = str(cell["stalls_over_40ms"]) if cell.get("stalls_over_40ms") is not None else "n/a"
            lines.append(
                f"| `{cell['label']}` | `{cell['tiny_readback']}` | `{cell['readback_small_buffers']}` "
                f"| {_fmt_num(cell.get('avg_fps'), 1)} | {_fmt_delta_fps(cell)} "
                f"| {_fmt_num(cell.get('p50_ms'), 2)} | {_fmt_num(cell.get('p95_ms'), 2)} | {_fmt_num(cell.get('p99_ms'), 2)} "
                f"| {vram_str} | {stalls_str} | {cell.get('ended', 'unknown')} ({cell.get('source', 'unknown')}) |"
            )
        lines.append("")

        if has_diag:
            lines.append(f"### {dev.get('label', dev_id)} — `BB_FRAME_STATS` Diagnostics")
            lines.append("")
            lines.append(
                "| Cell | Worst frame (ms) | GPU us/draw | Readbacks/s | Side queue/s | Host copy wait % | Gfx queue wait % |"
            )
            lines.append("|---|---:|---:|---:|---:|---:|---:|")
            for matrix_cell in MATRIX:
                cell = cells.get(matrix_cell.cell_id)
                if not cell:
                    continue
                lines.append(
                    f"| `{cell['label']}` | {_fmt_num(cell.get('worst_frame_ms'), 1)} "
                    f"| {_fmt_num(cell.get('gpu_us_per_draw'), 2)} "
                    f"| {_fmt_num(cell.get('readbacks_per_s'), 1)} "
                    f"| {_fmt_num(cell.get('side_queue_per_s'), 1)} "
                    f"| {_fmt_num(cell.get('host_copies_pct'), 1)} "
                    f"| {_fmt_num(cell.get('gfx_wait_pct'), 1)} |"
                )
            lines.append("")

    if len(devices) >= 2:
        lines.append("## Cross-Device Summary (`Δ FPS %` and `Δ p99 ms` vs `off/off`)")
        lines.append("")
        dev_items = list(devices.items())
        col_headers = " | ".join(
            f"{d.get('label', dev_id)} FPS (Δ%) | {d.get('label', dev_id)} Δp99 (ms)"
            for dev_id, d in dev_items
        )
        col_align = " | ".join("---:|---:" for _ in dev_items)
        lines.append(f"| Cell | {col_headers} |")
        lines.append(f"|---|{col_align}|")
        for matrix_cell in MATRIX:
            row_parts: list[str] = []
            for _, dev in dev_items:
                cell = dev.get("cells", {}).get(matrix_cell.cell_id, {})
                fps_s = _fmt_num(cell.get("avg_fps"), 1)
                if matrix_cell.cell_id != "off_off" and cell.get("delta_fps_pct") is not None:
                    fps_s += f" ({cell['delta_fps_pct']:+.1f}%)"
                dp99 = cell.get("delta_p99_ms")
                dp99_s = "baseline" if matrix_cell.cell_id == "off_off" else (f"{dp99:+.2f}" if dp99 is not None else "n/a")
                row_parts.append(f"{fps_s} | {dp99_s}")
            lines.append(f"| `{matrix_cell.label}` | " + " | ".join(row_parts) + " |")
        lines.append("")

    return "\n".join(lines)


def _archive_benchmark_json(user_dir: Path | None, cell_dir: Path) -> dict[str, Any] | None:
    if user_dir is None:
        return None
    src = user_dir / "benchmark.json"
    if not src.is_file():
        return None
    dst = cell_dir / "benchmark.json"
    shutil.copy2(src, dst)
    return parse_benchmark_json(dst.read_text(encoding="utf-8"))


def run_matrix(
    command: list[str],
    *,
    out_dir: Path,
    device_id: str = "strix-halo",
    device_label: str | None = None,
    seconds: int = 60,
    timeout: int | None = None,
    user_dir: Path | None = None,
    frame_stats: bool = True,
    vram_log: bool = True,
    cell_ids: list[str] | None = None,
    dry_run: bool = False,
    base_env: dict[str, str] | None = None,
) -> dict[str, Any]:
    """Execute (or dry-run) the 4-way matrix and emit JSON + Markdown reports."""
    if not command:
        raise ValueError("a target command after `--` is required")

    selected = [CELLS_BY_ID[cid] for cid in (cell_ids or [c.cell_id for c in MATRIX])]
    out_dir.mkdir(parents=True, exist_ok=True)
    effective_user_dir = user_dir
    if effective_user_dir is None and base_env and base_env.get("BB_USER_DIR"):
        effective_user_dir = Path(base_env["BB_USER_DIR"])

    run_timeout = timeout if timeout is not None else (seconds + 180 if seconds > 0 else 120)
    cells_out: dict[str, dict[str, Any]] = {}
    gpu_name: str | None = None

    for cell in selected:
        cell_dir = out_dir / cell.cell_id
        cell_dir.mkdir(parents=True, exist_ok=True)
        cell_env = build_cell_env(
            cell,
            base_env=base_env if base_env is not None else dict(os.environ),
            benchmark_seconds=seconds,
            frame_stats=frame_stats,
            vram_log=vram_log,
            user_dir=user_dir,
        )
        log_path = cell_dir / "run.log"
        if dry_run:
            plan = {
                "cell_id": cell.cell_id,
                "label": cell.label,
                "command": command,
                "env": {
                    k: cell_env[k]
                    for k in sorted(cell_env)
                    if k.startswith("BB_")
                },
            }
            (cell_dir / "dry_run.json").write_text(
                json.dumps(plan, indent=2) + "\n", encoding="utf-8"
            )
            cells_out[cell.cell_id] = combine_cell_metrics(cell)
            continue

        # Clear any stale benchmark.json before starting so a crash/timeout never reuses a prior cell's report.
        if effective_user_dir is not None and seconds > 0:
            (effective_user_dir / "benchmark.json").unlink(missing_ok=True)

        timed_out = False
        with log_path.open("w", encoding="utf-8", errors="replace") as log_file:
            try:
                proc = subprocess.run(
                    command,
                    env=cell_env,
                    stdout=log_file,
                    stderr=subprocess.STDOUT,
                    stdin=subprocess.DEVNULL,
                    timeout=run_timeout,
                    check=False,
                )
                exit_code: int | None = proc.returncode
            except subprocess.TimeoutExpired:
                timed_out = True
                exit_code = None

        log_text = log_path.read_text(encoding="utf-8", errors="replace")
        log_data = parse_run_log(log_text)
        if gpu_name is None and log_data.get("gpu"):
            gpu_name = log_data["gpu"]

        bench_data = _archive_benchmark_json(effective_user_dir, cell_dir)
        metrics = combine_cell_metrics(cell, bench_data, log_data, exit_code=exit_code)
        if timed_out and metrics["ended"] == "log-only":
            metrics["ended"] = "timeout"
        cells_out[cell.cell_id] = metrics

    compute_deltas(cells_out)
    report = build_report(
        {
            device_id: {
                "label": device_label_for(device_id, device_label),
                "gpu": gpu_name,
                "baseline_cell": "off_off",
                "cells": cells_out,
            }
        }
    )
    (out_dir / "u3_readback_report.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    (out_dir / "u3_readback_report.md").write_text(
        format_markdown(report) + "\n", encoding="utf-8"
    )
    return report


def _parse_device_input(spec: str) -> tuple[str, Path]:
    if "=" not in spec:
        raise argparse.ArgumentTypeError(f"expected DEVICE=PATH, got {spec!r}")
    device_id, raw_path = spec.split("=", 1)
    device_id = device_id.strip()
    if not device_id or not raw_path:
        raise argparse.ArgumentTypeError(f"expected DEVICE=PATH, got {spec!r}")
    return device_id, Path(raw_path)


def _write_output(target: str | None, content: str) -> None:
    if target is None:
        return
    if target == "-":
        sys.stdout.write(content)
        if not content.endswith("\n"):
            sys.stdout.write("\n")
        return
    path = Path(target)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content if content.endswith("\n") else content + "\n", encoding="utf-8")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Uncharted 3 readback cost 4-way matrix harness & report builder (Issue #191)."
    )
    sub = parser.add_subparsers(dest="subcommand", required=True)

    run_p = sub.add_parser("run", help="Run the 4-way readback matrix against a launch command.")
    run_p.add_argument(
        "--device",
        default="strix-halo",
        help="Device key (e.g. strix-halo, handheld, desktop). Default: strix-halo.",
    )
    run_p.add_argument("--device-label", default=None, help="Human-readable device label.")
    run_p.add_argument(
        "--out-dir",
        type=Path,
        required=True,
        help="Output directory for per-cell logs, benchmark.json copies, and reports.",
    )
    run_p.add_argument(
        "--user-dir",
        type=Path,
        default=None,
        help="Saves/user folder where the runtime writes benchmark.json (exports BB_USER_DIR).",
    )
    run_p.add_argument(
        "--seconds",
        type=int,
        default=60,
        help="BB_BENCHMARK duration in seconds (0 disables BB_BENCHMARK and uses log-only). Default: 60.",
    )
    run_p.add_argument(
        "--timeout",
        type=int,
        default=None,
        help="Per-cell process timeout in seconds (default: seconds + 180, or 120 when seconds=0).",
    )
    run_p.add_argument(
        "--cells",
        nargs="+",
        choices=[c.cell_id for c in MATRIX],
        default=None,
        help="Subset of matrix cells to run (default: all 4).",
    )
    run_p.add_argument(
        "--no-frame-stats",
        action="store_true",
        help="Do not force BB_FRAME_STATS=1 / BB_VRAM_LOG=1.",
    )
    run_p.add_argument(
        "--dry-run",
        action="store_true",
        help="Write per-cell dry_run.json plans and empty report without running the command.",
    )
    run_p.add_argument(
        "command",
        nargs=argparse.REMAINDER,
        help="Command to execute after `--` (e.g. -- target/release/uncharted-universal play u3).",
    )

    rep_p = sub.add_parser(
        "report",
        help="Parse logs / benchmark.json directories (or report JSONs) and emit Markdown + JSON.",
    )
    rep_p.add_argument(
        "--input",
        action="append",
        required=True,
        type=_parse_device_input,
        help="Device input as DEVICE=DIR_OR_REPORT_JSON (repeatable, e.g. strix-halo=... handheld=...).",
    )
    rep_p.add_argument("--md-out", default=None, help="Write Markdown table to path (or '-' for stdout).")
    rep_p.add_argument("--json-out", default=None, help="Write JSON report to path (or '-' for stdout).")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if args.subcommand == "run":
        cmd = list(args.command)
        if cmd and cmd[0] == "--":
            cmd = cmd[1:]
        if not cmd:
            parser.error("`run` requires a command after `--`")
        report = run_matrix(
            cmd,
            out_dir=args.out_dir,
            device_id=args.device,
            device_label=args.device_label,
            seconds=args.seconds,
            timeout=args.timeout,
            user_dir=args.user_dir,
            frame_stats=not args.no_frame_stats,
            vram_log=not args.no_frame_stats,
            cell_ids=args.cells,
            dry_run=args.dry_run,
        )
        sys.stdout.write(format_markdown(report) + "\n")
        return 0

    if args.subcommand == "report":
        devices: dict[str, dict[str, Any]] = {}
        for dev_id, path in args.input:
            devices[dev_id] = load_device_dir(dev_id, path)
        report = build_report(devices)
        md_text = format_markdown(report)
        json_text = json.dumps(report, indent=2) + "\n"
        if args.md_out is None and args.json_out is None:
            sys.stdout.write(md_text + "\n")
        else:
            _write_output(args.md_out, md_text)
            _write_output(args.json_out, json_text)
        return 0

    return 2


if __name__ == "__main__":
    raise SystemExit(main())
