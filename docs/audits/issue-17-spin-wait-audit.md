# Issue #17 Audit: Worker Thread Spin/Wait Loops and Power Impact

Refs #17 — *Performance: Uncharted 1 at 1080p is GPU-bound at 20 W; check whether spinning worker threads steal power.*

## Summary

On power-limited APUs (e.g. 20 W handhelds), CPU cores spinning in busy-wait or tight `sched_yield()` loops draw package power away from the integrated GPU, directly reducing GPU clocks when GPU-bound.

We audited both the Rust runtime (`crates/runtime/src/{thread,sync,time,fiber,os,abi/libc}.rs`), the C loader (`src/probe.c`), and the C++ GPU translation (`gpu/shadps4/` and `gpu/shim/`).

## Findings

### 1. Rust Runtime (`crates/runtime/src/`) & Loader (`src/probe.c`)
- **`crates/runtime/src/thread.rs`, `sync.rs`, `sync/{mutex,rwlock,sema}.rs`, `fiber.rs`**: No busy-wait loops. All guest mutex/condvar/rwlock/semaphore/join waits block on `std::sync::Condvar` (`wait` / `wait_timeout`).
- **`src/probe.c` (`patch_job_sleep`)**: Patches Naughty Dog's `NdJobWorkerThread` idle spin to sleep on the job manager's condition variable (default `BB_JOB_SLEEP_MS=1` ms on Linux; `BB_JOB_SLEEP=0` disables). Note: on Windows with `> 12` hardware threads, `patch_job_sleep` defaults to spinning unless `BB_JOB_SLEEP=1` is set; on Linux it is enabled by default.
- **`src/probe.c` (`patch_job_lock_wait`)**: Already sleeps (`usleep(100)`) between retries.

### 2. GPU Translation (`gpu/shadps4/` & `gpu/shim/`)
Several GPU-side worker and synchronization loops spun without bounded backoff or env-tunable spin budgets:

1. **`gpu/shadps4/video_core/amdgpu/liverpool.cpp` (`Liverpool::Process`, `bb:GpuCmd`)**:
   When all mapped queues were empty or yielded on `WaitRegMem`/`MemSemaphore`/`Rewind`, `waiting_visits` exceeded `num_mapped_queues` and the command processor spun in a tight loop with no pause, yield, or sleep.
   - **Fix**: Added `BB_GPU_SPIN_US` (default 50 us on `>= 12` threads, 20 us otherwise) followed by `BbThreads::WaitBackoff` (4 yields then 50 us sleep per round).

2. **`gpu/shadps4/video_core/renderer_vulkan/vk_scheduler.cpp`**:
   - `Scheduler::WaitDeferredSignals` and `Scheduler::WaitHostCopies`: spun indefinitely on `std::this_thread::yield()`. Replaced with `BbThreads::WaitBackoff` (pause -> yield -> 50 us sleep).
   - `Scheduler::RecorderThread` (`bb:VkRecorder`): hardcoded `200 : 20` us spin before `recorder_cv.wait()`. Added `BB_RECORDER_SPIN_US` env override via `BbThreads::EnvSpinMicros`.

3. **`gpu/shadps4/video_core/renderer_vulkan/vk_draw_pipe.h`**:
   - `WaitForSpace`: unbounded `__builtin_ia32_pause()` spin when the 16 MiB draw-pipe ring was full. Replaced with `BbThreads::WaitBackoff` (4096 pauses -> 32 yields -> 50 us sleep).
   - `Run` (`bb:DrawRec`): unified `BB_PIPE_SPIN_US` handling via `BbThreads::EnvSpinMicros`.

4. **`gpu/shadps4/video_core/renderer_vulkan/vk_bind_helper.h`**:
   - `Join`: unbounded `__builtin_ia32_pause()` spin. Replaced with `BbThreads::WaitBackoff`.
   - `Run` (`bb:TexBind`): hardcoded 100 us spin. Replaced with `BbThreads::EnvSpinMicros("BB_BIND_SPIN_US", 100, 20)`.

5. **`gpu/shadps4/core/libraries/videoout/video_out.cpp` (`sceVideoOutSubmitEopFlip`)**:
   - Up to 20 ms tight `std::this_thread::yield()` loop waiting on `port->buffer_labels[buf_id]`. Replaced with `BbThreads::WaitBackoff`.

6. **`gpu/shadps4/core/libraries/kernel/equeue.cpp` (`EqueueInternal::WaitForSmallTimer`)**:
   - Tight `std::this_thread::yield()` loop locking `m_mutex` on every iteration while waiting for a small timer deadline. Now sleeps until `min(wait_end_us, next_deadline) - 50 us` when > 100 us away, and uses `BbThreads::WaitBackoff` for the final window.

## Environment Knobs for Power-Limited Devices
Setting these to `0` (or `20`) eliminates GPU worker spin-wait power draw at the cost of a wakeup syscall on idle gaps:
- `BB_GPU_SPIN_US=0` (`bb:GpuCmd` queue-wait spin)
- `BB_PIPE_SPIN_US=0` (`bb:DrawRec` idle spin)
- `BB_RECORDER_SPIN_US=0` (`bb:VkRecorder` idle spin)
- `BB_BIND_SPIN_US=0` (`bb:TexBind` idle spin)
- `BB_JOB_SLEEP=1` / `BB_JOB_SLEEP_MS=1` (game `NdJobWorkerThread` idle sleep, already default on Linux)
