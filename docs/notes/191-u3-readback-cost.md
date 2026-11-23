# Issue #191: Uncharted 3 Readback Cost (`BB_TINY_READBACK` & `BB_READBACK_SMALL_BUFFERS`)

## Default-off policy

Per the 2026-11-02 owner ruling ("Keep ours off until measured"), both readback switches default to `"0"` for Uncharted 3 (`Game::U3`):

- `crates/core/src/run_env.rs` (`fix_defaults`): sets `BB_TINY_READBACK="0"` and `BB_READBACK_SMALL_BUFFERS="0"` for `Game::U3` unless present in `player_env`.
- `crates/core/src/recipe.rs` (`CHAPTER_FIXES`): defines chapter fixes only for `Game::U1` and `Game::U2`; `Game::U3` has none.
- `gpu/shadps4/video_core/texture_cache/texture_cache.cpp` (`TextureCache::TinyReadback`): off unless `BB_TINY_READBACK` starts with `'1'`.
- `gpu/shadps4/video_core/renderer_vulkan/vk_rasterizer.cpp` (`Rasterizer::NoteSmallWrittenBuffers`): enabled unless `BB_READBACK_SMALL_BUFFERS` starts with `'0'`, which is why the launcher must emit `BB_READBACK_SMALL_BUFFERS="0"` explicitly rather than leaving it unset.
- `gpu/shim/bbport_settings.cpp` (`BbSettings`): manages upscaler/frame-gen/display settings and does not override either readback variable.

The guard test `crates/core/tests/u3_readback_default_off.rs` verifies this policy across `fix_defaults`, `build_env` (all built-in `DeviceProfile`s), `recipe::CHAPTER_FIXES`, and `launch::Session::env`, while ensuring explicit `player_env` and `settings.extra` overrides still work.

## 4-way A/B matrix

`tools/measure_u3_readback.py` automates and summarizes the 4-way matrix:

| Cell ID | Label | `BB_TINY_READBACK` | `BB_READBACK_SMALL_BUFFERS` |
|---|---|---|---|
| `off_off` | `off/off` | `0` | `0` |
| `tiny_only` | `tiny=1` | `1` | `0` |
| `small_only` | `small=1` | `0` | `1` |
| `both_on` | `both=1` | `1` | `1` |

Each cell is launched with `BB_BENCHMARK=<seconds>` (writes `benchmark.json` with `avg_fps`, `frame_ms.{p50,p95,p99}`, `vram_peak_mib`, `stalls_over_40ms`, `ended`), `BB_FRAME_STATS=1` (prints 5-second `Frame stats`, `Queue waits`, `Frame pacing`, and `Stall:` lines), and `BB_VRAM_LOG=1` (prints periodic `VRAM` lines).

## Usage

### 1. Run the 4-way matrix on a device

Strix Halo (RADV):

```bash
python3 tools/measure_u3_readback.py run \
  --device strix-halo \
  --user-dir /path/to/u3/user \
  --out-dir /tmp/u3-readback-strix \
  --seconds 60 \
  -- ./target/release/uncharted-universal play u3
```

AMD Handheld (Steam Deck / ROG Ally Z1 Extreme):

```bash
python3 tools/measure_u3_readback.py run \
  --device handheld \
  --user-dir /path/to/u3/user \
  --out-dir /tmp/u3-readback-handheld \
  --seconds 60 \
  -- ./target/release/uncharted-universal play u3
```

Notes:
- Pass `--dry-run` to inspect the generated `BB_*` environment per cell (`dry_run.json`) without starting the game.
- Pass `--seconds 0` for log-only capture (`BB_FRAME_STATS=1` + `BB_VRAM_LOG=1` without `BB_BENCHMARK`).
- Before each cell starts, any stale `<user-dir>/benchmark.json` is unlinked so a crash/timeout cannot reuse the previous cell's report.

### 2. Combine Strix Halo and handheld results into Markdown + JSON

```bash
python3 tools/measure_u3_readback.py report \
  --input strix-halo=/tmp/u3-readback-strix \
  --input handheld=/tmp/u3-readback-handheld \
  --md-out /tmp/u3-readback-comparison.md \
  --json-out /tmp/u3-readback-comparison.json
```

`--input` accepts either a device run directory (containing `<cell_id>/benchmark.json`, `<cell_id>/run.log`, or flat `<cell_id>.json` / `<cell_id>.log`) or a previously generated `u3_readback_report.json`.

## Verification (without game files)

```bash
python3 -m unittest tests/test_measure_u3_readback.py
cargo test -p uncharted_core --test u3_readback_default_off
```
