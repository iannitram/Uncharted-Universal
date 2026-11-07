# Universal build readiness audit — 2026-11-06

Scope: source and documentation at `ef27c862dd15f60e64c51a1d285b96dec1be13b9`,
plus live peer repositories. Tracker: [#203](https://github.com/iannitram/Uncharted-Universal/issues/203).
Findings below are read from code unless explicitly marked measured. This is a
platform-readiness audit, not an exhaustive security review or a game playthrough.

## Goal and current position

The owner reaffirmed the existing [spec](../../SPEC.md): prepare Uncharted 1, 2
and 3 from the player's own decrypted collection dump for Linux, Windows, macOS,
iOS/iPadOS and Android. A build hub needs each target's toolchain; selecting a
target does not make any host capable of building every platform. In particular,
the Apple builds need a Mac and the iOS app needs signing.

The repository has a legacy Linux/Windows execution path and a partially
implemented Rust replacement. It does not yet have an end-to-end universal
prepare/build/install/play flow. Existing platform enums, Rust compilation and
unit tests do not establish that the games run on those targets.

| Target | Source present here | Missing before a usable universal build |
|---|---|---|
| Linux / SteamOS, x86-64 | `build.sh`, `run.py`, `src/`, Vulkan renderer, AppImage packaging | Rust preparation/launch integration, controller-first app, install/update flow, renewed game acceptance |
| Windows, x86-64 | MSYS2 branch of `build.sh`, Windows CMake targets, `packaging/windows/` | Universal app integration, full runtime/GPU build verification on Windows, game acceptance |
| macOS, Apple silicon | Rust OS enum and workspace; no adopted Apple engine | Guest CPU/AOT integration, Metal renderer, app package, device validation |
| iOS / iPadOS, ARM64 | Rust target check configuration only | Signed app, in-process runtime, no-JIT execution, renderer, lifecycle/input/memory validation |
| Android, ARM64 | Rust static-library cross-build configuration only | APK project, FEX guest bridge, native runtime/renderer, Turnip integration, lifecycle/input/device validation |

## Findings, in dependency order

### U1 — The universal commands cannot prepare or launch a game

[`crates/cli/src/main.rs`](../../crates/cli/src/main.rs) returns exit status 2
and “not built yet” for every recognized command. The library is similarly
incomplete: `prepare::{load_image, prepare}`, `link::link_modules`,
`recipe::recipe_for` and `launch::play` are stubs. The five OS values and the
hub ownership table are implemented, but the actual target build steps are not.

**Next:** ROADMAP stages 1 and 2. Complete one fake-dump preparation contract and
wire the CLI into it before presenting target selection as a working feature.
Retain the legacy path until its replacement passes equivalent checks.

### U2 — Revision detection is not a supported-build recipe

[`dump::check`](../../crates/core/src/dump.rs) currently reports 1.02 as
unsupported and expects 1.00, while [`games::SUPPORTED`](../../crates/core/src/games.rs)
contains empty revision lists and `games::check_revision` is a stub. Checking
four ELF magic bytes also does not validate its architecture or program headers.
The legacy [`scripts/prepare.py`](../../scripts/prepare.py) performs stronger
x86-64 ELF parsing. Some [peers](../../PEERS.md) explicitly use 1.02 recipes.

**Next:** finish the bounded ELF parser and use one verified revision/recipe
table across check, prepare and each target. Record executable compatibility
per game and target; do not loosen the gate from a peer's claim alone. This
is a blocker to reliable dump conversion, not evidence that our port supports
all revisions a peer supports. See ROADMAP 1.02–1.07 and #188.

### U3 — Changing the compiler target cannot port the runtime

[`src/probe.c`](../../src/probe.c) uses x86 assembly, Linux signal register
layouts and Linux syscalls; [`src/runtime_thread.c`](../../src/runtime_thread.c)
sets x86 guest TLS; [`src/runtime_fiber.c`](../../src/runtime_fiber.c) switches
x86 stacks and floating-point state. Windows has explicit adaptations. There
is no `android/`, `apple/` or `app/` implementation in this snapshot.
[`crates/runtime`](../../crates/runtime/src/lib.rs) has ABI version 0 and no
ported guest C symbols; its savedata metadata helper is not a guest runtime.
Its Cargo manifest does not yet build or link `gpu/`.

**Next:** preserve working desktop behavior through ROADMAP stage 3, then
adopt the Android guest bridge and evaluate the Apple recompiler/CPU/Metal
pieces in stages 7 and 8. Validate guest calls, TLS, faults, fibers, memory
mapping and GPU output together on each architecture.

### U4 — Apple needs an execution route distinct from Android

[`launch::Runner`](../../crates/core/src/launch.rs) is documented as a child
process. That contract needs an in-process implementation for the iOS game app.
The Android FEX/JIT route is not the approved no-JIT Apple route (ADR 0006).
The Bloodborne peers provide working code to study, but their recompiled
functions, game annotations and rendering hooks are not Uncharted support.
Their iOS proof of concept also retains an interpreter for uncovered code.

**Next:** ROADMAP 8.01 must measure adaptation for all three executables before
adoption. Specify the embedded runner, app lifecycle, controller input, signed
generated library, uncovered-instruction behavior and device memory budget.
Keep the Rosetta-based Uncharted Mac peer as a comparison, not evidence that
the no-Rosetta/no-JIT milestone is done. Reconfirm signing and memory limits
against the intended devices; an account tier alone is not a measured fix.

### U5 — Installation, hub transfer and save sync are interfaces only

[`bundle`](../../crates/core/src/bundle.rs), [`hub`](../../crates/core/src/hub.rs)
and [`launch::play`](../../crates/core/src/launch.rs) are stubs. There is no
implemented discovery/pairing/transfer path, verified installer, save-relay
adapter or prepare/update rollback. Enabling the `hub` feature adds stub code,
not a running service. #189 covers the separate home-network game-data copy.

**Next:** ROADMAP stages 4, 5 and 9. Specify the artifact boundary before serving
files: engine/tool binaries are separate from the player's dump-derived boot
images and AOT libraries. Those generated files also contain game code and
must stay private to the player's devices, never in public release artifacts
or CI uploads. Tests need fake data, malformed manifests, wrong targets,
tampering, interrupted installs and save preservation.

### U6 — CI checks components, not five complete applications

In [check.yml](../../.github/workflows/check.yml), Linux and Windows run Rust
workspace checks on self-hosted runners. macOS and iOS jobs are dispatch-only;
iOS uses `cargo check`, and Android builds only `uncharted_runtime` as a static
library without an NDK-backed APK or GPU link. [build.yml](../../.github/workflows/build.yml)
packages the legacy Linux application. There is no complete Apple/Android
application release job or cross-platform boot smoke test.

**Next:** implement target application builds and synthetic boot checks in
ROADMAP 3.01, 6.01, 7.02 and 8.02, then attach actual device evidence. A passing
workspace job must not be labeled a passing game port.

### U7 — Documentation and peer records obscured the gap

Before this audit, the README identified the project as Linux-only;
PIPELINE.md deferred mobile/Apple to 2.0 despite the newer approved spec;
AGENTS.fork.md still targeted `linux`; the code map described obsolete CI
runners; PEERS.md omitted the recompiler/CPU split and direct Mac launcher.

**Addressed in this documentation change:** universal entry points, current
status, integration branch, CI descriptions and a dated platform peer survey.
The spec's Musts and approved platform routes are unchanged.

## Peer adoption order

1. Keep Supermedo and spacepepperoni as the Windows/Linux lineage; compare
   independent Windows fixes rather than merging unrelated engine histories.
2. Study revvv991's Android build script and `src/guest_cpu_fex.cpp`, then
   compare eladharels' guest-fiber and Android integration.
3. For Apple, inspect bloodborne-recomp, bbcpu and the runtime's Metal backend
   together, followed by the Mac packaging and iOS shell. Verify instruction
   coverage and game-specific assumptions before implementing the route.
4. Learn from the direct Uncharted Mac launcher and the Uncharted iOS scaffold
   without confusing Rosetta or a successful stub build with the target goal.

Pinned heads, evidence links, maturity and reuse boundaries are in
[PEERS.md](../../PEERS.md). No peer code was imported by this audit.

## Verification and limits

Measured on a Linux x86-64 host on 2026-11-06:

- `cargo build --workspace`: passed.
- `cargo test --workspace --all-features`: 80 tests passed, none failed.
- `python3 -m unittest discover -s tests -p test_prepare.py`: 11 tests passed.
- `uncharted-universal prepare u1`: prints “not built yet”, exits 2, without
  opening any real dump.
- Peer repository heads and source paths were checked live on GitHub.

The full C/C++ build was not run: this host session has no `pkg-config`, so its
Vulkan/SDL dependency discovery cannot run. No SDKs were installed, no real
dump was read, and no game, mobile package, Apple build, Windows binary or
on-device performance was validated. Peer performance and playability remain
their maintainers' reports. Unit-test success is not platform acceptance.
