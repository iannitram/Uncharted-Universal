#!/usr/bin/env bash
# Builds the LGPL-2.1 FFmpeg DLLs the Windows zip ships next to bin/bb-probe.exe (owner ruling
# 2026-11-26, #625). MSYS2's own FFmpeg is configured with --enable-gpl and --enable-version3, so it
# cannot ship with this GPL-2.0-only project (#299). This is the same release as the CLANG64 package
# bb-probe.exe is compiled against (same DLL names, same ABI), from the official source tarball
# (pinned SHA256), with no GPL, version 3 or nonfree parts and no external libraries.
#   bash packaging/windows/build_ffmpeg.sh       (MSYS2 CLANG64: make and nasm, packaging/windows/README.md)
# Result: out/ffmpeg-lgpl/bin/*.dll, out/ffmpeg-lgpl/COPYING.LGPLv2.1 and out/ffmpeg-lgpl/SOURCE.txt;
# package.sh ships them and refuses any FFmpeg DLL whose configuration is GPL or nonfree.
set -euo pipefail
cd -- "$(dirname -- "$0")/../.."
source ./msys2-env.sh
version=9.0.2
sha256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
url=https://ffmpeg.org/releases/ffmpeg-$version.tar.xz
# Only what the avplayer and videodec code in bb-probe uses (gpu/CMakeLists.txt pkg_check_modules
# FFMPEG). --disable-autodetect keeps every external library (and its licence) out.
config=(--target-os=mingw32 --arch=x86_64 --cc=clang --cxx=clang++
    --enable-shared --disable-static --disable-programs --disable-doc --disable-debug
    --disable-avdevice --disable-avfilter --disable-network --disable-autodetect --enable-w32threads)
prefix=$PWD/out/ffmpeg-lgpl
stamp="$version ${config[*]}"
if [[ -f $prefix/SOURCE.txt && $(sed -n 's/^Build: //p' "$prefix/SOURCE.txt") == "$stamp" ]]; then
    echo "FFmpeg $version (LGPL-2.1) already built: $prefix"; exit 0
fi
for tool in make nasm clang; do
    command -v "$tool" >/dev/null || { echo "Need $tool (pacman -S make mingw-w64-clang-x86_64-nasm)." >&2; exit 1; }
done
work=out/ffmpeg-lgpl-build
rm -rf -- "$work" "$prefix"
mkdir -p "$work" out/component-cache
tarball=out/component-cache/ffmpeg-$version.tar.xz
if [[ ! -f $tarball ]] || ! echo "$sha256  $tarball" | sha256sum -c --status -; then
    curl -fL --retry 3 -o "$tarball.part" "$url"
    echo "$sha256  $tarball.part" | sha256sum -c -
    mv "$tarball.part" "$tarball"
fi
tar -xJf "$tarball" -C "$work"
src=$work/ffmpeg-$version
# configure's sed scripts break on a temp folder with a drive colon (TMP=B:/...): use a POSIX path.
mkdir -p "$work/tmp"
export TMPDIR=$PWD/$work/tmp
(cd "$src" && ./configure --prefix="$prefix" "${config[@]}" && make -j"${BB_JOBS:-4}" && make install)
# Fail here, not only in package.sh, when the build is not LGPL-2.1.
python3 -I packaging/licence_check.py ffmpeg "$prefix"/bin/*.dll
cp "$src/COPYING.LGPLv2.1" "$prefix/COPYING.LGPLv2.1"
cat > "$prefix/SOURCE.txt" <<EOF
FFmpeg $version, built for Uncharted Universal from the unmodified official release source.
Source: $url
SHA256: $sha256
Build: $stamp
EOF
rm -rf -- "$work"
echo "FFmpeg $version (LGPL-2.1) ready: $prefix/bin"
