cmake_minimum_required(VERSION 3.22)
project(uncharted_android_synthetic LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_library(uncharted_fex_smoke STATIC
    fex_smoke.cpp
)
target_include_directories(uncharted_fex_smoke PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/../..
)

add_executable(test_android_fex_smoke
    ../../tests/test_android_fex_smoke.cpp
)
target_link_libraries(test_android_fex_smoke PRIVATE uncharted_fex_smoke)
