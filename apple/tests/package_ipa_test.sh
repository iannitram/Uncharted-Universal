#!/usr/bin/env bash
# Tests for apple/package_ipa.sh and apple/check_ios.sh (#409). They build a fake app (one arm64 iphoneos
# executable that defines the symbols the checks look for, and apple/ios/Info.plist.in filled in), then
# check the packaged .ipa and every refusal. All folders are made under $RUNNER_TEMP (made here when it
# is not set), so the packager's CI rule applies to the whole test.
# Run: bash apple/tests/package_ipa_test.sh
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd -P)
script=$root/apple/package_ipa.sh
check=$root/apple/check_ios.sh
template=$root/apple/ios/Info.plist.in

own_runner_temp=
cleanup() {
    if [ -n "$own_runner_temp" ]; then rm -rf "$own_runner_temp"; else rm -rf "${work:-}"; fi
}
work=
trap cleanup EXIT
trap 'exit 130' INT TERM
say() { echo "ok: $*"; }
die() { echo "FAIL: $*" >&2; exit 1; }

if [ -z "${RUNNER_TEMP:-}" ]; then
    own_runner_temp=$(mktemp -d "${TMPDIR:-/tmp}/uu-package-test.XXXXXXXX")
    RUNNER_TEMP=$own_runner_temp
    export RUNNER_TEMP
fi
work=$(mktemp -d "$RUNNER_TEMP/package-ipa-test.XXXXXXXX")

# make_app <dir> <game u1|u2|u3> <iphoneos|iphonesimulator>
make_app() {
    local dir=$1 game=$2 sdk=$3 app exe platform min_flag
    exe=Fake$game
    app=$dir/$exe.app
    mkdir -p "$app/licenses" "$app/save-relay-manifests"
    {
        for s in save_relay_init save_relay_pull save_relay_push save_relay_status save_relay_setup \
            save_relay_sign_in_start save_relay_sign_in_finish save_relay_free save_relay_http_respond \
            uncharted_runtime_version; do
            echo "int $s(void) { return 1; }"
        done
        echo 'int main(void) { return 0; }'
    } > "$work/fake-$game.c"
    if [ "$sdk" = iphoneos ]; then platform=iPhoneOS; min_flag=-miphoneos-version-min=15.0
    else platform=iPhoneSimulator; min_flag=-mios-simulator-version-min=15.0; fi
    xcrun --sdk "$sdk" clang -arch arm64 "$min_flag" -Wno-everything -o "$app/$exe" "$work/fake-$game.c"
    sed -e "s|@NAME@|Fake $game|g" -e "s|@APP@|$exe|g" -e "s|@BUNDLE_ID@|test.$game|g" -e "s|@VERSION@|9.9.9|g" -e "s|@BUILD@|9.9.9|g" \
        -e "s|@PLATFORM@|$platform|g" -e "s|@GAME@|$game|g" -e "s|@NUMBER@|1|g" -e "s|@RELAY_MANIFEST@|uncharted-1.json|g" \
        "$template" > "$app/Info.plist"
    echo '{}' > "$app/save-relay-manifests/uncharted-1.json"
    echo licence > "$app/licenses/save-relay-LICENSE.txt"
    echo '{}' > "$app/components.lock.json"
    printf 'APPL????' > "$app/PkgInfo"
}

# A refusal must exit nonzero, say the reason on stderr, and leave the out folder empty or absent.
refuse() { # <label> <reason substring> <out folder to check> <command...>
    local label=$1 reason=$2 out_dir=$3 err
    shift 3
    if err=$("$@" 2>&1 >/dev/null); then die "$label was not refused"; fi
    case "$err" in *"$reason"*) ;; *) die "$label was refused for another reason: $err" ;; esac
    if [ -e "$out_dir" ] && [ -n "$(ls -A "$out_dir")" ]; then die "$label wrote to its out folder"; fi
    say "refused: $label ($reason)"
}

mkdir "$work/build" "$work/build-sim"
make_app "$work/build" u1 iphoneos
make_app "$work/build" u3 iphoneos
make_app "$work/build-sim" u2 iphonesimulator

# 1. The device app passes the app checks; the Simulator app passes as a simulator build and fails as a device build.
bash "$check" app "$work/build/Fakeu1.app" device > /dev/null || die "device app fails the app check"
bash "$check" app "$work/build-sim/Fakeu2.app" simulator > /dev/null || die "simulator app fails as simulator"
if bash "$check" app "$work/build-sim/Fakeu2.app" device > /dev/null 2>&1; then die "simulator app passed as device"; fi
say "app checks: device, simulator, and a simulator app is not a device app"

# 2. Packaging writes one versioned .ipa, no leftovers, a Payload of one app, ad hoc and no entitlements.
out=$work/out
env CI=1 bash "$script" 1.2.3 "$work/build/Fakeu1.app" "$out" > "$work/pack.log"
ipa=$out/Uncharted-Universal-U1-1.2.3.ipa
[ -f "$ipa" ] || die "no .ipa written"
[ "$(ls -A "$out")" = "Uncharted-Universal-U1-1.2.3.ipa" ] || die "files left beside the .ipa: $(ls -A "$out")"
grep -q '^SHA-256: [0-9a-f]\{64\}$' "$work/pack.log" || die "no SHA-256 printed"
grep -q '^Size: [0-9]* bytes$' "$work/pack.log" || die "no size printed"
zipinfo -1 "$ipa" > "$work/list.txt"
grep -qx 'Payload/Fakeu1.app/Fakeu1' "$work/list.txt" || die "the executable is not in Payload/Fakeu1.app"
if grep -q '_CodeSignature\|embedded.mobileprovision' "$work/list.txt"; then die "signature leftovers in the .ipa"; fi
bash "$check" ipa "$ipa" > /dev/null || die "the packaged .ipa fails the ipa check"
say "packaged U1: versioned name, nothing left over, passes the ipa check"
env CI=1 bash "$script" 1.2.3 "$work/build/Fakeu3.app" "$out" > /dev/null
[ -f "$out/Uncharted-Universal-U3-1.2.3.ipa" ] || die "U3 not written beside U1"
say "packaged U3 beside U1"

# 3. The ipa check refuses game data, a second app, an extension and a file outside Payload/.
tamper() { # <label> <reason> <setup command run in the unpacked folder>
    local label=$1 reason=$2 dir=$work/tamper-$RANDOM
    mkdir "$dir"
    unzip -q "$ipa" -d "$dir"
    (cd "$dir" && eval "$3")
    (cd "$dir" && zip -qry "$dir.ipa" .)
    if err=$(bash "$check" ipa "$dir.ipa" 2>&1 >/dev/null); then die "ipa with $label passed"; fi
    case "$err" in *"$reason"*) ;; *) die "ipa with $label failed for another reason: $err" ;; esac
    say "ipa check refuses $label"
}
tamper "a data file" "outside the allowlist" 'echo x > Payload/Fakeu1.app/eboot.bin'
tamper "a game folder" "outside the allowlist" 'mkdir Payload/Fakeu1.app/game && echo x > Payload/Fakeu1.app/game/a'
tamper "a second app" "exactly one app" 'cp -R Payload/Fakeu1.app Payload/Other.app'
tamper "an extension" "outside the allowlist" 'mkdir -p Payload/Fakeu1.app/PlugIns/x.appex && echo x > Payload/Fakeu1.app/PlugIns/x.appex/x'
tamper "a file outside Payload" "outside Payload" 'echo x > readme.txt'

# 4. Refusals, each before anything is written.
refuse "an out folder inside the checkout" "outside the checkout" "$root/out/none" \
    bash "$script" 1.2.3 "$work/build/Fakeu1.app" "$root/out/none-$$"
mkdir "$work/repo" && git -C "$work/repo" init -q
refuse "an out folder inside a git work tree" "Git work tree" "$work/repo/ipas" \
    bash "$script" 1.2.3 "$work/build/Fakeu1.app" "$work/repo/ipas"
refuse "a Simulator app" "iphoneos" "$work/out-sim" bash "$script" 1.2.3 "$work/build-sim/Fakeu2.app" "$work/out-sim"
refuse "a bad version" "usage" "$work/out-v" bash "$script" "1/2" "$work/build/Fakeu1.app" "$work/out-v"
refuse "no out folder" "out folder is required" "$work/none" bash "$script" 1.2.3 "$work/build/Fakeu1.app" ""
refuse "a missing app" "must be a directory" "$work/out-m" bash "$script" 1.2.3 "$work/nothing.app" "$work/out-m"
refuse "CI without RUNNER_TEMP" "RUNNER_TEMP is not" "$work/out-c" \
    env -u RUNNER_TEMP CI=1 bash "$script" 1.2.3 "$work/build/Fakeu1.app" "$work/out-c"
outside=$(mktemp -d /private/tmp/uu-package-outside.XXXXXXXX)
refuse "CI with an out folder outside RUNNER_TEMP" "inside \$RUNNER_TEMP" "$outside" \
    env CI=1 bash "$script" 1.2.3 "$work/build/Fakeu1.app" "$outside/ipas"
rmdir "$outside"
mkdir -p "$work/out-p/.Uncharted-Universal-U1-1.2.3.partial"
if err=$(bash "$script" 1.2.3 "$work/build/Fakeu1.app" "$work/out-p" 2>&1 >/dev/null); then die "leftover stage was not refused"; fi
case "$err" in *"left from an earlier run"*) say "refused: a leftover .partial folder" ;; *) die "leftover refused wrongly: $err" ;; esac
mkdir -p "$work/out-d/Uncharted-Universal-U1-1.2.3.ipa"
if err=$(bash "$script" 1.2.3 "$work/build/Fakeu1.app" "$work/out-d" 2>&1 >/dev/null); then die "directory destination not refused"; fi
case "$err" in *"symlink or directory"*) say "refused: a directory in the .ipa's place" ;; *) die "wrong refusal: $err" ;; esac

# 5. The export script is local only.
mkdir "$work/exp"
touch "$work/exp/Uncharted-Universal-U1-1.0.ipa"
if err=$(env CI=1 UU_IOS_EXPORT="$work/dest" bash "$root/apple/export_ipas.sh" "$work/exp" 2>&1); then die "export ran in CI"; fi
[ ! -e "$work/dest" ] || die "export made its folder in CI"
UU_IOS_EXPORT="$work/dest" env -u CI -u GITHUB_ACTIONS bash "$root/apple/export_ipas.sh" "$work/exp" > /dev/null
[ -f "$work/dest/Uncharted-Universal-U1-1.0.ipa" ] || die "export did not copy"
say "export_ipas.sh refuses CI and copies locally (to a test folder, never the real Dropbox one)"
echo "all package_ipa tests passed"
