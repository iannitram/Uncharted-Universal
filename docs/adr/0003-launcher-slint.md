# 0003: The launcher is all Rust, with a Slint UI

**Decided:** 2026-11-01, following ADR 0001 and totk-native ADR 0004 (same design for all our ports).

## Decision

`app/` is one Rust crate with a Slint UI, built for Linux (AppImage, full screen in Game Mode), Windows, macOS and Android. It links `uncharted_core` directly. Each page's state, focus order and button handling is a plain Rust view model in `app/src/vm/`, tested in `cargo test`; the `.slint` files only draw. Gamepad input comes from gilrs on desktop and Android key events on Android. It opens on the Continue screen (M10). Slint is used under its royalty-free licence; About shows its notice. The Python/Tk launcher stays until the Slint app has parity, then goes (M20).

## Why

The same launcher, look and controller flow as totk-native; generic modules are shared by copying with credit.
