#!/usr/bin/env bash
# The RelayHost test (#251): builds apple/ios/App/RelayHost.swift with Save Relay's SaveRelay.swift and a
# Simulator build of the pinned Save Relay library, then runs it inside the iOS Simulator against a local
# fake server (Keychain, URLSession, and the real C interface). No app, no Xcode project, no XCTest.
#
#   apple/tests/relay_host_test.sh [scratch folder]     (default: a new folder under $TMPDIR)
#
# It makes a Simulator device named UU-ios-shell and deletes it again, and never touches another device.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd -P)
here=$root/apple
scratch=${1:-$(mktemp -d "${TMPDIR:-/tmp}/uu-relay-host.XXXXXXXX")}
mkdir -p "$scratch/RelayHostTest.app"
udid=
server_pid=
cleanup() {
    [ -z "$server_pid" ] || { kill "$server_pid" 2>/dev/null; wait "$server_pid" 2>/dev/null; } || true
    if [ -n "$udid" ]; then
        xcrun simctl shutdown "$udid" 2>/dev/null || true
        xcrun simctl delete "$udid" 2>/dev/null || true
    fi
}
trap cleanup EXIT

relay=$(python3 -I "$root/packaging/ios/fetch_save_relay.py" "${CACHE:-$scratch/cache}" --sim)
sdk_path=$(xcrun --sdk iphonesimulator --show-sdk-path)
# The Keychain also needs an application identifier. On a device SideStore's signature supplies it; the
# Simulator reads it from an entitlements section in the executable, as Xcode does. Test app only.
cat > "$scratch/test.entitlements" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>application-identifier</key><string>TESTTEAM00.test.uncharted.relayhosttest</string>
<key>keychain-access-groups</key><array><string>TESTTEAM00.test.uncharted.relayhosttest</string></array>
</dict></plist>
PLIST
xcrun --sdk iphonesimulator swiftc -sdk "$sdk_path" -target arm64-apple-ios15.0-simulator -O \
    -module-name RelayHostTest -I "$relay/bindings/ios" \
    "$here/ios/App/RelayHost.swift" "$relay/bindings/ios/SaveRelay.swift" "$here/tests/relay_host/main.swift" \
    -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __entitlements -Xlinker "$scratch/test.entitlements" \
    -Xlinker "$relay/lib/libsave_relay.a" -framework Security -lc++ -lresolv \
    -o "$scratch/RelayHostTest.app/RelayHostTest"
# The Keychain refuses a bare executable (-34018, no application identifier), so the test runs as an app.
# The test plist (not the product's) lets the app reach the fake server over plain HTTP on this Mac.
cat > "$scratch/RelayHostTest.app/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>RelayHostTest</string>
<key>CFBundleIdentifier</key><string>test.uncharted.relayhosttest</string>
<key>CFBundleName</key><string>RelayHostTest</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>1</string>
<key>CFBundleSupportedPlatforms</key><array><string>iPhoneSimulator</string></array>
<key>MinimumOSVersion</key><string>15.0</string>
<key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
</dict></plist>
PLIST

python3 -I "$root/tests/apple/fake_server.py" "$scratch/port" &
server_pid=$!
for _ in $(seq 1 50); do [ -s "$scratch/port" ] && break; sleep 0.1; done
[ -s "$scratch/port" ] || { echo "the fake server did not start" >&2; exit 1; }

udid=$(bash "$here/tests/sim_create.sh")
xcrun simctl install "$udid" "$scratch/RelayHostTest.app"
xcrun simctl launch --console "$udid" test.uncharted.relayhosttest "$(cat "$scratch/port")" | tee "$scratch/output.txt"
grep -q '^all relay host tests passed$' "$scratch/output.txt" || { echo "the RelayHost test did not pass" >&2; exit 1; }
