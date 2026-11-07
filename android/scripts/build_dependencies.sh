#!/usr/bin/env bash
# Build the pinned Android prerequisites and link an authored SDL smoke binary.
# WORK=/path NDK=/path/to/android-ndk-r30 SDK=/path/to/sdk scripts/build_dependencies.sh
set -euo pipefail

HERE=$(cd -- "$(dirname -- "$0")/.." && pwd)
ROOT=$(cd -- "$HERE/.." && pwd)
LOCK=$HERE/components.lock.json
WORK=${WORK:?Set WORK to an external build directory}
NDK=${NDK:-${ANDROID_NDK_HOME:-}}
SDK=${SDK:-${ANDROID_SDK_ROOT:-}}
API=33
ABI=arm64-v8a
HOST=linux-x86_64
JOBS=${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)}
PREFIX=$WORK/prefix
mkdir -p "$WORK/src" "$WORK/build" "$WORK/dist" "$PREFIX"

need() {
    command -v "$1" >/dev/null 2>&1 || { echo "missing host tool: $1" >&2; exit 2; }
}
for tool in git cmake ninja python3 curl pkg-config bison flex file readelf; do need "$tool"; done
[ -n "$NDK" ] && [ -f "$NDK/source.properties" ] || {
    echo 'Set NDK to an installed Android NDK r30 directory.' >&2; exit 2;
}
grep -Eq 'Pkg\.Revision = 30\.' "$NDK/source.properties" || {
    echo "Expected Android NDK r30 at $NDK" >&2; exit 2;
}
[ -n "$SDK" ] && [ -f "$SDK/platforms/android-35/android.jar" ] &&
    [ -x "$SDK/build-tools/35.0.1/aapt2" ] || {
    echo 'SDK must include platforms;android-35 and build-tools;35.0.1.' >&2; exit 2;
}

read_component() {
    python3 - "$LOCK" "$1" "$2" <<'PY'
import json, sys
lock, name, key = sys.argv[1:]
data = json.load(open(lock, encoding="utf-8"))
component = next(c for c in data["components"] if c["name"] == name)
print(component[key])
PY
}

fetch_commit() {
    local name=$1 url=$2 revision=$3 path=$WORK/src/$1
    if [ -d "$path/.git" ] && [ "$(git -C "$path" rev-parse HEAD)" = "$revision" ]; then
        return
    fi
    rm -rf "$path"
    git init -q "$path"
    git -C "$path" remote add origin "$url"
    git -C "$path" fetch -q --depth 1 origin "$revision"
    git -C "$path" checkout -q --detach FETCH_HEAD
    [ "$(git -C "$path" rev-parse HEAD)" = "$revision" ]
}

fetch_ref() {
    local name=$1 url=$2 revision=$3 path=$WORK/src/$1
    if [ -d "$path/.git" ] && [ "$(git -C "$path" rev-parse HEAD)" = "$revision" ]; then
        return
    fi
    rm -rf "$path"
    git init -q "$path"
    git -C "$path" remote add origin "$url"
    git -C "$path" fetch -q --depth 1 origin "$revision"
    git -C "$path" checkout -q --detach FETCH_HEAD
    [ "$(git -C "$path" rev-parse HEAD)" = "$revision" ]
}

cmake_android() {
    local name=$1 source=$2; shift 2
    cmake -S "$source" -B "$WORK/build/$name" -G Ninja \
        -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
        -DANDROID_ABI="$ABI" -DANDROID_PLATFORM="$API" \
        -DANDROID_STL=c++_shared -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_POSITION_INDEPENDENT_CODE=ON "$@"
    cmake --build "$WORK/build/$name" --parallel "$JOBS"
    cmake --install "$WORK/build/$name"
}

FEX_URL=$(read_component fex url); FEX_REV=$(read_component fex revision)
MESA_URL=$(read_component mesa-turnip url); MESA_REV=$(read_component mesa-turnip revision)
ADRENO_URL=$(read_component libadrenotools url); ADRENO_REV=$(read_component libadrenotools revision)
SDL_URL=$(read_component sdl3 url); SDL_REV=$(read_component sdl3 revision)

echo '== Fetch FEXCore and its pinned submodules'
fetch_commit fex "$FEX_URL" "$FEX_REV"
git -C "$WORK/src/fex" submodule update --init --depth 1 \
    External/fmt External/xxhash External/range-v3 External/unordered_dense \
    External/rpmalloc External/jemalloc_glibc External/vixl External/zydis \
    Source/Common/cpp-optparse
cmake -S "$WORK/src/fex" -B "$WORK/build/fex" -G Ninja \
    -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI="$ABI" -DANDROID_PLATFORM="$API" -DANDROID_STL=c++_shared \
    -DCMAKE_BUILD_TYPE=Release -DBUILD_FEXCONFIG=OFF -DBUILD_TESTING=OFF \
    -DBUILD_THUNKS=OFF -DENABLE_JEMALLOC_GLIBC_ALLOC=OFF -DENABLE_LTO=OFF \
    -DENABLE_OFFLINE_TELEMETRY=OFF -DENABLE_CCACHE=OFF -DTUNE_CPU=cortex-a78 \
    -DBUILD_FEXCORE_ONLY=ON
cmake --build "$WORK/build/fex" --parallel "$JOBS" --target FEXCore Common CommonTools JemallocLibs rpmalloc

echo '== Build Mesa Turnip for Android KGSL'
fetch_commit mesa-turnip "$MESA_URL" "$MESA_REV"
python3 -m venv "$WORK/mesa-venv"
"$WORK/mesa-venv/bin/pip" install --disable-pip-version-check \
    'meson==1.12.1' 'mako==1.4.3' 'pyyaml==6.0.3' 'packaging==26.3'
TC=$NDK/toolchains/llvm/prebuilt/$HOST/bin
cat > "$WORK/build/mesa-android.ini" <<EOF
[binaries]
ar = '$TC/llvm-ar'
c = '$TC/aarch64-linux-android$API-clang'
cpp = ['$TC/aarch64-linux-android$API-clang++', '-fno-exceptions', '-fno-unwind-tables', '-fno-asynchronous-unwind-tables']
c_ld = 'lld'
cpp_ld = 'lld'
strip = '$TC/llvm-strip'
pkg-config = ['/usr/bin/pkg-config']

[host_machine]
system = 'android'
cpu_family = 'aarch64'
cpu = 'armv8'
endian = 'little'
EOF
"$WORK/mesa-venv/bin/meson" setup "$WORK/build/mesa" "$WORK/src/mesa-turnip" \
    --cross-file "$WORK/build/mesa-android.ini" --prefix "$PREFIX" \
    -Dbuildtype=release -Dplatforms=android -Dplatform-sdk-version="$API" \
    -Dandroid-stub=true -Dgallium-drivers= -Degl=disabled -Dopengl=false \
    -Dgles1=disabled -Dgles2=disabled -Dglx=disabled -Dvulkan-drivers=freedreno \
    -Dfreedreno-kmds=kgsl -Dvulkan-beta=true -Db_lto=false -Dstrip=true \
    -Dllvm=disabled -Dvalgrind=disabled -Dandroid-libbacktrace=disabled
ninja -C "$WORK/build/mesa" -j"$JOBS" src/freedreno/vulkan/libvulkan_freedreno.so
install -D "$WORK/build/mesa/src/freedreno/vulkan/libvulkan_freedreno.so" \
    "$WORK/dist/libvulkan_freedreno.so"

echo '== Build libadrenotools and SDL 3'
fetch_commit libadrenotools "$ADRENO_URL" "$ADRENO_REV"
git -C "$WORK/src/libadrenotools" submodule update --init --depth 1
cmake_android adrenotools "$WORK/src/libadrenotools"
fetch_commit sdl3 "$SDL_URL" "$SDL_REV"
cmake_android sdl3 "$WORK/src/sdl3" -DSDL_SHARED=ON -DSDL_STATIC=OFF \
    -DSDL_TEST_LIBRARY=OFF -DSDL_TESTS=OFF -DSDL_EXAMPLES=OFF

echo '== Link the authored synthetic Android smoke binary'
"$TC/aarch64-linux-android$API-clang" -fPIE -pie -O2 \
    -I"$PREFIX/include" "$ROOT/android/synthetic/smoke.c" \
    -L"$PREFIX/lib" -Wl,-rpath,'$ORIGIN' -lSDL3 \
    -o "$WORK/dist/uncharted-android-smoke"
cp "$PREFIX/lib/libSDL3.so" "$WORK/dist/"
cp "$NDK/toolchains/llvm/prebuilt/$HOST/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" "$WORK/dist/"
file "$WORK/dist/uncharted-android-smoke"
readelf -h "$WORK/dist/uncharted-android-smoke" | grep -E 'Class:|Machine:'

if command -v adb >/dev/null 2>&1 && adb get-state >/dev/null 2>&1; then
    echo '== Execute synthetic smoke binary on the attached Android device'
    adb shell mkdir -p /data/local/tmp/uncharted-android-smoke
    adb push "$WORK/dist/uncharted-android-smoke" "$WORK/dist/libSDL3.so" \
        "$WORK/dist/libc++_shared.so" \
        /data/local/tmp/uncharted-android-smoke/ >/dev/null
    adb shell 'cd /data/local/tmp/uncharted-android-smoke && chmod 755 uncharted-android-smoke && LD_LIBRARY_PATH=. ./uncharted-android-smoke'
else
    echo 'No attached adb device; Android execution was not performed.' >&2
fi
echo "Artifacts: $WORK/dist"
