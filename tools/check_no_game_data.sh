#!/usr/bin/env bash
# Fails when a folder, an .app, an .ipa or a .zip holds what looks like game data (decision #23:
# no game files in the repo, a package, a commit or CI). Usage:
#
#     bash tools/check_no_game_data.sh <directory | .app | .ipa | .zip>...
#
# Only generic patterns, nothing taken from a dump: PlayStation 4 package and executable
# containers by their format magic (SELF, PKG), names that only game dumps carry (.self, .prx,
# .sprx, .elf, eboot.bin, param.sfo, sce_sys, title-id folders such as CUSA12345, .pkg, .at9),
# and size (a file over MAX_FILE_BYTES, default 64 MiB; the whole tree over MAX_TOTAL_BYTES,
# default 512 MiB). tests/ and tests/fixtures are not exempt: pass only what you ship.
set -euo pipefail
max_file=${MAX_FILE_BYTES:-67108864}
max_total=${MAX_TOTAL_BYTES:-536870912}
(($# > 0)) || { echo "usage: check_no_game_data.sh <directory | .app | .ipa | .zip>..." >&2; exit 2; }
# A missing tool must never read as a clean tree (a failed find inside <(...) would).
for tool in find od wc tr basename mktemp; do
    command -v "$tool" >/dev/null || { echo "game data check: needs $tool" >&2; exit 2; }
done
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
failures=0
fail() { echo "game data check: $1" >&2; failures=$((failures + 1)); }

scan() { # <directory> <label>
    local dir=$1 label=$2 total=0 file size magic name
    while IFS= read -r -d '' file; do
        name=$(basename -- "$file")
        rel=${file#"$dir"/}
        shopt -s nocasematch
        case $name in
            *.self|*.prx|*.sprx|*.elf|*.pkg|*.at9|eboot.bin|param.sfo) fail "$label: $rel has a game-package file name" ;;
        esac
        shopt -u nocasematch
        size=$(wc -c < "$file" | tr -d ' ')
        total=$((total + size))
        ((size <= max_file)) || fail "$label: $rel is $size bytes (limit $max_file)"
        if ((size >= 4)); then
            magic=$(od -An -tx1 -N4 "$file" | tr -d ' \n')
            case $magic in
                4f153d1d) fail "$label: $rel starts with the PS4 SELF magic" ;;
                7f434e54) fail "$label: $rel starts with the PS4 PKG magic" ;;
            esac
        fi
    done < <(find "$dir" -type f -print0)
    # Folder names: a title id (CUSA + digits) or the sce_sys metadata folder anywhere in the tree.
    while IFS= read -r -d '' file; do
        fail "$label: ${file#"$dir"/} is a game folder name"
    done < <(find "$dir" -type d \( -iname 'CUSA[0-9]*' -o -iname 'sce_sys' -o -iname 'sce_module' \) -print0)
    ((total <= max_total)) || fail "$label: $total bytes in all (limit $max_total)"
}

# unzip where there is one (macOS, most Linux); Python's zipfile otherwise (a minimal chroot).
zip_names() {
    if command -v unzip >/dev/null; then unzip -Z1 -- "$1"
    else python3 -c 'import sys, zipfile; print("\n".join(zipfile.ZipFile(sys.argv[1]).namelist()))' "$1"; fi
}
zip_extract() {
    if command -v unzip >/dev/null; then unzip -q -- "$1" -d "$2"; else python3 -m zipfile -e "$1" "$2"; fi
}

for target in "$@"; do
    if [[ -d $target ]]; then
        scan "$target" "$target"
    elif [[ -f $target ]]; then
        case $target in
            *.zip|*.ipa)
                # Names first (cheap), then unpacked for the magic and size checks.
                while IFS= read -r name; do
                    if [[ $name =~ (^|/)(CUSA[0-9]+|sce_sys|sce_module)(/|$) ]]; then fail "$target: $name is a game folder name"; fi
                done < <(zip_names "$target")
                dest=$work/$(basename -- "$target").d
                mkdir -p "$dest"
                zip_extract "$target" "$dest"
                scan "$dest" "$target"
                ;;
            *) fail "$target: not a directory, .app, .ipa or .zip" ;;
        esac
    else
        fail "$target: no such file or directory"
    fi
done
((failures == 0)) || exit 1
echo "game data check: clean ($*)"
