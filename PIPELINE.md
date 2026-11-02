Stage: Dev Loop
Since: 2026-10-26 (notes refreshed 2026-10-31)
Kind: fork (upstream: Supermedo/uncharted_pc; also spacepepperoni/uncharted_Linux)
Spec: SPEC.md   Roadmap: docs/review-2026-10-24/ROADMAP-proposed.md (ours) and upstream docs/ROADMAP.md (history)   Code map: docs/code-map.md
Platforms: linux (1.0 target: SteamOS on the Legion Go), windows (builds, not a 1.0 gate), mac (research only)
Notes: Integration branch `main` (renamed from `linux` on 2026-11-01; GitHub's default branch; `upstream-main` is the copy of Supermedo's main). 1.0 checks need the owner's Legion Go and his own dump (needs-owner-device). Agent rules for our pipeline: AGENTS.fork.md. Upstream sync: Supermedo/uncharted_pc main is 26 commits ahead (0.3.10, 0.3.11, bbd1b3a); the merge into `main` conflicted in 10 files, so it is #164 (upstream-sync, Hard), taken before any other issue; PR #186 merges it. spacepepperoni/uncharted_Linux was last merged on 2026-10-28 (#133) and is 13 ahead. Shared saves (#163) follow 1.0, then 2.0 (Android, iOS, Mac; SPEC.md). Upstream's copy is the `upstream-main` branch: on 2026-11-01 the owner renamed `linux` to `main` and moved the copy off `main` (#164). All our work and fixes land on `main`, and issues are judged and closed against `main`.

Stages: Proposal -> Spec -> Roadmap -> Dev Loop -> Prototype / Beta.
What each stage means and who moves a project: the pipeline repo (private), PIPELINE.md.
The Project Manager updates this file when the stage changes and adds a CHANGELOG line.
