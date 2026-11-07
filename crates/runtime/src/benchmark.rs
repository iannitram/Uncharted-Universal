//! Deterministic benchmark configuration and report contracts (#173).
//!
//! This module does not start a game, read a save, or claim that a game run is repeatable.
//! It provides the bounded configuration, frame-statistics accumulator, and report writer
//! for the runtime integration that will drive the game and collect GPU telemetry.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_BENCHMARK_SECONDS: u32 = 86_400;
const STALL_THRESHOLD_MS: f64 = 40.0;
const REPORTED_SETTINGS: &[&str] = &[
    "BB_RESOLUTION",
    "BB_GAME_RES",
    "BB_RENDER_RES",
    "BB_OUTPUT_RES",
    "BB_VBLANK_HZ",
    "BB_FPS_LIMIT",
    "BB_UPSCALER",
    "BB_UPSCALE_QUALITY",
    "BB_FSR_SHARPNESS",
    "BB_FRAME_GEN",
    "BB_PRESENT_MODE",
    "BB_FULLSCREEN",
    "BB_TINY_READBACK",
    "BB_READBACK_SMALL_BUFFERS",
    "BB_TOGGLE",
    "BB_TOGGLE2",
    "BB_SCALAR_COPY64_SYNC",
    "BB_GPU_MERGE",
    "BB_MAX_FRAMES_AHEAD",
    "BB_DMEM_MB",
    "BB_GAME_FREE_MB",
    "BB_GAME_MOVIE_MB",
    "BB_GAME_RT_MB",
    "BB_GAME_RT2_MB",
];

/// A validated benchmark request from `BB_BENCHMARK=<seconds>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub seconds: u32,
    /// Public runtime settings only. Paths and file names are deliberately excluded.
    pub settings: BTreeMap<String, String>,
}

impl Config {
    /// Parse the benchmark value and a snapshot of the process environment.
    ///
    /// `None` means the benchmark mode is disabled. An empty, non-decimal, zero, or
    /// unreasonably large duration is rejected instead of silently selecting a default.
    pub fn from_values(
        benchmark: Option<&str>,
        environment: impl IntoIterator<Item = (String, String)>,
    ) -> io::Result<Option<Self>> {
        let Some(value) = benchmark else {
            return Ok(None);
        };
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid("BB_BENCHMARK must be a whole number of seconds"));
        }
        let seconds = value
            .parse::<u32>()
            .map_err(|_| invalid("BB_BENCHMARK is outside the supported range"))?;
        if seconds == 0 || seconds > MAX_BENCHMARK_SECONDS {
            return Err(invalid("BB_BENCHMARK must be between 1 and 86400 seconds"));
        }

        let settings = environment
            .into_iter()
            .filter(|(key, _)| is_report_setting(key))
            .collect();
        Ok(Some(Self { seconds, settings }))
    }

    /// Read the benchmark setting and safe `BB_*` settings from the process environment.
    pub fn from_environment() -> io::Result<Option<Self>> {
        let benchmark = match std::env::var_os("BB_BENCHMARK") {
            None => return Ok(None),
            Some(value) => Some(
                value
                    .into_string()
                    .map_err(|_| invalid("BB_BENCHMARK is not valid UTF-8"))?,
            ),
        };
        let settings = std::env::vars_os()
            .filter_map(|(key, value)| Some((key.to_str()?.to_owned(), value.to_str()?.to_owned())))
            .collect::<Vec<_>>();
        Self::from_values(benchmark.as_deref(), settings)
    }
}

fn is_report_setting(key: &str) -> bool {
    REPORTED_SETTINGS.contains(&key)
}

/// Which title is being benchmarked. The collection's title ID is shared by all three games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    U1,
    U2,
    U3,
}

impl Game {
    fn as_str(self) -> &'static str {
        match self {
            Self::U1 => "u1",
            Self::U2 => "u2",
            Self::U3 => "u3",
        }
    }
}

/// Final process outcome recorded in the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Ok,
    Crash,
}

impl Ended {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Crash => "crash",
        }
    }
}

/// Frame timing and GPU-memory observations gathered during one benchmark interval.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Samples {
    frame_ms: Vec<f64>,
    vram_peak_mib: u64,
    stalls_over_40ms: u64,
}

impl Samples {
    /// Add one completed frame. Invalid measurements are rejected and never enter the report.
    pub fn record_frame(&mut self, frame_ms: f64, vram_mib: u64) -> io::Result<()> {
        if !frame_ms.is_finite() || frame_ms <= 0.0 {
            return Err(invalid("frame time must be finite and greater than zero"));
        }
        self.frame_ms.push(frame_ms);
        self.vram_peak_mib = self.vram_peak_mib.max(vram_mib);
        if frame_ms > STALL_THRESHOLD_MS {
            self.stalls_over_40ms = self.stalls_over_40ms.saturating_add(1);
        }
        Ok(())
    }

    /// Build the stable JSON shape required by #173. At least one frame is required.
    pub fn finish(
        &self,
        game: Game,
        seconds: u32,
        settings: BTreeMap<String, String>,
        ended: Ended,
    ) -> io::Result<Report> {
        if seconds == 0 || seconds > MAX_BENCHMARK_SECONDS {
            return Err(invalid("benchmark duration is outside the supported range"));
        }
        if self.frame_ms.is_empty() {
            return Err(invalid("benchmark has no frame samples"));
        }
        let mut sorted = self.frame_ms.clone();
        sorted.sort_by(f64::total_cmp);
        let total_ms: f64 = sorted.iter().sum();
        let frame_count = sorted.len() as f64;
        Ok(Report {
            game,
            seconds,
            settings,
            frame_ms: Percentiles {
                p50: nearest_rank(&sorted, 0.50),
                p95: nearest_rank(&sorted, 0.95),
                p99: nearest_rank(&sorted, 0.99),
            },
            avg_fps: frame_count * 1_000.0 / total_ms,
            vram_peak_mib: self.vram_peak_mib,
            stalls_over_40ms: self.stalls_over_40ms,
            ended,
        })
    }
}

fn nearest_rank(sorted: &[f64], percentile: f64) -> f64 {
    let rank = (percentile * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// The report written to `<saves folder>/benchmark.json` after a completed run.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub game: Game,
    pub seconds: u32,
    pub settings: BTreeMap<String, String>,
    pub frame_ms: Percentiles,
    pub avg_fps: f64,
    pub vram_peak_mib: u64,
    pub stalls_over_40ms: u64,
    pub ended: Ended,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Percentiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

impl Report {
    /// Serialize with the field names and enum values specified by issue #173.
    pub fn to_json(&self) -> io::Result<Vec<u8>> {
        serde_json::to_vec(&serde_json::json!({
            "game": self.game.as_str(),
            "seconds": self.seconds,
            "settings": self.settings,
            "frame_ms": {
                "p50": self.frame_ms.p50,
                "p95": self.frame_ms.p95,
                "p99": self.frame_ms.p99,
            },
            "avg_fps": self.avg_fps,
            "vram_peak_mib": self.vram_peak_mib,
            "stalls_over_40ms": self.stalls_over_40ms,
            "ended": self.ended.as_str(),
        }))
        .map_err(io::Error::other)
    }

    /// Publish the finished report at the fixed, non-game-derived file name using a temporary
    /// file so a failed serialization or write does not expose a partial JSON document.
    pub fn write_to(&self, saves_folder: &Path) -> io::Result<()> {
        fs::create_dir_all(saves_folder)?;
        let bytes = self.to_json()?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let (temporary, mut file) = loop {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                saves_folder.join(format!(".benchmark.json.{}.{id}.tmp", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => break (path, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let written = file.write_all(&bytes).and_then(|()| file.sync_all());
        drop(file);
        let target = saves_folder.join("benchmark.json");
        let result = written.and_then(|()| replace_report(&temporary, &target));
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

#[cfg(not(windows))]
fn replace_report(temporary: &Path, target: &Path) -> io::Result<()> {
    fs::rename(temporary, target)
}

#[cfg(windows)]
fn replace_report(temporary: &Path, target: &Path) -> io::Result<()> {
    match fs::rename(temporary, target) {
        Ok(()) => Ok(()),
        Err(_) if target.exists() => {
            // std::fs::rename does not replace an existing destination on Windows.
            fs::remove_file(target)?;
            fs::rename(temporary, target)
        }
        Err(error) => Err(error),
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("uu-benchmark-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn config_accepts_only_a_bounded_positive_integer_and_filters_private_values() {
        let env = vec![
            ("BB_RESOLUTION".into(), "1280x720".into()),
            ("BB_SAVE_DIR".into(), "/private/saves".into()),
            ("PATH".into(), "/private/bin".into()),
            ("BB_BENCHMARK".into(), "90".into()),
        ];
        let config = Config::from_values(Some("90"), env).unwrap().unwrap();
        assert_eq!(config.seconds, 90);
        assert_eq!(config.settings.len(), 1);
        assert_eq!(config.settings["BB_RESOLUTION"], "1280x720");
        for invalid_value in ["", "0", "-1", "1.5", "86401", "4294967296"] {
            assert!(Config::from_values(Some(invalid_value), []).is_err());
        }
        assert!(Config::from_values(None, []).unwrap().is_none());
    }

    #[test]
    fn frame_metrics_use_nearest_rank_and_count_only_stalls_over_40ms() {
        let mut samples = Samples::default();
        for (frame_ms, vram_mib) in [(10.0, 512), (20.0, 1024), (40.0, 2048), (50.0, 1536)] {
            samples.record_frame(frame_ms, vram_mib).unwrap();
        }
        let report = samples
            .finish(Game::U2, 90, BTreeMap::new(), Ended::Ok)
            .unwrap();
        assert_eq!(
            report.frame_ms,
            Percentiles {
                p50: 20.0,
                p95: 50.0,
                p99: 50.0
            }
        );
        assert!((report.avg_fps - 33.3333333333).abs() < 1e-8);
        assert_eq!(report.vram_peak_mib, 2048);
        assert_eq!(report.stalls_over_40ms, 1);
    }

    #[test]
    fn report_has_the_issue_schema_and_replaces_the_previous_run() {
        let mut samples = Samples::default();
        samples.record_frame(16.0, 5120).unwrap();
        let report = samples
            .finish(
                Game::U2,
                90,
                BTreeMap::from([("BB_RESOLUTION".into(), "1280x720".into())]),
                Ended::Crash,
            )
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&report.to_json().unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "game": "u2", "seconds": 90,
                "settings": {"BB_RESOLUTION": "1280x720"},
                "frame_ms": {"p50": 16.0, "p95": 16.0, "p99": 16.0},
                "avg_fps": 62.5, "vram_peak_mib": 5120,
                "stalls_over_40ms": 0, "ended": "crash"
            })
        );
        let dir = TempDir::new();
        report.write_to(&dir.0).unwrap();
        report.write_to(&dir.0).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &fs::read(dir.0.join("benchmark.json")).unwrap()
            )
            .unwrap(),
            json
        );
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn invalid_samples_and_empty_reports_are_rejected() {
        let mut samples = Samples::default();
        for frame_ms in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(samples.record_frame(frame_ms, 0).is_err());
        }
        assert!(samples
            .finish(Game::U1, 10, BTreeMap::new(), Ended::Ok)
            .is_err());
        assert!(samples.record_frame(16.0, 0).is_ok());
        assert!(samples
            .finish(Game::U1, 0, BTreeMap::new(), Ended::Ok)
            .is_err());
    }
}
