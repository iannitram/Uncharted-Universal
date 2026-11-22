// SPDX-License-Identifier: GPL-2.0-only
#include "android/synthetic/fex_smoke.h"

#include <cassert>

int main() {
    BbFexSmokeReport report{};
    assert(bb_android_run_fex_smoke(nullptr) == -1);
    assert(bb_android_run_fex_smoke(&report) == 0);
    assert(report.scalar_ok == 1);
    assert(report.vector_ok == 1);
    assert(report.callback_ok == 1);
    assert(report.nested_callback_ok == 1);
    assert(report.tls_isolation_ok == 1);
    assert(report.memory_invalidation_ok == 1);
    assert(report.teardown_ok == 1);
    return 0;
}
