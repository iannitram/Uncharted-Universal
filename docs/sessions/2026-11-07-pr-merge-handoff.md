# PR review and merge handoff — 2026-11-07

## User request and pause point

The user asked to review, fix, commit/push, and merge all PRs from the current build batch, then explicitly asked to pause and leave notes for the next development cycle. Work is paused at that request. Do not restart CI edits or merge additional PRs until the user resumes.

The active sub-agent `/root/build_59_platform_ci` was interrupted. Its current branch/worktree and remote commits are preserved. The primary worktree remains on `docs/universal-platform-audit-203`; `docs/sessions/` was already untracked before this note. No files in the existing session notes were changed.

## Merged PRs

- [#235 game resolution launch environment](https://github.com/iannitram/Uncharted-Universal/pull/235) — merged at `c5ab5a1e7d20a61083646d75e0d9ab825b3b61ee`.
- [#230 Apple feasibility](https://github.com/iannitram/Uncharted-Universal/pull/230) — merged at `ec5ef64f334fb04a45b32c91a8e556d50cd26b64`.
- [#234 crash-line reporting](https://github.com/iannitram/Uncharted-Universal/pull/234) — merged at `02c20c5951fa9092e784800858b95e754885acd4`.

The merges of #230 and #234 succeeded. GitHub CLI then reported failure deleting their local branches because the branches are checked out in dedicated worktrees; the remote PRs are merged. Do not remove those worktrees as cleanup without checking their state.

All three linked issues retain documented acceptance work (Apple probes/hardware, crash-line integration; game resolution verification). The user's instruction authorized merging these incremental PRs while leaving the issues open.

## Open PRs and latest heads

All four are still drafts. Their branches have commits pushed to GitHub.

| PR | Latest head | State and checks at pause | Remaining work |
|---|---|---|---|
| [#223 Rust artwork](https://github.com/iannitram/Uncharted-Universal/pull/223) | `bb5352286ace94c60d525bd44ac0ab2aabe44ccf` | Check workflow passed Linux Rust, Windows Rust, and Android. Linux package build `42416488676` was queued. | Wait for package build. PR body still notes Windows packaging/device acceptance incomplete. |
| [#236 benchmark report contracts](https://github.com/iannitram/Uncharted-Universal/pull/236) | `56d9e1b7f657a409b30ff387cd80c9946c4678e5` | Windows Rust and Android passed; Linux Rust and package build `42414008415` / `42414008480` were queued. | Wait for checks. Runtime lifecycle, game-pad wiring, GPU telemetry, and AMD handheld evidence remain explicitly out of scope/incomplete. |
| [#237 DLSS fallback regression test](https://github.com/iannitram/Uncharted-Universal/pull/237) | `7b94ba5f661830c3490f8182d4d5428f96403489` | Linux/Windows Rust and Android passed; Linux package build `42414180934` was queued. | Wait for package build. This tests settings fallback only; it does not inject NGX Vulkan query failure or prove startup continuation / RTX 3090 Ti behavior. |
| [#238 platform CI and release verification](https://github.com/iannitram/Uncharted-Universal/pull/238) | `305c7ca7df6907c654da0077c8378c0db1568293` | Latest check `42420114549`: macOS and Windows Rust passed; native Windows failed at link; Linux Rust, Android, and Linux GPU/C jobs were queued. | See Windows failure and proposed next step below. Native macOS GPU/C coverage remains an explicit issue gap. |

## #238 Windows CI history and next action

The #238 branch repeatedly exposed actual Windows portability/build issues in the new native test path. Fixes already pushed include:

- Windows Git long-path configuration and complete CLANG64 dependency install, including `spdlog`.
- Rust chroot toolchain invocation and diagnostics improvements.
- Cross-platform jump-buffer test stub changes, filesystem/environment setup portability in `test_upscaler_support.cpp`, logging source-location compatibility, `wbinvl1-test` include fixes, and a Windows `bb_longjmp` stub.
- Latest commit `305c7ca7` allows `motion-history-test` and `motion-shader-test` to link, but `wbinvl1-test.exe` still fails to link due to unresolved `runtime_fault_recover`, `bb_longjmp`, and `runtime_disabled_optimizations`, plus unresolved Windows `CreateFileMapping2` from `io_file.cpp`.

The full last failure log is `/tmp/uncharted-windows-job-128546519499.txt` (job `128546519499`, run `42420114549`). The sub-agent recommended a precise Windows-only exclusion/expected-skip for the standalone `wbinvl1` translator test because it pulls in the full `bbgpu` archive and a Windows runtime/API dependency chain unavailable to the host harness. The primary agent approved that approach and asked for an explicit reason and expected-skip gate while retaining other portable GPU tests and all three C harnesses. This exclusion was **not yet implemented** when work was paused.

When resumed, inspect the current #238 head and CI state first, then implement the Windows-only exclusion without weakening Linux coverage. Push and wait for a fresh run. If the Windows test step then passes, wait for remaining Linux/Android jobs. Keep #238 draft until current checks are complete and accurately document the issue's remaining native macOS gap.

## Merge/verification cautions

- The repo has no effective required-check gate: earlier `gh pr merge --auto` merged #235 immediately despite queued checks. Always refresh head SHA, review state, mergeability, and every check immediately before merging; use explicit `--match-head-commit` and do not use `--auto`.
- Several CI runs were cancelled because newer commits superseded them. Only use check results for the current PR head.
- The Build Linux package workflow is slow and runner-capacity constrained. At pause, #223/#236/#237 package jobs and multiple self-hosted Linux jobs were still queued. Do not treat queue state as success.
- Post-merge checks for #235 may have been cancelled by later main pushes; re-query current merged-main runs if validating post-merge health.
- Last primary checkout status: branch `docs/universal-platform-audit-203` tracks origin with no tracked changes; `docs/sessions/` was untracked before this note and must be preserved.

## Suggested resume sequence

1. Read this note and inspect current GitHub PR heads/checks; do not assume the snapshot above is still current.
2. Continue #238’s Windows-only `wbinvl1` exclusion and refresh CI.
3. Wait for #223/#236/#237 queued package and Linux checks; repair only current-head failures.
4. Review remaining PR diffs and issue acceptance notes, then perform the user-requested merge wave on green current heads.
5. Re-scan open PRs, merged commits, post-merge checks, local branches/worktrees, and repository status. Preserve `docs/sessions/` content.
