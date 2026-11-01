# uncharted_pc (fork): spec for 1.0

**Approval:** 2026-10-26, the owner, in the Project Manager session's question tool: "Approve, to Dev Loop (Recommended)" (spec = upstream plan + our delta, R18; built on the owner's 2026-10-25 ruling "Spec next from #23: 1.0 = plays start to finish on the Legion Go"). A changed Must or Won't sets this back to "not yet".

## 1.0 in one sentence

Uncharted 1, 2 and 3 from the PS4 collection each play from the first chapter to the credits on the Legion Go under SteamOS (Linux), from our own build, at 20 W and a steady frame rate, with saves that load.

## The spec is upstream's plan plus our delta

- **Upstream plan:** Supermedo's port, [README](https://github.com/Supermedo/uncharted_pc/blob/main/README.md) and [docs/ROADMAP.md](https://github.com/Supermedo/uncharted_pc/blob/main/docs/ROADMAP.md) (inherited from the Bloodborne port; ours keeps it as history). Our `linux` branch also carries spacepepperoni's Linux fork (latest sync merged in 457e49e, PR #47).
- **Our delta:** the goals and rules in the pinned issue #23, and the delivery plan in [docs/review-2026-10-24/ROADMAP-proposed.md](docs/review-2026-10-24/ROADMAP-proposed.md) as far as it serves 1.0 (Window 1 correctness and builds, the Linux row of Window 2, Window 3 campaign stability). Our 173 commits ahead of upstream (`linux`) are the Linux build, SteamOS packaging, FSR 3.1/4 and DLSS on Linux, the Steam Deck 720p mode, third-party patches per executable, compute-fence and copy fixes, and the trophy and runtime fixes.

| Delta | Upstream? |
|---|---|
| Linux build, AppImage and SteamOS packaging | keep on our fork (upstream is Windows-first) |
| Correctness fixes ported from shadPS4 (#31-#37, #40) | send upstream as bug reports (the owner's standing yes for reports; code PRs need his word) |
| Compute-fence ordering and small compute-buffer copies (U2/U3 freezes, flicker) | send upstream as bug reports |
| Handheld presets, 20 W power target, 30/60 Hz choice | keep on our fork |

## Musts

Each Must is checked on the owner's Legion Go (AMD Z1 Extreme, a Phoenix APU, Radeon 780M, RADV) in SteamOS Game Mode at 20 W, with the owner's own decrypted dump. "needs-owner-device": no satellite can run these checks without that machine and the game.

| # | Must | Done looks like | Check | Issues |
|---|---|---|---|---|
| M1 | Uncharted 1 plays start to finish | Every chapter completes, the credits roll, at 30 or 60 Hz with no crash or hang | A playthrough log per chapter (build commit, preset, FPS from `BB_FRAME_STATS=1`, any assertion) | #76, #110 (mesh raycaster copies), #113 |
| M2 | Uncharted 2 plays start to finish | Same; chapter 21 progresses and the streaming assertion does not stop play | Same log | #109, #62, #77, #112, #113 |
| M3 | Uncharted 3 plays start to finish | Same; no view-blocking corruption, videos play in step with audio | Same log | #63, #64, #78, #111 |
| M4 | Saves work | A save made in each game loads after a restart and after switching games | Save, quit, restart, load, per game | #36 (closed: done on `linux`, 7b68786) |
| M5 | Power and frame rate | 20 W holds; 30 Hz is steady 30 FPS and 60 Hz holds 57-60 FPS, as measured 2026-10-18 | `BB_FRAME_STATS=1` over a 30-minute soak per game | #22 (default rate, open question) |
| M6 | Every build is verified | `bash build.sh` and the C, Python and GPU tests pass on Linux in CI for each merge to `linux` | `.github/workflows/build.yml` green | #60 open; #68 closed (done on `linux`, 4dd6034) |
| M7 | Our package starts each game | The Linux package (AppImage / tarball) starts U1, U2 and U3, including with third-party patches enabled | Package, start each game from the launcher | #72, #73 |
| M8 | The review's play-breaking correctness bugs are fixed | ExpandImage, flexible-mapping split, equeue wait, BUFFER_WBINVL1 decode, AJM MP3/AAC, the CE/DE counter wait | Each issue's own check | #31, #35, #37, #56, #67 open; #32, #33, #34 closed (done on `linux`: PR #45, PR #43, fa64614) |
| M9 | No game files, ever | Nothing from a dump, firmware, keys or game-binary analysis is in the repo, a package or an issue | Review of every PR; the package file list | #23 rules |

## Won'ts (after 1.0)

- The roadmap features: in-game graphics menu (#10), Xbox prompts (#11), trophies to the launcher (#12), 16:10 / ultrawide (#14), AI beta testers (#15), shader preload (#16), the U1 1080p power study (#17), FSR 4 (#18), U3 stereo mode (#19).
- Windows and macOS delivery (#54, #55, #65, #39), plain ELF input (#66), the save edge cases from the review (#38, #48-#53, #57), runner caching (#58), full platform CI and release verification (#59), and upstream PR pulls that wait on upstream (#41, #42).
- Running above 60 FPS (not a goal, #23).

## 2.0 pathway (decided 2026-10-31)

Not part of 1.0, so no Must or Won't changes. In order:

1. **Shared saves first:** the same saves load on the AMD handheld and the Strix Halo tablet (SteamOS) and the RTX 3090 Ti desktop (Windows), through [save-relay](https://github.com/iannitram/save-relay), my own small library that every game port reuses. The launcher pulls before Play and pushes after the game quits. Later the Android and iOS builds use the same saves.
2. **Then 2.0: Android, iOS (with controller support) and Mac apps.**
   - Android: the route revvv991/uncharted-android and eladharels/uncharted_android prove: the game's x86-64 code through FEXCore, the rest native ARM64, Turnip on Adreno.
   - iOS: iOS forbids JIT, so FEX does not carry over. The route bloodborne-native/bloodborne-ios proves for the same bbport family is a recompiled library built on a Mac from my own dump, plus a Metal renderer, sideloaded. Its limit: about 2 GiB of address space with a free Apple ID; the larger-memory entitlements need the paid Apple Developer Program.
   - Mac: native Apple Silicon on the same recompiler and Metal route (bloodborne-native/bloodborne-mac), and the natural first step toward iOS.

## Prior art

Notes and the survey's findings: [PEERS.md](PEERS.md).

peer: Supermedo/uncharted_pc | GPL-2.0 | upstream: Linux, handheld, resolution, frame rate and upscaler issues and PRs
peer: spacepepperoni/uncharted_Linux | GPL-2.0 | the Linux fork we sync from: launcher, Steam Deck mode, releases
peer: drakolordx7/uncharted_pc | GPL-2.0 | community build: VRAM budget, FSR fixes, 45 FPS limit
peer: Pepin208/uncharted_pc_linux | GPL-2.0 | Linux AppImage, shadPS4 texture-cache back-ports
peer: AdilDevStuff/uncharted_pc_linux | GPL-2.0 | raw mouse, CI packages, aim blur and exposure fixes
peer: Mevlock/uncharted_pc-dev | GPL-2.0 | performance builds: pools, buffer arenas, overlay stats
peer: revvv991/uncharted-android | GPL-2.0 | low-power GPU ideas (Adreno)
peer: shadps4-emu/shadPS4 | GPL-2.0 | texture and buffer cache fixes to back-port into gpu/shadps4
peer: bloodborne-native/bloodborne-runtime | GPL-2.0 | the shim's ancestor: loader and Vulkan layer fixes
peer: bloodborne-native/bloodborne-launcher | GPL-2.0 | launcher profiles and settings (#10)
peer: eladharels/uncharted_android | GPL-2.0 | a second Android port (FEX JIT + Turnip): device notes for the 2.0 Android build
peer: bloodborne-native/bloodborne-mac | GPL-2.0 | native Apple Silicon port of the same bbport family: own recompiler and Metal renderer, the route for the 2.0 Mac build
peer: bloodborne-native/bloodborne-ios | GPL-2.0 | Bloodborne on iPhone with no JIT (recompiled library + Metal, sideloaded): the route and its limits for the 2.0 iOS build
peer: Ryxm-a/uncharted-ios | GPL-2.0 | an Uncharted iOS scaffold that plans FEX with sideload JIT; watch whether it gets past scaffolding

## Platforms

Linux (SteamOS on the Legion Go) is the 1.0 target; the Strix Halo laptop is a second Linux check where it helps. Windows builds keep building but are not a 1.0 gate; macOS is research only (R04: see the code map's platform section).
