// SPDX-License-Identifier: GPL-2.0-only
// NativeActivity + JNI + FEX + Vulkan Driver + Gamepad bridge for Stage 7.03 thin APKs (#524).

#include "android/driver_loader.h"
#include "android/fex_bridge.h"

#include <android/configuration.h>
#include <android/input.h>
#include <android/keycodes.h>
#include <android/log.h>
#include <android/looper.h>
#include <android/native_activity.h>
#include <android/native_window.h>
#include <android/window.h>
#include <android_native_app_glue.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <jni.h>
#include <unistd.h>
#include <vulkan/vulkan.h>

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <sys/stat.h>

#define LOG_TAG "UnchartedAPK"
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)

struct AndroidApkFfiSession;

extern "C" {
const char* uncharted_apk_compiled_game_key(void);
const char* uncharted_apk_first_boot_prompt_text(void);
AndroidApkFfiSession* uncharted_apk_session_create(const char* game_id,
                                                   const char* package_name,
                                                   const char* native_lib_dir,
                                                   const char* app_storage_dir,
                                                   uint32_t gpu_vendor_id,
                                                   const char* gpu_driver_name,
                                                   uint32_t display_width,
                                                   uint32_t display_height,
                                                   bool game_files_present);
void uncharted_apk_session_destroy(AndroidApkFfiSession* session);
void uncharted_apk_session_connect_gamepad(AndroidApkFfiSession* session,
                                           int32_t device_id,
                                           int32_t vendor_id,
                                           const char* name);
void uncharted_apk_session_disconnect_gamepad(AndroidApkFfiSession* session, int32_t device_id);
int32_t uncharted_apk_session_on_key_event(AndroidApkFfiSession* session,
                                           int32_t device_id,
                                           int32_t key_code,
                                           int32_t action,
                                           bool shift_held,
                                           bool is_repeat);
int32_t uncharted_apk_session_on_axis_event(AndroidApkFfiSession* session,
                                            int32_t device_id,
                                            int32_t axis,
                                            float value);
int32_t uncharted_apk_session_on_display_change(AndroidApkFfiSession* session,
                                                uint32_t width,
                                                uint32_t height,
                                                uint64_t timestamp_ms);
int32_t uncharted_apk_session_first_boot_decision(const AndroidApkFfiSession* session);
}

namespace {

struct AppJniMetadata {
    std::string package_name;
    std::string game_id;
    std::string native_lib_dir;
    std::string app_storage_dir;
    bool game_files_present = false;
};

struct ApkRuntimeState {
    AndroidApkFfiSession* session = nullptr;
    AppJniMetadata meta;
    uint32_t gpu_vendor_id = 0;
    std::string gpu_driver_name = "unknown-vulkan";
    std::string vk_api_str = "0.0.0";
    int driver_family = 0;
    int driver_rc = -1;
    uint32_t driver_mode = 0;
    bool fex_ok = false;
    uint32_t rt_ver = 0;
    uint32_t fex_ver = 0;
    int32_t display_w = 0;
    int32_t display_h = 0;
    std::string last_input = "none";
    uint64_t timestamp_ms = 0;
};

ANativeActivity* g_ui_activity = nullptr;
int g_ui_pipe[2] = {-1, -1};
int32_t g_last_smallest_dp = 0;

void HideSystemBars(JNIEnv* env, jobject activity) {
    if (!env || !activity) {
        return;
    }
    jclass act = env->GetObjectClass(activity);
    jmethodID get_win = act ? env->GetMethodID(act, "getWindow", "()Landroid/view/Window;") : nullptr;
    jobject window = get_win ? env->CallObjectMethod(activity, get_win) : nullptr;
    if (env->ExceptionCheck() || !window) {
        env->ExceptionClear();
        if (act) env->DeleteLocalRef(act);
        return;
    }
    jclass winc = env->GetObjectClass(window);
    jmethodID get_ctl =
        winc ? env->GetMethodID(winc, "getInsetsController", "()Landroid/view/WindowInsetsController;")
             : nullptr;
    if (env->ExceptionCheck()) env->ExceptionClear();
    jobject ctl = get_ctl ? env->CallObjectMethod(window, get_ctl) : nullptr;
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        ctl = nullptr;
    }
    if (ctl) {
        jclass cc = env->GetObjectClass(ctl);
        jclass types = env->FindClass("android/view/WindowInsets$Type");
        jmethodID sys_bars =
            types ? env->GetStaticMethodID(types, "systemBars", "()I") : nullptr;
        jint bars = sys_bars ? env->CallStaticIntMethod(types, sys_bars) : 0;
        jmethodID set_beh =
            cc ? env->GetMethodID(cc, "setSystemBarsBehavior", "(I)V") : nullptr;
        jmethodID hide_m = cc ? env->GetMethodID(cc, "hide", "(I)V") : nullptr;
        if (set_beh) env->CallVoidMethod(ctl, set_beh, 2); // BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        if (hide_m && bars != 0) env->CallVoidMethod(ctl, hide_m, bars);
        if (types) env->DeleteLocalRef(types);
        if (cc) env->DeleteLocalRef(cc);
        env->DeleteLocalRef(ctl);
    } else if (winc) {
        jmethodID get_decor = env->GetMethodID(winc, "getDecorView", "()Landroid/view/View;");
        jobject decor = get_decor ? env->CallObjectMethod(window, get_decor) : nullptr;
        if (decor) {
            jclass dc = env->GetObjectClass(decor);
            jmethodID set_vis = dc ? env->GetMethodID(dc, "setSystemUiVisibility", "(I)V") : nullptr;
            if (set_vis) env->CallVoidMethod(decor, set_vis, 0x1706);
            if (dc) env->DeleteLocalRef(dc);
            env->DeleteLocalRef(decor);
        }
    }
    if (env->ExceptionCheck()) env->ExceptionClear();
    if (winc) env->DeleteLocalRef(winc);
    env->DeleteLocalRef(window);
    env->DeleteLocalRef(act);
}

// Large screens (>= 600 dp, e.g. Pixel Fold inner screen or tablet) allow any user orientation
// (SCREEN_ORIENTATION_FULL_USER = 13), while smaller cover/phone screens lock to
// SCREEN_ORIENTATION_SENSOR_LANDSCAPE (6), matching ACNH-Universal.
void ApplyPixelFoldOrientation(JNIEnv* env, jobject activity) {
    if (!env || !activity) {
        return;
    }
    int32_t sw = -1;
    jclass ac = env->GetObjectClass(activity);
    jmethodID get_res =
        ac ? env->GetMethodID(ac, "getResources", "()Landroid/content/res/Resources;") : nullptr;
    jobject res = get_res ? env->CallObjectMethod(activity, get_res) : nullptr;
    if (res) {
        jclass rc = env->GetObjectClass(res);
        jmethodID get_cfg =
            rc ? env->GetMethodID(rc, "getConfiguration", "()Landroid/content/res/Configuration;")
               : nullptr;
        jobject cfg = get_cfg ? env->CallObjectMethod(res, get_cfg) : nullptr;
        if (cfg) {
            jclass cc = env->GetObjectClass(cfg);
            jfieldID f = cc ? env->GetFieldID(cc, "smallestScreenWidthDp", "I") : nullptr;
            if (f) sw = env->GetIntField(cfg, f);
            if (cc) env->DeleteLocalRef(cc);
            env->DeleteLocalRef(cfg);
        }
        if (rc) env->DeleteLocalRef(rc);
        env->DeleteLocalRef(res);
    }
    if (env->ExceptionCheck()) env->ExceptionClear();
    if (sw > 0) g_last_smallest_dp = sw;
    const int want = sw >= 600 ? 13 : 6;
    static int asked = -1;
    if (want != asked) {
        jmethodID m = ac ? env->GetMethodID(ac, "setRequestedOrientation", "(I)V") : nullptr;
        if (m) env->CallVoidMethod(activity, m, want);
        if (env->ExceptionCheck()) env->ExceptionClear();
        asked = want;
        LOGI("android: smallest side %d dp: orientation %s", sw,
             want == 13 ? "any (large screen)" : "landscape");
    }
    if (ac) env->DeleteLocalRef(ac);
}

int OnUiThreadRequest(int fd, int, void*) {
    char buf[16];
    while (::read(fd, buf, sizeof(buf)) > 0) {
    }
    if (!g_ui_activity) {
        return 1;
    }
    HideSystemBars(g_ui_activity->env, g_ui_activity->clazz);
    ApplyPixelFoldOrientation(g_ui_activity->env, g_ui_activity->clazz);
    return 1;
}

void RequestUiBarsAndOrientation() {
    if (g_ui_pipe[1] >= 0) {
        const char b = 1;
        (void)::write(g_ui_pipe[1], &b, 1);
    }
}

std::string JStringToStd(JNIEnv* env, jstring jstr) {
    if (!jstr) {
        return {};
    }
    const char* utf = env->GetStringUTFChars(jstr, nullptr);
    std::string out = utf ? utf : "";
    if (utf) {
        env->ReleaseStringUTFChars(jstr, utf);
    }
    env->DeleteLocalRef(jstr);
    return out;
}

bool FileExists(const std::string& path) {
    struct stat st {};
    return ::stat(path.c_str(), &st) == 0;
}

AppJniMetadata QueryAppMetadataViaJni(ANativeActivity* activity) {
    AppJniMetadata meta;
    meta.game_id = uncharted_apk_compiled_game_key();
    if (activity && activity->internalDataPath) {
        meta.app_storage_dir = activity->internalDataPath;
    }

    if (!activity || !activity->vm || !activity->clazz) {
        return meta;
    }

    JNIEnv* env = nullptr;
    bool attached = false;
    const jint get_rc = activity->vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6);
    if (get_rc == JNI_EDETACHED) {
        if (activity->vm->AttachCurrentThread(&env, nullptr) == JNI_OK) {
            attached = true;
        }
    }
    if (!env) {
        return meta;
    }

    jclass activity_cls = env->GetObjectClass(activity->clazz);
    if (activity_cls) {
        jmethodID get_pkg = env->GetMethodID(activity_cls, "getPackageName", "()Ljava/lang/String;");
        if (get_pkg) {
            auto pkg_jstr = static_cast<jstring>(env->CallObjectMethod(activity->clazz, get_pkg));
            meta.package_name = JStringToStd(env, pkg_jstr);
        }

        jmethodID get_app_info =
            env->GetMethodID(activity_cls, "getApplicationInfo", "()Landroid/content/pm/ApplicationInfo;");
        if (get_app_info) {
            jobject app_info = env->CallObjectMethod(activity->clazz, get_app_info);
            if (app_info) {
                jclass app_info_cls = env->GetObjectClass(app_info);
                jfieldID native_lib_fid =
                    env->GetFieldID(app_info_cls, "nativeLibraryDir", "Ljava/lang/String;");
                if (native_lib_fid) {
                    auto lib_jstr =
                        static_cast<jstring>(env->GetObjectField(app_info, native_lib_fid));
                    meta.native_lib_dir = JStringToStd(env, lib_jstr);
                }
                env->DeleteLocalRef(app_info_cls);
                env->DeleteLocalRef(app_info);
            }
        }

        // Read <meta-data android:name="org.uncharted.game_id" ... /> from PackageManager.
        jmethodID get_pm = env->GetMethodID(activity_cls, "getPackageManager",
                                            "()Landroid/content/pm/PackageManager;");
        if (get_pm && !meta.package_name.empty()) {
            jobject pm = env->CallObjectMethod(activity->clazz, get_pm);
            if (pm) {
                jclass pm_cls = env->GetObjectClass(pm);
                jmethodID get_pm_app_info = env->GetMethodID(
                    pm_cls, "getApplicationInfo",
                    "(Ljava/lang/String;I)Landroid/content/pm/ApplicationInfo;");
                if (get_pm_app_info) {
                    jstring pkg_arg = env->NewStringUTF(meta.package_name.c_str());
                    constexpr jint kGetMetaData = 128;
                    jobject pm_app_info =
                        env->CallObjectMethod(pm, get_pm_app_info, pkg_arg, kGetMetaData);
                    env->DeleteLocalRef(pkg_arg);
                    if (env->ExceptionCheck()) {
                        env->ExceptionClear();
                    } else if (pm_app_info) {
                        jclass pm_info_cls = env->GetObjectClass(pm_app_info);
                        jfieldID meta_fid =
                            env->GetFieldID(pm_info_cls, "metaData", "Landroid/os/Bundle;");
                        if (meta_fid) {
                            jobject bundle = env->GetObjectField(pm_app_info, meta_fid);
                            if (bundle) {
                                jclass bundle_cls = env->GetObjectClass(bundle);
                                jmethodID get_str = env->GetMethodID(
                                    bundle_cls, "getString",
                                    "(Ljava/lang/String;)Ljava/lang/String;");
                                if (get_str) {
                                    jstring key = env->NewStringUTF("org.uncharted.game_id");
                                    auto val = static_cast<jstring>(
                                        env->CallObjectMethod(bundle, get_str, key));
                                    env->DeleteLocalRef(key);
                                    std::string from_manifest = JStringToStd(env, val);
                                    if (!from_manifest.empty()) {
                                        meta.game_id = from_manifest;
                                    }
                                }
                                env->DeleteLocalRef(bundle_cls);
                                env->DeleteLocalRef(bundle);
                            }
                        }
                        env->DeleteLocalRef(pm_info_cls);
                        env->DeleteLocalRef(pm_app_info);
                    }
                }
                env->DeleteLocalRef(pm_cls);
                env->DeleteLocalRef(pm);
            }
        }
        env->DeleteLocalRef(activity_cls);
    }

    if (attached) {
        activity->vm->DetachCurrentThread();
    }

    // M9 thin APK: check if Stage 7.04 downloaded game files already exist in internal storage.
    if (!meta.app_storage_dir.empty() && !meta.game_id.empty()) {
        const std::string marker = meta.app_storage_dir + "/games/" + meta.game_id + "/.installed";
        meta.game_files_present = FileExists(marker);
    }
    return meta;
}

void ProbeVulkanGpuAndDriver(const AppJniMetadata& meta, ApkRuntimeState& state) {
    state.gpu_vendor_id = 0;
    state.gpu_driver_name = "unknown-vulkan";
    state.vk_api_str = "0.0.0";

    void* vk_lib = dlopen("libvulkan.so", RTLD_NOW);
    if (!vk_lib) {
        LOGE("dlopen(libvulkan.so) failed: %s", dlerror());
        return;
    }

    auto vk_get_inst_proc =
        reinterpret_cast<PFN_vkGetInstanceProcAddr>(dlsym(vk_lib, "vkGetInstanceProcAddr"));
    if (vk_get_inst_proc) {
        auto vk_create_instance = reinterpret_cast<PFN_vkCreateInstance>(
            vk_get_inst_proc(VK_NULL_HANDLE, "vkCreateInstance"));
        if (vk_create_instance) {
            VkApplicationInfo app_info{};
            app_info.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO;
            app_info.pApplicationName = "UnchartedAPK";
            app_info.apiVersion = VK_API_VERSION_1_3;

            VkInstanceCreateInfo ci{};
            ci.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO;
            ci.pApplicationInfo = &app_info;

            VkInstance instance = VK_NULL_HANDLE;
            if (vk_create_instance(&ci, nullptr, &instance) == VK_SUCCESS && instance) {
                auto vk_enum_devs = reinterpret_cast<PFN_vkEnumeratePhysicalDevices>(
                    vk_get_inst_proc(instance, "vkEnumeratePhysicalDevices"));
                auto vk_get_props = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties>(
                    vk_get_inst_proc(instance, "vkGetPhysicalDeviceProperties"));
                auto vk_destroy_inst = reinterpret_cast<PFN_vkDestroyInstance>(
                    vk_get_inst_proc(instance, "vkDestroyInstance"));

                uint32_t count = 1;
                VkPhysicalDevice gpu = VK_NULL_HANDLE;
                if (vk_enum_devs && vk_get_props &&
                    vk_enum_devs(instance, &count, &gpu) >= 0 && count > 0 && gpu) {
                    VkPhysicalDeviceProperties props{};
                    vk_get_props(gpu, &props);
                    state.gpu_vendor_id = props.vendorID;
                    state.gpu_driver_name = props.deviceName;
                    char api_buf[32];
                    std::snprintf(api_buf, sizeof(api_buf), "%u.%u.%u",
                                  VK_API_VERSION_MAJOR(props.apiVersion),
                                  VK_API_VERSION_MINOR(props.apiVersion),
                                  VK_API_VERSION_PATCH(props.apiVersion));
                    state.vk_api_str = api_buf;
                    LOGI("Vulkan physical device: vendor=0x%04x device=\"%s\" api=%s",
                         props.vendorID, props.deviceName, api_buf);
                }
                if (vk_destroy_inst) {
                    vk_destroy_inst(instance, nullptr);
                }
            }
        }
    }
    dlclose(vk_lib);

    const BbAndroidGpuFamily family =
        bb_android_classify_gpu(state.gpu_vendor_id, state.gpu_driver_name.c_str());
    BbAndroidDriverConfig turnip_cfg{};
    turnip_cfg.mode = BB_ANDROID_DRIVER_BUNDLED;
    turnip_cfg.use_legacy_packaging = 1;
    turnip_cfg.native_library_dir = meta.native_lib_dir.c_str();
    turnip_cfg.app_storage_dir = meta.app_storage_dir.c_str();
    turnip_cfg.custom_driver_so = "";
    turnip_cfg.expected_driver_name = "Turnip";

    BbAndroidDriverConfig selected{};
    char err_buf[256] = {};
    const int sel_rc = bb_android_select_driver_for_gpu(
        state.gpu_vendor_id, state.gpu_driver_name.c_str(), &turnip_cfg, &selected, err_buf,
        sizeof(err_buf));
    state.driver_family = static_cast<int>(family);
    state.driver_rc = sel_rc;
    state.driver_mode = selected.mode;
    LOGI("Driver selector: family=%d rc=%d mode=%u msg=\"%s\"", state.driver_family, sel_rc,
         selected.mode, err_buf);
}

void VerifyFexRuntimeBridge(ApkRuntimeState& state) {
    state.rt_ver = uncharted_runtime_version();
    state.fex_ver = runtime_fex_abi_version();
    state.fex_ok = bb::android::VerifyRuntimeAbi();
    const uint64_t ctx = bb::android::CreateGuestContext(0x1000, 0x2000);
    const int destroy_rc = ctx != 0 ? bb::android::DestroyGuestContext(ctx) : -1;
    LOGI("FEX bridge verified: abi_ok=%d rt_ver=%u fex_ver=%u ctx=%llu destroy_rc=%d",
         state.fex_ok ? 1 : 0, state.rt_ver, state.fex_ver, static_cast<unsigned long long>(ctx),
         destroy_rc);
}

// Compact 5x7 uppercase/ASCII bitmap font for ANativeWindow_Buffer diagnostics UI.
const uint8_t* Glyph5x7(char raw) {
    const unsigned char c = static_cast<unsigned char>(std::toupper(static_cast<unsigned char>(raw)));
    static constexpr uint8_t kSpace[5] = {0x00, 0x00, 0x00, 0x00, 0x00};
    static constexpr uint8_t kBang[5] = {0x00, 0x00, 0x5F, 0x00, 0x00};
    static constexpr uint8_t kParenL[5] = {0x00, 0x1C, 0x22, 0x41, 0x00};
    static constexpr uint8_t kParenR[5] = {0x00, 0x41, 0x22, 0x1C, 0x00};
    static constexpr uint8_t kPlus[5] = {0x08, 0x08, 0x3E, 0x08, 0x08};
    static constexpr uint8_t kComma[5] = {0x00, 0x50, 0x30, 0x00, 0x00};
    static constexpr uint8_t kDash[5] = {0x08, 0x08, 0x08, 0x08, 0x08};
    static constexpr uint8_t kDot[5] = {0x00, 0x60, 0x60, 0x00, 0x00};
    static constexpr uint8_t kSlash[5] = {0x20, 0x10, 0x08, 0x04, 0x02};
    static constexpr uint8_t kColon[5] = {0x00, 0x36, 0x36, 0x00, 0x00};
    static constexpr uint8_t kEq[5] = {0x14, 0x14, 0x14, 0x14, 0x14};
    static constexpr uint8_t kQ[5] = {0x02, 0x01, 0x51, 0x09, 0x06};
    static constexpr uint8_t kUnder[5] = {0x40, 0x40, 0x40, 0x40, 0x40};
    static constexpr uint8_t kPipe[5] = {0x00, 0x00, 0x7F, 0x00, 0x00};
    static constexpr uint8_t kDigits[10][5] = {
        {0x3E, 0x51, 0x49, 0x45, 0x3E}, {0x00, 0x42, 0x7F, 0x40, 0x00},
        {0x42, 0x61, 0x51, 0x49, 0x46}, {0x21, 0x41, 0x45, 0x4B, 0x31},
        {0x18, 0x14, 0x12, 0x7F, 0x10}, {0x27, 0x45, 0x45, 0x45, 0x39},
        {0x3C, 0x4A, 0x49, 0x49, 0x30}, {0x01, 0x71, 0x09, 0x05, 0x03},
        {0x36, 0x49, 0x49, 0x49, 0x36}, {0x06, 0x49, 0x49, 0x29, 0x1E},
    };
    static constexpr uint8_t kAlpha[26][5] = {
        {0x7E, 0x11, 0x11, 0x11, 0x7E}, {0x7F, 0x49, 0x49, 0x49, 0x36},
        {0x3E, 0x41, 0x41, 0x41, 0x22}, {0x7F, 0x41, 0x41, 0x22, 0x1C},
        {0x7F, 0x49, 0x49, 0x49, 0x41}, {0x7F, 0x09, 0x09, 0x09, 0x01},
        {0x3E, 0x41, 0x49, 0x49, 0x7A}, {0x7F, 0x08, 0x08, 0x08, 0x7F},
        {0x00, 0x41, 0x7F, 0x41, 0x00}, {0x20, 0x40, 0x41, 0x3F, 0x01},
        {0x7F, 0x08, 0x14, 0x22, 0x41}, {0x7F, 0x40, 0x40, 0x40, 0x40},
        {0x7F, 0x02, 0x0C, 0x02, 0x7F}, {0x7F, 0x04, 0x08, 0x10, 0x7F},
        {0x3E, 0x41, 0x41, 0x41, 0x3E}, {0x7F, 0x09, 0x09, 0x09, 0x06},
        {0x3E, 0x41, 0x51, 0x21, 0x5E}, {0x7F, 0x09, 0x19, 0x29, 0x46},
        {0x46, 0x49, 0x49, 0x49, 0x31}, {0x01, 0x01, 0x7F, 0x01, 0x01},
        {0x3F, 0x40, 0x40, 0x40, 0x3F}, {0x1F, 0x20, 0x40, 0x20, 0x1F},
        {0x3F, 0x40, 0x38, 0x40, 0x3F}, {0x63, 0x14, 0x08, 0x14, 0x63},
        {0x07, 0x08, 0x70, 0x08, 0x07}, {0x61, 0x51, 0x49, 0x45, 0x43},
    };
    if (c >= '0' && c <= '9') return kDigits[c - '0'];
    if (c >= 'A' && c <= 'Z') return kAlpha[c - 'A'];
    switch (c) {
        case '!': return kBang;
        case '(': return kParenL;
        case ')': return kParenR;
        case '+': return kPlus;
        case ',': return kComma;
        case '-': return kDash;
        case '.': return kDot;
        case '/': return kSlash;
        case ':': return kColon;
        case '=': return kEq;
        case '?': return kQ;
        case '_': return kUnder;
        case '|': return kPipe;
        default: return kSpace;
    }
}

inline uint32_t PackRgba(uint8_t r, uint8_t g, uint8_t b, uint8_t a = 255) {
    return static_cast<uint32_t>(r) | (static_cast<uint32_t>(g) << 8) |
           (static_cast<uint32_t>(b) << 16) | (static_cast<uint32_t>(a) << 24);
}

void FillRect(const ANativeWindow_Buffer& buf, int x, int y, int w, int h, uint32_t rgba) {
    const int x0 = std::max(0, x);
    const int y0 = std::max(0, y);
    const int x1 = std::min(static_cast<int>(buf.width), x + w);
    const int y1 = std::min(static_cast<int>(buf.height), y + h);
    auto* pixels = static_cast<uint32_t*>(buf.bits);
    for (int py = y0; py < y1; ++py) {
        uint32_t* row = pixels + py * buf.stride;
        for (int px = x0; px < x1; ++px) {
            row[px] = rgba;
        }
    }
}

void DrawText(const ANativeWindow_Buffer& buf, int x, int y, int scale, uint32_t rgba,
              const std::string& text) {
    int cx = x;
    for (const char ch : text) {
        if (cx + 6 * scale > static_cast<int>(buf.width)) break;
        const uint8_t* col = Glyph5x7(ch);
        for (int gx = 0; gx < 5; ++gx) {
            const uint8_t bits = col[gx];
            for (int gy = 0; gy < 7; ++gy) {
                if ((bits >> gy) & 1) {
                    FillRect(buf, cx + gx * scale, y + gy * scale, scale, scale, rgba);
                }
            }
        }
        cx += 6 * scale;
    }
}

void RenderSurfaceFrame(ANativeWindow* window, const ApkRuntimeState& s) {
    if (!window) return;
    const int32_t w = ANativeWindow_getWidth(window);
    const int32_t h = ANativeWindow_getHeight(window);
    if (w <= 0 || h <= 0) return;

    ANativeWindow_setBuffersGeometry(window, w, h, WINDOW_FORMAT_RGBA_8888);
    ANativeWindow_Buffer buf{};
    if (ANativeWindow_lock(window, &buf, nullptr) != 0 || !buf.bits) {
        return;
    }

    const uint32_t bg = PackRgba(18, 24, 34);
    const uint32_t card = PackRgba(34, 42, 56);
    const uint32_t border = PackRgba(212, 160, 62);
    const uint32_t title_col = PackRgba(245, 240, 224);
    const uint32_t text_col = PackRgba(206, 214, 226);
    const uint32_t ok_col = PackRgba(112, 224, 152);
    const uint32_t warn_col = PackRgba(235, 180, 90);

    FillRect(buf, 0, 0, w, h, bg);

    const int margin = std::max(24, std::min(w, h) / 14);
    const int cw = w - margin * 2;
    const int ch = h - margin * 2;
    const int t = std::max(3, std::min(w, h) / 280);
    FillRect(buf, margin, margin, cw, ch, border);
    FillRect(buf, margin + t, margin + t, cw - 2 * t, ch - 2 * t, card);

    const int scale = std::max(2, std::min(w, h) / 260);
    const int line_h = scale * 12;
    const int lx = margin + scale * 8;
    int ly = margin + scale * 8;

    const char* game_title = "UNCHARTED: DRAKE'S FORTUNE";
    if (s.meta.game_id == "u2") game_title = "UNCHARTED 2: AMONG THIEVES";
    else if (s.meta.game_id == "u3") game_title = "UNCHARTED 3: DRAKE'S DECEPTION";

    DrawText(buf, lx, ly, scale + 1, border, game_title);
    ly += line_h + scale * 4;
    DrawText(buf, lx, ly, scale, title_col, "ANDROID RUNTIME DIAGNOSTICS (STAGE 7.03 / 7.07)");
    ly += line_h * 2;

    char line[256];
    std::snprintf(line, sizeof(line), "PACKAGE: %s", s.meta.package_name.c_str());
    DrawText(buf, lx, ly, scale, text_col, line);
    ly += line_h;

    std::snprintf(line, sizeof(line), "DISPLAY: %dx%d  SW: %d DP (%s)", w, h, g_last_smallest_dp,
                  g_last_smallest_dp >= 600 ? "UNFOLDED / FULL USER" : "COVER / LANDSCAPE");
    DrawText(buf, lx, ly, scale, text_col, line);
    ly += line_h;

    std::snprintf(line, sizeof(line), "VULKAN GPU: %s (0X%04X) API %s", s.gpu_driver_name.c_str(),
                  s.gpu_vendor_id, s.vk_api_str.c_str());
    DrawText(buf, lx, ly, scale, ok_col, line);
    ly += line_h;

    std::snprintf(line, sizeof(line), "DRIVER SELECTOR: FAMILY=%d RC=%d MODE=%u", s.driver_family,
                  s.driver_rc, s.driver_mode);
    DrawText(buf, lx, ly, scale, text_col, line);
    ly += line_h;

    std::snprintf(line, sizeof(line), "FEX BRIDGE: ABI=%s RT=%u FEX=%u",
                  s.fex_ok ? "OK" : "FAIL", s.rt_ver, s.fex_ver);
    DrawText(buf, lx, ly, scale, s.fex_ok ? ok_col : warn_col, line);
    ly += line_h;

    std::snprintf(line, sizeof(line), "GAME FILES: %s",
                  s.meta.game_files_present ? "INSTALLED" : "NOT DOWNLOADED (USE SETUP)");
    DrawText(buf, lx, ly, scale, s.meta.game_files_present ? ok_col : warn_col, line);
    ly += line_h;

    std::snprintf(line, sizeof(line), "LAST INPUT: %s", s.last_input.c_str());
    DrawText(buf, lx, ly, scale, text_col, line);

    const int fy = margin + ch - line_h - scale * 6;
    DrawText(buf, lx, fy, scale, border, "TAP SCREEN OR PRESS BACK TO RETURN TO SETUP");

    ANativeWindow_unlockAndPost(window);
    LOGI("Surface frame rendered (%dx%d)", w, h);
}

int32_t HandleInputEvent(struct android_app* app, AInputEvent* event) {
    auto* state = static_cast<ApkRuntimeState*>(app->userData);
    if (!state || !state->session || !event) {
        return 0;
    }

    const int32_t event_type = AInputEvent_getType(event);
    const int32_t device_id = AInputEvent_getDeviceId(event);

    if (event_type == AINPUT_EVENT_TYPE_KEY) {
        const int32_t key_code = AKeyEvent_getKeyCode(event);
        const int32_t action = AKeyEvent_getAction(event);
        const int32_t meta = AKeyEvent_getMetaState(event);
        const bool shift_held = (meta & AMETA_SHIFT_ON) != 0;
        const bool is_repeat = AKeyEvent_getRepeatCount(event) > 0;
        if (action == AKEY_EVENT_ACTION_UP &&
            (key_code == AKEYCODE_BACK || key_code == AKEYCODE_BUTTON_B)) {
            ANativeActivity_finish(app->activity);
            return 1;
        }
        const int32_t slint_code = uncharted_apk_session_on_key_event(
            state->session, device_id, key_code, action, shift_held, is_repeat);
        char buf[64];
        std::snprintf(buf, sizeof(buf), "KEY %d ACT %d", key_code, action);
        state->last_input = buf;
        if (app->window) RenderSurfaceFrame(app->window, *state);
        if (slint_code >= 0) {
            const int32_t decision = uncharted_apk_session_first_boot_decision(state->session);
            LOGI("Gamepad KeyEvent: dev=%d key=%d -> slint_code=%d first_boot_decision=%d",
                 device_id, key_code, slint_code, decision);
            return 1;
        }
    } else if (event_type == AINPUT_EVENT_TYPE_MOTION) {
        const int32_t source = AInputEvent_getSource(event);
        const int32_t action = AMotionEvent_getAction(event) & AMOTION_EVENT_ACTION_MASK;
        if ((source & AINPUT_SOURCE_TOUCHSCREEN) != 0 && action == AMOTION_EVENT_ACTION_UP) {
            ANativeActivity_finish(app->activity);
            return 1;
        }
        constexpr int32_t kAxes[] = {
            AMOTION_EVENT_AXIS_X,
            AMOTION_EVENT_AXIS_Y,
            AMOTION_EVENT_AXIS_HAT_X,
            AMOTION_EVENT_AXIS_HAT_Y,
        };
        bool handled = false;
        for (const int32_t axis : kAxes) {
            const float value = AMotionEvent_getAxisValue(event, axis, 0);
            const int32_t slint_code =
                uncharted_apk_session_on_axis_event(state->session, device_id, axis, value);
            if (slint_code >= 0) {
                const int32_t decision = uncharted_apk_session_first_boot_decision(state->session);
                LOGI("Gamepad MotionEvent: dev=%d axis=%d val=%.2f -> slint_code=%d decision=%d",
                     device_id, axis, value, slint_code, decision);
                handled = true;
            }
        }
        return handled ? 1 : 0;
    }

    return 0;
}

void HandleAppCmd(struct android_app* app, int32_t cmd) {
    auto* state = static_cast<ApkRuntimeState*>(app->userData);
    if (!state || !state->session) {
        return;
    }

    if ((cmd == APP_CMD_INIT_WINDOW || cmd == APP_CMD_WINDOW_RESIZED ||
         cmd == APP_CMD_GAINED_FOCUS || cmd == APP_CMD_CONFIG_CHANGED) &&
        app->window) {
        RequestUiBarsAndOrientation();
        const int32_t w = ANativeWindow_getWidth(app->window);
        const int32_t h = ANativeWindow_getHeight(app->window);
        if (w > 0 && h > 0) {
            state->display_w = w;
            state->display_h = h;
            state->timestamp_ms += 200;
            const int32_t rc = uncharted_apk_session_on_display_change(
                state->session, static_cast<uint32_t>(w), static_cast<uint32_t>(h),
                state->timestamp_ms);
            LOGI("Display update (%dx%d) -> rc=%d", w, h, rc);
            RenderSurfaceFrame(app->window, *state);
        }
    }
}

} // namespace

extern "C" void glue_ANativeActivity_onCreate(ANativeActivity* activity, void* saved, size_t size);

extern "C" JNIEXPORT void ANativeActivity_onCreate(ANativeActivity* activity,
                                                   void* saved_state,
                                                   size_t saved_state_size) {
    g_ui_activity = activity;
    if (g_ui_pipe[0] < 0 && ::pipe(g_ui_pipe) == 0) {
        ::fcntl(g_ui_pipe[0], F_SETFL, O_NONBLOCK);
        ::fcntl(g_ui_pipe[1], F_SETFL, O_NONBLOCK);
        if (ALooper* looper = ALooper_forThread()) {
            ALooper_addFd(looper, g_ui_pipe[0], ALOOPER_POLL_CALLBACK, ALOOPER_EVENT_INPUT,
                          OnUiThreadRequest, nullptr);
        }
    }
    HideSystemBars(activity->env, activity->clazz);
    ApplyPixelFoldOrientation(activity->env, activity->clazz);
    glue_ANativeActivity_onCreate(activity, saved_state, saved_state_size);
}

void android_main(struct android_app* app) {
    if (!app) {
        return;
    }

    ApkRuntimeState state;
    state.meta = QueryAppMetadataViaJni(app->activity);
    LOGI("Booting thin APK: game_id=%s package=%s m9_bundled_assets=false game_files_present=%d",
         state.meta.game_id.c_str(), state.meta.package_name.c_str(),
         state.meta.game_files_present ? 1 : 0);

    VerifyFexRuntimeBridge(state);
    ProbeVulkanGpuAndDriver(state.meta, state);

    state.session = uncharted_apk_session_create(
        state.meta.game_id.c_str(), state.meta.package_name.c_str(),
        state.meta.native_lib_dir.c_str(), state.meta.app_storage_dir.c_str(),
        state.gpu_vendor_id, state.gpu_driver_name.c_str(), 1280, 720,
        state.meta.game_files_present);

    if (!state.session) {
        LOGE("uncharted_apk_session_create failed fail-closed for game_id=%s package=%s",
             state.meta.game_id.c_str(), state.meta.package_name.c_str());
        return;
    }

    const int32_t decision = uncharted_apk_session_first_boot_decision(state.session);
    LOGI("Stage 7.04 handoff ready: decision=%d prompt=\"%s\"", decision,
         uncharted_apk_first_boot_prompt_text());

    app->userData = &state;
    app->onAppCmd = HandleAppCmd;
    app->onInputEvent = HandleInputEvent;

    while (app->destroyRequested == 0) {
        int events = 0;
        struct android_poll_source* source = nullptr;
        const int ident = ALooper_pollOnce(-1, nullptr, &events, reinterpret_cast<void**>(&source));
        if (ident >= 0 && source) {
            source->process(app, source);
        }
    }

    uncharted_apk_session_destroy(state.session);
    state.session = nullptr;
    LOGI("Thin APK exiting cleanly: game_id=%s", state.meta.game_id.c_str());
}
