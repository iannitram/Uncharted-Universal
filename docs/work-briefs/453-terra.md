# Issue 453: shared launch settings

Stacked base: controller PR #446 head `a843d9fb3a27039856aed570890ef00fc73745d3` (`origin/codex/session-438-controller`), not main. Merge/rebase after #446 and coordinate relay #445 and resolution #447 before integration. Parent #444 remains open; this does not wire CLI #454 or GUI launch #444c.

`crates/core/src/launch_settings.rs` owns display-only `Display`, existing `profile_rate` / `profile_allows` and the old app conversion `run_settings`. `load_for_launch(&Path, &'static DeviceProfile) -> Resolved { profile, settings }` rereads JSON, uses a known saved profile or detected, and keeps default `run_env::Settings` for missing, invalid, non-object, or no nonempty display fields. String-valued `extra.BB_*` keys, including suffixed keys, override typed settings; `run_env::build_env` retains final process override precedence. The loader does not derive environment or resolve per-game suffixes itself. UI-only prompt/mouse fields remain in the app.

`app/src/settings_store.rs` uses the shared display parser; `app/src/vm/settings.rs` uses shared profile policy and its existing conversion wrapper delegates to core for current tests. It is not yet a production launch consumer: #454 must call the loader. App save format and controller prompt behavior are retained.

Verification on SteamOS Strix Halo: `cargo test -p uncharted_core launch_settings` passed 3 tests (missing/invalid data, display/profile fallback, fresh reads, extra and process precedence); `cargo fmt --all --check` and `git diff --check` passed. `cargo test -p uncharted-app settings` could not build: host lacks `pkg-config` for fontconfig; app tests and hosted CI remain owed. An initial formatting command failed while an intermediate edit misplaced a module file; corrected before the passing format check. No device or gameplay claim.

Coordinator review confirmed conversion-body parity with the prior app mapping and unchanged controller/mouse fields. `cargo clippy -p uncharted_core --all-targets -- -D warnings` passed independently. No save-relay defect was fixed in this extraction, so no shared-core report is claimed.
