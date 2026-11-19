#!/bin/sh
# Installs (or updates) Uncharted: The Nathan Drake Collection for Linux/Steam Deck:
#   curl -fsSL https://raw.githubusercontent.com/spacepepperoni/uncharted_Linux/main/install.sh | sh
# The latest release's AppImage goes to ~/Applications, with an application menu entry and a
# desktop icon. Saves and settings live in ~/.local/share/uncharted-linux and are never touched.
# `sh install.sh --uninstall` removes the AppImage and the menu entry (saves are kept).
set -eu

repo=spacepepperoni/uncharted_Linux
apps="$HOME/Applications"
appimage="$apps/Uncharted-x86_64.AppImage"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
entry="$data/applications/uncharted-linux.desktop"
icon="$data/icons/hicolor/256x256/apps/uncharted-linux.png"
desktop=$(xdg-user-dir DESKTOP 2>/dev/null || echo "$HOME/Desktop")

if [ "${1:-}" = --uninstall ]; then
    rm -f "$appimage" "$entry" "$icon" "$desktop/uncharted-linux.desktop"
    echo "Uncharted removed. Saves and settings are still in $data/uncharted-linux."
    exit 0
fi

[ "$(uname -m)" = x86_64 ] || { echo 'Uncharted needs a 64-bit x86 PC.' >&2; exit 1; }
# The AppImage needs glibc 2.38 or newer (see packaging/linux/AppRun).
glibc=$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}')
if [ -n "$glibc" ] && [ "$(printf '%s\n2.38\n' "$glibc" | sort -V | head -1)" != 2.38 ]; then
    echo "Uncharted needs a newer Linux system (glibc 2.38 or later; this one has $glibc)." >&2
    echo 'Ubuntu 24.04, Linux Mint 22, Debian 13, Fedora 39 and newer releases work.' >&2
    exit 1
fi
command -v curl >/dev/null || { echo 'Install curl first.' >&2; exit 1; }

echo 'Finding the latest release...'
url=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" |
      grep -o '"browser_download_url": *"[^"]*Uncharted-x86_64\.AppImage"' | head -1 | sed 's/.*"\(http[^"]*\)"/\1/')
[ -n "$url" ] || { echo "No release found at https://github.com/$repo/releases" >&2; exit 1; }

mkdir -p "$apps" "$(dirname "$entry")" "$(dirname "$icon")"
echo "Downloading $url"
curl -fL --progress-bar -o "$appimage.part" "$url"
chmod +x "$appimage.part"
mv -f "$appimage.part" "$appimage"
curl -fsSL -o "$icon" "https://raw.githubusercontent.com/$repo/main/crates/core/data/icons/uncharted.png" || true

cat > "$entry" <<EOF
[Desktop Entry]
Type=Application
Name=Uncharted
Comment=Uncharted: The Nathan Drake Collection for Linux
Exec="$appimage"
Icon=$icon
Terminal=false
Categories=Game;
Actions=report;

[Desktop Action report]
Name=Create bug report
Exec="$appimage" --report
EOF
chmod +x "$entry"
if [ -d "$desktop" ]; then
    cp "$entry" "$desktop/uncharted-linux.desktop"
    chmod +x "$desktop/uncharted-linux.desktop"
fi
command -v update-desktop-database >/dev/null && update-desktop-database "$data/applications" 2>/dev/null || true

echo
echo "Installed: $appimage"
echo 'Open Uncharted from the application menu or the desktop icon, choose your game folder,'
if grep -q '^ID=steamos' /etc/os-release 2>/dev/null; then
    echo 'then press Add to Steam. Back in Gaming Mode, the games are in your library under Non-Steam.'
else
    echo 'then press Play (or Add to Steam to play from your Steam library).'
fi
