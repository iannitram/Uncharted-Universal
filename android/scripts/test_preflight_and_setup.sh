#!/usr/bin/env bash
# Unit/integration tests for build_dependencies.sh (--check-lock, --preflight)
# and setup_ndk_sdk.sh (--dry-run, idempotency).
set -euo pipefail

HERE=$(cd -- "$(dirname -- "$0")/.." && pwd)
BUILD_SCRIPT="$HERE/scripts/build_dependencies.sh"
SETUP_SCRIPT="$HERE/scripts/setup_ndk_sdk.sh"

TMP=$(mktemp -d "${TMPDIR:-/tmp}/uncharted-android-scripts-test.XXXXXX")
cleanup() {
    rm -rf "$TMP"
}
trap cleanup EXIT

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

expect_exit() {
    local expected=$1; shift
    local rc=0
    "$@" >"$TMP/last.out" 2>"$TMP/last.err" || rc=$?
    if [ "$rc" -ne "$expected" ]; then
        echo "=== stdout ===" >&2
        cat "$TMP/last.out" >&2
        echo "=== stderr ===" >&2
        cat "$TMP/last.err" >&2
        fail "expected exit $expected, got $rc for: $*"
    fi
}

make_mock_ndk() {
    local ndk=$1 rev=${2:-30.0.16248370}
    mkdir -p "$ndk/build/cmake" \
        "$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin" \
        "$ndk/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android"
    cat >"$ndk/source.properties" <<EOF
Pkg.Desc = Android NDK
Pkg.Revision = $rev
EOF
    : >"$ndk/build/cmake/android.toolchain.cmake"
    : >"$ndk/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so"
    for bin in aarch64-linux-android33-clang aarch64-linux-android33-clang++ llvm-ar llvm-strip; do
        printf '#!/bin/sh\nexit 0\n' >"$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/$bin"
        chmod +x "$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/$bin"
    done
}

make_mock_sdk() {
    local sdk=$1
    mkdir -p "$sdk/platforms/android-35" "$sdk/build-tools/35.0.1"
    : >"$sdk/platforms/android-35/android.jar"
    printf '#!/bin/sh\nexit 0\n' >"$sdk/build-tools/35.0.1/aapt2"
    chmod +x "$sdk/build-tools/35.0.1/aapt2"
}

make_mock_tools_dir() {
    local tdir=$1
    mkdir -p "$tdir"
    local tool real
    for tool in git cmake ninja python3 curl pkg-config bison flex file readelf; do
        if real=$(command -v "$tool" 2>/dev/null); then
            ln -sf "$real" "$tdir/$tool"
        else
            printf '#!/bin/sh\nexit 0\n' >"$tdir/$tool"
            chmod +x "$tdir/$tool"
        fi
    done
}

echo "== 1. --help flags"
expect_exit 0 bash "$BUILD_SCRIPT" --help
expect_exit 0 bash "$SETUP_SCRIPT" --help
expect_exit 2 bash "$BUILD_SCRIPT" --unknown-flag

echo "== 2. Real repo --check-lock"
expect_exit 0 bash "$BUILD_SCRIPT" --check-lock
grep -q "lock schema v1 valid (4 components:" "$TMP/last.out" || fail "missing lock ok line"
grep -q "patch series valid (fex: 3 patches, mesa: 23 patches)" "$TMP/last.out" || fail "missing patch summary"

echo "== 3. Lockfile negative tests"
python3 - "$HERE/components.lock.json" "$TMP/bad-schema.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
d["schemaVersion"] = 99
json.dump(d, open(sys.argv[2], "w"))
PY
expect_exit 3 env LOCK="$TMP/bad-schema.json" bash "$BUILD_SCRIPT" --check-lock
grep -q "schemaVersion must be integer 1" "$TMP/last.err" || fail "bad schema not reported"

python3 - "$HERE/components.lock.json" "$TMP/bad-sha.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
d["components"][0]["revision"] = "not-a-sha"
json.dump(d, open(sys.argv[2], "w"))
PY
expect_exit 3 env LOCK="$TMP/bad-sha.json" bash "$BUILD_SCRIPT" --check-lock
grep -q "40-char lowercase SHA-1" "$TMP/last.err" || fail "bad sha not reported"

python3 - "$HERE/components.lock.json" "$TMP/missing-comp.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
d["components"] = [c for c in d["components"] if c["name"] != "sdl3"]
json.dump(d, open(sys.argv[2], "w"))
PY
expect_exit 3 env LOCK="$TMP/missing-comp.json" bash "$BUILD_SCRIPT" --check-lock
grep -q "missing required components: sdl3" "$TMP/last.err" || fail "missing component not reported"

echo "== 4. Patch negative tests"
cp -a "$HERE/patches" "$TMP/patches-empty"
: >"$TMP/patches-empty/fex/fex-0001-android.patch"
expect_exit 3 env PATCHES="$TMP/patches-empty" bash "$BUILD_SCRIPT" --check-lock
grep -q "empty or non-regular patch file" "$TMP/last.err" || fail "empty patch not reported"

cp -a "$HERE/patches" "$TMP/patches-corrupt"
cat >"$TMP/patches-corrupt/mesa/mesa-0001-kgsl-zero-timeout-poll.patch" <<'EOF'
From d755ef3f0589fca9950cfd97718c2384ae22a4b2 Mon Sep 17 00:00:00 2001
From: test <test@example.com>
Subject: [PATCH] corrupt

diff --git a/foo.c b/foo.c
--- a/foo.c
+++ b/foo.c
@@ -1,5 +1,6 @@
 broken hunk
EOF
expect_exit 3 env PATCHES="$TMP/patches-corrupt" bash "$BUILD_SCRIPT" --check-lock
grep -q "git apply --numstat rejected" "$TMP/last.err" || fail "corrupt patch not reported"

cp -a "$HERE/patches" "$TMP/patches-dup"
cp "$TMP/patches-dup/fex/fex-0001-android.patch" "$TMP/patches-dup/fex/fex-0001-duplicate.patch"
expect_exit 3 env PATCHES="$TMP/patches-dup" bash "$BUILD_SCRIPT" --check-lock
grep -q "duplicate or out-of-order patch number" "$TMP/last.err" || fail "duplicate patch index not reported"

echo "== 5. --preflight failure diagnostics (missing NDK/SDK)"
expect_exit 2 env HOME="$TMP/empty-home" NDK="" SDK="" ANDROID_NDK_HOME="" ANDROID_NDK_ROOT="" ANDROID_SDK_ROOT="" ANDROID_HOME="" \
    bash "$BUILD_SCRIPT" --preflight
grep -q "setup_ndk_sdk.sh" "$TMP/last.err" || fail "missing setup_ndk_sdk.sh hint"
grep -q "NDK r30" "$TMP/last.err" || fail "missing NDK r30 diagnostic"
grep -q "SDK 35" "$TMP/last.err" || fail "missing SDK 35 diagnostic"

make_mock_ndk "$TMP/ndk-r27" "27.0.12077973"
make_mock_sdk "$TMP/sdk-bad"
rm -f "$TMP/sdk-bad/build-tools/35.0.1/aapt2"
expect_exit 2 env NDK="$TMP/ndk-r27" SDK="$TMP/sdk-bad" bash "$BUILD_SCRIPT" --preflight
grep -q "not Pkg.Revision = 30.x" "$TMP/last.err" || fail "wrong NDK version not reported"
grep -q "build-tools;35.0.1" "$TMP/last.err" || fail "missing build-tools;35.0.1 not reported"

echo "== 6. --preflight success with mock NDK r30 + SDK 35 + host tools"
make_mock_ndk "$TMP/ndk-r30" "30.0.16248370"
make_mock_sdk "$TMP/sdk-ok"
make_mock_tools_dir "$TMP/mock-bin"
expect_exit 0 env PATH="$TMP/mock-bin:$PATH" NDK="$TMP/ndk-r30" SDK="$TMP/sdk-ok" \
    bash "$BUILD_SCRIPT" --preflight
grep -q "Preflight PASSED" "$TMP/last.out" || fail "missing Preflight PASSED"

echo "== 7. setup_ndk_sdk.sh --dry-run (no disk writes)"
DRY_TARGET="$TMP/dry-run-android"
expect_exit 0 bash "$SETUP_SCRIPT" --dry-run --target "$DRY_TARGET" --accept-licenses
[ ! -e "$DRY_TARGET" ] || fail "--dry-run unexpectedly created $DRY_TARGET"
grep -q "android-ndk-r30-linux.zip" "$TMP/last.out" || fail "dry-run missing NDK URL"
grep -q "platforms;android-35" "$TMP/last.out" || fail "dry-run missing platforms;android-35"
grep -q "build-tools;35.0.1" "$TMP/last.out" || fail "dry-run missing build-tools;35.0.1"

echo "== 8. setup_ndk_sdk.sh idempotent skip when already installed"
IDEM_TARGET="$TMP/idem-android"
make_mock_ndk "$IDEM_TARGET/android-ndk-r30"
make_mock_sdk "$IDEM_TARGET/sdk"
expect_exit 0 bash "$SETUP_SCRIPT" --target "$IDEM_TARGET"
grep -q "already installed at $IDEM_TARGET/android-ndk-r30 (skipping)" "$TMP/last.out" || fail "idempotent NDK skip missing"
grep -q "already installed at $IDEM_TARGET/sdk (skipping)" "$TMP/last.out" || fail "idempotent SDK skip missing"

echo "ALL ANDROID SCRIPT TESTS PASSED"
