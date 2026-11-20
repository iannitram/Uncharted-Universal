# Next Flint handoff: #454, saved settings in the CLI Play plan

Status: PREPARED, NOT DISPATCHED. The first local-model change (#426 / PR #452)
is under CI. Do not start this task against the current baseline.

## Product defect and why this is next

Issue: https://github.com/iannitram/Uncharted-Universal/issues/444
At audited main `69672033`, Settings persists display choices, but
`crates/cli/src/commands.rs:199-229` (`play_plan`) constructs
`settings: Settings::default()`. GUI `Ui::begin` at `app/src/main.rs:747-755`
forwards only `settings_file::extra`, so the typed display values never enter
that CLI plan. This is a source finding, not a measured game run.

This parent spans shared conversion, CLI wiring, GUI wiring and Windows
acceptance. It is too large for one local-model dispatch. The local task below
is deliberately only the CLI wiring after the shared conversion exists.

## Dispatch gates: coordinator / Terra responsibilities

1. Wait for controller PR #446 (issue #438); it modifies these same CLI lines
   and adds the persisted `button_prompts` lookup. Preserve that contract.
2. Coordinate relay PR #445 (#163) and resolution PR #447 (#432). Rebase only
   after their owners finish; never overwrite their work. #443 separately owns
   packaged loader selection. This task does not fix #443.
3. First land or review a prerequisite #453 from the hosted lane: move the
   existing display-to-run_env conversion into one shared core module and leave
   the app delegating to it. The shared module accepts a core-owned display-only
   value type (the six persisted string fields); do not move the app's `Stored`,
   mouse settings, Slint types or other UI dependencies into core. The app's
   compatibility wrapper must call the shared converter in production; do not
   add a second converter that only tests call. Do not write a second independent
   mapping in CLI.
   Shared API contract reserved for this handoff:

   ```rust
   // crates/core/src/launch_settings.rs
   pub struct Resolved {
       pub profile: &'static crate::profile::DeviceProfile,
       pub settings: crate::run_env::Settings,
   }
   pub fn load_for_launch(
       settings_file: &std::path::Path,
       detected: &'static crate::profile::DeviceProfile,
   ) -> Resolved;
   ```

   The coordinator must confirm the actual landed API and refresh this excerpt
   before dispatch. It must load fresh JSON on every call, select a valid saved
   profile or retain detected, preserve missing/malformed-file defaults, use the
   existing typed conversion for configured fields, and merge string-valued
   persisted `extra.BB_*` after typed settings. Keep UI-only/mouse fields out.
   An empty/missing settings object must preserve the current
   `Settings::default()` launch behavior: blindly converting an empty app
   `Stored` would introduce explicit `off` values and change profile defaults.
   Preserve per-game-suffixed BB variables unchanged where currently supported;
   do not invent suffix resolution in this slice. No broad settings redesign.
4. Refresh the exact base SHA and line numbers in this brief, and supply Flint
   the actual helper signature plus four-line implementation below. No history
   or repository scan is required by the local lane.

## Local task #454: two files, one production function

Allowed files:
- `crates/cli/src/commands.rs`: `play_plan` only, plus one import if necessary.
- `crates/cli/src/commands/tests.rs`: focused production-plan regression tests.

At the current base, `play_plan` already creates `vars`, computes `device`, and
collects explicit `BB_*` variables from `ctx.env` into `player_env`. After the
shared helper exists, the intended change is:

```rust
let resolved = uncharted_core::launch_settings::load_for_launch(
    &AppDirs::new(&vars).settings(),
    device,
);
// In the existing PlayPlan initializer:
profile: resolved.profile,
settings: resolved.settings,
```

Do not replace `player_env`, mutate the caller environment, invent another
settings filename/path, change runtime selection, modify relay pull/push or
change save directories. Explicit process `BB_*` values must remain the final
precedence layer in `run_env::build_env`. Keep PR #446's manual prompt setting
and explicit `BB_BUTTON_PROMPTS` override handling intact.

If any of those anchors have changed after prerequisite merges, STOP and report
the mismatch to the coordinator; do not search for an alternative architecture.

## Focused acceptance tests (same production plan, synthetic files)

Use `home`, `set`, and `fixtures::TempDir` in
`crates/cli/src/commands/tests.rs:1-36`. Get the file through
`AppDirs::new(&vars(&ctx).unwrap()).settings()`. Call actual `play_plan`, then
`plan.session(ctx.os).env(game).unwrap()`, which returns
`Vec<(String, String)>` at this baseline. Copy the existing `get` lookup closure
from `play_plan_uses_the_selected_save_slot_the_profile_and_the_players_variables`
(lines 132-160). Do not test a second fake env builder.

1. Create `TempDir::new("play-plan-saved-display")`, then `home(dir.path())`.
   Create the settings file's parent directory before writing. Write this exact
   current-format fixture (top-level string fields, not a nested display object):

   ```json
   {"device_profile":"desktop","frame_rate":"30","render_resolution":"1280x720","anti_aliasing":"fsr1","upscale_preset":"quality","frame_generation":"off"}
   ```

   Name the test `play_plan_applies_saved_display_settings`. Call actual
   `play_plan(Game::U2, &ctx).unwrap()` and its Session environment. Assert
   `plan.profile.id == "desktop"` and the following exact values, verified from
   current `app/src/vm/settings.rs:154-197` and `run_env::build_env`:

   | Key | Expected value |
   |---|---|
   | `BB_VBLANK_HZ` | `30` |
   | `BB_RESOLUTION` | `1280x720` |
   | `BB_FSR1` | `1` |
   | `BB_UPSCALER` | `off` |
   | `BB_UPSCALE_QUALITY` | `quality` |
   | `BB_FRAME_GEN` | `off` |

   FSR1 deliberately maps to `BB_UPSCALER=off` plus `BB_FSR1=1`; do not
   "correct" it to `BB_UPSCALER=fsr1`. This fixture does not yet prove the
   selected resolution reaches `BB_GAME_RES` or the actual render allocation.
2. Rewrite the same fixture between two calls on the same Ctx, changing only
   `frame_rate` from `"30"` to `"60"`. The first plan's environment must retain
   `BB_VBLANK_HZ=30`; the newly constructed plan must contain `BB_VBLANK_HZ=60`.
   This proves fresh reads, not only initial-load behavior or mutating an old plan.
3. Add fixture `extra: {"BB_VBLANK_HZ":"60","BB_RESOLUTION_U2":"960x540"}`:
   persisted extra must override typed frame rate (`60`), and the suffix key
   must remain `960x540` without being renamed or silently flattened. Then use
   `set(ctx, "BB_VBLANK_HZ", "120")`: the actual Session environment must
   contain `BB_VBLANK_HZ=120`, proving explicit process variables win. This is
   passthrough coverage; before dispatch, confirm #453/#447's actual supported
   per-game resolution policy and supply a separate exact assertion if needed.
   Do not infer that a suffix alone changes the runtime render size.
4. Before writing any fixture, capture the missing-file plan's `profile.id`,
   `settings` and Session environment. Rewrite the file with `null`, then `[]`,
   then malformed `{broken`; each new plan must match that missing-file baseline
   (and have `Settings::default()`). Repeat with `{"device_profile":"unknown"}`:
   its profile must remain detected and no new display override may appear.
   No panic or silently substituted profile.
5. Retain the controller-prompt regression tests from #446 and the relay plan
   tests from #445. This task is partial #444, not end-to-end acceptance.

The table above is the confirmed baseline mapping, not a claim about unmerged
#453/#447. Before dispatch, the coordinator must revalidate these assertions
against the landed helper and resolution policy. Do not guess `BB_RESOLUTION`
versus `BB_GAME_RES` precedence; the full render-path regression belongs to
#444d until #447's behavior is reviewed.

Run exactly these checks in the local dispatch (all added test names must start
with `play_plan_`, so the focused filter actually executes them):

```sh
cargo test -p uncharted_cli play_plan_
cargo fmt --all --check
git diff --check
```

Report the number of tests executed; zero matches is a failed verification.
The coordinator owns full builds, independent review and CI. No commit/push/PR
from Flint. Report the patch and checks, then stop for review.

## Remaining parent work and reporting

#444c: coordinator/hosted lane wires the GUI's real Continue, Play, Play again
and --play paths to the same shared policy; review inherited environment
precedence and unsupported settings honestly. #444d: synthetic real child-process
capture on Windows and the supported-size regression with #432, then hosted CI.
Parent #444 remains open until all acceptance is met; no game/performance claim.

Owner instruction 2026-11-20: any save-relay-related correction discovered or
made while preserving the launch/relay contract is fixed in this build first,
then reported in a linked `iannitram/save-relay` issue with cause, implemented
fix, tests and limitations. Do not claim a shared-core defect from integration
source alone. The coordinator handles that report after a concrete fix exists.
