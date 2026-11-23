// SPDX-FileCopyrightText: Copyright 2026 shadPS4 Emulator Project
// SPDX-License-Identifier: GPL-2.0-or-later

// Unit test for StageSpecialization::operator== binding-start check
// (ported from Mevlock/uncharted_pc-dev@13a3d58).

#include "shader_recompiler/specialization.h"

#include <cassert>

namespace {

Shader::StageSpecialization MakeSpec(Shader::Info& info, u32 unified_start) {
    Shader::StageSpecialization spec;
    spec.info = &info;
    spec.start.unified = unified_start;
    return spec;
}

void TestSlotlessStageMatchesAcrossStarts() {
    Shader::Info info{};
    const auto a = MakeSpec(info, 0);
    const auto b = MakeSpec(info, 3);
    assert(a == b);
}

void TestNullBufferSlotDoesNotMatchAcrossStarts() {
    Shader::Info info{};
    auto a = MakeSpec(info, 2);
    auto b = MakeSpec(info, 0);
    // All sharps null -> bitset stays empty, but the stage still declares one buffer slot.
    a.buffers.emplace_back();
    b.buffers.emplace_back();
    assert(a.bitset.none() && b.bitset.none());
    assert(!(a == b));

    // Same start still matches.
    const auto c = MakeSpec(info, 2);
    auto d = MakeSpec(info, 2);
    d.buffers.emplace_back();
    auto a2 = MakeSpec(info, 2);
    a2.buffers.emplace_back();
    assert(a2 == d);
    (void)c;
}

void TestNullImageOrSamplerSlotDoesNotMatchAcrossStarts() {
    Shader::Info info{};
    auto img_a = MakeSpec(info, 1);
    auto img_b = MakeSpec(info, 4);
    img_a.images.emplace_back();
    img_b.images.emplace_back();
    assert(!(img_a == img_b));

    auto smp_a = MakeSpec(info, 1);
    auto smp_b = MakeSpec(info, 4);
    smp_a.samplers.emplace_back();
    smp_b.samplers.emplace_back();
    assert(!(smp_a == smp_b));
}

} // namespace

int main() {
    TestSlotlessStageMatchesAcrossStarts();
    TestNullBufferSlotDoesNotMatchAcrossStarts();
    TestNullImageOrSamplerSlotDoesNotMatchAcrossStarts();
    return 0;
}
