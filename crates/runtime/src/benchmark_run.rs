//! One benchmark run, from `BB_BENCHMARK` to `benchmark.json` (#173, #242).
//!
//! [`Session`] is the whole run as a pure state machine over a microsecond clock that its
//! caller supplies: the pad schedule (the title-screen lead-in, then the camera path), the frame
//! samples, the expiry and the report. The `uncharted_benchmark_*` functions at the bottom hold
//! one session in a global, read the real clock and are what the C loader and the GPU library
//! call (`src/runtime_benchmark.c`); nothing else in the runtime knows about benchmarks.
//!
//! The run is off unless `BB_BENCHMARK` is set: [`uncharted_benchmark_pad`] and
//! [`uncharted_benchmark_frame`] then return 0 after one atomic load and touch nothing.
//!
//! The clock starts at the first pad read, so loading before the game opens its pad does not
//! count. The measured interval of `BB_BENCHMARK` seconds starts [`EntryScript::DONE_MS`] later;
//! frames before it are not sampled, and the first frame at or after its end writes the report.

use std::ffi::{c_char, CStr};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};
use std::time::Instant;

use crate::benchmark::{Config, Ended, Game, Samples};
use crate::pad::{BenchmarkPath, EntryScript, Sample};

const MICROS_PER_MS: u64 = 1_000;
const MICROS_PER_SECOND: u64 = 1_000_000;
/// The camera path advances at 60 Hz, as [`BenchmarkPath`] defines it.
const TICKS_PER_SECOND: u64 = 60;
const ENTRY_US: u64 = EntryScript::DONE_MS * MICROS_PER_MS;

/// Where a run is, by time since the first pad read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Confirming the title menu and waiting for the level.
    Entering,
    /// Turning the camera and sampling frames.
    Measuring,
    /// The requested seconds are over; the next frame writes the report.
    Expired,
}

/// What a frame did to the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Running,
    /// The report is written; the process should exit now.
    Finished,
}

/// One benchmark run. All times are microseconds on one monotonic clock chosen by the caller.
#[derive(Debug)]
pub struct Session {
    config: Config,
    game: Game,
    saves_folder: PathBuf,
    samples: Samples,
    origin_us: Option<u64>,
    last_frame_us: Option<u64>,
    finished: bool,
}

impl Session {
    pub fn new(config: Config, game: Game, saves_folder: PathBuf) -> Self {
        Self {
            config,
            game,
            saves_folder,
            samples: Samples::default(),
            origin_us: None,
            last_frame_us: None,
            finished: false,
        }
    }

    fn measure_us(&self) -> u64 {
        u64::from(self.config.seconds) * MICROS_PER_SECOND
    }

    /// The phase `elapsed_us` after the first pad read.
    pub fn phase(&self, elapsed_us: u64) -> Phase {
        if elapsed_us < ENTRY_US {
            Phase::Entering
        } else if elapsed_us - ENTRY_US < self.measure_us() {
            Phase::Measuring
        } else {
            Phase::Expired
        }
    }

    /// The input for a pad read at `now_us`. The first read starts the run's clock.
    pub fn pad(&mut self, now_us: u64) -> Sample {
        let origin = *self.origin_us.get_or_insert(now_us);
        let elapsed = now_us.saturating_sub(origin);
        if self.finished {
            return Sample::NEUTRAL;
        }
        match self.phase(elapsed) {
            Phase::Entering => EntryScript.sample_at_ms(elapsed / MICROS_PER_MS),
            Phase::Measuring => {
                let tick = (elapsed - ENTRY_US) * TICKS_PER_SECOND / MICROS_PER_SECOND;
                BenchmarkPath.sample_at_tick(tick)
            }
            Phase::Expired => Sample::NEUTRAL,
        }
    }

    /// A frame was presented at `now_us` while `vram_mib` MiB of video memory was in use.
    ///
    /// Inside the measured interval the time since the previous frame is sampled. The first
    /// frame at or after its end writes the report and returns [`Progress::Finished`]; if that
    /// fails (no frame was sampled, the folder is not writable) the error is returned once and
    /// the run is over all the same, so the process still exits.
    pub fn frame(&mut self, now_us: u64, vram_mib: u64) -> io::Result<Progress> {
        if self.finished {
            return Ok(Progress::Finished);
        }
        let previous = self.last_frame_us.replace(now_us);
        let Some(origin) = self.origin_us else {
            return Ok(Progress::Running);
        };
        match self.phase(now_us.saturating_sub(origin)) {
            Phase::Entering => Ok(Progress::Running),
            Phase::Expired => self.finish(Ended::Ok).map(|_| Progress::Finished),
            Phase::Measuring => {
                // Only intervals that lie wholly inside the measured window count.
                if let Some(previous) = previous {
                    if previous.saturating_sub(origin) >= ENTRY_US && now_us > previous {
                        let frame_ms = (now_us - previous) as f64 / MICROS_PER_MS as f64;
                        self.samples.record_frame(frame_ms, vram_mib)?;
                    }
                }
                Ok(Progress::Running)
            }
        }
    }

    /// The process is dying: write what was sampled with `ended: "crash"`, if anything was.
    pub fn crash(&mut self) -> io::Result<()> {
        if self.finished || self.samples.is_empty() {
            return Ok(());
        }
        self.finish(Ended::Crash).map(|_| ())
    }

    fn finish(&mut self, ended: Ended) -> io::Result<PathBuf> {
        self.finished = true;
        let report = self.samples.finish(
            self.game,
            self.config.seconds,
            self.config.settings.clone(),
            ended,
        )?;
        report.write_to(&self.saves_folder)?;
        Ok(self.saves_folder.join("benchmark.json"))
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Reads `BB_BENCHMARK` (and `executable`, `BB_EXE` as `run.py` sets it; `None` is `u1`) and
/// builds the session, or `None` when no benchmark was asked for.
pub fn session_from(
    benchmark: Option<&str>,
    executable: Option<&str>,
    saves_folder: &Path,
    environment: impl IntoIterator<Item = (String, String)>,
) -> io::Result<Option<Session>> {
    let Some(config) = Config::from_values(benchmark, environment)? else {
        return Ok(None);
    };
    let game = match executable {
        None => Game::U1,
        Some(name) => Game::from_executable(name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "BB_EXE names no known Uncharted executable",
            )
        })?,
    };
    Ok(Some(Session::new(config, game, saves_folder.to_path_buf())))
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static CLOCK: OnceLock<Instant> = OnceLock::new();
/// Tests set the clock; `u64::MAX` means the real one.
#[cfg(test)]
static TEST_CLOCK_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);

fn now_us() -> u64 {
    #[cfg(test)]
    {
        let fake = TEST_CLOCK_US.load(Ordering::Relaxed);
        if fake != u64::MAX {
            return fake;
        }
    }
    CLOCK.get_or_init(Instant::now).elapsed().as_micros() as u64
}

fn session() -> MutexGuard<'static, Option<Session>> {
    SESSION.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the C side gets when a call ends the run: see [`uncharted_benchmark_frame`].
const RUNNING: i32 = 0;
const FINISHED: i32 = 1;
const FAILED: i32 = 2;

/// The pad state the benchmark wants the game to see (the C `PadData` fields it sets).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BenchmarkPad {
    pub buttons: u32,
    pub left_x: u8,
    pub left_y: u8,
    pub right_x: u8,
    pub right_y: u8,
}

impl From<Sample> for BenchmarkPad {
    fn from(sample: Sample) -> Self {
        Self {
            buttons: sample.buttons,
            left_x: sample.left_x,
            left_y: sample.left_y,
            right_x: sample.right_x,
            right_y: sample.right_y,
        }
    }
}

/// Called once from `main` after the options are read. `saves_dir` is the `--user` folder (the
/// saves folder; `benchmark.json` goes there). Returns 1 when a benchmark run is on, 0 when
/// `BB_BENCHMARK` is not set, and -1 after printing why when it is set but cannot run (a bad
/// value, an unknown `BB_EXE`, a saves folder that is not UTF-8): the caller must then exit
/// instead of playing a normal session the user did not ask for.
///
/// # Safety
/// `saves_dir` is null or points to a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn uncharted_benchmark_start(saves_dir: *const c_char) -> i32 {
    let Some(benchmark) = std::env::var_os("BB_BENCHMARK") else {
        return 0;
    };
    let result = (|| {
        let benchmark = benchmark
            .to_str()
            .ok_or_else(|| io::Error::other("BB_BENCHMARK is not valid UTF-8"))?;
        let executable = std::env::var_os("BB_EXE");
        let executable = match &executable {
            None => None,
            Some(name) => Some(
                name.to_str()
                    .ok_or_else(|| io::Error::other("BB_EXE is not valid UTF-8"))?,
            ),
        };
        if saves_dir.is_null() {
            return Err(io::Error::other("no saves folder"));
        }
        // SAFETY: the caller guarantees a NUL-terminated string; null was rejected above.
        let saves = unsafe { CStr::from_ptr(saves_dir) }
            .to_str()
            .map_err(|_| io::Error::other("the saves folder is not valid UTF-8"))?;
        let environment = std::env::vars_os()
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)));
        session_from(Some(benchmark), executable, Path::new(saves), environment)
    })();
    match result {
        Ok(Some(started)) => {
            *session() = Some(started);
            ACTIVE.store(true, Ordering::Release);
            println!("Benchmark: on (BB_BENCHMARK)");
            1
        }
        Ok(None) => 0,
        Err(error) => {
            eprintln!("ERROR: benchmark: {error}");
            -1
        }
    }
}

/// 1 while a benchmark run is on (so the game's own pad input is not used), else 0.
#[no_mangle]
pub extern "C" fn uncharted_benchmark_active() -> i32 {
    i32::from(ACTIVE.load(Ordering::Acquire))
}

/// Fills `out` and returns 1 when the benchmark owns this pad read; 0 (and `out` untouched)
/// when no benchmark is on.
///
/// # Safety
/// `out` is null or points to a writable [`BenchmarkPad`].
#[no_mangle]
pub unsafe extern "C" fn uncharted_benchmark_pad(out: *mut BenchmarkPad) -> i32 {
    if !ACTIVE.load(Ordering::Acquire) || out.is_null() {
        return 0;
    }
    let mut guard = session();
    let Some(run) = guard.as_mut() else {
        return 0;
    };
    // SAFETY: non-null, and the caller guarantees it is writable.
    unsafe { out.write(run.pad(now_us()).into()) };
    1
}

/// A game frame was presented. `vram_mib` is the video memory in use. Returns 0 while the run
/// goes on (and always when no benchmark is on), 1 once the report is written (exit now, code
/// 0), 2 when the run ended without a report (printed; exit with a failure code).
#[no_mangle]
pub extern "C" fn uncharted_benchmark_frame(vram_mib: u64) -> i32 {
    if !ACTIVE.load(Ordering::Acquire) {
        return RUNNING;
    }
    let mut guard = session();
    let Some(run) = guard.as_mut() else {
        return RUNNING;
    };
    let was_finished = run.is_finished();
    match run.frame(now_us(), vram_mib) {
        Ok(Progress::Running) => RUNNING,
        Ok(Progress::Finished) => {
            if !was_finished {
                println!("Benchmark: report written to benchmark.json");
            }
            FINISHED
        }
        Err(error) => {
            eprintln!("ERROR: benchmark: no report written: {error}");
            FAILED
        }
    }
}

/// The process is about to die from a fault: writes the report with `ended: "crash"` when frames
/// were sampled. Never blocks (a fault inside the benchmark's own lock is skipped).
#[no_mangle]
pub extern "C" fn uncharted_benchmark_crash() {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let mut guard = match SESSION.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    if let Some(run) = guard.as_mut() {
        // Best effort: the process is dying and there is no one to tell.
        let _ = run.crash();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::atomic::AtomicU64;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("uu-bench-run-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn config(seconds: u32) -> Config {
        Config::from_values(
            Some(&seconds.to_string()),
            [("BB_RESOLUTION".to_string(), "1280x720".to_string())],
        )
        .unwrap()
        .unwrap()
    }

    fn session_in(dir: &TempDir, seconds: u32) -> Session {
        Session::new(config(seconds), Game::U2, dir.0.clone())
    }

    const ORIGIN: u64 = 5_000_000;

    #[test]
    fn env_values_select_the_game_and_reject_what_cannot_run() {
        let dir = Path::new("saves");
        assert!(session_from(None, Some("u2"), dir, []).unwrap().is_none());
        let run = session_from(Some("30"), Some("big3-ps4_Shipping.elf"), dir, [])
            .unwrap()
            .unwrap();
        assert_eq!(run.game, Game::U3);
        assert_eq!(run.config.seconds, 30);
        let default = session_from(Some("30"), None, dir, []).unwrap().unwrap();
        assert_eq!(default.game, Game::U1);
        let named = session_from(Some("30"), Some("C:\\g\\u2"), dir, []);
        assert_eq!(named.unwrap().unwrap().game, Game::U2);
        for bad in ["", "0", "abc", "-5", "1e3"] {
            assert!(
                session_from(Some(bad), Some("u1"), dir, []).is_err(),
                "{bad}"
            );
        }
        assert!(session_from(Some("30"), Some("other.elf"), dir, []).is_err());
    }

    #[test]
    fn pad_enters_the_level_then_replays_the_camera_path_from_the_measured_start() {
        let dir = TempDir::new();
        let mut run = session_in(&dir, 90);
        // The first read starts the clock, whatever the absolute time is.
        assert_eq!(run.pad(ORIGIN), Sample::NEUTRAL);
        let ms = |ms: u64| ORIGIN + ms * MICROS_PER_MS;
        let cross = crate::pad::button::CROSS;
        assert_eq!(run.pad(ms(EntryScript::TITLE_WAIT_MS)).buttons, cross);
        assert_eq!(run.pad(ms(EntryScript::SECOND_PRESS_MS)).buttons, cross);
        assert_eq!(run.pad(ms(EntryScript::SECOND_PRESS_MS + 1_000)).buttons, 0);
        let start = ms(EntryScript::DONE_MS);
        assert_eq!(run.phase(start - ORIGIN), Phase::Measuring);
        assert_eq!(run.pad(start), BenchmarkPath.sample_at_tick(0));
        // 8 s later the camera turns the other way, exactly as the schedule says.
        assert_eq!(run.pad(start + 8_000_000).right_x, 195);
        assert_eq!(run.pad(start + 16_000_000).right_x, 60);
        // After the end the game gets a neutral pad.
        assert_eq!(run.pad(start + 91_000_000), Sample::NEUTRAL);
    }

    #[test]
    fn two_runs_see_the_same_input_sequence() {
        let sequence = || {
            let dir = TempDir::new();
            let mut run = session_in(&dir, 20);
            let first = 123_456;
            (0..(EntryScript::DONE_MS / 1000 + 25) * 60)
                .map(|n| run.pad(first + n * 1_000_000 / 60))
                .collect::<Vec<_>>()
        };
        assert_eq!(sequence(), sequence());
    }

    #[test]
    fn frames_before_the_pad_or_in_the_lead_in_are_not_sampled() {
        let dir = TempDir::new();
        let mut run = session_in(&dir, 5);
        assert_eq!(run.frame(1_000, 100).unwrap(), Progress::Running);
        run.pad(ORIGIN);
        for n in 0..100 {
            let now = ORIGIN + n * 16_000;
            assert_eq!(run.frame(now, 9_999).unwrap(), Progress::Running);
        }
        assert!(run
            .samples
            .clone()
            .finish(Game::U2, 5, BTreeMap::new(), Ended::Ok)
            .is_err());
    }

    #[test]
    fn report_is_written_on_expiry_with_the_sampled_frames() {
        let dir = TempDir::new();
        let mut run = session_in(&dir, 2);
        run.pad(ORIGIN);
        let start = ORIGIN + ENTRY_US;
        // A frame just before the window starts is the interval's left edge, not a sample.
        assert_eq!(run.frame(start - 5_000, 7_000).unwrap(), Progress::Running);
        let mut now = start;
        let mut count = 0;
        while now < start + 2_000_000 {
            // 16 ms frames, with every 50th taking 50 ms (a stall).
            now += if count % 50 == 49 { 50_000 } else { 16_000 };
            if now < start + 2_000_000 {
                assert_eq!(run.frame(now, 4_000 + count).unwrap(), Progress::Running);
            }
            count += 1;
        }
        assert!(!dir.0.join("benchmark.json").exists());
        assert_eq!(run.frame(start + 2_000_000, 1).unwrap(), Progress::Finished);
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.0.join("benchmark.json")).unwrap()).unwrap();
        assert_eq!(report["game"], "u2");
        assert_eq!(report["seconds"], 2);
        assert_eq!(report["ended"], "ok");
        assert_eq!(report["settings"]["BB_RESOLUTION"], "1280x720");
        assert_eq!(report["frame_ms"]["p50"], 16.0);
        assert_eq!(report["frame_ms"]["p99"], 50.0);
        assert_eq!(report["stalls_over_40ms"], 2);
        // The peak is over the sampled frames only (not the 7000 before the window).
        assert!(report["vram_peak_mib"].as_u64().unwrap() < 7_000);
        assert!(report["avg_fps"].as_f64().unwrap() > 50.0);
        // Later frames keep saying "exit" and do not write again.
        assert!(run.is_finished());
        assert_eq!(run.frame(start + 3_000_000, 1).unwrap(), Progress::Finished);
    }

    #[test]
    fn an_empty_window_fails_without_a_report_but_still_ends_the_run() {
        let dir = TempDir::new();
        let mut run = session_in(&dir, 1);
        run.pad(ORIGIN);
        let end = ORIGIN + ENTRY_US + 1_000_000;
        assert!(run.frame(end, 1).is_err());
        assert!(run.is_finished());
        assert!(!dir.0.join("benchmark.json").exists());
        assert_eq!(run.frame(end + 16_000, 1).unwrap(), Progress::Finished);
    }

    #[test]
    fn a_crash_writes_the_report_with_what_was_sampled() {
        let dir = TempDir::new();
        let mut run = session_in(&dir, 60);
        run.pad(ORIGIN);
        let start = ORIGIN + ENTRY_US;
        run.crash().unwrap(); // nothing sampled: nothing written
        assert!(!dir.0.join("benchmark.json").exists());
        let mut run = session_in(&dir, 60);
        run.pad(ORIGIN);
        for n in 0..10 {
            run.frame(start + n * 20_000, 2_048).unwrap();
        }
        run.crash().unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.0.join("benchmark.json")).unwrap()).unwrap();
        assert_eq!(report["ended"], "crash");
        assert_eq!(report["vram_peak_mib"], 2_048);
    }

    /// The exported C functions, end to end on the real globals. One test only: the globals
    /// are shared by the whole test binary.
    #[test]
    fn exported_functions_do_nothing_until_started_then_run_to_exit() {
        let dir = TempDir::new();
        let saves = std::ffi::CString::new(dir.0.to_str().unwrap()).unwrap();
        let mut pad = BenchmarkPad {
            buttons: 0xdead,
            ..Default::default()
        };
        // Off: no variable, no effect at all.
        std::env::remove_var("BB_BENCHMARK");
        assert_eq!(unsafe { uncharted_benchmark_start(saves.as_ptr()) }, 0);
        assert_eq!(uncharted_benchmark_active(), 0);
        assert_eq!(unsafe { uncharted_benchmark_pad(&mut pad) }, 0);
        assert_eq!(pad.buttons, 0xdead);
        assert_eq!(uncharted_benchmark_frame(1), 0);
        uncharted_benchmark_crash();
        assert!(!dir.0.join("benchmark.json").exists());
        // A bad value must stop the launch, not start a normal session.
        std::env::set_var("BB_BENCHMARK", "soon");
        assert_eq!(unsafe { uncharted_benchmark_start(saves.as_ptr()) }, -1);
        assert_eq!(uncharted_benchmark_active(), 0);

        std::env::set_var("BB_BENCHMARK", "1");
        std::env::set_var("BB_EXE", "u2");
        assert_eq!(unsafe { uncharted_benchmark_start(saves.as_ptr()) }, 1);
        assert_eq!(uncharted_benchmark_active(), 1);
        TEST_CLOCK_US.store(1_000_000, Ordering::Relaxed);
        assert_eq!(unsafe { uncharted_benchmark_pad(&mut pad) }, 1);
        assert_eq!(pad, BenchmarkPad::from(Sample::NEUTRAL));
        let start = 1_000_000 + ENTRY_US;
        TEST_CLOCK_US.store(start, Ordering::Relaxed);
        assert_eq!(uncharted_benchmark_frame(3_000), 0);
        for n in 1..=60 {
            TEST_CLOCK_US.store(start + n * 16_000, Ordering::Relaxed);
            assert_eq!(uncharted_benchmark_frame(3_000 + n), 0);
        }
        assert_eq!(unsafe { uncharted_benchmark_pad(&mut pad) }, 1);
        assert_eq!(pad.right_x, 60);
        TEST_CLOCK_US.store(start + 1_000_000, Ordering::Relaxed);
        assert_eq!(uncharted_benchmark_frame(1), 1);
        assert_eq!(uncharted_benchmark_frame(1), 1);
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.0.join("benchmark.json")).unwrap()).unwrap();
        assert_eq!(report["game"], "u2");
        assert_eq!(report["ended"], "ok");
        assert_eq!(report["vram_peak_mib"], 3_060);
        TEST_CLOCK_US.store(u64::MAX, Ordering::Relaxed);
        std::env::remove_var("BB_BENCHMARK");
        std::env::remove_var("BB_EXE");
    }
}
