#!/usr/bin/env bash
# Checks for the iOS apps and .ipa files (#409): what a build or a package must look like before it
# is handed to SideStore. CI runs it (.github/workflows/apple-ios.yml) and so does apple/package_ipa.sh.
#
#   apple/check_ios.sh app <X.app> [device|simulator]   (default: device)
#   apple/check_ios.sh ipa <X.ipa>
#
# App: one arm64 executable for the right platform, linked against system libraries only (this catches a
# Save Relay cdylib, which crashes on a device), the Save Relay C interface and the runtime defined in it,
# no app extension, and nothing in the bundle outside an allowlist, so no game file can ride along.
# Ipa: Payload/<App>.app only, the same allowlist, an ad-hoc signature and no entitlements.
# FOLLOW-UP: the allowlist below stands in for tools/check_no_game_data.sh (the game-data guard owned by
# the Mac release slice); call that script from here once it is on main.
set -euo pipefail
fail() { echo "error: $*" >&2; exit 1; }
ok() { echo "ok: $*"; }
usage() { echo "usage: apple/check_ios.sh app <X.app> [device|simulator] | ipa <X.ipa>" >&2; exit 2; }

# Every file in the bundle must be one of these; anything else (a data file, a second binary, an
# archive) fails. The executable is named by Info.plist.
check_contents() { # <app dir> <executable name>
    local app=$1 exe=$2 rel bad=
    while IFS= read -r rel; do
        case "$rel" in
            "$exe"|Info.plist|PkgInfo|components.lock.json) ;;
            licenses/*.txt|save-relay-manifests/*.json) ;;
            *) bad="$bad $rel" ;;
        esac
    done < <(cd "$app" && find . -type f -print | sed 's|^\./||')
    [ -z "$bad" ] || fail "files outside the allowlist in $(basename "$app"):$bad"
    [ -z "$(find "$app" -type l -print -quit)" ] || fail "symlink in $(basename "$app")"
    [ -z "$(find "$app" -type f -size +64M -print -quit)" ] || fail "file over 64 MiB in $(basename "$app")"
    [ ! -e "$app/PlugIns" ] && [ ! -e "$app/Frameworks" ] || fail "app extension or framework present"
    [ ! -e "$app/_CodeSignature" ] && [ ! -e "$app/embedded.mobileprovision" ] || fail "signature leftovers present"
    ok "bundle holds only allowlisted files"
}

check_app() { # <app dir> <device|simulator>
    local app=$1 kind=$2 exe archs platforms want symbol deps bad defined
    [ -d "$app" ] && [ ! -L "$app" ] || fail "$app is not a directory"
    plutil -lint "$app/Info.plist" > /dev/null || fail "Info.plist does not lint"
    exe=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Info.plist")
    [ -f "$app/$exe" ] && [ ! -L "$app/$exe" ] || fail "executable $exe missing"
    archs=$(lipo -archs "$app/$exe")
    [ "$archs" = arm64 ] || fail "executable archs are '$archs', not arm64"
    ok "lipo: arm64"
    platforms=$(vtool -show-build "$app/$exe" | awk '$1 == "platform" {print $2}')
    want=IOS; [ "$kind" = simulator ] && want=IOSSIMULATOR
    [ "$platforms" = "$want" ] || fail "platform is '$platforms', not $want"
    ok "vtool: platform $want"
    # Only system libraries: any other line (@rpath, a build path, a Homebrew path) is a failure.
    deps=$(otool -L "$app/$exe" | tail -n +2 | awk '{print $1}')
    bad=$(grep -v -e '^/System/' -e '^/usr/lib/' <<< "$deps" || true)
    [ -z "$bad" ] || fail "non-system libraries: $bad"
    ok "otool -L: system libraries and frameworks only"
    defined=$(nm -gU "$app/$exe" | awk '{print $NF}')
    for symbol in save_relay_init save_relay_pull save_relay_push save_relay_status save_relay_setup \
        save_relay_sign_in_start save_relay_sign_in_finish save_relay_free save_relay_http_respond \
        uncharted_runtime_version; do
        grep -qx "_$symbol" <<< "$defined" || fail "$symbol is not defined in $exe"
    done
    ok "nm: save_relay_* and uncharted_runtime_version defined"
    check_contents "$app" "$exe"
}

check_ipa() { # <ipa>
    local ipa=$1 work list apps name app exe signature entitlements
    [ -f "$ipa" ] && [ ! -L "$ipa" ] || fail "$ipa is not a file"
    unzip -tq "$ipa" > /dev/null || fail "archive fails unzip -t"
    work=$(mktemp -d "${TMPDIR:-/tmp}/check-ipa.XXXXXXXX")
    trap 'rm -rf "$work"' RETURN
    list=$work/list
    zipinfo -1 "$ipa" > "$list"
    bad=$(grep -v '^Payload/' "$list" || true)
    [ -z "$bad" ] || fail "entries outside Payload/: $bad"
    apps=$(awk -F/ '{print $2}' "$list" | sort -u | grep -v '^$' || true)
    [ "$(printf '%s\n' "$apps" | wc -l | tr -d ' ')" = 1 ] || fail "Payload/ must hold exactly one app, has: $apps"
    case "$apps" in *.app) ;; *) fail "Payload/$apps is not an .app" ;; esac
    ok "Payload/$apps"
    unzip -q "$ipa" -d "$work/x"
    app=$work/x/Payload/$apps
    check_app "$app" device
    exe=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Info.plist")
    signature=$(codesign -dvv "$app/$exe" 2>&1 | sed -n 's/^Signature=//p')
    [ "$signature" = adhoc ] || fail "signature is '$signature', not adhoc"
    entitlements=$(codesign -d --entitlements - "$app/$exe" 2>/dev/null | tr -d '[:space:]')
    [ -z "$entitlements" ] || fail "the executable carries entitlements"
    ok "Signature=adhoc, no entitlements"
}

[ "$#" -ge 2 ] || usage
case "$1" in
    app) check_app "$2" "${3:-device}" ;;
    ipa) [ "$#" -eq 2 ] || usage; check_ipa "$2" ;;
    *) usage ;;
esac
