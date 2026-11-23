# Apple Peer Scan Findings (since 2026-11-06)

Summary of commits and reusable patterns found across the Apple Silicon / Metal / iOS no-JIT peers for issues #396 (8.03 recompiler), #408 (8.04 Metal renderer), #409 (8.05 Mac app + split iOS `.ipas`), #410 (8.06 GameController), and #441 (8.12 button prompts).

All peer text was treated strictly as data. Excluded: `bbnet20` (GPL-3.0), any Bloodborne-specific annotations/offsets or game bytes (M9), and `truwa/nathan-drake-collection-macos` (`license: null`).

## Peer Activity Since 2026-11-06

| Peer | Root License | Latest Commit | Notes |
|---|---|---|---|
| `bloodborne-native/bloodborne-runtime` | GPL-2.0 (`gpu/shadps4/` GPL-2.0-or-later; `bbnet20` GPL-3.0 excluded) | `d97fc67` (2026-11-19) | Active Metal memory/residency/low-memory/macOS-input work (details below) |
| `bloodborne-native/bloodborne-mac` | GPL-2.0 | `295e603` (2026-11-19) | Launcher/release bumps (`0.10.0`, `0.10.1`) tracking runtime low-memory mode and `BB_GC_BUDGET_CAP_MB` |
| `bloodborne-native/bloodborne-recomp` | GPL-2.0 | `c60ccf8` (2026-11-03) | No commits since 2026-11-06 |
| `bloodborne-native/bbcpu` | GPL-2.0 | `3f9b6c1` (2026-11-02) | No commits since 2026-11-06 |
| `bloodborne-native/bloodborne-ios` | GPL-2.0 | `8f08be4` (2026-11-02) | No commits since 2026-11-06 |
| `bmy/bbport-mac` | GPL-2.0 | `2026-10-30` | No commits since 2026-11-06 |
| `Ryxm-a/uncharted-ios` | GPL-2.0 | `2026-10-21` | No commits since 2026-11-06; `ios/scripts/make_ipa.sh` has a reusable `Payload/` + `ldid` thin `.ipa` packaging pattern for #409 (without JIT entitlement) |
| `truwa/nathan-drake-collection-macos` | `null` (unlicensed) | `2026-11-08` (push) | No commits since 2026-11-06; excluded due to missing license |

## Reusable Findings from `bloodborne-native/bloodborne-runtime`

1. **Texture cache budget cap on integrated / unified-memory GPUs (`d97fc67`, 2026-11-19) — Adopted**
   - On Metal (and large unified-memory APUs), the driver's reported budget is the GPU working set (e.g. 13.3 GiB on an 18 GB M3 Pro), so a 70% GC trigger (~9.5 GiB) lets the texture cache grow unboundedly over long sessions.
   - `BB_GC_BUDGET_CAP_MB=N` caps the driver's budget at `N` MiB on integrated GPUs unless `BB_GC_BUDGET_MB` is explicitly forced.
   - Ported into [`gpu/shadps4/video_core/texture_cache/texture_cache.cpp`](../../gpu/shadps4/video_core/texture_cache/texture_cache.cpp) with credit.

2. **Metal guest memory pool regions, LRU eviction, and residency leak fix (`b093fad`, `ff6c2f0`, `2340067`, 2026-11-10) — For #408 / #409**
   - Importing the entire guest memory pool into Metal counts ~8.4 GiB against Metal's memory limit, exceeding the ~5.4 GiB budget on 8 GB Macs and iOS devices.
   - `BufferCache::PoolPiece` imports 16 MiB pieces on demand, merges adjacent/overlapping ranges into non-overlapping regions, and evicts LRU regions past `BB_POOL_BUDGET_MB` (default 2048 MiB) once `device.CompletedTick()` passes them.
   - **Residency leak fix (`b093fad`):** `BufferGone` in `mtl_frame.mm` must remove destroyed buffers from `address_buffers`, `declared_buffers`, and the Metal residency set; otherwise freed pool regions remain resident and counted for the life of the process.
   - **>4 GiB Metal window bug (`2340067`):** Importing the full pool as a single `MTLBuffer` caused rendering artifacts past 4 GiB offsets; the two-window split (`BB_POOL_SPLIT`) or per-region buffers avoid offsets approaching 4 GiB.

3. **Metal buffer accounting & headless soak mode (`b96d3e5`, 2026-11-19) — For #408**
   - `MetalResources::LiveBuffers` tracks live Metal buffers across 4 categories (private, shared, shared write-combined, host memory wrapped in place) to distinguish texture cache growth from buffer leaks.
   - `HEADLESS=1` (`BB_HIDDEN_WINDOW=1`, `BB_AUDIO=none`) enables automated soak tests on macOS.

4. **iPhone / 8 GB Mac low-memory sizing (`1dc6cc2`, 2026-11-09) — For #409**
   - `BB_LOW_MEMORY=1` + `BB_FLEX_SPAN_MB` (448–1024 MiB) pairs reduced direct/flexible spans with `BB_POOL_PIECE_MB=16` and `BB_GC_BUDGET_CAP_MB=5120`.

5. **macOS Command+Q quit input (`5c7b5b6`, 2026-11-09) — For #409**
   - Menu-bar Command shortcuts can trigger unwanted quits when holding movement keys and pressing Command (`app issue #14`); handling `Command+Q` as a non-repeating key-down chord with modifier already held avoids key-repeat quits.

6. **GameController (#410 / #441)**
   - No new `GameController` / `GCController` framework commits appeared in the scanned peers since 2026-11-06; `bloodborne-runtime` continues to use SDL3 gamepad/keyboard input.
