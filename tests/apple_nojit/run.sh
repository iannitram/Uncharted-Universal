#!/bin/sh
# Build bbcpu (linked, never vendored or edited) and the synthetic harness, then run it.
#   run.sh host [report.json] [--require-supported]   macOS arm64, interpreter forced (BB_JIT=0)
#   run.sh sim  [report.json] [--require-supported]   iOS Simulator arm64, no-JIT build, via simctl spawn
# bbcpu comes from $BBCPU_DIR if set, else is cloned at the pinned revision into a cache under
# $TMPDIR (outside any checkout). Zydis v4.1.1 (MIT) is cloned there too; both builds use our own
# compile lines, so bbcpu's build.sh is not needed.
set -eu
REV=3f9b6c1a11d5a443ee2e556a0d1f3aec4a73f0ca
ZYDIS=v4.1.1
mode=${1:-host}; shift 2>/dev/null || true
out=${1:-nojit-report.json}; shift 2>/dev/null || true
d=$(cd "$(dirname "$0")" && pwd)
out=$(cd "$(dirname "$out")" && pwd)/$(basename "$out")
cache=${TMPDIR:-/tmp}/nojit-cache
mkdir -p "$cache"

if [ -n "${BBCPU_DIR:-}" ]; then bb=$BBCPU_DIR; else
  bb=$cache/bbcpu
  if [ ! -d "$bb/.git" ]; then
    git clone -q https://github.com/bloodborne-native/bbcpu.git "$bb"
  fi
  git -C "$bb" -c advice.detachedHead=false checkout -q "$REV"
fi
got=$(git -C "$bb" rev-parse HEAD)
[ "$got" = "$REV" ] || { echo "run.sh: bbcpu is at $got, need $REV" >&2; exit 1; }

zy=$cache/zydis
if [ ! -d "$zy/.git" ]; then
  git -c advice.detachedHead=false clone -q --recursive --depth 1 -b "$ZYDIS" \
    https://github.com/zyantific/zydis.git "$zy"
fi

case $mode in
  host) sdk=macosx; tgt=arm64-apple-macos14.0; tname=arm64-apple-darwin ;;
  sim)  sdk=iphonesimulator; tgt=arm64-apple-ios17.0-simulator; tname=$tgt ;;
  *) echo "usage: run.sh host|sim [report] [--require-supported]" >&2; exit 2 ;;
esac
sysroot=$(xcrun --sdk $sdk --show-sdk-path)
w=$cache/build-$mode
rm -rf "$w"; mkdir -p "$w"
CC="xcrun --sdk $sdk clang -target $tgt -isysroot $sysroot"
inc="-I$zy/include -I$zy/dependencies/zycore/include -I$zy/src"
zdef="-DZYDIS_STATIC_BUILD -DZYCORE_STATIC_BUILD"

# Compile jobs one at a time (8 GB machine).
n=0
for s in $(find "$zy/src" "$zy/dependencies/zycore/src" -name '*.c' | sort); do
  n=$((n + 1))
  $CC -std=c11 -O2 $inc $zdef -c "$s" -o "$w/z${n}_$(basename "$s").o"
done
for s in "$bb"/src/cpu/*.c "$bb"/src/cpu/hostcall.S; do
  $CC -std=gnu11 -O2 -Wno-unused-parameter $inc $zdef -c "$s" -o "$w/b_$(basename "$s").o"
done
$CC -std=c11 -O2 -Wall -Wextra -Werror -Wno-missing-field-initializers \
  -I"$bb/src" -DBBCPU_REVISION="\"$REV\"" -DNOJIT_TARGET="\"$tname\"" \
  "$d/test_nojit.c" "$d/backend_bbcpu.c" "$w"/*.o -o "$w/nojit_test"

case $mode in
  host) BB_JIT=0 "$w/nojit_test" "$out" "$@" ;;
  sim)
    dev=$(xcrun simctl list devices booted | sed -n 's/.*(\([0-9A-F-]\{36\}\)) (Booted).*/\1/p' | head -1)
    booted_here=
    if [ -z "$dev" ]; then
      dev=$(xcrun simctl list devices available | grep 'iPhone' | sed -n 's/.*(\([0-9A-F-]\{36\}\)).*/\1/p' | head -1)
      xcrun simctl boot "$dev"; booted_here=1
    fi
    rc=0
    # The simulator process writes the report inside its own data area; print it to stdout instead.
    xcrun simctl spawn "$dev" "$w/nojit_test" "$@" > "$out" || rc=$?
    [ -z "$booted_here" ] || xcrun simctl shutdown "$dev"
    exit $rc ;;
esac
