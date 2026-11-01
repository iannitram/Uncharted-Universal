# 0006: Mac, iPhone and iPad run a recompiled library drawn with Metal

**Decided:** 2026-11-01, by the owner: 1.0 devices "Full TotK parity", then the spec approval.

## Decision

Adopt bloodborne-native's route (bloodborne-runtime, -mac, -ios; GPL-2.0 per their headers, licence files checked at piece 8.01 before code comes in, credited): on the Mac, the launcher recompiles each game's x86-64 code from my dump into a native ARM64 library (no JIT, so the same library serves iOS), and the game draws through their Metal renderer. Their code is vendored under `apple/` and stays in its language; our Rust runtime builds for `aarch64-apple-darwin` and `aarch64-apple-ios`. The Mac launcher builds the iOS app and installs it over a cable with Xcode signing.

## Risks

- This is engine work for us, unlike totk-native: the recompiler and renderer are written for Bloodborne, and the collection is three executables. 8.01 measures how much is game-specific before 8.03 starts.
- iOS: a free Apple ID gives about 2 GiB of address space and re-signs every 7 days; the paid Apple Developer Program lifts both (a purchase: asked first, R34).
