Stage: Dev Loop
Since: 2026-10-26 (scope/status refreshed 2026-11-06)
Kind: fork (upstream: Supermedo/uncharted_pc; also spacepepperoni/uncharted_Linux)
Spec: SPEC.md   Roadmap: ROADMAP.md   Code map: docs/code-map.md   Peers: PEERS.md
Platforms: linux / SteamOS, windows, macOS (Apple silicon), iOS / iPadOS, android (ARM64 / Adreno)
Notes: Integration branch `main` (renamed from `linux` on 2026-11-01; GitHub default); `upstream-main` is the upstream copy. The approved 1.0 spec includes all five platforms and shared saves. Linux/Windows retain the legacy C/C++/Python path; the Rust workspace is partially implemented, with a skeleton CLI. Apple and Android engine/app adoption is still planned. Rust library compilation does not establish a runnable platform application. The 2026-11-06 code audit is docs/audits/2026-11-06-universal-platforms.md (#203). Hardware acceptance still needs the owner's machines and dump; no new device claims are made by the audit. Prior upstream merges and scope changes remain in CHANGELOG.md.

Stages: Proposal -> Spec -> Roadmap -> Dev Loop -> Prototype / Beta.
What each stage means and who moves a project: the pipeline repo (private), PIPELINE.md.
The Project Manager updates this file when the stage changes and adds a CHANGELOG line.
