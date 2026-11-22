#!/usr/bin/env bash
# Build and verify the Android FEXCore synthetic guest smoke harness (#208).
# Runs the host C++20 verification unconditionally, and cross-builds (plus optionally executes via
# adb) for Android arm64-v8a when NDK / ANDROID_NDK_HOME / ANDROID_NDK_ROOT is configured.
set -euo pipefail

HERE=$(cd -- "$(dirname -- "$0")/.." && pwd)
ROOT=$(cd -- "$HERE/.." && pwd)
SYNTHETIC_DIR=$HERE/synthetic
WORK=${WORK:-$(mktemp -d "${TMPDIR:-/tmp}/uncharted-fex-smoke.XXXXXX")}
CLEAN_WORK=0
if [ "${WORK_KEEP:-0}" != "1" ] && [ -z "${WORK_EXTERNAL:-}" ]; then
    CLEAN_WORK=1
fi
cleanup() {
    if [ "$CLEAN_WORK" = "1" ] && [ -n "${WORK:-}" ] && [ -d "$WORK" ]; then
        rm -rf "$WORK"
    fi
}
trap cleanup EXIT

GEN_ARGS=()
if command -v ninja >/dev/null 2>&1; then
    GEN_ARGS=(-G Ninja)
fi

echo "== Host C++20 synthetic FEX smoke build & check"
HOST_BUILD="$WORK/host"
cmake -S "$SYNTHETIC_DIR" -B "$HOST_BUILD" "${GEN_ARGS[@]}" -DCMAKE_BUILD_TYPE=RelWithDebInfo
cmake --build "$HOST_BUILD" --parallel
ctest --test-dir "$HOST_BUILD" --output-on-failure

HOST_OUT=$("$HOST_BUILD/uncharted-android-fex-smoke")
echo "$HOST_OUT"
for marker in \
    "FEXCORE_ANDROID_SMOKE_OK" \
    "scalar=ok" \
    "vector=ok" \
    "rflags=ok" \
    "unaligned=ok" \
    "callback=ok" \
    "nested_callback=ok" \
    "pointer_arg=ok" \
    "stack_arg=ok" \
    "tls=ok" \
    "pthread_tls_dtor=ok" \
    "mapping=ok" \
    "overlap_rejected=ok" \
    "invalidation=ok" \
    "teardown=ok" \
    "revision=f2b679f6028ce1c38875233aecfcf5d3f8ebecec"; do
    case "$HOST_OUT" in
        *"$marker"*) ;;
        *)
            echo "missing required host smoke marker: $marker" >&2
            exit 1
            ;;
    esac
done

NDK=${NDK:-${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}}
if [ -n "$NDK" ] && [ -f "$NDK/build/cmake/android.toolchain.cmake" ]; then
    API=${ANDROID_API:-33}
    ABI=${ANDROID_ABI:-arm64-v8a}
    NDK_BUILD="$WORK/android-$ABI"
    echo "== Android NDK ($ABI, android-$API, bionic/libc++) cross-build"
    cmake -S "$SYNTHETIC_DIR" -B "$NDK_BUILD" "${GEN_ARGS[@]}" \
        -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
        -DANDROID_ABI="$ABI" \
        -DANDROID_PLATFORM="android-$API" \
        -DANDROID_STL=c++_shared \
        -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
        -DCMAKE_BUILD_TYPE=Release
    cmake --build "$NDK_BUILD" --parallel

    for bin in uncharted-android-fex-smoke test_android_fex_smoke; do
        BIN_PATH="$NDK_BUILD/$bin"
        [ -f "$BIN_PATH" ] || { echo "missing Android binary: $BIN_PATH" >&2; exit 1; }
        if command -v readelf >/dev/null 2>&1; then
            readelf -h "$BIN_PATH" | grep -q "Machine:[[:space:]]*AArch64" || {
                echo "$bin is not AArch64" >&2; exit 1;
            }
            readelf -l "$BIN_PATH" | grep -q "/system/bin/linker64" || {
                echo "$bin missing bionic /system/bin/linker64 interpreter" >&2; exit 1;
            }
            if readelf -d "$BIN_PATH" | grep -q "libc\.so\.6"; then
                echo "$bin unexpectedly links glibc libc.so.6" >&2; exit 1;
            fi
        fi
        if command -v strings >/dev/null 2>&1; then
            strings "$BIN_PATH" | grep -q "FEXCORE_ANDROID_SMOKE_OK" || {
                echo "$bin missing FEXCORE_ANDROID_SMOKE_OK marker" >&2; exit 1;
            }
        fi
    done

    if command -v adb >/dev/null 2>&1 && adb get-state >/dev/null 2>&1; then
        echo "== Execute Android NDK synthetic smoke on attached adb device"
        HOST_OS=linux-x86_64
        LIBCXX="$NDK/toolchains/llvm/prebuilt/$HOST_OS/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so"
        adb shell mkdir -p /data/local/tmp/uncharted-fex-smoke
        adb push "$NDK_BUILD/uncharted-android-fex-smoke" "$NDK_BUILD/test_android_fex_smoke" "$LIBCXX" \
            /data/local/tmp/uncharted-fex-smoke/ >/dev/null
        adb shell 'cd /data/local/tmp/uncharted-fex-smoke && chmod 755 uncharted-android-fex-smoke test_android_fex_smoke && LD_LIBRARY_PATH=. ./test_android_fex_smoke && LD_LIBRARY_PATH=. ./uncharted-android-fex-smoke'
    else
        echo "Android NDK cross-build verified; no attached adb device for on-device execution." >&2
    fi
else
    echo "Android NDK not set; host-side synthetic smoke verified only (not Android device proof)." >&2
fi
