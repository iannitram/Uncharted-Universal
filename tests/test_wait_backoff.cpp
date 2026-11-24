#include <cassert>
#include <chrono>
#include <cstdlib>

#include "bbport_threads.h"

#ifdef _WIN32
static void set_env_var(const char* name, const char* value) {
    _putenv_s(name, value ? value : "");
}
#else
static void set_env_var(const char* name, const char* value) {
    if (value) {
        setenv(name, value, 1);
    } else {
        unsetenv(name);
    }
}
#endif

int main() {
    // 1. EnvSpinMicros: respects explicit override, clamps negatives, falls back by core count.
    set_env_var("BB_TEST_SPIN_US", nullptr);
    const int expected_default = BbThreads::Available() >= 12 ? 200 : 20;
    assert(BbThreads::EnvSpinMicros("BB_TEST_SPIN_US", 200, 20) == expected_default);

    set_env_var("BB_TEST_SPIN_US", "0");
    assert(BbThreads::EnvSpinMicros("BB_TEST_SPIN_US", 200, 20) == 0);

    set_env_var("BB_TEST_SPIN_US", "75");
    assert(BbThreads::EnvSpinMicros("BB_TEST_SPIN_US", 200, 20) == 75);

    set_env_var("BB_TEST_SPIN_US", "-15");
    assert(BbThreads::EnvSpinMicros("BB_TEST_SPIN_US", 200, 20) == 0);
    set_env_var("BB_TEST_SPIN_US", nullptr);

    // 2. WaitBackoff: progresses through pause -> yield -> sleep and resets cleanly.
    BbThreads::WaitBackoff backoff{0, 2, 2, std::chrono::microseconds{200}};
    assert(backoff.step == 0);

    // Steps 0..1: pause phase (fast, no sleep).
    backoff.WaitOnce();
    assert(backoff.step == 1);
    backoff.WaitOnce();
    assert(backoff.step == 2);

    // Steps 2..3: yield phase.
    backoff.WaitOnce();
    assert(backoff.step == 3);
    backoff.WaitOnce();
    assert(backoff.step == 4);

    // Step 4+: sleep phase (must actually sleep >= ~100us).
    const auto t0 = std::chrono::steady_clock::now();
    backoff.WaitOnce();
    const auto elapsed = std::chrono::steady_clock::now() - t0;
    assert(backoff.step == 5);
    assert(elapsed >= std::chrono::microseconds{50});

    backoff.Reset();
    assert(backoff.step == 0);
    return 0;
}
