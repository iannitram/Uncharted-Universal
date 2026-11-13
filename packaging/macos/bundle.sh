#!/usr/bin/env bash
# Builds the macOS app bundle, out/stage/Uncharted Universal.app, and dist/uncharted-macos.zip
# (roadmap 2.10). CI builds it on every package build so the Mac face keeps building; it is
# not released. The Mac app that ships is roadmap 1.x (macOS has no game runtime yet: the
# loader, `uncharted-run`, is a probe until 3.16), and signing and notarizing come with it.
#
# The bundle holds the three Rust faces side by side in Contents/MacOS, where they look for
# each other: uncharted-app (the window, CFBundleExecutable), uncharted-universal (the command
# line, `play` and the rest) and uncharted-run. Run on a Mac with the Rust toolchain:
#
#     bash packaging/macos/bundle.sh
#
# BUNDLE_VERSION overrides the version (default: the launcher's VERSION).
set -euo pipefail
cd -- "$(dirname -- "$0")/../.."
# shellcheck source=packaging/rust_faces.sh
source packaging/rust_faces.sh
version=${BUNDLE_VERSION:-$(sed -n "s/^VERSION = '\(.*\)'/\1/p" launcher/uncharted_launcher.py | head -1)}
[[ -n $version ]] || { echo 'No version: set BUNDLE_VERSION' >&2; exit 1; }

app="out/stage/Uncharted Universal.app"
rm -rf -- "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/licenses"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
for face in "${faces[@]}"; do cp "$faces_dir/$face$exe_suffix" "$app/Contents/MacOS/$face"; done
cp LICENSE "$app/Contents/Resources/licenses/"
python3 packaging/rust_licences.py "$app/Contents/Resources/licenses/rust-crates.txt"
# The icon, from the launcher's PNG, with Apple's own tools (only on a Mac).
if command -v sips >/dev/null && command -v iconutil >/dev/null; then
    iconset=out/stage/uncharted.iconset
    rm -rf -- "$iconset"
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
        sips -z "$size" "$size" launcher/uncharted.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
        sips -z $((size * 2)) $((size * 2)) launcher/uncharted.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns -o "$app/Contents/Resources/uncharted.icns" "$iconset"
    rm -rf -- "$iconset"
fi
# An ad-hoc signature over the whole bundle (Apple silicon runs nothing unsigned); a real one
# comes with the shipped Mac app (1.x).
if command -v codesign >/dev/null; then codesign --force --deep --sign - "$app"; fi
mkdir -p dist
rm -f dist/uncharted-macos.zip
if command -v ditto >/dev/null; then
    ditto -c -k --keepParent "$app" dist/uncharted-macos.zip
else
    # Off a Mac (trying the script out): a plain zip, without the bundle's extended attributes.
    python3 -c "
import pathlib, zipfile
with zipfile.ZipFile('dist/uncharted-macos.zip', 'w', zipfile.ZIP_DEFLATED) as z:
    for p in sorted(pathlib.Path('out/stage').glob('Uncharted Universal.app/**/*')):
        z.write(p, p.relative_to('out/stage'))"
fi
du -sh "$app" dist/uncharted-macos.zip
