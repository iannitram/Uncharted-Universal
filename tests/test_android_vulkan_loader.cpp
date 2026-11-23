// SPDX-License-Identifier: BSD-2-Clause
#include "android/driver_loader.h"

#include <cassert>
#include <cstring>

namespace {

struct MockState {
    bool allow_system = true;
    bool allow_custom = true;
    const char* reported_driver = "turnip";
    int open_system_calls = 0;
    int open_custom_calls = 0;
    int close_calls = 0;
    int fake_system_handle = 1;
    int fake_custom_handle = 2;
    bool allow_caps = true;
    BbAndroidVulkanCaps caps = {};
};

MockState g_state;

void* MockOpenSystem(const char*) {
    ++g_state.open_system_calls;
    return g_state.allow_system ? &g_state.fake_system_handle : nullptr;
}

void* MockOpenCustom(const char*, const char*, const char*, const char*, const char*) {
    ++g_state.open_custom_calls;
    return g_state.allow_custom ? &g_state.fake_custom_handle : nullptr;
}

int MockQueryDriver(void*, char* out_name, size_t out_cap) {
    if (!g_state.reported_driver || g_state.reported_driver[0] == '\0') {
        return -1;
    }
    std::strncpy(out_name, g_state.reported_driver, out_cap - 1);
    out_name[out_cap - 1] = '\0';
    return 0;
}

void MockClose(void*) {
    ++g_state.close_calls;
}

int MockQueryCaps(void*, BbAndroidVulkanCaps* out_caps) {
    if (!g_state.allow_caps || !out_caps) {
        return -1;
    }
    *out_caps = g_state.caps;
    return 0;
}

const BbAndroidDriverProbe kProbe = {
    MockOpenSystem,
    MockOpenCustom,
    MockQueryDriver,
    MockClose,
    nullptr,
};

const BbAndroidDriverProbe kCapsProbe = {
    MockOpenSystem,
    MockOpenCustom,
    MockQueryDriver,
    MockClose,
    MockQueryCaps,
};

} // namespace

int main() {
    char err[256] = {};
    void* handle = nullptr;

    // 1. Custom mode with valid Turnip driver succeeds.
    g_state = MockState{};
    BbAndroidDriverConfig custom_ok = {
        BB_ANDROID_DRIVER_CUSTOM,
        1,
        "/data/app/lib/arm64",
        "/data/user/0/org.uncharted/files",
        "/data/user/0/org.uncharted/files/drivers/libvulkan_freedreno.so",
        "turnip",
    };
    assert(bb_android_open_vulkan_driver(&custom_ok, &kProbe, &handle, err, sizeof(err)) == 0);
    assert(handle == &g_state.fake_custom_handle);
    assert(g_state.open_custom_calls == 1);
    assert(g_state.open_system_calls == 0);

    // 2. Bundled mode with valid Turnip driver succeeds.
    g_state = MockState{};
    BbAndroidDriverConfig bundled_ok = custom_ok;
    bundled_ok.mode = BB_ANDROID_DRIVER_BUNDLED;
    assert(bb_android_open_vulkan_driver(&bundled_ok, &kProbe, &handle, err, sizeof(err)) == 0);
    assert(handle == &g_state.fake_custom_handle);

    // 3. Custom mode where libadrenotools silently falls back to Qualcomm stock driver fails closed.
    g_state = MockState{};
    g_state.reported_driver = "Qualcomm Adreno Vulkan";
    assert(bb_android_open_vulkan_driver(&custom_ok, &kProbe, &handle, err, sizeof(err)) == -3);
    assert(handle == nullptr);
    assert(g_state.close_calls == 1);
    assert(g_state.open_system_calls == 0);

    // 4. Custom mode without useLegacyPackaging or with missing driver fails before opening.
    g_state = MockState{};
    BbAndroidDriverConfig no_legacy = custom_ok;
    no_legacy.use_legacy_packaging = 0;
    assert(bb_android_open_vulkan_driver(&no_legacy, &kProbe, &handle, err, sizeof(err)) == -1);
    assert(handle == nullptr);
    assert(g_state.open_custom_calls == 0);

    g_state = MockState{};
    g_state.allow_custom = false;
    assert(bb_android_open_vulkan_driver(&custom_ok, &kProbe, &handle, err, sizeof(err)) == -2);
    assert(handle == nullptr);
    assert(g_state.open_system_calls == 0);

    // 5. Explicit system mode succeeds only when driver identity validates.
    g_state = MockState{};
    g_state.reported_driver = "Qualcomm Adreno Vulkan";
    BbAndroidDriverConfig sys_ok = {
        BB_ANDROID_DRIVER_SYSTEM, 1, nullptr, nullptr, nullptr, nullptr,
    };
    assert(bb_android_open_vulkan_driver(&sys_ok, &kProbe, &handle, err, sizeof(err)) == 0);
    assert(handle == &g_state.fake_system_handle);

    g_state = MockState{};
    g_state.reported_driver = "";
    assert(bb_android_open_vulkan_driver(&sys_ok, &kProbe, &handle, err, sizeof(err)) == -3);
    assert(handle == nullptr);
    assert(g_state.close_calls == 1);

    // 6. GPU classification & driver selection (#536): Snapdragon/Adreno vs Google Tensor/Mali.
    assert(bb_android_classify_gpu(BB_ANDROID_VENDOR_QUALCOMM, "Adreno 750") ==
           BB_ANDROID_GPU_ADRENO);
    assert(bb_android_classify_gpu(BB_ANDROID_VENDOR_ARM, "Mali-G710") == BB_ANDROID_GPU_MALI);
    assert(bb_android_classify_gpu(0, "Mali-G710 r0p0") == BB_ANDROID_GPU_MALI);
    assert(bb_android_classify_gpu(0x10de, "NVIDIA") == BB_ANDROID_GPU_UNKNOWN);

    BbAndroidDriverConfig selected = {};
    assert(bb_android_select_driver_for_gpu(BB_ANDROID_VENDOR_QUALCOMM, "Adreno 750", &bundled_ok,
                                            &selected, err, sizeof(err)) == 0);
    assert(selected.mode == BB_ANDROID_DRIVER_BUNDLED);

    // Adreno refuses silent fallback to system driver.
    assert(bb_android_select_driver_for_gpu(BB_ANDROID_VENDOR_QUALCOMM, "Adreno 750", &sys_ok,
                                            &selected, err, sizeof(err)) == -1);
    assert(bb_android_select_driver_for_gpu(BB_ANDROID_VENDOR_QUALCOMM, "Adreno 750", nullptr,
                                            &selected, err, sizeof(err)) == -1);

    // Tensor / Mali selects system libvulkan.so.
    assert(bb_android_select_driver_for_gpu(BB_ANDROID_VENDOR_ARM, "Mali-G710", nullptr, &selected,
                                            err, sizeof(err)) == 0);
    assert(selected.mode == BB_ANDROID_DRIVER_SYSTEM);

    // Unknown vendor fails closed.
    assert(bb_android_select_driver_for_gpu(0x9999, "UnknownGPU", &bundled_ok, &selected, err,
                                            sizeof(err)) == -1);

    // 7. Tensor/Mali capability validation succeeds with Vulkan 1.3 + required features.
    g_state = MockState{};
    g_state.reported_driver = "Mali-G710";
    g_state.caps = BbAndroidVulkanCaps{
        BB_ANDROID_VENDOR_ARM,
        BB_ANDROID_VK_API_VERSION_1_3,
        1,
        1,
        1,
        1,
        1,
        "Mali-G710",
    };
    assert(bb_android_open_vulkan_driver(&selected, &kCapsProbe, &handle, err, sizeof(err)) == 0);
    assert(handle == &g_state.fake_system_handle);

    // 8. Tensor/Mali fails closed when Vulkan < 1.3 or required features are missing.
    g_state = MockState{};
    g_state.reported_driver = "Mali-G710";
    g_state.caps = BbAndroidVulkanCaps{
        BB_ANDROID_VENDOR_ARM,
        (1u << 22) | (2u << 12), // Vulkan 1.2
        1,
        1,
        1,
        1,
        1,
        "Mali-G710",
    };
    assert(bb_android_open_vulkan_driver(&selected, &kCapsProbe, &handle, err, sizeof(err)) == -3);
    assert(handle == nullptr);
    assert(g_state.close_calls == 1);

    g_state.close_calls = 0;
    g_state.caps.api_version = BB_ANDROID_VK_API_VERSION_1_3;
    g_state.caps.has_dynamic_rendering = 0;
    assert(bb_android_open_vulkan_driver(&selected, &kCapsProbe, &handle, err, sizeof(err)) == -3);
    assert(handle == nullptr);
    assert(g_state.close_calls == 1);

    g_state.close_calls = 0;
    g_state.allow_caps = false;
    assert(bb_android_open_vulkan_driver(&selected, &kCapsProbe, &handle, err, sizeof(err)) == -3);
    assert(handle == nullptr);
    assert(g_state.close_calls == 1);

    return 0;
}
