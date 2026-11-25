// SPDX-License-Identifier: BSD-2-Clause
// Adapted from bylaws/libadrenotools (commit 8fae8ce254dfc1344527e05301e43f37dea2df80).

#include "driver_loader.h"

#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

#if defined(__ANDROID__) && __has_include(<adrenotools/driver.h>)
#define BB_ANDROID_HAS_ADRENOTOOLS 1
#include <adrenotools/driver.h>
#endif

#if defined(__ANDROID__) || defined(__linux__)
#include <dlfcn.h>
#if __has_include(<vulkan/vulkan_core.h>)
#define BB_ANDROID_HAS_VULKAN_PROBE 1
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan_core.h>
#endif
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
#ifdef BB_ANDROID_HAS_ADRENOTOOLS
            handle = adrenotools_open_libvulkan(
                RTLD_NOW, ADRENOTOOLS_DRIVER_CUSTOM, tmp_dir.c_str(), cfg->native_library_dir,
                driver_dir.c_str(), driver_name.c_str(), nullptr, nullptr);
#elif defined(__ANDROID__)
            SetErr(err_buf, err_cap, "libadrenotools not available in this Android build");
            return -2;
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
    BbAndroidVulkanCaps probed_caps = {};
    bool have_probed_caps = false;

    int query_rc = -1;
    if (probe && probe->query_driver_name) {
        query_rc = probe->query_driver_name(handle, reported, sizeof(reported));
    } else if (!probe || !probe->open_system) {
        if (bb_android_probe_vulkan_caps_from_handle(handle, &probed_caps, err_buf, err_cap) == 0 &&
            NonEmpty(probed_caps.driver_name)) {
            std::snprintf(reported, sizeof(reported), "%s", probed_caps.driver_name);
            have_probed_caps = true;
            query_rc = 0;
        }
    }

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
        if (!err_buf || err_cap == 0 || err_buf[0] == '\0') {
            SetErr(err_buf, err_cap, "failed to validate Vulkan driver identity before startup");
        }
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
    } else {
        if (NonEmpty(cfg->expected_driver_name) &&
            !ContainsCaseInsensitive(reported, cfg->expected_driver_name)) {
            close_fn(handle);
            SetErr(err_buf, err_cap,
                   "system Vulkan driver identity mismatch for selected GPU mode");
            return -3;
        }

        BbAndroidVulkanCaps caps = {};
        bool have_caps = false;
        if (probe && probe->query_vulkan_caps) {
            if (probe->query_vulkan_caps(handle, &caps) != 0) {
                close_fn(handle);
                SetErr(err_buf, err_cap, "failed to query system Vulkan capabilities");
                return -3;
            }
            have_caps = true;
        } else if (have_probed_caps) {
            caps = probed_caps;
            have_caps = true;
        } else if (!probe || !probe->open_system) {
            if (bb_android_probe_vulkan_caps_from_handle(handle, &caps, err_buf, err_cap) != 0) {
                close_fn(handle);
                return -3;
            }
            have_caps = true;
        }

        if (have_caps) {
            const char* caps_name = NonEmpty(caps.driver_name) ? caps.driver_name : reported;
            const BbAndroidGpuFamily family = bb_android_classify_gpu(caps.vendor_id, caps_name);
            if (family == BB_ANDROID_GPU_MALI) {
                if (bb_android_validate_mali_caps(&caps, err_buf, err_cap) != 0) {
                    close_fn(handle);
                    return -3;
                }
            } else if (family == BB_ANDROID_GPU_ADRENO && NonEmpty(cfg->expected_driver_name)) {
                close_fn(handle);
                SetErr(err_buf, err_cap,
                       "Snapdragon/Adreno requires bundled or custom Turnip (refusing stock fallback)");
                return -3;
            }
        }
    }

    *out_handle = handle;
    SetErr(err_buf, err_cap, "");
    return 0;
}

int bb_android_probe_vulkan_caps_from_handle(void* vulkan_handle,
                                             BbAndroidVulkanCaps* out_caps,
                                             char* err_buf,
                                             size_t err_cap) {
    if (!vulkan_handle) {
        SetErr(err_buf, err_cap, "vulkan_handle is null");
        return -1;
    }
    if (!out_caps) {
        SetErr(err_buf, err_cap, "out_caps is null");
        return -1;
    }
    std::memset(out_caps, 0, sizeof(*out_caps));

#ifdef BB_ANDROID_HAS_VULKAN_PROBE
    auto get_instance_proc_addr = reinterpret_cast<PFN_vkGetInstanceProcAddr>(
        dlsym(vulkan_handle, "vkGetInstanceProcAddr"));
    if (!get_instance_proc_addr) {
        SetErr(err_buf, err_cap, "vkGetInstanceProcAddr symbol not found in Vulkan driver");
        return -3;
    }

    auto vk_enumerate_instance_version = reinterpret_cast<PFN_vkEnumerateInstanceVersion>(
        get_instance_proc_addr(VK_NULL_HANDLE, "vkEnumerateInstanceVersion"));
    auto vk_create_instance = reinterpret_cast<PFN_vkCreateInstance>(
        get_instance_proc_addr(VK_NULL_HANDLE, "vkCreateInstance"));
    if (!vk_create_instance) {
        SetErr(err_buf, err_cap, "vkCreateInstance symbol not found in Vulkan driver");
        return -3;
    }

    uint32_t instance_api_version = VK_API_VERSION_1_0;
    if (vk_enumerate_instance_version) {
        vk_enumerate_instance_version(&instance_api_version);
    }
    const uint32_t target_api =
        (instance_api_version >= VK_API_VERSION_1_3) ? VK_API_VERSION_1_3 : instance_api_version;

    VkApplicationInfo app_info = {};
    app_info.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO;
    app_info.pApplicationName = "bb_android_driver_probe";
    app_info.apiVersion = target_api;

    VkInstanceCreateInfo ici = {};
    ici.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO;
    ici.pApplicationInfo = &app_info;

    VkInstance instance = VK_NULL_HANDLE;
    if (vk_create_instance(&ici, nullptr, &instance) != VK_SUCCESS || instance == VK_NULL_HANDLE) {
        app_info.apiVersion = VK_API_VERSION_1_0;
        if (vk_create_instance(&ici, nullptr, &instance) != VK_SUCCESS ||
            instance == VK_NULL_HANDLE) {
            SetErr(err_buf, err_cap, "vkCreateInstance failed during Vulkan driver probe");
            return -3;
        }
    }

    auto vk_destroy_instance = reinterpret_cast<PFN_vkDestroyInstance>(
        get_instance_proc_addr(instance, "vkDestroyInstance"));
    auto vk_enumerate_physical_devices = reinterpret_cast<PFN_vkEnumeratePhysicalDevices>(
        get_instance_proc_addr(instance, "vkEnumeratePhysicalDevices"));
    auto vk_get_physical_device_properties = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties>(
        get_instance_proc_addr(instance, "vkGetPhysicalDeviceProperties"));
    auto vk_get_physical_device_properties2 = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(
        get_instance_proc_addr(instance, "vkGetPhysicalDeviceProperties2"));
    auto vk_get_physical_device_features2 = reinterpret_cast<PFN_vkGetPhysicalDeviceFeatures2>(
        get_instance_proc_addr(instance, "vkGetPhysicalDeviceFeatures2"));
    auto vk_enumerate_device_ext = reinterpret_cast<PFN_vkEnumerateDeviceExtensionProperties>(
        get_instance_proc_addr(instance, "vkEnumerateDeviceExtensionProperties"));

    auto cleanup_instance = [&]() {
        if (vk_destroy_instance && instance != VK_NULL_HANDLE) {
            vk_destroy_instance(instance, nullptr);
        }
    };

    if (!vk_enumerate_physical_devices || !vk_get_physical_device_properties ||
        !vk_enumerate_device_ext) {
        cleanup_instance();
        SetErr(err_buf, err_cap, "required Vulkan physical device query symbols missing");
        return -3;
    }

    uint32_t dev_count = 0;
    if (vk_enumerate_physical_devices(instance, &dev_count, nullptr) != VK_SUCCESS ||
        dev_count == 0) {
        cleanup_instance();
        SetErr(err_buf, err_cap, "no Vulkan physical devices found");
        return -3;
    }
    std::vector<VkPhysicalDevice> devices(dev_count);
    if (vk_enumerate_physical_devices(instance, &dev_count, devices.data()) != VK_SUCCESS ||
        dev_count == 0) {
        cleanup_instance();
        SetErr(err_buf, err_cap, "failed to enumerate Vulkan physical devices");
        return -3;
    }
    const VkPhysicalDevice phys = devices[0];

    VkPhysicalDeviceProperties base_props = {};
    vk_get_physical_device_properties(phys, &base_props);
    out_caps->vendor_id = base_props.vendorID;
    out_caps->api_version = base_props.apiVersion;
    std::snprintf(out_caps->driver_name, sizeof(out_caps->driver_name), "%s",
                  base_props.deviceName);

    if (base_props.apiVersion >= VK_API_VERSION_1_2 && vk_get_physical_device_properties2) {
        VkPhysicalDeviceDriverProperties driver_props = {};
        driver_props.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DRIVER_PROPERTIES;
        VkPhysicalDeviceProperties2 props2 = {};
        props2.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2;
        props2.pNext = &driver_props;
        vk_get_physical_device_properties2(phys, &props2);
        if (NonEmpty(driver_props.driverName)) {
            std::snprintf(out_caps->driver_name, sizeof(out_caps->driver_name), "%s",
                          driver_props.driverName);
        }
    }

    if (base_props.apiVersion >= VK_API_VERSION_1_3 && vk_get_physical_device_features2) {
        VkPhysicalDeviceVulkan13Features vk13 = {};
        vk13.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES;
        VkPhysicalDeviceVulkan12Features vk12 = {};
        vk12.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES;
        vk12.pNext = &vk13;
        VkPhysicalDeviceFeatures2 feats2 = {};
        feats2.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2;
        feats2.pNext = &vk12;
        vk_get_physical_device_features2(phys, &feats2);

        out_caps->has_dynamic_rendering = vk13.dynamicRendering ? 1u : 0u;
        out_caps->has_synchronization2 = vk13.synchronization2 ? 1u : 0u;
        out_caps->has_timeline_semaphore = vk12.timelineSemaphore ? 1u : 0u;
        out_caps->has_buffer_device_address = vk12.bufferDeviceAddress ? 1u : 0u;
    }

    uint32_t ext_count = 0;
    if (vk_enumerate_device_ext(phys, nullptr, &ext_count, nullptr) == VK_SUCCESS && ext_count > 0) {
        std::vector<VkExtensionProperties> exts(ext_count);
        if (vk_enumerate_device_ext(phys, nullptr, &ext_count, exts.data()) == VK_SUCCESS) {
            for (const auto& ext : exts) {
                if (std::strcmp(ext.extensionName, VK_KHR_SWAPCHAIN_EXTENSION_NAME) == 0) {
                    out_caps->has_swapchain_ext = 1u;
                    break;
                }
            }
        }
    }

    cleanup_instance();
    SetErr(err_buf, err_cap, "");
    return 0;
#else
    SetErr(err_buf, err_cap, "Vulkan capability probe not supported on this host build");
    return -3;
#endif
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
    if (ContainsCaseInsensitive(driver_name, "mali") ||
        ContainsCaseInsensitive(driver_name, "immortalis")) {
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
