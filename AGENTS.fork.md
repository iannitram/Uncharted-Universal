# Rules for agents in this fork (our pipeline)

This fork has its own pipeline files beside upstream's. README.md describes Uncharted-Universal and its current implementation; the inherited Linux README is preserved at docs/original-readme/README-linux-fork.md. Keep docs/ROADMAP.md as upstream history. The full pipeline and rules live in the pipeline repo (private); this is the short copy.

1. **Find the stage** in `PIPELINE.md`. The spec is `SPEC.md`, the plan `ROADMAP.md`, the code map `docs/code-map.md`, and the platform sources to inspect are in `PEERS.md`. The goals and standing rules are the pinned issue #23.
2. **Build on `main`.** It is the integration branch; open PRs against it. `upstream-main` is the upstream copy, not our integration target.
3. **Work from an issue.** Take one that is not `in-progress` and not blocked; add `in-progress` before you start. An Easy issue has a "Qwen brief".
4. **Your own platform's issues first.** Say which machine class you built on.
5. **Check that it builds** (`bash build.sh`), then open the PR. CI and a review do the rest.
6. **Label the PR** `by:<machine>` and `model:<model>`, and write `Closes #N`.
7. **A failed job** gets a comment on the issue and the issue moves up one size.
8. **No game files, ever** (#23). No private details in code, commits or PR text. Commit as the no-reply identity.
9. **Upstream:** bug reports for solid findings are fine and say they are AI-assisted; code PRs to upstream need the owner's word. Nothing goes to shadPS4 (anti-AI).
