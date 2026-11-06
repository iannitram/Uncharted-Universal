# Peers

The projects doing the same work as this fork: the upstream, its most active forks, and sibling PS4 native ports. I watch these and take what is already done instead of building it twice. The machine-read list is the `peer:` lines in [SPEC.md](SPEC.md#prior-art); this page has the notes. Surveyed 2026-10-28.

**Licences.** Our `LICENSE` is GPL-2.0 and our source files carry `SPDX-License-Identifier: GPL-2.0-or-later`. Every peer below reports GPL-2.0 on GitHub, and the files checked (shadPS4-derived `gpu/shadps4/`, the shim, `src/`) carry `GPL-2.0-or-later` headers, so their code may come in (R41's table: GPL-2.0-or-later into ours, yes). A port keeps the source's copyright header and links the source commit in the PR. Not copyable: GPL-3.0 projects (`Awakenchan/LanternEngine`, `OmgItsEggy/bbport-world-warper`) and repos with no licence: learn only.

`bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port` is not in that last group (checked 2026-10-31). GitHub shows no licence for it because the licence file sits in `gow3_pc/LICENSE` (GPL-2.0 text, present since the first commit, 2026-10-14), not at the repo root; the README's "Credits and license" section says "Licensed under the GNU GPL v2 or later" and invites reuse under the GPL with credit and a link back. Its source files carry no per-file SPDX headers. So `scripts/render_res.py` and `gpu/shim/bbport_movie.*` may stay (GPL-2.0-or-later into ours); keep the credit and link in `README.md` and the file headers.

## Upstreams

| Repo | Role | Watch for |
|---|---|---|
| [Supermedo/uncharted_pc](https://github.com/Supermedo/uncharted_pc) | Upstream (2065 stars, 421 open issues) | Issues and PRs for Linux, handhelds, render resolution, frame rate, upscalers |
| [spacepepperoni/uncharted_Linux](https://github.com/spacepepperoni/uncharted_Linux) | The Linux fork we sync from (190 ahead of upstream) | Linux launcher, Steam Deck mode, AppImage releases |

## Forks of the upstream, most active first

Measured 2026-10-28 against Supermedo `main` (109 forks, all pushed within 60 days; spacepepperoni's own 6 forks have nothing of note).

| # | Fork | Ahead / behind | What its own work touches |
|---|---|---|---|
| 1 | [drakolordx7/uncharted_pc](https://github.com/drakolordx7/uncharted_pc) (`community`) | 413 / 113 | The big community build: VRAM budget for small cards, FSR 3 leak fix, 45 FPS limit, save slots, walk speed. Much of it reached us through spacepepperoni. |
| 2 | [Pepin208/uncharted_pc_linux](https://github.com/Pepin208/uncharted_pc_linux) | 22 / 0 | Linux AppImage; shadPS4 texture-cache back-ports (#4818, #5118); GPU checkpoint diagnostics; launcher sends the heap sizes with `BB_GAME_RES` |
| 3 | [AdilDevStuff/uncharted_pc_linux](https://github.com/AdilDevStuff/uncharted_pc_linux) | 28 / 0 | Raw mouse look; CI AppImage/deb/rpm; aim-blur and auto-exposure dark-screen fix |
| 4 | [revvv991/uncharted-android](https://github.com/revvv991/uncharted-android) | 10 / 0 | Android on Adreno: ideas for low-power GPUs |
| 5 | [Mevlock/uncharted_pc-dev](https://github.com/Mevlock/uncharted_pc-dev) (`dev`) | 406 / 70 | Performance builds: move-action pool 64 → 256, buffer-arena probation, FPS counter RAM/VRAM line, jitter fix for U3 blue dots |
| 6 | [Cosmo/uncharted_xbox](https://github.com/Cosmo/uncharted_xbox) | 22 / 70 | Xbox/UWP port; nothing to take |
| 7 | [antikmewzik/uncharted_pc_controls_qol](https://github.com/antikmewzik/uncharted_pc_controls_qol) | 9 / 70 | U1 keyboard and mouse, per-controller rumble |
| 8 | [currantsforeyes/uncharted_pc](https://github.com/currantsforeyes/uncharted_pc) | 61 / 70 | 30 FPS option, thread priorities, off-thread pipeline compile, per-GPU pipeline cache |

## Sibling projects

| Repo | Licence | Watch for |
|---|---|---|
| [shadps4-emu/shadPS4](https://github.com/shadps4-emu/shadPS4) | GPL-2.0 | The emulator our `gpu/shadps4/` comes from: back-port its texture and buffer cache fixes |
| [bloodborne-native/bloodborne-runtime](https://github.com/bloodborne-native/bloodborne-runtime) | GPL-2.0 | The Bloodborne loader and Vulkan layer our shim descends from |
| [bloodborne-native/bloodborne-launcher](https://github.com/bloodborne-native/bloodborne-launcher) | GPL-2.0 | A launcher with profiles and settings: a model for #10 |
| [GNSB/UFC-3-PS4-PORT-LINUX-WINDOWS](https://github.com/GNSB/UFC-3-PS4-PORT-LINUX-WINDOWS) | GPL-2.0 | A sibling port with Linux and Windows builds |
| [AbdelmoumenDeghbouche/wipeout_pc](https://github.com/AbdelmoumenDeghbouche/wipeout_pc) | GPL-2.0 | A sibling PS4 port |
| [zorde23/Uncharted_Collection_PC](https://github.com/zorde23/Uncharted_Collection_PC) | GPL-2.0 | An independent Windows port of the same collection, from the God of War III port line (FelipePrado0/SpartanPort): U3 start to end at 60 FPS in 1080p. Watch its releases for crash and rendering fixes to port, with credit (added 2026-10-31) |

## Findings from the 2026-10-28 survey

Each is an issue labelled `from-peer` (adopt: port their code; learn: their idea, written fresh; drop-ours: they already did one of ours).

| Kind | Issue | Source |
|---|---|---|
| adopt | #136 shadPS4 #4818 texture-cache `CanContain` back-port (U1 blue textures, #128) | Pepin208 1d027419b |
| adopt | #137 move-action pool 64 → 256 for high frame rates | Mevlock d82693625, 51e993ee4 |
| adopt | #138 FPS counter line with RAM and VRAM | Mevlock cf02c6cff |
| adopt (or drop-ours) | #139 launcher sends heap sizes with `BB_GAME_RES` | Pepin208 eb4d69899 |
| learn | #140 VRAM budget and buffer-arena probation for small cards | drakolordx7 0cdde58e4, Mevlock 2d0b81552 |
| learn | #141 U3 blue-dot jitter fix, aim-blur dark-screen fix | Mevlock b2d69bafa, AdilDevStuff ae027aa4b |
| learn | 45 FPS through `BB_FPS_LIMIT` (noted on #124) | drakolordx7 44a0556c8 |
| drop-ours (theirs) | FSR 3.1 protected-memory patch: we already carry it (#114) | drakolordx7 6bc6ea38e |

Upstream issues brought in with an `Upstream:` line: #142 (DLSS probe on GPUs without DLSS), #143 (Intel HD 630 extension), #144 (RDNA 4 black weapons), #145 (Steam Input on non-Steam shortcuts), #146 (zh-CN). Upstream links added to our existing #10, #41, #114, #115, #116, #117, #118, #124 and #126.

## Supporting crates

| Crate | Role | Version and licence |
|---|---|---|
| [RazrFalcon/roxmltree](https://github.com/RazrFalcon/roxmltree) | Read-only parser for third-party patch XML; DTD expansion and external entity resolution are disabled | Pin 0.21.1; [crate metadata](https://docs.rs/crate/roxmltree/0.21.1/source/Cargo.toml.orig) declares `MIT OR Apache-2.0`; use the [MIT option](https://github.com/RazrFalcon/roxmltree/blob/master/LICENSE-MIT). The adapter validates numeric references because the parser maps invalid Unicode scalar references (such as surrogates) to U+FFFD. |
