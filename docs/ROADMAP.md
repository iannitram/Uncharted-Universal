# Roadmap: Uncharted: The Nathan Drake Collection, native on Linux (SteamOS)

Status as of 2026-10-17. This replaces the Bloodborne roadmap, which is kept in
`history/ROADMAP_bloodborne_2026-09_ru.md` (Russian, unchanged).

A proposed roadmap from the 2026-10-24 review (aimed at main 68c8cf9, so linux needs reconciling): [review-2026-10-24/ROADMAP-proposed.md](review-2026-10-24/ROADMAP-proposed.md).

## Goal

Run Uncharted 1, 2 and 3 from the PS4 collection natively on Linux, on two machines
running SteamOS: a Strix Halo machine (main test and debug box) and a Legion Go
(handheld performance target). The emulation is not rewritten.

Ground rules:

- **Our `linux` branch builds on spacepepperoni's Linux fork** (`spacepepperoni/uncharted_Linux`,
  merged 2026-10-17 at c06843d, the owner's decision). That fork already builds and runs the collection
  on Linux and the Steam Deck and keeps merging SuperMedo's releases. We merge it (and through
  it SuperMedo) instead of redoing its work. Our own work stays on `linux`.
- Nothing goes to SuperMedo or spacepepperoni without the owner saying so, one change at a time.
- No game files, dumps or keys in the repository. Each machine uses its own dump of the
  collection (CUSA02343 / CUSA02320).
- Every claim in a report says whether it was measured or only read from the code.
- **Reverse-engineering output stays private** (decided 2026-10-18: the output stays private; it is for
  the owner's own use, to preserve a library they bought). Disassembly, decompiled code, and notes or patch files that quote or locate
  the games' code are kept privately, outside this repository and its releases. What
  lands here is our own engine code and the plan, never game code.
- Their GitHub workflows came with the merge: `sync-upstream.yml` (every 6 hours, force-pushes an
  `upstream` branch and merges into `main`) and `build.yml` (releases on pushes to `main`). They only
  run from the default branch, so they are inert on `linux`. **Do not merge `linux` into our fork's
  `main` without deciding about them first.**

## Where things stand (2026-10-17)

What spacepepperoni's fork already does (read from their commits and README, **not measured on our
machines**):

- **Builds on Linux.** `build.sh` Linux branch, libavcodec for MP3 audio, CI builds an AppImage.
- **Launches on Linux.** `run.py` is cross-platform now (`WINDOWS = sys.platform == 'win32'`), and
  `launcher/uncharted_linux.py` (1,280 lines) is a Linux launcher: game choice, per-game Add to
  Steam, a Saves folder per game, an update check against both repos, crash reports to SuperMedo's
  issue tracker. `packaging/linux/package.sh` builds the tarball, `install.sh` is a one-line
  installer, and the AppImage updates itself.
- **Steam Deck mode**, chosen automatically on SteamOS (their README: 25-30 FPS on a Deck).
- **Linux game fixes:** the games' code callbacks installed on Linux (Uncharted 2 camera NaN, mouse
  look), PS4 FPU state for guest threads, a NaN guard on colour outputs (Uncharted 2 black squares),
  constant-ring polling (Uncharted 3 stretching), AMD GPU hang markers.
- **Ported fixes** from sibling shadPS4 and bbport forks: texture cache, VRAM pressure, frame limiter,
  SRT walker, swapchain.
- Their reported status: Uncharted 1 in-game, seems stable; Uncharted 2 in-game, physics glitches;
  Uncharted 3 crashes often.

Still true from our 2026-10-16 code review: `run.sh` is the old Bloodborne launcher (`../CUSA03173`);
`src/runtime_fiber.c`'s Linux stack-bounds handling is a deliberate no-op; bbport is ahead of the fork
point with commits we have not looked at.

## Phases

### First goal: code maps (decided 2026-10-17)
Before anything else, map the code so every later session starts from a short, checked guide
instead of re-reading 900 files. The index is [`maps/overview.md`](maps/overview.md) (updated
2026-10-17 for the merged Linux fork; read from the code, nothing run). Next, the module maps it
lists, each in `docs/maps/`: loader, executable preparation, the OS-facing runtime, the game-facing
runtime, the GPU shim and our changes inside the vendored shadPS4 code, and launch, build and
packaging (Linux launcher included).
- Rules: every claim is checked against the code and marked read or measured; vendored code
  (`gpu/shadps4/`, `gpu/third_party/`) is mapped only by its `bbport:` changes; statements taken
  from the Bloodborne-era docs say "inherited from Bloodborne, not verified for Uncharted".
- The local Qwen model may draft module maps from a brief that carries the facts from the
  overview; Claude reviews every draft before it lands (on 2026-10-17, 7 of 41 Qwen maps and
  translations for the Craft apps were rejected and most others needed corrections).
- Done when the module maps exist and each has been checked against the code.

## Measured on the Legion Go (2026-10-18)

Legion Go (Ryzen Z1 Extreme, Radeon 780M, RADV PHOENIX, Mesa 26.2.0-devel), SteamOS 3.8, Desktop
Mode; `linux` at f0a21af; game CUSA02320 (APP_VER 01.00, package 01.02). Measured unless marked.

- **Build and tests:** builds in the craft-build container (LTO on). 5 C tests, 84 Python tests and
  the 7 GPU test targets pass (GPU tests in the container and on the host). The host needs four
  libraries from the build (libfmt, libZydis, libZycore, libminiupnpc), which `package.sh` bundles.
- **All three games boot** (run.py from source, scripted input):
  - Uncharted 1: collection menu, New Game, cutscenes and gameplay; saves and Continue work.
  - Uncharted 2: cutscene and gameplay.
  - Uncharted 3: the London bar opening; no crash in 5 minutes.
- **Frame rate:**
  - Steam Deck mode (anti-aliasing off, 30 Hz, 1280x800): a steady 30.0 FPS in all three
    (U1 frame pacing stddev 0.02 ms).
  - U1 gameplay at 1080p/60 Hz with anti-aliasing off: 59.6-60 FPS, with the GPU thread only
    2.5 % idle at about 1,100 draws per frame. The port's GPU translation is the limit, not the
    game's code.
- **First-run shader stutter:** cold-cache compiles per game:
  - U1 title: 113 in two 5-second windows, 193.5 ms (worst frame 50 ms).
  - U2: 489, 867 ms in total; worst stall 367 ms.
  - U3: 380; worst stall 533 ms.
  - Warm cache: 0 compiles, so the cache is reused.
- **FSR 3.1 renders gameplay as full-screen noise on RADV PHOENIX** (HUD fine, nothing in the log),
  at any window size and with or without the shadPS4 #5257 NaN port. Off and Deck mode are clean.
  Report kept privately.
- `bb-gpu-capabilities`: RADV PHOENIX lacks D32S8 blits, so live resolution changes fall back to
  the startup patch (a preset change needs a restart).
- The built-in controller is held by InputPlumber in Desktop Mode and SDL does not see it.
- **20 W is the design target** (decided 2026-10-18): at 30 W the Legion Go overheats and shuts off
  even with full fans, so it stays power-limited by design, and CPU spin takes power from the GPU.
- Memory is the tight part: in gameplay `bb-probe` held 2.0-2.8 GB resident on top of graphics
  memory taken from the same 15 GB, and with Steam and a browser open the free memory fell to
  1.5 GB.

### Phase 0: baseline on our machines (Strix Halo, then Legion Go)
- Build this branch on the Strix Halo (`bash build.sh`, in the craft-build container if the host
  lacks the libraries) and also try their released AppImage. Install the dump.
- Play each game about 15 minutes. Record per game: boots, menu, in-game, frame rate, visible
  problems, saved log. Compare with their reported status above.
- Done when all three games have a measured row on the Strix Halo, and the build steps for this
  machine are written down.

### Phase 1: done upstream (spacepepperoni)
Linux build, Linux launcher, relaunch on exit code 101, saves, AppImage, Steam shortcuts. Our part:
keep merging their `main` (and check it builds) before starting new work, so we never redo theirs.

### Phase 2: all three games playable
- Uncharted 3 first (theirs crashes often), then Uncharted 2's physics glitches, then the README's
  known problems (Uncharted 2 streaming crash `iSubMesh < m_numSubMeshInstances`, flashing shapes on
  some characters).
- Uncharted 3 crashes still open upstream: the Cartagena square, chapter 2
  ([Supermedo/uncharted_pc#303](https://github.com/Supermedo/uncharted_pc/issues/303)) and the
  Citadel, chapter 8 ([#230](https://github.com/Supermedo/uncharted_pc/issues/230)). The London bar
  crash is fixed upstream in [0.3.4](https://github.com/Supermedo/uncharted_pc/releases/tag/v0.3.4)
  and the MP3 audio crash in [0.3.5](https://github.com/Supermedo/uncharted_pc/releases/tag/v0.3.5);
  both are in our tree. Check their open issues and branches
  (`up/*`) before starting, so we do not duplicate a fix in progress.
- Done when each game plays 30 minutes without a crash on the Strix Halo, measured.

### Phase 3: launcher integration (install, update, per-machine settings)
The Craft launcher installs and updates the port on a fresh machine (Linux/SteamOS and Windows),
then detects the hardware and applies the best settings for it. Three target machines, by hardware:
a Legion Go handheld (Ryzen Z1 Extreme, 20 W Deck profile), a ROG Flow Z13 (Ryzen AI Max, Strix
Halo) and a Windows desktop with an RTX 3090 Ti.
- **Port and game dump.** The launcher downloads the port, then downloads the owner's own dump from
  the owner's private file server (WebDAV), checks it (sizes and hashes), and installs it beside
  the port. The game data never enters the repo, a release or a build artifact.
- **Sign-in:** the one hand step besides installing the launcher. It signs in to that server once,
  with an app password the owner creates, kept in the OS keychain (Windows Credential Manager, the
  Secret Service on SteamOS); never in a file or the repo. Server address, paths, user
  name and dump file names live in local config only.
- **Linux/SteamOS:** fetch the port's release, fetch the dump, pick the settings profile
  (Deck mode and the 20 W profile on the Legion Go; the Strix Halo profile on the Z13), keep the port
  updated.
- **Windows:** upstream already has Windows support (Supermedo/uncharted_pc#304; ours stacked on top
  is #444). Our part: build and package it for the launcher, and test it on the Windows desktop.
- **macOS: not in this phase** (decided 2026-10-20, after a read-only feasibility check; not
  measured). A Mac mini M1 with 8 GB is not a target as the port stands:
  - the port maps guest memory at fixed addresses (0x1000000000 to 0xfc00000000), a range macOS on
    Apple Silicon refuses for fixed mappings, under Rosetta 2 too; shadPS4 works around it game by
    game (shadps4-emu/shadPS4#4157, shadps4-emu/shadPS4#4694);
  - the port's runtime is Linux-only (`memfd_create`, `MAP_FIXED_NOREPLACE`, `arch_prctl` for guest
    thread-local storage) and has no macOS path;
  - memory and graphics: the port holds 2.0 to 2.8 GB resident plus graphics memory, too tight for
    8 GB shared; shadPS4 on macOS runs as x86-64 under Rosetta only and needs geometry and
    tessellation shaders that MoltenVK lacks.
  The Mac waits for a bigger Mac or another approach.
- **Hardware detection:** the launcher reads CPU, GPU, memory and OS and maps them to a settings
  profile per machine, with a manual override.
- Done when a freshly wiped Z13 and Windows desktop each go from installing the launcher
  to playing Uncharted 1 with no hand steps except installing the launcher and the one-time sign-in.

### Phase 4: 60 FPS on the Strix Halo
- Measure per game, find the bottleneck, tune. Done when each game holds 60 FPS in a defined test
  route.
- **Profile the GPU thread first (2026-10-18).** On the Legion Go the GPU thread is nearly saturated
  at 1080p/60 with anti-aliasing off. `BB_FRAME_STATS=1` plus `perf` (user space, our own process)
  on U1 gameplay: where the time goes per draw, then tune the hot path in our own code.
- **60 FPS: port-side performance, not a game unlock (the owner wants 60 FPS, 2026-10-18).**
  Reverse engineering on the Windows desktop (a reverse-engineering tool, static; output private) found nothing to
  unlock. All three games run at 60 on the PS4 and step their logic by measured frame time
  (U2: the CPU clock delta, capped at 100 ms per step; U3 built the same way). Only photo mode
  (all three) and one tweak setting in U2/U3 ask the video output for 30 Hz, and the port obeys.
  So 60 FPS is the port's own speed (see the profiling item above), plus:
  - an optional port setting to ignore a 30 Hz request (photo mode), off by default;
  - above 60 is not a goal: U3's cloth is inferred to be at risk, and the port's own comments
    mention a GPU hang near 80 FPS.
  Measured on the Legion Go (a flip-rate log line on a test build): in four U1 runs (menus,
  cutscenes, gameplay) and one U2 gameplay run, the games never changed the video-out flip
  rate from "every vblank". Photo mode was not reached (scripted menu input dropped presses), so
  its 30 Hz request is still read, not measured.
- **FSR 4 and DLSS (decided 2026-10-18).** Test both as anti-aliasing and upscaling options:
  - FSR 4: fetch its model files with `tools/fetch_fsr4_assets.sh` (the official source the
    script uses). Then compare it with anti-aliasing off on Uncharted 1 at 800p/60 and at 1080p:
    noise or not (FSR 3.1 renders gameplay as noise on RADV, issue Supermedo#351), FPS, frame time.
    Without the assets it falls back to FSR 3.1 today.
  - DLSS: test it through the hack project kept privately, on the Windows desktop with an RTX 3090 Ti (NVIDIA
    tests run there only, decided 2026-10-18).
  - Done when each has a measured row (picture correct or not, FPS, frame time) per machine it can
    run on.
- **First-launch shader preparation (decided 2026-10-16):** like God of War or The Last of Us
  Part I on Steam, show a one-time "preparing shaders" screen the first time a game starts,
  then play without compile stutter. Today shaders are translated and compiled while playing;
  the results are cached in `user/`, and the cache is thrown away when the graphics driver or
  card changes (log line "Pipeline cache isn't compatible with current system: rebuilding
  it"). Steps: (1) measure how bad the first-run stutter is per game; (2) record which
  shaders a full play route uses, keyed by shader hash; (3) at first launch, find those
  shaders in the player's own dump and compile them before the game starts, with a progress
  bar; (4) redo the step automatically after a driver change. Open question: whether the
  game's shaders can be found in the dump without running the game. The owner wants this
  (2026-10-18: shader preload), and the open question is now a
  job for the reverse-engineering tool on the Windows desktop: find how the games store and load their shader binaries, to read
  them from the player's own dump. Answer (reverse-engineering tool, 2026-10-18, details private): no shaders are in
  the executables; they are in the game data, built-ins in `.psarc` archives and materials in
  `.pak` files, each binary ending in an "OrbShdr" footer whose shader hash is the port's cache
  key. A streamed census of the dump (Legion Go, read-only, counts only) found 270,245 footers,
  34,010 unique shaders (U1 3,451, U2 8,689, U3 23,058; 518 in all three games, 670 in two or more), 268,553 of them in `.pak`
  files. So every shader can be listed without running the game; whole pipelines still need
  state that appears only in play, so the plan is to record a route and match hashes at first
  launch. Step 1 is partly measured on the Legion Go (above: up to
  489 compiles and a 533 ms stall on a cold first run). Do
  not ship translated or compiled shaders in the repository. Also check whether Steam's own
  shader pre-caching helps when this is added as a non-Steam shortcut (not known).

### Phase 5: the Legion Go
- Their Steam Deck mode is the starting point (also AMD, also SteamOS). Measure on the Legion Go,
  tune Deck mode or add a Legion Go profile.
- Started 2026-10-18: all three games boot and hold 30 FPS in Deck mode (measurements above).
  Still to do: the built-in controller (Game Mode, or an InputPlumber gamepad profile), long soak
  runs per game, and the AppImage on the device.
- Done when the AppImage runs on the Legion Go at a playable frame rate and a clean install is
  documented.

## Documentation

- Done: this roadmap; the old Russian one archived; the overview code map (`maps/overview.md`).
- Done 2026-10-18: the English translations of the Russian design notes and history (from Qwen's
  homework branch, reviewed; originals under `docs/ru/`), plus the `Research/` reports of 10-16.
- Done upstream: install instructions for Linux and the Steam Deck (their README: AppImage,
  one-line installer). To do: notes for our two machines once Phase 0 has measured them.

## Open items

- Which of bbport's 8 newer commits to take.
- Settled: the Linux launcher exists (`launcher/uncharted_linux.py`, spacepepperoni).
- Whether to send anything of ours to spacepepperoni, and how (the owner's call, one change at a time).
- In-game frame-rate choice (60/30) once the in-game menus land.
- `run.sh` is still Bloodborne's; drop it or point it at Uncharted once Phase 0 shows which
  launch path we use. (2026-10-18: `run.py` launches all three games on Linux.)
- FSR 3.1 noise on RADV (report above). Our side logs the driver at FSR start and warns off a
  validated list (fix/fsr-driver-log-1018). spacepepperoni's issues are turned off, so whether to
  send it as a pull request is the owner's call.
- `package.sh` stopped on Arch and SteamOS (the loader listed by path): fixed on our side
  (fix/package-sh-loader-1018); same question for upstream.
