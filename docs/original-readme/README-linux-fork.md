> Historical Linux fork README retained during the 2026-11-06 audit.
> For current Universal scope and status, start with [the project README](../../README.md).
> Downloads and compatibility claims below describe the inherited Linux launcher.

# Uncharted: The Nathan Drake Collection for Linux/Steam Deck

👾Join the Discord: https://discord.gg/X2YhSJg6R2

**Uncharted 1, 2 and 3 from the PS4 collection running natively on Linux, forked by spacepepperoni from [SuperMedo](https://github.com/Supermedo).**

**This client is a launcher that allows you to directly launch Uncharted 1-3, add to your desktop or add to your Steam Library. This fork features auto-updates, a modified save system and a Steam Deck mode that optimizes playback for Steam Deck.**
<img width="2568" height="1675" alt="image" src="https://github.com/user-attachments/assets/4b519fd9-af3f-4118-b37d-2ff2469cc5b6" />

✅Uncharted 1: Confirmed fully playable with some visual glitches

✅Uncharted 2: Confirmed fully playable with some visual glitches

❌Uncharted 3: Open crashes in the Cartagena square (chapter 2) and the Citadel (chapter 8). The London bar and MP3 audio crashes are fixed. Not yet fully playable

## Distributions tested
✅ Confirmed Working: SteamOS 3.7, Fedora 44, Debian 13 CachyOS, Arch, openSUSE Tumbleweed, Ubuntu 24.04, Mint 22, Pop!OS 24.04

❌ Not Supported: Ubuntu 22.04 and older, Linux Mint 21 and older, Debian 12 and older 

## Download
1. Download **[Uncharted-x86_64.AppImage](https://github.com/spacepepperoni/uncharted_Linux/releases/latest/download/Uncharted-x86_64.AppImage)**.
2. Right-click it, open **Properties → Permissions** and tick **Is executable** (or
   **Allow executing file as program**).

## Updates

This client ships with a built in updater that can be toggled on/off, and updates from two sources: the main repo & this fork. 

- If opening the launcher itself, you'll see available updates in the bottom left of the launcher.
<img width="400" height="98" alt="image" src="https://github.com/user-attachments/assets/911e9ded-29b0-465b-8c18-5f589e987063" />

- If launching one of the games through Steam or Game Mode (Steam Deck), you will be prompted with an update screen if one is available.
<img width="400" height="200" alt="image" src="https://github.com/user-attachments/assets/b8c1b51d-fee3-42b8-a200-edb5d8c8712e" />


## Steam Deck

<img width="1316" height="809" alt="image" src="https://github.com/user-attachments/assets/0e9cf5c1-2094-465f-b1b5-98d5575e88fe" />

- For Steam Deck, it is recommended to install the [.AppImage](https://github.com/spacepepperoni/uncharted_Linux/releases/latest/download/Uncharted-x86_64.AppImage) and add the games to Steam to play in Game Mode. 
- In the Display tab, a Steam Deck mode is auto-selected if you are on SteamOS. With this enabled, expect an average of 25-30fps with occasional dips during demanding segments.
- The Steam Deck runs at 30 FPS by default; 60 FPS can be picked in the launcher.
- For a 30 FPS cap, upstream recommends a Manual GPU Clock of 1300 MHz.

## Files needed 

You must have an unencrypted extracted dump of the Uncharted: The Nathan Drake Collection (CUSA02320) v1.00.

Point the launcher to this folder in the Game tab. By default, a new Saves folder will appear within your Game folder.

Once your game folder has been specified: Play, Add to Steam (recommended for Steam Deck) or create a Desktop shortcut.

> **No game files are included.** You need your own decrypted dump of the collection
> (CUSA02343, CUSA02320, ...: the folder with eboot.bin, big2-ps4_Shipping.elf,
> big3-ps4_Shipping.elf, sce_sys, u1data, u2data and u3data).
> This project is not affiliated with Sony Interactive Entertainment or Naughty Dog.

## Features

- All three games from one launcher: Drake's Fortune, Among Thieves and Drake's Deception.
- 60 FPS like on PS4, or 120 FPS (higher rates are experimental).
- Render resolution above 1080p (up to 3200x1800) and ultrawide aspect ratios: the games draw a real larger or wider picture, set in the launcher.
- Anti-aliasing with TAA, AMD FSR 3.1, AMD FSR 4 or NVIDIA DLSS (DLAA).
- Frame generation: AMD FSR 3 (any GPU) or NVIDIA DLSS (RTX 40 2x, RTX 50 up to 4x).
- Keyboard, mouse and controller together, all remappable. Mouse look in U3.
- Stereo, 5.1 or 7.1 sound: the games are told the real speaker layout (it follows the output device, or pick it in the launcher).
- Trophies: the games' own trophy lists in the launcher, and a notice in the game when you earn one.
- Button prompts for keyboard and mouse, Xbox or PlayStation controllers; controller vibration settings.
- Cheats page (infinite health and ammo per game), launcher languages, your own covers and Steam art.
- Performance and compatibility fixes apply automatically for each game.
- Add to Steam and desktop shortcuts from the launcher, automatic updates.

## System Requirements

- 64-bit x86 Linux with glibc 2.41 or newer: SteamOS 3.7+, Debian 13, Ubuntu 25.04, Fedora 42,
  Arch, or anything newer.
- A graphics card with Vulkan 1.3 with 6 GB of video memory or more.
  4 GB cards (GTX 1650, RX 570) run out of video memory and crash after a while; on those, keep
  Anti-aliasing and Frame generation off.
- About 8 GB of free RAM


## Reporting a bug
- Open the launcher, go to **Advanced** and click **Copy log for GitHub** (Steam Deck: in Desktop Mode). It copies your system details and the last game log, without your home folder or user name.
- Open a new issue in the [Issues tab](https://github.com/spacepepperoni/uncharted_Linux/issues/new?template=bug_report.yml), choose **Bug report**, and paste the log into the **Log** field.
- Say which game and where in it the problem happened.

## Credits

Built on [bbport](https://github.com/deadinside28/bloodborne_pc), the native Linux port of
Bloodborne by deadinside28, and on the [shadPS4](https://github.com/shadps4-emu/shadPS4)
renderer. Video decoding (Videodec2) and the DLSS bridge come from
[IFreemz/shadPS4-Bloodborne-DLSS-FSR](https://github.com/IFreemz/shadPS4-Bloodborne-DLSS-FSR).
Thanks to Mindro for the shadPS4 Uncharted build that showed which problems to look for.
Several fixes also come from BilalReloaded's port: the Uncharted 3 cloth solver barriers, guest memory committed as the game allocates it, the look-at reset and garbage descriptor fixes.
Performance work on Uncharted 2's draw recording and read-backs is [che2281337](https://github.com/che2281337)'s, and several freeze fixes are from [drakolordx7's community build](https://github.com/drakolordx7/uncharted_pc).
The render resolution patch (scripts/render_res.py, cutscene movie scaling in gpu/shim/bbport_movie.cpp, the Uncharted 2 aim focus fix) is ported from [BilalReloaded's Uncharted PC port](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port) (GPL-2.0-or-later).
Many of the 0.3.10 fixes were found and made by drakolordx7 in the
[community build](https://github.com/drakolordx7/uncharted_pc), and the FSR 3 frame generation
memory leak fix is from Mevlock, the NVIDIA crash fix above 55 FPS (large compute
workgroups) from Umar-Qayyum-dev.
Most 0.3.11 fixes come from Mevlock's [dev builds](https://github.com/Mevlock/uncharted_pc-dev).
The 0.3.12 fixes are pull requests by Jhojan Jiménez, FerManjarres, dbcalo and mato215, and picks
from drakolordx7's community build (Mevlock, Alex, iannitram).
0.3.13: the Uncharted 3 sinking characters fix is drakolordx7's, the Quit game button dbcalo's.

Also used: [FSR-Vulkan](https://github.com/FireBurn/FSR-Vulkan) and the AMD FidelityFX SDK (FSR),
FSR 4 assets from [FireBurn/Q2RTX](https://github.com/FireBurn/Q2RTX), the
[NVIDIA DLSS SDK](https://github.com/NVIDIA/DLSS) (`nvngx_dlss.dll` under NVIDIA's license),
[Intel XeSS SDK](https://github.com/intel/xess) (`libxess.dll` under Intel's license),
[LibAtrac9](https://github.com/Thealexbarney/LibAtrac9), [SDL3](https://github.com/libsdl-org/SDL),
[FFmpeg](https://ffmpeg.org), [Dear ImGui](https://github.com/ocornut/imgui),
[sirit](https://github.com/shadps4-emu/sirit), [magic_enum](https://github.com/Neargye/magic_enum),
[xbyak](https://github.com/herumi/xbyak),
[Vulkan Memory Allocator](https://github.com/GPUOpen-LibrariesAndSDKs/VulkanMemoryAllocator),
[MSYS2](https://www.msys2.org) / [LLVM](https://llvm.org), [PyInstaller](https://pyinstaller.org),
[Pillow](https://python-pillow.org) and the Cinzel font (SIL Open Font License).

NVIDIA, GeForce RTX and DLSS are trademarks of NVIDIA Corporation.

## License

GNU GPL v2 or later ([LICENSE](../../LICENSE)). Third-party components keep their own licenses
(see `licenses/` in the download).
