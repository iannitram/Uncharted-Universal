cmake_minimum_required(VERSION 3.22)
project(uncharted_android_synthetic LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

add_library(uncharted_fex_smoke STATIC
    fex_smoke.cpp
)
target_include_directories(uncharted_fex_smoke PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/../..
)
if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
    target_compile_options(uncharted_fex_smoke PRIVATE -Wall -Wextra -Werror)
endif()
if(NOT ANDROID)
    find_package(Threads REQUIRED)
    target_link_libraries(uncharted_fex_smoke PUBLIC Threads::Threads)
endif()

add_executable(uncharted-android-fex-smoke
    fex_smoke_main.cpp
)
target_link_libraries(uncharted-android-fex-smoke PRIVATE uncharted_fex_smoke)
if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
    target_compile_options(uncharted-android-fex-smoke PRIVATE -Wall -Wextra -Werror)
endif()

add_executable(test_android_fex_smoke
    ../../tests/test_android_fex_smoke.cpp
)
target_link_libraries(test_android_fex_smoke PRIVATE uncharted_fex_smoke)
if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
    target_compile_options(test_android_fex_smoke PRIVATE -Wall -Wextra -Werror)
endif()

enable_testing()
add_test(NAME android_fex_smoke_test COMMAND test_android_fex_smoke)
add_test(NAME android_fex_smoke_cli COMMAND uncharted-android-fex-smoke)
