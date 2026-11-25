#!/usr/bin/env bash
# Build the per-game iOS apps (#409, ROADMAP 8.05): one thin UIKit app each for Uncharted 1, 2 and 3,
# with no Xcode project: cargo builds the runtime staticlib, swiftc compiles apple/ios/App and links it
# together with the pinned Save Relay library, and this script writes the bundle.
#
#   apple/build_ios.sh <u1|u2|u3|all> [iphoneos|iphonesimulator]      (default sdk: iphoneos)
#
# Environment: OUT (default out/apple-ios) holds <sdk>/<App>.app; VERSION (default: the Cargo workspace
# version); CACHE (default $OUT/cache) holds the Save Relay download; CARGO_TARGET_DIR as for cargo.
# The apps carry no game files and no entitlements. Prints the .app paths on stdout.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd -P)
here=$root/apple
say() { echo "$@" >&2; }
fail() { echo "error: $*" >&2; exit 1; }
usage() { echo "usage: apple/build_ios.sh <u1|u2|u3|all> [iphoneos|iphonesimulator]" >&2; exit 2; }

[ "$#" -ge 1 ] && [ "$#" -le 2 ] || usage
which=$1
sdk=${2:-iphoneos}
case "$which" in u1|u2|u3) games=$which ;; all) games="u1 u2 u3" ;; *) usage ;; esac
case "$sdk" in
    iphoneos) rust_target=aarch64-apple-ios; swift_target=arm64-apple-ios15.0; platform=iPhoneOS; fetch_args=() ;;
    iphonesimulator) rust_target=aarch64-apple-ios-sim; swift_target=arm64-apple-ios15.0-simulator; platform=iPhoneSimulator; fetch_args=(--sim) ;;
    *) usage ;;
esac

out=${OUT:-$root/out/apple-ios}
cache=${CACHE:-$out/cache}
version=${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)}
[ -n "$version" ] || fail "no version: set VERSION"
# CFBundleVersion must be integers separated by dots: 0.0.0-ci.7 becomes 0.0.0.7.
build=$(printf '%s' "$version" | sed -E 's/-[A-Za-z]+\./\./')
case "$build" in
    ''|*[!0-9.]*|.*|*.|*..*) fail "VERSION $version has no numeric build form (got $build)" ;;
esac
case "$version" in *[!a-zA-Z0-9._-]*) fail "version has odd characters: $version" ;; esac

sdk_path=$(xcrun --sdk "$sdk" --show-sdk-path)

say "building the runtime staticlib for $rust_target"
(cd "$root" && cargo build --release --locked -p uncharted_runtime --lib --target "$rust_target" >&2)
target_dir=${CARGO_TARGET_DIR:-$root/target}
runtime_lib=$target_dir/$rust_target/release/libuncharted_runtime.a
[ -f "$runtime_lib" ] || fail "no $runtime_lib"

relay=$(python3 -I "$root/packaging/ios/fetch_save_relay.py" "$cache" ${fetch_args[@]+"${fetch_args[@]}"})
relay_lib=$relay/lib/libsave_relay.a
[ -f "$relay_lib" ] || fail "no $relay_lib"

crate_licences=$out/rust-crates-$rust_target.txt
mkdir -p "$out"
python3 -I "$root/packaging/rust_licences.py" "$crate_licences" --target "$rust_target" >&2

for game in $games; do
    field() { python3 -I -c 'import json,sys; g=json.load(open(sys.argv[1])); print(g["games"][sys.argv[2]][sys.argv[3]] if sys.argv[3] != "prefix" else g["bundle_prefix"])' "$here/ios/games.json" "$game" "$1"; }
    app_name=$(field app)
    display=$(field name)
    number=$(field number)
    manifest=$(field relay_manifest)
    prefix=$(field prefix)
    bundle_id=$prefix.$(field bundle_suffix)

    app=$out/$sdk/$app_name.app
    rm -rf "$app"
    mkdir -p "$app/licenses" "$app/save-relay-manifests"

    say "compiling $display ($sdk)"
    # Full paths for both libraries: never -L/-l, which can pick up a cdylib that would crash on a device.
    # PIE and ARC are the swiftc defaults for iOS. -dead_strip keeps only what the app reaches.
    xcrun --sdk "$sdk" swiftc -sdk "$sdk_path" -target "$swift_target" -O -parse-as-library \
        -module-name "$app_name" \
        -I "$here/Runtime" -I "$relay/bindings/ios" \
        "$here"/ios/App/*.swift "$relay/bindings/ios/SaveRelay.swift" \
        -Xlinker -dead_strip \
        -Xlinker "$runtime_lib" -Xlinker "$relay_lib" \
        -framework UIKit -framework Foundation -framework Security \
        -lc++ -lresolv \
        -o "$app/$app_name"

    sed -e "s|@NAME@|$display|g" -e "s|@APP@|$app_name|g" -e "s|@BUNDLE_ID@|$bundle_id|g" \
        -e "s|@VERSION@|$version|g" -e "s|@BUILD@|$build|g" -e "s|@PLATFORM@|$platform|g" -e "s|@GAME@|$game|g" \
        -e "s|@NUMBER@|$number|g" -e "s|@RELAY_MANIFEST@|$manifest|g" \
        "$here/ios/Info.plist.in" > "$app/Info.plist"
    plutil -lint "$app/Info.plist" >&2
    printf 'APPL????' > "$app/PkgInfo"

    cp "$root/packaging/save-relay/$manifest" "$app/save-relay-manifests/$manifest"
    cp "$here/ios/components.lock.json" "$app/components.lock.json"
    cp "$root/packaging/save-relay/LICENSE" "$app/licenses/save-relay-LICENSE.txt"
    cp "$root/LICENSE" "$app/licenses/Uncharted-Universal-LICENSE.txt"
    cp "$crate_licences" "$app/licenses/rust-crates.txt"
    echo "$app"
done
