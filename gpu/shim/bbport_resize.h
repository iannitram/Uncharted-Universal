// SPDX-License-Identifier: GPL-2.0-or-later
// bbport: debounced live-resize / fold-unfold state tracker (M21e, #436).

#pragma once

#include <cstdint>

inline constexpr uint32_t kBbResizeRestartThresholdPx = 16;
inline constexpr uint64_t kBbResizeDebounceMs = 250;

inline bool BbResizeNeedsRestart(uint32_t start_w, uint32_t start_h, uint32_t cur_w,
                                 uint32_t cur_h) {
    if (start_w == 0 || start_h == 0 || cur_w == 0 || cur_h == 0) {
        return false;
    }
    const uint32_t dw = start_w > cur_w ? start_w - cur_w : cur_w - start_w;
    const uint32_t dh = start_h > cur_h ? start_h - cur_h : cur_h - start_h;
    return dw > kBbResizeRestartThresholdPx || dh > kBbResizeRestartThresholdPx;
}

struct BbResizeState {
    bool needs_restart = false;
    uint32_t settled_width = 0;
    uint32_t settled_height = 0;
    bool just_settled = false;
};

class BbResizeTracker {
public:
    explicit BbResizeTracker(uint64_t debounce_ms = kBbResizeDebounceMs)
        : debounce_ms_(debounce_ms) {}

    BbResizeState Observe(uint32_t start_w, uint32_t start_h, uint32_t cur_w, uint32_t cur_h,
                          uint64_t now_ms) {
        if (start_w == 0 || start_h == 0 || cur_w == 0 || cur_h == 0) {
            return State();
        }
        if (!has_pending_ || pending_w_ != cur_w || pending_h_ != cur_h) {
            has_pending_ = true;
            pending_w_ = cur_w;
            pending_h_ = cur_h;
            pending_since_ms_ = now_ms;
            if (debounce_ms_ > 0) {
                return State();
            }
        }
        if (now_ms < pending_since_ms_ || (now_ms - pending_since_ms_) < debounce_ms_) {
            return State();
        }

        const bool next_restart = BbResizeNeedsRestart(start_w, start_h, pending_w_, pending_h_);
        const bool changed = !has_settled_ || settled_w_ != pending_w_ ||
                             settled_h_ != pending_h_ || needs_restart_ != next_restart;
        has_settled_ = true;
        settled_w_ = pending_w_;
        settled_h_ = pending_h_;
        needs_restart_ = next_restart;
        return BbResizeState{
            .needs_restart = needs_restart_,
            .settled_width = settled_w_,
            .settled_height = settled_h_,
            .just_settled = changed && next_restart,
        };
    }

    BbResizeState State() const {
        return BbResizeState{
            .needs_restart = needs_restart_,
            .settled_width = has_settled_ ? settled_w_ : 0,
            .settled_height = has_settled_ ? settled_h_ : 0,
            .just_settled = false,
        };
    }

private:
    uint64_t debounce_ms_ = kBbResizeDebounceMs;
    bool has_pending_ = false;
    uint32_t pending_w_ = 0;
    uint32_t pending_h_ = 0;
    uint64_t pending_since_ms_ = 0;
    bool has_settled_ = false;
    uint32_t settled_w_ = 0;
    uint32_t settled_h_ = 0;
    bool needs_restart_ = false;
};
