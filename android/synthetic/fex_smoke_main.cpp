// SPDX-License-Identifier: GPL-2.0-only
#include "android/synthetic/fex_smoke.h"

#include <cstdio>

int main() {
    BbFexSmokeReport report{};
    BbFexSmokeDetails details{};
    const int rc = bb_android_run_fex_smoke_detailed(&report, &details);

    char line[1024]{};
    if (bb_android_format_fex_smoke_report(&report, &details, line, sizeof(line)) < 0) {
        std::fprintf(stderr, "FEXCORE_ANDROID_SMOKE_FAIL reason=format\n");
        return 1;
    }
    std::fprintf(rc == 0 ? stdout : stderr, "%s\n", line);
    return rc == 0 ? 0 : 1;
}
