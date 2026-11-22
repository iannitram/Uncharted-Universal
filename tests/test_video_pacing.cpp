// SPDX-License-Identifier: GPL-2.0-or-later
#include <cassert>
#include <chrono>
#include <cmath>
#include <cstdio>

#include "bbport_video_pacing.h"

int main() {
    using namespace std::chrono;
    using BbVideoPacing::DefaultFrameMs;
    using BbVideoPacing::FlipVblankDivisor;
    using BbVideoPacing::FramePaceTracker;
    using BbVideoPacing::MovieTimeoutNs;
    using BbVideoPacing::MovieVblankPacer;

    // 1. FlipVblankDivisor: PS4 flip_rate targets 60 / (rate + 1) Hz without halving 30 Hz caps
    // or over-running 120/480 Hz host vblanks.
    assert(FlipVblankDivisor(60, 0) == 1);
    assert(FlipVblankDivisor(60, 1) == 2);
    assert(FlipVblankDivisor(60, 2) == 3);
    assert(FlipVblankDivisor(120, 0) == 1);
    assert(FlipVblankDivisor(120, 1) == 4);
    assert(FlipVblankDivisor(120, 2) == 6);
    assert(FlipVblankDivisor(480, 0) == 1);
    assert(FlipVblankDivisor(480, 1) == 16);
    assert(FlipVblankDivisor(30, 0) == 1);
    assert(FlipVblankDivisor(30, 1) == 1);
    assert(FlipVblankDivisor(60, -1) == 1); // defensive: never divides by zero

    // 2. MovieVblankPacer: 120 Hz vblank is paced down to 60 Hz during a movie, and cleanly
    // returns to full 120 Hz once movie frames clear or time out, without burst catch-up on
    // re-entry.
    {
        MovieVblankPacer pacer;
        const nanoseconds vblank_120{1000000000 / 120};
        steady_clock::time_point now{seconds(10)};

        // Cutscene active: alternates tick / skip at 120 Hz -> 60 Hz output.
        std::int64_t movie_ns = now.time_since_epoch().count();
        int fired = 0;
        for (int i = 0; i < 12; ++i) {
            const auto step = pacer.Step(true, movie_ns, now, vblank_120);
            if (!step.skip_tick) {
                assert(step.active);
                ++fired;
            }
            now += vblank_120;
            if (i % 2 == 0) {
                movie_ns = now.time_since_epoch().count();
            }
        }
        assert(fired == 6);

        // Cutscene ends (decoder closed -> movie_ns == 0): immediately returns to 120 Hz.
        fired = 0;
        for (int i = 0; i < 6; ++i) {
            const auto step = pacer.Step(true, 0, now, vblank_120);
            assert(!step.active && !step.skip_tick);
            ++fired;
            now += vblank_120;
        }
        assert(fired == 6);

        // Timeout path: 300 ms after last decode, movie pacing stops automatically.
        const auto after_timeout = steady_clock::time_point(nanoseconds(movie_ns + MovieTimeoutNs));
        const auto expired = pacer.Step(true, movie_ns, after_timeout, vblank_120);
        assert(!expired.active && !expired.skip_tick);

        // Second cutscene minutes later re-anchors next_vblank so it does not burst back-to-back.
        const auto later = after_timeout + seconds(120);
        const std::int64_t later_ns = later.time_since_epoch().count();
        const auto s0 = pacer.Step(true, later_ns, later, vblank_120);
        const auto s1 = pacer.Step(true, later_ns, later + vblank_120, vblank_120);
        const auto s2 = pacer.Step(true, later_ns, later + 2 * vblank_120, vblank_120);
        assert(s0.active && !s0.skip_tick);
        assert(s1.active && s1.skip_tick);
        assert(s2.active && !s2.skip_tick);
    }

    // 3. FramePaceTracker: a 30 FPS cutscene (generated == 0, !fast_game_paused) must not leave
    // stale 33.3 ms pacing behind to lock post-cutscene FIFO gameplay at 30 or 40 FPS.
    {
        FramePaceTracker tracker;
        steady_clock::time_point t{seconds(1)};

        // Simulate 120 frames (4 s) of a 30 FPS cutscene.
        for (int i = 0; i < 120; ++i) {
            const double ms = tracker.RecordPresent(t, false, false);
            assert(std::fabs(ms - DefaultFrameMs) < 1e-6);
            t += nanoseconds(33'333'333);
        }

        // First gameplay frame with frame generation starts at DefaultFrameMs (~16.67 ms -> ~8.33 ms
        // step) instead of the 33.3 ms cutscene interval.
        const double first_ms = tracker.RecordPresent(t, true, false);
        assert(std::fabs(first_ms - DefaultFrameMs) < 1e-6);

        // Normal 60 FPS gameplay updates the EMA smoothly.
        t += nanoseconds(16'666'667);
        const double second_ms = tracker.RecordPresent(t, true, false);
        assert(second_ms < 17.0 && second_ms > 16.0);

        // Fast-game pause (>175 FPS, generated == 0 while fast_game_paused == true) keeps measuring
        // frame intervals so PrepareFrame sees when the game drops back below 158 FPS.
        for (int i = 0; i < 30; ++i) {
            t += milliseconds(4);
            tracker.RecordPresent(t, false, true);
        }
        assert(tracker.CurrentMs() < 6.0);
        for (int i = 0; i < 30; ++i) {
            t += milliseconds(10);
            tracker.RecordPresent(t, false, true);
        }
        assert(tracker.CurrentMs() > 6.3);
    }

    std::puts("video pacing tests passed");
    return 0;
}
