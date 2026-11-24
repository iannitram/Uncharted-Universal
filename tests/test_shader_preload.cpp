// SPDX-FileCopyrightText: Copyright 2026 Uncharted-Universal contributors
// SPDX-License-Identifier: GPL-2.0-or-later

#include <cassert>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "bbport_shader_preload.h"

// Minimal standalone stubs for Common::Log / assert_fail_impl referenced by common/serdes.h.
void assert_fail_impl() {
    std::abort();
}
[[noreturn]] void unreachable_impl() {
    std::abort();
}
void assert_fail_debug_msg(const char*) {
    std::abort();
}
namespace Common::Log {
bool ShouldLog(spdlog::level) {
    return false;
}
void Write(std::string_view, spdlog::level, const char*, int, const char*, std::string&&) {}
} // namespace Common::Log

namespace {

BbShaderPreload::CacheIdentity SampleIdentity() {
    BbShaderPreload::CacheIdentity id{};
    id.meta_version = 9;
    id.binary_version = 16;
    id.pipeline_key_version = 6;
    id.vendor_id = 0x1002;
    id.device_id = 0x1586;
    id.driver_id = 3;
    id.driver_version = 240200;
    id.pipeline_cache_uuid = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16};
    id.feature_flags =
        BbShaderPreload::FeatureFlag::NanGuard | BbShaderPreload::FeatureFlag::EffectLoopClamp;
    id.game_serial = "CUSA02320";
    id.app_ver = "01.02";
    return id;
}

void TestIdentityRoundTripAndMismatch() {
    const auto base = SampleIdentity();
    auto bytes = base.Serialize();
    assert(!bytes.empty());

    BbShaderPreload::CacheIdentity decoded{};
    assert(decoded.Deserialize(bytes));
    assert(decoded == base);

    std::string reason;
    assert(decoded.Matches(base, reason));
    assert(reason.empty());

    auto wrong_rev = base;
    wrong_rev.app_ver = "01.00";
    assert(!wrong_rev.Matches(base, reason));
    assert(!reason.empty());

    auto wrong_driver = base;
    wrong_driver.driver_version += 1;
    assert(!wrong_driver.Matches(base, reason));
    assert(!reason.empty());

    auto wrong_flags = base;
    wrong_flags.feature_flags |= BbShaderPreload::FeatureFlag::SafeShaders;
    assert(!wrong_flags.Matches(base, reason));
    assert(!reason.empty());

    auto wrong_shader_ver = base;
    wrong_shader_ver.binary_version += 1;
    assert(!wrong_shader_ver.Matches(base, reason));
    assert(!reason.empty());
}

void TestCorruptBlobRejection() {
    const auto base = SampleIdentity();
    auto bytes = base.Serialize();

    // Truncated at every possible prefix length must fail cleanly without crashing.
    for (size_t len = 0; len < bytes.size(); ++len) {
        std::vector<u8> truncated(bytes.begin(), bytes.begin() + static_cast<ptrdiff_t>(len));
        BbShaderPreload::CacheIdentity out{};
        assert(!out.Deserialize(std::move(truncated)));
    }

    // Wrong magic must fail.
    auto bad_magic = bytes;
    bad_magic[0] ^= 0xff;
    BbShaderPreload::CacheIdentity out{};
    assert(!out.Deserialize(std::move(bad_magic)));

    // Serialization::Reader::TryRead bounds check on raw archive.
    std::vector<u8> small_buf{0x01, 0x02};
    Serialization::Archive ar{std::move(small_buf)};
    Serialization::Reader reader{ar};
    u64 too_big{};
    assert(!reader.CanRead(sizeof(u64)));
    assert(!reader.TryRead(too_big));
    u16 ok_val{};
    assert(reader.CanRead(sizeof(u16)));
    assert(reader.TryRead(ok_val));
    assert(ar.IsEoS());
}

void TestWarmUpProgressAndCancellation() {
    BbShaderPreload::WarmUpProgress progress;
    assert(progress.GetSnapshot().done);

    progress.Reset(10);
    auto snap = progress.GetSnapshot();
    assert(snap.total_entries == 10);
    assert(snap.completed_entries == 0);
    assert(!snap.cancelled);
    assert(!snap.done);

    progress.warmed_pipelines.fetch_add(3);
    progress.completed_entries.fetch_add(3);
    progress.RequestCancel();
    assert(progress.IsCancelled());

    progress.done.store(true);
    snap = progress.GetSnapshot();
    assert(snap.warmed_pipelines == 3);
    assert(snap.completed_entries == 3);
    assert(snap.cancelled);
    assert(snap.done);
}

} // namespace

int main() {
    TestIdentityRoundTripAndMismatch();
    TestCorruptBlobRejection();
    TestWarmUpProgressAndCancellation();
    return 0;
}
