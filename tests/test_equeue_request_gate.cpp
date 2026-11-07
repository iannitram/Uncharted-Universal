#include "gpu/shim/equeue_request_gate.h"

#include <atomic>
#include <cassert>
#include <chrono>
#include <condition_variable>
#include <mutex>
#include <thread>

int main() {
    using namespace std::chrono_literals;
    Libraries::Kernel::EqueueRequestGate gate;
    std::mutex mutex;
    std::condition_variable condition;
    bool in_run = false;
    bool finish_run = false;
    std::atomic<unsigned> passes{0};

    std::jthread service([&](std::stop_token stop) {
        while (gate.Wait(stop)) {
            const unsigned pass = ++passes;
            if (pass == 1) {
                std::unique_lock lock{mutex};
                in_run = true;
                condition.notify_one();
                condition.wait(lock, [&] { return finish_run; });
            }
        }
    });

    gate.Signal();
    {
        std::unique_lock lock{mutex};
        assert(condition.wait_for(lock, 2s, [&] { return in_run; }));
    }

    // Signals arrive while the service's work loop is active. They must stay
    // pending so the next wait cannot sleep with work queued.
    constexpr unsigned signals = 10000;
    std::jthread producer([&] {
        for (unsigned i = 0; i < signals; ++i) gate.Signal();
    });
    producer.join();
    {
        std::lock_guard lock{mutex};
        finish_run = true;
    }
    condition.notify_one();

    const auto deadline = std::chrono::steady_clock::now() + 2s;
    while (passes.load() < 2 && std::chrono::steady_clock::now() < deadline)
        std::this_thread::yield();
    assert(passes.load() >= 2);
    service.request_stop();
    gate.Signal(); // Wake the stop-aware wait so the thread can exit.
}
