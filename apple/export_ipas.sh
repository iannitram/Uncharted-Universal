#!/usr/bin/env bash
# LOCAL ONLY. Copy the finished .ipa files to the owner's Dropbox transfer folder so SideStore on the
# iPhone can fetch them. Never run in CI (it refuses when CI or GITHUB_ACTIONS is set) and it is not run
# by the build: the owner runs it.
#
#   apple/export_ipas.sh <folder holding the Uncharted-Universal-U*-<version>.ipa files>
#
# The destination folder is created here, only when this script runs.
set -euo pipefail
fail() { echo "error: $*" >&2; exit 1; }
[ -z "${CI:-}" ] && [ -z "${GITHUB_ACTIONS:-}" ] || fail "export_ipas.sh is local only and must not run in CI"
[ "$#" -eq 1 ] && [ -d "$1" ] || { echo "usage: apple/export_ipas.sh <ipa folder>" >&2; exit 2; }
dest=${UU_IOS_EXPORT:-$HOME/Dropbox/_transfers/Uncharted-Universal/ios}
shopt -s nullglob
files=("$1"/Uncharted-Universal-U[123]-*.ipa)
[ "${#files[@]}" -gt 0 ] || fail "no Uncharted-Universal-U1/U2/U3-<version>.ipa files in $1"
mkdir -p "$dest"
for file in "${files[@]}"; do
    cp -f "$file" "$dest/.$(basename "$file").partial"
    mv -f "$dest/.$(basename "$file").partial" "$dest/$(basename "$file")"
    echo "copied $(basename "$file") to $dest"
done
