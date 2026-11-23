// SPDX-License-Identifier: BSD-2-Clause
// Adapted from bylaws/libadrenotools (commit 8fae8ce254dfc1344527e05301e43f37dea2df80).

#include "driver_loader.h"

#include <cstdio>
#include <cstring>
#include <string>

#ifdef __ANDROID__
#include <adrenotools/driver.h>
#include <dlfcn.h>
#endif

namespace {

void SetErr(char* err_buf, size_t err_cap, const char* msg) {
    if (err_buf && err_cap > 0) {
        std::snprintf(err_buf, err_cap, "%s", msg ? msg : "unknown error");
    }
}

bool NonEmpty(const char* s) {
    return s && s[0] != '\0';
}

bool ContainsCaseInsensitive(const char* haystack, const char* needle) {
    if (!NonEmpty(haystack) || !NonEmpty(needle)) {
        return false;
    }
    const std::string h(haystack);
    const std::string n(needle);
    for (size_t i = 0; i + n.size() <= h.size(); ++i) {
        bool match = true;
        for (size_t j = 0; j < n.size(); ++j) {
            auto ch = static_cast<unsigned char>(h[i + j]);
            auto cn = static_cast<unsigned char>(n[j]);
            if (ch >= 'A' && ch <= 'Z') ch += 'a' - 'A';
            if (cn >= 'A' && cn <= 'Z') cn += 'a' - 'A';
            if (ch != cn) {
                match = false;
                break;
            }
        }
        if (match) {
            return true;
        }
    }
    return false;
}

bool SplitDriverPath(const char* app_storage_dir, const char* custom_driver_so,
                     std::string& out_dir, std::string& out_name) {
    if (!NonEmpty(app_storage_dir) || !NonEmpty(custom_driver_so)) {
        return false;
    }
    const std::string storage(app_storage_dir);
    const std::string full(custom_driver_so);
    if (full.find("..") != std::string::npos) {
        return false;
    }
    if (full.rfind(storage, 0) != 0) {
        return false;
    }
    const size_t slash = full.find_last_of('/');
    if (slash == std::string::npos || slash + 1 >= full.size()) {
        return false;
    }
    out_dir = full.substr(0, slash + 1);
    out_name = full.substr(slash + 1);
    return true;
}

} // namespace

int bb_android_open_vulkan_driver(const BbAndroidDriverConfig* cfg,
                                  const BbAndroidDriverProbe* probe,
                                  void** out_handle,
                                  char* err_buf,
                                  size_t err_cap) {
    if (!out_handle) {
        SetErr(err_buf, err_cap, "out_handle is null");
        return -1;
    }
    *out_handle = nullptr;

    if (!cfg) {
        SetErr(err_buf, err_cap, "driver config is null");
        return -1;
    }

    if (cfg->mode != BB_ANDROID_DRIVER_SYSTEM && cfg->mode != BB_ANDROID_DRIVER_CUSTOM &&
        cfg->mode != BB_ANDROID_DRIVER_BUNDLED) {
        SetErr(err_buf, err_cap, "unknown Android Vulkan driver mode");
        return -1;
    }

    void* handle = nullptr;
    if (cfg->mode == BB_ANDROID_DRIVER_SYSTEM) {
        if (probe && probe->open_system) {
            handle = probe->open_system("libvulkan.so");
        } else {
#ifdef __ANDROID__
            handle = dlopen("libvulkan.so", RTLD_NOW);
#else
            SetErr(err_buf, err_cap, "system Android loader probe missing on non-Android host");
            return -2;
#endif
        }
        if (!handle) {
            SetErr(err_buf, err_cap, "failed to open system libvulkan.so");
            return -2;
        }
    } else {
        if (!cfg->use_legacy_packaging) {
            SetErr(err_buf, err_cap, "custom/bundled driver requires useLegacyPackaging=true");
            return -1;
        }
        if (!NonEmpty(cfg->native_library_dir)) {
            SetErr(err_buf, err_cap, "nativeLibraryDir is required for custom/bundled driver");
            return -1;
        }
        std::string driver_dir;
        std::string driver_name;
        if (!SplitDriverPath(cfg->app_storage_dir, cfg->custom_driver_so, driver_dir,
                             driver_name)) {
            SetErr(err_buf, err_cap, "custom driver path must reside inside app_storage_dir");
            return -1;
        }
        const std::string tmp_dir = std::string(cfg->app_storage_dir) + "/adrenotools_tmp";
        if (probe && probe->open_custom) {
            handle = probe->open_custom(cfg->native_library_dir, tmp_dir.c_str(),
                                        cfg->native_library_dir, driver_dir.c_str(),
                                        driver_name.c_str());
        } else {
#ifdef __ANDROID__
            handle = adrenotools_open_libvulkan(
                RTLD_NOW, ADRENOTOOLS_DRIVER_CUSTOM, tmp_dir.c_str(), cfg->native_library_dir,
                driver_dir.c_str(), driver_name.c_str(), nullptr, nullptr);
#else
            SetErr(err_buf, err_cap, "custom Android loader probe missing on non-Android host");
            return -2;
#endif
        }
        if (!handle) {
            SetErr(err_buf, err_cap, "libadrenotools failed to open custom Vulkan driver");
            return -2;
        }
    }

    char reported[256] = {};
    const int query_rc =
        (probe && probe->query_driver_name)
            ? probe->query_driver_name(handle, reported, sizeof(reported))
            : -1;
    const auto close_fn = [&](void* h) {
        if (probe && probe->close_handle) {
            probe->close_handle(h);
        } else {
#ifdef __ANDROID__
            dlclose(h);
#endif
        }
    };

    if (query_rc != 0 || !NonEmpty(reported)) {
        close_fn(handle);
        SetErr(err_buf, err_cap, "failed to validate Vulkan driver identity before startup");
        return -3;
    }

    if (cfg->mode != BB_ANDROID_DRIVER_SYSTEM) {
        if (!NonEmpty(cfg->expected_driver_name) ||
            !ContainsCaseInsensitive(reported, cfg->expected_driver_name)) {
            close_fn(handle);
            SetErr(err_buf, err_cap,
                   "selected custom driver identity mismatch (refusing silent stock fallback)");
            return -3;
        }
    } else if (probe && probe->query_vulkan_caps) {
        BbAndroidVulkanCaps caps = {};
        if (probe->query_vulkan_caps(handle, &caps) != 0) {
            close_fn(handle);
            SetErr(err_buf, err_cap, "failed to query system Vulkan capabilities");
            return -3;
        }
        const char* caps_name = NonEmpty(caps.driver_name) ? caps.driver_name : reported;
        if (bb_android_classify_gpu(caps.vendor_id, caps_name) == BB_ANDROID_GPU_MALI) {
            if (bb_android_validate_mali_caps(&caps, err_buf, err_cap) != 0) {
                close_fn(handle);
                return -3;
            }
        }
    }

    *out_handle = handle;
    SetErr(err_buf, err_cap, "");
    return 0;
}

BbAndroidGpuFamily bb_android_classify_gpu(uint32_t vendor_id, const char* driver_name) {
    if (vendor_id == BB_ANDROID_VENDOR_QUALCOMM) {
        return BB_ANDROID_GPU_ADRENO;
    }
    if (vendor_id == BB_ANDROID_VENDOR_ARM) {
        return BB_ANDROID_GPU_MALI;
    }
    if (ContainsCaseInsensitive(driver_name, "adreno") ||
        ContainsCaseInsensitive(driver_name, "turnip") ||
        ContainsCaseInsensitive(driver_name, "freedreno")) {
        return BB_ANDROID_GPU_ADRENO;
    }
    if (ContainsCaseInsensitive(driver_name, "mali")) {
        return BB_ANDROID_GPU_MALI;
    }
    return BB_ANDROID_GPU_UNKNOWN;
}

int bb_android_validate_mali_caps(const BbAndroidVulkanCaps* caps, char* err_buf, size_t err_cap) {
    if (!caps) {
        SetErr(err_buf, err_cap, "Mali Vulkan capabilities struct is null");
        return -1;
    }
    if (bb_android_classify_gpu(caps->vendor_id, caps->driver_name) != BB_ANDROID_GPU_MALI) {
        SetErr(err_buf, err_cap, "Vulkan driver is not ARM/Mali");
        return -3;
    }
    if (caps->api_version < BB_ANDROID_VK_API_VERSION_1_3) {
        SetErr(err_buf, err_cap, "Tensor/Mali Vulkan driver requires Vulkan 1.3 or newer");
        return -3;
    }
    if (!caps->has_dynamic_rendering) {
        SetErr(err_buf, err_cap, "Tensor/Mali driver missing required dynamicRendering feature");
        return -3;
    }
    if (!caps->has_synchronization2) {
        SetErr(err_buf, err_cap, "Tensor/Mali driver missing required synchronization2 feature");
        return -3;
    }
    if (!caps->has_timeline_semaphore) {
        SetErr(err_buf, err_cap, "Tensor/Mali driver missing required timelineSemaphore feature");
        return -3;
    }
    if (!caps->has_buffer_device_address) {
        SetErr(err_buf, err_cap, "Tensor/Mali driver missing required bufferDeviceAddress feature");
        return -3;
    }
    if (!caps->has_swapchain_ext) {
        SetErr(err_buf, err_cap, "Tensor/Mali driver missing required VK_KHR_swapchain extension");
        return -3;
    }
    SetErr(err_buf, err_cap, "");
    return 0;
}

int bb_android_select_driver_for_gpu(uint32_t vendor_id,
                                     const char* driver_name,
                                     const BbAndroidDriverConfig* turnip_cfg,
                                     BbAndroidDriverConfig* out_cfg,
                                     char* err_buf,
                                     size_t err_cap) {
    if (!out_cfg) {
        SetErr(err_buf, err_cap, "out_cfg is null");
        return -1;
    }
    const BbAndroidGpuFamily family = bb_android_classify_gpu(vendor_id, driver_name);
    if (family == BB_ANDROID_GPU_ADRENO) {
        if (!turnip_cfg || (turnip_cfg->mode != BB_ANDROID_DRIVER_BUNDLED &&
                            turnip_cfg->mode != BB_ANDROID_DRIVER_CUSTOM)) {
            SetErr(err_buf, err_cap,
                   "Snapdragon/Adreno requires bundled or custom Turnip (refusing stock fallback)");
            return -1;
        }
        *out_cfg = *turnip_cfg;
        SetErr(err_buf, err_cap, "");
        return 0;
    }
    if (family == BB_ANDROID_GPU_MALI) {
        *out_cfg = BbAndroidDriverConfig{
            BB_ANDROID_DRIVER_SYSTEM,
            1,
            nullptr,
            nullptr,
            nullptr,
            "mali",
        };
        SetErr(err_buf, err_cap, "");
        return 0;
    }
    SetErr(err_buf, err_cap, "unsupported Android GPU vendor (expected Snapdragon/Adreno or Tensor/Mali)");
    return -1;
}
