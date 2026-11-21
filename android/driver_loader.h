// SPDX-License-Identifier: BSD-2-Clause
// Adapted from bylaws/libadrenotools (commit 8fae8ce254dfc1344527e05301e43f37dea2df80).
// Strict Android Vulkan driver loader with explicit mode selection and fail-closed validation (#210).

#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum BbAndroidDriverMode {
    BB_ANDROID_DRIVER_SYSTEM = 0,
    BB_ANDROID_DRIVER_CUSTOM = 1,
    BB_ANDROID_DRIVER_BUNDLED = 2,
} BbAndroidDriverMode;

typedef struct BbAndroidDriverConfig {
    uint32_t mode;
    uint32_t use_legacy_packaging;
    const char* native_library_dir;
    const char* app_storage_dir;
    const char* custom_driver_so;
    const char* expected_driver_name;
} BbAndroidDriverConfig;

/// Injectable loader/driver-identity probe for host synthetic testing; pass NULL on Android
/// to use libadrenotools / dlopen and real Vulkan driver property probing.
typedef struct BbAndroidDriverProbe {
    void* (*open_system)(const char* so_name);
    void* (*open_custom)(const char* native_library_dir, const char* tmp_dir,
                         const char* hook_dir, const char* custom_driver_dir,
                         const char* custom_driver_name);
    int (*query_driver_name)(void* vulkan_handle, char* out_name, size_t out_cap);
    void (*close_handle)(void* vulkan_handle);
} BbAndroidDriverProbe;

/// Opens and validates the explicitly selected Vulkan driver.
/// Returns 0 on success with *out_handle set; returns < 0 on failure with *out_handle == NULL
/// and an explanation in err_buf. Never silently falls back from custom/bundled to system.
int bb_android_open_vulkan_driver(const BbAndroidDriverConfig* cfg,
                                  const BbAndroidDriverProbe* probe,
                                  void** out_handle,
                                  char* err_buf,
                                  size_t err_cap);

#ifdef __cplusplus
}
#endif
