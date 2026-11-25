#!/usr/bin/env bash
# Package one iphoneos app as an unsigned-for-distribution .ipa for SideStore (#409). The app is signed
# ad hoc with no entitlements; SideStore replaces that with the owner's free Apple ID signature when it
# installs. Adapted from the ACNH-Universal packager (MIT, same owner).
#
#   apple/package_ipa.sh <version> <X.app from apple/build_ios.sh> <out folder>
#
# Writes <out>/Uncharted-Universal-<U1|U2|U3>-<version>.ipa and prints its path, size and SHA-256.
# The out folder is required, must be outside every git work tree, and under CI must be inside
# $RUNNER_TEMP. The .ipa is staged in <out>/.<name>.partial/ and moved into place when complete, so a
# refusal or a crash leaves no half-written .ipa.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd -P)
here=$root/apple

usage() { echo "usage: apple/package_ipa.sh <version> <X.app> <out folder>" >&2; exit 2; }
fail() { echo "error: $*" >&2; exit 1; }
realpath_py() { python3 -I -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$1"; }

[ "$#" -eq 3 ] || usage
version=$1
build_app=$2
[ -n "$3" ] || fail "the out folder is required (it has no default)"
case "$version" in ''|*[!a-zA-Z0-9._-]*|.|..) usage ;; esac
out=$(realpath_py "$3")
case "$out/" in "$root/"*) fail "out folder must be outside the checkout: $out" ;; esac

# Every refusal comes before anything is created, so a refusal leaves the out folder untouched.
[ ! -e "$out" ] || [ -d "$out" ] || fail "out folder is not a folder: $out"
existing=$out
while [ ! -d "$existing" ]; do existing=$(dirname "$existing"); done
errfile=$(mktemp "${TMPDIR:-/tmp}/package-ipa-git.XXXXXXXX")
if git_location=$(git -C "$existing" rev-parse --is-inside-work-tree --is-inside-git-dir 2>"$errfile"); then
    :
else
    git_err=$(cat "$errfile")
    case "$git_err" in
        *"not a git repository"*) git_location= ;;
        *) rm -f "$errfile"; fail "cannot tell whether the out folder is inside a Git work tree: $git_err" ;;
    esac
fi
rm -f "$errfile"
if printf '%s\n' "$git_location" | grep -qx true; then
    fail "out folder is inside a Git work tree (or its metadata): choose a folder outside every checkout"
fi
if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
    [ -n "${RUNNER_TEMP:-}" ] || fail "CI is set but RUNNER_TEMP is not, so the out folder cannot be checked"
    case "$out/" in
        "$(realpath_py "$RUNNER_TEMP")/"*) ;;
        *) fail "under CI the out folder must be inside \$RUNNER_TEMP ($RUNNER_TEMP)" ;;
    esac
fi

[ ! -L "$build_app" ] && [ -d "$build_app" ] || fail "the app must be a directory, not a symlink: $build_app"
case "$build_app" in *.app|*.app/) ;; *) fail "not an .app: $build_app" ;; esac
app_name=$(basename "${build_app%/}")
[ ! -L "$build_app/Info.plist" ] && [ -f "$build_app/Info.plist" ] || fail "Info.plist not found"
exe=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$build_app/Info.plist")
case "$exe" in ''|.|..|*/*) fail "invalid CFBundleExecutable" ;; esac
[ ! -L "$build_app/$exe" ] && [ -f "$build_app/$exe" ] || fail "the executable must be a regular file, not a symlink"
# Every slice must be for iOS devices: a Simulator or macOS build cannot be installed on a device.
[ "$(lipo -archs "$build_app/$exe")" = arm64 ] || fail "executable must be arm64"
[ "$(vtool -show-build "$build_app/$exe" | awk '$1 == "platform" {print $2}')" = IOS ] \
    || fail "executable must target iphoneos (not the Simulator)"
game=$(/usr/libexec/PlistBuddy -c 'Print :UUGame' "$build_app/Info.plist" 2>/dev/null || true)
case "$game" in u1|u2|u3) ;; *) fail "Info.plist has no UUGame of u1, u2 or u3" ;; esac
tag=$(printf '%s' "$game" | tr '[:lower:]' '[:upper:]')

name=Uncharted-Universal-$tag-$version
ipa=$out/$name.ipa
if [ -L "$ipa" ] || [ -d "$ipa" ]; then fail "the .ipa destination must not be a symlink or directory"; fi
stage_dir=$out/.$name.partial
if [ -e "$stage_dir" ] || [ -L "$stage_dir" ]; then
    fail "$stage_dir is left from an earlier run: remove it"
fi

stage=
made_out=
[ -d "$out" ] || made_out=1
cleanup() {
    if [ -n "$stage" ] && [ -e "$stage" ]; then rm -rf "$stage"; fi
    if [ -n "$made_out" ]; then rmdir "$out" 2>/dev/null || true; fi
    return 0
}
trap cleanup EXIT
trap 'exit 130' INT TERM
mkdir -p "$out"
stage=$stage_dir
mkdir "$stage"
mkdir "$stage/Payload"

copy=$stage/Payload/$app_name
cp -R "$build_app" "$copy"
xattr -cr "$copy"
rm -rf "$copy/_CodeSignature" "$copy/embedded.mobileprovision"
# Ad hoc, no entitlements and no provisioning profile; SideStore signs it again.
codesign --force -s - "$copy/$exe"
# Signing the executable can leave a bundle seal; SideStore makes its own.
rm -rf "$copy/_CodeSignature"

partial=$name.ipa.partial
(cd "$stage" && zip -qry "$partial" Payload)
# Check the archive itself before it gets its final name (contents, platform, signature, no game data).
bash "$here/check_ios.sh" ipa "$stage/$partial" >&2
mv -f "$stage/$partial" "$ipa"
size=$(stat -f %z "$ipa")
sha=$(shasum -a 256 "$ipa" | awk '{print $1}')
printf 'IPA: %s\nSize: %s bytes\nSHA-256: %s\n' "$ipa" "$size" "$sha"
