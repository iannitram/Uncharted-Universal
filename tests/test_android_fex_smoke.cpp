// SPDX-License-Identifier: GPL-2.0-only
#include "android/synthetic/fex_smoke.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace {

void Require(bool cond, const char* expr) {
    if (!cond) {
        std::fprintf(stderr, "test_android_fex_smoke failed: %s\n", expr);
        std::abort();
    }
}

#define CHECK(expr) Require(static_cast<bool>(expr), #expr)

} // namespace

int main() {
    BbFexSmokeReport report{};
    BbFexSmokeDetails details{};

    CHECK(bb_android_run_fex_smoke(nullptr) == -1);
    CHECK(bb_android_run_fex_smoke_detailed(nullptr, &details) == -1);

    CHECK(bb_android_run_fex_smoke(&report) == 0);
    CHECK(report.scalar_ok == 1);
    CHECK(report.vector_ok == 1);
    CHECK(report.callback_ok == 1);
    CHECK(report.nested_callback_ok == 1);
    CHECK(report.tls_isolation_ok == 1);
    CHECK(report.memory_invalidation_ok == 1);
    CHECK(report.teardown_ok == 1);

    report = BbFexSmokeReport{};
    CHECK(bb_android_run_fex_smoke_detailed(&report, &details) == 0);
    CHECK(details.page_size >= 4096);
    CHECK((details.page_size & (details.page_size - 1)) == 0);
    CHECK(details.rflags_ok == 1);
    CHECK(details.unaligned_ok == 1);
    CHECK(details.pointer_arg_ok == 1);
    CHECK(details.stack_arg_ok == 1);
    CHECK(details.vector_lanes_ok == 1);
    CHECK(details.overlap_rejected_ok == 1);
    CHECK(details.pthread_tls_dtor_ok == 1);
    CHECK(details.mapping_protect_ok == 1);
    CHECK(details.icache_invalidation_ok == 1);
    CHECK(details.threads_tested >= 4);
    CHECK(details.invalidation_epochs >= 2);
    CHECK(details.target_triple[0] != '\0');
    CHECK(std::strcmp(details.fex_revision, "f2b679f6028ce1c38875233aecfcf5d3f8ebecec") == 0);

    char formatted[1024]{};
    CHECK(bb_android_format_fex_smoke_report(nullptr, &details, formatted, sizeof(formatted)) == -1);
    CHECK(bb_android_format_fex_smoke_report(&report, &details, formatted, 8) == -1);
    CHECK(bb_android_format_fex_smoke_report(&report, &details, formatted, sizeof(formatted)) > 0);
    CHECK(std::strstr(formatted, "FEXCORE_ANDROID_SMOKE_OK") != nullptr);
    CHECK(std::strstr(formatted, "scalar=ok") != nullptr);
    CHECK(std::strstr(formatted, "vector=ok") != nullptr);
    CHECK(std::strstr(formatted, "rflags=ok") != nullptr);
    CHECK(std::strstr(formatted, "callback=ok") != nullptr);
    CHECK(std::strstr(formatted, "nested_callback=ok") != nullptr);
    CHECK(std::strstr(formatted, "tls=ok") != nullptr);
    CHECK(std::strstr(formatted, "overlap_rejected=ok") != nullptr);
    CHECK(std::strstr(formatted, "invalidation=ok") != nullptr);
    CHECK(std::strstr(formatted, "teardown=ok") != nullptr);
    return 0;
}
