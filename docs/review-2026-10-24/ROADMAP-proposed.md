# Uncharted PC roadmap — 2026-10-24

This plan supersedes the inherited Bloodborne roadmap, preserved in [history](../history/ROADMAP_bloodborne_2026-09_ru.md). It targets fork `iannitram/uncharted_pc`. Current review baseline: main snapshot `68c8cf9a6563d653ed17ce72d758aaa5f33a7a76`; the fork's separate linux branch has reported working builds and must be reconciled before implementation. Dates below are proposed work windows from project kickoff, not delivery commitments. Owners are roles to assign.

## Status on `linux` (2026-10-31)

Where it stands: Window 1 is mostly built (the play-breaking correctness bugs #31, #35, #37 and the header build #60 remain); the Linux row of Window 2 is in progress; Window 3 has many tracked bugs and no playthrough yet; Window 4 has not started. Upstream 0.3.10 and 0.3.11 are not merged yet: the 2026-10-31 merge conflicted, and #164 is taken before any other issue. 1.0 is defined in [SPEC.md](../../SPEC.md): each of the three games plays start to finish on the Legion Go. After 1.0 come shared saves and then 2.0 (sections below).

| Item | Status | Issues / PRs |
|---|---|---|
| Linux branch reconciled with main and the spacepepperoni fork | built; spacepepperoni synced again on 2026-10-28 | main merged in 9050e79; fork syncs PR #47 (457e49e) and #87 / PR #133 (0c68226) |
| Upstream 0.3.10 and 0.3.11 (Supermedo, 26 commits) | blocked: merge conflicts in 10 files | #164 open (upstream-sync, taken first) |
| Compute ring offset reset on queue map | built | #33 closed; PR #43 |
| `WAIT_ON_CE_COUNTER` missing-task guard and `WAIT_ON_DE_COUNTER_DIFF` operands | built | #32 closed, PR #45; #67 closed, PR #162 |
| Flexible-mapping split offset | built | #34 closed; fa64614 |
| Save mount table full reported as success | built | #36 closed; 7b68786 |
| Linux `-Werror` build (`host_priority`) | built on `linux` | #68 closed; 4dd6034 |
| AJM unsupported codec ends the process | built | #56 closed; c0718d8 (from PR #132) |
| Texture-cache `CanContain` and stencil-format back-ports (shadPS4) | built | #136 closed; PR #161 |
| `.pkg` extractor overlapping entries, bounded copies | built | #74 closed; PR #86 |
| ExpandImage stale copy, BUFFER_WBINVL1 decode, equeue loop hang | not started | #31, #37, #35 open |
| GPU build with Vulkan-Headers 1.4.365 | not started | #60 open |
| Save integrity findings S01-S06 | not started, after 1.0 | #48-#53 (also #38) |
| Platform CI and release verification | not started, after 1.0 | #59; Actions usage #98 |
| Windows release-candidate items | not started, after 1.0 | #39, #54, #65 |
| Linux package starts each game | partly: `scripts/patches.py` now shipped; launcher rows that contradict Linux behaviour remain | #73 closed (PR #83); #72 open |
| Launcher texts and options | built: missing texts in 6 languages, the sync's new texts, 540p render height, 45 FPS | #71 (PR #85), #134 (PR #149), #126 (PR #135), #124 closed; zh-CN #146 open |
| Linux key capture: scroll wheel as Mouse 4 / 5 | in review, idle (raised to Medium; PR closes 2026-11-01 if idle) | #125; PR #151 |
| Controller in Desktop Mode, upstream PR pulls | not started | #20, #41 (after 1.0), #42; controller ignored in game #119, Steam Input on shortcuts #145 |
| Per-game defaults: U1 mesh raycaster copies, U2 brightness-flicker fix | built | PR #84 and #110 closed; #123 closed (PR #148) |
| Campaign stability (U1/U2/U3) | in progress: bugs tracked from upstream and peers, playthroughs not started (need the owner's Legion Go) | playthroughs #76-#78; #62-#64, #109, #111-#122, #127-#131, #152, #154-#157, #159, #160; peer findings #137-#141. #61 and #79-#82 were deleted; #109-#113 track that work now |
| Plain ELF input | not started, after 1.0 | #66 |
| Window 4 quality items (shader preload, bounded save match, preparation cache, graphics menu, Xbox prompts, trophies, aspect ratio, AI testers, FSR 4, stereo) | not started, after 1.0 | #16, #57, #58, #10, #11, #12, #14, #15, #18, #19; graphics options #147, ultrawide #158 |
| macOS research | not started, after 1.0; superseded for delivery by the 2.0 Mac build | #55 |
| Shared saves (save-relay) | planned, after 1.0 | #163 (blocked by save-relay's CLI) |

## Window 1: correctness and reproducible builds (weeks 1–2)

Runtime maintainer: resolve save integrity findings [#48–53](https://github.com/iannitram/uncharted_pc/issues/48), existing #34–39, and AJM process termination #56. Renderer maintainer: triage #31–33/#37 and reproduce the current-header build failure #60. Release maintainer: establish dependency-pinned Windows/Linux CI #59. No release passes with unreported skipped runtime tests.

Deliverables: synthetic regression suite, build manifests, error propagation for saves, successful native compilation and explicit remaining failures. Exit: no known silent-save-success failures; build and test logs repeat on clean workers.

## Window 2: Windows and Linux release candidates (weeks 3–5)

| Platform | Deliverables | Dependencies | Release acceptance |
|---|---|---|---|
| Windows x86-64, Windows 10/11 | CLANG64 build recipe; staged portable ZIP; debug symbols; checksums; dependency/license manifest; update rollback and save preservation checks; direct-game Steam shortcut | #39, #54, #59, #60, #65 | U1/U2/U3 boot, save/load, executable switch, restart and controller checks; cold/warm shader runs; AMD/NVIDIA hardware smoke and 30-minute soaks; clean machine DLL check |
| Linux x86-64, SteamOS/Arch first | Reconcile linux branch without overwriting its fixes; shared Uncharted runner; reproducible build docs; portable archive/AppImage; desktop/Steam integration and symbols | #54, #59; existing #20/#41/#42 and linux branch comparison | U1/U2/U3 boot/save/load/switch/restart; RADV and NVIDIA checks; Steam Game Mode input; 20 W Phoenix handheld 30/60 Hz power and frame-time evidence; clean-system package launch |
| macOS Intel and Apple Silicon (research only) | Separate feasibility reports, unsupported-host diagnostics, synthetic ABI/TLS/mapping/fault probes, Vulkan/MoltenVK feature inventory | #55; CPU execution strategy before packaging | Architecture-specific go/no-go with reproducible smoke tests on physical Macs; no playable binary promised |

Linux branch achievements described in fork #23 are historical reports, not measured results of this checkout. Compare each existing fix before selecting commits. Windows packages remain Windows-specific; Linux artifacts must not ship DLL-centric launch paths.

## Window 3: campaign stability and power (weeks 6–8)

QA lead and renderer/runtime owners: reproduce and resolve U2 streaming (#61), chapter 21 progression (#62), U3 geometry (#63) and cutscene stalls (#64). Accept validated plain ELF inputs (#66). Review upstream idle-worker work through existing #41, retaining memory/fence ordering. Do not integrate unreviewed guest-byte patches from issue attachments.

Deliverables: per-game/per-revision chapter checklist, 30-minute streaming and save/load soaks, GPU/driver compatibility table, CPU/power and frame-time benchmarks. 60 Hz is the primary correctness target; 30 Hz is a handheld option. Above 60 FPS is not a fork objective (fork #23). Separate high-rate bug reports from supported-rate regressions.

## Window 4: quality and macOS decision (weeks 9–12, conditional)

Performance owner: shader preload (#16), bounded save matching (#57), shared preparation cache (#58), and measured draw/copy thread tuning. Product work reuses existing issues: graphics profiles #10, Xbox prompts #11, persistent trophies #12, aspect ratio #14, automated QA #15, FSR4 #18 and stereo #19. These follow correctness and require platform compatibility checks.

macOS owner: if window 2 proves a viable runtime, scope an Intel experimental build and/or Apple Silicon execution prototype separately. Deliver an experimental signed/notarized app bundle only after runtime and GPU acceptance; publish unsupported-feature limits. If feasibility fails, publish the blockers and defer playable macOS delivery rather than label a Windows/Wine bundle native.

## Release evidence required

Each candidate records commit, compiler/dependencies, OS/CPU/GPU/driver, game revision identifier (no game files), settings, cold/warm cache state, expected skips, logs and known limitations. Publish artifact checksums and third-party notices, preserve user saves/config, and test upgrade failure recovery. QA evidence includes p50/p95/p99 frame times, RSS/VRAM trend, device-lost/assertion counts and power for handhelds. Source code fixes are proposed in [the review register](../review-2026-10-24/BUG_REPORT.md).

## Mac compatibility bundle experiment

In window 2, also test the Windows release through a macOS-compatible Wine runtime and MoltenVK on physical Macs. Proton itself targets Linux. Apple Silicon needs a validated x86-64 translation path. Deliver a configuration, Vulkan feature inventory and go/no-go report; if successful, scope an experimental compatibility app in window 4 after redistribution and packaging checks. See [the feasibility assessment](../review-2026-10-24/MAC_COMPATIBILITY.md).

## After 1.0: shared saves (save-relay, #163)

The same saves load on every machine I play on: the AMD handheld and a Strix Halo tablet (SteamOS) and an RTX 3090 Ti desktop (Windows), through [save-relay](https://github.com/iannitram/save-relay), a small library every game port reuses. The launcher pulls before Play and pushes after the game quits; a conflict shows save-relay's prompt (keep the server's or this machine's; the other stays in history); with the server unreachable the game starts on local saves and they upload next time. Work: #163 (Medium; Linux and Windows launchers, a save-relay manifest for this port; blocked by save-relay's CLI). Exit: a save made on one machine loads on the others through Play only (needs-owner-device); launcher tests fake the CLI's JSON for pull, push, conflict and offline. The Android and iOS builds use the same saves later.

## 2.0: Android, iOS with controller support, Mac

Routes from SPEC.md's 2.0 pathway; issues are filed when 2.0 starts.

| Target | Route | Proven by | Exit |
|---|---|---|---|
| Android | the game's x86-64 code through FEXCore, the rest native ARM64, Turnip on Adreno; controller input | revvv991/uncharted-android, eladharels/uncharted_android | the three games boot and save on an Adreno device, with save-relay saves |
| Mac (first) | native Apple Silicon: a recompiled library built on a Mac from my own dump, plus a Metal renderer | bloodborne-native/bloodborne-mac | the three games boot and save natively on Apple Silicon; replaces the Wine / MoltenVK experiment above |
| iOS (after Mac) | iOS forbids JIT, so no FEX: the Mac route's recompiled library and Metal renderer, sideloaded, with controller support | bloodborne-native/bloodborne-ios (Ryxm-a/uncharted-ios watched) | the three games boot with a controller; limit stated: about 2 GiB of address space with a free Apple ID, more needs the paid Apple Developer Program |

Each target keeps the Mac / PC / Linux notes in the code map current (R04 in the pipeline repo).

### 2.0 design pass (last step of 2.0)

The last step before 2.0 ships: a design pass on the running builds (launcher and in-game menu on Linux, Windows, Mac, Android and iOS) against the one design system our apps share; findings are filed as `design-pass` issues (`kind:ui`), and 2.0 ships when they are built.
