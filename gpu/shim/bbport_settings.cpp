// SPDX-License-Identifier: GPL-2.0-or-later
#include "bbport_settings.h"

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <string>
#include <string_view>
#include <thread>

#ifdef _WIN32
#include <cwchar>
#endif

namespace BbSettings {

namespace {

std::string g_detected_gpu_name;

// The settings file. On Windows BB_CONFIG is read as UTF-16: the launcher sets it from a
// %LOCALAPPDATA% path, and getenv() would turn a user name outside the ANSI code page into '?'
// (the file would then be read from and written to the wrong place).
std::filesystem::path Path() {
#ifdef _WIN32
    const wchar_t* env = _wgetenv(L"BB_CONFIG");
    return env && env[0] ? std::filesystem::path(env) : std::filesystem::path("bbport.ini");
#else
    const char* env = std::getenv("BB_CONFIG");
    return env && env[0] ? std::filesystem::path(env) : std::filesystem::path("bbport.ini");
#endif
}

// The path as UTF-8 for the log: path::string() throws on Windows for a name outside the ANSI code page.
std::string Display(const std::filesystem::path& path) {
    const auto text = path.u8string();
    return std::string(text.begin(), text.end());
}

FILE* OpenFile(const std::filesystem::path& path, const char* mode) {
#ifdef _WIN32
    const std::wstring wide_mode(mode, mode + std::strlen(mode));
    return _wfopen(path.c_str(), wide_mode.c_str());
#else
    return std::fopen(path.c_str(), mode);
#endif
}

float Clamp(float v, float lo, float hi) {
    return std::clamp(v, lo, hi);
}

bool ContainsIcase(std::string_view haystack, std::string_view needle) {
    if (needle.empty()) return true;
    if (haystack.size() < needle.size()) return false;
    auto it = std::search(haystack.begin(), haystack.end(), needle.begin(), needle.end(),
                          [](char a, char b) {
                              return std::tolower(static_cast<unsigned char>(a)) ==
                                     std::tolower(static_cast<unsigned char>(b));
                          });
    return it != haystack.end();
}

bool IsSteamOsRelease(std::string_view os_release) {
    size_t pos = 0;
    while (pos < os_release.size()) {
        const size_t nl = os_release.find('\n', pos);
        std::string_view line = os_release.substr(pos, nl == std::string_view::npos ? os_release.size() - pos : nl - pos);
        while (!line.empty() && (line.front() == ' ' || line.front() == '\t')) line.remove_prefix(1);
        while (!line.empty() && (line.back() == ' ' || line.back() == '\t' || line.back() == '\r')) line.remove_suffix(1);
        if (line.starts_with("ID=")) {
            std::string_view id = line.substr(3);
            if (id.size() >= 2 && ((id.front() == '"' && id.back() == '"') ||
                                    (id.front() == '\'' && id.back() == '\''))) {
                id = id.substr(1, id.size() - 2);
            }
            if (id == "steamos") return true;
        }
        if (nl == std::string_view::npos) break;
        pos = nl + 1;
    }
    return false;
}

#ifdef __linux__ // the host files below exist only there; elsewhere they would be unused
std::string ReadSmallFile(const char* path) {
    FILE* f = std::fopen(path, "r");
    if (!f) return {};
    std::string out;
    char buf[512];
    while (size_t n = std::fread(buf, 1, sizeof(buf), f)) {
        out.append(buf, n);
        if (out.size() >= 8192) break;
    }
    std::fclose(f);
    return out;
}

std::string ReadLinuxCpuModel() {
    const std::string info = ReadSmallFile("/proc/cpuinfo");
    size_t pos = 0;
    while (pos < info.size()) {
        const size_t nl = info.find('\n', pos);
        const std::string_view line(info.data() + pos,
                                    (nl == std::string::npos ? info.size() : nl) - pos);
        if (line.starts_with("model name")) {
            const size_t colon = line.find(':');
            if (colon != std::string_view::npos) {
                std::string_view model = line.substr(colon + 1);
                while (!model.empty() && (model.front() == ' ' || model.front() == '\t'))
                    model.remove_prefix(1);
                return std::string(model);
            }
        }
        if (nl == std::string::npos) break;
        pos = nl + 1;
    }
    return {};
}

#endif

void UpdateHostDetectedProfile(Values& v, const char* gpu_name) {
#ifdef __linux__
    const std::string dmi = ReadSmallFile("/sys/devices/virtual/dmi/id/product_name");
    const std::string cpu = ReadLinuxCpuModel();
    const std::string os_rel = ReadSmallFile("/etc/os-release");
    v.detected_profile = DetectDeviceProfile(gpu_name, dmi.c_str(), cpu.c_str(), os_rel.c_str());
#else
    v.detected_profile = DetectDeviceProfile(gpu_name, "", "", "");
#endif
}

int ParseDeviceProfile(const std::string& value) {
    if (value == "auto") return ProfileAuto;
    if (value == "handheld" || value == "amd-handheld") return ProfileHandheld;
    if (value == "laptop" || value == "strix-halo-tablet") return ProfileLaptop;
    if (value == "desktop") return ProfileDesktop;
    if (value == "3d" || value == "stereo-3d" || value == "3d-stereo") return ProfileStereo3D;
    for (int p = 0; p < ProfileCount; ++p) {
        if (value == ProfileName(p)) return p;
    }
    return -1;
}

int ParseStereoMode(const std::string& value) {
    for (int m = 0; m < StereoModeCount; ++m) {
        if (value == StereoModeName(m)) return m;
    }
    if (value == "0") return StereoOff;
    if (value == "1") return StereoSideBySide;
    if (value == "2") return StereoTopBottom;
    if (value == "3") return StereoAnaglyph;
    return -1;
}

void Set(Values& v, const std::string& key, const std::string& value) {
    const float f = float(std::atof(value.c_str()));
    const int i = std::atoi(value.c_str());
    if (key == "upscaler") {
        for (int u = 0; u < UpscalerCount; ++u) {
            if (value == UpscalerName(u)) {
                v.upscaler = u;
                v.upscaler_chosen = true;
            }
        }
    } else if (key == "frame_gen") {
        for (int g = 0; g < FrameGenCount; ++g) {
            if (value == FrameGenName(g)) {
                v.frame_gen = g;
            }
        }
    } else if (key == "frame_gen_multiplier") {
        v.frame_gen_multiplier = std::clamp(i, 2, 4);
    } else if (key == "preset") {
        v.preset = std::clamp(i, 0, PresetCount - 1);
    } else if (key == "sharpen") {
        v.sharpen = i != 0;
    } else if (key == "sharpness") {
        v.sharpness = Clamp(f, 0.0f, 2.0f);
    } else if (key == "jitter") {
        v.jitter = i != 0;
    } else if (key == "settings_version") {
        v.settings_version = i;
    } else if (key == "reactive") {
        v.reactive = i != 0;
    } else if (key == "object_motion") {
        v.object_motion = i != 0;
    } else if (key == "reactive_scale") {
        v.reactive_scale = Clamp(f, 0.0f, 16.0f);
    } else if (key == "reactive_threshold") {
        v.reactive_threshold = Clamp(f, 0.0f, 1.0f);
    } else if (key == "reactive_max") {
        v.reactive_max = Clamp(f, 0.0f, 1.0f);
    } else if (key == "debug_view") {
        v.debug_view = std::clamp(i, 0, DebugViewCount - 1);
    } else if (key == "show_fps") {
        v.show_fps = i != 0;
    } else if (key == "fsr4_auto_exposure") {
        v.fsr4_auto_exposure = i != 0;
    } else if (key == "fsr4_invert_jitter") {
        v.fsr4_invert_jitter = i != 0;
    } else if (key == "model_lod") {
        v.model_lod = std::clamp(i, -2, 2);
    } else if (key == "live_resolution") {
        v.live_resolution = value == "auto" ? -1 : std::clamp(i, 0, 1);
    } else if (key == "device_profile") {
        if (const int p = ParseDeviceProfile(value); p >= 0) {
            v.device_profile = p;
        }
    } else if (key == "vblank_hz") {
        v.vblank_hz = i <= 0 ? 0 : std::clamp(i, 30, 240);
    } else if (key == "fps_limit") {
        v.fps_limit = i < 0 ? -1 : (i == 0 ? 0 : std::clamp(i, 15, 240));
    } else if (key == "stereo_mode") {
        v.stereo_mode = ParseStereoMode(value.c_str());
    } else if (key == "output_res") {
        for (int r = 0; r < OutputCount; ++r) {
            if (value == std::to_string(OutputWidths[r]) + "x" + std::to_string(OutputHeights[r])) {
                v.output_res = r;
            }
        }
    } else {
        for (int e = 0; e < EffectCount; ++e) {
            if (key == Effects[e].key) {
                v.effects[e] = i != 0;
            }
        }
    }
}

} // namespace

Values& Get() {
    static Values values;
    return values;
}

std::filesystem::path ConfigPath() {
    return Path();
}

void Load() {
    auto& v = Get();
    v.device_profile = ProfileAuto;
    v.vblank_hz = 60;
    v.fps_limit = -1;
    v.stereo_mode = StereoOff;
    v.upscaler_chosen = false;
    UpdateHostDetectedProfile(v, g_detected_gpu_name.empty() ? nullptr : g_detected_gpu_name.c_str());
    for (int e = 0; e < EffectCount; ++e) {
        v.effects[e] = Effects[e].default_on;
    }
    if (FILE* file = OpenFile(Path(), "r")) {
        char line[256];
        while (std::fgets(line, sizeof(line), file)) {
            std::string text{line};
            text.erase(text.find_last_not_of(" \t\r\n") + 1);
            const auto eq = text.find('=');
            if (text.empty() || text[0] == '#' || eq == std::string::npos) {
                continue;
            }
            Set(v, text.substr(0, eq), text.substr(eq + 1));
        }
        std::fclose(file);
        std::printf("Settings: %s\n", Display(Path()).c_str());
        // Linux fork: sub-pixel jitter off by default. Settings older than version 3 had it
        // switched back on (version 2); distant objects still shimmer and shake with it, so it is
        // turned off once. It can be switched on again in the overlay.
        if (v.settings_version < 3 && v.jitter) {
            v.jitter = false;
            std::printf("Settings: sub-pixel jitter switched off (default)\n");
        }
    }
    v.settings_version = 3;
    // Environment overrides (scripts, A/B tests).
    if (const char* env = std::getenv("BB_UPSCALER")) {
        v.upscaler = UpscalerOff;
        v.upscaler_chosen = true;
        for (int u = 0; u < UpscalerCount; ++u) {
            if (std::strcmp(env, UpscalerName(u)) == 0) v.upscaler = u;
        }
    }
    const std::pair<const char*, const char*> env_keys[] = {
        {"BB_FSR_SHARPNESS", "sharpness"},        {"BB_JITTER", "jitter"},
        {"BB_REACTIVE", "reactive"},              {"BB_REACTIVE_SCALE", "reactive_scale"},
        {"BB_REACTIVE_THRESHOLD", "reactive_threshold"}, {"BB_REACTIVE_MAX", "reactive_max"},
        {"BB_UPSCALE_PRESET", "preset"},            {"BB_OBJECT_MOTION", "object_motion"},
        {"BB_FRAME_GEN", "frame_gen"},              {"BB_FRAME_GEN_MULTIPLIER", "frame_gen_multiplier"},
        {"BB_DEVICE_PROFILE", "device_profile"},    {"BB_VBLANK_HZ", "vblank_hz"},
        {"BB_FPS_LIMIT", "fps_limit"},              {"BB_STEREO_MODE", "stereo_mode"},
    };
    for (const auto& [env, key] : env_keys) {
        if (const char* value = std::getenv(env)) {
            Set(v, key, value);
        }
    }
    EnforceProfileConstraints(EffectiveProfile());
    v.startup_preset = v.preset;
    v.startup_upscaler = v.upscaler;
    v.startup_object_motion = v.object_motion;
    for (int e = 0; e < EffectCount; ++e) {
        v.startup_effects[e] = v.effects[e];
    }
    v.startup_model_lod = v.model_lod;
    v.startup_output_res = v.output_res;
    v.startup_live_resolution = v.live_resolution;
    v.startup_vblank_hz = v.vblank_hz;
    v.startup_fps_limit = v.fps_limit;
    v.startup_stereo_mode = v.stereo_mode;
}

void ConfigureUpscalerSupport(bool fsr4, bool fsr411, bool fsr4_assets) {
    auto& v = Get();
    // FSR 4.1.1 has its own assets (fsr4_411/) and keeps its own check.
    v.fsr4_supported = fsr4 && fsr4_assets;
    v.fsr411_supported = fsr4 && fsr411;
    if (fsr4 && !fsr4_assets) {
        v.fsr4_problem = "assets not installed (fsr4_shaders, tools/fetch_fsr4_assets.sh)";
        std::printf("Upscaler: FSR 4 assets not installed (fsr4_shaders or BB_FSR4_DIR); FSR 4 is "
                    "not offered\n");
    }
    const int requested = v.upscaler;
    if ((requested == UpscalerFsr4 && !v.fsr4_supported) ||
        (requested == UpscalerFsr411 && !v.fsr411_supported)) {
        const bool assets_only = requested == UpscalerFsr4 && fsr4 && !fsr4_assets;
        if (!assets_only) {
            v.fsr4_problem = "GPU does not support the selected FSR 4 shaders; using FSR 3.1";
        }
        std::printf("Upscaler: %s %s; falling back to FSR 3.1 before the first frame\n",
                    UpscalerName(requested),
                    assets_only ? "assets not installed" : "unsupported on this GPU");
        v.upscaler = UpscalerFsr3;
    }
}

int DriverDefaultUpscaler(int current, bool chosen, bool driver_validated) {
    return !chosen && !driver_validated && current == UpscalerFsr3 ? UpscalerOff : current;
}

void ConfigureDriverDefault(unsigned driver_id, const char* driver_name) {
    auto& v = Get();
    g_detected_gpu_name = driver_name ? driver_name : "";
    UpdateHostDetectedProfile(v, driver_name);
    const char* display_name = driver_name ? driver_name : "(unknown)";
    const bool validated = FsrDriverValidated(driver_id) || FsrDeviceValidated(driver_name);
    const int before = v.upscaler;
    const int now = DriverDefaultUpscaler(before, v.upscaler_chosen, validated);
    if (now != before) {
        v.upscaler = now;
        v.startup_upscaler = now;
        std::printf("Upscaler: decision: anti-aliasing Off by default: FSR 3.1 has not been checked "
                    "on %s (driver id %u); turn it on in the menu (Insert / L3+R3) to try it\n",
                    display_name, driver_id);
    } else if (before == UpscalerFsr3 && validated) {
        std::printf("Upscaler: decision: FSR 3.1 on by default: measured clean on %s (driver id %u)\n",
                    display_name, driver_id);
    } else if (before == UpscalerFsr3 && !validated) {
        std::printf("Upscaler: decision: FSR 3.1 kept (the player chose it) on %s (driver id %u), "
                    "which it has not been checked on; if the scene shows noise or garbage, set "
                    "anti-aliasing to Off\n",
                    display_name, driver_id);
    }
    EnforceProfileConstraints(EffectiveProfile());
}

void ConfigureDlssSupport(bool available, const char* problem) {
    auto& v = Get();
    v.dlss_supported = available;
    static std::string kept;
    kept = problem ? problem : "";
    v.dlss_problem = available || kept.empty() ? nullptr : kept.c_str();
    if (v.upscaler == UpscalerDlss && !available) {
        std::printf("Upscaler: DLSS unavailable (%s); falling back to FSR 3.1\n",
                    kept.empty() ? "bbport_dlss.dll or nvngx_dlss.dll missing" : kept.c_str());
        v.upscaler = UpscalerFsr3;
    }
}

void ConfigureXessSupport(bool available, const char* problem) {
    auto& v = Get();
    v.xess_supported = available;
    static std::string kept;
    kept = problem ? problem : "";
    v.xess_problem = available || kept.empty() ? nullptr : kept.c_str();
    if (v.upscaler == UpscalerXess && !available) {
        std::printf("Upscaler: XeSS unavailable (%s); falling back to FSR 3.1\n",
                    kept.empty() ? "libxess.dll missing" : kept.c_str());
        v.upscaler = UpscalerFsr3;
    }
}

bool FixedRenderSession() {
    const char* size = std::getenv("BB_RENDER_RES");
    return size && size[0];
}

int RenderPreset() {
    const auto& v = Get();
    return FixedRenderSession() ? v.startup_preset :
        v.upscaler == UpscalerTaa ? NativeAA : v.preset.load();
}

bool ResolutionNeedsRestart() {
    const auto& v = Get();
    // TAA needs the live path (native guest targets): run.sh selects it on restart.
    return FixedRenderSession() &&
        (v.preset != v.startup_preset || v.output_res != v.startup_output_res ||
         (v.upscaler == UpscalerOff) != (v.startup_upscaler == UpscalerOff) ||
         (v.upscaler == UpscalerTaa) != (v.startup_upscaler == UpscalerTaa));
}

bool FrameRateNeedsRestart() {
    const auto& v = Get();
    return v.vblank_hz != v.startup_vblank_hz || v.fps_limit != v.startup_fps_limit;
}

bool StereoNeedsRestart() {
    const auto& v = Get();
    return v.stereo_mode != v.startup_stereo_mode;
}

int DetectDeviceProfile(const char* gpu_name, const char* dmi_product, const char* cpu_model,
                        const char* os_release) {
    const std::string_view gpu = gpu_name ? gpu_name : "";
    const std::string_view dmi = dmi_product ? dmi_product : "";
    const std::string_view cpu = cpu_model ? cpu_model : "";
    const std::string_view osr = os_release ? os_release : "";

    if (ContainsIcase(gpu, "STRIX_HALO") || ContainsIcase(gpu, "8060S") ||
        ContainsIcase(dmi, "ROG Flow Z13") ||
        (ContainsIcase(cpu, "AI Max") && ContainsIcase(cpu, "395"))) {
        return ProfileLaptop;
    }
    if (ContainsIcase(gpu, "PHOENIX") || ContainsIcase(gpu, "VANGOGH") ||
        ContainsIcase(cpu, "Z1 Extreme") || ContainsIcase(cpu, "Custom APU 0405") ||
        ContainsIcase(cpu, "Custom APU 0932") || IsSteamOsRelease(osr)) {
        return ProfileHandheld;
    }
    return ProfileDesktop;
}

int EffectiveProfile() {
    const auto& v = Get();
    const int p = v.device_profile.load();
    if (p == ProfileAuto) {
        const int det = v.detected_profile.load();
        return (det > ProfileAuto && det < ProfileCount) ? det : ProfileDesktop;
    }
    return std::clamp(p, 1, ProfileCount - 1);
}

bool ProfileAllowsUpscaler(int effective_profile, int upscaler) {
    if (effective_profile == ProfileHandheld) {
        return upscaler != UpscalerFsr4 && upscaler != UpscalerFsr411 && upscaler != UpscalerDlss;
    }
    return upscaler >= 0 && upscaler < UpscalerCount;
}

bool ProfileAllowsFrameGen(int effective_profile, int frame_gen) {
    if (effective_profile == ProfileHandheld) {
        return frame_gen != FrameGenDlss;
    }
    return frame_gen >= 0 && frame_gen < FrameGenCount;
}

bool ProfileAllowsFrameRate(int effective_profile, int vblank_hz) {
    if (effective_profile == ProfileHandheld) {
        return vblank_hz == 30 || vblank_hz == 45 || vblank_hz == 60;
    }
    if (effective_profile == ProfileStereo3D) {
        return vblank_hz == 30 || vblank_hz == 60;
    }
    return vblank_hz == 0 || vblank_hz == 30 || vblank_hz == 45 || vblank_hz == 60 ||
           vblank_hz == 120;
}

bool ProfileAllowsStereo(int effective_profile, int stereo_mode) {
    if (effective_profile == ProfileHandheld) {
        return stereo_mode == StereoOff;
    }
    return stereo_mode >= 0 && stereo_mode < StereoModeCount;
}

ProfilePreset PresetForProfile(int effective_profile, bool fsr3_validated, bool fsr4_supported,
                               bool dlss_supported) {
    switch (effective_profile) {
    case ProfileHandheld:
        return ProfilePreset{
            .vblank_hz = 30,
            .fps_limit = -1,
            .output_res = 0, // 1280x720
            .upscaler = UpscalerOff,
            .preset = Quality,
            .frame_gen = FrameGenOff,
            .stereo_mode = StereoOff,
        };
    case ProfileLaptop:
        return ProfilePreset{
            .vblank_hz = 60,
            .fps_limit = -1,
            .output_res = 2, // 2560x1440
            .upscaler = fsr4_supported ? UpscalerFsr4 : (fsr3_validated ? UpscalerFsr3 : UpscalerOff),
            .preset = Quality,
            .frame_gen = FrameGenOff,
            .stereo_mode = StereoOff,
        };
    case ProfileStereo3D:
        return ProfilePreset{
            .vblank_hz = 60,
            .fps_limit = -1,
            .output_res = 1, // 1920x1080
            .upscaler = fsr3_validated ? UpscalerFsr3 : UpscalerOff,
            .preset = NativeAA,
            .frame_gen = FrameGenOff,
            .stereo_mode = StereoSideBySide,
        };
    case ProfileDesktop:
    default:
        return ProfilePreset{
            .vblank_hz = 60,
            .fps_limit = -1,
            .output_res = 1, // 1920x1080
            .upscaler = dlss_supported ? UpscalerDlss
                : fsr4_supported       ? UpscalerFsr4
                : fsr3_validated       ? UpscalerFsr3
                                       : UpscalerOff,
            .preset = Quality,
            .frame_gen = FrameGenOff,
            .stereo_mode = StereoOff,
        };
    }
}

void EnforceProfileConstraints(int effective_profile) {
    auto& v = Get();
    if (!ProfileAllowsUpscaler(effective_profile, v.upscaler.load())) {
        v.upscaler = ProfileAllowsUpscaler(effective_profile, UpscalerFsr3) ? UpscalerFsr3 : UpscalerOff;
    }
    if (!ProfileAllowsFrameGen(effective_profile, v.frame_gen.load())) {
        v.frame_gen = FrameGenOff;
    }
    if (!ProfileAllowsFrameRate(effective_profile, v.vblank_hz.load())) {
        v.vblank_hz = (effective_profile == ProfileHandheld) ? 30 : 60;
    }
    if (!ProfileAllowsStereo(effective_profile, v.stereo_mode.load())) {
        v.stereo_mode = StereoOff;
    }
}

void ApplyProfile(int profile) {
    auto& v = Get();
    v.device_profile = std::clamp(profile, 0, ProfileCount - 1);
    const int effective = EffectiveProfile();
    const bool fsr3_ok = FsrDeviceValidated(g_detected_gpu_name.c_str()) ||
                         effective == ProfileHandheld || effective == ProfileLaptop;
    const ProfilePreset p = PresetForProfile(effective, fsr3_ok, v.fsr4_supported.load(),
                                             v.dlss_supported.load());
    v.vblank_hz = p.vblank_hz;
    v.fps_limit = p.fps_limit;
    v.output_res = std::clamp(p.output_res, 0, OutputCount - 1);
    v.upscaler = p.upscaler;
    v.upscaler_chosen = true;
    v.preset = std::clamp(p.preset, 0, PresetCount - 1);
    v.frame_gen = (p.frame_gen == FrameGenDlss && v.dlssg_max_multiplier.load() <= 1) ? FrameGenOff
                                                                                       : p.frame_gen;
    v.stereo_mode = p.stereo_mode;
    EnforceProfileConstraints(effective);
}

bool Save() {
    const auto& v = Get();
    // Written next to the file and renamed over it: a crash or a full disk while writing leaves
    // the old settings, not a cut-off file.
    const std::filesystem::path path = Path();
    std::filesystem::path temp = path;
    temp += ".tmp";
    std::error_code ec;
    if (path.has_parent_path()) {
        std::filesystem::create_directories(path.parent_path(), ec);
    }
    FILE* file = OpenFile(temp, "w");
    if (!file) {
        std::printf("Settings: cannot write %s\n", Display(temp).c_str());
        return false;
    }
    std::fprintf(file,
                 "# bbport settings (in-game menu: Insert / L3+R3)\n"
                 "settings_version=3\n"
                 "device_profile=%s\nvblank_hz=%d\nfps_limit=%d\nstereo_mode=%s\n"
                 "upscaler=%s\npreset=%d\nsharpen=%d\nsharpness=%.2f\njitter=%d\n"
                 "reactive=%d\nobject_motion=%d\nreactive_scale=%.2f\nreactive_threshold=%.2f\nreactive_max=%.2f\n"
                 "debug_view=%d\nshow_fps=%d\nfsr4_auto_exposure=%d\nfsr4_invert_jitter=%d\n",
                 ProfileName(v.device_profile), v.vblank_hz.load(), v.fps_limit.load(),
                 StereoModeName(v.stereo_mode), UpscalerName(v.upscaler), v.preset.load(),
                 int(v.sharpen.load()), v.sharpness.load(), int(v.jitter.load()),
                 int(v.reactive.load()), int(v.object_motion.load()), v.reactive_scale.load(),
                 v.reactive_threshold.load(), v.reactive_max.load(), v.debug_view.load(),
                 int(v.show_fps.load()), int(v.fsr4_auto_exposure.load()),
                 int(v.fsr4_invert_jitter.load()));
    // Read by patches.py at start.
    for (int e = 0; e < EffectCount; ++e) {
        std::fprintf(file, "%s=%d\n", Effects[e].key, int(v.effects[e].load()));
    }
    std::fprintf(file, "model_lod=%d\noutput_res=%dx%d\n", v.model_lod.load(),
                 OutputWidths[v.output_res], OutputHeights[v.output_res]);
    std::fprintf(file, "frame_gen=%s\nframe_gen_multiplier=%d\n", FrameGenName(v.frame_gen),
                 v.frame_gen_multiplier.load());
    // Read by run.sh at start.
    std::fprintf(file, "live_resolution=%s\n", v.live_resolution < 0 ? "auto"
                                                  : v.live_resolution ? "1" : "0");
    const bool wrote = std::fflush(file) == 0 && !std::ferror(file);
    const bool closed = std::fclose(file) == 0;
    if (!wrote || !closed) {
        std::filesystem::remove(temp, ec);
        std::printf("Settings: cannot write %s\n", Display(temp).c_str());
        return false;
    }
    // On Windows a virus scanner or the indexer can hold the new file (or the old one) for a
    // moment; the rename is tried a few times before the save is given up.
    for (int attempt = 0;; ++attempt) {
        std::filesystem::rename(temp, path, ec);
        if (!ec || attempt == 4) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    if (ec) {
        std::filesystem::remove(temp, ec);
        std::printf("Settings: cannot replace %s\n", Display(path).c_str());
        return false;
    }
    return true;
}

float PresetScale(int preset) {
    static constexpr float scales[PresetCount] = {1.0f, 1.5f, 1.7f, 2.0f, 3.0f};
    return scales[std::clamp(preset, 0, PresetCount - 1)];
}

const char* PresetName(int preset) {
    static constexpr const char* names[PresetCount] = {"Native AA", "Quality", "Balanced",
                                                       "Performance", "Ultra Performance"};
    return names[std::clamp(preset, 0, PresetCount - 1)];
}

const char* UpscalerName(int upscaler) {
    static constexpr const char* names[UpscalerCount] = {"off", "fsr3", "fsr4", "fsr411", "taa", "dlss", "xess"};
    return names[std::clamp(upscaler, 0, UpscalerCount - 1)];
}

const char* FrameGenName(int frame_gen) {
    static constexpr const char* names[FrameGenCount] = {"off", "fsr3", "dlss"};
    return names[std::clamp(frame_gen, 0, FrameGenCount - 1)];
}

const char* ProfileName(int profile) {
    static constexpr const char* names[ProfileCount] = {"auto", "handheld", "laptop", "desktop", "3d"};
    return names[std::clamp(profile, 0, ProfileCount - 1)];
}

const char* ProfileLabel(int profile) {
    static constexpr const char* labels[ProfileCount] = {
        "Auto", "Handheld", "Laptop (Strix Halo)", "Desktop", "3D (Uncharted 3 stereo)"};
    return labels[std::clamp(profile, 0, ProfileCount - 1)];
}

int ParseStereoMode(const char* value) {
    if (!value || !value[0]) {
        return StereoOff;
    }
    std::string lower;
    for (const char* p = value; *p; ++p) {
        lower.push_back(static_cast<char>(std::tolower(static_cast<unsigned char>(*p))));
    }
    if (lower == "passthrough" || lower == "sequential" || lower == "frame-sequential" ||
        lower == "alternate" || lower == "1") {
        return StereoPassthrough;
    }
    if (lower == "side-by-side" || lower == "sbs" || lower == "left-right" || lower == "lr" ||
        lower == "2") {
        return StereoSideBySide;
    }
    if (lower == "top-bottom" || lower == "tb" || lower == "over-under" || lower == "ou" ||
        lower == "3") {
        return StereoTopBottom;
    }
    if (lower == "anaglyph" || lower == "red-cyan" || lower == "4") {
        return StereoAnaglyph;
    }
    return StereoOff;
}

const char* StereoModeName(int stereo_mode) {
    static constexpr const char* names[StereoModeCount] = {"off", "passthrough", "side-by-side",
                                                           "top-bottom", "anaglyph"};
    return names[std::clamp(stereo_mode, 0, StereoModeCount - 1)];
}

const char* StereoModeLabel(int mode) {
    static constexpr const char* labels[StereoModeCount] = {
        "Off", "Passthrough (Sequential)", "Side-by-Side (SBS)", "Top-Bottom (TB)",
        "Anaglyph (Red/Cyan)"};
    return labels[std::clamp(mode, 0, StereoModeCount - 1)];
}

bool FsrDeviceValidated(const char* device) {
    // Device names as the driver reports them (VkPhysicalDeviceProperties::deviceName). Driver
    // ids can't tell these apart: RADV is one id for every AMD GPU. Each entry was measured clean
    // with FSR 3.1 on (U3, the London pub): PHOENIX (Ryzen Z1 Extreme, Legion Go) once FSR-Vulkan
    // patch 0004 keeps FSR out of protected memory; STRIX_HALO (Radeon 8060S, Z13). 2026-10-19.
    static constexpr const char* validated[] = {"(RADV PHOENIX)", "(RADV STRIX_HALO)"};
    if (!device) return false;
    const std::string_view name{device};
    for (const char* v : validated) {
        if (name.find(v) != std::string_view::npos) return true;
    }
    return false;
}

bool FsrDriverValidated(unsigned driver_id) {
    static constexpr unsigned validated[] = {0}; // VkDriverId values; 0 is no driver
    for (const unsigned id : validated) {
        if (id != 0 && id == driver_id) return true;
    }
    return false;
}

} // namespace BbSettings
