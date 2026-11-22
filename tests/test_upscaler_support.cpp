// SPDX-License-Identifier: GPL-2.0-or-later
// Saved FSR 4 choices must be safe before the first rendered frame on older GPUs.
#include <cassert>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <initializer_list>
#include <string>
#include <string_view>
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

int main() {
    using namespace BbSettings;
    // No driver is validated for FSR 3.1 yet (RADV = 3, NVIDIA proprietary = 4, none = 0).
    for (unsigned id : {0u, 1u, 3u, 4u, 27u}) assert(!FsrDriverValidated(id));
    const auto timestamp = std::chrono::steady_clock::now().time_since_epoch().count();
    const auto name = "bbport-upscaler-test-" + std::to_string(timestamp);
    // An MSYS2 shell exports TMP=/tmp, which Windows cannot open: fall back to the working directory.
    auto path = (std::filesystem::temp_directory_path() / name).string();
    FILE* created = std::fopen(path.c_str(), "w");
    if (!created) {
        path = name;
        created = std::fopen(path.c_str(), "w");
    }
    assert(created);
    std::fclose(created);
    SetEnv("BB_CONFIG", path.c_str());
    UnsetEnv("BB_UPSCALER");
    UnsetEnv("BB_UPSCALE_PRESET");
    UnsetEnv("BB_RENDER_RES");
    auto& s = Get();
    s.upscaler = UpscalerTaa;
    s.preset = Performance;
    assert(RenderPreset() == NativeAA && s.preset == Performance);
    Save();
    s.upscaler = UpscalerOff;
    Load();
    assert(s.upscaler == UpscalerTaa && s.preset == Performance);
    assert(RenderPreset() == NativeAA && !ResolutionNeedsRestart());
    s.upscaler = UpscalerFsr3;
    assert(RenderPreset() == Performance);

    // The settings layer hides an unavailable DLSS provider and falls back from a saved DLSS
    // choice. This covers settings behavior; it does not exercise the NGX query or Vulkan device
    // creation path. A successful DLSS probe leaves the selected provider unchanged.
    s.upscaler = UpscalerDlss;
    ConfigureDlssSupport(true, nullptr);
    assert(s.dlss_supported && s.upscaler == UpscalerDlss);
    s.upscaler = UpscalerDlss;
    ConfigureDlssSupport(false, "feature requirement query failed");
    assert(!s.dlss_supported && s.upscaler == UpscalerFsr3);
    assert(s.dlss_problem && std::string_view{s.dlss_problem.load()} ==
                                 "feature requirement query failed");
    // If the player chose TAA, an unavailable optional DLSS provider must not override that
    // explicit choice.
    s.upscaler = UpscalerTaa;
    ConfigureDlssSupport(false, "feature requirement query failed");
    assert(!s.dlss_supported && s.upscaler == UpscalerTaa);

    // XeSS availability and fallback match DLSS.
    s.upscaler = UpscalerXess;
    ConfigureXessSupport(true, nullptr);
    assert(s.xess_supported && s.upscaler == UpscalerXess);
    ConfigureXessSupport(false, "libxess.dll missing");
    assert(!s.xess_supported && s.upscaler == UpscalerFsr3);
    assert(s.xess_problem && std::string_view{s.xess_problem.load()} == "libxess.dll missing");

    // FSR 4 assets missing on an otherwise capable GPU hides FSR 4 and falls back to FSR 3.1.
    s.upscaler = UpscalerFsr4;
    s.fsr4_problem = nullptr;
    ConfigureUpscalerSupport(true, false, false);
    assert(!s.fsr4_supported && s.upscaler == UpscalerFsr3);
    assert(s.fsr4_problem && std::string_view{s.fsr4_problem.load()}.find("assets not installed") !=
                                 std::string_view::npos);

    // RADV driver allow-list & default guard (issue #114): only devices measured clean after the
    // protected-memory fix keep FSR 3.1 on by default; other GPUs (including PHOENIX2, NAVI31,
    // or non-RADV drivers) turn the default off unless the player explicitly chose FSR 3.1.
    assert(FsrDeviceValidated("AMD Radeon 780M Graphics (RADV PHOENIX), driver 26.2.0"));
    assert(FsrDeviceValidated("AMD Radeon 8060S Graphics (RADV STRIX_HALO), driver 26.2.0"));
    assert(!FsrDeviceValidated("AMD Radeon 740M Graphics (RADV PHOENIX2), driver 26.2.0"));
    assert(!FsrDeviceValidated("AMD Radeon RX 7900 XTX (RADV NAVI31), driver 26.2.0"));
    assert(!FsrDeviceValidated("AMD Radeon(TM) 780M Graphics, driver 24.10.1"));
    assert(!FsrDeviceValidated(nullptr));
    assert(DriverDefaultUpscaler(UpscalerFsr3, false, false) == UpscalerOff);
    assert(DriverDefaultUpscaler(UpscalerFsr3, false, true) == UpscalerFsr3);
    assert(DriverDefaultUpscaler(UpscalerFsr3, true, false) == UpscalerFsr3);

    {
        FILE* config = std::fopen(path.c_str(), "w");
        assert(config);
        std::fprintf(config, "preset=0\n");
        std::fclose(config);
        s.upscaler = UpscalerFsr3;
        s.startup_upscaler = UpscalerFsr3;
        s.upscaler_chosen = false;
        Load();
        assert(!s.upscaler_chosen && s.upscaler == UpscalerFsr3);
        ConfigureDriverDefault(3 /* VK_DRIVER_ID_MESA_RADV */,
                               "AMD Radeon 780M Graphics (RADV PHOENIX), driver 26.2.0");
        assert(s.upscaler == UpscalerFsr3 && s.startup_upscaler == UpscalerFsr3);
        ConfigureDriverDefault(3, "AMD Radeon 740M Graphics (RADV PHOENIX2), driver 26.2.0");
        assert(s.upscaler == UpscalerOff && s.startup_upscaler == UpscalerOff);

        // Null driver_name must not crash and must treat the device as unvalidated.
        s.upscaler = UpscalerFsr3;
        s.startup_upscaler = UpscalerFsr3;
        ConfigureDriverDefault(3, nullptr);
        assert(s.upscaler == UpscalerOff && s.startup_upscaler == UpscalerOff);

        // Explicit player choice in ini survives on unvalidated drivers.
        config = std::fopen(path.c_str(), "w");
        assert(config);
        std::fprintf(config, "upscaler=fsr3\n");
        std::fclose(config);
        Load();
        assert(s.upscaler_chosen && s.upscaler == UpscalerFsr3);
        ConfigureDriverDefault(3, "AMD Radeon RX 7900 XTX (RADV NAVI31), driver 26.2.0");
        assert(s.upscaler == UpscalerFsr3);
    }

    for (bool fsr4 : {false, true}) {
        for (bool fsr411 : {false, true}) {
            for (int requested = 0; requested < UpscalerCount; ++requested) {
                FILE* config = std::fopen(path.c_str(), "w");
                assert(config);
                std::fprintf(config, "upscaler=%s\npreset=3\noutput_res=2560x1440\n",
                             UpscalerName(requested));
                std::fclose(config);
                s.fsr4_problem = nullptr;
                Load();
                assert(s.upscaler == requested);
                ConfigureUpscalerSupport(fsr4, fsr411);
                const bool unsupported = (requested == UpscalerFsr4 && !fsr4) ||
                    (requested == UpscalerFsr411 && !(fsr4 && fsr411));
                assert(s.upscaler == (unsupported ? UpscalerFsr3 : requested));
                assert(s.fsr4_supported == fsr4);
                assert(s.fsr411_supported == (fsr4 && fsr411));
                assert(bool(s.fsr4_problem.load()) == unsupported);
                assert(s.preset == Performance && s.output_res == 2);
                // Startup patch settings still describe the already applied guest patches.
                assert(s.startup_upscaler == requested);
                ConfigureUpscalerSupport(fsr4, fsr411);
                assert(s.upscaler == (unsupported ? UpscalerFsr3 : requested));
            }
        }
    }
    s.startup_preset = Quality;
    s.startup_upscaler = UpscalerFsr3;
    s.upscaler = UpscalerFsr3;
    s.preset = Performance;
    assert(RenderPreset() == Performance && !ResolutionNeedsRestart());
    SetEnv("BB_RENDER_RES", "1706x960");
    assert(RenderPreset() == Quality && ResolutionNeedsRestart());
    s.preset = Quality;
    assert(!ResolutionNeedsRestart());
    s.upscaler = UpscalerOff;
    assert(ResolutionNeedsRestart());
    s.upscaler = UpscalerFsr4;
    assert(!ResolutionNeedsRestart()); // provider change at the same patched size is live
    s.upscaler = UpscalerTaa;          // TAA needs native guest targets
    assert(ResolutionNeedsRestart());
    s.upscaler = UpscalerFsr4;
    const int output = s.startup_output_res;
    s.output_res = (output + 1) % OutputCount;
    assert(ResolutionNeedsRestart());
    s.output_res = output;
    SetEnv("BB_RENDER_RES", "");
    assert(!FixedRenderSession() && !ResolutionNeedsRestart());
    UnsetEnv("BB_RENDER_RES");
    std::remove(path.c_str());
    std::puts("Upscaler support: saved choices and FSR 3.1 fallback PASS");
}
