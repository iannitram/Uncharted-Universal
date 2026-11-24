// SPDX-License-Identifier: GPL-2.0-or-later
// Issue #153: the overlay's settings survive a restart. Every setting the menu changes goes
// through Save() into bbport.ini and comes back through Load(); the write is atomic (no temp
// file left, an unwritable target keeps the old file); and on Windows BB_CONFIG may name a folder
// outside the ANSI code page (a user name with non-Latin letters).
#include <cstdio>
#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>

#include "bbport_settings.h"

namespace fs = std::filesystem;
using namespace BbSettings;

static int failures = 0;
static void Check(bool ok, const char* what) {
    std::printf("%s: %s\n", ok ? "ok" : "FAIL", what);
    if (!ok) ++failures;
}

static void SetConfig(const fs::path& path) {
#ifdef _WIN32
    _wputenv_s(L"BB_CONFIG", path.c_str());
#else
    setenv("BB_CONFIG", path.c_str(), 1);
#endif
}

static void UnsetVar(const char* name) {
#ifdef _WIN32
    _putenv_s(name, ""); // an empty value removes the variable
#else
    unsetenv(name);
#endif
}

static void ClearEnv() {
    for (const char* name : {"BB_UPSCALER", "BB_FSR_SHARPNESS", "BB_JITTER", "BB_REACTIVE",
                             "BB_REACTIVE_SCALE", "BB_REACTIVE_THRESHOLD", "BB_REACTIVE_MAX",
                             "BB_UPSCALE_PRESET", "BB_OBJECT_MOTION", "BB_FRAME_GEN",
                             "BB_FRAME_GEN_MULTIPLIER", "BB_DEVICE_PROFILE", "BB_VBLANK_HZ",
                             "BB_FPS_LIMIT", "BB_STEREO_MODE"}) {
        UnsetVar(name);
    }
}

static std::string ReadAll(const fs::path& path) {
    std::ifstream in(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()};
}

static fs::path WithSuffix(fs::path path, const char* suffix) {
    path += suffix;
    return path;
}

// What the menu can change, set to values that differ from the defaults.
static void SetMenuValues(Values& s) {
    s.show_fps = true;
    s.sharpen = false;
    s.sharpness = 0.75f;
    s.jitter = true;
    s.reactive = true;
    s.object_motion = false;
    s.reactive_scale = 2.5f;
    s.reactive_threshold = 0.35f;
    s.reactive_max = 0.6f;
    s.debug_view = DebugMotion;
    s.fsr4_auto_exposure = false;
    s.fsr4_invert_jitter = true;
    s.upscaler = UpscalerTaa;
    s.preset = Balanced;
    s.frame_gen = FrameGenFsr3;
    s.frame_gen_multiplier = 3;
    s.device_profile = ProfileDesktop; // #10
    s.vblank_hz = 120;
    s.fps_limit = 90;
    s.stereo_mode = StereoAnaglyph; // #19
    s.output_res = 3;
    s.model_lod = -1;
    s.live_resolution = -1;
    for (int e = 0; e < EffectCount; ++e) s.effects[e] = !Effects[e].default_on;
}

static void ScrambleMenuValues(Values& s) {
    s.show_fps = false;
    s.sharpen = true;
    s.sharpness = 0.1f;
    s.jitter = false;
    s.reactive = false;
    s.object_motion = true;
    s.reactive_scale = 1.0f;
    s.reactive_threshold = 0.2f;
    s.reactive_max = 0.9f;
    s.debug_view = DebugNone;
    s.fsr4_auto_exposure = true;
    s.fsr4_invert_jitter = false;
    s.upscaler = UpscalerOff;
    s.preset = NativeAA;
    s.frame_gen = FrameGenOff;
    s.frame_gen_multiplier = 2;
    s.device_profile = ProfileAuto;
    s.vblank_hz = 60;
    s.fps_limit = -1;
    s.stereo_mode = StereoOff;
    s.output_res = 1;
    s.model_lod = 0;
    s.live_resolution = 0;
    for (int e = 0; e < EffectCount; ++e) s.effects[e] = Effects[e].default_on;
}

static void CheckMenuValues(const Values& s, const char* when) {
    std::printf("-- %s\n", when);
    Check(s.show_fps, "FPS counter checkbox survives");
    Check(!s.sharpen, "sharpen checkbox survives");
    Check(s.sharpness == 0.75f, "sharpness slider survives");
    Check(s.jitter && s.reactive && !s.object_motion, "jitter, reactive, object motion survive");
    Check(s.reactive_scale == 2.5f && s.reactive_threshold == 0.35f && s.reactive_max == 0.6f,
          "reactive sliders survive");
    Check(s.debug_view == DebugMotion, "debug view survives");
    Check(!s.fsr4_auto_exposure && s.fsr4_invert_jitter, "FSR 4 checks survive");
    Check(s.upscaler == UpscalerTaa && s.preset == Balanced, "upscaler and preset survive");
    Check(s.frame_gen == FrameGenFsr3 && s.frame_gen_multiplier == 3, "frame generation survives");
    Check(s.device_profile == ProfileDesktop, "per-device profile (#10) survives");
    Check(s.vblank_hz == 120 && s.fps_limit == 90, "target frame rate and cap (#10) survive");
    Check(s.stereo_mode == StereoAnaglyph, "stereo mode (#19) survives");
    Check(s.output_res == 3 && s.model_lod == -1 && s.live_resolution == -1,
          "output resolution, model LOD and live resolution survive");
    bool effects = true;
    for (int e = 0; e < EffectCount; ++e) {
        effects = effects && s.effects[e] == !Effects[e].default_on;
    }
    Check(effects, "every game effect switch survives");
}

int main() {
    const fs::path root =
        fs::temp_directory_path() / ("bbport-roundtrip-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    // A folder with letters outside every ANSI code page: a Windows user name can have them.
    const fs::path dir = root / fs::path(std::u8string(u8"пример-テスト"));
    fs::create_directories(dir);
    const fs::path ini = dir / "bbport.ini";
    ClearEnv();
    SetConfig(ini);
    Check(ConfigPath() == ini, "BB_CONFIG is read back whole, non-ANSI folder name included");

    auto& s = Get();
    Load();
    Check(!fs::exists(ini), "Load() of a missing file does not create it");
    SetMenuValues(s);
    Check(Save(), "Save() succeeds");
    Check(fs::exists(ini), "settings file is written at BB_CONFIG");
    Check(!fs::exists(WithSuffix(ini, ".tmp")), "no temp file is left after a save");

    ScrambleMenuValues(s);
    Load(); // a restart
    CheckMenuValues(s, "after a restart (Load)");

    // The next save replaces the file in place; a second restart still reads the new values.
    s.sharpness = 1.25f;
    s.show_fps = false;
    Check(Save(), "a second Save() replaces the file");
    Check(!fs::exists(WithSuffix(ini, ".tmp")), "no temp file after the second save");
    Load();
    Check(s.sharpness == 1.25f && !s.show_fps, "the second save wins");

    // An unwritable target keeps the old file whole.
    const std::string before = ReadAll(ini);
    const fs::path blocked = dir / "blocked" / "bbport.ini";
    fs::create_directories(blocked); // a folder where the file should go
    SetConfig(blocked);
    Check(!Save(), "Save() reports a target it cannot replace");
    Check(!fs::exists(WithSuffix(blocked, ".tmp")), "the failed save leaves no temp file");
    SetConfig(ini);
    Check(ReadAll(ini) == before, "the old settings file is untouched");

    // A missing folder is created.
    const fs::path fresh = root / "new" / "folder" / "bbport.ini";
    SetConfig(fresh);
    Check(Save() && fs::exists(fresh), "Save() creates the settings folder");

    // Windows editors write CRLF line ends: Load() still reads them.
    SetConfig(ini);
    {
        const std::string text = ReadAll(ini);
        std::string crlf;
        for (char c : text) {
            if (c == '\n') crlf += '\r';
            crlf += c;
        }
        std::ofstream out(ini, std::ios::binary | std::ios::trunc);
        out << crlf;
    }
    s.sharpness = 0.0f;
    Load();
    Check(s.sharpness == 1.25f, "a CRLF settings file loads");

    ClearEnv();
    UnsetVar("BB_CONFIG");
    std::error_code ec;
    fs::remove_all(root, ec);
    if (failures) {
        std::printf("settings-roundtrip-test: %d FAILED\n", failures);
    } else {
        std::printf("settings-roundtrip-test: all checks passed\n");
    }
    return failures ? 1 : 0;
}
