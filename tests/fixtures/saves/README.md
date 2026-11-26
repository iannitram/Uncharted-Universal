# Test saves

Decrypted PS4 save folders of Uncharted: The Nathan Drake Collection, for live checks that need a
point past the first chapter (a museum crash, a train benchmark, a Chapter Select run). Players
shared them on the issue trackers of other community PC ports; each folder below is one download,
kept as it came (save data, `param.bin`, `icon0.png`, `thumb.png`, `.backup/`). The one change: from
`sp-81`, which was a whole emulator user folder, only the save folders are kept, without its logs
and caches. These folders are an owner-ruled exception to the no-game-files rule, for save data
only; game dump files still never go in the repository.

Install a set into a user directory and run with it:

```sh
python tools/install_test_save.py --list                 # sets, folders and the label each save shows
python tools/install_test_save.py bc-183 out/try/user-museum --slots 0CC 02P 023
DRIVE_USER=out/try/user-museum python tools/drive.py u2 "wait 60; press cross; ..." <game dir>
```

The script copies into `<user>/savedata/1/CUSA02320/`. Sets from other regions (CUSA02343,
CUSA02344, CUSA02826) load under CUSA02320 the same way; the game shows them in its own language.
Folder names: `SAVEDATA01x` U1, `02x` U2, `03x` U3, `0xP` that game's profile (Chapter Select
unlocks, store), `0CC` the collection's shared profile. On the U2 main menu, Continue loads the
newest campaign save, so install only the slot you want plus `0CC` and the game's `0xP`.

## Picks by check

| Check | Set | Slots | What it is |
| --- | --- | --- | --- |
| U2 ch2 museum (#155, #154, #158) | bc-183 | 0CC 02P 023 (or 021/022) | "Breaking and Entering", US |
| U2 ch3 Borneo jungle (#157, #117, #129, #130) | bc-183 | 0CC 02P 025 | "Borneo" |
| U2 ch4 The Dig (#154) | bc-183 | 0CC 02P 024 | "The Dig" |
| U2 ch5 Nepal city (#109, #140) | bc-183 | 0CC 02P 020 | "Urban Warfare" |
| U2 Path of Light, tower puzzles (#112) | bc-183 | 0CC 02P 026 | "Path of Light" |
| U2 any chapter, train (#115), monastery (#430) | sm-601 or sp-81 | 0CC 02P 020 | profile "Completed on Explorer": Chapter Select has every U2 chapter |
| U3 any chapter, Sully follow (#160), #63, #64, #111 | bc-277 | 0CC 03P 033 | profile "Completado en Normal" (finished on Normal), slot at 100% |
| U3 ch2 / ch3 Cartagena (#63, #111) | bc-200a / bc-200b | 03P 030 | "Greatness from Small Beginnings" / "Second-Story Work" |
| U1 any chapter (#128, #111, #117) | bc-229 | 0CC 01P 011 | U1 slot at 100% |
| U1 ch2 El Dorado / ch4 plane (#128) | bc-277 | 0CC 01P 010 / 011 | "En busca de El Dorado" / "Avión siniestrado" |

## Sets

| Set | Source | Title id | Language | Holds |
| --- | --- | --- | --- | --- |
| bc-6 | [bc160201983 port #6](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port/issues/6) | CUSA02826 | Spanish | U1 to ch4 (30%), U2 Borneo |
| bc-18 | [bc160201983 port #18](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port/issues/18) | CUSA02344 | Russian | U3 ch2 (3%) |
| bc-76 | [bc160201983 port #76](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port/issues/76) | CUSA02826 | Spanish | U1 to 54%, U2 Borneo, U3 finished plus "La ciudadela" (38%) |
| bc-180 | [bc160201983 port #180](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port/issues/180) | CUSA02826 | Spanish | U1 to 54%, U2 Borneo, U3 finished |
| bc-183 | [bc160201983 port #183](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port/issues/183) | CUSA02320 | English | U2 ch2 to Path of Light (7 slots), U3 ch1 |
| bc-200a | [bc160201983 port #200](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port/issues/200) | CUSA02320 | English | U3 ch2 |
| bc-200b | [bc160201983 port #200](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port/issues/200) | CUSA02320 | English | U3 ch3 |
| bc-229 | [bc160201983 port #229](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port/issues/229) | CUSA02344 | Russian | U1 finished (100%) and at 79%, U2 ch2 |
| bc-277 | [bc160201983 port #277](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port/issues/277) | CUSA02826 | Spanish | U1 to 54%, U2 The Dig, U3 finished plus "El camino del medio" (45%) |
| bc-378 | [bc160201983 port #378](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port/issues/378) | CUSA02344 | Russian | U1 finished, U2 ch4 The Dig (19%), with `.backup/` and thumbnails |
| dk-4 | [drakolordx7/uncharted_pc #4](https://github.com/drakolordx7/uncharted_pc/issues/4) | CUSA02344 | English | U3 ch3 |
| sm-601 | [Supermedo/uncharted_pc #601](https://github.com/Supermedo/uncharted_pc/issues/601) | CUSA02344 | English | U2 finished on Explorer, U3 "Cruisin' for a Bruisin'" (70%) |
| sp-81 | [spacepepperoni/uncharted_Linux #81](https://github.com/spacepepperoni/uncharted_Linux/issues/81) | CUSA02343 | English | U2 finished on Explorer, U3 ch5 London Underground |

Every slot's label (the game's own title and subtitle from `param.bin`) is what
`install_test_save.py --list` prints. A percentage is campaign progress; a chapter name is where
the save resumes.
