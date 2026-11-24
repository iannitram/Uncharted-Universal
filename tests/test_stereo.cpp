// SPDX-License-Identifier: GPL-2.0-or-later
#include "gpu/shim/bbport_settings.h"
#include "gpu/shim/bbport_stereo.h"

#include <cassert>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

#ifdef _WIN32
static void set_env(const char* name, const char* value) {
    _putenv_s(name, value ? value : "");
}
#else
static void set_env(const char* name, const char* value) {
    if (value) {
        setenv(name, value, 1);
    } else {
        unsetenv(name);
    }
}
#endif

static void test_parse_stereo_mode() {
    assert(BbSettings::ParseStereoMode(nullptr) == BbSettings::StereoOff);
    assert(BbSettings::ParseStereoMode("") == BbSettings::StereoOff);
    assert(BbSettings::ParseStereoMode("off") == BbSettings::StereoOff);
    assert(BbSettings::ParseStereoMode("0") == BbSettings::StereoOff);
    assert(BbSettings::ParseStereoMode("unknown") == BbSettings::StereoOff);

    assert(BbSettings::ParseStereoMode("passthrough") == BbSettings::StereoPassthrough);
    assert(BbSettings::ParseStereoMode("sequential") == BbSettings::StereoPassthrough);
    assert(BbSettings::ParseStereoMode("frame-sequential") == BbSettings::StereoPassthrough);
    assert(BbSettings::ParseStereoMode("alternate") == BbSettings::StereoPassthrough);
    assert(BbSettings::ParseStereoMode("1") == BbSettings::StereoPassthrough);
    assert(BbSettings::ParseStereoMode("PASSTHROUGH") == BbSettings::StereoPassthrough);

    assert(BbSettings::ParseStereoMode("side-by-side") == BbSettings::StereoSideBySide);
    assert(BbSettings::ParseStereoMode("sbs") == BbSettings::StereoSideBySide);
    assert(BbSettings::ParseStereoMode("left-right") == BbSettings::StereoSideBySide);
    assert(BbSettings::ParseStereoMode("lr") == BbSettings::StereoSideBySide);
    assert(BbSettings::ParseStereoMode("2") == BbSettings::StereoSideBySide);
    assert(BbSettings::ParseStereoMode("SBS") == BbSettings::StereoSideBySide);

    assert(BbSettings::ParseStereoMode("top-bottom") == BbSettings::StereoTopBottom);
    assert(BbSettings::ParseStereoMode("tb") == BbSettings::StereoTopBottom);
    assert(BbSettings::ParseStereoMode("over-under") == BbSettings::StereoTopBottom);
    assert(BbSettings::ParseStereoMode("ou") == BbSettings::StereoTopBottom);
    assert(BbSettings::ParseStereoMode("3") == BbSettings::StereoTopBottom);
    assert(BbSettings::ParseStereoMode("Top-Bottom") == BbSettings::StereoTopBottom);
}

static void test_stereo_mode_names() {
    assert(std::strcmp(BbSettings::StereoModeName(BbSettings::StereoOff), "off") == 0);
    assert(std::strcmp(BbSettings::StereoModeName(BbSettings::StereoPassthrough), "passthrough") == 0);
    assert(std::strcmp(BbSettings::StereoModeName(BbSettings::StereoSideBySide), "side-by-side") == 0);
    assert(std::strcmp(BbSettings::StereoModeName(BbSettings::StereoTopBottom), "top-bottom") == 0);
    assert(std::strcmp(BbSettings::StereoModeName(-1), "off") == 0);
    assert(std::strcmp(BbSettings::StereoModeName(999), "anaglyph") == 0);
}

static void test_stereo_env_and_ini_roundtrip() {
    const std::string ini_path = "test_stereo_settings.ini";
    std::remove(ini_path.c_str());

    set_env("BB_CONFIG", ini_path.c_str());
    set_env("BB_STEREO_MODE", "side-by-side");
    BbSettings::Load();
    assert(BbSettings::Get().stereo_mode.load() == BbSettings::StereoSideBySide);
    BbSettings::Save();

    set_env("BB_STEREO_MODE", nullptr);
    BbSettings::Get().stereo_mode = BbSettings::StereoOff;
    BbSettings::Load();
    assert(BbSettings::Get().stereo_mode.load() == BbSettings::StereoSideBySide);

    set_env("BB_STEREO_MODE", "top-bottom");
    BbSettings::Load();
    assert(BbSettings::Get().stereo_mode.load() == BbSettings::StereoTopBottom);

    set_env("BB_STEREO_MODE", nullptr);
    set_env("BB_CONFIG", nullptr);
    std::remove(ini_path.c_str());
}

static void test_stereo_eye_rects() {
    assert(!BbStereo::IsComposed(BbSettings::StereoOff));
    assert(!BbStereo::IsComposed(BbSettings::StereoPassthrough));
    assert(BbStereo::IsComposed(BbSettings::StereoSideBySide));
    assert(BbStereo::IsComposed(BbSettings::StereoTopBottom));

    // Off and Passthrough keep full fitted rect for both eyes.
    const BbStereo::Rect full = BbStereo::EyeRect(BbSettings::StereoPassthrough, 1, 10, 20, 1920, 1080);
    assert(full.x == 10 && full.y == 20 && full.width == 1920 && full.height == 1080);

    // Side-by-side splits horizontally, preserving letterbox offset and odd widths.
    const BbStereo::Rect sbs_l = BbStereo::EyeRect(BbSettings::StereoSideBySide, 0, 0, 80, 1921, 1080);
    const BbStereo::Rect sbs_r = BbStereo::EyeRect(BbSettings::StereoSideBySide, 1, 0, 80, 1921, 1080);
    assert(sbs_l.x == 0 && sbs_l.y == 80 && sbs_l.width == 960 && sbs_l.height == 1080);
    assert(sbs_r.x == 960 && sbs_r.y == 80 && sbs_r.width == 961 && sbs_r.height == 1080);

    // Top-bottom splits vertically, preserving pillarbox offset and odd heights.
    const BbStereo::Rect tb_l = BbStereo::EyeRect(BbSettings::StereoTopBottom, 0, 160, 0, 1600, 1081);
    const BbStereo::Rect tb_r = BbStereo::EyeRect(BbSettings::StereoTopBottom, 1, 160, 0, 1600, 1081);
    assert(tb_l.x == 160 && tb_l.y == 0 && tb_l.width == 1600 && tb_l.height == 540);
    assert(tb_r.x == 160 && tb_r.y == 540 && tb_r.width == 1600 && tb_r.height == 541);
}

int main() {
    test_parse_stereo_mode();
    test_stereo_mode_names();
    test_stereo_env_and_ini_roundtrip();
    test_stereo_eye_rects();
    std::printf("test_stereo: ok\n");
    return 0;
}
