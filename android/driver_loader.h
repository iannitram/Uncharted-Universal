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

/// PCI / Khronos vendor IDs used to distinguish Snapdragon (Adreno) from Google Tensor (Mali).
#define BB_ANDROID_VENDOR_QUALCOMM 0x5143u
#define BB_ANDROID_VENDOR_ARM 0x13B5u

/// Vulkan 1.3 packed API version (VK_MAKE_API_VERSION(0, 1, 3, 0)).
#define BB_ANDROID_VK_API_VERSION_1_3 ((1u << 22) | (3u << 12))

typedef enum BbAndroidGpuFamily {
    BB_ANDROID_GPU_UNKNOWN = 0,
    BB_ANDROID_GPU_ADRENO = 1,
    BB_ANDROID_GPU_MALI = 2,
} BbAndroidGpuFamily;

typedef struct BbAndroidVulkanCaps {
    uint32_t vendor_id;
    uint32_t api_version;
    uint32_t has_dynamic_rendering;
    uint32_t has_synchronization2;
    uint32_t has_timeline_semaphore;
    uint32_t has_buffer_device_address;
    uint32_t has_swapchain_ext;
    char driver_name[256];
} BbAndroidVulkanCaps;

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
    /// Optional capability probe (required for Tensor/Mali Vulkan 1.3 feature validation).
    int (*query_vulkan_caps)(void* vulkan_handle, BbAndroidVulkanCaps* out_caps);
} BbAndroidDriverProbe;

/// Classifies an Android GPU from its Vulkan vendor ID and/or device/driver name (#536).
BbAndroidGpuFamily bb_android_classify_gpu(uint32_t vendor_id, const char* driver_name);

/// Validates that a system Tensor/Mali Vulkan driver exposes Vulkan 1.3 and all required
/// shadPS4 device features/extensions (#536). Returns 0 on success or < 0 with a diagnostic.
int bb_android_validate_mali_caps(const BbAndroidVulkanCaps* caps, char* err_buf, size_t err_cap);

/// Selects the driver configuration for the detected GPU (#536):
/// - Snapdragon / Adreno (0x5143): requires a Turnip config (CUSTOM or BUNDLED); never falls back.
/// - Google Tensor / Mali (0x13B5): selects SYSTEM (`libvulkan.so`) for explicit cap validation.
/// - Unknown GPU: fails closed with a diagnostic.
int bb_android_select_driver_for_gpu(uint32_t vendor_id,
                                     const char* driver_name,
                                     const BbAndroidDriverConfig* turnip_cfg,
                                     BbAndroidDriverConfig* out_cfg,
                                     char* err_buf,
                                     size_t err_cap);

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
