// SPDX-License-Identifier: GPL-2.0-or-later
// Unit tests for Issue #10: per-device graphics profiles, constraints, persistence, and restart flags.
#include <cassert>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <string>
#include "bbport_settings.h"

static void SetEnv(const char* key, const char* value) {
#ifdef _WIN32
    if (_putenv_s(key, value) != 0) std::abort();
#else
    if (setenv(key, value, 1) != 0) std::abort();
#endif
}

static void UnsetEnv(const char* key) {
#ifdef _WIN32
    if (_putenv_s(key, "") != 0) std::abort();
#else
    if (unsetenv(key) != 0) std::abort();
#endif
}

static void ClearMenuEnv() {
    for (const char* k : {"BB_DEVICE_PROFILE", "BB_VBLANK_HZ", "BB_FPS_LIMIT", "BB_STEREO_MODE",
                          "BB_UPSCALER", "BB_UPSCALE_PRESET", "BB_FRAME_GEN", "BB_RENDER_RES"}) {
        UnsetEnv(k);
    }
}

int main() {
    using namespace BbSettings;

    // 1. Device profile auto-detection.
    assert(DetectDeviceProfile("AMD Radeon 8060S Graphics (RADV STRIX_HALO)", "", "", "") ==
           ProfileLaptop);
    assert(DetectDeviceProfile("", "ROG Flow Z13 GZ302EA", "", "") == ProfileLaptop);
    assert(DetectDeviceProfile("", "", "AMD RYZEN AI MAX+ 395 w/ Radeon 8060S", "") ==
           ProfileLaptop);
    assert(DetectDeviceProfile("AMD Radeon 780M Graphics (RADV PHOENIX)", "", "", "") ==
           ProfileHandheld);
    assert(DetectDeviceProfile("AMD Custom GPU 0405 (RADV VANGOGH)", "", "", "") ==
           ProfileHandheld);
    assert(DetectDeviceProfile("", "", "AMD Ryzen Z1 Extreme", "") == ProfileHandheld);
    assert(DetectDeviceProfile("", "", "AMD Custom APU 0932", "") == ProfileHandheld);
    assert(DetectDeviceProfile("", "", "Some APU", "NAME=\"SteamOS\"\nID=steamos\n") ==
           ProfileHandheld);
    assert(DetectDeviceProfile("", "", "Some APU", "ID_LIKE=steamos\n") == ProfileDesktop);
    assert(DetectDeviceProfile("NVIDIA GeForce RTX 4090", "", "Intel Core i9-13900K", "") ==
           ProfileDesktop);

    // 2. Per-device constraints.
    assert(!ProfileAllowsUpscaler(ProfileHandheld, UpscalerFsr4));
    assert(!ProfileAllowsUpscaler(ProfileHandheld, UpscalerFsr411));
    assert(!ProfileAllowsUpscaler(ProfileHandheld, UpscalerDlss));
    assert(ProfileAllowsUpscaler(ProfileHandheld, UpscalerOff));
    assert(ProfileAllowsUpscaler(ProfileHandheld, UpscalerFsr3));
    assert(ProfileAllowsUpscaler(ProfileHandheld, UpscalerTaa));
    assert(!ProfileAllowsFrameGen(ProfileHandheld, FrameGenDlss));
    assert(ProfileAllowsFrameGen(ProfileHandheld, FrameGenFsr3));
    assert(ProfileAllowsFrameRate(ProfileHandheld, 30));
    assert(ProfileAllowsFrameRate(ProfileHandheld, 45));
    assert(ProfileAllowsFrameRate(ProfileHandheld, 60));
    assert(!ProfileAllowsFrameRate(ProfileHandheld, 120));
    assert(!ProfileAllowsFrameRate(ProfileHandheld, 0));
    assert(ProfileAllowsStereo(ProfileHandheld, StereoOff));
    assert(!ProfileAllowsStereo(ProfileHandheld, StereoSideBySide));
    assert(!ProfileAllowsStereo(ProfileHandheld, StereoTopBottom));

    assert(ProfileAllowsUpscaler(ProfileLaptop, UpscalerFsr4));
    assert(ProfileAllowsFrameRate(ProfileLaptop, 0));
    assert(ProfileAllowsFrameRate(ProfileLaptop, 120));
    assert(ProfileAllowsStereo(ProfileLaptop, StereoSideBySide));

    assert(ProfileAllowsFrameRate(ProfileStereo3D, 30));
    assert(ProfileAllowsFrameRate(ProfileStereo3D, 60));
    assert(!ProfileAllowsFrameRate(ProfileStereo3D, 45));
    assert(!ProfileAllowsFrameRate(ProfileStereo3D, 0));
    assert(ProfileAllowsStereo(ProfileStereo3D, StereoSideBySide));
    assert(ProfileAllowsStereo(ProfileStereo3D, StereoAnaglyph));

    // 3. Profile presets and ApplyProfile.
    const auto ts = std::chrono::steady_clock::now().time_since_epoch().count();
    auto path = (std::filesystem::temp_directory_path() /
                 ("bbport-gfx-menu-test-" + std::to_string(ts) + ".ini"))
                    .string();
    FILE* f = std::fopen(path.c_str(), "w");
    if (!f) {
        path = "bbport-gfx-menu-test-" + std::to_string(ts) + ".ini";
        f = std::fopen(path.c_str(), "w");
    }
    assert(f);
    std::fclose(f);
    SetEnv("BB_CONFIG", path.c_str());
    ClearMenuEnv();

    auto& s = Get();
    Load();

    ApplyProfile(ProfileHandheld);
    assert(EffectiveProfile() == ProfileHandheld);
    assert(s.vblank_hz == 30);
    assert(s.output_res == 0); // 1280x720
    assert(s.upscaler == UpscalerOff);
    assert(s.stereo_mode == StereoOff);

    s.fsr4_supported = true;
    ApplyProfile(ProfileLaptop);
    assert(EffectiveProfile() == ProfileLaptop);
    assert(s.vblank_hz == 60);
    assert(s.output_res == 2); // 2560x1440
    assert(s.upscaler == UpscalerFsr4);
    assert(s.stereo_mode == StereoOff);

    ApplyProfile(ProfileStereo3D);
    assert(EffectiveProfile() == ProfileStereo3D);
    assert(s.vblank_hz == 60);
    assert(s.output_res == 1); // 1920x1080
    assert(s.stereo_mode == StereoSideBySide);

    // 4. EnforceProfileConstraints clamps disallowed values on Handheld.
    s.device_profile = ProfileHandheld;
    s.upscaler = UpscalerFsr4;
    s.frame_gen = FrameGenDlss;
    s.vblank_hz = 120;
    s.stereo_mode = StereoSideBySide;
    EnforceProfileConstraints(ProfileHandheld);
    assert(s.upscaler == UpscalerFsr3);
    assert(s.frame_gen == FrameGenOff);
    assert(s.vblank_hz == 30);
    assert(s.stereo_mode == StereoOff);

    // 5. Persistence round-trip via bbport.ini.
    s.device_profile = ProfileLaptop;
    s.vblank_hz = 45;
    s.fps_limit = 90;
    s.output_res = 2; // 2560x1440
    s.upscaler = UpscalerFsr4;
    s.preset = Quality;
    s.stereo_mode = StereoTopBottom;
    Save();

    // Clobber in-memory state and reload.
    s.device_profile = ProfileHandheld;
    s.vblank_hz = 30;
    s.fps_limit = 0;
    s.output_res = 0;
    s.upscaler = UpscalerOff;
    s.preset = UltraPerformance;
    s.stereo_mode = StereoOff;
    Load();
    assert(s.device_profile == ProfileLaptop);
    assert(s.vblank_hz == 45);
    assert(s.fps_limit == 90);
    assert(s.output_res == 2);
    assert(s.upscaler == UpscalerFsr4);
    assert(s.preset == Quality);
    assert(s.stereo_mode == StereoTopBottom);
    assert(!FrameRateNeedsRestart());
    assert(!StereoNeedsRestart());

    // 6. Restart-required detection.
    s.vblank_hz = 60;
    assert(FrameRateNeedsRestart());
    s.vblank_hz = 45;
    assert(!FrameRateNeedsRestart());
    s.fps_limit = 60;
    assert(FrameRateNeedsRestart());
    s.fps_limit = 90;
    assert(!FrameRateNeedsRestart());
    s.stereo_mode = StereoAnaglyph;
    assert(StereoNeedsRestart());
    s.stereo_mode = StereoTopBottom;
    assert(!StereoNeedsRestart());

    // 7. Environment overrides and profile constraint enforcement on Load().
    SetEnv("BB_DEVICE_PROFILE", "amd-handheld");
    SetEnv("BB_VBLANK_HZ", "60");
    SetEnv("BB_FPS_LIMIT", "0");
    SetEnv("BB_STEREO_MODE", "sbs"); // disallowed on Handheld -> clamped to Off
    Load();
    assert(s.device_profile == ProfileHandheld);
    assert(s.vblank_hz == 60);
    assert(s.fps_limit == 0);
    assert(s.stereo_mode == StereoOff);
    assert(s.upscaler == UpscalerFsr3); // FSR 4 from ini clamped to FSR 3.1 on Handheld

    ClearMenuEnv();
    UnsetEnv("BB_CONFIG");
    std::filesystem::remove(path);
    std::printf("gfx-menu-profiles-test: all checks passed\n");
    return 0;
}
