# Apple Metal peers for #408 (2026-11-24)

Read-only peer research (Rule 9: nothing submitted upstream). Licences were checked at the pinned commits; re-check each file's SPDX line before vendoring anything.

**Bottom line:** no project offers an Uncharted-tuned Metal renderer that drops into our shadPS4 `gpu/` tree. bloodborne-runtime's `renderer_metal` remains the base. wie-project/PS4Recomp is the only project that renders an Uncharted Collection title on native Metal without Rosetta or JIT. Its Metal driver is the wrong shape for us, but its shader-recompiler patches save real time. gs-soft-it/lumen-emu might remove most of the 159-file reconcile, but nobody has tested that yet.

## Candidates

| Repo (pin) | Licence | Uncharted / ND? | Metal code? | Verdict | Reason |
|---|---|---|---|---|---|
| wie-project/PS4Recomp (97d9f6e3) | GPL-2.0; 28 files GPL-2.0-or-later, 12 no SPDX | Yes: U1 "Playable" (CUSA02343, Drake's Fortune only), TLOU-R, U4 notes | Yes, a GNM-level Metal 3/4 driver | **Adopt (recompiler parts) / learn (driver)** | Its portable SRT walker unblocks the #396 arm64 build. Also has wave64 passes, `host_fp_env.h`, and the Neo HTILE fix as a cross-check. |
| gs-soft-it/lumen-emu (6127cba8) | GPL-2.0-only; tessellator MIT | No | Yes, Metal behind the plain Vulkan C API | **Probe first** | Could sit under our near-stock `renderer_vulkan` and skip the reconcile. Coverage not checked. |
| Wooloomooloo2/bloodborne_mac_port (eba46328) | GPL-2.0-or-later, all 59 files | No | Yes, 16.4k lines | **Vendor selected files** | Original upstream of the shared Metal files. Take the SPIR-V rewrites, `vk_to_mtl`, blit shaders and fence ordering from here. |
| braito4/sotc-mac (c4645464) | GPL-2.0 / -or-later; 112 files no SPDX | No | Overlays on bloodborne-runtime | **Cherry-pick** | Generic fixes on our exact base: retained-reference command buffers (a real lifetime bug), index rebase above 4 GiB, upload gather, in-flight cap, RAM-tiered staging. |
| Huax-1839/bloodborne-macos (c1e85c75) | GPL-2.0-or-later | No | Yes, 21.6k lines | **Vendor small pieces / learn** | Take `mtl_index_expand`, the MSL tiling-header generator and its test. Learn from its zero-copy guest memory design. Needs its own core layer. |
| manjarqo/ps4-native-mac (40062a1a) | GPL-2.0; some files no SPDX | TLOU-R colour, U4 bring-up | Yes (a copy of wie) | **Learn** | `membudget` and pipeline archive ideas, gamma fix. Stale copy: take code from wie. |
| VladZ97/fifa16_mac_port (c0543c8e) | GPL-2.0-or-later | No | Yes, a Wooloomooloo2 fork | **Learn** | 17 modified files hold a second game's texture and draw fixes. Not diffed yet. |
| evill33t/bloodborne_mac (bbacb95c) | Mixed; combined runtime **GPL-3.0-or-later** | No | Yes, its own PM4-to-Metal renderer | **Learn; take files one by one only** | GPL-2 files like `tiling.cpp`, `mtl_formats.h` and the MetalFX wrappers are usable. Avoid bbhost and nightwatch. |
| BlackLineInteractive/Driveclub-Port (80823216) | GPL-2.0, mostly no SPDX | No | Yes, direct GCN-to-MSL | Learn | Reference for a format table and detiler. |
| fladnaG86/Driveclub-Port-AppleSilicon (5f669a0d) | GPL-2.0, no SPDX at all | No | Yes, monolithic | Learn | Generic GCN-on-Metal fixes: wave64 LDS barriers, BC-on-GPU copy, GPU watchdog. |
| ihabfawaqa-hue/GOW3-APPLE-SILICON-NATIVE (4145a672) | GPL-2.0-or-later | No | Prototype | Learn | `ps4_gcn_translate.cpp` is a compact example of shadPS4 recompiler to MSL. |
| GUTY345/shadps4-ios (8de9e132) | GPL-2.0-or-later; patches MoltenVK | No | No | Learn | Only the iOS RAM and device gate idea. |
| ashenwattegedera/uncharted_mac (a6547eb0) | GPL-2.0, no SPDX | Yes (CPU side only) | No | Drop for Metal | Fault-bridge pattern and 4 KB tracker inside 16 KB pages; belongs to #211/#212. |
| ramon141/uncharted_mac (d2fd7789) | GPL-2.0 | Yes | No | Drop | Rosetta plus KosmicKrisp. Only the guest memory layout and mimalloc low-window idea. |
| macgametest/nathan-drake-collection-macos (aa0a3835) | **No licence** | Yes (launcher) | No | Drop | It is truwa's repo renamed. Settings list is a checklist only. |
| Sqhh99/Bloodborne-for-Mac (bd07d181) | GPL-2.0, plus GPL-3.0 bbnet | No | Yes (stale copy) | Drop | Older copy of Wooloomooloo2. |
| nacimoualla/orbis-on-macos (3743444d) | MIT | No | Immature | Drop | JIT-based, no macro tiling. |
| bloodborne-native/bloodborne-mac (295e603b) | GPL-2.0 | No | No | No change | Confirms the inventory pin d97fc678 is current. |
| SamoZ256/shadPS4, TheBrinkOfTomorrow, burhanmoin1, jarthcomps, Acelogic/VibeStation5, ravencaio/tlou-remastered-pc | GPL-2.0 | TLOU-R only (ravencaio) | No | Drop | No Metal code. ravencaio is a source of ND-engine Vulkan fixes, not Apple ones. |
| MohinShaikh5689/god-of-war-3-ios | **GPL-3.0** | No | No | Excluded | GPL-3.0, MoltenVK and JIT. |
| shubhxho/ps4-emu | **No licence** | No | Small | Excluded | Cannot copy. |

## Recommendation

1. **Nothing is more Uncharted-customized than bloodborne-runtime in a form we can vendor.** wie's Uncharted fixes are generic and live in a GNM driver bound to its own runtime. Use them as cross-checks, not code:
   - the Neo HTILE slice size
   - async-compute WAIT_REG_MEM ordering (U4 depth copy)
   - cube T# face ranges
   - unmapped T# table entries treated as unbound
   - the U4 known-issues list
2. **The biggest time saver is in the shader recompiler, which we need on any route.** Port from wie at 97d9f6e3, as separate PRs that keep SPDX and provenance lines:
   - `srt.h` plus `flatten_extended_userdata_pass.cpp` (the Xbyak-free SRT walker). This unblocks #396.
   - the wave64 passes
   - `compat/host_fp_env.h` (the FTZ constant bug)

   Compare each against bloodborne-runtime's `srt.h` first.
3. **Run a probe of lumen-emu's MetalVulkan adapter before starting the 159-file reconcile.** Build our `gpu/` with it as the static Vulkan provider and list the missing entry points and features (`robustBufferAccess2`, `nullDescriptor` and others). If the gaps are small, this route is the cleanest on licence (GPL-2.0-only) and already has iOS branches.
4. **If the bloodborne route goes ahead:**
   - Take the shared Metal files from Wooloomooloo2 (the canonical upstream), not from bloodborne-runtime.
   - Then apply sotc-mac's generic fixes 1, 2, 3, 4 and 7, with credit.
   - Add Huax's `mtl_index_expand` and its tiling test harness.

## Licence blockers

- **GPL-3.0-or-later, never take:**
  - evill33t: the combined runtime, bbhost, nightwatch and `native_motion_capture.h`
  - Sqhh99 and bloodborne-runtime: `bbnet` / `thd`
  - god-of-war-3-ios
- **No licence, cannot vendor:** macgametest/truwa, shubhxho. zenithblue-oss/shadps4-arm64 declares no licence either; only its upstream shadPS4 files (GPL-2.0-or-later) could be used through Lumen.
- **No SPDX line, so we must add headers and record provenance:**
  - wie: 12 driver files
  - manjarqo: the videoout files
  - fladnaG86 and BlackLine: all files
  - sotc-mac: 112 files
  - ashenwattegedera: all files
- **Take under MIT, not Apache-2.0:** SPIRV-Cross (Apache-2.0 OR MIT).
- **Excluded by rule:** MoltenVK patches in GUTY345. KosmicKrisp is not our route.

## Weak or unchecked

- lumen-emu adapter: API coverage, renderer drift and the shadps4-arm64 provenance are all unverified.
- bmy/bbport-mac: not inspected. Its buffer-copy batching may overlap bloodborne-runtime a03929b5.
- VladZ97: its 17 modified `renderer_metal` files are not diffed against Wooloomooloo2.
- SamoZ256/luft (AIR shader generation): not vetted.
- fladnaG86 and orbis-on-macos: file SPDX only partly checked.
- wie's Uncharted claim covers U1 only, on region CUSA02343, from its own screenshots. Its perf evidence was deleted, and its tests are not in CI.

## PEERS.md updates

- Repin wie-project/PS4Recomp from 5b4066404570 to 97d9f6e3f747 and record it as an Uncharted-specific Metal peer.
- Move the truwa link to macgametest/nathan-drake-collection-macos.
- Add lumen-emu, Wooloomooloo2, sotc-mac and Huax-1839 rows with the verdicts above.
