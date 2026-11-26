# Sourced by the Windows build/run scripts: puts the MSYS2 CLANG64 toolchain first on PATH.
# MSYS2_ROOT overrides the default C:/msys64.
msys2_root=${MSYS2_ROOT:-/c/msys64}
if [[ -d $msys2_root/clang64/bin ]]; then
    export PATH="$msys2_root/clang64/bin:$msys2_root/usr/bin:$PATH"
    export MSYSTEM=CLANG64
fi
# MSYS2's bash.exe started from another shell (not a login shell) gets no TMP or TEMP, and
# Windows tools then put their temp files in C:\WINDOWS, where a user cannot write: MSVC
# link.exe under cargo fails with LNK1104, std::filesystem::temp_directory_path() tests with
# "Access is denied" (#630). /tmp is the user's temp folder there; a TMP or TEMP already set wins.
if [[ -z ${TMP:-} || -z ${TEMP:-} ]] && command -v cygpath >/dev/null; then
    msys2_tmp=${TEMP:-${TMP:-$(cygpath -w /tmp)}}
    if [[ -n $msys2_tmp ]]; then
        export TMP="$msys2_tmp" TEMP="$msys2_tmp"
    fi
    unset msys2_tmp
fi
