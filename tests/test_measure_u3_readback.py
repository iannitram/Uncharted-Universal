"""Tests for tools/measure_u3_readback.py (Issue #191)."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import sys
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parent.parent
MODULE_PATH = ROOT / "tools" / "measure_u3_readback.py"

_SPEC = importlib.util.spec_from_file_location("measure_u3_readback", MODULE_PATH)
assert _SPEC is not None and _SPEC.loader is not None
harness = importlib.util.module_from_spec(_SPEC)
sys.modules["measure_u3_readback"] = harness
_SPEC.loader.exec_module(harness)


def make_benchmark_json(
    *,
    avg_fps: float,
    p50: float,
    p95: float,
    p99: float,
    vram_peak_mib: int,
    stalls_over_40ms: int,
    tiny: str,
    small: str,
    ended: str = "ok",
    seconds: int = 60,
) -> str:
    return json.dumps(
        {
            "game": "u3",
            "seconds": seconds,
            "settings": {
                "BB_RESOLUTION": "1920x1080",
                "BB_TINY_READBACK": tiny,
                "BB_READBACK_SMALL_BUFFERS": small,
            },
            "frame_ms": {"p50": p50, "p95": p95, "p99": p99},
            "avg_fps": avg_fps,
            "vram_peak_mib": vram_peak_mib,
            "stalls_over_40ms": stalls_over_40ms,
            "ended": ended,
        }
    )


SYNTHETIC_LOG = textwrap.dedent(
    """\
    GPU: AMD Radeon 8060S Graphics (RADV STRIX_HALO) (integrated), 32768 MiB VRAM, driver radv 25.0 (Mesa 25.0.0), Vulkan 1.4.350
    Stall: 44.5 ms frame; GPU thread on CPU 12.1 ms; 3 images registered, 0.2 MB image uploads, 0.1 MB buffer uploads
    Frame stats: 80.0 FPS, worst frame 44.5 ms (vblank 120 Hz); 0 shader/pipeline compiles, 0.0 ms; 4 recorder syncs; 120 write faults/s (10/s over dead GPU data, 5/s merged, 24/s read back, 6/s on the side queue (2/s on the faulting thread), 18 us each), 12 hot pages; GPU thread 9.50 us/draw, 1400 draws/frame, idle 12.0%; blocked: recorder 0.2%, host copies 1.5% (3/frame), copy threads 0.0%, GPU ticks 0.1%; reduced-size draws 0/frame of 1400 in the scene
    Queue waits: graphics 2.4% (3/frame; ended by end-of-pipe fences 1.8%, end-of-shader 0.3%, WriteData 0.1%, compute fences 0.2%), compute 0.1% (0/frame); all queues waiting 0.5%; longest 3.20 ms at 0x1000
    Frame pacing: median 12.40 ms, stddev 1.10 ms, p99 18.20 ms, 1 frames over 1.5x median
    VRAM periodic: allocs 40+2 (limit 4096); process 4120 MiB, driver budget 28000 of 32768 MiB; VMA blocks 3900 MiB, allocations 3700 MiB; images 500 (2100 MiB), arenas 2 (1024 MiB, 0 MiB in system memory), buffers 500 MiB; allocation failures 0, recovered 0, host fallbacks 0, reclaims 0 (0 images, 0 MiB)
    Frame stats: 84.0 FPS, worst frame 19.2 ms (vblank 120 Hz); 0 shader/pipeline compiles, 0.0 ms; 2 recorder syncs; 100 write faults/s (8/s over dead GPU data, 4/s merged, 16/s read back, 4/s on the side queue (0/s on the faulting thread), 14 us each), 10 hot pages; GPU thread 8.50 us/draw, 1380 draws/frame, idle 14.0%; blocked: recorder 0.1%, host copies 0.5% (1/frame), copy threads 0.0%, GPU ticks 0.1%; reduced-size draws 0/frame of 1380 in the scene
    Queue waits: graphics 1.6% (2/frame; ended by end-of-pipe fences 1.2%, end-of-shader 0.2%, WriteData 0.1%, compute fences 0.1%), compute 0.0% (0/frame); all queues waiting 0.2%; longest 1.80 ms at 0x2000
    Frame pacing: median 11.80 ms, stddev 0.80 ms, p99 16.00 ms, 0 frames over 1.5x median
    VRAM periodic: allocs 42+2 (limit 4096); process 4380 MiB, driver budget 28000 of 32768 MiB; VMA blocks 4050 MiB, allocations 3850 MiB; images 520 (2200 MiB), arenas 2 (1024 MiB, 0 MiB in system memory), buffers 520 MiB; allocation failures 0, recovered 0, host fallbacks 0, reclaims 0 (0 images, 0 MiB)
    """
)


class MeasureU3ReadbackTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp(prefix="u3-readback-test-"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)

    def test_matrix_cells_and_env_generation(self) -> None:
        self.assertEqual(
            [
                (c.cell_id, c.label, c.tiny_readback, c.readback_small_buffers)
                for c in harness.MATRIX
            ],
            [
                ("off_off", "off/off", "0", "0"),
                ("tiny_only", "tiny=1", "1", "0"),
                ("small_only", "small=1", "0", "1"),
                ("both_on", "both=1", "1", "1"),
            ],
        )

        base = {"PATH": "/usr/bin", "BB_BENCHMARK": "999"}
        env_bench = harness.build_cell_env(
            harness.CELLS_BY_ID["tiny_only"],
            base_env=base,
            benchmark_seconds=45,
            frame_stats=True,
            vram_log=True,
            user_dir=self.tmp / "saves",
        )
        self.assertEqual(env_bench["BB_TINY_READBACK"], "1")
        self.assertEqual(env_bench["BB_READBACK_SMALL_BUFFERS"], "0")
        self.assertEqual(env_bench["BB_BENCHMARK"], "45")
        self.assertEqual(env_bench["BB_FRAME_STATS"], "1")
        self.assertEqual(env_bench["BB_VRAM_LOG"], "1")
        self.assertEqual(env_bench["BB_USER_DIR"], str(self.tmp / "saves"))
        self.assertEqual(env_bench["BB_GPU_USER_DIR"], str(self.tmp / "saves"))
        # Original dict is not mutated.
        self.assertEqual(base["BB_BENCHMARK"], "999")

        env_log_only = harness.build_cell_env(
            harness.CELLS_BY_ID["small_only"],
            base_env=base,
            benchmark_seconds=0,
            frame_stats=True,
            vram_log=False,
        )
        self.assertEqual(env_log_only["BB_TINY_READBACK"], "0")
        self.assertEqual(env_log_only["BB_READBACK_SMALL_BUFFERS"], "1")
        self.assertNotIn("BB_BENCHMARK", env_log_only)
        self.assertEqual(env_log_only["BB_FRAME_STATS"], "1")
        self.assertNotIn("BB_VRAM_LOG", env_log_only)

    def test_parse_benchmark_json_valid_and_invalid(self) -> None:
        raw = make_benchmark_json(
            avg_fps=72.5,
            p50=13.7,
            p95=16.2,
            p99=19.8,
            vram_peak_mib=4600,
            stalls_over_40ms=2,
            tiny="1",
            small="0",
        )
        parsed = harness.parse_benchmark_json(raw)
        self.assertEqual(parsed["game"], "u3")
        self.assertEqual(parsed["seconds"], 60)
        self.assertAlmostEqual(parsed["avg_fps"], 72.5)
        self.assertAlmostEqual(parsed["p50_ms"], 13.7)
        self.assertAlmostEqual(parsed["p95_ms"], 16.2)
        self.assertAlmostEqual(parsed["p99_ms"], 19.8)
        self.assertEqual(parsed["vram_peak_mib"], 4600)
        self.assertEqual(parsed["stalls_over_40ms"], 2)
        self.assertEqual(parsed["ended"], "ok")

        for broken in ("{}", '{"frame_ms": {"p50": 1, "p95": 2}}', "[]"):
            with self.assertRaises(ValueError):
                harness.parse_benchmark_json(broken)

    def test_parse_run_log_extracts_fps_pacing_vram_and_diagnostics(self) -> None:
        parsed = harness.parse_run_log(SYNTHETIC_LOG)
        self.assertEqual(parsed["gpu"], "AMD Radeon 8060S Graphics (RADV STRIX_HALO) (integrated)")
        self.assertEqual(parsed["gpu_total_vram_mib"], 32768)
        self.assertEqual(parsed["frame_stats_windows"], 2)
        self.assertAlmostEqual(parsed["avg_fps"], 82.0)
        self.assertAlmostEqual(parsed["p50_ms"], 12.1)
        self.assertIsNone(parsed["p95_ms"])
        self.assertAlmostEqual(parsed["p99_ms"], 17.1)
        self.assertAlmostEqual(parsed["worst_frame_ms"], 44.5)
        self.assertEqual(parsed["vram_peak_mib"], 4380)
        self.assertEqual(parsed["vma_blocks_peak_mib"], 4050)
        self.assertEqual(parsed["stalls_over_40ms"], 1)
        self.assertAlmostEqual(parsed["gpu_us_per_draw"], 9.0)
        self.assertAlmostEqual(parsed["readbacks_per_s"], 20.0)
        self.assertAlmostEqual(parsed["side_queue_per_s"], 5.0)
        self.assertAlmostEqual(parsed["fault_thread_per_s"], 1.0)
        self.assertAlmostEqual(parsed["host_copies_pct"], 1.0)
        self.assertAlmostEqual(parsed["gfx_wait_pct"], 2.0)

    def test_combine_deltas_and_multi_device_markdown_and_json_report(self) -> None:
        strix_dir = self.tmp / "strix"
        handheld_dir = self.tmp / "handheld"
        strix_dir.mkdir()
        handheld_dir.mkdir()

        strix_fixtures = {
            "off_off": (88.0, 11.3, 13.1, 15.8, 4200, 0, "0", "0"),
            "tiny_only": (76.5, 13.0, 15.4, 19.2, 4220, 1, "1", "0"),
            "small_only": (84.0, 11.9, 14.0, 17.5, 4250, 2, "0", "1"),
            "both_on": (72.0, 13.8, 16.5, 22.0, 4290, 4, "1", "1"),
        }
        for cell_id, (fps, p50, p95, p99, vram, stalls, tiny, small) in strix_fixtures.items():
            cdir = strix_dir / cell_id
            cdir.mkdir()
            (cdir / "benchmark.json").write_text(
                make_benchmark_json(
                    avg_fps=fps,
                    p50=p50,
                    p95=p95,
                    p99=p99,
                    vram_peak_mib=vram,
                    stalls_over_40ms=stalls,
                    tiny=tiny,
                    small=small,
                ),
                encoding="utf-8",
            )
            (cdir / "run.log").write_text(SYNTHETIC_LOG, encoding="utf-8")

        # Handheld directory exercises log-only + flat `<cell>.log` layout.
        handheld_fps = {
            "off_off": (34.0, 29.4, 34.0, 3100),
            "tiny_only": (29.5, 33.9, 41.2, 3120),
            "small_only": (32.0, 31.2, 38.0, 3150),
            "both_on": (27.2, 36.7, 46.5, 3180),
        }
        for cell_id, (fps, med, p99, vram) in handheld_fps.items():
            log = (
                "GPU: AMD Custom APU 0932 (RADV VANGOGH) (integrated), 4096 MiB VRAM, driver radv 24.3, Vulkan 1.4.300\n"
                f"Frame stats: {fps:.1f} FPS, worst frame {p99:.1f} ms (vblank 30 Hz); 0 shader/pipeline compiles, 0.0 ms; 1 recorder syncs; "
                "80 write faults/s (5/s over dead GPU data, 2/s merged, 12/s read back, 3/s on the side queue (1/s on the faulting thread), 20 us each), "
                "6 hot pages; GPU thread 18.00 us/draw, 1400 draws/frame, idle 2.0%; blocked: recorder 0.5%, host copies 2.5% (4/frame), copy threads 0.0%, GPU ticks 0.1%; reduced-size draws 0/frame of 1400 in the scene\n"
                f"Frame pacing: median {med:.2f} ms, stddev 1.50 ms, p99 {p99:.2f} ms, 0 frames over 1.5x median\n"
                f"VRAM periodic: allocs 20+1 (limit 4096); process {vram} MiB, driver budget 3600 of 4096 MiB; VMA blocks {vram - 100} MiB, allocations {vram - 200} MiB; images 300 (1500 MiB), arenas 1 (512 MiB, 0 MiB in system memory), buffers 300 MiB; allocation failures 0, recovered 0, host fallbacks 0, reclaims 0 (0 images, 0 MiB)\n"
            )
            (handheld_dir / f"{cell_id}.log").write_text(log, encoding="utf-8")

        devices = {
            "strix-halo": harness.load_device_dir("strix-halo", strix_dir),
            "handheld": harness.load_device_dir("handheld", handheld_dir),
        }
        report = harness.build_report(devices, generated_at="2026-11-23T01:00:00Z")

        strix_tiny = report["devices"]["strix-halo"]["cells"]["tiny_only"]
        self.assertAlmostEqual(strix_tiny["delta_fps"], -11.5)
        self.assertAlmostEqual(strix_tiny["delta_fps_pct"], -13.07)
        self.assertAlmostEqual(strix_tiny["delta_p99_ms"], 3.4)
        self.assertEqual(strix_tiny["delta_vram_mib"], 20)
        self.assertEqual(strix_tiny["source"], "benchmark+log")

        hh_both = report["devices"]["handheld"]["cells"]["both_on"]
        self.assertEqual(hh_both["source"], "log")
        self.assertEqual(hh_both["ended"], "log-only")
        self.assertIsNone(hh_both["p95_ms"])
        self.assertAlmostEqual(hh_both["delta_fps_pct"], -20.0)

        md = harness.format_markdown(report)
        self.assertIn("## Strix Halo (RADV) (`strix-halo`)", md)
        self.assertIn("## AMD Handheld (`handheld`)", md)
        self.assertIn("## Cross-Device Summary", md)
        self.assertIn("`off/off`", md)
        self.assertIn("`tiny=1`", md)
        self.assertIn("`small=1`", md)
        self.assertIn("`both=1`", md)
        self.assertIn("-13.1%", md)
        self.assertIn("-20.0%", md)
        self.assertIn("`BB_FRAME_STATS` Diagnostics", md)

    def test_run_matrix_executes_fake_runner_and_clears_stale_benchmark(self) -> None:
        fake_runner = self.tmp / "fake_runner.py"
        fake_runner.write_text(
            textwrap.dedent(
                """\
                import json, os, pathlib, sys
                tiny = os.environ["BB_TINY_READBACK"]
                small = os.environ["BB_READBACK_SMALL_BUFFERS"]
                user_dir = pathlib.Path(os.environ["BB_USER_DIR"])
                user_dir.mkdir(parents=True, exist_ok=True)
                print(f"GPU: AMD Radeon 8060S Graphics (RADV STRIX_HALO), 32768 MiB VRAM, driver radv 25.0, Vulkan 1.4.350")
                print(f"Frame stats: {90.0 if tiny == '0' else 78.0:.1f} FPS, worst frame 20.0 ms (vblank 120 Hz); 0 shader/pipeline compiles, 0.0 ms; 0 recorder syncs; 10 write faults/s (0/s over dead GPU data, 0/s merged, 5/s read back, 1/s on the side queue (0/s on the faulting thread), 10 us each), 0 hot pages; GPU thread 8.00 us/draw, 1000 draws/frame, idle 10.0%; blocked: recorder 0.0%, host copies 0.5% (1/frame), copy threads 0.0%, GPU ticks 0.0%; reduced-size draws 0/frame of 1000 in the scene")
                # Simulate a crash on tiny=1 & small=1 before benchmark.json is written.
                if tiny == "1" and small == "1":
                    sys.exit(1)
                fps = 90.0 - (12.0 if tiny == "1" else 0.0) - (4.0 if small == "1" else 0.0)
                (user_dir / "benchmark.json").write_text(json.dumps({
                    "game": "u3",
                    "seconds": int(os.environ["BB_BENCHMARK"]),
                    "settings": {"BB_TINY_READBACK": tiny, "BB_READBACK_SMALL_BUFFERS": small},
                    "frame_ms": {"p50": 1000.0 / fps, "p95": 14.0, "p99": 18.0},
                    "avg_fps": fps,
                    "vram_peak_mib": 4000,
                    "stalls_over_40ms": 0,
                    "ended": "ok",
                }), encoding="utf-8")
                """
            ),
            encoding="utf-8",
        )
        out_dir = self.tmp / "out"
        user_dir = self.tmp / "user"

        report = harness.run_matrix(
            [sys.executable, str(fake_runner)],
            out_dir=out_dir,
            device_id="strix-halo",
            seconds=15,
            user_dir=user_dir,
        )
        self.assertTrue((out_dir / "u3_readback_report.json").is_file())
        self.assertTrue((out_dir / "u3_readback_report.md").is_file())

        cells = report["devices"]["strix-halo"]["cells"]
        self.assertAlmostEqual(cells["off_off"]["avg_fps"], 90.0)
        self.assertAlmostEqual(cells["tiny_only"]["avg_fps"], 78.0)
        self.assertAlmostEqual(cells["small_only"]["avg_fps"], 86.0)
        # Crash on both_on must not pick up stale small_only benchmark.json!
        self.assertFalse((out_dir / "both_on" / "benchmark.json").exists())
        self.assertEqual(cells["both_on"]["source"], "log")
        self.assertEqual(cells["both_on"]["ended"], "exit:1")

        # Test `report` CLI subcommand on the generated report JSON.
        md_out = self.tmp / "cli.md"
        json_out = self.tmp / "cli.json"
        rc = harness.main(
            [
                "report",
                "--input",
                f"strix-halo={out_dir / 'u3_readback_report.json'}",
                "--md-out",
                str(md_out),
                "--json-out",
                str(json_out),
            ]
        )
        self.assertEqual(rc, 0)
        self.assertIn("`off/off`", md_out.read_text(encoding="utf-8"))
        self.assertEqual(json.loads(json_out.read_text(encoding="utf-8"))["game"], "u3")

    def test_dry_run_writes_plans_without_running_command(self) -> None:
        out_dir = self.tmp / "dry"
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = harness.main(
                [
                    "run",
                    "--device",
                    "handheld",
                    "--out-dir",
                    str(out_dir),
                    "--seconds",
                    "30",
                    "--dry-run",
                    "--",
                    "/nonexistent/runner",
                ]
            )
        self.assertEqual(rc, 0)
        self.assertIn("AMD Handheld", buf.getvalue())
        plan = json.loads((out_dir / "both_on" / "dry_run.json").read_text(encoding="utf-8"))
        self.assertEqual(plan["env"]["BB_TINY_READBACK"], "1")
        self.assertEqual(plan["env"]["BB_READBACK_SMALL_BUFFERS"], "1")
        self.assertEqual(plan["env"]["BB_BENCHMARK"], "30")
        self.assertEqual(plan["env"]["BB_FRAME_STATS"], "1")


if __name__ == "__main__":
    unittest.main()
