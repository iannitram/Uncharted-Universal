#!/usr/bin/env bash
# Linux fork: builds dist/uncharted-linux.tar.gz and the AppImage after `bash build.sh`. The
# AppImage opens uncharted-app, the Rust launcher (roadmap 2.10), with uncharted-universal (the
# command line Steam shortcuts run: `play u1`) and uncharted-run beside it; the three clean
# Steam's LD_PRELOAD out of their own environment (crates/core host_env). Uncharted is the
# classic launcher (frozen with PyInstaller, so no Python is needed; `--classic`, and the
# `--play` shortcuts it wrote, until roadmap 2.11 retires it), bin/ holds bb-probe, its GPU library
# and the libraries a Steam Deck or another distribution may lack (bin/lib), the preparation
# scripts and run.py. Graphics-driver, glibc and libstdc++ libraries come from the system.
# LINUX_BUILD (e.g. 0.3.1-linux.4) is written into the package for the launcher's update check.
set -euo pipefail
cd -- "$(dirname -- "$0")/../.."
cargo run --release -p uncharted_cli -- generate-art "$PWD/launcher/covers"
# shellcheck source=packaging/rust_faces.sh
source packaging/rust_faces.sh
[[ -f out/bb-probe && -f out/gpu/libbbgpu.so && -f out/bb-gpu-capabilities ]] || { echo 'Build first: bash build.sh' >&2; exit 1; }

python=${PYTHON:-python3}
if [[ ! -x out/pyenv/bin/python ]]; then
    "$python" -m venv out/pyenv
fi
out/pyenv/bin/python -m pip install -q --disable-pip-version-check pyinstaller pillow
[[ -f launcher/uncharted.png ]] || out/pyenv/bin/python - <<'EOF'
from PIL import Image
icon = Image.open('launcher/uncharted.ico')
icon.size = max(icon.ico.sizes())
icon.convert('RGBA').save('launcher/uncharted.png')
EOF
hidden=()
for module in argparse base64 collections hashlib json re shlex shutil struct tempfile zlib cheats save_slots xml.etree.ElementTree \
              uncharted_art uncharted_fonts uncharted_i18n uncharted_widgets uncharted_linux uncharted_logs bbport_steam PIL.ImageTk PIL._tkinter_finder; do
    hidden+=(--hidden-import "$module")
done
out/pyenv/bin/python -m PyInstaller --noconfirm --clean --log-level WARN --windowed \
    --name Uncharted --distpath out/pyi-dist --workpath out/pyi-work --specpath out/pyi-work \
    --paths "$PWD/scripts" --paths "$PWD/launcher" \
    --add-data "$PWD/launcher/fonts:fonts" --add-data "$PWD/launcher/covers:covers" \
    --add-data "$PWD/launcher/uncharted.png:." --add-data "$PWD/launcher/icons:icons" \
    "${hidden[@]}" launcher/uncharted_launcher.py

dest=out/stage/uncharted-linux
rm -rf -- out/stage
mkdir -p "$dest/bin/gpu" "$dest/bin/lib" "$dest/launcher" "$dest/scripts" "$dest/licenses"
cp -r out/pyi-dist/Uncharted/. "$dest/"
for face in "${faces[@]}"; do strip --strip-debug -o "$dest/$face" "$faces_dir/$face"; done
strip --strip-debug -o "$dest/bin/bb-probe" out/bb-probe
strip --strip-debug -o "$dest/bin/bb-gpu-capabilities" out/bb-gpu-capabilities
strip --strip-debug -o "$dest/bin/gpu/libbbgpu.so" out/gpu/libbbgpu.so
# Linux fork: NVIDIA DLSS (DLAA) when packaging/linux/build_dlss.sh has put the bridge and the
# SDK's DLSS library in out/dlss: next to bb-probe, where vk_dlss.cpp looks for them.
if [[ -f out/dlss/libbbport_dlss.so ]] && compgen -G 'out/dlss/libnvidia-ngx-dlss.so.*' >/dev/null; then
    cp out/dlss/libbbport_dlss.so out/dlss/libnvidia-ngx-dlss*.so.* "$dest/bin/"
    if [[ -f out/dlss/LICENSE-DLSS-SDK.txt ]]; then cp out/dlss/LICENSE-DLSS-SDK.txt "$dest/licenses/"; fi
    echo 'DLSS: bridge and NVIDIA DLSS libraries packaged'
fi
# FSR 4 assets (tools/fetch_fsr4_assets.sh) when present: run.py starts the game in the package
# folder, where vk_fsr4.cpp looks for fsr4_shaders/ by default.
if [[ -d fsr4_shaders ]]; then cp -r fsr4_shaders "$dest/"; echo 'FSR 4: assets packaged'; fi
# shellcheck source=packaging/linux/system_libraries.sh
source packaging/linux/system_libraries.sh
# The Rust faces load only system libraries (Slint opens X11, Wayland, xkbcommon and GL itself;
# fontconfig and udev are linked): foreign_needed checks the libraries each names itself.
for face in "${faces[@]}"; do
    foreign=$(readelf -d "$dest/$face" | foreign_needed)
    [[ -z $foreign ]] || { echo "$face links libraries a player's system may lack: $foreign" >&2; exit 1; }
done
ldd out/bb-probe out/gpu/libbbgpu.so out/bb-gpu-capabilities | awk '/=> \// {print $1, $3}' | sort -u |
    while read -r name path; do
        # Arch and SteamOS list the loader as /lib64/ld-linux-x86-64.so.2 => ...: match by file name.
        name=$(basename -- "$name")
        # The GPU library is bin/gpu's stripped copy (bb-probe's RUNPATH): an unstripped second
        # copy here, first on LD_LIBRARY_PATH, was the one loaded and added 60 MB to the AppImage.
        [[ $name == libbbgpu.so ]] && continue
        [[ $name =~ $system ]] || cp -L "$path" "$dest/bin/lib/$name"
    done
cp scripts/prepare.py scripts/link_modules.py scripts/link_libc.py scripts/content_profile.py scripts/render_720p.py scripts/render_res.py scripts/trophies.py scripts/cheats.py scripts/patches.py scripts/pkg.py scripts/save_slots.py "$dest/scripts/"
cp run.py LICENSE "$dest/"
cp launcher/uncharted.png "$dest/launcher/"
cp README.md "$dest/README.md"
echo "${LINUX_BUILD:-}" > "$dest/LINUX_BUILD"
# A custom build that must never update itself to an official release (NO_UPDATE_CHECK=1).
if [[ ${NO_UPDATE_CHECK:-0} == 1 ]]; then : > "$dest/NO_UPDATE_CHECK"; fi
for license in gpu/third_party/httplib/LICENSE:cpp-httplib launcher/fonts/OFL.txt:Cinzel; do
    if [[ -f ${license%%:*} ]]; then cp "${license%%:*}" "$dest/licenses/${license##*:}-LICENSE.txt"; fi
done
# The bundled libraries' own copyright files (Debian keeps them per package). A library whose
# package has no copyright file fails the package step with its name (#302).
bundled=()
for library in "$dest"/bin/lib/*; do
    [[ -e $library ]] && bundled+=("/usr/lib/x86_64-linux-gnu/$(basename "$library")")
done
"$python" packaging/licence_check.py linux "$dest/licenses" "${bundled[@]}"
# The crates statically linked into the Rust faces.
"$python" packaging/rust_licences.py "$dest/licenses/rust-crates.txt"
find "$dest" -name __pycache__ -prune -exec rm -r {} +
mkdir -p dist
tar -C out/stage -czf dist/uncharted-linux.tar.gz uncharted-linux

# The AppImage: the same files in one executable. Saves, settings and generated files go to
# ~/.local/share/uncharted-linux, since the AppImage itself is read-only. AppRun opens
# uncharted-app, hands the command line's commands to uncharted-universal, and the classic
# launcher's flags to Uncharted.
appdir=out/stage/Uncharted.AppDir
cp -a "$dest" "$appdir"
cp packaging/linux/AppRun "$appdir/AppRun"
chmod +x "$appdir/AppRun"
cp packaging/linux/uncharted-linux.desktop "$appdir/"
cp launcher/uncharted.png "$appdir/uncharted-linux.png"
cp launcher/uncharted.png "$appdir/.DirIcon"
tool=${APPIMAGETOOL:-out/appimagetool-1.9.1}
if [[ -z ${APPIMAGETOOL:-} && ! -x $tool ]]; then
    curl -fsSL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
    chmod +x "$tool"
fi
[[ -x $tool ]] || { echo "AppImage tool is not executable: $tool" >&2; exit 1; }
# Extract-and-run: works without FUSE (containers, CI).
# APPIMAGE_NAME: another file name (a custom build next to the official one).
appimage=dist/${APPIMAGE_NAME:-Uncharted-x86_64.AppImage}
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$appdir" "$appimage" >/dev/null
ls -lh dist/uncharted-linux.tar.gz "$appimage"
