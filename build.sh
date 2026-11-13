#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
mkdir -p out
# Windows: MSYS2 CLANG64 (packages in packaging/windows/README.md). The GPU library is static
# there and CMake links bb-probe.exe as well; clang PGO with pgo/merged.profdata, LTO off by default.
case $(uname -s) in
MINGW*|MSYS*)
    # shellcheck source=msys2-env.sh
    source ./msys2-env.sh
    # MSYS2's Vulkan headers, as on the GitHub runner: an installed Vulkan SDK (VULKAN_SDK) would
    # point CMake at its own, older headers, which lack extensions the port uses.
    unset VULKAN_SDK
    # cargo (rustup, with the x86_64-pc-windows-gnullvm target) builds crates/runtime, part of the
    # loader since piece 3.01 (gpu/CMakeLists.txt runs it).
    [[ -d $HOME/.cargo/bin ]] && PATH=$PATH:$HOME/.cargo/bin
    command -v cargo >/dev/null || { echo "Need cargo (rustup target add x86_64-pc-windows-gnullvm): crates/runtime is part of the loader." >&2; exit 1; }
    for tool in clang cmake ninja pkg-config; do
        command -v "$tool" >/dev/null || { echo "Need $tool from MSYS2 CLANG64 (packaging/windows/README.md)." >&2; exit 1; }
    done
    if [[ ! -f gpu/third_party/fsr-vulkan/CMakeLists.txt || ! -f gpu/third_party/imgui/imgui.h ||
          ! -f gpu/third_party/xbyak/xbyak/xbyak.h ]]; then
        git submodule update --init --recursive
    fi
    for patch in gpu/patches/fsr-vulkan/*.patch; do
        if ! git -C gpu/third_party/fsr-vulkan apply --ignore-whitespace --reverse --check "$PWD/$patch" 2>/dev/null; then
            git -C gpu/third_party/fsr-vulkan apply --ignore-whitespace "$PWD/$patch"
        fi
    done
    # perf7: BB_PGO=generate|use|off (use = pgo/merged.profdata, see gpu/CMakeLists.txt), BB_LTO=ON|OFF.
    # Default: the committed profile when it is there (release builds are PGO builds).
    pgo=${BB_PGO:-}
    if [[ -z $pgo ]]; then
        if [[ -f pgo/merged.profdata ]]; then pgo=use; else pgo=off; fi
    fi
    if [[ $pgo == use && ! -f pgo/merged.profdata ]]; then
        echo 'BB_PGO=use needs pgo/merged.profdata.' >&2; exit 1
    fi
    # libc++ stamps its version into the names of its internal helpers (abi tag "nqe230103" =
    # 23.1.3): with any other clang (the GitHub runner updates MSYS2) none of them matched the
    # profile, two thirds of it (0.3.6.41's release build). The profile gets this compiler's
    # stamp, in a copy under out/pgo; code that really changed in libc++ is still skipped.
    pgo_dir=$PWD/pgo
    if [[ $pgo == use ]]; then
        here=$(echo '#include <version>' | clang++ -x c++ -dM -E - | awk '/define _LIBCPP_VERSION /{print $3}')
        mkdir -p out/pgo
        llvm-profdata merge --text pgo/merged.profdata -o out/pgo/profile.proftext
        # The tag as written (three signature letters, the version), found and replaced as plain
        # text by awk: Git for Windows' grep and sed missed these patterns.
        tag=$(awk 'match($0, /B9[a-z][a-z][a-z][0-9][0-9][0-9][0-9][0-9][0-9]/) {
                       print substr($0, RSTART + 2, 9); exit }' out/pgo/profile.proftext)
        stamp=${tag:3}
        if [[ -n $here && -n $stamp && $here != "$stamp" ]]; then
            awk -v from="B9$tag" -v to="B9${tag:0:3}$here" '{
                     out = ""; while ((i = index($0, from)) > 0) {
                         out = out substr($0, 1, i - 1) to; $0 = substr($0, i + length(from)) }
                     print out $0 }' out/pgo/profile.proftext > out/pgo/stamped.proftext
            llvm-profdata merge out/pgo/stamped.proftext -o out/pgo/new.profdata
            if ! cmp -s out/pgo/new.profdata out/pgo/merged.profdata; then mv out/pgo/new.profdata out/pgo/merged.profdata; fi
            pgo_dir=$PWD/out/pgo
            echo "PGO profile: libc++ names from $stamp renamed for this compiler's $here"
        fi
        rm -f out/pgo/profile.proftext out/pgo/stamped.proftext out/pgo/new.profdata
    fi
    # perf7: BB_OPT=2|3: optimization level of the RelWithDebInfo build (CMake's default is -O2).
    opt="-O${BB_OPT:-2} -g -DNDEBUG"
    cmake -S gpu -B out/gpu -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo -DBB_PGO="$pgo" \
        -DBB_PGO_DIR="$pgo_dir" -DBB_LTO="${BB_LTO:-OFF}" -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ \
        -DCMAKE_C_FLAGS_RELWITHDEBINFO="$opt" -DCMAKE_CXX_FLAGS_RELWITHDEBINFO="$opt" >/dev/null
    echo "GPU library and loader: LTO ${BB_LTO:-OFF}, PGO $pgo, -O${BB_OPT:-2}"
    if ! ninja -C out/gpu bb-probe bb-gpu-capabilities > out/gpu-build.log 2>&1; then
        awk 'substr($0, 1, 1) != "["' out/gpu-build.log | tail -40 >&2 || true
        echo 'Build failed (full log: out/gpu-build.log)' >&2; exit 1
    fi
    cp out/gpu/bb-probe.exe out/gpu/bb-gpu-capabilities.exe out/
    if [[ $pgo == use ]]; then
        # How far the code has moved from the profile: clang's per-file counts of functions
        # changed since it was recorded (out of date) or added (no data). Only the files
        # compiled in this build: a full build (CI, a clean out/) gives the whole picture.
        profiled=$(llvm-profdata show pgo/merged.profdata 2>/dev/null | awk '/^Total functions:/ {print $3}')
        # BB_PGO_MAX_UNMATCHED=<percent> (the GitHub build): more than that unmatched fails the
        # build, so a release never ships mostly without its profile.
        awk -v profiled="${profiled:-0}" -v limit="${BB_PGO_MAX_UNMATCHED:-}" '
             /(hash mismatch|no profile data available for function)/ {
                 file=$0; sub(/^warning: /, "", file); sub(/: (no profile data|function control flow).*/, "", file)
                 sub(/^.*\/(gpu|src|tools|packaging)\//, "", file)
                 if ($0 ~ /hash mismatch/) stale++; else added++
                 sf[file]++
             }
             /^\[[0-9]+\/[0-9]+\] / { compiled++ }
             END {
                 if (!compiled) exit
                 printf "PGO profile: %d functions changed and %d are new since the profile, in the %d steps built now", stale, added, compiled
                 if (profiled) printf " (%.1f%% of the %d profiled)", 100*(stale+added)/profiled, profiled
                 printf "\n"
                 for (f in sf) printf "%d %s\n", sf[f], f | "sort -rn | head -5 | sed \"s/^/  /\""
                 close("sort -rn | head -5 | sed \"s/^/  /\"")
                 if (limit != "" && profiled && 100*(stale+added)/profiled > limit+0) {
                     printf "PGO profile: more than %s%% unmatched (BB_PGO_MAX_UNMATCHED): build stopped\n", limit
                     exit 3
                 }
             }' out/gpu-build.log || exit 1
    fi
    echo "Built $PWD/out/bb-probe.exe"
    if [[ ${1:-} == --test ]]; then
        tests=(motion-history-test motion-shader-test wbinvl1-test ui-composition-test upscaler-support-test expand-image-test)
        ninja_jobs=()
        if [[ -n ${BB_JOBS:-} ]]; then ninja_jobs=(-j"$BB_JOBS"); fi
        if ! ninja -C out/gpu "${ninja_jobs[@]}" "${tests[@]}" > out/gpu-tests-build.log 2>&1; then
            tail -40 out/gpu-tests-build.log >&2
            echo 'GPU unit test build failed (full log: out/gpu-tests-build.log)' >&2; exit 1
        fi
        for test in "${tests[@]}"; do "out/gpu/$test.exe"; done
        clang -std=c11 -O2 -g -Wall -Wextra -Werror -pthread -I. -Isrc tests/test_file_mods.c src/compat_win.c -lbcrypt -lws2_32 -lonecore -o out/file-mods-test.exe
        out/file-mods-test.exe
        clang -std=c11 -O2 -g -Wall -Wextra -Werror -Isrc tests/test_speakers.c -lm -o out/speakers-test.exe
        out/speakers-test.exe
        clang -std=c11 -O2 -g -Wall -Wextra -Werror -pthread -I. -Isrc tests/test_cpu_patch.c src/cpu_patch.c -lZydis -o out/cpu-patch-test.exe
        out/cpu-patch-test.exe
    fi
    exit 0
    ;;
esac
if [[ -z ${CC:-} ]]; then
    CC=$(command -v cc || command -v gcc || true)
    if [[ -z $CC ]]; then
        for candidate in /nix/store/*-gcc-wrapper-*/bin/gcc; do
            if [[ -x $candidate ]]; then CC=$candidate; break; fi
        done
    fi
fi
if [[ -z ${CC:-} ]]; then echo 'Install GCC/Clang or set CC.' >&2; exit 1; fi
# Dependencies come from pkg-config (Vulkan loader/headers, SDL3). On NixOS the
# environment is provided by shell.nix; re-enter it automatically if needed.
if ! { command -v pkg-config >/dev/null && pkg-config --exists vulkan sdl3 && command -v cmake >/dev/null && command -v ninja >/dev/null; }; then
    if [[ -z ${BB_IN_NIX_SHELL:-} ]] && command -v nix-shell >/dev/null; then
        exec env BB_IN_NIX_SHELL=1 nix-shell shell.nix --run "bash build.sh $*"
    fi
    echo 'Need pkg-config with vulkan and sdl3, cmake and ninja (see shell.nix).' >&2; exit 1
fi
read -r -a includes <<< "$(pkg-config --cflags vulkan sdl3)"
# Linux fork: newer Vulkan headers than some distributions ship (third_party/Vulkan-Headers).
[[ -f third_party/Vulkan-Headers/include/vulkan/vulkan.h ]] || git submodule update --init third_party/Vulkan-Headers
vulkan_headers=$PWD/third_party/Vulkan-Headers/include
includes=(-I"$vulkan_headers" "${includes[@]}")
# Linux fork: libavcodec/libavutil for runtime_ajm.c's MP3 decoder (Windows links them through CMake).
read -r -a libraries <<< "$(pkg-config --libs vulkan sdl3 libavcodec libavutil)"
# GPU library (shadPS4 video core + drivers), built by CMake into out/gpu/libbbgpu.so.
# BB_PGO: generate (instrumented build that writes pgo/ while the game runs), use, off.
# Default: use the profile in pgo/ when there is one. BB_LTO=OFF disables link-time optimization.
pgo=${BB_PGO:-}
if [[ -z $pgo ]]; then
    if [[ -n $(find pgo -name '*.gcda' -print -quit 2>/dev/null) ]]; then pgo=use; else pgo=off; fi
fi
mkdir -p pgo
# Submodules (git clone --recursive, or: git submodule update --init) and this port's changes
# to FSR-Vulkan (gpu/patches/fsr-vulkan), applied to its working tree once.
if [[ ! -f gpu/third_party/fsr-vulkan/CMakeLists.txt || ! -f gpu/third_party/imgui/imgui.h ]]; then
    git submodule update --init --recursive
fi
for patch in gpu/patches/fsr-vulkan/*.patch; do
    if ! git -C gpu/third_party/fsr-vulkan apply --ignore-whitespace --reverse --check "$PWD/$patch" 2>/dev/null; then
        git -C gpu/third_party/fsr-vulkan apply --ignore-whitespace "$PWD/$patch"
    fi
done
# crates/runtime (piece 3.01): the runtime modules ported to Rust (src/runtime.c is now its `abi`)
# and the benchmark mode, a static library linked into the loader. Its build script builds the
# GPU library first (CMake over gpu/ into out/gpu/libbbgpu.so, crates/runtime/build.rs) with the
# CMake arguments below, joined by the unit separator. A failed GPU build stops the build there:
# an older libbbgpu.so would otherwise be used silently. UNCHARTED_RUNTIME_LOADER=1 lets the
# Rust side call the C modules that remain.
command -v cargo >/dev/null || { echo 'Need cargo (rustup): crates/runtime is part of the loader.' >&2; exit 1; }
gpu_cmake=(-DCMAKE_BUILD_TYPE=RelWithDebInfo -DBB_PGO="$pgo" -DBB_LTO="${BB_LTO:-ON}" -DBB_PGO_DIR="$PWD/pgo"
    -DVulkan_INCLUDE_DIR="$vulkan_headers")
echo "GPU library: PGO $pgo, LTO ${BB_LTO:-ON}"
UNCHARTED_GPU_BUILD=$PWD/out/gpu UNCHARTED_GPU_CMAKE_ARGS=$(IFS=$'\x1f'; printf '%s' "${gpu_cmake[*]}") \
    UNCHARTED_GPU_LOG=$PWD/out/gpu-build.log UNCHARTED_RUNTIME_LOADER=1 \
    cargo build -q -p uncharted_runtime --lib --release --target-dir out/rust
# $ORIGIN/gpu: packaged copies keep the library next to the binary without patching it.
gpu=(-Lout/gpu -lbbgpu -Wl,-rpath,'$ORIGIN/gpu' -Wl,-rpath,"$PWD/out/gpu" -rdynamic)
# Linux and Android build the memory module from crates/runtime (piece 3.12), so
# src/runtime_memory.c is Windows only.
runtime=()
for source in src/runtime*.c; do
    if [[ $source != src/runtime_memory.c ]]; then runtime+=("$source"); fi
done
# Right after the C sources that call it and ahead of the GPU library it calls (bbgpu_resolve),
# with the system libraries Rust's std needs (rustc --print native-static-libs). Symbols only
# libbbgpu.so asks for (memory's): --undefined makes the linker take them from the archive.
rust=(out/rust/release/libuncharted_runtime.a)
for symbol in runtime_map_guest_memory runtime_memory_set_gpu_hooks runtime_memory_write_backing     runtime_memory_clamp runtime_memory_region runtime_memory_gpu_protect runtime_memory_is_mapped     runtime_disabled_optimizations runtime_disabled_optimizations2; do
    rust+=(-Wl,--undefined="$symbol")
done
rust+=(-lgcc_s -lutil -lrt -lpthread -lm -ldl)
# Third-party decoders: compiled once, without this project's -Werror policy.
atrac9=(third_party/LibAtrac9/C/src/*.c)
if [[ ! -f out/libatrac9.a || -n $(find third_party/LibAtrac9/C/src -newer out/libatrac9.a -name '*.c') ]]; then
    rm -rf out/atrac9 && mkdir -p out/atrac9
    for source in "${atrac9[@]}"; do "$CC" -std=c99 -O2 -g -w -c "$source" -o "out/atrac9/$(basename "${source%.c}").o"; done
    ar rcs out/libatrac9.a out/atrac9/*.o
fi
# BB_BENCHMARK=<seconds> (#173/#242): BB_RUST_BENCHMARK sends the glue to the Rust benchmark in
# bb-probe only; the C unit tests below keep the no-op glue.
"$CC" -std=c11 -O2 -g -Wall -Wextra -Werror -pthread -no-pie -DBB_RUST_BENCHMARK "${includes[@]}" -I. -Isrc src/probe.c src/cpu_patch.c "${runtime[@]}" "${rust[@]}" out/libatrac9.a -lm -lZydis "${gpu[@]}" "${libraries[@]}" -o out/bb-probe
echo "Built $PWD/out/bb-probe"
# GPU check for run.sh (live_resolution=auto): links only the Vulkan loader.
"$CC" -std=c11 -O2 -Wall -Wextra -Werror tools/gpu_capabilities.c "${libraries[@]}" -o out/bb-gpu-capabilities
if [[ ${1:-} == --test ]]; then
    tests=(motion-history-test motion-shader-test wbinvl1-test ui-composition-test upscaler-support-test expand-image-test scene-resolution-test taa-shader-test camera-motion-test vulkan-smoke-test)
    ninja_jobs=()
    if [[ -n ${BB_JOBS:-} ]]; then ninja_jobs=(-j"$BB_JOBS"); fi
    if ! ninja -C out/gpu "${ninja_jobs[@]}" "${tests[@]}" > out/gpu-tests-build.log 2>&1; then
        tail -40 out/gpu-tests-build.log >&2
        echo 'GPU test build failed (full log: out/gpu-tests-build.log)' >&2; exit 1
    fi
    for test in "${tests[@]:0:6}"; do "out/gpu/$test"; done
    for candidate in /run/opengl-driver/share/vulkan/icd.d/lvp_icd*.json /usr/share/vulkan/icd.d/lvp_icd*.json; do
        if [[ -f $candidate ]]; then export VK_ICD_FILENAMES=$candidate; break; fi
    done
    if [[ -n ${VK_ICD_FILENAMES:-} ]]; then echo "GPU tests: VK_ICD_FILENAMES=$VK_ICD_FILENAMES"; fi
    for test in scene-resolution-test taa-shader-test camera-motion-test vulkan-smoke-test; do "out/gpu/$test"; done
    "$CC" -std=c11 -O2 -g -Wall -Wextra -Werror -pthread "${includes[@]}" -I. -Isrc tests/test_pad.c "${libraries[@]}" -o out/pad-test
    out/pad-test
    "$CC" -std=c11 -O2 -g -Wall -Wextra -Werror -pthread -I. -Isrc tests/test_runtime.c "${runtime[@]}" "${rust[@]}" out/libatrac9.a -lm "${gpu[@]}" "${libraries[@]}" -o out/runtime-test
    out/runtime-test
    "$CC" -std=c11 -O2 -g -Wall -Wextra -Werror -pthread -Isrc tests/test_file_mods.c -o out/file-mods-test
    out/file-mods-test
    "$CC" -std=c11 -O2 -g -Wall -Wextra -Werror -pthread -I. -Isrc tests/test_sema.c "${runtime[@]}" "${rust[@]}" out/libatrac9.a -lm "${gpu[@]}" "${libraries[@]}" -o out/sema-test
    out/sema-test
    "$CC" -std=c11 -O2 -g -Wall -Wextra -Werror -pthread -I. -Isrc tests/test_ajm.c "${runtime[@]}" "${rust[@]}" out/libatrac9.a -lm "${gpu[@]}" "${libraries[@]}" -o out/ajm-test
    out/ajm-test
    "$CC" -std=c11 -O2 -g -Wall -Wextra -Werror -Isrc tests/test_speakers.c -lm -o out/speakers-test
    out/speakers-test
    "$CC" -std=c11 -O2 -g -Wall -Wextra -Werror -pthread -no-pie "${includes[@]}" -I. -Isrc tests/test_eop_matcher.c src/cpu_patch.c "${runtime[@]}" "${rust[@]}" out/libatrac9.a -lm -lZydis "${gpu[@]}" "${libraries[@]}" -o out/eop-matcher-test
    out/eop-matcher-test
    "${CXX:-c++}" -std=c++20 -O2 -g -Wall -Wextra -Werror -I. tests/test_settings_default.cpp gpu/shim/bbport_settings.cpp -o out/settings-default-test
    out/settings-default-test
    "${CXX:-c++}" -std=c++20 -O2 -g -Wall -Wextra -Werror -pthread -Igpu/shim \
        tests/test_upscaler_support.cpp gpu/shim/bbport_settings.cpp -o out/upscaler-support-test
    out/upscaler-support-test
    "${CXX:-c++}" -std=c++20 -O2 -g -Wall -Wextra -Werror -pthread -Igpu/shim \
        tests/test_vertex_input_deny.cpp gpu/shim/vertex_input_deny.cpp -o out/vertex-input-deny-test
    out/vertex-input-deny-test
    "$CC" -std=c11 -O2 -g -Wall -Wextra -Werror -pthread -Isrc tests/test_cpu_patch.c src/cpu_patch.c -lZydis -o out/cpu-patch-test
    out/cpu-patch-test
    "${CXX:-c++}" -std=c++20 -O2 -g -Wall -Wextra -Werror -pthread -I. tests/test_equeue_request_gate.cpp -o out/equeue-request-gate-test
    out/equeue-request-gate-test
fi
