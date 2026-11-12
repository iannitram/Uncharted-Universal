# Uncharted Universal foundation batch: handoff

Verified live at 2026-11-07T04:50:50.604702+00:00. Owner requested a handoff; stop implementation here.
No active sub-agents remain. Do not treat the older paused or in-progress logs
as the current status. This handoff supersedes those status sections.

## User scope and constraints

The owner chose the foundation-first roadmap batch: platform paths, game/revision
contracts, package parsing/extraction, content profiles, and literal patch handling.
Only Luna sub-agents were dispatched by this chat, including fresh independent
reviewers. Concurrent changes from another session were preserved and reviewed;
some later reviews/merges happened outside this chat. Do not claim this chat
performed those later merges or final validation runs.

- Canonical repo: /home/deck/dev/Uncharted-Universal.
- Integration branch: main. Former upstream: Supermedo/uncharted_pc.
- Read AGENTS.fork.md, SPEC.md, ROADMAP.md, PEERS.md and docs/code-map.md.
- Own platform/core/runtime/launcher code is Rust; adopted graphics/Android/Apple
  code can retain its language behind a C ABI.
- No game/dump/firmware/keys/generated game code or binary-derived annotations.
  Fixtures must be synthetic. Never infer supported revisions or title regions.
- save-relay remains reference-only until its interface/platform support is ready.
- Preserve hooks; commit as iannitram <4624371+iannitram@users.noreply.github.com>.
  Nothing is sent to shadPS4; upstream code PRs need explicit owner authorization.

## Confirmed current GitHub state

Remote main: `5812afc708b63fa7cbc2df2d965922aff2971198`.
All five implementation PRs are MERGED. Issues #213, #214, #215, #216 and #217
are CLOSED. The corresponding remote feature branches have been deleted.

| PR | Work | Final PR head | State |
|---|---|---|---|
| #218 | Implement universal platform directory paths | `871e64afe80c2a3b9cfaa612faeef376a71b096a` | MERGED |
| #219 | 1.03: implement game identity and revision gate contracts | `29000827699d424d22c30f36c697fade277de6ca` | MERGED |
| #220 | Implement offline content profile contracts | `4ffad3c5998a190bb02c21637fb0c0093bb16842` | MERGED |
| #221 | Implement bounded package analysis and safe extraction | `296796533688d0d7b6c09840a4c5073698552860` | MERGED |
| #222 | Implement safe third-party literal patches | `bc209dfdcc48b4ecf253c9b0cb21bc975d99562f` | MERGED |

Links: https://github.com/iannitram/Uncharted-Universal/pull/218,
https://github.com/iannitram/Uncharted-Universal/pull/219,
https://github.com/iannitram/Uncharted-Universal/pull/220,
https://github.com/iannitram/Uncharted-Universal/pull/221,
https://github.com/iannitram/Uncharted-Universal/pull/222.

Every implementation PR's latest recorded Linux Rust, Windows Rust, Android and
legacy package-build checks succeeded. macOS and iOS checks were skipped by the
workflow. These results are read from live GitHub, not new local test runs in this
handoff. They establish neither Apple/mobile app execution nor real-game acceptance.

## Implementation and review outcomes

- Platform lookup implements desktop folder contracts and rejects mobile env lookup
  before touching the lookup callback; mobile apps supply their directories.
- Game identity/revision gate fails closed. Production SUPPORTED revision tables
  remain empty and title-region mapping still requires verified evidence.
- Package extraction checks bounded metadata/body ranges and safe destinations,
  handles empty-file overlays, and stages replacement files before rename so an
  existing hardlink does not redirect a truncation. Later amendments improved
  nested file/directory collision checks; read merged main for the final version.
- Content profiles provide typed SFO conversion, exact 28-byte binary and JSON
  metadata. They are not wired to a runnable app/runtime workflow.
- Patches cover build identity, literal encoding, whole-patch refusal, loadable
  segments and loader-compatible records. The handwritten XML parser was replaced
  with pinned roxmltree 0.21.1 under its compatible MIT option, with strict numeric
  reference validation and resource guards. PEERS documents the library.
- Subsequent external reviews caught stack/depth protection and boundary-test gaps.
  Final PR222 review PASS at bc209df records pre-parse deep-nesting refusal on a
  small stack, hard-coded depth/size/node/write boundaries and mutation checks.
  Read the final merged implementation; local 4ee5d1f predates those later repairs.

Final review examples:
- PR218: https://github.com/iannitram/Uncharted-Universal/pull/218#issuecomment-6696603073
- PR219: https://github.com/iannitram/Uncharted-Universal/pull/219#issuecomment-6696541363
- PR220: https://github.com/iannitram/Uncharted-Universal/pull/220#issuecomment-6696598855
- PR221: https://github.com/iannitram/Uncharted-Universal/pull/221#issuecomment-6697854926
- PR222: https://github.com/iannitram/Uncharted-Universal/pull/222#issuecomment-6696938281

This chat's last combined local run passed 117 tests plus fmt, build, both clippy
configurations and Windows-target check, but that run predates the final parser
replacement and later amendments. Do not describe it as validation of current main.
Current main's final acceptance evidence is the live CI/review record above.

## Remaining documentation PR

PR204 is OPEN (draft in this session's earlier state; refresh before changing it):
https://github.com/iannitram/Uncharted-Universal/pull/204.
Branch `docs/universal-platform-audit-203`, pushed head
`be8f1b6e244f3e0daffe80d77dcfc6f17070c744`.
It contains the Universal audit, peer plan, issue dispositions, dependency index
and foundation issue/PR table. Its latest configured checks passed, but it is
based on the older main. It must be reconciled against the newly merged code,
PEERS and code-map rows before any merge. Preserve the merged module statuses
and parser safety notes; do not overwrite them with historical stub/audit rows.
Earlier audit counts are dated snapshots, not current backlog totals.

## Local checkout and preservation

Canonical checkout is still `docs/universal-platform-audit-203` at be8f1b6, with
only untracked docs/sessions/. origin/main has been refreshed to 5812afc.
The implementation worktrees are stale; all their current local branch commits
were verified ancestors of remote main:

- /home/deck/.codex/worktrees/uncharted-foundation-platform — 9f7119d
- /home/deck/.codex/worktrees/uncharted-foundation-games — 2900082
- /home/deck/.codex/worktrees/uncharted-foundation-content — d8cc6bb
- /home/deck/.codex/worktrees/uncharted-foundation-pkg — f99a24e
- /home/deck/.codex/worktrees/uncharted-foundation-patches — 4ee5d1f

All are clean. They were shell-created because managed worktree creation and
attachment were rejected by the host's workspace binding. They are outside dev/.
Do not re-submit their commits or restore their stale code over current main.

The temporary integration checkout is clean at
`3514cac96b614770bfabc23af692f80f75a96688` on verify/foundation-batch. Its cherry-pick
commit IDs are local-only verification history, not pending product work; it
predates final merged fixes. Archive it rather than merge it.

Durable snapshot: /home/deck/Archives/Uncharted-PM-2026-11-06/foundation-handoff.
It contains this handoff, the live PR/check/comment snapshot, prior local session
logs, worktree inventory, and an incremental Git bundle of the local integration
and documentation branch history not reachable from current main. The bundle
requires this repository's existing main history. No worktrees were deleted.
The /tmp directory no longer has the earlier local test logs; do not cite them
as still available. Older PM evidence may also exist under
/home/deck/Archives/Uncharted-PM-2026-11-06/.

## Safe next steps

1. Read this handoff, refresh GitHub/main, and preserve untracked session logs.
2. Reconcile PR204 with merged main; inspect current issue dependency records and
   keep historical audit evidence separate from current implementation status.
3. After preserving any newly discovered local work, return the canonical checkout
   to main with a fast-forward-only update and remove stale temporary worktrees.
   Do not reset or delete work blindly. The integration branch is verification only.
4. Run an appropriate combined workspace check on current main if making further
   changes; retain the distinction between library checks and device/game tests.
5. Choose the next dependency-ready implementation batch from the updated code map
   (likely remaining linking/preparation contracts, #206 loader parity and #188 dump
   refusal). Re-read live issue scope first. Do not invent supported revisions or
   start save-relay integration ahead of its external gate.

No merge, cleanup, code change or new test run was performed as part of this
handoff request. Fetching refs and writing local recovery artifacts were the only
local state changes.
