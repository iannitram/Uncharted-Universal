#pragma once

#include <condition_variable>
#include <mutex>
#include <stop_token>

namespace Libraries::Kernel {
// Keeps signals sent while io_context::run() is active pending for the next pass.
class EqueueRequestGate {
public:
    void Signal() {
        {
            std::lock_guard lock{mutex};
            ++requests;
        }
        condition.notify_one();
    }

    bool Wait(std::stop_token stop) {
        std::unique_lock lock{mutex};
        condition.wait(lock, stop, [this] { return requests != 0; });
        if (stop.stop_requested()) return false;
        requests = 0;
        return true;
    }

private:
    std::mutex mutex;
    std::condition_variable_any condition;
    unsigned requests = 0;
};
} // namespace Libraries::Kernel
