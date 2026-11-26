Uncharted: The Nathan Drake Collection for Windows
===================================================

A native port: the original PS4 game code runs directly on Windows. No game files are
included. You need your own decrypted dump of the collection (CUSA02343, CUSA02320, ...;
the folder with eboot.bin, big2-ps4_Shipping.elf, big3-ps4_Shipping.elf, sce_sys,
u1data, u2data, u3data).

Requirements
- Windows 10 (1903 or later) or Windows 11, 64-bit.
- A Vulkan 1.3 graphics card with a current driver.
- About 8 GB of free memory (RAM + page file).
  Nothing else: Python and the libraries are inside this folder.

Starting
- uncharted-app.exe opens the launcher: pick a game and continue where you left off.
- uncharted-universal.exe is the same launcher without a window, for Steam and shortcuts:
  "uncharted-universal.exe play u1" (u2, u3) starts a game and waits for it, and
  "uncharted-universal.exe steam-add" adds the games to your Steam library with artwork.
  "uncharted-universal.exe help" lists the other commands.
- Play page -> "Add to Steam" adds the collection to your Steam library, with its cover
  from your game folder. "Desktop shortcut" puts it on the desktop.
- Your own covers: put u1.jpg, u2.jpg and u3.jpg (and a wide collection_banner.jpg for the
  top of the Play page) in a folder named art next to uncharted-app.exe. Updates keep it.
- Display page -> Frame rate: 60 FPS as on PS4, or 120 FPS. Gameplay keeps its normal speed
  and the cutscene videos still play at 60. Higher rates are experimental. Above your
  display's refresh rate use the Mailbox or Immediate presentation.
- Display page -> Anti-aliasing: TAA, AMD FSR 3.1, AMD FSR 4, NVIDIA DLSS (DLAA) or Intel XeSS smooth the
  games' 1920x1080 picture with their own motion vectors, before their post-processing.
  DLSS needs a GeForce RTX. Upscaling to a larger window or screen stays with FSR 1.
- Display page -> Frame generation: AMD FSR 3 Frame Generation (any GPU, 2x) or NVIDIA DLSS
  Frame Generation (GeForce RTX 40: 2x, RTX 50: up to 4x) shows frames made between two of the
  game's: 60 FPS becomes 120 (or 240). Use the Mailbox or Immediate presentation and a display
  with that refresh rate. Menus and cutscene videos are shown as they are.
- Controls page: keyboard, mouse and controller work together, all remappable.
  Defaults: WASD move, mouse look, left button fire (R2), right button aim (L2), Space Cross,
  C Circle, F Square, E Triangle, Q/G L1/R1, Left Shift/V L3/R3, arrows and wheel d-pad,
  Escape/Enter Options, Tab touchpad. The cursor stays in the game window; hold Left Alt to
  free it. Insert opens the in-game settings menu. Bindings are saved in input.ini.
  Raw mouse (Uncharted 1 and 2, US 1.02): aiming, the camera in cover and fast turns follow the
  mouse directly, not through the right stick's top speed. It is off until you switch it on
  (Controls page, with its own aim sensitivity); it needs the 1.02 update folder.
- Trophies: the three games' trophy lists are read from sce_sys\trophy\trophy01-03.trp in the game
  folder. A trophy the game awards shows as a notice at the top of the screen and on the launcher's
  Trophies page (BB_TROPHY_POPUP=0 in Advanced -> Environment hides the notice). Earned trophies
  are kept in user\trophy\ with the saves, one list per game.

Data
- Saves and shader caches: user\ next to uncharted-app.exe (the launcher can pick another folder).
- Launcher options: %APPDATA%\uncharted-launcher. Generated files (prepared game image): out\.
- The log of the last run: user\last_run.log (the launcher's Log page shows it live).

Updates
- The launcher looks for a new version when it starts and offers it on the left: Update
  downloads it, closes the launcher, installs it over this folder and opens it again.
  Saves, settings, input.ini and out\ are kept. Advanced page -> Updates turns the check off
  or runs it now.

Known problems
- Uncharted 3, Cartagena square (chapter 2): at high frame rates the game could close with
  "GPU crash: the graphics device was lost". Fixed in 0.3. If it still happens, set Frame
  rate to 60 on the Display page.
- Uncharted 3: green or black shapes can flash for one frame on some characters (also in
  shadPS4). Not fixed yet.

Port by SuperMedo.

Shared saves
Save Relay v1.0.0-rc.2 ships beside the launcher with its own copy of Microsoft's Visual C++ runtime (vcruntime140.dll, see licenses\), so the Visual C++ Redistributable is not needed. Once configured, Play syncs your selected slot before starting each game and after it quits. Without setup or while offline, your saves stay available locally. Until launcher sign-in is available, use the bundled save-relay.exe setup. A console Play session shows both sides of a conflict. The launcher leaves an unanswered conflict pending.

Licence and source offer
This project is licensed under the GNU General Public License, version 2 only (GPL-2.0-only; see LICENSE). Third-party library licence texts are in licenses\. Complete corresponding source code is available at https://github.com/iannitram/Uncharted-Universal (and for any bundled GPL/LGPL components, from the upstream packages named in licenses\). bin\ holds FFmpeg's avcodec, avformat, avutil, swresample and swscale DLLs, built from the unmodified FFmpeg release source under the LGPL version 2.1 or later, without GPL, version 3 or nonfree parts. licenses\ffmpeg\ holds their licence, a notice, and where to get their source.
