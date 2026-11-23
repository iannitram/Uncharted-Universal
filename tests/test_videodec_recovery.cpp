// SPDX-FileCopyrightText: Copyright 2026 Uncharted-Universal Contributors
// SPDX-License-Identifier: GPL-2.0-or-later

#include <cassert>
#include <cstdio>

#include "core/libraries/videodec/videodec2_recovery.h"

int main() {
    using namespace Libraries::Videodec2;
    constexpr int kEagain = -11;
    constexpr int kEof = -541478725;
    constexpr int kInvalid = -1094995529;
    constexpr int kFatal = -12;

    // 1. Normal path: send succeeds, receive delivers a frame.
    {
        DecodeRecoveryStats stats{};
        int sends = 0, recvs = 0, flushes = 0;
        const auto res = RunDecodeStep(
            [&] {
                ++sends;
                return 0;
            },
            [&] {
                ++recvs;
                return 0;
            },
            [&] { ++flushes; }, kEagain, kEof, kInvalid, stats);
        assert(res.status == DecodeStepStatus::FrameReady);
        assert(sends == 1 && recvs == 1 && flushes == 0);
        assert(!res.drained_before_send && !res.flushed && !res.skipped_corrupt);
    }

    // 2. EAGAIN on send (full multithreaded output queue): drains pending frame, then retries send.
    {
        DecodeRecoveryStats stats{};
        int sends = 0, recvs = 0, flushes = 0;
        const auto res = RunDecodeStep(
            [&] {
                ++sends;
                return sends == 1 ? kEagain : 0;
            },
            [&] {
                ++recvs;
                return 0;
            },
            [&] { ++flushes; }, kEagain, kEof, kInvalid, stats);
        assert(res.status == DecodeStepStatus::FrameReady);
        assert(res.drained_before_send);
        assert(sends == 2 && recvs == 1 && flushes == 0);
        assert(stats.send_eagain_drains == 1);
    }

    // 3. EAGAIN on both initial and retry send: still delivers the drained frame instead of stalling.
    {
        DecodeRecoveryStats stats{};
        int sends = 0, recvs = 0;
        const auto res = RunDecodeStep(
            [&] {
                ++sends;
                return kEagain;
            },
            [&] {
                ++recvs;
                return 0;
            },
            [] {}, kEagain, kEof, kInvalid, stats);
        assert(res.status == DecodeStepStatus::FrameReady);
        assert(res.drained_before_send);
        assert(sends == 2 && recvs == 1);
    }

    // 4. EOF on send (post-drain scene transition): flushes decoder and retries send.
    {
        DecodeRecoveryStats stats{};
        int sends = 0, flushes = 0;
        const auto res = RunDecodeStep(
            [&] {
                ++sends;
                return sends == 1 ? kEof : 0;
            },
            [] { return 0; },
            [&] { ++flushes; }, kEagain, kEof, kInvalid, stats);
        assert(res.status == DecodeStepStatus::FrameReady);
        assert(res.flushed && flushes == 1 && sends == 2);
        assert(stats.send_eof_flushes == 1);
    }

    // 5. Corrupt/invalid packet: flushes once, skips cleanly instead of returning FatalError.
    {
        DecodeRecoveryStats stats{};
        int sends = 0, flushes = 0;
        const auto res = RunDecodeStep(
            [&] {
                ++sends;
                return kInvalid;
            },
            [] { return kEagain; },
            [&] { ++flushes; }, kEagain, kEof, kInvalid, stats);
        assert(res.status == DecodeStepStatus::NeedMoreInput);
        assert(res.flushed && res.skipped_corrupt);
        assert(sends == 2 && flushes == 1);
        assert(stats.send_invalid_flushes == 1 && stats.corrupt_packets_skipped == 1);
    }

    // 6. Unrecoverable send error still surfaces FatalError.
    {
        DecodeRecoveryStats stats{};
        const auto res = RunDecodeStep(
            [] { return kFatal; }, [] { return kEagain; }, [] {}, kEagain, kEof, kInvalid, stats);
        assert(res.status == DecodeStepStatus::FatalError);
        assert(res.av_error == kFatal);
    }

    // 7. Diagnostic rate limiter logs first 5 and powers of two only.
    assert(DecodeRecoveryStats::ShouldLog(1));
    assert(DecodeRecoveryStats::ShouldLog(5));
    assert(!DecodeRecoveryStats::ShouldLog(6));
    assert(DecodeRecoveryStats::ShouldLog(8));
    assert(!DecodeRecoveryStats::ShouldLog(9));

    std::puts("videodec recovery tests passed");
    return 0;
}
