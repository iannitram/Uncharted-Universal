# Rules for agents in this fork (our pipeline)

This fork has its own pipeline files beside upstream's. README.md describes Uncharted-Universal and its current implementation; the inherited Linux README is preserved at docs/original-readme/README-linux-fork.md. docs/ROADMAP.md stays a one-line pointer to the root ROADMAP.md, and an upstream sync takes our side of it (decided 2026-11-12: "Keep the pointer"). The full pipeline and rules live in the pipeline repo (private); this is the short copy.

1. **Find the stage** in `PIPELINE.md`. The spec is `SPEC.md`, the plan `ROADMAP.md`, the code map `docs/code-map.md`, and the platform sources to inspect are in `PEERS.md`. The goals and standing rules are the pinned issue #23.
2. **Build on `main`.** It is the integration branch; open PRs against it. `upstream-main` is the upstream copy, not our integration target.
3. **Work from an issue.** Take one that is not `in-progress` and not blocked; add `in-progress` before you start. An Easy issue has a "Qwen brief".
4. **Your own platform's issues first.** Say which machine class you built on.
5. **Check that it builds** (`bash build.sh`), then open the PR. CI and a review do the rest.
6. **Label the PR** `by:<machine>` and `model:<model>`, and write `Closes #N`.
7. **A failed job** gets a comment on the issue and the issue moves up one size.
8. **No game files, ever** (#23). The one exception is the downloaded test save folders in `tests/fixtures/saves/` (owner ruling 2026-11-24, save data only; its `README.md`). No private details in code, commits or PR text. Commit as the no-reply identity.
9. **Upstream:** bug reports for solid findings are fine and say they are AI-assisted; code PRs to upstream need the owner's word. Nothing goes to shadPS4 (anti-AI).

## Save Relay fixes (owner instruction, 2026-11-20)

For every save-relay-related defect, first fix it in this build. Then open an issue
in `iannitram/save-relay` explaining the observed problem, root cause, exact fix,
validation and remaining limits, with links to the Uncharted issue and PR. Link
the Save Relay issue back from the Uncharted work so the shared project can adopt
the correction too. Distinguish an integration defect from a defect proven in the
shared core, and do not claim an untested shared-core reproduction. This reporting
step is required even when the immediate fix lands only in this build.
