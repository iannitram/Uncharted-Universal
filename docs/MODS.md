# File mods

_Translated from Russian; original: [ru/MODS.md](ru/MODS.md)._

## Uncharted layouts (`crates/core` `mods`, piece 1.11)

The text after this section came with the Bloodborne launcher and describes
`scripts/mods.py`, whose layouts use Bloodborne's `dvdroot_ps4` folder. Uncharted's dump has
no `dvdroot_ps4`: its game files sit in `u1data/`, `u2data/`, `u3data/` and `shareddata/`
beside the executables. The Rust `mods` module (`crates/core/src/mods.rs`), which the Rust
launcher and runtime use (pieces 1.25 and 3.09), accepts these layouts instead:

```text
mods/
  Mod name/
    u1data/...        (and/or u2data/, u3data/, shareddata/)
```

- `Mod name/app0/u1data/...`, and `Mod name/CUSA02320/u1data/...` (or one of the other
  collection title ids: `CUSA02343`, `CUSA02344`, `CUSA02826`);
- one extra wrapper folder around any of these, as after extracting an archive into a folder
  named after it: `Mod name/Mod name v1.2/u1data/...` (`.txt`, `.md`, `.jpg`, `.png` and
  `.ini` files may sit beside the wrapper).

Name case does not matter, and dotfiles are ignored. Files outside the four game folders
(a readme, screenshots) are not mounted. A mod that replaces `eboot.bin`,
`big2-ps4_Shipping.elf`, `big3-ps4_Shipping.elf`, `sce_discmap.plt`, `sce_module` or
`sce_sys`, or that holds a symbolic link or junction, is refused. shadPS4's sibling folder
is `CUSA02320-mods/u1data/...` beside the game folder. Nothing in the dump or the mods is
changed: the module writes a list of the replaced and added files, and the runtime's file
system serves them in place of the game's.

The launcher now has a **Mods** group: folder selection, a master toggle,
per-mod enable switches and arrows for load order. Changes apply on the
next launch. The lowest enabled mod has the highest priority; when files
collide, the log shows which mod replaced the previous one.

By default `bbport/mods/` is used, and in the AppImage —
`~/.local/share/bbport/mods/`. Unpack a downloaded mod like this:

```text
mods/
  Mod name/
    dvdroot_ps4/
      chr/...
      param/...
      menu/...
```

Also accepted:

- `Mod name/app0/dvdroot_ps4/...` and `Mod name/CUSA03173/dvdroot_ps4/...`;
- an extra wrapper folder, as after extracting an archive into a folder named after it:
  `Mod name/Mod name v1.2/dvdroot_ps4/...` (readme files/images may sit alongside);
- a mod without `dvdroot_ps4` that directly contains the game folders: `Mod name/chr/...`,
  `Mod name/parts/...` (`chr`, `parts`, `map`, `menu`, `msg`, `param`, `sfx`,
  `sound`, `font` and the other `dvdroot_ps4` folders).

Name case in a mod does not matter: `DVDROOT_PS4/Chr/C0000.chrbnd.dcx` from a mod built
on Windows replaces the game's `dvdroot_ps4/chr/c0000.chrbnd.dcx`. Previously such a file
was added alongside under its own name, and the game did not see it. The launch log shows how many
game files the mod replaced and how many it added: `Mods: 12 game files replaced, 0 added`.
If a mod is supposed to replace files but reports `0 game files replaced`, check the paths inside the mod.
ZIP/7z archives must be extracted first. Ready-made replacements of
`dvdroot_ps4` files are supported: textures, models, params, UI, sound and fonts.
The manager does not merge the contents of two identical `.dcx`/`.bnd` files: the last
replacement wins entirely.

For a folder already prepared for shadPS4, the sibling directory
`CUSA03173-mods/dvdroot_ps4/...` next to `CUSA03173/` works. It is attached automatically
before the mods from the list. The master toggle disables it too. You can also select
a folder that directly contains `dvdroot_ps4` as the mods folder; such a folder
is attached as a whole, without a list of individual mods.
The `-mods` convention was checked against the
[shadPS4 sources](https://github.com/shadps4-emu/shadPS4/blob/main/src/core/file_sys/fs.cpp).

The game's original files are not overwritten. For the duration of a launch a separate
merged tree of links is created in the data directory at `out/mod-game-*`; reads, stat and
directory listing see the selected replacements and the remaining original files.
The game's `/app0` and `/hostapp` are read-only. Saves stay
in the same folder as before and are writable. On a normal exit or a stop from
the launcher the temporary tree is deleted. After SIGKILL/a crash or power loss it
may remain; such directories are not used on the next launch.

Mods that replace `eboot.bin`, `sce_module` or `sce_sys`, DLL injectors and script
plugins are not supported by this loader. Executable code patches stay
in the existing `patches/` / `BB_PATCHES` system. Symbolic links inside a mod
are not accepted. Compatibility of a particular mod with CUSA03173 1.09 is up to
its author.

To launch from a terminal:

```sh
BB_GAME_DIR=/path/to/CUSA03173 BB_MODS_DIR=/path/to/mods bash run.sh
BB_GAME_DIR=/path/to/CUSA03173 BB_MODS_ENABLED=0 bash run.sh
```

Order and disabled mods are saved in the data directory's `mods.json`:

```json
{"order": ["First", "Second"], "disabled": ["First"]}
```

New folders are enabled automatically and appended at the end in alphabetical order.
`BB_MODS_CONFIG` changes the profile path. `BB_MOD_TRACE=1` with `BB_MODS_DIR` prints
the first reads of the actually attached files, for diagnostics.

# Third-party patches (shadPS4 XML)

The `patches/` folder in the data directory (in the AppImage — `~/.local/share/bbport/patches/`; in the
launcher, the **Third-party patches** group, where another folder can also be selected). Patch XML files
in shadPS4/GoldHEN format are accepted: `Metadata` for `AppVer="01.09"` and `eboot.bin` is used, files with
the `TitleID` of other games are skipped. The default enabled state comes from the file's `isEnabled`; the
choice made in the launcher is saved to `patches.json` in the data directory:

```json
{"enabled": ["file.xml/Patch name"], "disabled": ["file.xml/Another patch"]}
```

Supported line types: `bytes`, `bytes16/32/64`, `float32/64`, `utf8`, `utf16`.
A patch with `mask` lines (pattern search) or with an address outside the eboot is skipped entirely,
with a message in the log. Third-party patches are applied after the built-in ones
(`patches/Bloodborne.xml`): on overlap the third-party patch wins. From a terminal:
`BB_PATCHES_DIR=/path BB_PATCHES_CONFIG=/path/patches.json bash run.sh`.

Patches that replace pointers in tables (such as `60 FPS++` and `90 FPS++`) are relocated by the
loader to the real image address. Previously such patches stopped the launch with the error
`patch overlaps a relocation`.

# TAA sharpening

The `sharpen` and `sharpness` settings now also work for TAA, in the launcher and the
in-game menu. The range is **0…1**; at 0 or with the toggle off there is no extra
pass. RCAS is applied to the finished scene before the HUD. TAA history stores the
color before sharpening, so that sharpening does not compound through repeated accumulation.
Flat areas keep their original color, including HDR; no extra buffer is needed.
