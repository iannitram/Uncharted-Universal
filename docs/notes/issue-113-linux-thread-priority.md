# Issue #113: is the Windows-only thread-priority fix needed on Linux?

Finding (code reading only; no device run): the Linux path is safe and no port is needed for the
first-fight job-system crash.

- `src/runtime_thread.c`, `host_priority()` and its `#ifdef _WIN32` block in `apply_priority()`:
  the Windows priority bands are applied only when `BB_THREAD_PRIO=1`. The default is off
  (since 0.3.11, per the comment above the block).
- `apply_priority()` has an `#else` branch that does nothing (`(void)t; (void)current_thread;`).
  So on Linux, `thread_set_prio` and the `host_start` call record the PS4 priority but never
  change the host thread's scheduling. This is the same as the Windows default with
  `BB_THREAD_PRIO` unset.
- `apply_affinity()` is the same: Windows-only, behind `BB_AFFINITY=1`.
- The crash fix the comment refers to is the job wait patch in `src/probe.c` (the
  "cmdBuf.m_bInUse == false" patch, `BB_JOB_LOCK_WAIT_MS`). It patches guest code and is not
  platform-specific, so it applies on Linux too.
- Rust: `crates/runtime` `thread` is still planned (`docs/code-map.md`, row `fiber`, `thread`).
  There is no Rust priority code to port yet. `sync/sema.rs` and `sync/mutex.rs` note that host
  threads run at equal priority.

Decision: no Linux scheduling port in this change. A `setpriority`/`pthread_setschedparam` port
would change behaviour on Linux only when `BB_THREAD_PRIO=1`, and raising priority needs
CAP_SYS_NICE. It would not affect the default build.

Still open: the first fight on the AMD handheld under SteamOS (needs-owner-device).
