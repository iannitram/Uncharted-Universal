#!/usr/bin/env bash
# Make and boot a Simulator device named UU-ios-shell (the only device these tests may touch) and print
# its UDID. The caller deletes it: xcrun simctl shutdown <udid>; xcrun simctl delete <udid>.
set -euo pipefail
pair=$(xcrun simctl list runtimes -j | python3 -I -c '
import json, sys
for runtime in reversed(json.load(sys.stdin)["runtimes"]):
    if runtime["platform"] != "iOS" or not runtime["isAvailable"]:
        continue
    phones = [d["identifier"] for d in runtime.get("supportedDeviceTypes", []) if d.get("productFamily") == "iPhone"]
    if phones:
        print(runtime["identifier"], phones[0])
        break
')
[ -n "$pair" ] || { echo "no iOS Simulator runtime with an iPhone device type" >&2; exit 1; }
udid=$(xcrun simctl create UU-ios-shell "${pair#* }" "${pair%% *}")
xcrun simctl boot "$udid" >&2
xcrun simctl bootstatus "$udid" -b > /dev/null
echo "$udid"
