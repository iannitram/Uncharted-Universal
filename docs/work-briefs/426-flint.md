# Flint brief: issue #426, stencil association guard

Base: origin/main at 69672033. Issue: https://github.com/iannitram/Uncharted-Universal/issues/426
Machine: Linux, AMD Strix Halo. This is source work; no game files or gameplay validation.

## Exact task

In `gpu/shadps4/video_core/texture_cache/texture_cache.cpp`, function
`TextureCache::FindDepthTarget`, lines 912-914 at this base, replace the existing
`if (desc.info.stencil_addr != 0)` condition with the positive form of all four
upstream preconditions:

```cpp
if (desc.info.stencil_addr != 0 &&
    desc.info.stencil_addr != desc.info.guest_address &&
    desc.info.props.has_stencil && desc.info.stencil_size != 0) {
```

Preserve the entire association body, its format compatibility check, and the
relookup after `slot_images.insert`. Keep the SPDX copyright/license header.
Place a concise source attribution comment above the condition, linking
shadPS4 PR #5737, commit `a267e8fd71c6ae097ac09efb7312cfe2a4031b6f`.
Do not invent a new AssociateStencil function: upstream has one, this fork does not.

Why: nonzero stale stencil addresses can appear on a depth-only descriptor.
Zero stencil size and stencil/depth address aliasing must also skip association.
For valid separate stencil attachments, existing behavior must remain intact.

Read only these supporting locations if needed:
- `gpu/shadps4/video_core/texture_cache/image_info.h:22-100` (fields)
- `gpu/shadps4/video_core/texture_cache/image_info.cpp:85-110` (field construction)
- `gpu/shadps4/video_core/texture_cache/texture_cache.cpp:898-943` (entire function)

## Documentation

Add one short dated CHANGELOG.md entry for #426 after its introduction, crediting
upstream PR and commit. State device gameplay not run. Add a narrow entry to
PEERS.md next to other shadPS4 imports: source link, exact commit, GPL-2.0-or-later,
header retained. Do not rewrite existing roadmap stages or mark gameplay complete.

## Boundaries and checks

Only change the texture_cache.cpp condition/comment, CHANGELOG.md and PEERS.md.
Do not add a helper or a test that duplicates a boolean expression. The overseer
will independently check the exact upstream diff and production guard truth table.
Do not edit submodules, install packages, publish anything, commit, push or open a PR.
Run `git diff --check` and inspect `git diff --stat`. Report changed files and any
failure. The overseer owns the required full `bash build.sh` check and CI.

This brief deliberately supplies the code so you do not need to scan the repository.

## Dispatch refinement

The first OpenCode run spent time inspecting documentation history and was
interrupted before edits. The coordinator narrowed Flint's second dispatch to
the single production-code guard/comment and `git diff --check`, with repository
search and arbitrary shell commands denied. The coordinator supplies provenance
text above and owns independent review/build verification. This is the preferred
size for the next local-model task: one bounded edit and a focused check.

## Independent review

The production guard was extracted verbatim and compiled as a standalone C++20
harness without system headers on Linux/AMD Strix Halo. All 36 combinations of
three stencil addresses, three depth addresses, stencil present/absent and
zero/nonzero stencil size matched the inverse of upstream's four rejection
conditions. The old nonzero-address-only guard falsely accepted 20 of those
cases. This checks the actual production expression, not a copied replacement.
The entire association body and subsequent production code match the base;
format compatibility checks and post-insert image relookup remain unchanged.
Upstream PR #5737's exact patch and the retained SPDX header were verified.
`git diff --check` passed. This focused check is not a full translation-unit,
Vulkan-device or gameplay test; hosted build checks remain required.
