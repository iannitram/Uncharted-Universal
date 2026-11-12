# Legal, Licensing and Distribution Research for uncharted_pc

> **Not legal advice.** This is informational research compiled on 2026-10-16 from public sources and a
> read of this repository. It is not a legal opinion, and it mostly reflects US law (Ninth Circuit case
> law and the DMCA), with notes on other places where relevant. Facts about other projects come from news
> reports and GitHub pages that can change. Points marked **(uncertain)** could not be confirmed from
> primary sources. Before making decisions that carry real exposure, such as monetization, a public
> relaunch, or answering a takedown notice, talk to a lawyer, or ask the EFF / Software Freedom
> Conservancy for a referral.

> **Update 2026-11-12 (owner ruling): the project is now GPL-2.0-only.** Several sources it builds on
> (uncharted_pc, uncharted_Linux, Uncharted_Collection_PC, SpartanPort) ship plain GPL-2.0 with no "or
> later" grant, so this project cannot offer later versions. Every place below that relied on the project
> being "or later" no longer holds. In particular, the GPL-3.0 FFmpeg build cannot ship with a
> GPL-2.0-only program; that is tracked in issue #299.

**Key takeaways for uncharted_pc**

- **The DLSS files are the weakest licensing point.** The release zip ships NVIDIA's proprietary
  `nvngx_dlss.dll`/`nvngx_dlssg.dll` and a bridge DLL that statically links NVIDIA's `nvsdk_ngx_s.lib`, in
  the same package as a GPL-2.0-or-later program that loads the bridge at run time. NVIDIA's license
  forbids anything that would make the SDK "subject to an open source software license" (§4(e)). The FSF
  says a GPL program and a plug-in it loads that share data structures form a single combined work. The
  "system library" exception does not cover DLLs the application ships itself. The current shadPS4-derived
  code base cannot add a linking exception without the consent of every copyright holder. The cleaner
  option is to make DLSS a component the user installs (see [GPL and the NVIDIA DLSS SDK](#gpl-and-the-nvidia-dlss-sdk)).
- **The FFmpeg in the zip is GPL-3.0-or-later.** MSYS2's build uses `--enable-gpl --enable-version3`
  with x264/x265. As a result, (a) the binary distribution is effectively under **GPLv3** terms (allowed
  because the project is "or later", but it makes the DLSS conflict worse), and (b) `package.sh` ships
  FFmpeg, x264, x265, SDL3 and other DLLs **without their license texts or a source offer**. That is a
  plain GPL compliance gap that can be fixed.
- **Sony has not acted against bbport, the Supermedo Bloodborne/Uncharted ports or shadPS4** (no DMCA
  notice or news found as of 2026-10-16). What Sony did act on (2023 to 2026) were projects that (a)
  published **keys or game data**, (b) used the **Bloodborne name/trademark** (Lance McDonald's 60 fps
  patch page, the Bloodborne PSX demake, a fan "Top Down Arena"), or (c) distributed **pirated games**
  (Overdrive store, October 2026, reported). Uncharted is an active Sony franchise that Sony already sells
  on PC (Legacy of Thieves, 2022), so this project is highly visible.
- **Nintendo v. Yuzu shows what made a project vulnerable:** paid Patreon "early access" (around
  US$30k/month), guides for dumping keys and games, links to key sources, compatibility testing that
  pointed to pre-release piracy (Tears of the Kingdom), and decryption with unauthorized keys at run time.
  uncharted_pc avoids most of these (no decryption code, no keys, no donations). It should keep it that
  way in writing and in practice.
- **The auto-updater runs unverified code.** It takes the first `.zip` from a GitHub "latest release",
  extracts it and runs the new `Uncharted.exe` with no signature or hash check. It also points at
  **`Supermedo/uncharted_pc`**, not this fork. Add Ed25519 (minisign) or Authenticode verification and
  GitHub immutable releases, and decide on purpose which repository a fork updates from.

---

## Scope and project facts

What the repository shows (read 2026-10-16):

| Item | Where | Note |
|---|---|---|
| Project license GPL-2.0-only (was GPL-2.0-or-later until 2026-11-12) | `LICENSE`, `README.md`, `Cargo.toml` | Vendored shadPS4 code is GPL-2.0-or-later (`gpu/VENDOR.txt`). |
| No decryption code | `scripts/prepare.py` ("No decryption…"), `README.md` | The user supplies an already decrypted dump (CUSA02343, CUSA02320). |
| DLSS bridge (MIT source) links `nvsdk_ngx_s.lib` statically | `gpu/dlss_bridge/CMakeLists.txt`, `LICENSE.txt` | The renderer loads `bbport_dlss.dll` at run time (`vk_temporal_upscaler.cpp`). `package.sh` ships `bbport_dlss.dll`, `nvngx_dlss.dll`, `nvngx_dlssg.dll` and `NVIDIA-DLSS-LICENSE.txt`. |
| FSR 4 v07 INT8 assets | `tools/fetch_fsr4_assets.sh` → `fsr4_shaders/` (shipped if present) | Taken from FireBurn/Q2RTX at a pinned commit, with `LICENSE-FSR4-v07.txt` (MIT, © 2025 AMD). |
| FSR 4.1.1 assets extracted from AMD's DLL | `tools/fsr4cap/` | `packaging/default.nix`: "extracted from AMD's DLLs: never in a public package". The Windows `package.sh` **does not** copy `fsr4_411/`. If release zips do include them, that contradicts the stated policy (check). |
| FFmpeg, SDL3, etc. | `package.sh` copies every CLANG64 DLL that `ldd` reports | License texts copied only for protobuf, abseil, openssl, miniupnpc, zlib, wepoll, cpp-httplib, plus `licenses/nlohmann-json-LICENSE.txt`. |
| Launcher | PyInstaller-frozen `Uncharted.exe` with Pillow and Cinzel (OFL) | Cinzel was chosen as "roman capitals close to the Uncharted logo" (`launcher/uncharted_fonts.py`). |
| Covers | `launcher/covers/u1–u3.jpg` | Original artwork per the README. "Add to Steam" builds Steam grid art from the **user's own** dump (`sce_sys`) locally. |
| Updater | `launcher/uncharted_launcher.py` `latest_release()` / `install_update()` | HTTPS GitHub API, no signature or hash check, `RELEASES_API` = `Supermedo/uncharted_pc`. |
| Leftover Bloodborne material | `patches/Bloodborne.xml`, `bloodborne.ico`, `bbport_launcher_win.py` | The "Bloodborne" mark appears in the tree. Consider pruning it from Uncharted releases. |

---

## License compatibility of bundled components

### GPL and the NVIDIA DLSS SDK

**What NVIDIA's license says** (NVIDIA RTX SDKs License, v. 14 March 2024, the `LICENSE.txt` in
[NVIDIA/DLSS](https://github.com/NVIDIA/DLSS/blob/main/LICENSE.txt)):

- §1(c): you may distribute SDK materials only "as incorporated in object code format into a software
  application". §2(a): the application "must have material additional functionality". §4(b): no
  distribution "as a stand-alone product".
- §2(c): you must distribute it "subject to the terms at least as protective as the terms of this license".
- **§4(e): "You may not use the SDK in any manner that would cause it to become subject to an open source
  software license"**, giving as examples licenses that require disclosure of source, permission to make
  derivative works, or free redistribution. Those are exactly the GPL's conditions.
- Supplement §1: licensed only "for their use in systems with NVIDIA GPUs". Supplement §4: notify NVIDIA
  before a *commercial* release. Supplement §7.1(b): you "must attribute the use of the applicable SDK and
  include the NVIDIA Marks on splash screens, in the about box of the application (if present), and in
  credits".
- The license is between NVIDIA and whoever downloaded the SDK. **Each fork that builds and ships the DLLs
  accepts it separately.**

**The GPL side.** The GPL governs distribution of the *combined work*. The FSF's view
([GPL FAQ, GPLPluginsInNF](https://www.gnu.org/licenses/gpl-faq.html#GPLPluginsInNF)) is that a main
program and a dynamically loaded plug-in that make function calls to each other and share data structures
form a single combined program. The bridge receives Vulkan handles and image structs and is called every
frame, which fits that description. The GPLv2 §3 / GPLv3 §1 **system library exception** covers components
"normally distributed with the major components of the operating system". NVIDIA's *driver-side* NGX core
arguably fits, but `nvngx_dlss.dll` **shipped by the application** does not. Others dispute that dynamic
linking creates a derivative work (see the debates collected in the
[linux-audio-dev](https://lists.linuxaudio.org/archives/linux-audio-dev/2010-June/027293.html) and
[Blender](https://lists.blender.org/pipermail/bf-committers/2007-May/018374.html) list archives). The
question is unsettled in court **(uncertain)**. The standard fix in the FAQ
([GPLIncompatibleLibs](https://www.gnu.org/licenses/gpl-faq.html#GPLIncompatibleLibs)) is a
linking exception. Only the copyright holders can grant one, and here they include every contributor to
the vendored shadPS4 code and bbport, so the fork cannot realistically add one on its own.

Two licenses make opposite demands on the shipped package. The GPL says the whole combined work must be
GPL when distributed. NVIDIA §4(e) says the SDK must never become subject to such a license. In practice
the people who could complain are (a) GPL copyright holders (shadPS4/bbport contributors), and (b) NVIDIA,
which could object to DLSS shipping inside a GPL package or in a project that may be infringing. Neither
has acted against the shadPS4 DLSS forks so far (none found).

**How other projects handle it**

| Project | License | DLSS handling |
|---|---|---|
| [IFreemz/shadPS4-Bloodborne-DLSS-FSR](https://github.com/IFreemz/shadPS4-Bloodborne-DLSS-FSR) (this repo's bridge source) | GPL-2.0-or-later fork, MIT bridge | Ships `nvngx_dlss.dll`/`nvngx_dlssg.dll` "under the NVIDIA DLSS SDK license, not covered by the licenses above". No GPL exception. Same pattern as uncharted_pc. |
| OptiScaler | GPL-3.0 ([mirror](https://gitee.com/mirrors/OptiScaler)) | Does **not** bundle `nvngx_dlss.dll`. Users supply it (from a game, TechPowerUp, or the Streamline SDK). |
| Blender ([Phoronix, Feb 2026](https://www.phoronix.com/news/NVIDIA-DLSS-Blender)) | GPL | PR includes headers only. "The runtime library is supplied separately", found by the NGX driver in the application path. Licensing concerns were still open. |
| `dlss_wgpu` (Bevy) ([docs.rs](https://docs.rs/dlss_wgpu)) | MIT/Apache | "The DLSS SDK cannot be redistributed by this crate". The application developer ships the DLL and NVIDIA's notice text. |
| DXVK-NVAPI / Proton ([GamingOnLinux](https://gamingonlinux.com/2020/07/nvidia-open-sourced-part-of-nvapi-sdk-to-aid-windows-emulation-environments)) | MIT/zlib | Forwards the *game's own* DLSS calls. Ships no NVIDIA DLL (NVIDIA relicensed `nvapi.h` as MIT for this purpose). |
| Mainline shadPS4, RPCS3, Dolphin, Ryujinx forks | GPL | No built-in DLSS found **(uncertain; not exhaustively checked)**. |
| Nexus Mods / Unigine community ([Nexus](https://forums.nexusmods.com/profile/61800016-other2w/), [Unigine](https://developer.unigine.com/forum/topic/8984-dlss-licensing-and-the-streamline-sdk)) | n/a | Common view: do not redistribute the DLSS DLLs outside a licensed application. |

**Options, from most to least conservative**

1. **User-supplied DLSS:** ship no NVIDIA binaries. Ship the bridge as a separate optional download, or
   have users build it, and have the user drop in `nvngx_dlss.dll`/`nvngx_dlssg.dll` from a game they own
   (OptiScaler/Blender model). The launcher can detect the files and show the NVIDIA license text.
2. **A separate "DLSS add-on" zip** with only `bbport_dlss.dll`, the NVIDIA DLLs and NVIDIA's license,
   kept out of the GPL package. This lowers but does not remove the "combined work" argument, because the
   same distributor still ships both.
3. **The current bundle**, with: clear notices that the NVIDIA files and the bridge *binary* are not
   GPL/MIT (the bridge binary contains `nvsdk_ngx_s.lib`, so "MIT" covers only its source); NVIDIA
   attribution in the launcher's About/credits (Supplement §7.1(b)); and no commercial release (so §4
   notification is not triggered).

### AMD FidelityFX SDK and FSR 3.1

- FireBurn's [FSR-Vulkan](https://github.com/FireBurn/FSR-Vulkan) and the FidelityFX SDK source used for
  FSR 3.1 are MIT, which is GPL-compatible. The only obligation is to keep the copyright and permission
  notice. **No AMD/FSR-Vulkan license text is currently in `licenses/`.** Add it.
- The current FidelityFX SDK `docs/license.md`
  ([raw](https://raw.githubusercontent.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/main/docs/license.md))
  has **two** licenses. A default AMD license ("distribute … in binary form only", "No reverse
  engineering, decompilation, or disassembly") applies to all files *not* listed. A long list of files is
  under an MIT-style license. Do not assume every file in an AMD SDK is MIT. Check which list a file is in.

### FSR 4 v07 assets and AMD's 2025 accidental source release

- **What happened.** Around 20 August 2025 AMD's FidelityFX SDK 2.0 update on GPUOpen GitHub briefly
  contained FSR 4 source, including an INT8 path, under the MIT license. AMD then removed it and said:
  "Some of our development code was posted on GPUOpen GitHub in error. It has since been taken down"
  ([HotHardware](https://hothardware.com/news/amd-responds-accidental-fsr4-release),
  [Guru3D](https://www.guru3d.com/story/amd-accidentally-opensources-fsr-4-code-on-github),
  [3DTested](https://www.3dtested.com/pc-components/gpus/amds-already-taken-down-mistakenly-released-fsr-4-source-code-but-the-internet-never-forgets-forked-github-repositories-remain-accessible)).
  Forks remained, and community INT8 builds followed
  ([Wccftech](https://wccftech.com/leaked-amd-fsr-4-int8-files-helps-users-turn-on-fsr-4-on-rx-7000-on-windows/)).
- **Legal status (uncertain).** Commentators argue the MIT grant cannot be revoked for people who received
  it ([itc.ua](https://itc.ua/en/news/amd-accidentally-opened-fsr-4-code-it-is-no-longer-possible-to-revoke-the-license),
  [ProfesionalReview](https://www.profesionalreview.com/2025/08/23/amd-fsr-4-irreversible-licencia-mit/)).
  HotHardware called this "highly arguable". There is no ruling, and AMD has not stated a position on
  downstream use. An open question is whether a license published "in error" by an employee binds the
  licensor (apparent authority, mistake). No AMD takedown of Q2RTX, OptiScaler or the shadPS4 FSR 4 forks
  was found.
- **What uncharted_pc ships.** The fetch script pulls `fsr4_model_v07_i8_*` assets from
  [FireBurn/Q2RTX](https://github.com/FireBurn/Q2RTX/tree/master/baseq2/fsr4_shaders) at commit
  `ae8d628`. Their `LICENSE-FSR4-v07.txt` says they were generated from a FidelityFX SDK fork revision
  under MIT, "Copyright (c) 2025 Advanced Micro Devices, Inc.", and that they are *not* AMD's
  binary-only 4.1.1 release. If MIT applies, shipping them with that notice (already inside
  `fsr4_shaders/`) complies. Copy the notice into `licenses/` as well. The remaining risk is AMD
  disputing the grant, which seems low given AMD's silence for over a year **(uncertain)**.
- Model **weights** may not be copyrightable at all in the US (they are machine-generated numbers). That
  question is open, and contract or trade-secret theories could still apply **(uncertain)**.

### FSR 4.1.1 assets extracted from AMD's DLL

- `tools/fsr4cap` captures the shaders and weights that AMD's `amd_fidelityfx_upscaler_dx12.dll` (4.1.x)
  dispatches under vkd3d-proton, then translates them to SPIR-V. The repository says these assets are
  never shipped publicly (`packaging/default.nix`), and the Windows `package.sh` does not include them.
- **License of the DLL.** The current FidelityFX SDK `docs/license.md` lists
  `Kits\FidelityFX\signedbin\amd_fidelityfx_upscaler_dx12.dll` (and the loader and frame generation DLLs)
  among the files under the **MIT-style** license, not the default "binary only, no reverse engineering"
  license. If that holds for the exact SDK release the DLL came from, extracting and redistributing
  derived assets with AMD's MIT notice would be allowed. This is surprising, may be a drafting artifact,
  and depends on the version. Check the `license.md` **in the specific SDK release tag** that shipped the
  4.1.1 DLL before relying on it **(uncertain)**.
- **Where the DLL came from matters.** A DLL taken from a game or from OptiScaler comes with that
  distributor's terms. A 4.1 INT8 DLL that **leaked via a Valve Proton Experimental depot**
  ([OC3D](https://overclock3d.net/news/software/valve-leaked-fsr-4-1-dll-works-on-amd-rdna-3-and-3-5-gpus/),
  [TechPowerUp](https://www.techpowerup.com/350170/leaked-fsr-4-1-int8-model-confirms-compatibility-with-rdna-3-5))
  should not be used as a source.
- **Recommendation:** keep the current policy (users generate the assets themselves from DLLs they have)
  unless the SDK license for that exact release is confirmed MIT. If you ship them, include AMD's MIT
  notice.

### FFmpeg build and GPL source obligations

- MSYS2's `mingw-w64-ffmpeg` (9.0.2) is configured with `--enable-gpl --enable-version3`, x264, x265,
  xvid, vidstab and others, and its package license is `GPL-3.0-or-later`
  ([PKGBUILD](https://github.com/msys2/MINGW-packages/blob/master/mingw-w64-ffmpeg/PKGBUILD)). So:
  - The distributed binary combination must follow **GPLv3**. That was fine while the project was GPL-2.0-or-later; it is not for the GPL-2.0-only project (see the 2026-11-12 update at the top)
    (OpenSSL 3's Apache-2.0 license is also compatible with v3, though not v2-only). It does mean the DLSS
    conflict is judged under v3.
  - **Source obligations:** anyone distributing GPL binaries must provide the Corresponding Source, or a
    written offer, for FFmpeg, x264, x265 and so on (GPLv3 §6). Pointing to MSYS2's source packages for
    the exact package versions is a common way to do it (§6(d) allows a third-party server if you make
    sure it stays available).
  - FFmpeg's [legal checklist](https://ffmpeg.org/legal.html) also asks you to credit FFmpeg, ship its
    license, and state how it was built.
- **Simpler option:** build an **LGPL-only FFmpeg** with just the decoders the cutscenes need (H.264 and
  AAC through Videodec2/AvPlayer), dynamically linked. This removes x264/x265 and GPLv3 from the bundle and
  makes license notices trivial. Patent licensing for H.264/AAC decoding is a separate, usually theoretical
  issue for non-commercial open source **(uncertain)**.

### PyInstaller, Python and Pillow

- PyInstaller is GPL-2.0-or-later **with a bootloader exception**: you may "link or embed compiled
  bootloader and related files into combinations with other programs, and to distribute those
  combinations without any restriction coming from the use of those files"
  ([license text](https://ftp.riken.jp/Linux/gentoo-portage/licenses/GPL-2+-with-Pyinstaller-Bootloader-exception),
  [SPDX: Bootloader-exception](https://scancode-licensedb.aboutcode.org/bootloader-exception)). Since the
  launcher is GPL anyway, the exception does not change much. Include PyInstaller's `COPYING.txt`.
- The frozen `Uncharted.exe` also contains CPython (PSF License), Tcl/Tk (BSD-style) and Pillow (MIT-CMU).
  All are GPL-compatible permissive licenses. Their texts should go in `licenses/`.

### Fonts

- **Cinzel (SIL OFL 1.1):** may be bundled with software, including commercial software, as long as the
  font is not sold by itself. Keep `OFL.txt`. A modified version may not use a Reserved Font Name
  ([OFL FAQ](https://ctan.math.washington.edu/tex-archive/fonts/cm-unicode/doc/OFL-FAQ.txt),
  [Wikipedia](https://en.wikipedia.org/wiki/SIL_Open_Font_License)). `OFL.txt` is currently only inside
  the PyInstaller bundle. Copy it to `licenses/`. Note for the trademark section: the font was picked to
  look like the Uncharted logo.
- **DejaVu** (Bitstream Vera and Arev licenses, permissive) is bundled only by the Linux Nix package
  (`packaging/default.nix`). Include its license there.

### Other bundled libraries

All are permissive and GPL-compatible (verify each against the vendored copy). SDL3 (zlib), Dear ImGui
(MIT), sirit (check its `LICENSE` in `gpu/third_party`), magic_enum (MIT), xbyak (BSD-3-Clause), VMA
(MIT), LibAtrac9 (MIT), nlohmann-json (MIT), miniz (MIT), cpp-httplib (MIT), wepoll (BSD-2). **Of these,
only nlohmann-json, wepoll and httplib licenses currently reach `licenses/`.** Most permissive licenses
require the notice to accompany binary copies, so the release is out of compliance on paper. Generating
`licenses/` from a manifest is a cheap fix.

---

## Publisher enforcement history

### Sony and Bloodborne fan projects (2023 to 2026)

| Date | Target | Action | Basis |
|---|---|---|---|
| 2023-11-29 | GitHub repo `D4rkMoons/CuS4-03173-Bl00dBorn` | SIE DMCA notice, whole repo ([notice](https://github.com/github/dmca/blob/master/2023/11/2023-11-29-sony.md)) | The repo held "Bloodborne's access keys" and "extracted game ROM data". |
| Jan 2025 | Lance McDonald's 2021 Bloodborne 60 fps patch | DMCA via MarkScan; links removed ([Push Square](https://www.pushsquare.com/news/2025/01/sony-issues-dmca-takedown-against-bloodborne-60fps-mod-four-years-after-release), [PC Gamer](https://www.pcgamer.com/games/action/sony-bullies-bloodborne-60fps-mod-off-the-internet-with-dmca-takedown-as-it-continues-to-let-the-fromsoft-classic-gather-dust-on-ps4/), [Siliconera](https://siliconera.com/bloodborne-60fps-patch-pulled-over-sony-dmca-takedown)) | McDonald later said the issue was the **trademarked word "Bloodborne"** on his page. The patch is "100% code" ([Push Square](https://www.pushsquare.com/news/2025/02/sonys-bloodborne-60fps-mod-dmca-takedown-was-about-the-name-not-the-patch-itself)). The same patch stayed in shadPS4. |
| Feb 2025 | Bloodborne PSX demake (itch.io) and YouTube videos | MarkScan copyright notices; the demake was pulled ([Push Square](https://pushsquare.com/news/2025/02/copium-as-second-bloodborne-fan-project-hit-by-sony-dmca-takedown), [Wikipedia](https://en.wikipedia.org/wiki/Bloodborne_PSX)) | Copyright/trademark in the IP. The earlier "Bloodborne Kart" had already been renamed "Nightmare Kart". |
| Mar 2025 (disclosed Feb 2026) | "Bloodborne: Top Down Arena" | C&D from Sony IP lawyers ([MP1st](https://mp1st.com/news/sony-blocked-fan-bloodborne-remake)) | Trademark. The developer moved the assets to his own IP. |
| 2025 | "Concord Delta" revived servers | MarkScan DMCA to YouTube ([Notebookcheck](https://www.notebookcheck.net/Concord-Delta-modders-bring-back-Sony-s-canceled-200M-shooter-before-DMCA-takedowns-throw-sand-in-the-gears.1164445.0.html)) | Videos of a reverse-engineered backend. |
| 2026-10-14 | "Overdrive" PS5/PS4 homebrew store (1,600+ games) | Taken offline. Reports of an SIE C&D are **unconfirmed** ([TechSpot](https://www.techspot.com/news/114203-sony-shut-down-overdrive-fake-playstation-store-pirated.html), [Tom's Hardware](https://www.tomshardware.com/video-games/playstation/ps5-homebrew-storefront-with-over-1-600-free-ps5-and-ps4-games-taken-down-by-sony-company-plays-piracy-whack-a-mole-as-two-further-repos-spring-up), [OneJailbreak](https://onejailbreak.com/blog/overdrive-ps5-store-closes/)) | Piracy. This happened two days ago. Sony's attention to PS4 piracy is high right now. |

Older precedent: Sony v. Hotz (PS3 root keys, 2011) settled with an injunction
([Wikipedia](https://en.wikipedia.org/wiki/Sony_Computer_Entertainment_America_v._Hotz)). It shows Sony
treats published **keys** as a top priority.

**Pattern:** Sony's agent MarkScan files notices on (1) keys and game data, (2) the **franchise name**
used as a project title, and (3) piracy distribution. Fan *code* that requires the user's own copy has
not been hit (shadPS4 has run Bloodborne since 2024 with no Sony action; a June 2026 report says Sony
"appears to be monitoring" it ([Mixvale](https://www.mixvale.com.br/2026/06/26/shadps4-lemulatore-ps4-ottiene-shadnet-per-la-rete-online-senza-psn-it/amp/))).
Notices against **pages** that carry the trademark were enough to remove the 60 fps patch.

### Nintendo v. Yuzu and Ryujinx (2024)

- **Yuzu (Tropic Haze), D.R.I., filed Feb 2024; settled Mar 2024 for US$2.4M** with a permanent injunction,
  shutdown, and handover of domains and code
  ([Siliconera](https://www.siliconera.com/yuzu-creators-pay-2-4-million-to-nintendo-as-settlement-fee/),
  [TechSpot](https://www.techspot.com/news/102128-yuzu-developer-pays-nintendo-24-million-settle-lawsuit.html),
  [Mondaq](https://webiis08.mondaq.com/unitedstates/copyright/1445382/nintendo-vs-yuzu-lawsuit-settlement-explained)).
  The complaint relied on DMCA §1201 anti-circumvention and anti-trafficking. What made Yuzu vulnerable:
  - **Monetization:** Patreon with more than 7,000 patrons, about US$30k/month, paid "early access"
    builds and game-specific fixes.
  - **Piracy facilitation:** guides for dumping keys and games, links to sites for `prod.keys`, and
    testing thousands of games, including Tears of the Kingdom fixes before release while the leaked game
    was pirated more than 1M times.
  - **Decryption:** the emulator decrypted games at run time with keys not obtained from Nintendo. The
    stipulated judgment says Yuzu "executes code that decrypts … using unauthorized copies of the Nintendo
    Switch cryptographic keys".
- **Ryujinx (Oct 2024):** the lead developer accepted an "agreement" after Nintendo contacted him, and the
  repository and organization disappeared without a lawsuit
  ([VGC](https://videogameschronicle.com/news/switch-emulator-ryujinx-has-been-taken-down-after-nintendo-contacted-its-lead-developer),
  [Nintendo Life](https://nintendolife.com/news/2024/10/nintendo-slays-switch-emulator-ryujinx)).
  Lesson: an identifiable lead developer can be pressured directly.
- Nintendo's March 2025 and 2026 GitHub §1201 notices removed thousands of fork repositories
  ([Gigazine](https://www.gigazine.net/news/20260421-dmca-github/),
  [XenoSpectrum](https://xenospectrum.com/en/nintendo-github-dmca-fork-networks/)). GitHub can disable an
  entire fork network in one notice.

### Dolphin on Steam (2023)

Valve asked Nintendo about Dolphin's planned Steam release. Nintendo's lawyers cited the DMCA and objected
that Dolphin included the Wii common key and decrypted games at run time. Valve then required Nintendo's
approval, and Dolphin dropped the Steam release. Dolphin itself was not taken down
([Game Informer](https://gameinformer.com/2023/05/29/nintendo-shuts-down-steam-launch-of-dolphin-emulator),
[TheSixthAxis](https://www.thesixthaxis.com/2023/05/30/dolphin-emulator-steam-release-blocked-by-nintendo-dmca-notice/),
[GamingOnLinux](https://www.gamingonlinux.com/2023/05/nintendo-hits-valve-with-dmca-against-the-dolphin-emulator-release/)).
Relevance: (a) embedded keys are the trigger, and (b) **Valve will not list** such software without the
platform holder's consent. See [Steam integration](#steam-integration).

### Bleem and Connectix

- **Sony v. Connectix (Virtual Game Station), 203 F.3d 596 (9th Cir. 2000):** the court held that
  intermediate copying of the PlayStation BIOS during reverse engineering was **fair use**, because it was
  needed to reach unprotected functional elements in order to build a non-infringing, interoperable
  product ([opinion](https://law.resource.org/pub/us/case/reporter/F3/203/203.F3d.596.99-15852.html),
  [Copyright Office summary](https://www.copyright.gov/fair-use/summaries/sony-connectix-9thcir2000.pdf)).
  Sony later bought VGS and discontinued it **(per secondary sources)**.
- **Sony v. Bleem, 214 F.3d 1022 (9th Cir. 2000):** using real PlayStation **screenshots** in comparative
  advertising was fair use
  ([opinion](https://law.resource.org/pub/us/case/reporter/F3/214/214.F3d.1022.99-17137.html),
  [summary](https://www.copyright.gov/fair-use/summaries/sonycomputer-bleem-9thcir2000.pdf)). Bleem won,
  but it reportedly went out of business partly because of legal costs
  ([Wikipedia](https://www.wikipedia.org/wiki/Bleem!)). Being right on the law does not stop a defendant
  from running out of money.

### Legal principles: Sega v. Accolade, Sony v. Connectix, DMCA 1201

- **Sega v. Accolade, 977 F.2d 1510 (9th Cir. 1992):** disassembling copyrighted object code is fair use
  "where disassembly is the only way to gain access to the ideas and functional elements" and there is a
  legitimate reason, such as compatibility
  ([summary](https://www.copyright.gov/fair-use/summaries/segaenters-accolade-9thcir1992.pdf),
  [opinion](https://web.mst.edu/canisr/legalenvironment/cases/sega.pdf)). The court also rejected Sega's
  trademark claim based on its TMSS startup screen, holding Sega responsible for the mislabeling its own
  lock-out caused.
- **Connectix** extended this to repeated intermediate copies and to emulators that compete with the
  console. Both cases protect *the act of reverse engineering to build interoperable code*. Neither
  protects **distributing the copyrighted code or assets themselves**.
- **These cases predate the DMCA.** 17 U.S.C. §1201 forbids circumventing access controls and trafficking
  in circumvention tools, whether or not there is infringement. §1201(f) allows circumvention by someone
  who lawfully obtained a program, only to achieve interoperability of an independently created program
  ([DisCo analysis](https://project-disco.org/intellectual-property/070617-interoperability-and-the-copyright-offices-section-1201-report/)).
  uncharted_pc **does not decrypt** PS4 SELFs or PFS (`scripts/prepare.py`: "No decryption"). That puts it
  in the shadPS4/bbport position: it needs an already decrypted dump. This is a real difference from
  Yuzu/Dolphin, where the software decrypted with console keys. The *user's* act of dumping from a
  jailbroken PS4 involves circumvention by the user. The project should not provide, link to, or explain
  how to do it (the Yuzu "facilitation" allegations).
- **How this project is built:** it runs Sony's original executables with HLE replacements for the
  system libraries, plus game-specific runtime patches. Distributing *patches* that contain no Sony bytes
  is generally treated like other mods. Avoid shipping copied Sony code, shaders, or data tables taken
  from the game.
- **EU/UK:** Software Directive 2009/24/EC Art. 6 allows decompilation for interoperability under narrow
  conditions. Anti-circumvention rules (InfoSoc Art. 6) apply as well. The principles are similar but the
  details differ **(not researched in depth)**.

### Status of bbport and the Bloodborne/Uncharted ports

As of 2026-10-16, fetched from GitHub pages and news searches:

- [deadinside28/bloodborne_pc](https://github.com/deadinside28/bloodborne_pc) (bbport, Linux):
  **public**, GPL-2.0, no takedown notice. The README says: "not affiliated with Sony Interactive
  Entertainment, FromSoftware or AMD"; "AMD's FSR 4 DLLs and model data and NVIDIA's DLSS SDK and
  libraries are not distributed here." Upstream bbport does **not** bundle DLSS.
- [Supermedo/bloodborne_pc](https://github.com/Supermedo/bloodborne_pc) (Windows): **public**, fork of
  bbport, GPL-2.0, no donation links visible, no takedown.
- [Supermedo/uncharted_pc](https://github.com/Supermedo/uncharted_pc): **public**, about 976 stars and 47
  forks, Discord link only, no donation links, no takedown. It has had press coverage
  ([GameplaysCassi](https://gameplayscassi.com.br/noticias/uncharted-the-nathan-drake-collection-recebe-port-nativo-nao-oficial-para-pc/134272/),
  [X/Pirat_Nation](https://x.com/Pirat_Nation/status/2110461518374449594), mirrors such as
  [bc160201983/…](https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port)).
- **No Sony DMCA notice against any of these was found**, either in the
  [github/dmca](https://github.com/github/dmca) index searches or in news
  ([search summary](https://gigazine.net/gsc_news/en/20260421-dmca-github)). The only Sony GitHub notice
  found is the 2023 key/ROM-data one above **(absence of evidence is not proof; the github/dmca repository
  was not exhaustively grepped)**.
- Context: Sony sells *Uncharted: Legacy of Thieves Collection* on PC (Steam/Epic, 19 Oct 2022,
  [GameSpot](https://gamespot.com/articles/uncharted-legacy-of-thieves-collection-hits-pc-october-19-system-requirements-and-features-revealed/1100-6507543/)).
  The Nathan Drake Collection has no PC release. A free port of a franchise Sony monetizes on PC carries
  more commercial sensitivity than Bloodborne, where Sony has been inactive.

---

## Risk factors and good practices

### Game data, keys and dump instructions

Already done: no game files, no keys, no decryption, and a "your own decrypted dump" notice.

Keep doing, and write it into `CONTRIBUTING`/issue templates:

- Never commit or accept in issues: keys, `eboot.bin`/`.elf`, `sce_sys` contents, shader dumps from the
  game, save files with game data, or screenshots of piracy sources. Ask for logs with paths redacted.
  Close issues that mention pirated sources or "repacks" (the Yuzu complaint cited this kind of
  tolerance).
- Describe requirements in neutral terms: "a decrypted dump of your own copy of CUSA02343/CUSA02320". Do
  **not** link dumping guides, jailbreak tools, FTP payloads, PKG sites or "where to get it" Discord
  channels, and moderate Discord the same way. Naming title IDs and expected file layouts is fine and
  standard (shadPS4 does it).
- Do not publish fixes or compatibility work for **unreleased** or leaked builds (the Tears of the Kingdom
  issue).
- Remove leftovers like `patches/Bloodborne.xml` from Uncharted releases unless they are needed.

### Artwork and trademarks

- **Art:** the original covers and icon are good practice. Official box art taken from the user's
  `sce_sys` and kept local (Steam grid) is the user's own copy and is not distributed, which is fine. Do
  not upload official art to the repository, release zips, websites or social media banners. *Bleem*
  shows that screenshots for comparison can be fair use, but it was litigated, so avoid it in branding.
- **Trademark ("Uncharted", "Nathan Drake", "Naughty Dog", "PlayStation"):** the McDonald, Top Down Arena
  and PSX demake cases show Sony enforces the **name used as a title or brand**. Nominative fair use
  allows naming the product you are compatible with if (1) it is necessary, (2) you use only as much as
  needed, and (3) you do not suggest endorsement (*New Kids on the Block v. News America*, 971 F.2d 302,
  9th Cir. 1992; [Playboy v. Welles](https://law.resource.org/pub/us/case/reporter/F3/279/279.F3d.796.00-55538.00-55537.00-55229.00-55009.html)).
  Circuits apply this differently
  ([Arnold & Porter](https://www.arnoldporter.com/en/perspectives/publications/2016/11/2016_11_16_second_circuit_expands_split__13324)).
  Current risk points:
  - The repo, exe and window titles are "Uncharted" / `Uncharted.exe` / "Play Uncharted.exe", so the
    product is *named* after the mark rather than *describing compatibility* with it.
  - The launcher font was picked to resemble the Uncharted logo (trade dress), and the Steam shortcut is
    titled "Uncharted: The Nathan Drake Collection".
  - Lower-risk naming: a neutral project name with a descriptive subtitle ("… a native runtime for *Uncharted:
    The Nathan Drake Collection* (PS4)"), a non-logo-like typeface, the disclaimer kept prominent, and no
    PlayStation or Naughty Dog logos. Renaming is a trade-off against discoverability. Keep at least the
    trade-dress and disclaimer items.
- **NVIDIA marks:** the license *requires* attribution (Supplement §7.1(b)) but restricts combining
  NVIDIA marks with other content without approval (§7.3). Plain-text "NVIDIA DLSS" credits with the
  trademark notice are the usual practice. Do not use NVIDIA/RTX logos.

### Donations and monetization

- Upstream removed its donation links, which was a good move. Yuzu's Patreon (paid early access, about
  US$30k/month) was central to Nintendo's narrative of "facilitating piracy at a colossal scale" for
  profit, and commercial purpose weighs against fair use and raises damages exposure.
- If funding is ever reconsidered: no paid builds, no early access, no per-game bounties for a specific
  Sony title, and no ads on download pages. Prefer contributions to upstream infrastructure (shadPS4,
  bbport) or general open-source funds. Note that NVIDIA's DLSS license requires notifying NVIDIA before
  any *commercial* release (Supplement §4), and monetization may be what counts as commercial.
- Keep developers' personal details limited. Ryujinx shows that an identifiable lead developer can be
  pressured directly.

### Auto-updater security

Current flow (`latest_release()` / `install_update()`): GitHub API over HTTPS, take the first `.zip`
asset, `extractall`, run the new `Uncharted.exe --install-update` from it. HTTPS protects the transport,
but **anyone who controls the GitHub account, a token, or the release can push code to every user**, and
nothing checks integrity. Recommended:

1. **Sign releases.** Publish `SHA256SUMS` plus an Ed25519 signature (minisign/signify) and embed the
   public key in the launcher. Verify before extracting, and refuse unsigned or mismatched updates. This
   costs nothing and is independent of GitHub.
2. **GitHub immutable releases** (GA October 2025) lock the assets and tag and add a Sigstore release
   attestation, checkable with `gh release verify-asset`
   ([changelog](https://github.blog/changelog/2025-10-28-immutable-releases-are-now-generally-available/),
   [docs](https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/verify-release-integrity)).
   Build releases in CI from tagged source, not on a personal machine. This also covers the GPL "binary
   matches source" expectation.
3. **Authenticode** reduces SmartScreen warnings. [SignPath Foundation](https://signpath.org/terms) signs
   free OSS projects, but it requires an OSI license and **no proprietary components**, so the bundled
   DLSS DLLs would probably disqualify the package (another reason to split them out).
4. Match the asset **by exact name**, not "first .zip". Pin the repository and **decide what a fork does**:
   this fork's `RELEASES_API` points at `Supermedo/uncharted_pc`, so fork users would silently get
   upstream builds, overwriting fork changes and trusting upstream's signing.
5. Keep the update opt-out (already present on the Advanced page). Show the version and changelog before
   installing (already partly present). Python's `zipfile.extractall` strips `..` and absolute paths, but
   validate member paths anyway and extract into a fresh directory.
6. FSR 4 asset downloads in the Bloodborne launcher (`FSR4_BASE` raw.githubusercontent at a pinned
   commit) should also check SHA-256 values. Q2RTX's manifests already record them.

---

## Steam integration

- **Mechanism:** "Add to Steam" writes a non-Steam shortcut into `userdata/<id>/config/shortcuts.vdf`.
  Steam supports this officially through "Add a Non-Steam Game"
  ([Valve Developer Wiki](https://developer.valvesoftware.com/wiki/Add_Non-Steam_Game)). Tools like Steam
  ROM Manager, Heroic and Lutris do the same thing. No Steam Subscriber Agreement term or Valve policy
  forbidding third-party tools from writing shortcuts was found **(no official Valve statement
  located)**. Using it for an emulator-like port is not a policy issue in itself.
- **The implementation is careful:** it asks before shutting Steam down (Steam overwrites the file
  otherwise), keeps `shortcuts.vdf.bak`, and generates grid art locally from the user's own dump. Steam
  may add fields over time that a rewrite could drop
  ([steam-static-shortcuts](https://github.com/amini-allight/steam-static-shortcuts)), so keep the
  unknown-field round-trip as it is.
- **Things to watch:**
  - Friends see "playing *Uncharted: The Nathan Drake Collection*" as a non-Steam game. That is
    harmless, but it puts the trademarked name in public presence. A neutral shortcut name is an option.
  - **Do not list the project on the Steam store** or in Steam Workshop. Dolphin shows Valve requires
    the platform holder's approval for such software.
  - Do not upload generated grid art to SteamGridDB or similar from the launcher (that would be
    distributing official art).
  - Steam Input and the overlay work with non-Steam shortcuts, and no Steamworks SDK is linked, so no
    Steamworks license applies.

---

## Recommended actions

Ordered by value for effort:

1. **Licenses folder:** add texts for FFmpeg (GPLv3) plus x264/x265 and the other GPL codec libraries,
   SDL3, Dear ImGui, sirit, magic_enum, xbyak, VMA, LibAtrac9, FSR-Vulkan/FidelityFX (MIT), the FSR 4 v07
   notice, PyInstaller, CPython, Tcl/Tk, Pillow, Cinzel OFL, and the Vulkan loader. Generate it from the
   `ldd` list plus a manifest.
2. **GPL source availability:** add a `SOURCE.txt` that names the exact git commit and the MSYS2 package
   versions, with links to their source packages, or switch to an LGPL-only, decoder-only FFmpeg.
3. **DLSS:** either move to user-supplied NVIDIA DLLs (and an optional bridge), or at least ship them
   with a notice that they and `bbport_dlss.dll` are **not** GPL/MIT, add NVIDIA credit in the launcher
   About, and stay non-commercial.
4. **Updater:** signed checksums, immutable releases, exact asset name, and a deliberate update source
   for the fork.
5. **Trademark hygiene:** keep the disclaimer prominent, use a non-logo-like font, consider a neutral
   product name with "for *Uncharted: The Nathan Drake Collection*" as a description, and prune Bloodborne
   leftovers.
6. **Community rules:** a no-piracy policy for issues and Discord, no dumping guides, no donations or
   early access.
7. **FSR 4.1.1:** keep it user-generated. Confirm the AMD SDK license of the exact DLL release before any
   redistribution.

---

## Sources

**Licenses and primary documents**
- NVIDIA RTX SDKs License (v. 14 Mar 2024), DLSS repo: https://github.com/NVIDIA/DLSS/blob/main/LICENSE.txt
- NVIDIA RTX SDKs License 23 Jan 2023 (PDF): https://developer.download.nvidia.com/gameworks/NVIDIA-RTX-SDKs-License-23Jan2023.pdf
- AMD FidelityFX SDK license: https://raw.githubusercontent.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/main/docs/license.md · repo: https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK
- Q2RTX FSR 4 v07 assets and notice: https://github.com/FireBurn/Q2RTX/tree/master/baseq2/fsr4_shaders
- MSYS2 FFmpeg PKGBUILD: https://github.com/msys2/MINGW-packages/blob/master/mingw-w64-ffmpeg/PKGBUILD · FFmpeg legal: https://ffmpeg.org/legal.html
- GNU GPL FAQ: https://www.gnu.org/licenses/gpl-faq.html (GPLPluginsInNF, GPLIncompatibleLibs)
- PyInstaller bootloader exception: https://ftp.riken.jp/Linux/gentoo-portage/licenses/GPL-2+-with-Pyinstaller-Bootloader-exception · https://scancode-licensedb.aboutcode.org/bootloader-exception
- SIL OFL FAQ: https://ctan.math.washington.edu/tex-archive/fonts/cm-unicode/doc/OFL-FAQ.txt · https://en.wikipedia.org/wiki/SIL_Open_Font_License
- Sony DMCA notice to GitHub (2023-11-29): https://github.com/github/dmca/blob/master/2023/11/2023-11-29-sony.md

**DLSS in open-source projects**
- IFreemz/shadPS4-Bloodborne-DLSS-FSR: https://github.com/IFreemz/shadPS4-Bloodborne-DLSS-FSR
- Phoronix, DLSS for Blender licensing (Feb 2026): https://www.phoronix.com/news/NVIDIA-DLSS-Blender
- dlss_wgpu docs: https://docs.rs/dlss_wgpu
- OptiScaler mirror: https://gitee.com/mirrors/OptiScaler
- Unigine forum on DLSS licensing: https://developer.unigine.com/forum/topic/8984-dlss-licensing-and-the-streamline-sdk
- NVIDIA forum, Unity DLSS DLL licensing: https://forums.developer.nvidia.com/t/updating-unity-dlss-nvngx-dlss-dll-file-licensing/329558
- GamingOnLinux, NVAPI header MIT: https://gamingonlinux.com/2020/07/nvidia-open-sourced-part-of-nvapi-sdk-to-aid-windows-emulation-environments
- GPL plug-in debates: https://lists.linuxaudio.org/archives/linux-audio-dev/2010-June/027293.html · https://lists.blender.org/pipermail/bf-committers/2007-May/018374.html

**FSR 4 release and leaks**
- Guru3D: https://www.guru3d.com/story/amd-accidentally-opensources-fsr-4-code-on-github
- HotHardware (AMD statement): https://hothardware.com/news/amd-responds-accidental-fsr4-release
- 3DTested: https://www.3dtested.com/pc-components/gpus/amds-already-taken-down-mistakenly-released-fsr-4-source-code-but-the-internet-never-forgets-forked-github-repositories-remain-accessible
- itc.ua: https://itc.ua/en/news/amd-accidentally-opened-fsr-4-code-it-is-no-longer-possible-to-revoke-the-license
- ProfesionalReview: https://www.profesionalreview.com/2025/08/23/amd-fsr-4-irreversible-licencia-mit/
- Wccftech (INT8 builds): https://wccftech.com/leaked-amd-fsr-4-int8-files-helps-users-turn-on-fsr-4-on-rx-7000-on-windows/
- OC3D (Valve-leaked 4.1 DLL): https://overclock3d.net/news/software/valve-leaked-fsr-4-1-dll-works-on-amd-rdna-3-and-3-5-gpus/
- TechPowerUp (4.1 INT8 leak): https://www.techpowerup.com/350170/leaked-fsr-4-1-int8-model-confirms-compatibility-with-rdna-3-5

**Enforcement history**
- Push Square, 60 fps takedown: https://www.pushsquare.com/news/2025/01/sony-issues-dmca-takedown-against-bloodborne-60fps-mod-four-years-after-release
- Push Square, "about the name": https://www.pushsquare.com/news/2025/02/sonys-bloodborne-60fps-mod-dmca-takedown-was-about-the-name-not-the-patch-itself
- PC Gamer: https://www.pcgamer.com/games/action/sony-bullies-bloodborne-60fps-mod-off-the-internet-with-dmca-takedown-as-it-continues-to-let-the-fromsoft-classic-gather-dust-on-ps4/
- Siliconera: https://siliconera.com/bloodborne-60fps-patch-pulled-over-sony-dmca-takedown
- Push Square, PSX demake: https://pushsquare.com/news/2025/02/copium-as-second-bloodborne-fan-project-hit-by-sony-dmca-takedown · Wikipedia: https://en.wikipedia.org/wiki/Bloodborne_PSX
- MP1st, Top Down Arena C&D: https://mp1st.com/news/sony-blocked-fan-bloodborne-remake
- Notebookcheck, Concord Delta: https://www.notebookcheck.net/Concord-Delta-modders-bring-back-Sony-s-canceled-200M-shooter-before-DMCA-takedowns-throw-sand-in-the-gears.1164445.0.html
- Overdrive: https://www.techspot.com/news/114203-sony-shut-down-overdrive-fake-playstation-store-pirated.html · https://www.tomshardware.com/video-games/playstation/ps5-homebrew-storefront-with-over-1-600-free-ps5-and-ps4-games-taken-down-by-sony-company-plays-piracy-whack-a-mole-as-two-further-repos-spring-up · https://onejailbreak.com/blog/overdrive-ps5-store-closes/
- shadPS4 and Sony monitoring (Mixvale, Jun 2026): https://www.mixvale.com.br/2026/06/26/shadps4-lemulatore-ps4-ottiene-shadnet-per-la-rete-online-senza-psn-it/amp/
- Sony v. Hotz: https://en.wikipedia.org/wiki/Sony_Computer_Entertainment_America_v._Hotz
- Yuzu: https://www.siliconera.com/yuzu-creators-pay-2-4-million-to-nintendo-as-settlement-fee/ · https://www.techspot.com/news/102128-yuzu-developer-pays-nintendo-24-million-settle-lawsuit.html · https://webiis08.mondaq.com/unitedstates/copyright/1445382/nintendo-vs-yuzu-lawsuit-settlement-explained · https://steamdeckhq.com/news/nintendo-v-yuzu-court-case/
- Ryujinx: https://videogameschronicle.com/news/switch-emulator-ryujinx-has-been-taken-down-after-nintendo-contacted-its-lead-developer · https://nintendolife.com/news/2024/10/nintendo-slays-switch-emulator-ryujinx
- GitHub DMCA trends 2025–26: https://www.gigazine.net/news/20260421-dmca-github/ · https://xenospectrum.com/en/nintendo-github-dmca-fork-networks/
- Dolphin on Steam: https://gameinformer.com/2023/05/29/nintendo-shuts-down-steam-launch-of-dolphin-emulator · https://www.thesixthaxis.com/2023/05/30/dolphin-emulator-steam-release-blocked-by-nintendo-dmca-notice/ · https://www.gamingonlinux.com/2023/05/nintendo-hits-valve-with-dmca-against-the-dolphin-emulator-release/

**Case law and statutes**
- Sony v. Connectix: https://law.resource.org/pub/us/case/reporter/F3/203/203.F3d.596.99-15852.html · https://www.copyright.gov/fair-use/summaries/sony-connectix-9thcir2000.pdf
- Sony v. Bleem: https://law.resource.org/pub/us/case/reporter/F3/214/214.F3d.1022.99-17137.html · https://www.copyright.gov/fair-use/summaries/sonycomputer-bleem-9thcir2000.pdf · https://www.wikipedia.org/wiki/Bleem!
- Sega v. Accolade: https://www.copyright.gov/fair-use/summaries/segaenters-accolade-9thcir1992.pdf · https://web.mst.edu/canisr/legalenvironment/cases/sega.pdf
- §1201(f) and the Copyright Office report: https://project-disco.org/intellectual-property/070617-interoperability-and-the-copyright-offices-section-1201-report/
- Nominative fair use: https://law.resource.org/pub/us/case/reporter/F3/279/279.F3d.796.00-55538.00-55537.00-55229.00-55009.html · https://www.arnoldporter.com/en/perspectives/publications/2016/11/2016_11_16_second_circuit_expands_split__13324

**Project status and context**
- https://github.com/Supermedo/uncharted_pc · https://github.com/Supermedo/bloodborne_pc · https://github.com/deadinside28/bloodborne_pc
- Coverage: https://gameplayscassi.com.br/noticias/uncharted-the-nathan-drake-collection-recebe-port-nativo-nao-oficial-para-pc/134272/ · https://x.com/Pirat_Nation/status/2110461518374449594 · https://github.com/bc160201983/Uncharted-The-Nathan-Drake-Collection-PC-port
- Legacy of Thieves on PC: https://gamespot.com/articles/uncharted-legacy-of-thieves-collection-hits-pc-october-19-system-requirements-and-features-revealed/1100-6507543/

**Updater and Steam**
- GitHub immutable releases: https://github.blog/changelog/2025-10-28-immutable-releases-are-now-generally-available/ · https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/verify-release-integrity
- SignPath Foundation terms: https://signpath.org/terms
- Valve, Add Non-Steam Game: https://developer.valvesoftware.com/wiki/Add_Non-Steam_Game
- steam-static-shortcuts: https://github.com/amini-allight/steam-static-shortcuts
