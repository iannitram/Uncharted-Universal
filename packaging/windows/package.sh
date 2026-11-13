#!/usr/bin/env bash
# Builds dist/uncharted-windows/ (and dist/uncharted-windows.zip): uncharted-app.exe (the Rust
# launcher, roadmap 2.10) with uncharted-universal.exe (the command line; Steam shortcuts run
# `uncharted-universal.exe play u1`, in place of Play Uncharted.exe) and uncharted-run.exe,
# Uncharted.exe (the classic launcher, frozen with PyInstaller so players need no Python, until
# roadmap 2.11), bb-probe.exe with the MSYS2 CLANG64 DLLs it needs, the preparation scripts and
# run.py. Run from an MSYS2 CLANG64 shell after `bash build.sh`. Freezing uses a Windows Python 3.10+ (python.org; WINPYTHON
# overrides) and a private venv in out/pyenv with PyInstaller and Pillow.
set -euo pipefail
cd -- "$(dirname -- "$0")/../.."
cargo run --release -p uncharted_cli -- generate-art "$PWD/launcher/covers"
# The Rust faces, with the host's (MSVC) toolchain, before CLANG64 comes first on PATH. The C
# runtime is linked in, so a player needs no Visual C++ redistributable.
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="${CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS:-} -C target-feature=+crt-static"
# shellcheck source=packaging/rust_faces.sh
source packaging/rust_faces.sh
rust_host=$(rustc -vV | sed -n 's/^host: //p')
source ./msys2-env.sh
[[ -f out/bb-probe.exe && -f out/bb-gpu-capabilities.exe ]] || { echo 'Build first: bash build.sh' >&2; exit 1; }

# A Windows Python (not MSYS2's) for PyInstaller.
python=${WINPYTHON:-}
if [[ -z $python ]]; then
    for candidate in /c/Python3*/python.exe "${LOCALAPPDATA:-/c/Users/$USER/AppData/Local}"/Programs/Python/Python3*/python.exe; do
        [[ -x $candidate ]] && python=$candidate
    done
fi
[[ -n $python ]] || { echo 'Need a Windows Python 3 (python.org) or WINPYTHON=path\to\python.exe' >&2; exit 1; }
# Windows Python needs USERPROFILE (some MSYS2 shells start without it).
export USERPROFILE=${USERPROFILE:-$(cygpath -w "/c/Users/$(id -un)")}
if [[ ! -x out/pyenv/Scripts/python.exe ]]; then
    "$python" -m venv out/pyenv
fi
out/pyenv/Scripts/python.exe -m pip install -q --disable-pip-version-check pyinstaller pillow tkinterdnd2
# The scripts run inside Uncharted.exe (--script): the standard modules they import come along.
hidden=()
for module in argparse base64 collections hashlib json re shutil struct tempfile zlib ctypes.wintypes cheats \
              uncharted_art uncharted_fonts uncharted_i18n uncharted_widgets bbport_steam PIL.ImageTk; do
    hidden+=(--hidden-import "$module")
done
out/pyenv/Scripts/python.exe -m PyInstaller --noconfirm --clean --log-level WARN --windowed \
    --name Uncharted --icon "$(cygpath -w "$PWD/launcher/uncharted.ico")" --distpath out/pyi-dist \
    --workpath out/pyi-work --specpath out/pyi-work --paths "$(cygpath -w "$PWD/scripts")" \
    --paths "$(cygpath -w "$PWD/launcher")" \
    --add-data "$(cygpath -w "$PWD/launcher/fonts");fonts" \
    --add-data "$(cygpath -w "$PWD/launcher/covers");covers" \
    "${hidden[@]}" \
    "$(cygpath -w "$PWD/launcher/uncharted_launcher.py")"

# The package is assembled in a fresh staging folder and zipped from there; dist/uncharted-windows
# (a playable copy that may hold saves and settings) is only refreshed afterwards.
dest=out/stage/uncharted-windows
rm -rf -- out/stage
mkdir -p "$dest/bin" "$dest/launcher" "$dest/scripts"
cp -r out/pyi-dist/Uncharted/. "$dest/"
# The Rust faces at the top, where the app and the command line look for each other. Release
# builds carry no debug information (the .pdb stays in the target folder).
for face in "${faces[@]}"; do cp "$faces_dir/$face.exe" "$dest/"; done
cp launcher/uncharted.ico launcher/uncharted.png "$dest/launcher/"
# The executables without debug information (out/ keeps the symbols for crash reports).
for exe in bb-probe.exe bb-gpu-capabilities.exe; do
    llvm-strip --strip-debug -o "$dest/bin/$exe" "out/$exe"
done
# Every DLL the executables load from the CLANG64 tree (SDL3, FFmpeg, Vulkan loader, ...).
ldd "$dest/bin/bb-probe.exe" "$dest/bin/bb-gpu-capabilities.exe" |
    awk '/\/clang64\/bin\// {print $3}' | sort -u > out/bundled-dlls.txt
while read -r dll; do cp -u "$dll" "$dest/bin/"; done < out/bundled-dlls.txt
# Only what run.py runs to prepare a game (Bloodborne's patch and mod tools stay out).
cp scripts/prepare.py scripts/link_modules.py scripts/link_libc.py scripts/content_profile.py scripts/trophies.py "$dest/scripts/"
cp run.py LICENSE "$dest/"
cp packaging/windows/README-Uncharted.txt "$dest/README.txt"
mkdir -p "$dest/licenses"
cp packaging/windows/licenses/*.txt "$dest/licenses/" 2>/dev/null || true
for license in out/pyenv/Lib/site-packages/tkinterdnd2-*.dist-info/licenses/LICENSE; do
    [[ -f $license ]] && cp "$license" "$dest/licenses/tkinterdnd2-LICENSE.txt"
done
# The crates statically linked into the Rust faces.
out/pyenv/Scripts/python.exe packaging/rust_licences.py "$dest/licenses/rust-crates.txt" --target "$rust_host"
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
# owning package or no licence files fails the package step with its name (#302).
mapfile -t dlls < out/bundled-dlls.txt
# Libraries linked statically into the binaries (gpu/CMakeLists.txt) ship no DLL; their archives
# name the packages whose licences must ship too.
for archive in libprotobuf libabsl_base libssl libminiupnpc libz; do dlls+=("/clang64/lib/$archive.a"); done
# MSYS2's own Python (CLANG64), so pacman is on its PATH; MSYS2_ARG_CONV_EXCL keeps the /clang64 paths
# as they are for pacman.
MSYS2_LICENSES=$(cygpath -m /clang64/share/licenses) MSYS2_ARG_CONV_EXCL='*'     python3 packaging/licence_check.py windows "$dest/licenses" "${dlls[@]}" ||
    { echo 'A bundled library has no licence text (see above).' >&2; exit 1; }
find "$dest" -name __pycache__ -prune -exec rm -r {} +
mkdir -p dist
rm -f dist/uncharted-windows.zip
# Not Compress-Archive, which writes backslashes that Linux tools keep in the file names (#246).
(cd out/stage && ../pyenv/Scripts/python.exe -c "
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
