# shellcheck shell=bash
# Sourced by packaging/linux/package.sh (and tests/test_linux_system_libraries.py): the libraries
# the package leaves to the player's system, and the guard on the Rust faces.
#
# Libraries every desktop and SteamOS have, or that must match the graphics driver, stay out
# (whole families: libX*, libdrm_*, libpulse*, ...). libva is bundled: the bundled ffmpeg (Debian 13)
# needs libva 2.21+ (vaMapBuffer2) and Ubuntu 24.04 / Mint 22 / Pop!_OS 24.04 have 2.20, so no game
# started there. The game decodes in software and never loads a VA driver.
system='^(ld-linux|libc\.|libm\.|libdl\.|libpthread|librt\.|libresolv|libutil\.|libgcc_s|libstdc\+\+|libvulkan|libX|libxcb|libxkb|libwayland|libdrm|libGL|libEGL|libgbm|libasound|libpulse|libpipewire|libdbus|libudev|libsystemd|libz\.|libzstd|libexpat|libelf|libLLVM|libffi|libcap\.|libgcrypt|libgpg-error|liblzma|libmount|libblkid|libglib|libgobject|libgio|libgmodule|libpcre2|libdecor|libxshmfence|libsensors|libfontconfig|libfreetype)'

# foreign_needed: reads `readelf -d <program>` and prints each library the program names itself
# (a NEEDED entry) that is not a system library. The Rust faces have no RUNPATH to bin/lib, so
# those must all come from the system. Only the program's own entries count: what a system
# library pulls in comes with it on the player's system (fontconfig's freetype brings libpng16,
# libbz2 and libbrotli on Debian 13, which `ldd` lists too), and is not the package's to carry.
foreign_needed() {
    local name
    sed -n 's/^.*(NEEDED).*\[\(.*\)\][[:space:]]*$/\1/p' | while read -r name; do
        [[ $name =~ $system ]] || echo "$name"
    done
}
