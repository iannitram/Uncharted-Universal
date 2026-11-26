// SPDX-License-Identifier: GPL-2.0-or-later
// bbport (#198): Canonical shim-to-shadPS4-core boundary contract.
//
// Everything under gpu/shadps4/ (or an external BB_SHADPS4_CORE_DIR checkout)
// that calls into Uncharted-Universal's runtime, settings, or title hooks
// crosses through one of two surfaces:
//   1. The C ABI in gpu/bbgpu.h (host <-> GPU library).
//   2. The shim headers in gpu/shim/ listed below (core <-> title hooks/runtime).
//
// Keeping this boundary explicit allows gpu/ to build against either the
// vendored gpu/shadps4/ copy or an external shared shadPS4 core tree via
// -DBB_SHADPS4_CORE_DIR=<path>.
#pragma once

#define BBPORT_SHIM_BOUNDARY_VERSION 1

#include "bbgpu.h"
#include "bbport_overlay.h"
#include "bbport_copy.h"
#include "bbport_diag.h"
#include "bbport_focus_fix.h"
#include "bbport_movie.h"
#include "bbport_nan_guard.h"
#include "bbport_resize.h"
#include "bbport_settings.h"
#include "bbport_shader_preload.h"
#include "bbport_stereo.h"
#include "bbport_threads.h"
#include "bbport_toggles.h"
#include "bbport_video_pacing.h"
#include "bbport_write_log.h"
#include "vertex_input_deny.h"

namespace BbCoreBoundary {

inline constexpr int Version = BBPORT_SHIM_BOUNDARY_VERSION;

// Header names in gpu/shim/ that gpu/shadps4/ (or BB_SHADPS4_CORE_DIR) is
// permitted to include directly across the core/shim boundary.
inline constexpr const char* const AllowedShimHeaders[] = {
    "bbgpu.h",
    "bbport_copy.h",
    "bbport_core_boundary.h",
    "bbport_diag.h",
    "bbport_dlss_policy.h",
    "bbport_focus_fix.h",
    "bbport_movie.h",
    "bbport_nan_guard.h",
    "bbport_overlay.h",
    "bbport_resize.h",
    "bbport_settings.h",
    "bbport_shader_preload.h",
    "bbport_stereo.h",
    "bbport_threads.h",
    "bbport_toggles.h",
    "bbport_video_pacing.h",
    "bbport_write_log.h",
    "dlss_bridge/bbport_dlss_bridge.h",
    "sdl_window.h",
    "vertex_input_deny.h",
};

} // namespace BbCoreBoundary
