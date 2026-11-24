// SPDX-FileCopyrightText: Copyright 2026 Uncharted-Universal contributors
// SPDX-License-Identifier: GPL-2.0-or-later

#pragma once

#include <array>
#include <atomic>
#include <cstdlib>
#include <cstring>
#include <string>
#include <string_view>
#include <vector>

#include "common/serdes.h"
#include "common/types.h"

namespace BbShaderPreload {

/// Bitmask of runtime shader/pipeline feature flags that affect translated SPIR-V or pipeline keys.
namespace FeatureFlag {
inline constexpr u64 SafeShaders = 1ull << 0;
inline constexpr u64 NanGuard = 1ull << 1;
inline constexpr u64 EffectLoopClamp = 1ull << 2;
inline constexpr u64 InterpIntFix = 1ull << 3;
inline constexpr u64 MultiwaveLdsBarriers = 1ull << 4;
inline constexpr u64 ScopeSplitBarrier = 1ull << 5;
} // namespace FeatureFlag

/// Reads the current shader-affecting feature flags from environment toggles.
inline u64 CurrentFeatureFlags() {
    auto flag_on = [](const char* name, bool default_val) {
        if (const char* env = std::getenv(name)) {
            return env[0] != '0';
        }
        return default_val;
    };
    u64 flags = 0;
    if (flag_on("BB_SAFE_SHADERS", false)) {
        flags |= FeatureFlag::SafeShaders;
    }
    if (flag_on("BB_NAN_GUARD", true)) {
        flags |= FeatureFlag::NanGuard;
    }
    if (flag_on("BB_EFFECT_LOOP_CLAMP", true)) {
        flags |= FeatureFlag::EffectLoopClamp;
    }
    if (flag_on("BB_INTERP_INT_FIX", false)) {
        flags |= FeatureFlag::InterpIntFix;
    }
    if (flag_on("BB_MULTIWAVE_LDS_BARRIERS", true)) {
        flags |= FeatureFlag::MultiwaveLdsBarriers;
    }
    if (flag_on("BB_SCOPE_SPLIT_BARRIER", true)) {
        flags |= FeatureFlag::ScopeSplitBarrier;
    }
    return flags;
}

/// Full cache compatibility identity stored beside the shader profile.
struct CacheIdentity {
    static constexpr u32 Magic = 0x42425043u; // "BBPC"
    static constexpr u32 FormatVersion = 1u;

    u32 meta_version{};
    u32 binary_version{};
    u32 pipeline_key_version{};
    u32 vendor_id{};
    u32 device_id{};
    u32 driver_id{};
    u32 driver_version{};
    std::array<u8, 16> pipeline_cache_uuid{};
    u64 feature_flags{};
    std::string game_serial{};
    std::string app_ver{};

    bool operator==(const CacheIdentity& other) const = default;

    /// Returns true if `other` is compatible with `this`, or sets `reason` on mismatch.
    bool Matches(const CacheIdentity& other, std::string& reason) const {
        if (meta_version != other.meta_version || binary_version != other.binary_version ||
            pipeline_key_version != other.pipeline_key_version) {
            reason = "translated shader/pipeline version changed";
            return false;
        }
        if (game_serial != other.game_serial || app_ver != other.app_ver) {
            reason = "game serial or revision changed";
            return false;
        }
        if (vendor_id != other.vendor_id || device_id != other.device_id) {
            reason = "GPU device changed";
            return false;
        }
        if (driver_id != other.driver_id || driver_version != other.driver_version ||
            pipeline_cache_uuid != other.pipeline_cache_uuid) {
            reason = "GPU driver version or UUID changed";
            return false;
        }
        if (feature_flags != other.feature_flags) {
            reason = "shader feature flags changed";
            return false;
        }
        reason.clear();
        return true;
    }

    std::vector<u8> Serialize() const {
        Serialization::Archive ar{};
        Serialization::Writer w{ar};
        w.Write(Magic);
        w.Write(FormatVersion);
        w.Write(meta_version);
        w.Write(binary_version);
        w.Write(pipeline_key_version);
        w.Write(vendor_id);
        w.Write(device_id);
        w.Write(driver_id);
        w.Write(driver_version);
        w.Write(pipeline_cache_uuid.data(), pipeline_cache_uuid.size());
        w.Write(feature_flags);
        w.Write(game_serial);
        w.Write(app_ver);
        return ar.TakeOff();
    }

    bool Deserialize(std::vector<u8> data) {
        Serialization::Archive ar{std::move(data)};
        Serialization::Reader r{ar};
        u32 magic{};
        u32 fmt_ver{};
        if (!r.TryRead(magic) || magic != Magic || !r.TryRead(fmt_ver) ||
            fmt_ver != FormatVersion) {
            return false;
        }
        if (!r.TryRead(meta_version) || !r.TryRead(binary_version) ||
            !r.TryRead(pipeline_key_version) || !r.TryRead(vendor_id) || !r.TryRead(device_id) ||
            !r.TryRead(driver_id) || !r.TryRead(driver_version) ||
            !r.TryRead(pipeline_cache_uuid.data(), pipeline_cache_uuid.size()) ||
            !r.TryRead(feature_flags)) {
            return false;
        }
        auto read_str = [&](std::string& out) {
            size_t len{};
            if (!r.TryRead(len) || len > 1024 || !r.CanRead(len)) {
                return false;
            }
            out.resize(len);
            return len == 0 || r.TryRead(out.data(), len);
        };
        return read_str(game_serial) && read_str(app_ver);
    }
};

/// Cancellable progress state for background shader/pipeline warmup.
struct WarmUpProgress {
    struct Snapshot {
        u32 total_entries{};
        u32 completed_entries{};
        u32 warmed_pipelines{};
        u32 skipped_entries{};
        u32 failed_entries{};
        bool cancelled{};
        bool done{};
    };

    std::atomic<u32> total_entries{0};
    std::atomic<u32> completed_entries{0};
    std::atomic<u32> warmed_pipelines{0};
    std::atomic<u32> skipped_entries{0};
    std::atomic<u32> failed_entries{0};
    std::atomic<bool> cancel_requested{false};
    std::atomic<bool> done{true};

    void Reset(u32 total) {
        total_entries.store(total, std::memory_order_release);
        completed_entries.store(0, std::memory_order_release);
        warmed_pipelines.store(0, std::memory_order_release);
        skipped_entries.store(0, std::memory_order_release);
        failed_entries.store(0, std::memory_order_release);
        cancel_requested.store(false, std::memory_order_release);
        done.store(false, std::memory_order_release);
    }

    void RequestCancel() {
        cancel_requested.store(true, std::memory_order_release);
    }

    [[nodiscard]] bool IsCancelled() const {
        return cancel_requested.load(std::memory_order_acquire);
    }

    [[nodiscard]] Snapshot GetSnapshot() const {
        return Snapshot{
            .total_entries = total_entries.load(std::memory_order_acquire),
            .completed_entries = completed_entries.load(std::memory_order_acquire),
            .warmed_pipelines = warmed_pipelines.load(std::memory_order_acquire),
            .skipped_entries = skipped_entries.load(std::memory_order_acquire),
            .failed_entries = failed_entries.load(std::memory_order_acquire),
            .cancelled = cancel_requested.load(std::memory_order_acquire),
            .done = done.load(std::memory_order_acquire),
        };
    }
};

} // namespace BbShaderPreload
