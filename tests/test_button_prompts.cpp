// Unit test for ButtonPrompts (roadmap 3.23, #439):
// - Builds the ps4_sheet icon sheet for Keyboard, Xbox, and Nintendo styles and verifies distinct
//   output, and Original returns an empty patch list.
// - Verifies BB_BUTTON_PROMPTS=nintendo overrides SetDevice(1) (PlayStation).
#include <cassert>
#include <cstdlib>
#include <cstring>
#include <vector>

#include "video_core/texture_cache/button_prompts.h"

#ifndef TEST_LINK_BBGPU
#ifdef BB_FONT_PATH
asm(".section .rodata\n"
    ".balign 16\n"
    ".global bb_font_ttf\n"
    "bb_font_ttf:\n"
    ".incbin \"" BB_FONT_PATH "\"\n"
    ".previous\n");
#else
extern "C" const unsigned char bb_font_ttf[1] = {0};
#endif
#endif

using VideoCore::ButtonPrompts::Find;
using VideoCore::ButtonPrompts::Format;
using VideoCore::ButtonPrompts::Patch;
using VideoCore::ButtonPrompts::SetDevice;
using VideoCore::ButtonPrompts::Shown;
using VideoCore::ButtonPrompts::Style;

int main(int argc, char** argv) {
    if (argc == 2 && std::strcmp(argv[1], "--forced-nintendo") == 0) {
        SetDevice(1); // PlayStation
        assert(Shown() == Style::Nintendo);
        const auto* patches = Find(0x97779d0906f16551ULL, 512, 512, 1, Format::Rgba8);
        assert(patches != nullptr && !patches->empty());
        return 0;
    }

    // U2 icon sheet (512x512 RGBA8, ps4_sheet layout).
    constexpr u64 u2_sheet_hash = 0x97779d0906f16551ULL;

    SetDevice(0);
    assert(Shown() == Style::Keyboard);
    const auto* kb = Find(u2_sheet_hash, 512, 512, 1, Format::Rgba8);
    assert(kb != nullptr && !kb->empty());

    SetDevice(1);
    assert(Shown() == Style::Original);
    const auto* ps = Find(u2_sheet_hash, 512, 512, 1, Format::Rgba8);
    assert(ps != nullptr && ps->empty());

    SetDevice(2);
    assert(Shown() == Style::Xbox);
    const auto* xbox = Find(u2_sheet_hash, 512, 512, 1, Format::Rgba8);
    assert(xbox != nullptr && xbox->size() == kb->size());

    SetDevice(3);
    assert(Shown() == Style::Nintendo);
    const auto* nintendo = Find(u2_sheet_hash, 512, 512, 1, Format::Rgba8);
    assert(nintendo != nullptr && nintendo->size() == kb->size());

    // Nintendo sheet patches must differ from both Xbox and Keyboard.
    bool differs_from_xbox = false;
    bool differs_from_kb = false;
    for (size_t i = 0; i < nintendo->size(); ++i) {
        if ((*nintendo)[i].data != (*xbox)[i].data) {
            differs_from_xbox = true;
        }
        if ((*nintendo)[i].data != (*kb)[i].data) {
            differs_from_kb = true;
        }
    }
    assert(differs_from_xbox);
    assert(differs_from_kb);
    return 0;
}
