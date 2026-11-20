# Issue 431: Windows-style save layout audit

Base `origin/main` 69672033bf92d102573862a66ac800082ca59a77. Peer reference [spacepepperoni/uncharted_Linux #84, merge e3a92723fa68de8de6fb517fb1a61f3863986ae9](https://github.com/spacepepperoni/uncharted_Linux/pull/84) describes slot 1 shared under `savedata/1/<title ID>`, with later slots separate. #360 is closed; runtime savedata is Rust on this base.

## Current paths and behavior

- `crates/runtime/src/savedata/store.rs:297-303` computes the unmapped root `<user>/savedata/<user id>/<title id>`; tests `store_tests.rs:85-90` cover user 1 and configured/explicit title. That establishes the Windows-style path, not a hardcoded title ID.
- `crates/core/src/saves.rs:9-11,222-238,480-493` makes default slot 1 `<user>/Uncharted N`, slots 2–10 `<user>/Save slots/Uncharted N/Slot n`, and Linux launch mapping `SAVEDATA0N=<selected relative slot>`. `crates/core/src/launch.rs:145` installs that mapping for non-Windows, with player environment taking precedence. `split_saves` moves shared entries into per-game folders but is currently only called by its tests.
- `saves.rs:185-208`: each manifest uses `saves_from: launcher`, includes only its game's prefix, and `relay_saves_dir` resolves the selected slot. `crates/cli/src/commands.rs:199-203` and `app/src/main.rs:1117` consume it. The manifest has no path of its own; the supplied selected slot folder must follow whichever layout is actually active.
- `saves.rs:333-365,398-467` `save_entries`, `saves_in`, `used`, `details`, `copy` and `clear` currently assume a folder holds one game's entries. In a shared slot-1 folder they would count, copy or delete other games' saves. `app/src/load.rs:23-24` also reads all saves in a slot without game filtering. `app/src/pages_io.rs` uses `slot_dir` for operations. Shared layout therefore needs end-to-end game-prefix isolation, not only a path change.
- `app/src/main.rs` and CLI launch/relay paths overlap current PR #445; app launch/controller work also overlaps #446/#447. A setting and its activation are not present at this base; there is no safe existing input that selects the requested layout.

## Outcome and required follow-up

No independently useful production-wired core-only layout slice was found under the overlap constraint. An unused layout enum/path helper would be speculative; silently reading manually edited state would enable a shared folder before UI, safe migration and game isolation are complete. Do not change the default or add dead code.

Coordinate a non-overlapping integration after the active app/relay PRs land: persist an explicit Files-page layout and verified title ID, reject invalid/unknown/ambiguous IDs, and arrange collision-safe non-destructive migration in both directions. Resolve slot 1 using the selected layout; retain slots 2–10 and player overrides. Audit and update every list, details, copy, delete, save-card and relay consumer to filter by game when sharing a folder. Keep the manifest per-game include and supply the selected slot directory for both layouts. Define switching semantics when copies conflict or migration is interrupted before changing persisted state; do not overwrite saves.

Use synthetic tests for U1/U2/U3 in one shared folder, each game's visibility/copy/delete isolation, per-game manifest inclusion and selected slot 1 versus 2–10, both layouts, default unchanged, bad/absent title IDs, collisions, interrupted migration, player override precedence and Windows/Linux roots. Tests must verify actual launch and relay consumers after integration, not only a path helper. No game files or gameplay claims; no production edit or build performed in this audit.
