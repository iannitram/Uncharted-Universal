# #379 (part 1): translate the Continue screen's Slint strings

Issue: https://github.com/iannitram/Uncharted-Universal/issues/379
Status: ready. Part 2 (move the language button to Settings) waits for roadmap 2.06 and is not in this brief.

## What the issue says

Owner ruling 2026-11-12: the Continue screen's strings stay English until they are translated. Make them translatable: wrap the Slint literals in `@tr(...)`, add entries to the six catalogues (de, es, fr, it, ja, pt), and let the language test cover them.

## Where it is now

- `app/ui/app.slint:513`: `export component AppWindow`. The test below requires every `text:` or `label:` literal in this file to be on the allowlist, or to use `@tr`.
- `app/src/lang.rs:239`: test `the_slint_file_shows_no_untranslated_text`.
- `app/src/lang.rs:245-247`: `CONTINUE_ENGLISH`, the allowlist of English literals the test lets through. It also covers the Saves and Settings pages. Remove the Continue-screen entries from it.
- Catalogues: `app/lang/<code>/LC_MESSAGES/uncharted-app.po` for de, es, fr, it, ja, pt.
- Continue-screen design: `docs/continue-screen.md`.
- Literals on the allowlist, by line in `app/ui/app.slint`: "B  Quit" (869), "Quit the launcher?" (1232), "Quit" (1239), "Stay" (1248), "A  Continue this save" (1334), "B  Back" (1338), "Newer" (1377), "A  Keep this save" (1414), "B  Decide later" (1421), "Play anyway" (1442), "Back" (1451). Also "Backups" (996), "In use" (1064), "<" (1127), ">" (1147), "Delete" (1204), "Keep" (1213). Also "LB" (767, 912) and "RB" (791, 936), which are button glyphs.

## Steps

1. Read `docs/continue-screen.md`. List which allowlist literals belong to the Continue screen. Put the list in the PR body. If a literal is unclear, leave it English and say so in the PR body.
2. Leave the button glyphs ("LB", "RB", "<", ">") in English. Translate only the words.
3. Wrap each Continue-screen literal in `@tr("...")` in `app/ui/app.slint`. Check how `@tr` is already used in that file first: `git grep -n "@tr" app/ui/app.slint | head`.
4. Add a msgid and msgstr for each string to all six `.po` files. Copy the format from an existing entry in `app/lang/es/LC_MESSAGES/uncharted-app.po`.
5. Remove the translated literals from `CONTINUE_ENGLISH` in `app/src/lang.rs`. The test must still pass.
6. Run `cargo test -p uncharted-app lang`.

## Commands

- `cargo test -p uncharted-app lang`
- `bash build.sh` (before any PR)

## Done when

- The lang test passes.
- `CONTINUE_ENGLISH` has no Continue-screen entries left.
- The six `.po` files carry each translated string.
- The PR body lists which literals stayed English and why.
- The translations are marked as drafts that a native speaker must review. That review is pending, and it is the owner's.

## Out of scope

The Continue-screen strings built in Rust (`app/src/screen.rs`, and the status lines in `app/src/main.rs`). These are a later brief. Moving the language button (part 2, roadmap 2.06).
