# shellcheck shell=bash
# Sourced by the package scripts (packaging/linux/package.sh, packaging/windows/package.sh,
# packaging/macos/bundle.sh) from the repository root: builds the Rust faces in release mode and
# sets `faces` (their program names) and `faces_dir` (the folder they are in). The three ship
# side by side: the app runs `uncharted-universal` beside itself, and `uncharted-universal play`
# runs `uncharted-run` beside itself (roadmap 2.10).
faces=(uncharted-app uncharted-universal uncharted-run)
# The Mac app is Apple silicon only (arm64): build that target explicitly so an Intel host
# cannot produce a different bundle. Elsewhere the host triple, as before.
cargo_target=()
target_subdir=
if [[ $(uname -s) == Darwin ]]; then
    cargo_target=(--target aarch64-apple-darwin)
    target_subdir=/aarch64-apple-darwin
fi
cargo build --release --locked --bins ${cargo_target[@]+"${cargo_target[@]}"} -p uncharted-app -p uncharted_cli -p uncharted_runtime
faces_dir=${CARGO_TARGET_DIR:-$PWD/target}
# Git Bash and MSYS2 may hold a Windows path (B:\...) in CARGO_TARGET_DIR.
if command -v cygpath >/dev/null 2>&1; then faces_dir=$(cygpath -u "$faces_dir"); fi
faces_dir=$faces_dir$target_subdir/release
exe_suffix=
case $(uname -s) in MINGW*|MSYS*|CYGWIN*) exe_suffix=.exe ;; esac
for face in "${faces[@]}"; do
    [[ -f $faces_dir/$face$exe_suffix ]] || { echo "Rust build gave no $faces_dir/$face$exe_suffix" >&2; exit 1; }
done
