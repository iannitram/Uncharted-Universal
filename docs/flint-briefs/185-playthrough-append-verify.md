# #185: after-run step calls playthrough::append (likely already done)

Issue: https://github.com/iannitram/Uncharted-Universal/issues/185
Status: verify, then reply. Write code only if the test fails. Owner closes.

## What the issue says

When a run ends (exit, crash, or credits), the Slint app calls `playthrough::append` with that run's record. Done when: after a test run ends, one new record is in `<logs>/` with the right `Ended` value.

## What the code already does

- `app/src/vm/after_crash.rs:71`: `after_run` runs the after-run step for a game whose run ended.
- `app/src/vm/after_crash.rs:88`: `logs::append(&ctx.logs, &run)`. A failure is printed at line 89, not raised.
- `crates/core/src/logs.rs:631`: the doc comment of `append`. It writes one line to `<logs>/playthrough.jsonl`.
- `app/src/main.rs:684-690`: the call site. The after-run step runs for `game` here.
- `app/src/vm/after_crash/tests.rs:185`: test `the_run_is_added_to_the_playthrough_log_and_marked_seen`. Its module doc at line 3 names #185.

## Steps

1. Confirm the call site: `git grep -n "playthrough" -- app crates`.
2. Run `cargo test -p uncharted-app after_crash`. The first build of the app crate is slow.
3. If the test passes: draft a reply for #185 that names the test and the call site at `after_crash.rs:88`. Do not post or close it. The owner posts and closes.
4. If the test fails: the failing assertion names the gap. Fix it in `app/src/vm/after_crash.rs` at the append call, then rerun the test.

## Commands

- `cargo test -p uncharted-app after_crash`
- `bash build.sh` (before any PR, and only if step 4 changes code)

## Done when

- The test result is recorded, with the pass or fail output.
- Either a reply draft exists, or a fix PR is open with the test passing.

## Out of scope

The Mac and Windows runs, the `playthrough.jsonl` format, and the crash-help screen.
