# The Continue screen brief: Uncharted 1, 2 and 3

Roadmap 2.01 (#288), for Must M10. Status: **draft, waiting for the owner's 'approved' on #288**. Piece 2.04 builds the screen from this brief; nothing here is code. The shape is totk-native's (its SPEC.md M10: one controller-first Continue screen, the latest save's picture, device, time and play time, one button to sync and play, a conflict shown as two saves side by side). This brief keeps that shape and adds what a collection of three games needs: a game strip, one card per game, and a conflict per game.

## What the screen is for

It is the first thing the player sees on every start: the Steam shortcut (M13) opens the launcher, and the launcher opens here. From Game Mode, **A starts the game in one press** (the M10 check). It is full screen in Game Mode (the AMD handheld and the Strix Halo tablet) and a window on the desktop (at least 640 x 400, the 2.03 scaffold's minimum); mouse and keyboard work everywhere a controller does. First run (2.05) comes before it until setup is done, and the after-crash screen (2.07) comes before it after a crash. Saves, Settings, Fleet and About (2.06, 2.08, 2.09) are one button away.

## Layout, all states

```
+------------------------------------------------------------------+
| [LB]   Uncharted 1  |  >Uncharted 2<  |  Uncharted 3   [RB]       |  game strip
|                                                                  |
|   +----------------------+   Title                               |
|   |                      |   Detail line                         |  save card
|   |       picture        |   Saved today 21:14                   |
|   |                      |   on the Strix Halo tablet            |
|   +----------------------+   Play time 6 h 12 min                |
|                                                                  |
|     [ A  Continue ]     [ Play ]     [ Y  Other saves ]          |  action row
|                                                                  |
|   Saves     Settings     Fleet     About                         |  page row
|   Synced 21:20                                      B  Quit      |  status line
+------------------------------------------------------------------+
```

- **Game strip:** the three games, the focused one marked. LB/RB change game from anywhere on the screen.
- **Save card:** the newest save of the focused game (`savecard::latest`).
- **Action row:** Continue, Play, Other saves; buttons a state does not have are left out, not greyed.
- **Page row:** the other pages, reached with D-pad down.
- **Status line:** sync state on the left (checking, synced, offline), the button hint for B on the right.

The layout follows the display (M21). At 16:10 and 16:9 (the three 1.0 devices) the picture sits left of the text as above. In a window narrower than 4:3 the picture goes above the text and the action row wraps. Text sizes come from the window height, so the handheld at 1280 x 800 and the desktop at 4K read the same.

## Save card fields

The card shows the `SaveCard` of roadmap 1.18 (`crates/core/src/savecard.rs`). A field that is `None` never shows an error or a placeholder word; the card closes up around it.

| Field | `SaveCard` | Shown as | When it is `None` |
|---|---|---|---|
| Picture | `picture` | the save's `sce_sys/icon0.png`, scaled to fit the panel, never stretched (bars inside the panel if needed) | the collection's `sce_sys/icon0.png` from the installed copy, read at run time, with the game's name over it; if that cannot be read either, a plain panel with the game's name in large text |
| Title | `title` | the heading of the card | the game's name (`Game::name()`: "Uncharted 2") |
| Detail | `detail` | one line under the title, cut with an ellipsis when long | the line is left out |
| Time | `saved_unix` | "Saved today 21:14", "Saved yesterday 18:02", else the date ("Saved 3 Nov 2026"), in the device's time zone and language | not an `Option`; 0 shows "Save time not known"; a time ahead of this device's clock shows its date, never "in 3 hours" |
| Device | `device` | "on the Strix Halo tablet": the paired device's name from the fleet records (1.23, 4.04) | the line is left out |
| Play time | `play_time_secs` | "Play time 6 h 12 min" (minutes only under an hour) | the line is left out; expect this for all three games until 1.18 finds play time in their files |
| Build | `build_id` | not on the card; used by the build warning and shown in the conflict prompt | no build warning; the prompt shows "Build not recorded" |
| Slot | `slot` and its slot folder | "Slot 3" in Other saves and the conflict prompt only (slot 1 is the game's own folder, slots 2-10 are `Save slots/Uncharted N/Slot n`) | not an `Option` |

## Actions

- **Continue** (shown when the focused game has a save; A by default): pull through save-relay (9.01), show the conflict prompt if the pull finds one, make the card's slot the active one (`saves`, `BB_SAVEDATA_DIRS`), start the game (`launch`, 1.25), push after it quits, then come back here with the new card. The game opens at its own title menu, where its own Continue or Load picks up the save; the launcher does not reach inside the game.
- **Play** (always shown; A by default when there is no save): the same pull, start and push, with the slot that is already active and no save chosen. It is how a new game or a game's own Load menu is reached.
- **Other saves** (shown when the game has more than one save; Y): the list in state 3.

While Continue or Play runs, the action row is replaced by one line, "Checking saves..." then "Starting Uncharted 2...". B during "Checking saves" cancels and nothing starts; once the game process starts, B goes to the game. If save-relay cannot be reached, the status line says "Offline: playing this device's save; it syncs next time" and the game starts.

## Controller path, all states

| Input | Keyboard and mouse | Does |
|---|---|---|
| A (Cross) | Enter, Space, click | the focused button |
| B (Circle) | Esc | back one step; on this screen, "Quit the launcher?" (A quits, B stays) |
| LB / RB (L1 / R1) | Page Up / Page Down, click a game | the previous / next game; focus goes back to the default button |
| D-pad, left stick | arrow keys, Tab | move focus along the action row, down to the page row and back |
| Y (Triangle) | O | Other saves, when shown |

On opening, and after each game change, focus is on the **default button**: Continue when the game has a save, else Play. So the one-press path is: open, A. The screen opens on the game whose newest save is the newest of the three; with no save at all, on the game last started; on the very first open, on Uncharted 1. `app/src/input.rs` today maps the D-pad, the stick and South (A); 2.04 adds East (B), North (Y), LB and RB, and the keys above.

The screen pulls when it opens and again on Continue or Play, so a save made on another device shows up here, and a conflict appears before the player presses anything. A pull that brings a newer card replaces the card in place and keeps focus.

## State 1: no save

**On screen:** the game strip; the card with the fallback picture, the game's name as the title, and "No save yet" as the only line; the action row with **Play** only; the page row; the status line.
**Controller path:** open, focus on Play, A: pull, start. LB/RB to another game. D-pad down to the pages.

```
| [LB]  >Uncharted 1<  |  Uncharted 2  |  Uncharted 3   [RB]  |
|   +-----------------+   Uncharted 1                         |
|   | collection icon |   No save yet                         |
|   |   Uncharted 1   |                                       |
|   +-----------------+                                       |
|        [ A  Play ]                                          |
|   Saves   Settings   Fleet   About             B  Quit      |
```

## State 2: one save

**On screen:** the card with that save's picture, title, detail, time, device and play time (each line only when its field is not `None`); the action row with **Continue** and **Play**.
**Controller path:** open, focus on Continue, A: pull, start in the save's slot. Right then A for Play. LB/RB, D-pad down as in state 1.

```
| [LB]   Uncharted 1  |  >Uncharted 2<  |  Uncharted 3   [RB] |
|   +-----------------+   <title from param.sfo>              |
|   |  save picture   |   <detail from param.sfo>             |
|   |  (icon0.png)    |   Saved today 21:14                   |
|   +-----------------+   on the RTX 3090 Ti desktop          |
|        [ A  Continue ]   [ Play ]                           |
|   Saves   Settings   Fleet   About             B  Quit      |
```

## State 3: several saves

**On screen:** the card of the newest save, as in state 2; the action row with **Continue**, **Play** and **Other saves (n)**, n being the number of saves of this game across its slots.
**Controller path:** open, A continues the newest save. Y (or focus Other saves, A) opens the list: one row per save, newest first, each with a small picture, title, detail, time, device and slot. Up/Down move, A continues that save (its slot becomes the active one), B goes back with focus on Other saves. When two saves share a slot, their rows say "In the game: Load" and the game's own Load menu picks between them.

```
| [LB]   Uncharted 1  |  Uncharted 2  |  >Uncharted 3<  [RB] |
|   Other saves: Uncharted 3                                  |
|   > [pic] <title>   <detail>   today 21:14   tablet  Slot 1 |
|     [pic] <title>   <detail>   3 Nov 2026    desktop Slot 2 |
|     [pic] <title>               1 Nov 2026            Slot 4 |
|        A  Continue this save          B  Back               |
```

## State 4: sync conflict

A conflict is save-relay finding that this device's save and the synced one both changed since the last sync. It is per game: a conflict in Uncharted 2 does not touch the cards of 1 and 3.

**On screen:** the side-by-side prompt over the screen (`savecard::side_by_side`): "Two saves of Uncharted 1. Keep which one?", this device's card on the left, the synced card on the right, each with picture, title, detail, time, device, play time, slot and build. The newer side (`server_is_newer`) carries a "Newer" mark. Focus starts on the newer side.
**Controller path:** Left/Right choose a side, A keeps it, B decides later. After A, the other side is kept as a backup (1.19 `backup`), never deleted, and the game starts if the prompt came from Continue or Play, else the screen shows the kept card. After B the game's card shows "Two saves: pick one to play" and Continue and Play open the prompt again instead of starting; the other two games play as normal.

```
|   Two saves of Uncharted 1. Keep which one?                 |
|   +-------------------------+   +-------------------------+ |
|   | This device             |   | Synced          Newer   | |
|   | [save picture]          |   | [save picture]          | |
|   | <title> / <detail>      |   | <title> / <detail>      | |
|   | today 20:05, handheld   |   | today 21:14, tablet     | |
|   | Slot 1, build a1b2c3    |   | Slot 1, build a1b2c3    | |
|   +-------------------------+   +=========================+ |
|        A  Keep this save               B  Decide later      |
```

## Per game

The mocks use one game each; the others look the same with these changes. All three games ship in one installed copy and share its title ids, but each has its own save folder, so each has its own card, its own state and its own conflict.

| | Uncharted 1 | Uncharted 2 | Uncharted 3 |
|---|---|---|---|
| Strip label and fallback title | Uncharted 1 | Uncharted 2 | Uncharted 3 |
| Saves read from | `Uncharted 1/SAVEDATA01*`, slots 2-10 | `Uncharted 2/SAVEDATA02*`, slots 2-10 | `Uncharted 3/SAVEDATA03*`, slots 2-10 |
| Started on Continue or Play | `eboot.bin` | `big2-ps4_Shipping.elf` | `big3-ps4_Shipping.elf` |
| State 1 | collection icon with "Uncharted 1", Play | same, "Uncharted 2" | same, "Uncharted 3" |
| State 2 | its card, Continue and Play | its card, Continue and Play | its card, Continue and Play |
| State 3 | newest card, Other saves (n) | newest card, Other saves (n) | newest card, Other saves (n) |
| State 4 | prompt over Uncharted 1 only | prompt over Uncharted 2 only | prompt over Uncharted 3 only |

The title and detail are whatever each game writes into its save's `param.sfo`; 2.04 notes one real example per game at its device check, in words only, and commits no save.

## Two warnings that are not states

- **Build differs** (M12, 4.04): when the card's `build_id` and this device's build are both known and differ, Continue first shows "This save was made by build x on the tablet; this device has build y." A plays anyway, B goes back.
- **Not set up on this device:** when the game is not prepared here, Continue and Play are replaced by **Set up**, which opens first run (2.05) at the right step.

## How 2.04 tests it

View-model tests in `cargo test` with fake `SaveCard`s: each of the four states for each game; each field `None` in turn; the default focus; open then A starts in one press; LB/RB change game; a forced conflict keeps the side picked and backs up the other. One headless Slint run of the page. The run from Game Mode on the AMD handheld and the Strix Halo tablet is a device check (`needs-owner-device` on 2.04, not on this brief).

## Open for the owner

1. Continue starts the game at its title menu, where the game's own Continue loads the save. Skipping the title menu would need a runtime hook; this brief leaves it out of 1.0.
2. The side not picked in a conflict is kept as a backup, not deleted. Confirm, and whether Saves (2.06) shows those backups.
3. The fallback picture is the collection icon read from the player's own installed copy at run time. Confirm, or name another source (the launcher's existing images, or 1.22 artwork).

## No game files

This brief has text sketches only. No game files, saves, screenshots or artwork are committed with it; pictures appear only at run time, from the player's own saves and installed copy (M9).
