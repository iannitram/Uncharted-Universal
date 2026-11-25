#!/usr/bin/env bash
# Launch smoke for one app in the iOS Simulator (#409): build it for the Simulator, install it on a device
# named UU-ios-shell, launch it, check it is still running after a few seconds and that it wrote its first
# log line (game, version and the runtime ABI it linked). Local and CI (non-blocking); never a real device.
#
#   apple/tests/app_smoke_sim.sh [u1|u2|u3] [scratch folder]
#
# The screenshot lands in the scratch folder as launch.png. It makes the device and deletes it again.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd -P)
game=${1:-u1}
scratch=${2:-$(mktemp -d "${TMPDIR:-/tmp}/uu-app-smoke.XXXXXXXX")}
mkdir -p "$scratch"
udid=
cleanup() {
    if [ -n "$udid" ]; then
        xcrun simctl shutdown "$udid" 2>/dev/null || true
        xcrun simctl delete "$udid" 2>/dev/null || true
    fi
}
trap cleanup EXIT

app=$(OUT=$scratch/out bash "$root/apple/build_ios.sh" "$game" iphonesimulator | tail -n 1)
bash "$root/apple/check_ios.sh" app "$app" simulator
bundle_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")

udid=$(bash "$root/apple/tests/sim_create.sh")
xcrun simctl install "$udid" "$app"
launched=$(xcrun simctl launch "$udid" "$bundle_id")
pid=${launched##*: }
sleep 4
kill -0 "$pid" 2>/dev/null || { echo "FAIL: the app is not running after 4 seconds (crashed?)" >&2; exit 1; }
echo "ok: $bundle_id still running (pid $pid)"
xcrun simctl io "$udid" screenshot "$scratch/launch.png" > /dev/null 2>&1 || true
container=$(xcrun simctl get_app_container "$udid" "$bundle_id" data)
grep -q "launch $game " "$container/Documents/uncharted.log" || { echo "FAIL: no launch line in Documents/uncharted.log" >&2; exit 1; }
echo "ok: Documents/uncharted.log: $(tail -n 1 "$container/Documents/uncharted.log")"
