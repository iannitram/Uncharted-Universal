# Roadmap

Updated: 2026-11-20

features:
- campaign: Uncharted 1, 2 and 3 play start to finish on the AMD handheld (M1-M3): the playthroughs, progress blockers, crashes, physics, input and audio in today's runtime
- gpu: The graphics library draws each game correctly (M8): play-breaking renderer bugs, shadPS4 fixes, upscalers and GPU vendor workarounds
- perf: Power and frame rate (M5): render size, run settings, the benchmark and auto-tune
- crash-help: Crash help: the crash line, the known-issues list, the playthrough log and the after-crash screen
- prep: Stage 1, the prep tools in Rust: dump check, revision gate, package, patches, link, prepare, mods, and no game files (M9, M11)
- launcher: Stage 2 and stage 10, the faces: the CLI, the Slint app, the Continue screen, first run, Saves and Settings, About and languages, the design pass (M7, M10)
- runtime: Stage 3, the runtime to Rust, one module per piece, then the C build path deleted (M20)
- hubs: Stage 4, build hubs: discovery, pairing, serve and pull, fleet records (M12)
- steamos: Stage 5, SteamOS handhelds: Steam shortcut, artwork, handheld profile, Game Mode, the device hours (M13)
- windows: Stage 6, the Windows desktop: the build, the Windows shims, an hour per game (M14)
- android: Stage 7, Android on Adreno through FEXCore and Turnip (M17) (1.x)
- apple: Stage 8, Mac, iPhone and iPad through a recompiled library and Metal (M15, M16): the apps and their controllers are 1.0 (owner rulings 2026-11-17), and their button prompts (owner rulings 2026-11-18); the hour and session per game and the M21 check (8.08-8.10) are 1.x
- saves: Saves work, one save everywhere through the adopted shared save-relay core (https://github.com/iannitram/save-relay), and updates keep saves (M4, M18, M19, stage 9)
- builds: Stage 0 and every build verified: housekeeping, CI, release builds, upstream syncs, project rules (M6)
- later: After 1.0 only, the new features among the Won'ts: in-game graphics menu, Game Mode menu, launcher trophies, AI beta testers, shader preload, FSR 4, stereo, plain ELF input, Mali GPUs

From `SPEC.md` to 1.0 (the SPEC was approved 2026-11-17, with the changes of that day: "Everything is approved"; its changes of 2026-11-18, M15 for the iOS install from Dropbox and M16 and M18 for the launcher 1.0 minimums, set the approval back to "not yet" until the owner approves them again): Uncharted 1, 2 and 3 on the AMD handheld, the Strix Halo tablet and the RTX 3090 Ti desktop, the same save on each and on the Mac and the iPhone or iPad, the Mac and the iPad played with a controller, on every one of these devices the launcher 1.0 minimums (detected button prompts, full controller support, and save-relay built in and set up from the launcher; owner rulings 2026-11-18, Decisions), each game start to finish on the AMD handheld, and our code in Rust. The Mac and iPhone/iPad apps, their controllers and their save-relay are 1.0 by the owner's rulings of 2026-11-17 (Decisions, below); the hour per game on the Mac, the session per game on the iPhone or iPad, their M21 check and Android stay 1.x (scope ruling 2026-11-12, #268). The code map is `docs/code-map.md`; the decisions are `docs/adr/` (0001 workspace, 0002 runtime to Rust in strangler order, 0003 Slint launcher, 0004 build hubs, 0005 Android through FEXCore, 0006 Apple through a recompiled library and Metal). Every piece is an issue titled `<id>: ...`; `Blocked by` in each issue sets the order. The plan of 2026-10-24 (`docs/review-2026-10-24/ROADMAP-proposed.md`) is history now; its open work is carried into the stages below.

## Decisions

- Owner ruling 2026-11-17 (Mac satellite session, question tool): "Fold the save relay into the Mac app via a step in the roadmap. Should be a 1.0 that exists in every universal app with and ios and Mac component. Revise all roadmaps if you need to."
- Owner ruling 2026-11-17: "The save relay Mac and iOS functions need to be brought into 1.0" and "Revise the roadmap". It was told to the Project Manager and confirmed in the Mac satellite session's question tool: "Yes, all into 1.0 (Recommended)". It supersedes the earlier "Ships with iOS app" placement. In 1.0: save-relay in the Mac app (9.03, #252), save-relay in the iOS app (9.04, #251), and the round trip between them and the other 1.0 devices (9.02).
- Owner ruling 2026-11-17: "The ipad os and mac versions are obviously going to need to support an external controller. Make sure that's in the road map", confirmed in the question tool: "Yes, 1.0, Mac and iPad (Recommended)". In 1.0: the controller step 8.06 (#410), and the pieces the apps need, 8.01 to 8.05.
- For these pieces, the second and third rulings supersede the 2026-11-12 scope ruling (#268), which had put the Mac and the iPhone/iPad in 1.x.
- Owner ruling 2026-11-18 (Mac satellite session): "To test install on my iPad I'll need a file on Dropbox. I don't have direct access to the Mac so we'll need a different side load process".
- Owner ruling 2026-11-18, in the question tool: "SideStore on the iPad (Recommended)"; "Free Apple ID for now (Recommended)"; "All universal apps (Recommended)". So the iOS app no longer goes over a cable with Xcode signing: the Mac builds an unsigned .ipa (iphoneos, arm64) for the iPhone/iPad app and puts it in `Dropbox/_transfers/Uncharted-Universal/ios/` with a version in its file name, and I install it from Dropbox with SideStore on the iPad, which signs it with my free Apple ID (re-signed every 7 days on the device). The folder is outside Dropbox's "Apps" folder, which the Mac mini's Dropbox does not sync. A free Apple ID gets neither the increased-memory-limit nor the extended-virtual-addressing entitlement, so the memory figures of 8.05 are measured without them; the paid Apple Developer Program stays a later upgrade. Logs from the iPad come back through the app's own log file in Dropbox or the share sheet, not a cable. Changed here: 8.05 and the stage 4 note; the issues of 8.05 (#409) and 9.04 (#251) are amended. SPEC M15 changes with it and waits for the owner's re-approval.
- owner ruling 2026-11-18: "Final deliverables getting to me via Dropbox are really the only way in this case so exception cleared". It settles the question the SPEC Won't (builds move between my devices "over the home network only, never through a server") and M11 (game files stay on the home network) raised for the .ipa in Dropbox, which carries the per-game library generated from my dump (8.03; ADR 0006, the library is a build-time app input): the final builds (the iPad `.ipa` and the Mac app), delivered to my own Dropbox under `_transfers/Uncharted-Universal/`, are an allowed exception to M12 and to the home-network-only Won't; game data still never goes into a repo, a public place or a server of ours. The re-approval of M15 is still the owner's.
- owner ruling 2026-11-18 (Mac satellite session), L1: "Minimum launcher 1.0 steps being detected button overlays, full controller support, and save relay built and incorporated should be the same for every platform of every universal app".
- owner ruling 2026-11-18, L2: "Let's build the whole launcher with the relay into it now. That is all 1.0".
- So every 1.0 platform (Linux/SteamOS on the AMD handheld and the Strix Halo tablet, Windows on the RTX 3090 Ti desktop, the Mac, the iPhone or iPad) has the same three minimums at 1.0, each a step with a device check:
  - (a) detected button prompts: the game's on-screen buttons redrawn to match the controller (Xbox, PlayStation, Nintendo; any other controller gets Xbox's), chosen automatically with a manual choice, from pictures generated at setup from the game's own icon sheets, never shipped: 3.23 (#439) on Linux/SteamOS and Windows, 8.12 (#441) on the Mac and the iPhone or iPad, with #11 folded in.
  - (b) full controller support: every input on Xbox, PlayStation, Switch Pro and Joy-Con and generic pads through SDL3 or GameController, hot-plug mid-game, touch controls hidden with a controller on the iPhone or iPad, and the launcher by controller: 3.10 (#370) and 8.06 (#410), tightened, and 2.12 (#438), new.
  - (c) save-relay built and incorporated: on in each release build, pull before play and push after, the conflict prompt with both saves, offline play continuing, first-time setup and sign-in in the launcher (in the app on the iPhone or iPad) with the credential in the system's credential store, checked by a round trip: 9.01 (#163), 9.03 (#252) and 9.04 (#251), tightened, and 9.05 (#440), new; 9.02 checks the round trip across them.
  - Android stays after 1.0 (the 2026-11-12 scope ruling, #268), so nothing is added for it here. SPEC M16 and M18 widen with these rulings and the Xbox prompts leave the after-1.0 Won'ts; that waits for the owner's re-approval.

## Game files

On the RTX 3090 Ti desktop (`desktop-3090ti`), recorded 2026-11-12. These files never go into a repo or a bundle (M9).

| | Path |
|---|---|
| Raw package (US, v1.02, CUSA02320) | `L:\Dropbox\Software\Emulation\roms\PS4\Uncharted The Nathan Drake Collection - [US] [EN] [1.02].pkg` (read only; never install or extract into Dropbox) |
| Installed copy (55.7 GB: 15,195 files in 336 folders, 59,807,175,456 bytes, counted 2026-11-12) | `B:\games-donotdelete\Uncharted-Universal\CUSA02320\` (`eboot.bin`, `big2-ps4_Shipping.elf`, `big3-ps4_Shipping.elf`, `sce_discmap.plt`, `sce_module`, `sce_sys`, `shareddata`, `u1data`, `u2data`, `u3data`) |

How it was made: the shadPS4 fork's `shadps4.exe --extract-pkg <pkg> --extract-dir <new empty folder> --extract-threads 8`. shadPS4's `config.toml` `installDirs` points at `B:/games-donotdelete/Uncharted-Universal`.

## Features, by Must

| Must | Pieces |
|---|---|
| M1-M3 playthroughs on the AMD handheld | the correctness and campaign work already open (#31, #35, #62-#64, #109, #111-#113 and the bug issues; #37, #60, #164 closed), crash help (#165: #168-#170, #175; the after-crash screen is 2.07, which replaced closed #176), playthroughs #76-#78; 5.02 |
| M4 saves work | 3.06 (savedata in Rust), 1.17, 9.02 |
| M5 power and frame rate | 1.14, 1.15, 1.16, 1.27, auto-tune (#166: #171, #173 (the `BB_BENCHMARK` runtime benchmark that the M5 soak reads), #174 and #178 are 1.27, and #172 is 1.05 `dump`; the display cards are 2.06, which replaced closed #177), #22; 5.02 |
| M6 every build verified | 0.02, 2.10, 3.01, 6.01, 8.02 (#407); 1.x: 7.02 |
| M7 the launcher starts each game | 1.04-1.15, 1.25, 2.02-2.05 |
| M8 play-breaking bugs | #31 (merged, handheld check owed), #35 (merged, handheld check owed), #37 (closed), all in `gpu/` (#164 closed) |
| M9 no game files | 1.23 (bundle never holds the dump), 1.26 (redact), PR review |
| M10 Continue screen | 1.18, 1.25, 2.01, 2.04, 2.05, 2.06, 2.07, 2.12 (#438), 9.05 (#440), 10.01 |
| M11 prepare on a hub, revision gate | 1.02-1.10, 1.24, 2.05 |
| M12 pull from a hub, fleet | 1.23, 4.01-4.05, 2.08 |
| M13 Steam shortcut, artwork, handheld profile | 1.16, 1.21, 1.22, 5.01, 5.02 |
| M14 Windows desktop | 3.15, 6.01, 6.02 |
| M15 Mac and the iOS install | 8.01-8.05 (1.0, the apps that M16 and M18 need: #211, #396, #407, #408, #409); 1.x: 8.08 |
| M16 controller play and button prompts on every 1.0 device (owner rulings 2026-11-18) | 3.10 (#370), 3.23 (#439), 2.12 (#438), 8.05 (#409), 8.06 (#410), 8.12 (#441); on the devices 5.01, 5.02, 6.02; 1.x: 8.09 |
| M17 Android | 1.x: 7.01-7.06 |
| M18 save-relay everywhere | 1.17, 1.25, 9.01 (#163), 9.02, 9.03 (#252), 9.04 (#251), 9.05 (#440), on the apps of 8.05 (#409); 1.x: 7.05 |
| M19 updates keep saves | 1.19, 1.20 |
| M20 our code in Rust | stage 1 (tools), stage 2 (launcher), stage 3 (runtime), 2.10 (the C in `packaging/`), 2.11, 3.17 |
| M21 resolution independence | 1.14 `render_size` (done, PR #294), 1.28 `aspect` (done, #305: pure Rust aspect-math module in `crates/core` with unit tests), M21a #432 (in progress: render size chosen from the display or window), M21b #433 (HUD/menu anchoring in `gpu/`), M21c #434 (FOV rule), M21d #435 (video letter/pillarbox), M21e #436 (runtime resize, presenter-only rescale plus restart prompt for a full re-render) |

Game-side pieces for M21 resolution independence (#297, carrying #14):
- M21a (#432): the Windows native loader chooses the startup render size from SDL window pixels via `aspect` (1.28) and `render_size` (1.14), bounded by the game and direct memory budgets. Tested with synthetic windows and executable images, including incomplete movie patch patterns that must fall back without changing the image. Projector and Strix Halo gameplay checks remain `needs-owner-device`.
- M21b (#433): HUD, menus, pause and map anchoring in `gpu/` to a safe area, not stretched.
- M21c (#434): camera FOV rule: Hor+ for wider screens, Vert+ or kept horizontal FOV for taller screens.
- M21d (#435): pre-rendered video and cutscenes letterboxed or pillarboxed to their own aspect, with pillarbox only as a fallback.
- M21e (#436): runtime resize and fold/unfold via presenter-only rescale plus a restart prompt for a full re-render.

## Stages

Each stage names its platforms (R04). Stages overlap: anything not blocked can be taken. The campaign work on today's C runtime (M1-M3, M8, #164 first) continues alongside; a bug fixed in C before its module moves is ported with the module.

0. **Housekeeping** (all). 0.01 the rename inside the repo: `uncharted_pc` and the `linux` branch in our docs, workflows, launcher update check and bug-report links (done). 0.02 the Rust workspace check (`check.yml`) on Linux, Windows, macOS, beside `build.yml` (done).
1. **Engine core: the prep tools in Rust** (Linux, Windows, macOS in CI). All 28 modules (`1.01`–`1.28`) are implemented in `crates/core` (the only remaining `NotBuilt` stub is `games::title_region` awaiting verified `param.sfo` evidence, and `#173` runtime benchmark wiring is `#242`): 1.01 error and platform; 1.02 `sfo`; 1.03 `games` and the revision gate; 1.04 `pkg`; 1.05 `dump` check (#172 folds in); 1.06 `content_profile`; 1.07 `patches`; 1.08 `link` (modules); 1.09 `link` (libc); 1.10 `prepare`; 1.11 `mods`; 1.12 `cheats`; 1.13 `trophies`; 1.14 `render_size`; 1.15 `run_env`; 1.16 `profile`; 1.17 `saves` and the save-relay manifest; 1.18 `savecard`; 1.19 `backup`; 1.20 `update`; 1.21 `vdf` and `steam` (from totk-native); 1.22 `artwork`; 1.23 `bundle` and `fleet` records; 1.24 `recipe` (targets, hubs); 1.25 `launch`; 1.26 `logs` (redact, crash line, known issues, playthrough log: #169 and #170 move here; #168 writes the line in the runtime, 3.06 and 3.16, and 1.26 reads it); 1.27 `tune` (the hardware report read from `uncharted-run --probe`, the choices it hides, the three cards, the chapter list of `recipe`: #171, #174, #178; reads #173's benchmark report); 1.28 `aspect`.
2. **Faces** (Linux, Windows; macOS builds in CI, and 8.05 releases the Mac app at 1.0). 2.01–2.11 are closed (`2.01` Continue screen brief #288; `2.02` CLI #316; `2.03` Slint scaffold; `2.04` Continue screen #293; `2.05` guided first run #369; `2.06` Saves and Settings #357, with shared launch-settings conversion #453 merged and #444/#454 open; `2.07` after-crash screen #343, with `#185`'s `logs::append` call present in `app/src/vm/after_crash.rs` pending acceptance audit; `2.08` Fleet #340; `2.09` About and languages #322, with Continue screen translations and moving the language button to Settings open in #379; `2.10` release builds #356; `2.11` retire `launcher/`, `scripts/`, `run.py` and PyInstaller packaging #377). `2.12` (#438, #446) is merged on `main` (controller navigation across the launcher and Button prompts choice; physical controller matrix on Windows/handheld/tablet/Mac remains open).
3. **Runtime to Rust** (Linux first in CI, Windows with 3.15; macOS: CI build check only). 3.01–3.12, 3.14 and 3.15 are closed (`3.01` #337 `abi` and `gpu/` build script; `3.02` #366 `time` and `rtc`; `3.03` #361 `sync`; `3.04` #367 `content` and `display`; `3.05` #326 `trophy`; `3.06` #360 `savedata` and `where.json`; `3.07` #364 `audio`; `3.08` #368 `ajm`; `3.09` #359 `file`; `3.10` #370 `pad`; `3.11` #373 `services` and `kernel`; `3.12` #325 `memory` on Linux/Android, Windows keeps `src/runtime_memory.c` until 3.17; `3.14` #328 `cpu_patch` and `red_zone`; `3.15` #358 `os` Windows shims). Open in Stage 3: 3.13 `fiber` and `thread` (#362, merged on Linux); 3.16 `loader` and the `uncharted-run` binary (#457, partial: `BBPROBE*` parser, `--list-missing`, pattern scanner, `--probe`, `--force-fault` #168; full guest/GPU/signal wiring and `3.16a` #443 remain); 3.17 delete `src/` and the C build path. M21 in the runtime (#297): 3.18 M21a (#432, in progress) render size from the display or window, through `render_size` (1.14) and `aspect` (1.28); 3.19 M21b (#433) HUD and menu anchoring to the safe area in `gpu/`; 3.20 M21c (#434) the FOV rule; 3.21 M21d (#435) video and cutscene letterbox or pillarbox; 3.22 M21e (#436) live resize and fold/unfold, presenter rescale plus a restart prompt. Button prompts (owner rulings 2026-11-18, Decisions): 3.23 the game's button prompts follow the controller on Linux/SteamOS and Windows: Xbox, PlayStation and Nintendo pictures (any other controller gets Xbox's), chosen from the controller type `pad` reads (3.10), the manual choice of 2.12, the pictures generated at setup from the game's own icon sheets in the dump and never shipped (#439; #11 folds in). A device check on the AMD handheld follows each of 3.02-3.16 (SPEC.md); ADR 0002 names six of them (savedata, audio, AJM, memory, threads, the loader).
4. **Build hubs** (Linux, Windows; at 1.0 the Mac prepares only for itself and for the iPhone or iPad, which installs the Mac's .ipa from Dropbox with SideStore (8.03, 8.05; owner ruling 2026-11-18), so the Mac hub's discovery, pairing and serving and the Android client are 1.x). 4.01 discovery (#330, closed); 4.02 pairing (#333, closed); 4.03 serve and pull with the shader translations (#346, closed); 4.04 fleet records and drift (`crates/core::fleet` and `app/src/vm/fleet.rs` present on fixtures; real device records pending); 4.05 `hub` and `fleet` commands (#351, closed). CI runs a fake hub on loopback.
5. **SteamOS handhelds** (Linux). 5.01 the Strix Halo tablet as hub: prepare all three games on itself, play an hour each from Game Mode; 5.02 the AMD handheld: install pulled from the tablet, Steam artwork, an hour each, the M5 soak; then the playthroughs #76-#78.
6. **Windows desktop.** 6.01 the Rust workspace and the GPU library on `windows-latest` (CLANG64, Rust target chosen here); 6.02 an hour per game with a controller.
7. **Android** (Adreno) (1.x, after 1.0). 7.01 adapt selected reviewed Android dependency/build, FEX bridge and Turnip/libadrenotools sources from revvv991 and zenithblue with credit through #207-#210, after the runtime ABI/thread/memory prerequisites; do not graft the whole peer project. 7.02 the runtime cross-builds for `aarch64-linux-android` in CI; 7.03 the Slint launcher's Android APK build with gamepad keys; 7.04 the hub serves the APK and install, the launcher installs them; 7.05 save-relay in the Android launcher; 7.06 a session per game with a controller; 7.07 the M21 check on the Pixel Fold: unfolded (about 6:5), the cover screen, and a fold/unfold mid-game.
8. **Mac, iPhone and iPad** (1.0 for the apps, their controllers and the pieces they need, by the owner's rulings of 2026-11-17, and their button prompts by those of 2026-11-18, see Decisions; 8.08-8.10 stay 1.x, after 1.0). 8.01 study bloodborne-native's recompiler, Metal renderer and iOS shell: licence files, what is Bloodborne-specific, a written plan (docs; #211, with 8.01a #212; both closed); 8.02 the runtime builds for `aarch64-apple-darwin` and `aarch64-apple-ios` in CI (#407, closed); 8.03 bring in the recompiler, run on the Mac from my dump to a library per game (#396); 8.04 the Metal renderer for the collection (#408); 8.05 the Mac app from the launcher, built for Apple silicon (arm64) and published by the release, then the iOS app built on the Mac as an unsigned .ipa (iphoneos, arm64), put in `Dropbox/_transfers/Uncharted-Universal/ios/` with a version in its file name and installed from Dropbox with SideStore on the iPad, which signs it with my free Apple ID (re-signed every 7 days on the device; owner ruling 2026-11-18, see Decisions), each reaching the game (#409); 8.06 Mac and iOS controller mapping: the game takes Xbox, PlayStation, Switch Pro and Joy-Con, MFi and generic controllers through Apple's GameController framework (Switch and generic added by the owner rulings of 2026-11-18), every button mapped, touch hidden with a controller, a controller connected or disconnected mid-game picked up, the iOS app's own screens usable with a controller alone, checked on the Mac mini and the iPad; the launcher keeps gilrs for its menus (2.12) (#410). 8.12 the game's button prompts on the Mac and the iPhone or iPad, as 3.23, from the GameController type, drawn by the Metal renderer (#441; owner rulings 2026-11-18). 8.07 and 8.11, save-relay in the iOS app and in the Mac app, are now 9.04 (#251) and 9.03 (#252) in stage 9. After 1.0 (1.x): 8.08 the Mac: an hour per game; 8.09 the iPhone or iPad: a session per game with a controller; 8.10 the M21 check on the iPad mini (about 3:2) and the Mac window resized.
9. **One save everywhere.** External dependency: [iannitram/save-relay](https://github.com/iannitram/save-relay) for Linux, Windows, macOS, iOS/iPadOS and Android. Save Relay reached 1.0 on 2026-11-11, so it is now adopted, not just referenced: every build carries it (tracker #249). Desktop integration (9.01, #163) is partial (#445): the Windows zip carries the pinned `v1.0.0-rc.2` CLI, MIT notice, component lock and three manifests, and Play invokes it for each title and selected save slot before the runtime starts and after it exits; wiring its conflict events to the launcher, the AppImage package and cross-device release acceptance remain open. The Mac app's save-relay (9.03, #252) waits on a supported macOS build (save-relay#238). The iOS app (9.04, #251) and Android (7.05) use its C interface (save-relay#49 and save-relay#254, both closed); Android is 1.x and does not block 1.0. Save slots move the save folders, so the save-relay manifest and adapter follow the selected slot. 9.01 attaches the adapter in the launcher, on in the released AppImage and Windows zip, with the conflict prompt and offline play (#163 reworked; tightened by the owner rulings of 2026-11-18); 9.05 save-relay's first-time setup and sign-in in the launcher on Linux/SteamOS, Windows and the Mac, the credential in the system's credential store, usable by controller (#440, after 9.01; owner rulings 2026-11-18); 9.03 puts save-relay in the released Mac app, pinned to a save-relay release tag, set up in the launcher with the credential in the Keychain (#252, was 8.11; after 9.01, 9.05 and 8.05); 9.04 puts it in the iOS/iPadOS app through its C interface and Swift wrapper, built from a save-relay release tag, set up and signed in from the app with the credential in the iOS Keychain, and checks the round trip from the Mac to the iPhone or iPad and back (#251, was 8.07; after 8.05 and 9.03); 7.05 (1.x) covers Android. 9.03 and 9.04 are 1.0 by the owner's rulings of 2026-11-17, and 9.05 by those of 2026-11-18 (Decisions). 9.02 verifies the round trip across the 1.0 devices (the AMD handheld, the Strix Halo tablet, the RTX 3090 Ti desktop, the Mac and the iPhone or iPad) on their release builds, after 9.01, 9.03, 9.04 and 9.05: the 1.0 sentence. This project does not build a separate sync service.
10. **Design pass** (R04), last before 1.0 (Linux/SteamOS and Windows; macOS: CI build check only). 10.01 the built screens against the Continue screen brief and the shared design system, including the Steam artwork and Game Mode; findings become `design-pass` issues.

## Coordinated implementation lanes (2026-11-20)

The owner requested one local Flint lane and one hosted lane (GPT-6.1 Terra, selected
in place of Luna), with the coordinator preparing exact file-and-line briefs,
reviewing changes, assigning focused repairs, and merging only after checks pass.
Each lane works from an unclaimed, unblocked issue on its own branch/worktree.
Owner instruction 2026-11-20: fix save-relay-related defects in this build, then
open a matching `iannitram/save-relay` issue explaining the problem and fix, with
linked evidence and tests. `AGENTS.fork.md` records this handoff requirement.
Existing PRs #423, #445, #446 and #447 retain their owners; their build-header,
relay, controller and resolution files are not parallel implementation targets.

| Lane | Issue and production boundary | Status / acceptance still owed |
|---|---|---|
| Local Flint through OpenCode / LM Studio | #426 (closed, PR #452, `dba42a2d` / `42132a7c`): the stencil-association condition in `TextureCache::FindDepthTarget`, `gpu/shadps4/video_core/texture_cache/texture_cache.cpp` | **Merged on `main`**; U1/U2/U3 device gameplay still owed |
| GPT-6.1 Terra | #403 (closed, PR #448, `591dabe8`): reject unknown tile modes 27-30 in both texture-binding descriptor guards, adapting only the pinned Mevlock hunk | **Merged on `main`**; Windows CI and U1/U2/U3 checkpoint loads still owed |
| GPT-6.1 Terra | #427 (PR #451, `896ac130` / `6f406a69`): S_LSHR_B64, S_BFM_B64, 64-bit shift amount, V_CVT_F32_UBYTE | **4 of 5 units merged on `main`** with `shader-scalar-64-test` passing in CI run `44766909839`; `#5714` atomic fallback split into `#450` (open) |

The next local handoff is [#454 / 444b](docs/work-briefs/444-flint-next.md):
only the CLI `play_plan` saved-settings wiring and its focused tests, building on
the merged #453 / 444a conversion and the merged relay (#445) and controller (#446)
launch paths (alongside resolution #447). The
brief includes exact baseline anchors, proposed call-site code, JSON fixtures,
environment assertions, stop conditions and test commands. It is prepared, not
dispatched; the coordinator refreshes the actual base/API before starting Flint.
Parent #444 retains GUI wiring and Windows child-process acceptance.

Large local-model jobs are divided before dispatch. In particular #427 is five
separate review units under the same parent: (1) S_LSHR_B64 (the actual hunk in
upstream #5723 despite its V_LSHR_B64 title), (2) S_BFM_B64 (#5724), (3) scalar
64-bit shift amount (#5725), (4) V_CVT_F32_UBYTE format (#5719), and (5) f32
min/max fallback (#5714, split into #450). The source audit found V_LSHR_B64 already dispatched
and implemented; that is not evidence that the scalar fix is present. The
coordinator first compares each source hunk to production, supplies exact symbols
and test commands in a brief, and records already-present fixes with evidence.
The #5714 fallback is tracked separately in #450 and needs a dependency assessment before implementation;
its control-flow/device-feature changes must not be transplanted as a small
local-model task. This is a work breakdown, not completed code.

Two further audits are preserved before implementation: [#299's FFmpeg build and
packaging brief](docs/work-briefs/299-ffmpeg.md) separates actual staged binary
provenance from merely shipping a license text; [#428's TSC brief](docs/work-briefs/428-tsc.md)
splits the guest clock contract, Rust instruction rewriting, and coordinated
guest/GPU timestamp wiring. Neither is a direct local-model import: #428's peer
uses a different patcher, and #299 needs a reproducible build and dependency audit.
Their parent issues remain open; no compatibility, timing or gameplay result is
claimed from these source audits. The [#431 save-layout brief](docs/work-briefs/431-save-layout.md)
also records why a shared save-folder option cannot be added as a core-only helper:
folder-level listing/copy/delete assumes one game per folder. Coordinate the
layout and manifest change with the active relay/controller launcher PRs first.

The #297 resolution umbrella carries the earlier #14 16:10 requirements; its
implementation order is #432, then #433/#434/#435, with #436 retaining the runtime
resize gate. These are outside the first two lane assignments. Feature `saves`
includes the shared save-relay integration; there is no separate feature id.

## Platform readiness evidence

The [2026-11-06 code audit](docs/audits/2026-11-06-universal-platforms.md) maps current blockers to these existing stages. The [platform peer survey](PEERS.md) supplies frozen source and file-level license links; the [issue-disposition ledger](docs/audits/2026-11-06-issue-dispositions.md) records the full open-issue review. Prioritize the dump/revision/recipe contracts and a wired preparation flow before advertising target builds. Stage 8.01 is a hard source/synthetic feasibility gate (#211): it does not inspect or retain game dumps. It must assess the bloodborne-recomp, bbcpu and Metal route before downstream Apple implementation; the direct Uncharted Mac launcher is a Rosetta-based comparison, and the Uncharted iOS peer is a scaffold. No stage or acceptance gate is marked complete by the survey.

### Current implementation checkpoint (2026-11-06)

The foundations of #188 (PR #228), #189 (PR #231), #206 (PR #232), #207 (PR #229) and #211 (PR #230) are merged into `main`; the code map lists what each still owes. In particular, #189 still needs listener/discovery/CLI wiring and device acceptance; #207 has not been built with the Android SDK/NDK; and #211 is plan-only because CPU/Metal probes were not run on Apple hardware. No platform application or gameplay milestone is complete from this work. #188 is closed. #211 and #212 (both closed) became 1.x work by the scope ruling of 2026-11-12 (#268), and stage 8's app pieces are 1.0 again since the owner's rulings of 2026-11-17 (Decisions).

The next implementation wave (status 2026-11-12): #37 shader barrier is closed (PR #263, with a `wbinvl1` test; credits shadPS4 6752d8b1). #175 known-issue data is done on `main` (19 verified entries, each linked to an issue, loaded by a test that reads the shipped file). #190 Rust artwork generator: PR #223 is merged and emits the original images from Rust; #190 is reopened to delete `launcher/uncharted_art.py` and its test once nothing calls them. #31 ExpandImage stale-cache fix (PR #226) and #35 equeue lost-request fix (PR #225) are fixed on `main` since 2026-11-07; both issues stay open until the handheld check is recorded. Also merged 2026-11-12: #168 the Rust `crash::report` and its `--force-fault` test hook (PR #267; the C runtime that ships today still prints no `Crash:` line, so #168 stays open), #171 `uncharted-run --probe` hardware report (PR #260), #173 benchmark report and pad-path contracts only (PR #236; nothing reads `BB_BENCHMARK` yet, wiring is #242), #147 tune DLSS/FSR rules (PR #257), #138 RAM/VRAM line on the FPS counter (PR #264), #246 save slots in the Linux launcher (PR #254), #247 Turnip KGSL/FEX patches vendored (PR #255), CI on GitHub-hosted runners for Linux, Windows and macOS (PR #243), and an upstream sync of Supermedo/uncharted_pc `main` on 2026-11-11 (PR #239). Device and visual acceptance remain required; none of this marks its containing roadmap stage complete.


### Issue-backed peer import sequence

These issues refine work inside the existing platform stages; they do not close
or replace stage commitments. Issue #207 builds pinned Android dependencies and
a synthetic native artifact only, not the full game APK/engine. #208 depends on
#207 and adapts the Linux ARM64/glibc donor smoke harness to Android NDK/bionic
and Android execution. #209 depends on #208 plus the runtime C ABI scaffold
(3.01), memory (3.12), and thread/fiber (3.13); the Windows OS shims in 3.15
are not an Android prerequisite. #210 is a standalone native Turnip/libadrenotools
bootstrap after #207, before Slint app wiring: only explicitly selected,
validated drivers may start; invalid custom selection must fail closed and may
not silently fall back to an unvalidated stock Adreno driver. App wiring remains
inside 7.03. The exact source boundaries are in [PEERS.md](PEERS.md#issue-backed-platform-survey-2026-11-06).

For Apple, #211 gates implementation on source/synthetic coverage and a
file-level license/dependency map; it creates no game-dump inspection. #212 is
synthetic no-JIT CPU coverage after #211. Existing #31 and #37 remain the
small, issue-scoped shadPS4 import candidates; link to their pinned upstream changes in
PEERS.md rather than creating duplicates. The upstream `main` was merged into production `main` on 2026-11-11 (PR #239), so no sync batch is planned. #206 carries the
separate Rust SELF/plain-ELF loader-parity obligation from the closure of #66.
Save Relay reached 1.0 on 2026-11-11 and is folded into every build; tracker #249
holds one row per build. This project still builds no sync service of its own.

## Open-source pieces and where they fit

| Project | Licence evidence | Where | How |
|---|---|---|---|
| shadPS4 | GPL-2.0 | `gpu/` | kept in our fork; read-only source research only; no reports or PRs to shadPS4 (anti-AI) |
| Supermedo/uncharted_pc, spacepepperoni/uncharted_Linux | GPL-2.0 roots | current C/C++ lineage and Linux base | upstream `main` merged into production `main` on 2026-11-11 (PR #239); no sync batch now. Port behavior into Rust modules as separate issues. |
| zorde23/Uncharted_Collection_PC and older community forks | GPL-2.0 roots | Windows/Linux comparisons | targeted fixes only when an open bug has the same cause; no broad engine graft |
| revvv991/uncharted-android | GPL-2.0-or-later root; [`android/LICENSES.md`](https://github.com/revvv991/uncharted-android/blob/847faf5a802253cf90664144765f0e5d12eb410e/android/LICENSES.md#L1-L30) inventories dependencies | Android, #207-#210 | selected build/bridge references; exclude recipes, byte patches, generated game output and assets |
| zenithblue-oss/shadps4-arm64 | per-file SPDX MIT for selected FEX source; transitive `hle_trace.h` is GPL-2.0-or-later | Android FEX, #208/#209 | experimental synthetic harness/context/HLE reference, not a drop-in engine; preserve exact file and dependency notices |
| FEX-Emu/FEX | MIT root; bundled components have individual notices | Android, #207-#209 | FEXCore dependency through a pinned build recipe |
| bylaws/libadrenotools | BSD-2-Clause root and loader files | Android, #210 | verified original API/implementation; use explicit choice and verify loaded driver before Vulkan startup |
| JICA98/libadrenotools | BSD-2-Clause | Android dependency comparison | peer's exact pinned variant; prefer bylaws source unless its fork-specific patch is required |
| SDL3 (`sdl3-sys` crate, `no-link`: the loader links SDL3) | zlib | runtime input/window/audio (audio since 3.07) | existing dependency; launcher UI remains Slint |
| eladharels/uncharted_android | GPL-2.0 root | Android device notes | learn; incomplete coverage and no additional import identified |
| bloodborne-native/bloodborne-recomp, bbcpu, bloodborne-runtime, -mac, -ios | mostly GPL-2.0 roots; exact files/dependencies vary; [`bbnet20` is GPL-3.0](https://github.com/bloodborne-native/bloodborne-runtime/blob/42736cf7ca7704b087e847482417d9f475bb2dd7/gpu/bbnet20/LICENSE.GPL-3.0#L1-L8) | Apple, #211/#212 before 8.03-8.06 | hard feasibility/license gate; no blanket adoption, no Bloodborne annotations/generated output, exclude GPL-3.0 component |
| Ryxm-a/uncharted-ios | GPL-2.0 root | iOS scaffold | learn only; default lite build does not run engine; conflicts with no-JIT route |
| bmy/bbport-mac | GPL-2.0 root | Apple comparison | Rosetta guest is outside approved no-Rosetta route |
| truwa/nathan-drake-collection-macos | no root license/API identifier established | Mac packaging comparison | learn only; Rosetta and different engine; no source copy until terms established |
| iannitram/totk-native | MIT | `vdf`, `steam`, `platform`, `bundle`, `hub`, `fleet`, `savecard`, Continue brief | copy with credit as already planned |
| iannitram/save-relay | MIT | `relay` and cross-platform saves | adopted at 1.0 (2026-11-11) in every build, tracker #249; #163 is the desktop adapter; no separate sync service |
| slint-ui/slint | royalty-free (attribution) | `app/` | dependency; About carries notice |
| gilrs, mdns-sd, spake2, chacha20poly1305, sha2, cbindgen, cmake, roxmltree, serde_json, sys-locale | MIT or Apache-2.0 | `app/`, `hub`, `patches`, `language`, `runtime` | dependencies; the ones the app and core link are credited on About (`app/src/vm/about.rs`, tested against this table) |
| Revivedx/SavageWindRecomp | MIT | packaging | learn, for 2.10 |
| bloodborne-native/bloodborne-launcher | GPL-2.0 | launcher profiles and settings | learn, for 1.16 and 2.06 |
| GNSB/UFC-3-PS4-PORT-LINUX-WINDOWS | GPL-2.0 | sibling PS4 port | watch |

## Save Relay integration

I'm adopting [Save Relay](https://github.com/iannitram/save-relay) for this port's saves. It shares game saves between machines through my own Nextcloud, pulling at launch and pushing at exit, as one small Rust core reused by every game port. Save Relay reached 1.0 on 2026-11-11 (release tag to follow). This port will adopt it for saves, so a save made on one of my machines is there on the next.

The owner's rulings of 2026-11-17 (Decisions) put save-relay at 1.0 in the Mac app (9.03, #252) and in the iOS/iPadOS app (9.04, #251), both on the apps of 8.05 (#409). 9.02's 1.0 round trip now covers the Mac and the iPhone or iPad. The owner's rulings of 2026-11-18 (Decisions) make save-relay on in every release build, set up and signed in from the launcher (9.05, #440; from the app on the iPhone or iPad, 9.04), part of 1.0 on every 1.0 platform.

## After 1.0

The Won'ts in `SPEC.md`: the in-game graphics menu and the Game Mode menu (#10, #167), trophies in the launcher (#12), ultrawide beyond the target aspects (#158), AI beta testers (#15), shader preload (#16), the U1 1080p power study (#17), FSR 4 (#18), stereo (#19), plain ELF input (#66), the save edge cases (#38, #48-#53, #57), runner caching (#58), Mali GPUs.
