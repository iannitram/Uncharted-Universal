#!/usr/bin/env bash
# Stage 7.03 (#524): Build and verify three per-game thin Android APKs (U1, U2, U3)
# using NDK r30 and Android SDK 35 (aapt2 + zipalign + apksigner) with zero bundled game assets (M9).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT_DIR="${OUT_DIR:-/tmp/uncharted-android-apks}"
WORK="${WORK:-/tmp/uncharted-android-apks-work}"
GAMES_CSV="u1,u2,u3"
PROFILE="release"
NDK="${NDK:-$HOME/Android/android-ndk-r30}"
SDK="${SDK:-$HOME/Android/sdk}"
BUILD_TOOLS="${BUILD_TOOLS:-$SDK/build-tools/35.0.1}"
ANDROID_JAR="${ANDROID_JAR:-$SDK/platforms/android-35/android.jar}"

usage() {
    cat <<'EOF'
Usage: build_apks.sh [options]

Options:
  --out-dir <dir>    Output directory for signed APKs (default: /tmp/uncharted-android-apks)
  --work <dir>       Scratch build directory outside repo (default: /tmp/uncharted-android-apks-work)
  --games <list>     Comma-separated subset of u1,u2,u3 (default: u1,u2,u3)
  --profile <mode>   Cargo profile: release or debug (default: release)
  -h, --help         Show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --out-dir) OUT_DIR="$2"; shift 2 ;;
        --work) WORK="$2"; shift 2 ;;
        --games) GAMES_CSV="$2"; shift 2 ;;
        --profile) PROFILE="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

export PATH="$HOME/.cargo/bin:$PATH"

for req in cargo rustc python3 keytool; do
    if ! command -v "$req" >/dev/null 2>&1; then
        echo "Missing required host tool: $req" >&2
        exit 2
    fi
done

CLANGXX="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android33-clang++"
CLANG="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android33-clang"
LLVM_STRIP="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"
GLUE_DIR="$NDK/sources/android/native_app_glue"
LIBCXX_SHARED="$NDK/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so"
AAPT2="$BUILD_TOOLS/aapt2"
ZIPALIGN="$BUILD_TOOLS/zipalign"
APKSIGNER="$BUILD_TOOLS/apksigner"

for path_req in "$CLANGXX" "$CLANG" "$GLUE_DIR/android_native_app_glue.c" "$ANDROID_JAR" "$AAPT2" "$ZIPALIGN" "$APKSIGNER"; do
    if [[ ! -e "$path_req" ]]; then
        echo "Missing required Android NDK/SDK artifact: $path_req" >&2
        exit 2
    fi
done

mkdir -p "$OUT_DIR" "$WORK/obj" "$WORK/stage"

# Locate or build libadrenotools for android/driver_loader.cpp.
ADRENOTOOLS_INC="${ADRENOTOOLS_INC:-}"
ADRENOTOOLS_LIB="${ADRENOTOOLS_LIB:-}"
LINKERNSBYPASS_LIB="${LINKERNSBYPASS_LIB:-}"

if [[ -z "$ADRENOTOOLS_INC" || -z "$ADRENOTOOLS_LIB" ]]; then
    for candidate_work in /tmp/uncharted-android-work-207 /tmp/uncharted-android-work "$WORK"; do
        if [[ -f "$candidate_work/src/libadrenotools/include/adrenotools/driver.h" && \
              -f "$candidate_work/build/adrenotools/libadrenotools.a" ]]; then
            ADRENOTOOLS_INC="$candidate_work/src/libadrenotools/include"
            ADRENOTOOLS_LIB="$candidate_work/build/adrenotools/libadrenotools.a"
            if [[ -f "$candidate_work/build/adrenotools/lib/linkernsbypass/liblinkernsbypass.a" ]]; then
                LINKERNSBYPASS_LIB="$candidate_work/build/adrenotools/lib/linkernsbypass/liblinkernsbypass.a"
            fi
            break
        fi
    done
fi

if [[ -z "$ADRENOTOOLS_INC" || -z "$ADRENOTOOLS_LIB" ]]; then
    echo "== Building pinned libadrenotools into $WORK"
    readarray -t ADRENO_META < <(python3 - "$REPO_ROOT/android/components.lock.json" <<'PY'
import json, sys
lock = json.load(open(sys.argv[1]))
comp = next(c for c in lock["components"] if c["name"] == "libadrenotools")
print(comp["url"])
print(comp["revision"])
PY
)
    ADRENO_URL="${ADRENO_META[0]}"
    ADRENO_REV="${ADRENO_META[1]}"
    ADRENO_SRC="$WORK/src/libadrenotools"
    ADRENO_BLD="$WORK/build/adrenotools"
    if [[ ! -d "$ADRENO_SRC/.git" ]]; then
        mkdir -p "$WORK/src"
        git clone --filter=blob:none "$ADRENO_URL" "$ADRENO_SRC"
    fi
    git -C "$ADRENO_SRC" checkout --force "$ADRENO_REV"
    git -C "$ADRENO_SRC" submodule update --init --depth 1
    cmake -S "$ADRENO_SRC" -B "$ADRENO_BLD" -G Ninja \
        -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
        -DANDROID_ABI=arm64-v8a \
        -DANDROID_PLATFORM=android-33 \
        -DANDROID_STL=c++_shared \
        -DCMAKE_BUILD_TYPE=Release
    cmake --build "$ADRENO_BLD"
    ADRENOTOOLS_INC="$ADRENO_SRC/include"
    ADRENOTOOLS_LIB="$ADRENO_BLD/libadrenotools.a"
    LINKERNSBYPASS_LIB="$ADRENO_BLD/lib/linkernsbypass/liblinkernsbypass.a"
fi

# Debug keystore for local/CI thin APK signing unless overridden.
KEYSTORE="${APK_KEYSTORE:-$WORK/debug.keystore}"
KEYSTORE_PASS="${APK_KEYSTORE_PASS:-android}"
KEY_ALIAS="${APK_KEY_ALIAS:-androiddebugkey}"

if [[ ! -f "$KEYSTORE" ]]; then
    keytool -genkeypair -v \
        -keystore "$KEYSTORE" \
        -storepass "$KEYSTORE_PASS" \
        -alias "$KEY_ALIAS" \
        -keypass "$KEYSTORE_PASS" \
        -keyalg RSA -keysize 2048 -validity 10000 \
        -dname "CN=Uncharted Universal Debug,O=UnchartedUniversal,C=US" >/dev/null 2>&1
fi

CARGO_PROFILE_FLAGS=()
CARGO_TARGET_SUBDIR="debug"
if [[ "$PROFILE" == "release" ]]; then
    CARGO_PROFILE_FLAGS+=(--release)
    CARGO_TARGET_SUBDIR="release"
fi

echo "== Building uncharted_runtime ($PROFILE, aarch64-linux-android)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$WORK/cargo-target}"
cargo build --manifest-path "$REPO_ROOT/Cargo.toml" \
    -p uncharted_runtime --lib --target aarch64-linux-android "${CARGO_PROFILE_FLAGS[@]}"
RUNTIME_LIB="$CARGO_TARGET_DIR/aarch64-linux-android/$CARGO_TARGET_SUBDIR/libuncharted_runtime.a"

echo "== Compiling NDK r30 native glue and C++ bridges"
"$CLANG" -c -fPIC -O2 -DANDROID -DANativeActivity_onCreate=glue_ANativeActivity_onCreate -I"$GLUE_DIR" \
    "$GLUE_DIR/android_native_app_glue.c" \
    -o "$WORK/obj/android_native_app_glue.o"

"$CLANGXX" -std=c++20 -c -fPIC -O2 -Wall -Wextra -Werror \
    -I"$REPO_ROOT" \
    "$REPO_ROOT/android/fex_bridge.cpp" \
    -o "$WORK/obj/fex_bridge.o"

"$CLANGXX" -std=c++20 -c -fPIC -O2 -Wall -Wextra -Werror \
    -I"$REPO_ROOT" -I"$ADRENOTOOLS_INC" \
    "$REPO_ROOT/android/driver_loader.cpp" \
    -o "$WORK/obj/driver_loader.o"

"$CLANGXX" -std=c++20 -c -fPIC -O2 -Wall -Wextra -Werror \
    -I"$REPO_ROOT" -I"$GLUE_DIR" \
    "$REPO_ROOT/android/apk/native/android_bridge.cpp" \
    -o "$WORK/obj/android_bridge.o"

verify_apk_m9() {
    local apk_path="$1"
    local game="$2"
    local expected_pkg="org.uncharted.universal.$game"
    local expected_label
    case "$game" in
        u1) expected_label="Uncharted 1" ;;
        u2) expected_label="Uncharted 2" ;;
        u3) expected_label="Uncharted 3" ;;
        *) echo "Unsupported game key: $game" >&2; exit 2 ;;
    esac

    local badging
    badging="$("$AAPT2" dump badging "$apk_path")"
    if ! grep -q "package: name='$expected_pkg'" <<<"$badging"; then
        echo "M9/7.03 check failed: missing package $expected_pkg in $apk_path" >&2
        exit 1
    fi
    if ! grep -q "application: label='$expected_label'" <<<"$badging"; then
        echo "M9/7.03 check failed: missing label '$expected_label' in $apk_path" >&2
        exit 1
    fi
    local xmltree
    xmltree="$("$AAPT2" dump xmltree --file AndroidManifest.xml "$apk_path")"
    if ! grep -q "org.uncharted.game_id" <<<"$xmltree" || ! grep -q "\"$game\"" <<<"$xmltree"; then
        echo "M9/7.03 check failed: missing org.uncharted.game_id=$game in $apk_path" >&2
        exit 1
    fi

    python3 - "$apk_path" <<'PY'
import sys, zipfile

apk_path = sys.argv[1]
forbidden_exts = (
    ".psarc", ".pak", ".bin", ".elf", ".self", ".prx", ".sprx",
    ".dds", ".gnf", ".png", ".jpg", ".jpeg", ".webp", ".bmp",
    ".mp4", ".bik", ".bk2", ".at9", ".wem", ".bnk", ".pkg", ".iso",
)
forbidden_parts = ("game/", "usrdir/", "tnd_", "artwork/", "assets/game", "sce_sys/")

with zipfile.ZipFile(apk_path, "r") as zf:
    names = zf.namelist()
    if "lib/arm64-v8a/libuncharted_android_apk.so" not in names:
        raise SystemExit(f"Missing lib/arm64-v8a/libuncharted_android_apk.so in {apk_path}")
    for name in names:
        low = name.lower()
        if low.endswith(forbidden_exts) or any(part in low for part in forbidden_parts):
            raise SystemExit(f"M9 violation: forbidden game/artwork entry {name} in {apk_path}")
print(f"M9/7.03 verified clean ({len(names)} entries): {apk_path}")
PY
}

IFS=',' read -ra GAMES <<<"$GAMES_CSV"
for game in "${GAMES[@]}"; do
    game="${game// /}"
    manifest="$REPO_ROOT/android/apk/manifests/$game/AndroidManifest.xml"
    if [[ ! -f "$manifest" ]]; then
        echo "Missing manifest for game '$game': $manifest" >&2
        exit 2
    fi

    echo "== Building uncharted_android_apk ($game, $PROFILE, aarch64-linux-android)"
    cargo build --manifest-path "$REPO_ROOT/android/apk/Cargo.toml" \
        --lib --target aarch64-linux-android --features "game-$game" "${CARGO_PROFILE_FLAGS[@]}"
    APK_RUST_LIB="$CARGO_TARGET_DIR/aarch64-linux-android/$CARGO_TARGET_SUBDIR/libuncharted_android_apk.a"

    STAGE="$WORK/stage/$game"
    rm -rf "$STAGE"
    mkdir -p "$STAGE/lib/arm64-v8a"

    SO_OUT="$STAGE/lib/arm64-v8a/libuncharted_android_apk.so"
    "$CLANGXX" -shared -Wl,-soname,libuncharted_android_apk.so \
        -Wl,--gc-sections \
        -Wl,--no-undefined \
        -Wl,-u,ANativeActivity_onCreate \
        -o "$SO_OUT" \
        "$WORK/obj/android_native_app_glue.o" \
        "$WORK/obj/android_bridge.o" \
        "$WORK/obj/fex_bridge.o" \
        "$WORK/obj/driver_loader.o" \
        "$APK_RUST_LIB" \
        "$RUNTIME_LIB" \
        "$ADRENOTOOLS_LIB" \
        ${LINKERNSBYPASS_LIB:+"$LINKERNSBYPASS_LIB"} \
        -landroid -llog -ldl -lm

    if readelf -d "$SO_OUT" | grep -q "libc++_shared.so"; then
        cp "$LIBCXX_SHARED" "$STAGE/lib/arm64-v8a/libc++_shared.so"
        "$LLVM_STRIP" --strip-unneeded "$STAGE/lib/arm64-v8a/libc++_shared.so"
    fi
    "$LLVM_STRIP" --strip-unneeded "$SO_OUT"

    "$AAPT2" compile \
        --dir "$REPO_ROOT/android/apk/res" \
        -o "$STAGE/res_flat.zip"

    "$AAPT2" link \
        -I "$ANDROID_JAR" \
        --manifest "$manifest" \
        "$STAGE/res_flat.zip" \
        -o "$STAGE/base.apk"

    python3 - "$STAGE/base.apk" "$STAGE/lib/arm64-v8a" <<'PY'
import os, sys, zipfile

apk_path, lib_dir = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(apk_path, "a", compression=zipfile.ZIP_DEFLATED) as zf:
    for fn in sorted(os.listdir(lib_dir)):
        if fn.endswith(".so"):
            zf.write(os.path.join(lib_dir, fn), f"lib/arm64-v8a/{fn}")
PY

    "$ZIPALIGN" -f -p 4 "$STAGE/base.apk" "$STAGE/aligned.apk"
    FINAL_APK="$OUT_DIR/uncharted-$game-thin.apk"
    "$APKSIGNER" sign \
        --ks "$KEYSTORE" \
        --ks-pass "pass:$KEYSTORE_PASS" \
        --ks-key-alias "$KEY_ALIAS" \
        --out "$FINAL_APK" \
        "$STAGE/aligned.apk"
    "$APKSIGNER" verify "$FINAL_APK"
    verify_apk_m9 "$FINAL_APK" "$game"
done

echo "== Built thin APKs in $OUT_DIR:"
ls -lh "$OUT_DIR"/uncharted-*-thin.apk
