# Uncharted-Universal

Build and play **Uncharted 1, 2 and 3 from your own decrypted PS4 Nathan Drake
Collection dump**, with target versions for **Linux / SteamOS, Windows, macOS,
iOS / iPadOS and Android**.

The intended app checks the dump, prepares the selected game's platform build,
installs it on your own device, and keeps your saves together through
[save-relay](https://github.com/iannitram/save-relay).
Linux and Windows use the original x86-64 game code with the port's runtime and
Vulkan renderer. Android uses an ARM64 runtime around FEXCore. The approved
Apple route is a library recompiled from your dump on a Mac, with Metal rendering
and no JIT on iOS. These are the target architecture and scope; the five-platform
application is still being built.

No game files, generated game libraries, firmware or keys are distributed here.
You supply the supported dump revision. This project is not affiliated with Sony
Interactive Entertainment, Naughty Dog or Bluepoint.

## Current implementation

Audited 2026-11-06; full evidence and limits: [platform readiness audit](docs/audits/2026-11-06-universal-platforms.md).

| Platform | Present in this repository | Universal work still needed |
|---|---|---|
| Linux / SteamOS, x86-64 | Legacy C/C++ runtime, Python launcher, Vulkan renderer and AppImage packaging | Rust prepare/play integration, new app and device acceptance |
| Windows, x86-64 | Legacy runtime, MSYS2 CLANG64 build and packaging | New app integration, full build and device acceptance |
| macOS, Apple silicon | Rust workspace and target model | Recompiler/CPU adaptation, Metal renderer, app packaging and device acceptance |
| iOS / iPadOS, ARM64 | Rust runtime target-check configuration | Embedded runtime, no-JIT execution, signed app and device acceptance |
| Android, ARM64 / Adreno | Rust runtime static-library cross-build configuration | APK, FEX bridge, Turnip integration and device acceptance |

The Rust CLI commands, including `prepare` and `play`, currently report
**not built yet** and exit 2. Passing Rust tests or compiling the static runtime
library does not produce a playable application. `app/`, `android/` and `apple/`
are planned directories, not implemented integrations.

## Preparing a version from a dump

The planned flow is: choose your dump, validate its game and executable revision,
choose a target, prepare the game with that target's runtime and renderer, then
install and launch it. The approved build hubs are a Linux machine for Linux and
Android, a Windows machine for Windows, and a Mac for macOS and iOS. Apple builds
need the Apple toolchain and the iOS app needs signing; this is not a promise that
one host can cross-build every target.

The dump and generated game code stay on your own devices. The home-network
installation and save-relay flows are planned; they are not current services.
Supported revisions must be verified per game and platform before preparation.

## Save syncing

[iannitram/save-relay](https://github.com/iannitram/save-relay) is the shared
save-sync project for all five target platforms: Linux, Windows, macOS,
iOS/iPadOS and Android. Uncharted-Universal will integrate it when its relevant
platform support and integration interface are ready. For now this is a
dependency reference; no sync library or service is attached here. The intended
flow is to pull saves before play and push them after play, using the same saves
across devices. Readiness is tracked in save-relay itself; this is not a claim
that it currently supports all five platforms.

## Build and inspect the current code

```sh
git clone --recurse-submodules https://github.com/iannitram/Uncharted-Universal.git
cd Uncharted-Universal
cargo build --workspace
cargo test --workspace --all-features
```

Those commands build and test the Rust components. They do not build the legacy
GPU library or convert a dump. For the existing Linux path, read
[BUILDING_LINUX.md](docs/BUILDING_LINUX.md); for Windows, read the
[CLANG64 build instructions](packaging/windows/README.md). Both still use
`bash build.sh`. The [inherited Linux README](docs/original-readme/README-linux-fork.md)
is preserved for its launcher instructions, compatibility notes and credits;
its upstream downloads are separate from Universal's planned platform builds.

## Plan, peers and contributions

- [SPEC.md](SPEC.md): the approved five-platform product and acceptance criteria.
- [ROADMAP.md](ROADMAP.md): implementation stages; [PIPELINE.md](PIPELINE.md): current stage.
- [docs/code-map.md](docs/code-map.md): existing, stub and planned modules.
- [PEERS.md](PEERS.md): platform-by-platform head starts, pinned source snapshots,
  adoption candidates and limits. Start here before writing a new platform layer.
- [Platform readiness audit](docs/audits/2026-11-06-universal-platforms.md): code gaps and verification.
- [AGENTS.fork.md](AGENTS.fork.md): contribution rules; integration branch is `main`.

Built on [Supermedo/uncharted_pc](https://github.com/Supermedo/uncharted_pc),
[spacepepperoni/uncharted_Linux](https://github.com/spacepepperoni/uncharted_Linux),
[bbport](https://github.com/deadinside28/bloodborne_pc) and
[shadPS4](https://github.com/shadps4-emu/shadPS4). Existing source credits and
third-party notices remain in their files and in the
[preserved Linux README](docs/original-readme/README-linux-fork.md).
The render-resolution and movie-scaling adaptations credit
[BilalReloaded's port](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port).

GNU GPL v2 or later ([LICENSE](LICENSE)); third-party components retain their own
licenses. Listing a peer is not blanket approval to copy its code.
