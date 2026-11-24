# bbport GPU library: shadPS4 video core + shader recompiler + Gnm/VideoOut
# drivers, adapted to the native_probe runtime through shims in shim/.
cmake_minimum_required(VERSION 3.24)
project(bbgpu LANGUAGES C CXX)
# Newer libc++ (clang 21+) no longer includes these transitively; the shadPS4, sirit and FFX
# sources rely on them (std::abort, _countof, swprintf_s, log2, placement new, ...).
if(WIN32)
    foreach(header cstdlib cstring cstdio cstdint cmath cwchar new limits utility algorithm)
        add_compile_options("$<$<COMPILE_LANGUAGE:CXX>:SHELL:-include ${header}>")
    endforeach()
endif()
set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# bbport (#198): Allow pointing the GPU build at an external shared shadPS4 core tree
# (e.g., a pinned iannitram/shadPS4 gpucore checkout) instead of the vendored gpu/shadps4 copy.
set(BB_SHADPS4_CORE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/shadps4" CACHE PATH
    "Path to the shadPS4 core source tree (video_core, shader_recompiler, common, core/libraries)")
if (NOT IS_ABSOLUTE "${BB_SHADPS4_CORE_DIR}")
    get_filename_component(BB_SHADPS4_CORE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/${BB_SHADPS4_CORE_DIR}" ABSOLUTE)
endif()
foreach(bb_core_sub video_core shader_recompiler common core/libraries)
    if (NOT EXISTS "${BB_SHADPS4_CORE_DIR}/${bb_core_sub}")
        message(FATAL_ERROR
            "BB_SHADPS4_CORE_DIR (${BB_SHADPS4_CORE_DIR}) is missing required directory '${bb_core_sub}'")
    endif()
endforeach()
message(STATUS "shadPS4 core dir: ${BB_SHADPS4_CORE_DIR}")

set(BB_SHADPS4_NET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/shadps4" CACHE PATH
    "Path to the shadNet / network / np source tree (defaults to vendored gpu/shadps4)")
if (NOT IS_ABSOLUTE "${BB_SHADPS4_NET_DIR}")
    get_filename_component(BB_SHADPS4_NET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/${BB_SHADPS4_NET_DIR}" ABSOLUTE)
endif()
if (NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE RelWithDebInfo)
endif()
# Frame pointers keep the loader's thread dumps (watchdog, SIGUSR2 sampling) walkable.
add_compile_options(-fno-omit-frame-pointer)

# bbport: link-time optimization (BB_LTO, default on) and profile-guided optimization
# (BB_PGO: off, generate = instrumented build writing into BB_PGO_DIR, use = build with that
# profile). Covers bbgpu and the static libraries linked into it. No -march: the same build
# runs on the Steam Deck.
option(BB_LTO "Link-time optimization of bbgpu" ON)
set(BB_PGO "off" CACHE STRING "Profile-guided optimization: off, generate, use")
set(BB_PGO_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../pgo" CACHE PATH "Profile directory")
if (BB_LTO)
    include(CheckIPOSupported)
    check_ipo_supported(RESULT bb_ipo_ok OUTPUT bb_ipo_error)
    if (bb_ipo_ok)
        set(CMAKE_INTERPROCEDURAL_OPTIMIZATION ON)
    else()
        message(WARNING "LTO unavailable: ${bb_ipo_error}")
    endif()
endif()
if (BB_PGO MATCHES "^(generate|use)$" AND CMAKE_CXX_COMPILER_ID MATCHES "Clang")
    # Functions local to a file (static, lambdas, anonymous namespaces, coroutine parts: a fifth
    # of the profile, command processor included) are named in the profile with their source
    # path. Without this the path holds the checkout's folder, so a profile recorded in one
    # folder missed all of them in any other (another worktree, the GitHub runner). The leading
    # folders up to the repository go: names read "gpu/...;f" or "src/...;f" everywhere.
    get_filename_component(bb_repo_root "${CMAKE_CURRENT_SOURCE_DIR}/.." ABSOLUTE)
    string(REGEX MATCHALL "/" bb_repo_seps "${bb_repo_root}/")
    list(LENGTH bb_repo_seps bb_repo_depth)
    add_compile_options("SHELL:-mllvm -static-func-strip-dirname-prefix=${bb_repo_depth}")
endif()
if (BB_PGO STREQUAL "generate" AND CMAKE_CXX_COMPILER_ID MATCHES "Clang")
    # perf7, clang (Windows): IR instrumentation; the profile goes where LLVM_PROFILE_FILE says and is
    # written every 30 s by bbgpu.cpp (the game often ends without the exit-time write). Atomic
    # counters: several threads run the same code.
    add_compile_options(-fprofile-generate -fprofile-update=atomic)
    add_link_options(-fprofile-generate)
    add_compile_definitions(BB_PGO_GENERATE BB_PGO_CLANG)
elseif (BB_PGO STREQUAL "use" AND CMAKE_CXX_COMPILER_ID MATCHES "Clang")
    # perf7, clang: the merged profile (llvm-profdata merge -o pgo/merged.profdata pgo/*.profraw).
    # Functions changed since the profile keep the normal optimization. The IR profile's pass
    # names each function whose code changed ("hash mismatch") or that is new ("no profile
    # data"): build.sh counts them, so the build tells when the profile is due again. Warnings
    # only, never errors.
    add_compile_options(-fprofile-use=${BB_PGO_DIR}/merged.profdata
                        "SHELL:-mllvm -pgo-warn-missing-function"
                        -Wbackend-plugin -Wno-error=backend-plugin
                        -Wno-profile-instr-out-of-date -Wno-profile-instr-unprofiled)
elseif (BB_PGO STREQUAL "generate")
    # Atomic counters: the GPU, recording, preparation and copy threads run the same code.
    add_compile_options(-fprofile-generate=${BB_PGO_DIR} -fprofile-update=atomic)
    add_link_options(-fprofile-generate=${BB_PGO_DIR})
    add_compile_definitions(BB_PGO_GENERATE)
elseif (BB_PGO STREQUAL "use")
    # Partial training keeps unprofiled code optimized normally; counters from concurrent
    # threads can be slightly inconsistent (correction). Functions changed since the profile
    # (or built differently for it, bbgpu_init) are compiled without it.
    add_compile_options(-fprofile-use=${BB_PGO_DIR} -fprofile-partial-training
                        -fprofile-correction -Wno-missing-profile -Wno-coverage-mismatch)
endif()

find_package(Vulkan REQUIRED)
# Declared minimum Vulkan-Headers (1.4.x patch). v1.4.344 is the tested, bundled version
# (docs/BUILDING_LINUX.md); newer headers such as 1.4.365 build as well. Read from the header the
# build actually uses, so a stale system copy fails here instead of deep inside a compile.
set(BB_VULKAN_HEADERS_MIN "1.4.344" CACHE STRING "Minimum Vulkan-Headers version (1.4.x)")
set(bb_vk_header "${Vulkan_INCLUDE_DIR}/vulkan/vulkan_core.h")
if (NOT EXISTS "${bb_vk_header}")
    message(FATAL_ERROR "vulkan/vulkan_core.h not found under ${Vulkan_INCLUDE_DIR}")
endif()
file(STRINGS "${bb_vk_header}" bb_vk_version_line REGEX "^#define VK_HEADER_VERSION_COMPLETE")
string(REGEX MATCH "VK_MAKE_API_VERSION\\( *0 *, *([0-9]+) *, *([0-9]+) *, *([0-9]+|VK_HEADER_VERSION) *\\)"
       bb_vk_version_match "${bb_vk_version_line}")
if (NOT bb_vk_version_match)
    message(FATAL_ERROR "Cannot read VK_HEADER_VERSION_COMPLETE from ${bb_vk_header}")
endif()
set(bb_vk_major "${CMAKE_MATCH_1}")
set(bb_vk_minor "${CMAKE_MATCH_2}")
file(STRINGS "${bb_vk_header}" bb_vk_patch_line REGEX "^#define VK_HEADER_VERSION [0-9]+")
string(REGEX MATCH "VK_HEADER_VERSION +([0-9]+)" bb_vk_patch_match "${bb_vk_patch_line}")
if (NOT bb_vk_patch_match)
    message(FATAL_ERROR "Cannot read VK_HEADER_VERSION from ${bb_vk_header}")
endif()
set(bb_vk_patch "${CMAKE_MATCH_1}")
set(bb_vk_found "${bb_vk_major}.${bb_vk_minor}.${bb_vk_patch}")
if (bb_vk_found VERSION_LESS BB_VULKAN_HEADERS_MIN)
    message(FATAL_ERROR "Vulkan-Headers ${bb_vk_found} is older than the minimum "
                        "${BB_VULKAN_HEADERS_MIN} (set BB_VULKAN_HEADERS_MIN to override)")
endif()
message(STATUS "Vulkan-Headers ${bb_vk_found} (minimum ${BB_VULKAN_HEADERS_MIN})")
find_package(fmt REQUIRED)
find_package(Boost REQUIRED)
# bbport (Windows): MSYS2 has no magic_enum/miniz packages; the submodules are used instead.
find_package(magic_enum QUIET)
if (NOT magic_enum_FOUND)
    add_library(magic_enum INTERFACE)
    add_library(magic_enum::magic_enum ALIAS magic_enum)
    target_include_directories(magic_enum INTERFACE third_party/magic_enum/include
                                                    third_party/magic_enum/include/magic_enum)
endif()
find_package(tsl-robin-map REQUIRED)
find_package(VulkanMemoryAllocator CONFIG QUIET)
if (NOT VulkanMemoryAllocator_FOUND)
    # MSYS2 ships the header (include/vma/vk_mem_alloc.h) without a CMake package.
    find_path(VMA_INCLUDE_DIR vk_mem_alloc.h PATH_SUFFIXES vma REQUIRED)
    add_library(VulkanMemoryAllocator INTERFACE)
    add_library(GPUOpen::VulkanMemoryAllocator ALIAS VulkanMemoryAllocator)
    target_include_directories(VulkanMemoryAllocator INTERFACE ${VMA_INCLUDE_DIR})
endif()
find_package(PkgConfig REQUIRED)
pkg_check_modules(XXHASH REQUIRED IMPORTED_TARGET libxxhash)
pkg_check_modules(SDL3 REQUIRED IMPORTED_TARGET sdl3)
if (NOT WIN32)
    pkg_check_modules(X11 REQUIRED IMPORTED_TARGET x11)
endif()
find_package(miniz QUIET)
if (NOT miniz_FOUND)
    set(BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
    set(BUILD_TESTS OFF CACHE BOOL "" FORCE)
    set(BUILD_FUZZERS OFF CACHE BOOL "" FORCE)
    set(BUILD_SHARED_LIBS OFF)
    set(CMAKE_POLICY_VERSION_MINIMUM 3.5)
    add_subdirectory(third_party/miniz EXCLUDE_FROM_ALL)
    unset(CMAKE_POLICY_VERSION_MINIMUM)
    add_library(miniz::miniz ALIAS miniz)
endif()
find_package(Zydis REQUIRED)
find_program(GLSLANG_COMPILER NAMES glslang glslangValidator REQUIRED)

set(SIRIT_USE_SYSTEM_SPIRV_HEADERS OFF CACHE BOOL "" FORCE)
add_subdirectory(third_party/sirit EXCLUDE_FROM_ALL)

# bbport: FSR 3.1 temporal upscaling / frame generation, native Vulkan (MIT, FireBurn/FSR-Vulkan).
set(FFX_VK_PORTABLE_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(FFX_VK_PORTABLE_BUILD_PROBE OFF CACHE BOOL "" FORCE)
add_subdirectory(third_party/fsr-vulkan EXCLUDE_FROM_ALL)

# Host shaders -> headers (same scheme as shadPS4's host_shaders/CMakeLists.txt).
set(SHADER_SRC ${BB_SHADPS4_CORE_DIR}/video_core/host_shaders)
set(SHADER_INCLUDE ${CMAKE_CURRENT_BINARY_DIR}/include)
set(SHADER_DIR ${SHADER_INCLUDE}/video_core/host_shaders)
set(SHADER_HEADERS "")
function(add_host_shader FILENAME SHADER_NAME)
    set(OUT ${SHADER_DIR}/${SHADER_NAME}.h)
    string(TOUPPER ${SHADER_NAME} SYMBOL)
    set(DEFINES ${ARGN})
    list(TRANSFORM DEFINES PREPEND "-D")
    add_custom_command(OUTPUT ${OUT}
        COMMAND ${CMAKE_COMMAND} -E make_directory ${SHADER_DIR}
        COMMAND ${GLSLANG_COMPILER} --target-env vulkan1.3 -o ${OUT} --vn ${SYMBOL} ${DEFINES} ${SHADER_SRC}/${FILENAME}
        COMMAND ${CMAKE_COMMAND} "-DHEADER_FILE=${OUT}" -P ${SHADER_SRC}/AddShaderHeaderInclude.cmake
        DEPENDS ${SHADER_SRC}/${FILENAME} VERBATIM)
    set(SHADER_HEADERS ${SHADER_HEADERS} ${OUT} PARENT_SCOPE)
endfunction()
add_host_shader(color_to_ms_depth.frag color_to_ms_depth_frag)
add_host_shader(ms_image_blit.frag ms_image_blit_frag)
add_host_shader(ms_image_blit.frag ms_image_blit_msaa_frag SRC_MSAA=1)
add_host_shader(fault_buffer_process.comp fault_buffer_process_comp)
add_host_shader(fs_tri.vert fs_tri_vert)
add_host_shader(fsr.comp fsr_easu_comp SAMPLE_EASU=1)
add_host_shader(fsr.comp fsr_rcas_comp SAMPLE_RCAS=1)
add_host_shader(post_process.frag post_process_frag)
add_host_shader(camera_motion.comp camera_motion_comp)
add_host_shader(upscale_merge.comp upscale_merge_comp)
add_host_shader(upscale_reactive.comp upscale_reactive_comp)
add_host_shader(taa.comp taa_comp)
add_host_shader(taa_sharpen.comp taa_sharpen_comp)
add_host_shader(taa_sharpen.comp taa_sharpen_ldr_comp LDR_OUTPUT=1)
add_host_shader(buffer_multi_copy.comp buffer_multi_copy_comp)
add_host_shader(depth_resample.frag depth_resample_frag)
add_host_shader(depth_resample.frag depth_stencil_resample_frag HAS_STENCIL=1)
add_host_shader(depth_resample.frag depth_stencil_bits_frag STENCIL_BITS=1)
add_host_shader(camera_motion.comp camera_motion_debug_comp DEBUG_OVERLAY=1)
add_host_shader(frame_gen_inputs.comp frame_gen_inputs_comp)
add_host_shader(frame_gen_ui.comp frame_gen_ui_comp)
foreach(BPP IN ITEMS 8 16 32 64 96 128)
    add_host_shader(tiling.comp tiling_micro_${BPP}_comp BITS_PER_PIXEL=${BPP} IS_MACRO_TILED=0)
    add_host_shader(tiling.comp tiling_macro_${BPP}_comp BITS_PER_PIXEL=${BPP} IS_MACRO_TILED=1)
endforeach()
add_custom_target(host_shaders DEPENDS ${SHADER_HEADERS})

file(GLOB_RECURSE VIDEO_CORE CONFIGURE_DEPENDS ${BB_SHADPS4_CORE_DIR}/video_core/*.cpp)
file(GLOB_RECURSE RECOMPILER CONFIGURE_DEPENDS ${BB_SHADPS4_CORE_DIR}/shader_recompiler/*.cpp)
list(FILTER VIDEO_CORE EXCLUDE REGEX "renderdoc\\.cpp$")
set(COMMON
    ${BB_SHADPS4_CORE_DIR}/common/decoder.cpp ${BB_SHADPS4_CORE_DIR}/common/error.cpp
    ${BB_SHADPS4_CORE_DIR}/common/io_file.cpp ${BB_SHADPS4_CORE_DIR}/common/native_clock.cpp ${BB_SHADPS4_CORE_DIR}/common/number_utils.cpp
    ${BB_SHADPS4_CORE_DIR}/common/path_util.cpp ${BB_SHADPS4_CORE_DIR}/common/rdtsc.cpp ${BB_SHADPS4_CORE_DIR}/common/spin_lock.cpp ${BB_SHADPS4_CORE_DIR}/common/recursive_lock.cpp
    ${BB_SHADPS4_CORE_DIR}/common/string_util.cpp ${BB_SHADPS4_CORE_DIR}/common/thread.cpp ${BB_SHADPS4_CORE_DIR}/common/signal_context.cpp
    shim/common/logging/log.cpp)
file(GLOB AVPLAYER CONFIGURE_DEPENDS ${BB_SHADPS4_CORE_DIR}/core/libraries/avplayer/*.cpp)
pkg_check_modules(FFMPEG REQUIRED IMPORTED_TARGET libavformat libavcodec libavutil libswscale libswresample)
# Includes bbport_settings.cpp and vertex_input_deny.cpp for every GPU build.
file(GLOB SHIM_SOURCES CONFIGURE_DEPENDS shim/*.cpp shim/*/*.cpp)
# Windows: a DLL cannot leave the runtime_* symbols for bb-probe.exe to resolve, so the
# library is static there and linked into the executable.
if (WIN32)
    set(BBGPU_KIND STATIC)
else()
    set(BBGPU_KIND SHARED)
endif()
add_library(bbgpu ${BBGPU_KIND} ${VIDEO_CORE} ${RECOMPILER} ${COMMON} ${SHIM_SOURCES}
    ${BB_SHADPS4_CORE_DIR}/core/libraries/gnmdriver/gnmdriver.cpp
    ${BB_SHADPS4_CORE_DIR}/core/libraries/videoout/driver.cpp ${BB_SHADPS4_CORE_DIR}/core/libraries/videoout/video_out.cpp
    ${BB_SHADPS4_CORE_DIR}/core/libraries/kernel/equeue.cpp ${AVPLAYER}
    ${BB_SHADPS4_CORE_DIR}/core/libraries/videodec/video_utils.cpp
    ${BB_SHADPS4_CORE_DIR}/core/libraries/videodec/videodec2.cpp ${BB_SHADPS4_CORE_DIR}/core/libraries/videodec/videodec2_impl.cpp)
add_dependencies(bbgpu host_shaders)
# bbport co-op: PS4 network and PSN libraries with the shadNet client, from shadp2p
# (github.com/Wozzardman/shadp2p, GPL-2.0), for private Bloodborne co-op servers.
find_package(Protobuf CONFIG QUIET)
if (NOT Protobuf_FOUND)
    # distributions with protobuf 3.x (Debian) ship no CMake package.
    find_package(Protobuf REQUIRED)
endif()
set(SHADNET_PROTO_OUT "${CMAKE_CURRENT_BINARY_DIR}/shadnet_proto_gen")
file(MAKE_DIRECTORY "${SHADNET_PROTO_OUT}")
add_custom_command(
    OUTPUT "${SHADNET_PROTO_OUT}/shadnet.pb.cc" "${SHADNET_PROTO_OUT}/shadnet.pb.h"
    COMMAND protobuf::protoc "--proto_path=${BB_SHADPS4_NET_DIR}/shadnet"
            "--cpp_out=${SHADNET_PROTO_OUT}" "${BB_SHADPS4_NET_DIR}/shadnet/shadnet.proto"
    DEPENDS "${BB_SHADPS4_NET_DIR}/shadnet/shadnet.proto")
file(GLOB_RECURSE BBNET_SOURCES CONFIGURE_DEPENDS ${BB_SHADPS4_NET_DIR}/core/libraries/network/*.cpp
    ${BB_SHADPS4_NET_DIR}/core/libraries/np/*.cpp ${BB_SHADPS4_NET_DIR}/shadnet/*.cpp)
# Trophies and the system dialogs stay in the C runtime; their shadPS4 versions need the
# emulator's UI layers.
list(FILTER BBNET_SOURCES EXCLUDE REGEX
    "np_trophy|trophy_ui|np_sns_facebook_dialog|np_profile_dialog/|np_commerce/")
find_package(nlohmann_json CONFIG REQUIRED)
add_library(bbnet STATIC ${BBNET_SOURCES} "${SHADNET_PROTO_OUT}/shadnet.pb.cc" bbnet/bbnet_glue.cpp)
target_include_directories(bbnet PRIVATE shim "${BB_SHADPS4_NET_DIR}" "${BB_SHADPS4_NET_DIR}/shadnet" "${SHADNET_PROTO_OUT}"
    third_party/imgui third_party/httplib)
target_link_libraries(bbnet PRIVATE nlohmann_json::nlohmann_json)
# HTTPS (the community game server only speaks TLS) and gzip replies in cpp-httplib.
find_package(OpenSSL REQUIRED)
find_package(ZLIB REQUIRED)
target_compile_definitions(bbnet PRIVATE CPPHTTPLIB_OPENSSL_SUPPORT CPPHTTPLIB_ZLIB_SUPPORT)
target_link_libraries(bbnet PRIVATE OpenSSL::SSL OpenSSL::Crypto ZLIB::ZLIB)
if (WIN32)
    target_sources(bbnet PRIVATE third_party/wepoll/wepoll.c)
    target_include_directories(bbnet PRIVATE third_party/wepoll)
endif()
find_path(MINIUPNPC_INCLUDE miniupnpc.h PATH_SUFFIXES miniupnpc REQUIRED)
find_library(MINIUPNPC_LIBRARY NAMES libminiupnpc.a miniupnpc REQUIRED)
target_include_directories(bbnet PRIVATE "${MINIUPNPC_INCLUDE}")
if (WIN32)
    target_compile_definitions(bbnet PRIVATE MINIUPNP_STATICLIB)
endif()
target_link_libraries(bbnet PRIVATE "${MINIUPNPC_LIBRARY}")
target_compile_definitions(bbnet PRIVATE BOOST_ASIO_STANDALONE NDEBUG)
target_compile_options(bbnet PRIVATE -Wno-deprecated-declarations -Wno-missing-field-initializers)
target_link_libraries(bbnet PUBLIC Vulkan::Headers fmt::fmt Boost::headers magic_enum::magic_enum
    protobuf::libprotobuf)
if (WIN32)
    target_compile_definitions(bbnet PUBLIC NOMINMAX WIN32_LEAN_AND_MEAN)
    target_compile_options(bbnet PRIVATE
        $<$<COMPILE_LANGUAGE:CXX>:-include ${CMAKE_CURRENT_SOURCE_DIR}/shim/bb_win_compat.h>)
    target_link_libraries(bbnet PUBLIC ws2_32 iphlpapi bcrypt crypt32)
endif()
target_link_libraries(bbgpu PUBLIC bbnet)
# In-game settings menu: Dear ImGui (third_party/imgui, MIT) with its Vulkan backend, and an
# embedded Cyrillic font (third_party/fonts).
target_sources(bbgpu PRIVATE third_party/imgui/imgui.cpp third_party/imgui/imgui_draw.cpp
    third_party/imgui/imgui_tables.cpp third_party/imgui/imgui_widgets.cpp
    third_party/imgui/backends/imgui_impl_vulkan.cpp)
target_include_directories(bbgpu PRIVATE third_party/imgui third_party/imgui/backends)
target_compile_definitions(bbgpu PRIVATE IMGUI_IMPL_VULKAN_NO_PROTOTYPES
    BB_FONT_PATH="${CMAKE_CURRENT_SOURCE_DIR}/third_party/fonts/DejaVuSans.ttf")
set_source_files_properties(shim/bbport_overlay.cpp PROPERTIES
    OBJECT_DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/third_party/fonts/DejaVuSans.ttf)
target_include_directories(bbgpu PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}" shim "${BB_SHADPS4_CORE_DIR}" ${SHADER_INCLUDE} third_party/gcn/include third_party/half/include)
target_compile_definitions(bbgpu PRIVATE BOOST_ASIO_STANDALONE NDEBUG)
target_compile_options(bbgpu PRIVATE -Wno-deprecated-declarations -Wno-missing-field-initializers)
# Runtime symbols (runtime_memory_*, runtime_process_*) resolve against bb-probe.
if (NOT WIN32)
    target_link_options(bbgpu PRIVATE -Wl,--allow-shlib-undefined)
endif()
target_link_libraries(bbgpu PRIVATE ffx-vulkan::portable ffx-vulkan::fsr4-v07-vulkan
    ffx-vulkan::fsr3-vk-framegeneration-3.1.6)
target_link_libraries(bbgpu PUBLIC Vulkan::Headers fmt::fmt Boost::headers magic_enum::magic_enum
    tsl::robin_map GPUOpen::VulkanMemoryAllocator PkgConfig::XXHASH PkgConfig::SDL3 PkgConfig::FFMPEG miniz::miniz Zydis::Zydis sirit)
if (WIN32)
    # Guest code is entered with the System V ABI; NOMINMAX/WIN32_LEAN_AND_MEAN keep
    # <windows.h> out of the way of std::min/max and the Vulkan/SDL headers.
    target_compile_definitions(bbgpu PUBLIC NOMINMAX WIN32_LEAN_AND_MEAN)
    target_compile_options(bbgpu PRIVATE -include ${CMAKE_CURRENT_SOURCE_DIR}/shim/bb_win_compat.h)
    target_include_directories(bbgpu PRIVATE third_party/xbyak)
    target_link_libraries(bbgpu PUBLIC Vulkan::Vulkan ws2_32 bcrypt ntdll synchronization winmm onecore)
else()
    # the xbyak submodule, as on Windows (no system package needed).
    target_include_directories(bbgpu PRIVATE third_party/xbyak)
    target_link_libraries(bbgpu PUBLIC PkgConfig::X11)
endif()

# bbport (Windows): bbgpu is a static library there, so the loader and the HLE runtime are
# linked here, where CMake knows its dependencies (build.sh builds them with ninja).
if (WIN32)
    enable_language(RC)
    file(GLOB BB_RUNTIME CONFIGURE_DEPENDS ../src/runtime*.c)
    list(FILTER BB_RUNTIME EXCLUDE REGEX "runtime_(memory|fiber|thread)\\.c")
    file(GLOB BB_ATRAC9 CONFIGURE_DEPENDS ../third_party/LibAtrac9/C/src/*.c)
    add_library(atrac9 STATIC ${BB_ATRAC9})
    target_compile_options(atrac9 PRIVATE -w)
    set_target_properties(atrac9 PROPERTIES C_STANDARD 99 INTERPROCEDURAL_OPTIMIZATION OFF)
    # cpu_patch and red_zone come from crates/runtime (Rust, piece 3.14); src/cpu_patch.c and
    # src/red_zone.c stay until 3.17.
    add_executable(bb-probe ../src/probe.c ${BB_RUNTIME} ../src/compat_win.c
        ../packaging/windows/bb-probe.rc)
    set_target_properties(bb-probe PROPERTIES C_STANDARD 11 C_EXTENSIONS OFF)
    target_compile_options(bb-probe PRIVATE $<$<COMPILE_LANGUAGE:C>:-Wall -Wextra -Werror>)
    target_include_directories(bb-probe PRIVATE .. ../src)
    target_link_libraries(bb-probe PRIVATE bbgpu atrac9 Zydis::Zydis onecore bcrypt ws2_32 -pthread)
    # crates/runtime (piece 3.01): the runtime modules ported to Rust (src/runtime.c is now its
    # `abi`) and the benchmark mode (#173/#242), a static library linked into bb-probe. It needs
    # cargo with the x86_64-pc-windows-gnullvm target. UNCHARTED_RUNTIME_LOADER=1 lets it call the
    # C modules that remain (crates/runtime/build.rs). Here CMake builds gpu/ and runs cargo; on
    # Linux the crate's build script builds gpu/ instead (build.sh).
    find_program(BB_CARGO cargo)
    if (NOT BB_CARGO)
        message(FATAL_ERROR "bb-probe needs cargo (rustup target add x86_64-pc-windows-gnullvm): crates/runtime is part of the loader")
    endif()
    set(bb_rust_dir "${CMAKE_CURRENT_BINARY_DIR}/rust")
    set(bb_rust_lib "${bb_rust_dir}/x86_64-pc-windows-gnullvm/release/libuncharted_runtime.a")
    add_custom_target(uncharted_runtime_rust
        COMMAND ${CMAKE_COMMAND} -E env "CARGO_TARGET_DIR=${bb_rust_dir}" UNCHARTED_RUNTIME_LOADER=1
                ${BB_CARGO} build -p uncharted_runtime --lib --release --target x86_64-pc-windows-gnullvm
        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/.."
        BYPRODUCTS "${bb_rust_lib}"
        COMMENT "Building crates/runtime (Rust)"
        USES_TERMINAL)
    add_dependencies(bb-probe uncharted_runtime_rust)
    set_source_files_properties(../src/runtime_benchmark.c PROPERTIES COMPILE_DEFINITIONS BB_RUST_BENCHMARK)
    target_link_libraries(bb-probe PRIVATE "${bb_rust_lib}" kernel32 ntdll userenv ws2_32 dbghelp unwind)
    # The PS4 range (64 GiB - 1 TiB) must stay free: no high-entropy ASLR for heaps and stacks.
    # The guest main thread runs on the process stack: 16 MiB, committed (no stack probes).
    target_link_options(bb-probe PRIVATE -Wl,--disable-high-entropy-va "SHELL:-Xlinker --stack -Xlinker 16777216,16777216")
    add_executable(bb-gpu-capabilities ../tools/gpu_capabilities.c ../packaging/windows/bb-gpu-capabilities.rc)
    target_link_libraries(bb-gpu-capabilities PRIVATE Vulkan::Vulkan)
endif()

# Explicit test targets; neither launches the game nor needs a Vulkan device.
add_executable(motion-history-test EXCLUDE_FROM_ALL ../tests/test_motion_history.cpp)
target_compile_options(motion-history-test PRIVATE -UNDEBUG -Wall -Wextra -Werror)
target_include_directories(motion-history-test PRIVATE ..)
add_executable(motion-shader-test EXCLUDE_FROM_ALL ../tests/test_motion_shaders.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(motion-shader-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" third_party/gcn/include)
target_link_libraries(motion-shader-test PRIVATE bbgpu)
add_executable(wbinvl1-test EXCLUDE_FROM_ALL ../tests/test_wbinvl1.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(wbinvl1-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" ${SHADER_INCLUDE} third_party/gcn/include)
target_link_libraries(wbinvl1-test PRIVATE bbgpu)
add_executable(shader-scalar-64-test EXCLUDE_FROM_ALL ../tests/test_shader_scalar_64.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(shader-scalar-64-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" ${SHADER_INCLUDE} third_party/gcn/include)
target_link_libraries(shader-scalar-64-test PRIVATE bbgpu)
add_executable(image-sample-dref-grad-test EXCLUDE_FROM_ALL ../tests/test_image_sample_dref_grad.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(image-sample-dref-grad-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" ${SHADER_INCLUDE} third_party/gcn/include)
target_link_libraries(image-sample-dref-grad-test PRIVATE bbgpu)
add_executable(ui-composition-test EXCLUDE_FROM_ALL ../tests/test_ui_composition.cpp)
target_compile_options(ui-composition-test PRIVATE -UNDEBUG -Wall -Wextra -Werror)
target_include_directories(ui-composition-test PRIVATE ..)

add_executable(scene-resolution-test EXCLUDE_FROM_ALL ../tests/test_scene_resolution.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(scene-resolution-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" third_party/gcn/include)
# The test's Vulkan-Hpp inline methods have a different set of platform extensions
# from vk_platform.cpp. Keep them local and use an explicit dispatcher in the test.
target_compile_options(scene-resolution-test PRIVATE -UNDEBUG -fPIC)
set_source_files_properties(../tests/test_scene_resolution.cpp PROPERTIES
    COMPILE_OPTIONS "-fvisibility=hidden;-fvisibility-inlines-hidden")
target_link_libraries(scene-resolution-test PRIVATE bbgpu)
add_executable(taa-shader-test EXCLUDE_FROM_ALL ../tests/test_taa_shader.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(taa-shader-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" ${SHADER_INCLUDE} third_party/gcn/include)
target_compile_options(taa-shader-test PRIVATE -UNDEBUG -fPIC)
set_source_files_properties(../tests/test_taa_shader.cpp PROPERTIES
    COMPILE_OPTIONS "-fvisibility=hidden;-fvisibility-inlines-hidden")
target_link_libraries(taa-shader-test PRIVATE bbgpu)
add_dependencies(taa-shader-test host_shaders)

add_executable(camera-motion-test EXCLUDE_FROM_ALL ../tests/test_camera_motion.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(camera-motion-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" ${SHADER_INCLUDE} third_party/gcn/include)
target_compile_options(camera-motion-test PRIVATE -UNDEBUG -fPIC)
target_link_libraries(camera-motion-test PRIVATE bbgpu)
add_dependencies(camera-motion-test host_shaders)

add_executable(upscaler-support-test EXCLUDE_FROM_ALL ../tests/test_upscaler_support.cpp
    shim/bbport_settings.cpp)
target_include_directories(upscaler-support-test PRIVATE shim)
target_compile_options(upscaler-support-test PRIVATE -UNDEBUG)

add_executable(vertex-input-deny-test EXCLUDE_FROM_ALL ../tests/test_vertex_input_deny.cpp
    shim/vertex_input_deny.cpp)
target_include_directories(vertex-input-deny-test PRIVATE shim)
target_compile_options(vertex-input-deny-test PRIVATE -UNDEBUG -Wall -Wextra -Werror)

add_executable(video-pacing-test EXCLUDE_FROM_ALL ../tests/test_video_pacing.cpp)
target_include_directories(video-pacing-test PRIVATE shim)
target_compile_options(video-pacing-test PRIVATE -UNDEBUG -Wall -Wextra -Werror)

add_executable(runtime-resize-test EXCLUDE_FROM_ALL ../tests/test_runtime_resize.cpp)
target_include_directories(runtime-resize-test PRIVATE shim)
target_compile_options(runtime-resize-test PRIVATE -UNDEBUG -Wall -Wextra -Werror)

add_executable(videodec-recovery-test EXCLUDE_FROM_ALL ../tests/test_videodec_recovery.cpp)
target_include_directories(videodec-recovery-test PRIVATE "${BB_SHADPS4_CORE_DIR}")
target_compile_options(videodec-recovery-test PRIVATE -UNDEBUG -Wall -Wextra -Werror)

add_executable(expand-image-test EXCLUDE_FROM_ALL ../tests/test_expand_image.cpp)
target_include_directories(expand-image-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
# vk_common.h carries clang-only pragmas; GCC (the Linux package build) must not fail on them.
target_compile_options(expand-image-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas)
target_link_libraries(expand-image-test PRIVATE bbgpu)

add_executable(tiny-readback-test EXCLUDE_FROM_ALL ../tests/test_tiny_readback.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(tiny-readback-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(tiny-readback-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas)
target_link_libraries(tiny-readback-test PRIVATE bbgpu)

add_executable(small-readback-batch-test EXCLUDE_FROM_ALL ../tests/test_small_readback_batch.cpp)
target_include_directories(small-readback-batch-test PRIVATE "${BB_SHADPS4_CORE_DIR}")
target_compile_options(small-readback-batch-test PRIVATE -UNDEBUG -Wall -Wextra -Werror)

add_executable(macro-mip-tiling-test EXCLUDE_FROM_ALL ../tests/test_macro_mip_tiling.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(macro-mip-tiling-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(macro-mip-tiling-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas)
target_link_libraries(macro-mip-tiling-test PRIVATE bbgpu)

add_executable(vram-fallback-test EXCLUDE_FROM_ALL ../tests/test_vram_fallback.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(vram-fallback-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(vram-fallback-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas)
target_link_libraries(vram-fallback-test PRIVATE bbgpu)

add_executable(vram-probation-test EXCLUDE_FROM_ALL ../tests/test_vram_probation.cpp)
target_include_directories(vram-probation-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(vram-probation-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas -Wno-sign-compare)
target_link_libraries(vram-probation-test PRIVATE Vulkan::Headers Boost::headers)

add_executable(clear-state-and-submit-waits-test EXCLUDE_FROM_ALL
    ../tests/test_clear_state_and_submit_waits.cpp
    ${BB_SHADPS4_CORE_DIR}/video_core/amdgpu/regs.cpp)
target_include_directories(clear-state-and-submit-waits-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(clear-state-and-submit-waits-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas -Wno-unused-variable)
target_link_libraries(clear-state-and-submit-waits-test PRIVATE Vulkan::Headers Boost::headers)

add_executable(bind-state-cache-test EXCLUDE_FROM_ALL
    ../tests/test_bind_state_cache.cpp)
target_include_directories(bind-state-cache-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(bind-state-cache-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas -Wno-unused-variable)
target_link_libraries(bind-state-cache-test PRIVATE Vulkan::Headers Boost::headers)

add_executable(nv-perf-diag-test EXCLUDE_FROM_ALL
    ../tests/test_nv_perf_diag.cpp)
target_include_directories(nv-perf-diag-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(nv-perf-diag-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas)
target_link_libraries(nv-perf-diag-test PRIVATE Vulkan::Headers)

add_executable(signal-flip-gate-test EXCLUDE_FROM_ALL
    ../tests/test_signal_flip_gate.cpp
    ../tests/test_gpu_runtime_stubs.cpp
    shim/bbport_copy.cpp)
target_include_directories(signal-flip-gate-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(signal-flip-gate-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas -Wno-unused-variable)
target_link_libraries(signal-flip-gate-test PRIVATE Vulkan::Headers Boost::headers Threads::Threads)

add_executable(u2-stall-guards-test EXCLUDE_FROM_ALL
    ../tests/test_u2_stall_guards.cpp
    ../tests/test_gpu_runtime_stubs.cpp
    shim/common/assert.cpp
    shim/common/logging/log.cpp)
target_include_directories(u2-stall-guards-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(u2-stall-guards-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas -Wno-unused-variable)
target_link_libraries(u2-stall-guards-test PRIVATE Vulkan::Headers Boost::headers fmt::fmt)

add_executable(wait-backoff-test EXCLUDE_FROM_ALL
    ../tests/test_wait_backoff.cpp)
target_include_directories(wait-backoff-test PRIVATE shim)
target_compile_options(wait-backoff-test PRIVATE -UNDEBUG -Wall -Wextra -Werror)

add_executable(movie-scaling-test EXCLUDE_FROM_ALL ../tests/test_movie_scaling.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(movie-scaling-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(movie-scaling-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas)
target_link_libraries(movie-scaling-test PRIVATE bbgpu)

# Vulkan backend smoke test (command submission and readback, no game graphics): needs a Vulkan
# device, so CI runs it on Mesa lavapipe (.github/workflows/build.yml). It was bb-probe --vulkan-only.
add_executable(vulkan-smoke-test EXCLUDE_FROM_ALL ../tests/test_vulkan_smoke.c)
set_target_properties(vulkan-smoke-test PROPERTIES C_STANDARD 11 C_EXTENSIONS OFF)
target_compile_options(vulkan-smoke-test PRIVATE -Wall -Wextra -Werror)
target_link_libraries(vulkan-smoke-test PRIVATE Vulkan::Vulkan)

add_executable(shared-memory-bounds-test EXCLUDE_FROM_ALL ../tests/test_shared_memory_bounds.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(shared-memory-bounds-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" ${SHADER_INCLUDE} third_party/gcn/include)
target_compile_options(shared-memory-bounds-test PRIVATE -UNDEBUG -fPIC)
set_source_files_properties(../tests/test_shared_memory_bounds.cpp PROPERTIES
    COMPILE_OPTIONS "-fvisibility=hidden;-fvisibility-inlines-hidden")
target_link_libraries(shared-memory-bounds-test PRIVATE bbgpu)
add_dependencies(shared-memory-bounds-test host_shaders)

add_executable(nan-guard-test EXCLUDE_FROM_ALL ../tests/test_nan_guard.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(nan-guard-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" ${SHADER_INCLUDE} third_party/gcn/include)
target_compile_options(nan-guard-test PRIVATE -UNDEBUG -fPIC)
set_source_files_properties(../tests/test_nan_guard.cpp PROPERTIES
    COMPILE_OPTIONS "-fvisibility=hidden;-fvisibility-inlines-hidden")
target_link_libraries(nan-guard-test PRIVATE bbgpu)
add_dependencies(nan-guard-test host_shaders)

add_executable(stage-specialization-start-test EXCLUDE_FROM_ALL ../tests/test_stage_specialization_start.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(stage-specialization-start-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" ${SHADER_INCLUDE} third_party/gcn/include)
target_compile_options(stage-specialization-start-test PRIVATE -UNDEBUG -fPIC)
set_source_files_properties(../tests/test_stage_specialization_start.cpp PROPERTIES
    COMPILE_OPTIONS "-fvisibility=hidden;-fvisibility-inlines-hidden")
target_link_libraries(stage-specialization-start-test PRIVATE bbgpu)
add_dependencies(stage-specialization-start-test host_shaders)

add_executable(button-prompts-test EXCLUDE_FROM_ALL ../tests/test_button_prompts.cpp ../tests/test_gpu_runtime_stubs.cpp)
target_include_directories(button-prompts-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}" ${SHADER_INCLUDE} third_party/gcn/include)
target_compile_definitions(button-prompts-test PRIVATE TEST_LINK_BBGPU)
target_compile_options(button-prompts-test PRIVATE -UNDEBUG -fPIC)
set_source_files_properties(../tests/test_button_prompts.cpp PROPERTIES
    COMPILE_OPTIONS "-fvisibility=hidden;-fvisibility-inlines-hidden")
target_link_libraries(button-prompts-test PRIVATE bbgpu)
add_dependencies(button-prompts-test host_shaders)

add_executable(shader-preload-test EXCLUDE_FROM_ALL ../tests/test_shader_preload.cpp)
target_include_directories(shader-preload-test PRIVATE shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(shader-preload-test PRIVATE -UNDEBUG -Wall -Wextra -Werror)
target_link_libraries(shader-preload-test PRIVATE fmt::fmt)

add_executable(shim-boundary-test EXCLUDE_FROM_ALL ../tests/test_shim_boundary.cpp)
target_include_directories(shim-boundary-test PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}" shim "${BB_SHADPS4_CORE_DIR}")
target_compile_options(shim-boundary-test PRIVATE -UNDEBUG -Wall -Wextra -Werror -Wno-unknown-pragmas)
target_link_libraries(shim-boundary-test PRIVATE Vulkan::Headers)

# bbport: FSR 4 benchmark outside the game (tools/fsr4_bench.cpp): ninja -C out/gpu fsr4-bench.
add_executable(fsr4-bench EXCLUDE_FROM_ALL tools/fsr4_bench.cpp
    ${BB_SHADPS4_CORE_DIR}/video_core/renderer_vulkan/fsr411/fsr411.cpp)
target_include_directories(fsr4-bench PRIVATE "${BB_SHADPS4_CORE_DIR}/video_core/renderer_vulkan/fsr411")
target_link_libraries(fsr4-bench PRIVATE ffx-vulkan::fsr4-v07-vulkan ffx-vulkan::fsr4-v07-assets Vulkan::Vulkan)
