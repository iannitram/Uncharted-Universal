# Android dependency patches

`scripts/build_dependencies.sh` applies these series in file-name order right after
each checkout. It resets the checkout first, runs `git apply --check` on each patch
before applying it, and stops with the patch name if one does not apply.

- Source: [revvv991/uncharted-android](https://github.com/revvv991/uncharted-android)
  at `82fb03069`, `android/scripts/port-arm64/patches/`. The files are byte-for-byte
  copies under their original names. That repository is GPL-2.0-or-later; the
  patches modify MIT-licensed Mesa and FEX and are carried under MIT (see
  `../LICENSES.md`). Not taken: `experimental/` (the peer's build skips it too),
  `scripts/g2/`, `np3/` and the GPU tree.
- Issues: #247 (this import), #207 (the recipe).

## mesa/ (Mesa Turnip, KGSL)

Base: Mesa tag `mesa-26.2.5`, commit `ddaf85e353442d9d7f192fdcf1823e652e3f2ee3`
(the `mesa-turnip` pin in `../components.lock.json`, the same tag the peer's
`build_all.sh` clones). Numbers 0018-0021 belong to the peer's standalone Turnip
package and are not part of this series. 0016 is upstream Mesa's own commit
118ec7f4 (Connor Abbott, after 26.2); the others are by revvv991. The switches
named below are environment variables (0015 also reads them from
`debug.mesa.<name>` Android properties); the experiments 0004-0011 are off by default.

| Patch | What it does |
| --- | --- |
| mesa-0001-kgsl-zero-timeout-poll | KGSL: polls timestamps when asked to wait with a zero timeout (upstream later fixed it the same way, e984ef294ea) |
| mesa-0002-u-trace-gate | u_trace prints only while a gate file exists (`MESA_GPU_TRACE_GATE`) |
| mesa-0003-kgsl-diagnostics-bo-log-ctx-flags | Diagnostics: `TU_BO_LOG=1` logs BO allocations with addresses; `TU_KGSL_CTX_FLAGS` sets KGSL context flags |
| mesa-0004-memcheck-pipeline-shader-memory | Diagnostics: `TU_MEMCHECK=1` re-reads pipeline and shader memory 2 s after creation and reports changes |
| mesa-0005-ds-no-cross-experiment | Experiment: `TU_DS_NO_CROSS=1` keeps draw-state groups inside one page |
| mesa-0006-ib-align-draw-state-packets | Experiment: `TU_IB_ALIGN=<bytes>` keeps draw-state and register-bunch packets from crossing block boundaries |
| mesa-0007-ib-align-default-1k-load-state | Extends that alignment to `CP_LOAD_STATE6*`, 1 KiB by default |
| mesa-0008-br-sync-experiment | Experiment: `TU_BR_SYNC=1` makes BV wait for BR in sysmem passes |
| mesa-0009-cs-fill-nop-experiment | Experiment: `TU_CS_FILL_NOP=1` fills new command BOs with `CP_NOP` |
| mesa-0010-ib2-split-experiment | Experiment: `TU_IB2_SPLIT=<dwords>` cuts draw IB2s at draw boundaries |
| mesa-0011-ib-align-off-by-default | Turns the packet alignment off by default again |
| mesa-0012-ib2-split-on-by-default | Cuts draw IB2s every 256 dwords by default (`TU_IB2_SPLIT=0` disables) |
| mesa-0013-kgsl-context-recovery | KGSL: after repeated GPU faults the queue moves to a new context (`TU_KGSL_RECOVER`, on by default; `TU_KGSL_FAULT_TEST` to exercise it) |
| mesa-0014-cs-skip-page1 | Command streams used as IBs leave the second 4 KiB page of their BO unused (`TU_PAGE1_SKIP=0` disables) |
| mesa-0015-switches-from-properties | The switches above can also come from Android properties (environment wins); notes also go to the Android log |
| mesa-0016-32b-rollover-workaround | Upstream's workaround that keeps command BOs below 4 GB; a no-op on KGSL by itself |
| mesa-0017-kgsl-low4g-gpuobj | KGSL: allocates command BOs through `kgsl_gpuobj_alloc` so they do land below 4 GB (`TU_IB_LOW4G=0` disables) |
| mesa-0022-adreno-825 | Adds the Adreno 825 (KGSL chip id 0x44030000) as an a8xx_gen1 entry with 2 slices and 4 CCUs |
| mesa-0023-gralloc-newer-qcom-handle | u_gralloc fallback: reads the private handle of newer Qualcomm grallocs (no `gmsm` magic) |
| mesa-0024-low4g-not-on-a8xx | Keeps 0017's below-4 GB rule off on Adreno 8xx (`TU_IB_LOW4G=1` forces it) |
| mesa-0025-a8xx-sysmem-by-default | Sysmem rendering by default on Adreno 8xx (`TU_A8XX_GMEM=1` restores the autotuner choice) |
| mesa-0026-a8xx-ib-alias-and-cache | Adreno 8xx: command BOs behind a virtual BO alias plus a cache of retired command BOs (`TU_KGSL_IB_ALIAS=0`, `TU_KGSL_IB_CACHE=0`) |
| mesa-0027-a8xx-ib-alias-4gb-rule-and-guard | The 0026 alias follows the chip's 4 GB rule and gets a 64 KiB zero-mapped guard (`TU_KGSL_IB_GUARD=0` drops it) |

## fex/ (FEXCore)

Base: FEX commit `f2b679f6028ce1c38875233aecfcf5d3f8ebecec` (the `fex` pin). Our
recipe configures FEX with `-DBUILD_FEXCORE_ONLY=ON`, an option these patches add,
so the recipe needs them.

| Patch | What it does |
| --- | --- |
| fex-0001-android | Android (bionic, libc++) build of FEXCore and Common; adds the `BUILD_FEXCORE_ONLY` target |
| fex-0002-pause-wfe | The guest's `pause` becomes a `wfe` (`BB_PAUSE=yield` makes it a yield) |
| fex-0003-smoke-optional | `BUILD_FEXCORE_ONLY` no longer requires a smoke executable source |
