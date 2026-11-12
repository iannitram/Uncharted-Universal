## Current handoff supersedes older status sections

All five foundation PRs #218-#222 are now merged and their issues are closed.
See `2026-11-06-foundation-handoff.md` in this folder for the live verified
state, PR204 reconciliation, stale worktrees and safe next steps. No agents
remain active. Older in-progress and paused sections below are historical.

## Active implementation batch — owner resumed and authorized 2026-11-06

Supersedes the older paused/documentation-only status below. Owner selected the
foundation-first batch and requested Luna builders plus fresh Luna PR reviews.
No merges are authorized by this batch request.

- Platform directory contracts: #213, PR #218; fresh Luna review passed.
- Game identity/revision contracts: #217, PR #219; fresh Luna review passed; keep supported
  revision tables empty until independently verified device evidence exists.
- Bounded package parsing/extraction: #215, PR #221, head f99a24e109034c0f4ca962c68f954cb193a1b78a; fresh Luna review and Windows CI passed.
- Content profiles: #214, PR #220; fresh Luna review passed.
- Literal patch handling: #216, PR #222; final zero-write/XML-normalization repair in progress.
- All code branches base origin/main ef27c86, independent of draft docs PR #204.
- Canonical checkout remains docs/universal-platform-audit-203, with this log
  untracked. Isolated shell worktrees live under /home/deck/.codex/worktrees/.
  Managed creation/attachment was rejected by the host workspace binding.
- Pure Rust workspace verification is required; native GPU prerequisites are
  unavailable and must not trigger uncontrolled submodule/SDK downloads.
- No game data or real dump tests; save-relay remains reference-only.

## Follow-up: issue dependency audit — completed 2026-11-06

Reviewed all 102 open issues using Luna-only sub-agents. Added the dependency
index to docs/code-map.md and pushed 0ff2bce3c43b87c278b7c8a7b44c366440adebcf to draft PR #204.
The table and Mermaid diagram encode 12 full-scope issue edges; #166's
location-only edge is separate. Closed references are scheduling history,
not active blockers. Roadmap contracts, external save-relay readiness and
optional iannitram/shadPS4#41, partial work and device acceptance are distinct.
No issue bodies/states/labels, runtime code, specs or roadmap were changed by
this follow-up. Validated coverage, live open/closed refs, no cycles, exact
table/diagram parity, local links, whitespace and unchanged final issue state.
Evidence: `$HOME/Archives/Uncharted-PM-2026-11-06/dependency-evidence/`.

# Current PM session state — 2026-11-06

Resumed at the owner's request and completed the project-management package.
All delegated audits, source research, document edits and review used Luna.

- Branch: docs/universal-platform-audit-203
- Pushed commit: 97a8f51f171697bce7dee82b8eda07d68dad589d
- Review: https://github.com/iannitram/Uncharted-Universal/pull/204 (draft, not merged)
- All 95 original issues have a recorded disposition in docs/audits/2026-11-06-issue-dispositions.md.
- Closed #66 after ten synthetic checks; Rust parity remains #206.
- Created seven source-linked implementation/feasibility issues #206–#212 and PM tracker #205.
- Clarified 13 existing issues: #11, #12, #22, #31, #37, #60, #70, #111, #127, #143, #153, #155, #163. Also refined new #209's license boundary/prerequisites.
- Current observed open issue count: 102. After-1.0 and device-dependent work was retained.
- PEERS includes 17 reviewed source repositories, selected-code license boundaries and historical notes.
- Code map and roadmap now tie proposed source imports to destinations, statuses and dependency gates.
- #209 requires #208 plus C ABI 3.01, memory 3.12 and thread/fiber 3.13; Windows OS shims 3.15 are not an Android prerequisite.
- #210 is standalone native Android driver bootstrap after #207, with explicit selected-driver verification; no silent fallback to stock after custom failure.
- Apple is gated by #211, then synthetic no-JIT testing #212. No blanket Bloodborne runtime import or game-derived data/annotations.
- Save-relay remains an external pointer until relevant support and interface are ready; no sync implementation was added.
- Maintained SPEC Musts/Won'ts and all runtime/source/test/workflow files are unchanged by this PM pass.
- Validation passed: 95-row coverage; seven issue schemas/labels/pinned-source checks; acyclic dependency graph; documentation links/anchors; cached and Luna-verified frozen source paths; whitespace checks; Luna review with both findings corrected.
- No game/dump/device work, SDK installation or full C/GPU build was done. Earlier 80 Rust/11 Python counts are from the initial audit, not repeated in this PM phase.

No code implementation/import has begun. Next work is review/merge of PR #204,
then choose ready implementation issues from the documented dependency graph.
The owner has not asked this chat to implement the ports or merge the PR.
The session log stays local/uncommitted; the PM documentation package is pushed.
Historical pause notes follow in the local session handoff for recovery context.

---

# PM session handoff — 2026-11-06

**Status: PAUSED at the owner's explicit request.** Stop all work until the owner
asks to resume. All three Luna sub-agents for this task were interrupted or had
completed. No build, install, test or GitHub mutation is left running by this task.

## Objective to resume

Act as project manager, **Luna sub-agents only**:
1. Audit open issues against former upstream Supermedo/uncharted_pc; close only
   genuinely fixed or obsolete issues, preserving remaining obligations.
2. Search for reputable, license-compatible peers with reusable source.
3. Create bounded import issues linking directly to that code.
4. Update PEERS, the Universal code map and related planning documents.

Five targets: Linux/SteamOS, Windows, macOS, iOS/iPadOS and Android from the
player's own dump. Own core/runtime code stays Rust; adopted engine code can
remain in its language behind a C ABI. No game files, binary-derived annotations,
generated game libraries, keys or private data in the repository/issues.
Save sync is only a reference to iannitram/save-relay until that project's
relevant platform support and integration contract are ready. No sync code or
new sync implementation tasks are authorized now.

## Repository and branch

- Canonical checkout: Uncharted-Universal (the single active source checkout).
- Branch: `docs/universal-platform-audit-203`.
- HEAD: `90eca479c911c2f47bc39a2da219060a7ffdb6e2` (already pushed before this PM session).
- Existing draft PR: https://github.com/iannitram/Uncharted-Universal/pull/204
- Production main snapshot: `ef27c862dd15f60e64c51a1d285b96dec1be13b9`.
- Former upstream main: `8b66edc0192ea246ca63b360857402a0aa55626d`, fetched as
  `refs/remotes/former-upstream/main`. It is already an ancestor of production
  main; there is no unmerged former-upstream commit batch to import.
- Runtime/source paths in this branch match production main; prior PR changes
  are documentation only. Recheck live refs/issue states on resume.
- Workspace was clean immediately before writing this handoff. A user-only
  README heading edit was present at session start and saved as a patch in the
  evidence pack, but was no longer present at pause. Do not restore that patch
  or assume it is still desired.

## GitHub changes already completed (do not duplicate)

PM tracker **#205** created, open/in-progress:
https://github.com/iannitram/Uncharted-Universal/issues/205

**#66 closed as completed**, after independent synthetic validation of the
original plain-ELF rejection. A closure comment links current parser, linker and
test code, and states that this is not Universal preparation/gameplay completion.
Measured: ten authored-fixture checks passed (equivalent SELF/plain ELF,
architecture/header bounds, truncation, oversized extent, both module-linker
callers, and filesz > memsz rejection). No dump/game data was used.
The separate Rust migration obligation is preserved in **#206**.

New issues with source permalinks, import boundaries, sizes and acceptance:

| Issue | Work | Dependencies |
|---|---|---|
| [#206](https://github.com/iannitram/Uncharted-Universal/issues/206) | Rust SELF/plain-ELF loader contract and parity | existing core scaffold; later linker/orchestration remains separate |
| [#207](https://github.com/iannitram/Uncharted-Universal/issues/207) | Pinned Android FEXCore/Turnip dependency build recipe | none; dependencies and synthetic artifact, not full game/APK |
| [#208](https://github.com/iannitram/Uncharted-Universal/issues/208) | Android ARM64 synthetic FEX guest harness | #207; adapt donor Linux/glibc harness to Android NDK/bionic |
| [#209](https://github.com/iannitram/Uncharted-Universal/issues/209) | Generic FEX guest-call bridge to Rust runtime ABI | #208 plus runtime ROADMAP 3.13/3.15 prerequisites |
| [#210](https://github.com/iannitram/Uncharted-Universal/issues/210) | Standalone Android Turnip/libadrenotools bootstrap | #207; no Slint shell prerequisite, app wiring later |
| [#211](https://github.com/iannitram/Uncharted-Universal/issues/211) | Apple AOT/interpreter/Metal feasibility and license gate | source/synthetic review; owner-run checklist, no dump inspection now |
| [#212](https://github.com/iannitram/Uncharted-Universal/issues/212) | Apple synthetic no-JIT CPU coverage | #211 |

Created labels `platform:android` and `platform:ios`.

Existing issues **#111, #127, #143, #153, #155 and #163** received current PM
notes/source evidence. #153 now names verification of the landed Windows
checkbox/slider fix; #143 names the remaining per-device Intel deny-list/test
work. #155 acceptance was corrected: RTX 3090 Ti can test DLSS upscaling without
DLSS frame generation; FG needs supported hardware. #163 explicitly holds
implementation for save-relay readiness and revalidation of old CLI assumptions.
All six updates were checked against fresh issue state first.

No other issue was closed. Initial audit snapshot: **95 open issues**. Two Luna
reports cover 40 older issues (excluding pinned #23) and 54 newer issues. Add #23
as governance/keep to make the complete 95-row ledger. Retain deferred #158 and
optional #198: deferral/option status alone does not make a goal obsolete.

## Evidence retained durably outside the repository

Local evidence pack: `$HOME/Archives/Uncharted-PM-2026-11-06/`:

- `pm-evidence/open-issues.json`: initial 95 issue bodies/labels/timestamps.
- `runtime-issue-audit.json`: 40 source-backed records, including #31/#35/#37
  excerpts and narrowed acceptance. These files are under `pm-evidence/`.
- `recent-issue-audit.json`: 54 records, concise gaps and suggested updates.
- `peer-import-research.json` and `peer-research-summary.md`: revised 17-peer
  survey, source/license links and reviewed recommendations.
- `created-issues.json`: actual #207–#212 bodies/URLs/dependencies as created.
- `issue-plan-reviewed.json`: root-rendered issue drafts; compare to the later
  revised research artifact before treating scope/wording as final.
- `issue-updates.json`: the six existing issue body/title updates already made.
- `close-66.md`, `verify-issue-66.py`: closure evidence and synthetic check script.
- `shad-import-evidence.json`: upstream source diffs for existing #31/#37.
- `snapshots.json`, `goals-and-rules.txt`, labels and PR snapshots.
- `peer-source-cache/`: prior source/tree evidence cached during this chat.

Original working artifacts remain at `/tmp/uncharted-pm-2026-11-06/`; use the
archive if temporary files disappear. Raw issue snapshots are local evidence;
do not blindly commit them or private historical comments to the repo.

## Peer conclusions to carry into docs

- New useful donor: `zenithblue-oss/shadps4-arm64` at
  `be6bc2e9c60799e071dd2fafa6216e8d80ec619c`. Small experimental project, not a
  mature drop-in engine. Its synthetic FEX tests and bridge files have explicit
  MIT markers; the direct Uncharted Android port credits the lineage. Some
  transitive/local headers (notably hle_trace) are GPL-2.0-or-later; do not label
  the entire transitive bridge MIT. Keep exact source and dependency notices.
- `revvv991/uncharted-android` at `847faf5a802253cf90664144765f0e5d12eb410e`
  provides the direct-port build recipe and license map. Exclude title-derived
  resolution recipes, byte patch XML, generated data and game assets.
- Actual driver library: `bylaws/libadrenotools` at
  `8fae8ce254dfc1344527e05301e43f37dea2df80` (BSD-2-Clause), with donor-pinned
  `JICA98/libadrenotools` at `7db3328e8d5e4762bcdf91c0279d32a52223899e` as a
  separately reviewed variant. The old K11MCH1/libadrenotools pointer is not a
  verified library repo and should be replaced in SPEC/ROADMAP/PEERS.
- Loader source: `include/adrenotools/driver.h`, `priv.h`, `src/driver.cpp`.
  A successful handle can still mean stock-driver fallback after hook failure.
  #210 requires explicit driver selection and actual identity validation;
  missing/invalid/mismatched custom drivers must not silently start stock.
  revvv reports that stock Adreno hangs in this game.
- Apple full engine adoption is gated by #211. Recompiler inputs/annotations,
  runtime hooks and app layout are Bloodborne-specific. Exclude GPL-3.0
  `gpu/bbnet20`; no blanket tree import. truwa remains learn-only (no root
  license, Rosetta route), Ryxm remains a scaffold rather than proven iOS port.
- Existing **#31** and **#37** already track small useful shadPS4 imports; do not
  create duplicates. Add exact source links to their bodies/code map:
  `699009c94932c64080391cf08d8131fc653ecee8` (ExpandImage) and
  `6752d8b19ec5e7f68302e7b5bebedf5447c78eac` (opcode decode/barrier).
  Root checked their source diffs and GPL-2.0-or-later headers. #31 needs semantic
  adaptation because this fork has no HasGpuValidData helper. Nothing is to be
  posted to shadPS4; upstream reads only. Existing #140/#141 also must not be
  duplicated.

## Next steps — not yet completed

1. Recheck branch/workspace, live fork/upstream heads, issue states and PR #204.
   Do not overwrite user changes. Keep Luna-only sub-agents.
2. Finish the compact 95-row issue disposition ledger with actual source links,
   #66 closure + #206 successor, and narrow remaining validation vs code work.
   No blanket closures for missing device evidence or after-1.0 goals.
3. Add precise import links/provenance/adaptation notes to existing #31/#37;
   preserve their regression/build acceptance. Consider similarly clarifying
   #11/#12 (legacy implementation exists, game checks remain), #22 (30 Hz
   decision settled; photo-mode measurement remains), #60 and #70 narrowed gaps.
4. Compare the final revised peer research to the actual created issue bodies.
   Fix any stale wording/license boundary (especially transitive GPL headers).
   All six new recommendations are Hard by current rubric. Do not duplicate
   issues or claim all are immediately unblocked.
5. Update PEERS.md, SPEC machine-readable peer pointers, ROADMAP and CHANGELOG
   with the chosen donors, source licenses, gating decisions and real issue IDs.
6. Finish docs/code-map.md: an exclusive Luna edit task was interrupted before
   any file changes landed. Required heading: `## Peer import plan` (new issues
   already link to this anchor). Map source -> proposed destination -> issue ->
   dependency -> status. Show desktop native CPU, Android FEX, Apple AOT plus
   interpreter/Metal, and desktop process vs mobile embedded execution. ABI
   version is still 0; app/android/apple directories and runtime build.rs are
   absent. Never mark proposed imports as present. Save-relay reference only.
7. Validate issue graph, source links, documentation links, unchanged spec
   Musts/Won'ts and preserved user edits. No source runtime change has been made;
   do not substitute unit/source checks for builds or device acceptance.
8. Have a Luna sub-agent review the final PM package, commit/push intended docs
   only, update/attach draft PR #204 and tracker #205, and report exact counts.
   No PM documentation commit/push or tracker completion has happened yet.

## Pause boundary

No implementation, SDK installation or real-dump/game work was done. The only
new measured runtime-adjacent result in this PM session is the ten synthetic
parser checks. Earlier 80 Rust/11 Python test results belong to the prior audit.
The handoff itself is a local, uncommitted file; do not infer it is on GitHub.
