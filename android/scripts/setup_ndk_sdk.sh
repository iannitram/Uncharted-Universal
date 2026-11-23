#!/usr/bin/env bash
# Idempotent bootstrap helper for Android NDK r30 + SDK platform 35 + build-tools 35.0.1
# on Linux x86_64. Supports --dry-run to preview actions without modifying disk.
set -euo pipefail

HERE=$(cd -- "$(dirname -- "$0")/.." && pwd)
TARGET_DIR=${ANDROID_DIR:-$HOME/Android}
NDK_DIR=""
SDK_DIR=""
DRY_RUN=0
ACCEPT_LICENSES=${ANDROID_ACCEPT_LICENSES:-0}
SKIP_NDK=0
SKIP_SDK=0

NDK_URL=${NDK_URL:-https://dl.google.com/android/repository/android-ndk-r30-linux.zip}
CMDLINE_TOOLS_URL=${CMDLINE_TOOLS_URL:-https://dl.google.com/android/repository/commandlinetools-linux-12700392_latest.zip}
SDK_PLATFORM_PKG=${SDK_PLATFORM_PKG:-platforms;android-35}
SDK_BUILD_TOOLS_PKG=${SDK_BUILD_TOOLS_PKG:-build-tools;35.0.1}

usage() {
    cat <<'EOF'
Usage: android/scripts/setup_ndk_sdk.sh [OPTIONS]

Idempotently install Android NDK r30, SDK platform 35 (platforms;android-35),
and SDK build-tools 35.0.1 (build-tools;35.0.1) on Linux x86_64.

Options:
  --target, --dir <path>   Base Android directory (default: $HOME/Android)
  --ndk-dir <path>         NDK r30 destination (default: <target>/android-ndk-r30)
  --sdk-dir <path>         Android SDK root (default: <target>/sdk)
  --dry-run                Print planned checks, URLs, and commands without downloading
                           or modifying the filesystem
  --accept-licenses, -y    Accept Android SDK licenses non-interactively
  --skip-ndk               Skip Android NDK r30 installation
  --skip-sdk               Skip Android SDK platform 35 / build-tools 35.0.1 installation
  -h, --help               Show this help message and exit

Environment overrides:
  ANDROID_DIR              Default base directory if --target is omitted
  ANDROID_ACCEPT_LICENSES  Set to 1 to accept SDK licenses non-interactively
  NDK_URL                  Override NDK r30 zip download URL
  CMDLINE_TOOLS_URL        Override Android commandlinetools zip download URL
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --target|--dir)
            [ "$#" -ge 2 ] || { echo "Missing argument for $1" >&2; exit 2; }
            TARGET_DIR=$2
            shift
            ;;
        --ndk-dir)
            [ "$#" -ge 2 ] || { echo "Missing argument for $1" >&2; exit 2; }
            NDK_DIR=$2
            shift
            ;;
        --sdk-dir)
            [ "$#" -ge 2 ] || { echo "Missing argument for $1" >&2; exit 2; }
            SDK_DIR=$2
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            ;;
        --accept-licenses|--yes|-y)
            ACCEPT_LICENSES=1
            ;;
        --skip-ndk)
            SKIP_NDK=1
            ;;
        --skip-sdk)
            SKIP_SDK=1
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

if [ -z "$NDK_DIR" ]; then
    NDK_DIR="$TARGET_DIR/android-ndk-r30"
fi
if [ -z "$SDK_DIR" ]; then
    if [ -d "$TARGET_DIR/Sdk" ] && [ ! -d "$TARGET_DIR/sdk" ]; then
        SDK_DIR="$TARGET_DIR/Sdk"
    else
        SDK_DIR="$TARGET_DIR/sdk"
    fi
fi

TMP_DIR=""
cleanup() {
    if [ -n "$TMP_DIR" ] && [ -d "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}
trap cleanup EXIT

uname_s=$(uname -s)
uname_m=$(uname -m)
if [ "$uname_s" != "Linux" ] || [ "$uname_m" != "x86_64" ]; then
    echo "[WARN] Host is $uname_s $uname_m; NDK/SDK packages target Linux x86_64." >&2
    if [ "$DRY_RUN" -eq 0 ]; then
        echo "Refusing non-Linux-x86_64 install without --dry-run." >&2
        exit 2
    fi
fi

is_ndk_r30_installed() {
    [ -f "$NDK_DIR/source.properties" ] &&
        grep -Eq 'Pkg\.Revision = 30\.' "$NDK_DIR/source.properties" &&
        [ -f "$NDK_DIR/build/cmake/android.toolchain.cmake" ] &&
        [ -x "$NDK_DIR/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android33-clang" ]
}

is_sdk_installed() {
    [ -f "$SDK_DIR/platforms/android-35/android.jar" ] &&
        [ -x "$SDK_DIR/build-tools/35.0.1/aapt2" ]
}

find_sdkmanager() {
    if [ -x "$SDK_DIR/cmdline-tools/latest/bin/sdkmanager" ]; then
        echo "$SDK_DIR/cmdline-tools/latest/bin/sdkmanager"
    elif [ -x "$SDK_DIR/cmdline-tools/bin/sdkmanager" ]; then
        echo "$SDK_DIR/cmdline-tools/bin/sdkmanager"
    elif command -v sdkmanager >/dev/null 2>&1; then
        command -v sdkmanager
    fi
}

require_tool() {
    local tool=$1 purpose=$2
    if command -v "$tool" >/dev/null 2>&1; then
        echo "[ok] tool $tool available ($(command -v "$tool"))"
        return 0
    fi
    if [ "$DRY_RUN" -eq 1 ]; then
        echo "[note] missing '$tool' (needed for $purpose when running without --dry-run)"
        return 0
    fi
    echo "Missing required tool '$tool' (needed for $purpose)." >&2
    return 1
}

echo "== Android NDK r30 + SDK 35 setup plan"
echo "   target dir : $TARGET_DIR"
echo "   ndk dir    : $NDK_DIR"
echo "   sdk dir    : $SDK_DIR"
echo "   mode       : $([ "$DRY_RUN" -eq 1 ] && echo 'dry-run (no filesystem or network changes)' || echo 'install')"

ensure_tmp_dir() {
    if [ -z "$TMP_DIR" ]; then
        TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/uncharted-android-setup.XXXXXX")
    fi
}

if [ "$SKIP_NDK" -eq 0 ]; then
    echo ""
    echo "== Step 1: Android NDK r30"
    if is_ndk_r30_installed; then
        echo "[ok] Android NDK r30 already installed at $NDK_DIR (skipping)"
    else
        require_tool curl "downloading NDK r30"
        require_tool unzip "extracting NDK r30"
        if [ "$DRY_RUN" -eq 1 ]; then
            echo "[dry-run] mkdir -p \"$TARGET_DIR\""
            echo "[dry-run] curl -fsSL \"$NDK_URL\" -o \"<tmp>/android-ndk-r30-linux.zip\""
            echo "[dry-run] unzip -q \"<tmp>/android-ndk-r30-linux.zip\" -d \"<tmp>/ndk-extract\""
            echo "[dry-run] install extracted NDK r30 atomically into \"$NDK_DIR\""
        else
            ensure_tmp_dir
            mkdir -p "$(dirname "$NDK_DIR")"
            echo "Downloading Android NDK r30 from $NDK_URL ..."
            curl -fL --retry 3 --retry-delay 2 "$NDK_URL" -o "$TMP_DIR/android-ndk-r30-linux.zip"
            mkdir -p "$TMP_DIR/ndk-extract"
            unzip -q "$TMP_DIR/android-ndk-r30-linux.zip" -d "$TMP_DIR/ndk-extract"
            extracted_ndk=""
            if [ -f "$TMP_DIR/ndk-extract/source.properties" ]; then
                extracted_ndk="$TMP_DIR/ndk-extract"
            else
                for cand in "$TMP_DIR/ndk-extract"/android-ndk-r30*; do
                    if [ -f "$cand/source.properties" ]; then
                        extracted_ndk="$cand"
                        break
                    fi
                done
            fi
            [ -n "$extracted_ndk" ] || {
                echo "Extracted NDK archive did not contain source.properties" >&2
                exit 2
            }
            rm -rf "$NDK_DIR"
            mv "$extracted_ndk" "$NDK_DIR"
            is_ndk_r30_installed || {
                echo "Installed NDK at $NDK_DIR failed r30 verification" >&2
                exit 2
            }
            echo "[ok] Installed Android NDK r30 at $NDK_DIR"
        fi
    fi
fi

if [ "$SKIP_SDK" -eq 0 ]; then
    echo ""
    echo "== Step 2: Android SDK ($SDK_PLATFORM_PKG, $SDK_BUILD_TOOLS_PKG)"
    if is_sdk_installed; then
        echo "[ok] Android SDK $SDK_PLATFORM_PKG and $SDK_BUILD_TOOLS_PKG already installed at $SDK_DIR (skipping)"
    else
        SDKMANAGER=$(find_sdkmanager || true)
        require_tool java "running Android sdkmanager (JDK/JRE 17+)"
        if [ -z "$SDKMANAGER" ]; then
            require_tool curl "downloading Android command-line tools"
            require_tool unzip "extracting Android command-line tools"
            if [ "$DRY_RUN" -eq 1 ]; then
                echo "[dry-run] mkdir -p \"$SDK_DIR/cmdline-tools\""
                echo "[dry-run] curl -fsSL \"$CMDLINE_TOOLS_URL\" -o \"<tmp>/commandlinetools-linux.zip\""
                echo "[dry-run] unzip -q \"<tmp>/commandlinetools-linux.zip\" -d \"<tmp>/cmdline-extract\""
                echo "[dry-run] install sdkmanager into \"$SDK_DIR/cmdline-tools/latest\""
                SDKMANAGER="$SDK_DIR/cmdline-tools/latest/bin/sdkmanager"
            else
                ensure_tmp_dir
                mkdir -p "$SDK_DIR/cmdline-tools"
                echo "Downloading Android command-line tools from $CMDLINE_TOOLS_URL ..."
                curl -fL --retry 3 --retry-delay 2 "$CMDLINE_TOOLS_URL" -o "$TMP_DIR/commandlinetools-linux.zip"
                mkdir -p "$TMP_DIR/cmdline-extract"
                unzip -q "$TMP_DIR/commandlinetools-linux.zip" -d "$TMP_DIR/cmdline-extract"
                [ -d "$TMP_DIR/cmdline-extract/cmdline-tools" ] || {
                    echo "Unexpected commandlinetools archive layout" >&2
                    exit 2
                }
                rm -rf "$SDK_DIR/cmdline-tools/latest"
                mv "$TMP_DIR/cmdline-extract/cmdline-tools" "$SDK_DIR/cmdline-tools/latest"
                SDKMANAGER="$SDK_DIR/cmdline-tools/latest/bin/sdkmanager"
                [ -x "$SDKMANAGER" ] || {
                    echo "sdkmanager binary missing at $SDKMANAGER" >&2
                    exit 2
                }
                echo "[ok] Installed Android cmdline-tools at $SDK_DIR/cmdline-tools/latest"
            fi
        else
            echo "[ok] Using existing sdkmanager: $SDKMANAGER"
        fi

        if [ "$DRY_RUN" -eq 1 ]; then
            if [ "$ACCEPT_LICENSES" -eq 1 ]; then
                echo "[dry-run] yes | \"$SDKMANAGER\" --licenses --sdk_root=\"$SDK_DIR\""
            else
                echo "[dry-run] \"$SDKMANAGER\" --licenses --sdk_root=\"$SDK_DIR\" (pass --accept-licenses for non-interactive)"
            fi
            echo "[dry-run] \"$SDKMANAGER\" --sdk_root=\"$SDK_DIR\" \"platform-tools\" \"$SDK_PLATFORM_PKG\" \"$SDK_BUILD_TOOLS_PKG\""
        else
            if [ "$ACCEPT_LICENSES" -eq 1 ]; then
                set +o pipefail
                yes | "$SDKMANAGER" --licenses --sdk_root="$SDK_DIR" >/dev/null
                set -o pipefail
            elif [ -t 0 ]; then
                "$SDKMANAGER" --licenses --sdk_root="$SDK_DIR"
            else
                echo "Non-interactive shell without --accept-licenses; re-run with --accept-licenses to accept Android SDK licenses." >&2
                exit 2
            fi

            "$SDKMANAGER" --sdk_root="$SDK_DIR" "platform-tools" "$SDK_PLATFORM_PKG" "$SDK_BUILD_TOOLS_PKG"
            is_sdk_installed || {
                echo "SDK installation completed but verification failed at $SDK_DIR" >&2
                exit 2
            }
            echo "[ok] Installed $SDK_PLATFORM_PKG and $SDK_BUILD_TOOLS_PKG at $SDK_DIR"
        fi
    fi
fi

echo ""
echo "== Next steps"
echo "export NDK=\"$NDK_DIR\""
echo "export SDK=\"$SDK_DIR\""
echo "bash \"$HERE/scripts/build_dependencies.sh\" --preflight"
