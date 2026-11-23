// SPDX-FileCopyrightText: Copyright 2026 Uncharted-Universal Contributors
// SPDX-License-Identifier: GPL-2.0-or-later

#pragma once

#include <cstdint>

namespace Libraries::Videodec2 {

enum class DecodeStepStatus {
    FrameReady,
    NeedMoreInput,
    FatalError,
};

struct DecodeRecoveryStats {
    std::uint64_t send_eagain_drains = 0;
    std::uint64_t send_eof_flushes = 0;
    std::uint64_t send_invalid_flushes = 0;
    std::uint64_t corrupt_packets_skipped = 0;
    std::uint64_t receive_errors = 0;
    int last_error = 0;

    /// Rate-limits diagnostic logging: logs the first 5 events and then powers of two.
    static bool ShouldLog(std::uint64_t count) {
        return count <= 5 || (count & (count - 1)) == 0;
    }
};

struct DecodeStepResult {
    DecodeStepStatus status = DecodeStepStatus::NeedMoreInput;
    bool drained_before_send = false;
    bool flushed = false;
    bool skipped_corrupt = false;
    int av_error = 0;
};

/// Resolves an output frame's presentation timestamp, avoiding FFmpeg's `AV_NOPTS_VALUE`
/// (`INT64_MIN`) sentinel which would appear as a huge future `u64` timestamp and stall A/V sync.
inline std::uint64_t ResolveOutputPts(std::int64_t frame_pts, std::int64_t best_effort_ts,
                                      std::int64_t av_nopts_value,
                                      std::uint64_t& last_valid_pts) {
    if (frame_pts != av_nopts_value) {
        last_valid_pts = static_cast<std::uint64_t>(frame_pts);
        return last_valid_pts;
    }
    if (best_effort_ts != av_nopts_value) {
        last_valid_pts = static_cast<std::uint64_t>(best_effort_ts);
        return last_valid_pts;
    }
    return last_valid_pts;
}

/// Coordinates one FFmpeg send_packet / receive_frame decode step without stalling when
/// multithreaded frame decoding fills the internal output queue (`AVERROR(EAGAIN)` on send)
/// or encounters a seek/scene-change EOF or corrupt packet (`AVERROR_INVALIDDATA`) on either
/// send or deferred frame-thread receive.
template <typename SendFn, typename ReceiveFn, typename FlushFn>
DecodeStepResult RunDecodeStep(SendFn&& send_packet, ReceiveFn&& receive_frame,
                               FlushFn&& flush_buffers, int err_eagain, int err_eof,
                               int err_invaliddata, DecodeRecoveryStats& stats) {
    DecodeStepResult result{};
    bool have_frame = false;

    int send_ret = send_packet();
    if (send_ret == err_eagain) {
        // Output queue is full: drain a decoded frame first, then retry sending the packet.
        const int drain_ret = receive_frame();
        if (drain_ret >= 0) {
            have_frame = true;
            result.drained_before_send = true;
            ++stats.send_eagain_drains;
            send_ret = send_packet();
            if (send_ret == err_eagain) {
                // Still full after one drain; deliver the drained frame so the caller makes progress.
                result.status = DecodeStepStatus::FrameReady;
                return result;
            }
        } else if (drain_ret == err_eof) {
            // Decoder reached EOF before draining; flush so the new packet can be sent below.
            flush_buffers();
            result.flushed = true;
            ++stats.send_eof_flushes;
            send_ret = send_packet();
        } else if (drain_ret == err_invaliddata) {
            // Frame-threaded decode surfaced a corrupt buffered AU while draining.
            flush_buffers();
            result.flushed = true;
            ++stats.send_invalid_flushes;
            send_ret = send_packet();
        } else if (drain_ret != err_eagain) {
            ++stats.receive_errors;
            stats.last_error = drain_ret;
            result.status = DecodeStepStatus::FatalError;
            result.av_error = drain_ret;
            return result;
        } else {
            // Both send and receive returned EAGAIN; ask caller for more input instead of failing.
            result.status = DecodeStepStatus::NeedMoreInput;
            return result;
        }
    }

    if (send_ret == err_eof) {
        flush_buffers();
        result.flushed = true;
        ++stats.send_eof_flushes;
        send_ret = send_packet();
    }
    if (send_ret == err_invaliddata) {
        if (!result.flushed) {
            flush_buffers();
            result.flushed = true;
            ++stats.send_invalid_flushes;
            send_ret = send_packet();
        }
        if (send_ret == err_invaliddata) {
            // Corrupt/seek-boundary AU: skip cleanly instead of freezing playback.
            ++stats.corrupt_packets_skipped;
            stats.last_error = send_ret;
            result.skipped_corrupt = true;
            result.av_error = send_ret;
            result.status =
                have_frame ? DecodeStepStatus::FrameReady : DecodeStepStatus::NeedMoreInput;
            return result;
        }
    }

    if (send_ret < 0) {
        stats.last_error = send_ret;
        if (have_frame) {
            result.status = DecodeStepStatus::FrameReady;
            return result;
        }
        result.status = DecodeStepStatus::FatalError;
        result.av_error = send_ret;
        return result;
    }

    if (have_frame) {
        result.status = DecodeStepStatus::FrameReady;
        return result;
    }

    const int recv_ret = receive_frame();
    if (recv_ret >= 0) {
        result.status = DecodeStepStatus::FrameReady;
        return result;
    }
    if (recv_ret == err_eagain || recv_ret == err_eof) {
        result.status = DecodeStepStatus::NeedMoreInput;
        return result;
    }
    if (recv_ret == err_invaliddata) {
        // With FF_THREAD_FRAME, a corrupt AU can be accepted by send_packet and fail later on
        // receive_frame; flush and skip instead of returning a fatal API error.
        flush_buffers();
        result.flushed = true;
        result.skipped_corrupt = true;
        result.av_error = recv_ret;
        ++stats.send_invalid_flushes;
        ++stats.corrupt_packets_skipped;
        stats.last_error = recv_ret;
        result.status = DecodeStepStatus::NeedMoreInput;
        return result;
    }

    ++stats.receive_errors;
    stats.last_error = recv_ret;
    result.status = DecodeStepStatus::FatalError;
    result.av_error = recv_ret;
    return result;
}

} // namespace Libraries::Videodec2
