#!/usr/bin/env bash
# Build the pinned Android prerequisites and link an authored SDL smoke binary.
# WORK=/path NDK=/path/to/android-ndk-r30 SDK=/path/to/sdk android/scripts/build_dependencies.sh
# Also supports --preflight, --check-lock, and --help.
set -euo pipefail

HERE=$(cd -- "$(dirname -- "$0")/.." && pwd)
ROOT=$(cd -- "$HERE/.." && pwd)
LOCK=${LOCK:-$HERE/components.lock.json}
PATCHES=${PATCHES:-$HERE/patches}
LICENSES=${LICENSES:-$HERE/LICENSES.md}
API=${ANDROID_API:-33}
ABI=${ANDROID_ABI:-arm64-v8a}
HOST=linux-x86_64
JOBS=${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)}

usage() {
    cat <<'EOF'
Usage: android/scripts/build_dependencies.sh [OPTIONS]

Build pinned Android dependencies (FEXCore, Mesa Turnip KGSL, libadrenotools,
SDL3) and link the authored synthetic Android smoke binaries.

Modes:
  (default) / --build   Run preflight checks, fetch pinned sources, apply
                        android/patches/{fex,mesa}, build artifacts, and run
                        adb smoke checks if a device is attached. Requires WORK,
                        NDK, and SDK.
  --preflight           Validate components.lock.json and android/patches/,
                        check host build tools, and verify NDK r30 / SDK 35 +
                        build-tools 35.0.1. Does not require WORK or network.
  --check-lock          Validate components.lock.json schema/pins, LICENSES.md
                        coverage, and android/patches/ well-formedness only.
  -h, --help            Show this help message and exit.

Environment variables:
  WORK                  External build/staging directory (required for --build)
  NDK                   Path to Android NDK r30 (or ANDROID_NDK_HOME / ANDROID_NDK_ROOT)
  SDK                   Path to Android SDK (or ANDROID_SDK_ROOT / ANDROID_HOME)
  JOBS                  Parallel build jobs (default: nproc)
  ANDROID_API           Target Android API level (default: 33)
  ANDROID_ABI           Target Android ABI (default: arm64-v8a)

Exit codes:
  0  Success
  2  Missing required host tools, NDK r30, SDK 35, or invalid CLI usage
  3  Lockfile schema/pin failure or invalid/unapplicable patch series
EOF
}

MODE=build
while [ "$#" -gt 0 ]; do
    case "$1" in
        --preflight)
            MODE=preflight
            ;;
        --check-lock)
            MODE=check-lock
            ;;
        --build)
            MODE=build
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

NDK=${NDK:-${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}}
SDK=${SDK:-${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}}

validate_lock_and_patches() {
    if ! command -v python3 >/dev/null 2>&1; then
        echo "[FAIL] python3 is required to validate $LOCK" >&2
        return 3
    fi

    python3 - "$LOCK" "$PATCHES" "$LICENSES" <<'PY'
import glob
import json
import os
import re
import shutil
import subprocess
import sys

lock_path, patches_root, licenses_path = sys.argv[1:4]
errors = []

if not os.path.isfile(lock_path):
    print(f"[FAIL] missing lockfile: {lock_path}", file=sys.stderr)
    sys.exit(3)

try:
    with open(lock_path, encoding="utf-8") as fh:
        data = json.load(fh)
except Exception as exc:
    print(f"[FAIL] invalid JSON in {lock_path}: {exc}", file=sys.stderr)
    sys.exit(3)

if not isinstance(data, dict):
    print(f"[FAIL] {lock_path}: root must be a JSON object", file=sys.stderr)
    sys.exit(3)

schema_ver = data.get("schemaVersion")
if type(schema_ver) is not int or schema_ver != 1:
    errors.append(f"components.lock.json: schemaVersion must be integer 1, got {schema_ver!r}")

host = data.get("host")
expected_host = {
    "os": "linux-x86_64",
    "ndk": "r30",
    "sdkPlatform": 35,
    "buildTools": "35.0.1",
    "androidApi": 33,
    "abi": "arm64-v8a",
}
if not isinstance(host, dict):
    errors.append("components.lock.json: 'host' must be an object")
else:
    for key, expected in expected_host.items():
        val = host.get(key)
        if type(val) is not type(expected) or val != expected:
            errors.append(f"components.lock.json: host.{key} expected {expected!r}, got {val!r}")

components = data.get("components")
required_names = {"fex", "mesa-turnip", "libadrenotools", "sdl3"}
seen_names = set()
sha_re = re.compile(r"^[0-9a-f]{40}$")
name_re = re.compile(r"^[a-z0-9-]+$")
parsed_components = []

if not isinstance(components, list) or not components:
    errors.append("components.lock.json: 'components' must be a non-empty list")
else:
    for idx, comp in enumerate(components):
        if not isinstance(comp, dict):
            errors.append(f"components.lock.json: components[{idx}] must be an object")
            continue
        cname = comp.get("name")
        if not isinstance(cname, str) or not name_re.match(cname):
            errors.append(f"components.lock.json: components[{idx}].name invalid: {cname!r}")
            continue
        if cname in seen_names:
            errors.append(f"components.lock.json: duplicate component {cname!r}")
            continue
        seen_names.add(cname)

        url = comp.get("url")
        if not isinstance(url, str) or not url.startswith("https://"):
            errors.append(f"components.lock.json: {cname}.url must be an https:// URL, got {url!r}")

        rev = comp.get("revision")
        if not isinstance(rev, str) or not sha_re.match(rev):
            errors.append(f"components.lock.json: {cname}.revision must be a 40-char lowercase SHA-1, got {rev!r}")

        for req_field in ("license", "build"):
            val = comp.get(req_field)
            if not isinstance(val, str) or not val.strip():
                errors.append(f"components.lock.json: {cname}.{req_field} must be a non-empty string")

        if "ref" in comp:
            ref_val = comp.get("ref")
            if not isinstance(ref_val, str) or not ref_val.strip():
                errors.append(f"components.lock.json: {cname}.ref must be a non-empty string when present")

        parsed_components.append(comp)

missing_required = sorted(required_names - seen_names)
if missing_required:
    errors.append(f"components.lock.json: missing required components: {', '.join(missing_required)}")

if not os.path.isfile(licenses_path):
    errors.append(f"missing license manifest: {licenses_path}")
else:
    licenses_text = open(licenses_path, encoding="utf-8").read()
    for comp in parsed_components:
        cname = comp["name"]
        rev = comp.get("revision", "")
        ref = comp.get("ref", "")
        if rev not in licenses_text and (not ref or ref not in licenses_text):
            errors.append(f"LICENSES.md does not mention revision/ref for {cname} ({rev or ref})")

if not os.path.isdir(patches_root):
    errors.append(f"missing patches directory: {patches_root}")
else:
    for entry in sorted(os.listdir(patches_root)):
        full = os.path.join(patches_root, entry)
        if os.path.isfile(full) and entry.endswith(".patch"):
            errors.append(f"stray patch outside fex/ or mesa/: {full}")
        elif os.path.isdir(full) and entry not in ("fex", "mesa"):
            stray = glob.glob(os.path.join(full, "*.patch"))
            if stray:
                errors.append(f"unexpected patch directory: {full}")

git_bin = shutil.which("git")
series_counts = {}
for series in ("fex", "mesa"):
    sdir = os.path.join(patches_root, series)
    if not os.path.isdir(sdir):
        errors.append(f"missing patch series directory: {sdir}")
        continue
    patch_files = sorted(glob.glob(os.path.join(sdir, f"{series}-*.patch")))
    other_files = [
        os.path.join(sdir, f)
        for f in sorted(os.listdir(sdir))
        if f.endswith(".patch") and not f.startswith(f"{series}-")
    ]
    for other in other_files:
        errors.append(f"patch filename does not match '{series}-*.patch': {other}")
    if not patch_files:
        errors.append(f"no {series}-*.patch files found in {sdir}")
        continue

    prev_num = -1
    name_pat = re.compile(rf"^{re.escape(series)}-([0-9]{{4}})-[A-Za-z0-9._-]+\.patch$")
    for pfile in patch_files:
        bname = os.path.basename(pfile)
        m = name_pat.match(bname)
        if not m:
            errors.append(f"invalid patch filename (expected {series}-NNNN-slug.patch): {pfile}")
        else:
            num = int(m.group(1))
            if num <= prev_num:
                errors.append(f"duplicate or out-of-order patch number {num:04d} in {pfile}")
            prev_num = num

        if not os.path.isfile(pfile) or os.path.getsize(pfile) == 0:
            errors.append(f"empty or non-regular patch file: {pfile}")
            continue

        raw = open(pfile, "rb").read()
        if b"\x00" in raw:
            errors.append(f"patch contains NUL byte: {pfile}")
            continue
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError as exc:
            errors.append(f"patch is not valid UTF-8 ({exc}): {pfile}")
            continue
        if "\r\n" in text:
            errors.append(f"patch uses CRLF line endings: {pfile}")
        if "diff --git " not in text or ("--- " not in text or "+++ " not in text) or "@@ " not in text:
            errors.append(f"patch missing diff headers or hunk markers: {pfile}")

        if git_bin:
            proc = subprocess.run([git_bin, "apply", "--numstat", pfile], capture_output=True, text=True)
            if proc.returncode != 0 or not proc.stdout.strip():
                msg = (proc.stderr or proc.stdout).strip()
                errors.append(f"git apply --numstat rejected {pfile}: {msg}")

    series_counts[series] = len(patch_files)

if errors:
    for err in errors:
        print(f"[FAIL] {err}", file=sys.stderr)
    sys.exit(3)

comp_list = ", ".join(c["name"] for c in parsed_components)
print(f"[ok] lock schema v{schema_ver} valid ({len(parsed_components)} components: {comp_list})")
for comp in parsed_components:
    ref_note = f" ({comp['ref']})" if "ref" in comp else ""
    print(f"     - {comp['name']}: {comp['revision']}{ref_note} [{comp['license']}]")
print(
    f"[ok] patch series valid (fex: {series_counts.get('fex', 0)} patches, "
    f"mesa: {series_counts.get('mesa', 0)} patches)"
)
PY
}

MISSING_TOOLS=()
MISSING_ANDROID=()
CANDIDATE_HINTS=()

check_host_tools() {
    local uname_s uname_m tool
    uname_s=$(uname -s)
    uname_m=$(uname -m)
    if [ "$uname_s" = "Linux" ] && [ "$uname_m" = "x86_64" ]; then
        echo "[ok] host platform: $uname_s $uname_m"
    else
        echo "[WARN] host platform is $uname_s $uname_m (expected Linux x86_64 for prebuilt NDK $HOST)"
    fi

    for tool in git cmake ninja python3 curl pkg-config bison flex file readelf; do
        if command -v "$tool" >/dev/null 2>&1; then
            echo "[ok] host tool: $tool ($(command -v "$tool"))"
        else
            echo "[MISSING] host tool: $tool" >&2
            MISSING_TOOLS+=("$tool")
        fi
    done

    if command -v python3 >/dev/null 2>&1; then
        if python3 -m venv --help >/dev/null 2>&1 && python3 -c "import venv, ensurepip" >/dev/null 2>&1; then
            echo "[ok] host tool: python3 venv + ensurepip"
        else
            echo "[MISSING] host tool: python3-venv / ensurepip (required for Mesa meson venv)" >&2
            MISSING_TOOLS+=("python3-venv")
        fi
    fi

    for tool in unzip adb; do
        if command -v "$tool" >/dev/null 2>&1; then
            echo "[ok] optional tool: $tool ($(command -v "$tool"))"
        else
            echo "[info] optional tool not present: $tool"
        fi
    done
}

probe_candidate_paths() {
    local candidate
    if [ -z "$NDK" ]; then
        for candidate in "$HOME/Android/android-ndk-r30" "$HOME/Android/Sdk/ndk"/30.* "$HOME/Android/sdk/ndk"/30.*; do
            if [ -f "$candidate/source.properties" ] && grep -Eq 'Pkg\.Revision = 30\.' "$candidate/source.properties"; then
                CANDIDATE_HINTS+=("Detected NDK r30 candidate at $candidate (export NDK=\"$candidate\")")
                break
            fi
        done
    fi
    if [ -z "$SDK" ]; then
        for candidate in "$HOME/Android/sdk" "$HOME/Android/Sdk"; do
            if [ -f "$candidate/platforms/android-35/android.jar" ] && [ -x "$candidate/build-tools/35.0.1/aapt2" ]; then
                CANDIDATE_HINTS+=("Detected SDK 35 candidate at $candidate (export SDK=\"$candidate\")")
                break
            fi
        done
    fi
}

check_ndk_and_sdk() {
    local tc bin libcxx
    probe_candidate_paths

    if [ -z "$NDK" ]; then
        echo "[MISSING] NDK r30: NDK / ANDROID_NDK_HOME / ANDROID_NDK_ROOT is not set" >&2
        MISSING_ANDROID+=("NDK r30 (unset NDK)")
    else
        if [ ! -d "$NDK" ]; then
            echo "[MISSING] NDK r30 directory does not exist: $NDK" >&2
            MISSING_ANDROID+=("NDK r30 directory ($NDK)")
        else
            if [ -f "$NDK/source.properties" ] && grep -Eq 'Pkg\.Revision = 30\.' "$NDK/source.properties"; then
                echo "[ok] NDK revision: $(grep -E 'Pkg\.Revision' "$NDK/source.properties" | tr -d '\r')"
            else
                echo "[MISSING] NDK r30: $NDK/source.properties missing or not Pkg.Revision = 30.x" >&2
                MISSING_ANDROID+=("NDK r30 source.properties ($NDK)")
            fi

            if [ -f "$NDK/build/cmake/android.toolchain.cmake" ]; then
                echo "[ok] NDK CMake toolchain: $NDK/build/cmake/android.toolchain.cmake"
            else
                echo "[MISSING] NDK CMake toolchain: $NDK/build/cmake/android.toolchain.cmake" >&2
                MISSING_ANDROID+=("NDK android.toolchain.cmake")
            fi

            tc="$NDK/toolchains/llvm/prebuilt/$HOST/bin"
            for bin in "aarch64-linux-android$API-clang" "aarch64-linux-android$API-clang++" llvm-ar llvm-strip; do
                if [ -x "$tc/$bin" ]; then
                    echo "[ok] NDK LLVM binary: $bin"
                else
                    echo "[MISSING] NDK LLVM binary: $tc/$bin" >&2
                    MISSING_ANDROID+=("NDK binary $bin")
                fi
            done

            libcxx="$NDK/toolchains/llvm/prebuilt/$HOST/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so"
            if [ -f "$libcxx" ]; then
                echo "[ok] NDK libc++_shared.so: $libcxx"
            else
                echo "[MISSING] NDK libc++_shared.so: $libcxx" >&2
                MISSING_ANDROID+=("NDK libc++_shared.so")
            fi
        fi
    fi

    if [ -z "$SDK" ]; then
        echo "[MISSING] SDK 35: SDK / ANDROID_SDK_ROOT / ANDROID_HOME is not set" >&2
        MISSING_ANDROID+=("SDK platforms;android-35 & build-tools;35.0.1 (unset SDK)")
    else
        if [ ! -d "$SDK" ]; then
            echo "[MISSING] SDK directory does not exist: $SDK" >&2
            MISSING_ANDROID+=("SDK directory ($SDK)")
        else
            if [ -f "$SDK/platforms/android-35/android.jar" ]; then
                echo "[ok] SDK platform: platforms;android-35 ($SDK/platforms/android-35/android.jar)"
            else
                echo "[MISSING] SDK platform: $SDK/platforms/android-35/android.jar (package: platforms;android-35)" >&2
                MISSING_ANDROID+=("platforms;android-35")
            fi

            if [ -x "$SDK/build-tools/35.0.1/aapt2" ]; then
                echo "[ok] SDK build-tools: build-tools;35.0.1 ($SDK/build-tools/35.0.1/aapt2)"
            else
                echo "[MISSING] SDK build-tools: $SDK/build-tools/35.0.1/aapt2 (package: build-tools;35.0.1)" >&2
                MISSING_ANDROID+=("build-tools;35.0.1")
            fi
        fi
    fi
}

print_remediation() {
    local hint
    if [ "${#CANDIDATE_HINTS[@]}" -gt 0 ]; then
        echo "" >&2
        echo "Detected local Android toolchain candidates:" >&2
        for hint in "${CANDIDATE_HINTS[@]}"; do
            echo "  $hint" >&2
        done
    fi

    if [ "${#MISSING_TOOLS[@]}" -gt 0 ]; then
        echo "" >&2
        echo "Missing host build tools: ${MISSING_TOOLS[*]}" >&2
        echo "Install hints:" >&2
        echo "  - Inside this repo's build container: distrobox enter craft-build" >&2
        echo "  - Debian/Ubuntu: sudo apt-get install git cmake ninja-build python3 python3-venv curl pkg-config bison flex file binutils" >&2
        echo "  - Arch Linux:    sudo pacman -S git cmake ninja python curl pkgconf bison flex file binutils" >&2
        echo "  - Fedora:        sudo dnf install git cmake ninja-build python3 curl pkgconf-pkg-config bison flex file binutils" >&2
    fi

    if [ "${#MISSING_ANDROID[@]}" -gt 0 ]; then
        echo "" >&2
        echo "Missing Android NDK/SDK components: ${MISSING_ANDROID[*]}" >&2
        echo "Install NDK r30 + SDK platforms;android-35 + build-tools;35.0.1:" >&2
        echo "  1. Preview install steps:" >&2
        echo "     bash android/scripts/setup_ndk_sdk.sh --dry-run" >&2
        echo "  2. Install into ~/Android (requires curl, unzip, and Java for sdkmanager):" >&2
        echo "     bash android/scripts/setup_ndk_sdk.sh --target \"\$HOME/Android\" --accept-licenses" >&2
        echo "  3. Export paths and re-run preflight:" >&2
        echo "     export NDK=\"\$HOME/Android/android-ndk-r30\"" >&2
        echo "     export SDK=\"\$HOME/Android/sdk\"" >&2
        echo "     bash android/scripts/build_dependencies.sh --preflight" >&2
    fi
}

run_preflight() {
    validate_lock_and_patches
    check_host_tools
    check_ndk_and_sdk

    if [ "${#MISSING_TOOLS[@]}" -gt 0 ] || [ "${#MISSING_ANDROID[@]}" -gt 0 ]; then
        print_remediation
        echo "" >&2
        echo "Preflight FAILED." >&2
        return 2
    fi

    echo "[ok] Preflight PASSED (lockfile, patches, host tools, NDK r30, and SDK 35 ready)."
    return 0
}

if [ "$MODE" = "check-lock" ]; then
    validate_lock_and_patches
    exit $?
fi

if [ "$MODE" = "preflight" ]; then
    run_preflight
    exit $?
fi

# Full build mode preserves the existing WORK/NDK/SDK contract.
WORK=${WORK:?Set WORK to an external build directory}
if ! run_preflight; then
    exit $?
fi

PREFIX=$WORK/prefix
mkdir -p "$WORK/src" "$WORK/build" "$WORK/dist" "$PREFIX"

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

apply_series() {
    # <source dir> <patch dir> <prefix>: reset the checkout, then check and apply each patch in order.
    local path=$1 dir=$2 prefix=$3 patch name
    local had_nullglob=0
    shopt -q nullglob && had_nullglob=1
    shopt -s nullglob
    local patches=("$dir"/"$prefix"-*.patch)
    [ "$had_nullglob" -eq 0 ] && shopt -u nullglob
    [ "${#patches[@]}" -gt 0 ] && [ -f "${patches[0]}" ] || {
        echo "no $prefix-*.patch files in $dir" >&2
        exit 3
    }
    git -C "$path" reset -q --hard HEAD
    git -C "$path" clean -qfd
    for patch in "${patches[@]}"; do
        name=$(basename "$patch")
        if ! git -C "$path" apply --check --whitespace=nowarn "$patch"; then
            echo "PATCH MISMATCH: $name does not apply to $path at $(git -C "$path" rev-parse HEAD)" >&2
            exit 3
        fi
        git -C "$path" apply --whitespace=nowarn "$patch"
        echo "   applied $name"
    done
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
echo '== Apply the FEX patch series (android/patches/fex)'
apply_series "$WORK/src/fex" "$PATCHES/fex" fex
cmake -S "$WORK/src/fex" -B "$WORK/build/fex" -G Ninja \
    -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI="$ABI" -DANDROID_PLATFORM="$API" -DANDROID_STL=c++_shared \
    -DCMAKE_BUILD_TYPE=Release -DBUILD_FEXCONFIG=OFF -DBUILD_TESTING=OFF \
    -DBUILD_THUNKS=OFF -DENABLE_JEMALLOC_GLIBC_ALLOC=OFF -DENABLE_LTO=OFF \
    -DENABLE_OFFLINE_TELEMETRY=OFF -DENABLE_CCACHE=OFF -DTUNE_CPU=cortex-a78 \
    -DBUILD_FEXCORE_ONLY=ON \
    -DFEXCORE_SMOKE_SOURCE="$ROOT/android/synthetic/fexcore_smoke_entry.cpp"
cmake --build "$WORK/build/fex" --parallel "$JOBS" --target FEXCore Common CommonTools JemallocLibs rpmalloc fexcore-smoke
cmake --install "$WORK/build/fex" --prefix "$WORK/stage-fex"
install -D "$WORK/stage-fex/bin/fexcore-smoke" "$WORK/dist/uncharted-android-fexcore-smoke"

echo '== Build Mesa Turnip for Android KGSL'
fetch_commit mesa-turnip "$MESA_URL" "$MESA_REV"
echo '== Apply the Mesa Turnip KGSL patch series (android/patches/mesa)'
apply_series "$WORK/src/mesa-turnip" "$PATCHES/mesa" mesa
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
rm -rf "$WORK/build/mesa"
PKG_CONFIG_LIBDIR=/dev/null PKG_CONFIG_PATH="" PATH="$WORK/mesa-venv/bin:$PATH" "$WORK/mesa-venv/bin/meson" setup "$WORK/build/mesa" "$WORK/src/mesa-turnip" \
    --cross-file "$WORK/build/mesa-android.ini" --prefix "$PREFIX" \
    -Dbuildtype=release -Dplatforms=android -Dplatform-sdk-version="$API" \
    -Dandroid-stub=true -Dgallium-drivers= -Degl=disabled -Dopengl=false \
    -Dgles1=disabled -Dgles2=disabled -Dglx=disabled -Dvulkan-drivers=freedreno \
    -Dfreedreno-kmds=kgsl -Dvulkan-beta=true -Db_lto=false -Dstrip=true \
    -Dllvm=disabled -Dzstd=disabled -Dvalgrind=disabled -Dandroid-libbacktrace=disabled
PATH="$WORK/mesa-venv/bin:$PATH" ninja -C "$WORK/build/mesa" -j"$JOBS" src/freedreno/vulkan/libvulkan_freedreno.so
install -D "$WORK/build/mesa/src/freedreno/vulkan/libvulkan_freedreno.so" \
    "$WORK/dist/libvulkan_freedreno.so"

echo '== Build libadrenotools and SDL 3'
fetch_commit libadrenotools "$ADRENO_URL" "$ADRENO_REV"
git -C "$WORK/src/libadrenotools" submodule update --init --depth 1
cmake_android adrenotools "$WORK/src/libadrenotools"
fetch_commit sdl3 "$SDL_URL" "$SDL_REV"
cmake_android sdl3 "$WORK/src/sdl3" -DSDL_SHARED=ON -DSDL_STATIC=OFF \
    -DSDL_TEST_LIBRARY=OFF -DSDL_TESTS=OFF -DSDL_EXAMPLES=OFF

echo '== Link the authored synthetic Android smoke binaries'
"$TC/aarch64-linux-android$API-clang" -fPIE -pie -O2 \
    -I"$PREFIX/include" "$ROOT/android/synthetic/smoke.c" \
    -L"$PREFIX/lib" -Wl,-rpath,'$ORIGIN' -lSDL3 \
    -o "$WORK/dist/uncharted-android-smoke"
cmake -S "$ROOT/android/synthetic" -B "$WORK/build/synthetic" -G Ninja \
    -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI="$ABI" -DANDROID_PLATFORM="android-$API" \
    -DANDROID_STL=c++_shared -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON
cmake --build "$WORK/build/synthetic" --parallel "$JOBS"
install -D "$WORK/build/synthetic/uncharted-android-fex-smoke" "$WORK/dist/uncharted-android-fex-smoke"
install -D "$WORK/build/synthetic/test_android_fex_smoke" "$WORK/dist/test_android_fex_smoke"
cp "$PREFIX/lib/libSDL3.so" "$WORK/dist/"
cp "$NDK/toolchains/llvm/prebuilt/$HOST/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" "$WORK/dist/"

for bin in uncharted-android-smoke uncharted-android-fex-smoke test_android_fex_smoke uncharted-android-fexcore-smoke; do
    file "$WORK/dist/$bin"
    readelf -h "$WORK/dist/$bin" | grep -E 'Class:|Machine:'
    readelf -l "$WORK/dist/$bin" | grep -q '/system/bin/linker64' || {
        echo "$bin is missing Android bionic /system/bin/linker64 interpreter" >&2; exit 1
    }
    if readelf -d "$WORK/dist/$bin" | grep -q 'libc\.so\.6'; then
        echo "$bin unexpectedly links glibc libc.so.6" >&2; exit 1
    fi
done

if command -v adb >/dev/null 2>&1 && adb get-state >/dev/null 2>&1; then
    echo '== Execute synthetic smoke binaries on the attached Android device'
    adb shell mkdir -p /data/local/tmp/uncharted-android-smoke
    adb push "$WORK/dist/uncharted-android-smoke" \
        "$WORK/dist/uncharted-android-fex-smoke" \
        "$WORK/dist/test_android_fex_smoke" \
        "$WORK/dist/uncharted-android-fexcore-smoke" \
        "$WORK/dist/libSDL3.so" \
        "$WORK/dist/libc++_shared.so" \
        /data/local/tmp/uncharted-android-smoke/ >/dev/null
    adb shell 'cd /data/local/tmp/uncharted-android-smoke && chmod 755 uncharted-android-smoke uncharted-android-fex-smoke test_android_fex_smoke uncharted-android-fexcore-smoke && LD_LIBRARY_PATH=. ./uncharted-android-smoke && LD_LIBRARY_PATH=. ./test_android_fex_smoke && LD_LIBRARY_PATH=. ./uncharted-android-fex-smoke && LD_LIBRARY_PATH=. ./uncharted-android-fexcore-smoke'
else
    echo 'No attached adb device; Android execution was not performed.' >&2
fi
echo "Artifacts: $WORK/dist"
