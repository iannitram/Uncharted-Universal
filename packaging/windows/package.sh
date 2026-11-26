#!/usr/bin/env bash
# Builds dist/uncharted-windows/ (and dist/uncharted-windows.zip): uncharted-app.exe (the Rust
# launcher, roadmap 2.10) with uncharted-universal.exe (the command line; Steam shortcuts run
# `uncharted-universal.exe play u1`, in place of Play Uncharted.exe) and uncharted-run.exe,
# bb-probe.exe with the MSYS2 CLANG64 DLLs it needs.
# Run from an MSYS2 CLANG64 shell after `bash build.sh`.
set -euo pipefail
cd -- "$(dirname -- "$0")/../.."
# The Rust faces, with the host's (MSVC) toolchain, before CLANG64 comes first on PATH. The C
# runtime is linked into these faces. The separately released Save Relay helper needs the x64 VC++ runtime.
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="${CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS:-} -C target-feature=+crt-static"
# shellcheck source=packaging/rust_faces.sh
source packaging/rust_faces.sh
rust_host=$(rustc -vV | sed -n 's/^host: //p')
source ./msys2-env.sh
[[ -f out/bb-probe.exe && -f out/bb-gpu-capabilities.exe ]] || { echo 'Build first: bash build.sh' >&2; exit 1; }

# The package is assembled in a fresh staging folder and zipped from there; dist/uncharted-windows
# (a playable copy that may hold saves and settings) is only refreshed afterwards.
dest=out/stage/uncharted-windows
rm -rf -- out/stage
mkdir -p "$dest/bin" "$dest/licenses"
# The Rust faces at the top, where the app and the command line look for each other. Release
# builds carry no debug information (the .pdb stays in the target folder).
for face in "${faces[@]}"; do cp "$faces_dir/$face.exe" "$dest/"; done
# The executables without debug information (out/ keeps the symbols for crash reports).
for exe in bb-probe.exe bb-gpu-capabilities.exe; do
    llvm-strip --strip-debug -o "$dest/bin/$exe" "out/$exe"
done
# Every DLL the executables import, and the DLLs those import, read from their import tables
# (packaging/windows/dll_imports.py): FFmpeg from the LGPL-2.1 build of build_ffmpeg.sh (owner
# ruling 2026-11-26, #625), everything else (SDL3, libc++, ...) from the CLANG64 tree. MSYS2's
# own FFmpeg is GPL and version 3 (#299) and is never shipped: licence_check.py ffmpeg refuses
# any FFmpeg DLL built with --enable-gpl, --enable-version3 or --enable-nonfree.
ffmpeg=out/ffmpeg-lgpl
[[ -f $ffmpeg/SOURCE.txt && -f $ffmpeg/COPYING.LGPLv2.1 ]] ||
    { echo 'Build the LGPL-2.1 FFmpeg DLLs first: bash packaging/windows/build_ffmpeg.sh' >&2; exit 1; }
ffmpeg_bin=$(cygpath -am "$ffmpeg/bin")
python3 -I packaging/windows/dll_imports.py closure "$ffmpeg_bin" "$(cygpath -am /clang64/bin)" -- \
    "$dest/bin/bb-probe.exe" "$dest/bin/bb-gpu-capabilities.exe" > out/bundled-dlls.txt ||
    { echo 'An imported DLL was found in neither the FFmpeg build nor CLANG64 (see above).' >&2; exit 1; }
mapfile -t ffmpeg_dlls < <(python3 -I -c "
import sys; sys.path.insert(0, 'packaging')
import licence_check
for line in open('out/bundled-dlls.txt'):
    if licence_check.is_ffmpeg_library(line.strip().rsplit('/', 1)[-1]): print(line.strip())" | tr -d '\r')
[[ ${#ffmpeg_dlls[@]} -gt 0 ]] || { echo 'bb-probe.exe imports no FFmpeg DLL: check out/bundled-dlls.txt' >&2; exit 1; }
for dll in "${ffmpeg_dlls[@]}"; do
    [[ $dll == "$ffmpeg_bin"/* ]] || { echo "FFmpeg DLL not from the LGPL build: $dll" >&2; exit 1; }
done
python3 -I packaging/licence_check.py ffmpeg "${ffmpeg_dlls[@]}" ||
    { echo 'Refusing to package: FFmpeg must be an LGPL-2.1 build (no GPL, version 3 or nonfree parts).' >&2; exit 1; }
while read -r dll; do cp -u "$dll" "$dest/bin/"; done < out/bundled-dlls.txt
python3 packaging/windows/bundle_relay.py "$dest" out/component-cache
cp LICENSE "$dest/"
cp packaging/windows/README-Uncharted.txt "$dest/README.txt"
# Texts for code built into the executables. The DLLs' texts come from licence_check.py below
# (pacman, else this folder). Not copied: the GPL-3.0 MSYS2 FFmpeg and the libraries only it
# linked (x264, x265, lame, ...), none of which ships since #625.
msys2_ffmpeg_only=' ffmpeg gmp gnutls lame libidn2 libssh libtheora libvorbis libvpx libx264 nettle opencore-amr p11-kit rtmpdump x265 xvidcore '
for f in packaging/windows/licenses/*-LICENSE.txt; do
    name=${f##*/}; name=${name%-LICENSE.txt}
    [[ $msys2_ffmpeg_only == *" $name "* ]] || cp "$f" "$dest/licenses/"
done
# FFmpeg (LGPL-2.1): the licence, the notice and where its source is.
mkdir -p "$dest/licenses/ffmpeg"
cp "$ffmpeg/COPYING.LGPLv2.1" "$dest/licenses/ffmpeg/COPYING.LGPLv2.1"
{
    echo 'This package includes FFmpeg (https://ffmpeg.org), licensed under the GNU Lesser General'
    echo 'Public License version 2.1 or later (COPYING.LGPLv2.1 in this folder). FFmpeg is a trademark'
    echo 'of Fabrice Bellard. These DLLs are built from the unmodified official release source,'
    echo 'without --enable-gpl, --enable-version3 or --enable-nonfree and without external libraries:'
    printf '  %s\n' "${ffmpeg_dlls[@]##*/}"
    echo
    sed 's/^/  /' "$ffmpeg/SOURCE.txt"
    echo
    echo 'Source offer: the complete corresponding source of this FFmpeg build is the tarball above,'
    echo 'also kept with this project'"'"'s build script (packaging/windows/build_ffmpeg.sh, which holds'
    echo 'the exact configure line) at https://github.com/iannitram/Uncharted-Universal. For three years'
    echo 'from the date you received this package you may also ask for a copy of that source through'
    echo 'https://github.com/iannitram/Uncharted-Universal/issues. You may replace these DLLs with your'
    echo 'own build of the same FFmpeg version (same DLL names).'
} > "$dest/licenses/ffmpeg/FFmpeg-NOTICE.txt"
# The crates statically linked into the Rust faces.
python3 packaging/rust_licences.py "$dest/licenses/rust-crates.txt" --target "$rust_host"
# FSR 4 (the v07 INT8 model) when its assets are in fsr4_shaders/ (tools/fetch_fsr4_assets.sh).
if [[ -d fsr4_shaders ]]; then cp -r fsr4_shaders "$dest/"; fi
# DLSS and DLSS Frame Generation (NVIDIA RTX): the MSVC-built bridge and NVIDIA's runtimes, next
# to bb-probe.exe (packaging/windows/build_dlss.sh). Without them those options stay unavailable.
if [[ -f out/bbport_dlss.dll && -f out/nvngx_dlss.dll ]]; then
    cp out/bbport_dlss.dll out/nvngx_dlss.dll "$dest/bin/"
    if [[ -f out/nvngx_dlssg.dll ]]; then cp out/nvngx_dlssg.dll "$dest/bin/"; fi
    cp out/NVIDIA-DLSS-LICENSE.txt "$dest/licenses/"
    cp gpu/dlss_bridge/LICENSE.txt "$dest/licenses/bbport_dlss-LICENSE.txt"
else
    echo "DLSS bridge not built (packaging/windows/build_dlss.sh): no DLSS in this package" >&2
fi
# Intel XeSS (any GPU): Intel's libxess.dll next to bb-probe.exe (packaging/windows/fetch_xess.sh).
# Without it the XeSS option stays hidden.
if [[ -f out/libxess.dll && -f out/Intel-XeSS-LICENSE.txt ]]; then
    cp out/libxess.dll "$dest/bin/"
    cp out/Intel-XeSS-LICENSE.txt out/Intel-XeSS-third-party-programs.txt "$dest/licenses/"
else
    echo "XeSS not fetched (packaging/windows/fetch_xess.sh): no XeSS in this package" >&2
fi
for license in gpu/third_party/wepoll/LICENSE:wepoll gpu/third_party/httplib/LICENSE:cpp-httplib; do
    if [[ -f ${license%%:*} ]]; then cp "${license%%:*}" "$dest/licenses/${license##*:}-LICENSE.txt"; fi
done
# Every bundled DLL: its MSYS2 package's licence files go to licenses/<package>/. A DLL with no
# owning package or no licence files fails the package step with its name (#302). FFmpeg is not an
# MSYS2 package here: its texts are in licenses/ffmpeg/ (above). pacman wants /clang64/... paths.
clang64=$(cygpath -am /clang64)
mapfile -t dlls < <(grep -vF "$ffmpeg_bin/" out/bundled-dlls.txt | sed "s#^$clang64/#/clang64/#")
# Libraries linked statically into the binaries (gpu/CMakeLists.txt) ship no DLL; their archives
# name the packages whose licences must ship too.
for archive in libprotobuf libabsl_base libssl libminiupnpc libz; do dlls+=("/clang64/lib/$archive.a"); done
# MSYS2's own Python (CLANG64), so pacman is on its PATH; MSYS2_ARG_CONV_EXCL keeps the /clang64 paths
# as they are for pacman.
MSYS2_LICENSES=$(cygpath -m /clang64/share/licenses) MSYS2_ARG_CONV_EXCL='*' \
    python3 packaging/licence_check.py windows "$dest/licenses" "${dlls[@]}" ||
    { echo 'A bundled library has no licence text (see above).' >&2; exit 1; }
# Every non-system DLL each shipped .exe and .dll imports must be in its own folder of the zip
# (#625: bb-probe.exe without FFmpeg exited 0xC0000135). save-relay.exe imports VCRUNTIME140.dll,
# which a clean Windows lacks: the app-local copy comes from Visual Studio's Redist folder
# (Microsoft's Distributable Code), or BB_VC_CRT names that Microsoft.VC14x.CRT folder.
crt=${BB_VC_CRT:-$(ls -d /c/Program\ Files*/Microsoft\ Visual\ Studio/*/*/VC/Redist/MSVC/*/x64/Microsoft.VC14*.CRT \
    2>/dev/null | sort -V | tail -n 1)}
python3 -I packaging/windows/dll_imports.py check "$(cygpath -m "$dest")" ${crt:+--crt "$(cygpath -m "$crt")"} ||
    { echo 'Refusing to package: a shipped program imports a DLL the zip does not have (see above).' >&2; exit 1; }
mkdir -p dist
rm -f dist/uncharted-windows.zip
# Not Compress-Archive, which writes backslashes that Linux tools keep in the file names (#246).
(cd out/stage && python3 -c "
import pathlib, zipfile
with zipfile.ZipFile('../../dist/uncharted-windows.zip', 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for p in sorted(pathlib.Path('uncharted-windows').rglob('*')): z.write(p)")

# Refresh dist/uncharted-windows, keeping what players create there (saves, settings), and only
# while nothing runs from it: deleting a running launcher's files breaks it.
play=dist/uncharted-windows
running=$(powershell -NoProfile -Command \
    "@(Get-Process | Where-Object { \$_.Path -like '$(cygpath -w "$PWD/$play")\\*' }).Count" | tr -d '\r')
if [[ ${running:-0} != 0 ]]; then
    echo "dist/uncharted-windows is in use ($running processes): not refreshed; the zip is ready." >&2
else
    mkdir -p "$play"
    find "$play" -mindepth 1 -maxdepth 1 ! -name user ! -name out ! -name art ! -name bbport.ini \
        ! -name input.ini ! -name last_run.log -exec rm -rf {} +
    cp -r "$dest/." "$play/"
fi
du -sh "$dest" dist/uncharted-windows.zip
