# #190 (last piece): retire the launcher icon generator

Issue: https://github.com/iannitram/Uncharted-Universal/issues/190
Status: verify, then delete one unused generator. Stop and ask the owner if the output does not match the shipped icon.

## What the issue and docs say

- `ROADMAP.md:108`: #190 is reopened to delete `launcher/uncharted_art.py` and its test once nothing calls them.
- `docs/code-map.md:21`: the five original artwork outputs are written from Rust, and `launcher/uncharted_art.py` is deleted in 2.11 (#377).
- The collection art ships as bundled reference images in `crates/core/data/collection-artwork/` (`collection_banner.jpg`, `u1.jpg`, `u2.jpg`, `u3.jpg`, `emblem.png`). `crates/core/src/artwork.rs:49` (`generate_collection_art`) writes them.

## What the code has now

- `packaging/windows/make_uncharted_icon.py` (69 lines, Pillow) draws the launcher icon as `.ico` and `.png`. Its usage line is `make_uncharted_icon.py launcher/uncharted.ico launcher/uncharted.png`.
- Nothing calls it, as far as `git grep` shows. The only hits are the file itself and two lines in `Research/codebase-architecture.md` (292, 320), which are docs.
- The icon the app ships is `crates/core/data/icons/uncharted.ico` and `uncharted.png`.
- SPEC.md M20 says no Python of ours ships in 1.0. This script is in the tree but no build step packages it.
- Out of scope: `packaging/windows/make_icon.py`. It is the old Bloodborne icon script, and the architecture doc calls it misleading. Leave it alone here.

## Steps

1. Confirm no caller: `git grep -n -E "make_uncharted_icon|uncharted_art" -- ':!Research'`. Record the output. If anything other than the script and docs calls it, stop and report.
2. Render the script to the scratchpad, not the repo: `python3 packaging/windows/make_uncharted_icon.py /tmp/claude-1000/-home-deck--claude-spares-spare-Ekg2EA/a5db0825-cc6a-4054-99f7-50b5afbfeb40/scratchpad/uncharted.ico /tmp/claude-1000/-home-deck--claude-spares-spare-Ekg2EA/a5db0825-cc6a-4054-99f7-50b5afbfeb40/scratchpad/uncharted.png`. Compare with `crates/core/data/icons/uncharted.png` by eye, and by byte comparison if the encoder gives identical bytes. Record the result.
3. If the render does not match the shipped icon, stop. Do not delete. The script may be the only record of the icon's design. Report the difference and ask the owner.
4. If there is no caller and the render matches, or the owner has agreed to remove it: `git rm packaging/windows/make_uncharted_icon.py`.
5. Run `cargo test -p uncharted_core artwork` to confirm the artwork tests still pass.
6. Run `git grep -n "make_uncharted_icon"` again. Only the docs may match, and the docs should be updated in the PR.

## Commands

- `git grep -n -E "make_uncharted_icon|uncharted_art" -- ':!Research'`
- `python3 packaging/windows/make_uncharted_icon.py <scratch>/uncharted.ico <scratch>/uncharted.png` (needs Pillow; the scratchpad path is in the Environment section)
- `cargo test -p uncharted_core artwork`
- `bash build.sh` (before any PR)

## Done when

- The caller check is recorded.
- Either the script is deleted and its output matches the shipped icon, or the work stops and the owner is asked.
- The PR body links #190 and says the Windows packaging step is not verified on a device.
