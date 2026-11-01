Stage: Dev Loop
Since: 2026-10-26 (notes refreshed 2026-10-31)
Kind: fork (upstream: Supermedo/uncharted_pc; also spacepepperoni/uncharted_Linux)
Spec: SPEC.md   Roadmap: docs/review-2026-10-24/ROADMAP-proposed.md (ours) and upstream docs/ROADMAP.md (history)   Code map: docs/code-map.md
Platforms: linux (1.0 target: SteamOS on the Legion Go), windows (builds, not a 1.0 gate), mac (research only)
Notes: Integration branch `linux`, not `main`. 1.0 checks need the owner's Legion Go and his own dump (needs-owner-device). Agent rules for our pipeline: AGENTS.fork.md. Upstream sync 2026-10-31: Supermedo/uncharted_pc main is 26 ahead (0.3.10, 0.3.11, bbd1b3a); the merge into `linux` conflicted in 10 files, so it is #164 (upstream-sync, Hard), taken before any other issue. spacepepperoni/uncharted_Linux was last merged on 2026-10-28 (#133) and is 13 ahead. Shared saves (#163) follow 1.0, then 2.0 (Android, iOS, Mac; SPEC.md). `main` is kept as a copy of upstream only (owner 2026-10-26: "Keep main = upstream copy (Recommended)"); all our work and fixes land on `linux`, and issues are judged and closed against `linux`. Our `main` still holds 4 commits upstream lacks (the #32/#33 fixes, PRs #43 and #45), so it cannot fast-forward to upstream and was left at 817e7d2 on 2026-10-31.

Stages: Proposal -> Spec -> Roadmap -> Dev Loop -> Prototype / Beta.
What each stage means and who moves a project: the pipeline repo (private), PIPELINE.md.
The Project Manager updates this file when the stage changes and adds a CHANGELOG line.
