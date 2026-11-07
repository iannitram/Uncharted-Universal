# Changelog (our fork)

Newest first. One line per change of stage, release or decision that the next session needs.

- 2026-11-06 Owner clarification: save sync on all five target platforms belongs to iannitram/save-relay; reference it now and attach its integration when ready. No sync dependency or implementation added here.
- 2026-11-06 Universal readiness audit (#203): README now describes all five target platforms and current stubs; preserved the inherited Linux README, refreshed PEERS.md with pinned platform sources and reuse limits, and aligned pipeline, agent notes and CI descriptions with the approved spec. No platform implementation or acceptance claim added.
- 2026-11-02 Upstream sync #164 (PR #186): Supermedo main bbd1b3a (26 commits, 0.3.10 and 0.3.11) merged into `main`; ten conflicts resolved, ours kept where it differs (`run.py` and launcher defaults and texts: Uncharted 3 aim blur and small-buffer readback stay off until measured in #191; the art generator stays until ported in #190).
- 2026-11-01 Player tools added to the roadmap and code map: crash help (#165), auto-tune (#166), and the Game Mode menu after 1.0 (#167); split into #168-#178.
- 2026-10-31 Peer added (owner's word): zorde23/Uncharted_Collection_PC (GPL-2.0), a source of fixes to port with credit.
- 2026-10-31 Licence of the Uncharted port we ported `render_res.py` and `bbport_movie.*` from checked: GPL-2.0 text in its `gow3_pc/LICENSE` since its first commit, README grants "GPL v2 or later" with a credit request; PEERS.md corrected, the files stay.
- 2026-10-31 SPEC.md issue references updated (#61, #79-#82 were deleted; now #109-#113: #61 to #109, #79 to #110, #80 to #111, #81 to #112, #82 to #113); no Must changed, approval stands (owner's ruling).
- 2026-10-31 Docs refreshed: code map to 359c888 (new modules, planned save-relay hook and Android / Mac / iOS rows), roadmap status from the PRs and issues closed since 2026-10-26 plus shared-saves and 2.0 sections, PIPELINE.md notes.
- 2026-10-31 Upstream sync: Supermedo main (bbd1b3a, 26 ahead) conflicts with `linux` in 10 files: #164 (upstream-sync, Hard), taken first; `main` not fast-forwarded (it holds 4 commits upstream lacks).
- 2026-10-31 2.0 pathway recorded in SPEC.md (owner's word): shared saves through save-relay first, then Android, iOS with controller support, and Mac. Four peers added for those builds.
- 2026-10-26 Normalized to pipeline v2 at Dev Loop: SPEC.md (upstream plan + our delta, approved by the owner), PIPELINE.md, AGENTS.fork.md; open issues sized; U1/U2/U3 playthrough issues filed.
- 2026-10-26 Code map added (docs/code-map.md).
- 2026-10-24 Full code review and cross-platform delivery roadmap (docs/review-2026-10-24/).
- 2026-10-18 All three games boot and play on the Phoenix handheld at 20 W; nested-task leak fixed (#3).
