// SPDX-License-Identifier: GPL-2.0-or-later
// bbport: frame-pacing helpers shared by VideoOut (flip rate, 60 Hz movie vblank pacing) and
// Vulkan::Presenter (frame-generation interval tracking across cutscene/menu transitions).
#pragma once

#include <algorithm>
#include <chrono>
#include <cstdint>

namespace BbVideoPacing {

/// Default frame interval (~60 FPS) used before gameplay pacing is measured or after a
/// non-generated pause (cutscenes, menus, loading).
inline constexpr double DefaultFrameMs = 1000.0 / 60.0;

/// Prerecorded movies are timed for a 60 Hz display and time out 300 ms after the last decode.
inline constexpr std::chrono::nanoseconds MoviePeriod{1000000000 / 60};
inline constexpr std::int64_t MovieTimeoutNs = 300'000'000;

/// Host vblank divisor corresponding to PS4 `sceVideoOutSetFlipRate(handle, rate)`.
/// `rate == 0` flips on every host vblank; `rate > 0` targets `60 / (rate + 1)` Hz relative to
/// the PS4 60 Hz vblank baseline so higher host vblank rates (120/480 Hz) keep the requested
/// rate and lower host caps (30 Hz) are not halved again.
inline std::uint32_t FlipVblankDivisor(std::uint32_t vblank_hz, int flip_rate) {
    if (flip_rate <= 0) {
        return 1u;
    }
    const std::uint32_t ps4_divisor = static_cast<std::uint32_t>(flip_rate + 1);
    if (vblank_hz == 0) {
        return ps4_divisor;
    }
    return std::max<std::uint32_t>(1u, (vblank_hz * ps4_divisor + 30u) / 60u);
}

/// Result of one `MovieVblankPacer::Step` tick.
struct MovieStep {
    bool active = false;
    bool skip_tick = false;
};

/// Paces fast host vblanks (> 60 Hz) down to 60 Hz while prerecorded video frames are being
/// decoded, and re-anchors cleanly when entering or leaving a cutscene.
class MovieVblankPacer {
public:
    static bool IsActive(std::int64_t last_movie_ns, std::int64_t now_ns) {
        return last_movie_ns > 0 && now_ns >= last_movie_ns &&
               (now_ns - last_movie_ns) < MovieTimeoutNs;
    }

    MovieStep Step(bool fast_vblank, std::int64_t last_movie_ns,
                   std::chrono::steady_clock::time_point now,
                   std::chrono::nanoseconds vblank_period) {
        const bool active = fast_vblank && IsActive(last_movie_ns, now.time_since_epoch().count());
        if (!active) {
            was_active = false;
            return {false, false};
        }
        if (!was_active) {
            was_active = true;
            next_vblank = now;
        }
        if (now + vblank_period / 2 < next_vblank) {
            return {true, true};
        }
        next_vblank = std::max(next_vblank + MoviePeriod, now - MoviePeriod);
        return {true, false};
    }

private:
    bool was_active = false;
    std::chrono::steady_clock::time_point next_vblank{};
};

/// Tracks the game's frame interval for spacing generated frames without letting 30 FPS
/// cutscenes, menus, or loading transitions lock the post-cutscene cadence under FIFO vsync.
class FramePaceTracker {
public:
    double RecordPresent(std::chrono::steady_clock::time_point now, bool has_generated,
                         bool fast_game_paused) {
        if (!has_generated && !fast_game_paused) {
            frame_ms = DefaultFrameMs;
            in_generated_run = false;
            last_present = {};
            return frame_ms;
        }
        if (has_generated && !in_generated_run) {
            frame_ms = DefaultFrameMs;
            in_generated_run = true;
            last_present = now;
            return frame_ms;
        }
        if (last_present != std::chrono::steady_clock::time_point{}) {
            const double ms =
                std::chrono::duration<double, std::milli>(now - last_present).count();
            if (ms > 1.0 && ms < 100.0) {
                frame_ms = frame_ms + (ms - frame_ms) * 0.1;
            }
        }
        last_present = now;
        return frame_ms;
    }

    double CurrentMs() const {
        return frame_ms;
    }

private:
    double frame_ms = DefaultFrameMs;
    bool in_generated_run = false;
    std::chrono::steady_clock::time_point last_present{};
};

} // namespace BbVideoPacing
