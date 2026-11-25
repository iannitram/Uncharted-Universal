#!/usr/bin/env bash
# Builds the macOS app bundle, out/stage/Uncharted Universal.app, and dist/uncharted-macos.zip
# (roadmap 2.10). It is the Mac 1.0 app, arm64 only (Apple silicon), and the release job of
# build.yml publishes the zip. It is signed ad hoc: docs/apple/signing.md says why and what
# the upgrade path is.
#
# The bundle holds the three Rust faces side by side in Contents/MacOS, where they look for
# each other: uncharted-app (the window, CFBundleExecutable), uncharted-universal (the command
# line, `play` and the rest) and uncharted-run. Beside them are the pinned Save Relay helper
# (packaging/macos/bundle_relay.py, components.lock.json) and a link to its per-game manifests
# (kept in Resources), which `play` and the launcher find there. Its credentials live in the
# macOS Keychain, in Save Relay's own store (9.05), never in a file or a log. Run on a Mac with
# the Rust toolchain:
#
#     bash packaging/macos/bundle.sh
#
# BUNDLE_VERSION overrides the version (default: the Rust workspace version).
set -euo pipefail
cd -- "$(dirname -- "$0")/../.."
# shellcheck source=packaging/rust_faces.sh
source packaging/rust_faces.sh
version=${BUNDLE_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
[[ -n $version ]] || { echo 'No version: set BUNDLE_VERSION' >&2; exit 1; }

app="out/stage/Uncharted Universal.app"
rm -rf -- "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/licenses"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
for face in "${faces[@]}"; do cp "$faces_dir/$face$exe_suffix" "$app/Contents/MacOS/$face"; done
cp LICENSE "$app/Contents/Resources/licenses/"
python3 packaging/macos/bundle_relay.py "$app/Contents/MacOS" "$app/Contents/Resources" "${RELAY_CACHE:-out/relay-cache}"
# Everything in Contents/MacOS runs on Apple silicon only: refuse any other architecture.
for exe in "${faces[@]}" save-relay; do
    [[ $(lipo -archs "$app/Contents/MacOS/$exe") == arm64 ]] || { echo "$exe is not arm64" >&2; exit 1; }
done
python3 packaging/rust_licences.py "$app/Contents/Resources/licenses/rust-crates.txt"
# The icon, from the launcher's PNG, with Apple's own tools (only on a Mac).
if command -v sips >/dev/null && command -v iconutil >/dev/null; then
    iconset=out/stage/uncharted.iconset
    rm -rf -- "$iconset"
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
        sips -z "$size" "$size" crates/core/data/icons/uncharted.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
        sips -z $((size * 2)) $((size * 2)) crates/core/data/icons/uncharted.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns -o "$app/Contents/Resources/uncharted.icns" "$iconset"
    rm -rf -- "$iconset"
fi
# An ad-hoc signature over the bundle, after everything is copied in (Apple silicon runs nothing
# unsigned); no Developer ID for 1.0 (docs/apple/signing.md). Not --deep: the helpers already carry
# the linker's ad-hoc signature, and re-signing them would change the pinned Save Relay bytes that
# components.lock.json checksums. Then verify the bundle and every nested binary.
if command -v codesign >/dev/null; then
    codesign --force --sign - "$app"
    codesign --verify --deep --strict "$app"
fi
bash tools/check_no_game_data.sh "$app"
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
bash tools/check_no_game_data.sh dist/uncharted-macos.zip
du -sh "$app" dist/uncharted-macos.zip
