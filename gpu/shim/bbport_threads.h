// SPDX-License-Identifier: GPL-2.0-or-later
// bbport: helper thread sizing. Counts follow the hardware threads this process may run on
// (the affinity mask, so `taskset` can emulate a Steam Deck), and speculative helpers run as
// SCHED_IDLE: they use cores the game leaves idle and never take time from its threads.

#pragma once

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <thread>
#ifdef _MSC_VER
#include <intrin.h>
#endif
#ifdef _WIN32
#include <bit>
#include <windows.h>
#else
#include <sched.h>
#include <sys/resource.h>
#include <unistd.h>
#endif

namespace BbThreads {
/// Set on the helper threads (draw preparation, preload): they are not the GPU thread.
inline thread_local bool background_thread = false;
inline bool IsBackground() {
    return background_thread;
}
#ifdef _WIN32
/// Hardware threads available to the process (its affinity mask).
inline unsigned Available() {
    DWORD_PTR process = 0, system = 0;
    if (GetProcessAffinityMask(GetCurrentProcess(), &process, &system) && process) {
        return std::max(1, std::popcount(static_cast<unsigned long long>(process)));
    }
    return std::max(1u, std::thread::hardware_concurrency());
}

/// The calling thread stays out of the way of the game's own threads: below normal, so it
/// still runs when the job workers (lowest) and the rest of the game use all cores. At the
/// idle level the draw preparation was starved by them and the GPU thread fell back to doing
/// the work itself.
inline void MakeBackground() {
    background_thread = true;
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
}

/// The calling thread yields to the game's own threads but is not starved by them.
inline void MakeBelowNormal() {
    background_thread = true;
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
}

/// Normal priority while in scope (a background helper that holds something the GPU thread may
/// wait for must not be starved by busy game threads).
struct ScopedNormalPriority {
    int previous = GetThreadPriority(GetCurrentThread());
    ScopedNormalPriority() {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_NORMAL);
    }
    ~ScopedNormalPriority() {
        SetThreadPriority(GetCurrentThread(), previous);
    }
};
#else

/// Hardware threads available to the process.
inline unsigned Available() {
    cpu_set_t set;
    CPU_ZERO(&set);
    if (sched_getaffinity(0, sizeof(set), &set) == 0) {
        return std::max(1, CPU_COUNT(&set));
    }
    return std::max(1u, std::thread::hardware_concurrency());
}

/// The calling thread only runs on otherwise idle cores (falls back to the lowest nice level).
inline void MakeBackground() {
    background_thread = true;
    sched_param param{};
    if (sched_setscheduler(0, SCHED_IDLE, &param) != 0) {
        setpriority(PRIO_PROCESS, static_cast<id_t>(gettid()), 19);
    }
}

/// The calling thread yields to the game's own threads but is not starved by them.
inline void MakeBelowNormal() {
    background_thread = true;
    setpriority(PRIO_PROCESS, static_cast<id_t>(gettid()), 5);
}

/// Does nothing here: a thread that went to SCHED_IDLE or nice 19 cannot raise itself again
/// without privileges, so a helper holding a claim is only slowed while every core is busy.
/// (The empty constructor keeps -Wunused-variable quiet at the call sites.)
struct ScopedNormalPriority {
    ScopedNormalPriority() {}
    ~ScopedNormalPriority() {}
};
#endif

/// Emits a CPU spin-loop hint without burning full ALU power.
inline void SpinPause() noexcept {
#if defined(__x86_64__) || defined(_M_X64) || defined(__i386__) || defined(_M_IX86)
#ifdef _MSC_VER
    _mm_pause();
#else
    __builtin_ia32_pause();
#endif
#elif defined(__aarch64__) || defined(_M_ARM64)
    __asm__ __volatile__("yield" ::: "memory");
#endif
}

/// Returns the spin budget in microseconds for a worker/wait loop, overridden by `env_name`
/// if set (clamped to >= 0). Defaults to `default_gte12_us` when >= 12 hardware threads are
/// available, or `default_lt12_us` on smaller CPUs.
inline int EnvSpinMicros(const char* env_name, int default_gte12_us, int default_lt12_us) {
    if (env_name) {
        if (const char* env = std::getenv(env_name); env && env[0]) {
            return std::max(0, std::atoi(env));
        }
    }
    return Available() >= 12 ? default_gte12_us : default_lt12_us;
}

/// Bounded spin-then-yield-then-sleep backoff for polling wait loops without a condvar.
/// Starts with `pause_limit` CPU pause hints, then `yield_limit` thread yields, then sleeps
/// for `sleep_duration` per step so stalled waits do not burn package power.
struct WaitBackoff {
    std::uint32_t step = 0;
    std::uint32_t pause_limit = 64;
    std::uint32_t yield_limit = 8;
    std::chrono::microseconds sleep_duration{50};

    void WaitOnce() {
        if (step < pause_limit) {
            SpinPause();
        } else if (step < pause_limit + yield_limit) {
            std::this_thread::yield();
        } else {
            std::this_thread::sleep_for(sleep_duration);
        }
        ++step;
    }

    void Reset() noexcept {
        step = 0;
    }
};

} // namespace BbThreads
