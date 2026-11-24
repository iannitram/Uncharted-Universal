#!/bin/sh
# Synthetic Metal probe (#408, feasibility plan step 4). Authored code only; no game data.
#   run.sh macos [report.json]   build for macOS arm64 and run on this Mac
#   run.sh sim   [report.json]   build for the iOS Simulator and run it in a simulator device
#                                named UU-metal-probe (created here, deleted afterwards)
#   run.sh build-sim             compile only for the iOS Simulator (no device needed)
# Exit 0 on pass or on a clearly reported skip (no Metal device), 1 on a failed check.
# The JSON record goes to the report path (default metal-probe-report.json) and stderr carries
# the per-check log. Shaders compile at run time, so no Metal Toolchain component is needed.
set -eu
mode=${1:-macos}; shift 2>/dev/null || true
out=${1:-metal-probe-report.json}
d=$(cd "$(dirname "$0")" && pwd)
out=$(cd "$(dirname "$out")" && pwd)/$(basename "$out")
w=${UU_PROBE_BUILD:-${TMPDIR:-/tmp}/uu-metal-probe-build}
mkdir -p "$w"
clang_v=$(xcrun clang --version | head -1)
xcode_v=$(xcodebuild -version 2>/dev/null | tr '\n' ' ' | sed 's/ *$//' || true)
[ -n "$xcode_v" ] || xcode_v=unknown

build() { # sdk target name
  xcrun --sdk "$1" clang -fobjc-arc -O1 -Wall -Wextra -Werror -arch arm64 -target "$2" \
    -DPROBE_TARGET="\"$2\"" -DPROBE_CLANG="\"$clang_v\"" -DPROBE_XCODE="\"$xcode_v\"" \
    "$d/probe.m" -framework Foundation -framework Metal -o "$w/$3"
}

case "$mode" in
  macos)
    build macosx arm64-apple-macos13.0 probe_macos
    rc=0; "$w/probe_macos" > "$out" || rc=$?
    ;;
  build-sim)
    build iphonesimulator arm64-apple-ios16.4-simulator probe_sim
    echo "built $w/probe_sim"; exit 0
    ;;
  sim)
    build iphonesimulator arm64-apple-ios16.4-simulator probe_sim
    # First available iOS runtime and the newest iPhone device type that runtime supports.
    pair=$(xcrun simctl list -j runtimes available | python3 -I -c '
import json, sys
for r in json.load(sys.stdin)["runtimes"]:
    if ".iOS" not in r["identifier"]: continue
    ph = [t["identifier"] for t in r["supportedDeviceTypes"] if ".iPhone-" in t["identifier"] and "Duo" not in t["identifier"]]
    if ph: print(r["identifier"], ph[-1]); break
')
    [ -n "$pair" ] || { echo "no usable iOS simulator runtime" >&2; exit 1; }
    rt=${pair% *}; ty=${pair#* }
    dev=$(xcrun simctl create UU-metal-probe "$ty" "$rt")
    trap 'xcrun simctl shutdown "$dev" >/dev/null 2>&1 || true; xcrun simctl delete "$dev" >/dev/null 2>&1 || true' EXIT
    xcrun simctl boot "$dev"
    xcrun simctl bootstatus "$dev" -b >/dev/null 2>&1 || true
    rc=0; xcrun simctl spawn "$dev" "$w/probe_sim" > "$out" || rc=$?
    ;;
  *) echo "usage: run.sh macos|sim|build-sim [report.json]" >&2; exit 2 ;;
esac
cat "$out"
exit "$rc"
