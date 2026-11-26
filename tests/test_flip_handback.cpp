// SPDX-License-Identifier: GPL-2.0-or-later
// #592: a flip deferred to the landing thread hands its frame back to the GPU command thread.
//
// A model of the threads around the draw scheduler, with the port's real routing decision
// (AmdGpu::PrepareEopFlipInline, used by VideoOutDriver::SubmitFlip):
// - stage A (the GPU command thread) hands draw packets to stage B and, at its safe point
//   (Liverpool::ProcessCommands), drains stage B and then runs the commands sent to it;
// - stage B (the draw recording thread) records each packet into the scheduler; a flip packet
//   is deferred to the landing thread (SignalFlip while a landing is pending), and stage B goes
//   on recording the next frame;
// - the landing thread runs the flip interrupt handler, which prepares the frame (end and
//   submit the command buffer) inline or sends it to stage A;
// - stage A waits for every deferred signal at the end of a frame (WaitDeferredSignals), the
//   wait behind the #112 freeze.
// The fake scheduler counts any overlap of recording and end/submit; every wait has a timeout.

#include "video_core/amdgpu/liverpool.h"

#include <atomic>
#include <cassert>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <deque>
#include <functional>
#include <mutex>
#include <thread>

namespace Common {
void SetCurrentThreadName(const char*) {}
} // namespace Common

namespace {

using namespace std::chrono_literals;

thread_local bool on_stage_a = false;
thread_local bool on_stage_b = false;

void Busy(std::chrono::microseconds time) {
    const auto until = std::chrono::steady_clock::now() + time;
    while (std::chrono::steady_clock::now() < until) {
        std::this_thread::yield();
    }
}

/// Recording into the current command buffer and ending/submitting it must never overlap.
struct FakeScheduler {
    std::atomic<int> recording{0};
    std::atomic<int> submitting{0};
    std::atomic<unsigned> overlaps{0};
    std::atomic<unsigned> submits{0};
    std::atomic<unsigned> submits_off_gpu_side{0};

    void Record() {
        recording.fetch_add(1);
        if (submitting.load() != 0) {
            overlaps.fetch_add(1);
        }
        Busy(20us);
        if (submitting.load() != 0) {
            overlaps.fetch_add(1);
        }
        recording.fetch_sub(1);
    }

    /// Presenter::PrepareFrame -> Scheduler::Flush -> SubmitExecution (current_cmdbuf.end()).
    void Flush() {
        if (!on_stage_a && !on_stage_b) {
            submits_off_gpu_side.fetch_add(1);
        }
        submitting.fetch_add(1);
        if (recording.load() != 0) {
            overlaps.fetch_add(1);
        }
        Busy(50us);
        if (recording.load() != 0) {
            overlaps.fetch_add(1);
        }
        submitting.fetch_sub(1);
        submits.fetch_add(1);
    }
};

/// A FIFO worker (the landing thread).
class Worker {
public:
    Worker() : thread{[this] { Run(); }} {}
    ~Worker() {
        {
            std::scoped_lock lk{mutex};
            stop = true;
        }
        cv.notify_all();
        thread.join();
    }
    void Post(std::function<void()> fn) {
        {
            std::scoped_lock lk{mutex};
            queue.push_back(std::move(fn));
        }
        cv.notify_all();
    }

private:
    void Run() {
        std::unique_lock lk{mutex};
        for (;;) {
            cv.wait(lk, [&] { return stop || !queue.empty(); });
            if (queue.empty()) {
                return;
            }
            auto fn = std::move(queue.front());
            queue.pop_front();
            lk.unlock();
            fn();
            lk.lock();
        }
    }
    std::mutex mutex;
    std::condition_variable cv;
    std::deque<std::function<void()>> queue;
    bool stop = false;
    std::thread thread;
};

enum class Route {
    Handback,     ///< the port: PrepareEopFlipInline, else SendCommand without waiting
    WaitForStageA ///< the #112 shape: the handler waits for stage A to prepare the frame
};

struct Result {
    unsigned frames_done = 0;
    bool deadlocked = false;
    unsigned overlaps = 0;
    unsigned submits = 0;
    unsigned submits_off_gpu_side = 0;
};

Result RunModel(Route route, unsigned frames, unsigned draws_per_frame,
                std::chrono::milliseconds wait_timeout) {
    FakeScheduler scheduler;
    Worker landing;

    // Stage A's command queue (Liverpool::SendCommand / ProcessCommands).
    std::mutex command_mutex;
    std::condition_variable command_cv;
    std::deque<std::function<void()>> commands;
    std::atomic<unsigned> commands_run{0};
    const auto send_command = [&](std::function<void()> fn) {
        {
            std::scoped_lock lk{command_mutex};
            commands.push_back(std::move(fn));
        }
        command_cv.notify_all();
    };

    // Stage B's packet ring (DrawPipe). A packet is a draw or a flip.
    std::mutex ring_mutex;
    std::condition_variable ring_cv;
    std::deque<bool> ring; // true: flip
    bool ring_stop = false;
    bool stage_b_busy = false;

    // Deferred signals (Scheduler::deferred_signals_issued / deferred_signals_done).
    std::atomic<unsigned> signals_issued{0};
    std::atomic<unsigned> signals_done{0};
    std::atomic<bool> gave_up{false};

    // VideoOutDriver::SubmitFlip for an EOP flip, from the interrupt handler.
    const auto submit_flip = [&] {
        const auto prepare = [&] { scheduler.Flush(); };
        if (route == Route::Handback) {
            if (AmdGpu::PrepareEopFlipInline(on_stage_a, on_stage_b)) {
                prepare();
            } else {
                send_command(prepare);
            }
            return;
        }
        // The #112 shape: block until stage A ran it (bounded here so the test ends).
        std::mutex done_mutex;
        std::condition_variable done_cv;
        bool done = false;
        send_command([&] {
            prepare();
            {
                std::scoped_lock lk{done_mutex};
                done = true;
            }
            done_cv.notify_all();
        });
        std::unique_lock lk{done_mutex};
        if (!done_cv.wait_for(lk, wait_timeout, [&] { return done; })) {
            gave_up.store(true);
            // Stage A still runs the command later; it must not touch this frame's locals.
            done_cv.wait(lk, [&] { return done; });
        }
    };

    std::thread stage_b{[&] {
        on_stage_b = true;
        std::unique_lock lk{ring_mutex};
        for (;;) {
            ring_cv.wait(lk, [&] { return ring_stop || !ring.empty(); });
            if (ring.empty()) {
                return;
            }
            const bool flip = ring.front();
            ring.pop_front();
            stage_b_busy = true;
            lk.unlock();
            if (flip) {
                // SignalFlip with a landing pending: the interrupt goes to the landing thread.
                signals_issued.fetch_add(1);
                landing.Post([&] {
                    submit_flip();
                    signals_done.fetch_add(1);
                });
            } else {
                scheduler.Record();
            }
            lk.lock();
            stage_b_busy = false;
            ring_cv.notify_all();
        }
    }};

    on_stage_a = true;
    const auto drain_stage_b = [&] {
        std::unique_lock lk{ring_mutex};
        ring_cv.wait(lk, [&] { return ring.empty() && !stage_b_busy; });
    };
    const auto process_commands = [&] {
        {
            std::scoped_lock lk{command_mutex};
            if (commands.empty()) {
                return;
            }
        }
        drain_stage_b();
        for (;;) {
            std::function<void()> fn;
            {
                std::scoped_lock lk{command_mutex};
                if (commands.empty()) {
                    return;
                }
                fn = std::move(commands.front());
                commands.pop_front();
            }
            fn();
            commands_run.fetch_add(1);
        }
    };
    const auto hand_to_stage_b = [&](bool flip) {
        {
            std::scoped_lock lk{ring_mutex};
            ring.push_back(flip);
        }
        ring_cv.notify_all();
    };

    Result result;
    for (unsigned frame = 0; frame < frames && !result.deadlocked; ++frame) {
        for (unsigned draw = 0; draw < draws_per_frame; ++draw) {
            hand_to_stage_b(false);
            if (draw % 4 == 0) {
                process_commands(); // between packets, as the Process loop does
            }
        }
        hand_to_stage_b(true);
        // Stage B goes on recording the next frame while the flip is on the landing thread.
        for (unsigned draw = 0; draw < draws_per_frame / 2; ++draw) {
            hand_to_stage_b(false);
        }
        // The end of the submission: drain stage B and wait for the deferred signals. This
        // thread does not run its commands while it waits (it is inside the wait).
        drain_stage_b();
        const auto deadline = std::chrono::steady_clock::now() + wait_timeout;
        while (signals_done.load() < signals_issued.load()) {
            if (std::chrono::steady_clock::now() >= deadline) {
                result.deadlocked = true;
                break;
            }
            std::this_thread::yield();
        }
        process_commands();
        ++result.frames_done;
    }
    // Let a timed-out handler finish: its command is still queued for stage A.
    for (auto until = std::chrono::steady_clock::now() + 5s;
         (signals_done.load() < signals_issued.load() || commands_run.load() < signals_issued.load()) &&
         std::chrono::steady_clock::now() < until;) {
        process_commands();
        std::this_thread::yield();
    }
    {
        std::scoped_lock lk{ring_mutex};
        ring_stop = true;
    }
    ring_cv.notify_all();
    stage_b.join();
    on_stage_a = false;

    result.deadlocked = result.deadlocked || gave_up.load();
    result.overlaps = scheduler.overlaps.load();
    result.submits = scheduler.submits.load();
    result.submits_off_gpu_side = scheduler.submits_off_gpu_side.load();
    return result;
}

} // namespace

int main() {
    // A test that hangs fails instead.
    std::thread watchdog{[] {
        std::this_thread::sleep_for(60s);
        std::fprintf(stderr, "flip-handback-test: timed out (deadlock)\n");
        std::fflush(nullptr);
        std::_Exit(1);
    }};
    watchdog.detach();

    // 1. Only the GPU command thread and the draw recording thread prepare an EOP flip's frame
    // where the interrupt was raised; the landing, Vulkan recording and EOP flip worker threads
    // (neither) send it to the GPU command thread.
    static_assert(AmdGpu::PrepareEopFlipInline(true, false));
    static_assert(AmdGpu::PrepareEopFlipInline(false, true));
    static_assert(!AmdGpu::PrepareEopFlipInline(false, false));
    for (int bits = 0; bits < 4; ++bits) {
        assert(AmdGpu::PrepareEopFlipInline(bits & 1, bits & 2) == (bits != 0));
    }

    // 2. Deferred flips: every frame is prepared on stage A, never while stage B records, and
    // stage A's wait for the deferred flip signal does not deadlock (#112).
    const Result handback = RunModel(Route::Handback, 200, 16, 2000ms);
    std::printf("flip-handback-test: handback: %u frames, %u submits, %u overlaps, %u off the GPU "
                "side, deadlock %d\n",
                handback.frames_done, handback.submits, handback.overlaps,
                handback.submits_off_gpu_side, handback.deadlocked);
    assert(!handback.deadlocked);
    assert(handback.frames_done == 200);
    assert(handback.submits == 200);
    assert(handback.overlaps == 0);
    assert(handback.submits_off_gpu_side == 0);

    // 3. The deadlock check sees the #112 shape: a handler that waits for stage A while stage A
    // waits for the handler's signal.
    const Result blocking = RunModel(Route::WaitForStageA, 3, 4, 200ms);
    std::printf("flip-handback-test: waiting handler: deadlock %d (expected)\n",
                blocking.deadlocked);
    assert(blocking.deadlocked);

    std::printf("flip-handback-test: ok\n");
    return 0;
}
