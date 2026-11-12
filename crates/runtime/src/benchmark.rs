//! Deterministic benchmark configuration and report contracts (#173).
//!
//! This module does not start a game, read a save, or claim that a game run is repeatable.
//! It provides the bounded configuration, frame-statistics accumulator, and report writer.
//! `benchmark_run` joins them to the pad schedule and the clock, and exports the C functions
//! the loader and the GPU library call.

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
            .filter_map(|(key, value)| {
                normalize_report_setting(&key, &value).map(|value| (key, value))
            })
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

fn normalize_report_setting(key: &str, value: &str) -> Option<String> {
    if !REPORTED_SETTINGS.contains(&key) {
        return None;
    }
    match key {
        "BB_RESOLUTION" | "BB_GAME_RES" | "BB_RENDER_RES" | "BB_OUTPUT_RES" => {
            normalize_resolution(value)
        }
        "BB_VBLANK_HZ" => normalize_unsigned(value, 0, 1_000),
        "BB_FPS_LIMIT" => normalize_unsigned(value, 0, 1_000),
        "BB_UPSCALER" => normalize_enum(
            value,
            &[
                "off", "none", "taa", "fsr3", "fsr4", "fsr411", "dlss", "xess",
            ],
        ),
        "BB_UPSCALE_QUALITY" => normalize_enum(
            value,
            &["off", "quality", "balanced", "performance", "ultra"],
        ),
        "BB_FSR_SHARPNESS" => {
            let sharpness = value.parse::<f32>().ok()?;
            (sharpness.is_finite() && (0.0..=2.0).contains(&sharpness))
                .then(|| sharpness.to_string())
        }
        "BB_FRAME_GEN" => normalize_enum(value, &["off", "fsr3", "dlss"]),
        "BB_PRESENT_MODE" => {
            normalize_enum(value, &["Mailbox", "Fifo", "FifoRelaxed", "Immediate"])
        }
        "BB_FULLSCREEN" | "BB_TINY_READBACK" | "BB_GPU_MERGE" => normalize_unsigned(value, 0, 1),
        "BB_READBACK_SMALL_BUFFERS" => normalize_enum(value, &["0", "1", "sync"]),
        "BB_TOGGLE" | "BB_TOGGLE2" => normalize_mask(value),
        "BB_SCALAR_COPY64_SYNC" => normalize_unsigned(value, 0, 2),
        "BB_MAX_FRAMES_AHEAD" => normalize_unsigned(value, 0, u64::MAX),
        "BB_DMEM_MB" | "BB_GAME_FREE_MB" | "BB_GAME_MOVIE_MB" | "BB_GAME_RT_MB"
        | "BB_GAME_RT2_MB" => normalize_unsigned(value, 0, 65_536),
        _ => None,
    }
}

fn normalize_enum(value: &str, choices: &[&str]) -> Option<String> {
    choices
        .iter()
        .find(|choice| value.eq_ignore_ascii_case(choice))
        .map(|choice| (*choice).to_owned())
}

fn normalize_unsigned(value: &str, minimum: u64, maximum: u64) -> Option<String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = value.parse::<u64>().ok()?;
    (minimum..=maximum)
        .contains(&number)
        .then(|| number.to_string())
}

fn normalize_mask(value: &str) -> Option<String> {
    let number = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        if hex.is_empty() || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        u64::from_str_radix(hex, 16).ok()?
    } else {
        value.parse::<u64>().ok()?
    };
    Some(number.to_string())
}

fn normalize_resolution(value: &str) -> Option<String> {
    let (width, height) = value
        .split_once('x')
        .or_else(|| value.split_once('X'))
        .or_else(|| value.split_once('×'))?;
    let width = parse_dimension(width)?;
    let height = parse_dimension(height)?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 67_108_864 {
        return None;
    }
    Some(format!("{width}x{height}"))
}

fn parse_dimension(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// Which title is being benchmarked. The collection's title ID is shared by all three games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    U1,
    U2,
    U3,
}

impl Game {
    /// The game for `BB_EXE` as `run.py` sets it: `u1`, `u2`, `u3`, or the executable's file name
    /// (`eboot.bin`, `big2-ps4_Shipping.elf`, `big3-ps4_Shipping.elf`), with or without a folder.
    pub fn from_executable(name: &str) -> Option<Self> {
        let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
        match file.to_ascii_lowercase().as_str() {
            "u1" | "eboot.bin" => Some(Self::U1),
            "u2" | "big2-ps4_shipping.elf" => Some(Self::U2),
            "u3" | "big3-ps4_shipping.elf" => Some(Self::U3),
            _ => None,
        }
    }

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
    pub fn is_empty(&self) -> bool {
        self.frame_ms.is_empty()
    }

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
        // Keep the privacy boundary here as well as in Config::from_values: callers may
        // construct a settings map directly instead of passing through the environment parser.
        let settings = settings
            .into_iter()
            .filter_map(|(key, value)| {
                normalize_report_setting(&key, &value).map(|value| (key, value))
            })
            .collect();
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
        // Report fields are public for consumers, so enforce the allowlist again at the
        // serialization boundary in case a Report was constructed directly.
        let settings: BTreeMap<_, _> = self
            .settings
            .iter()
            .filter_map(|(key, value)| {
                normalize_report_setting(key, value).map(|value| (key.clone(), value))
            })
            .collect();
        serde_json::to_vec(&serde_json::json!({
            "game": self.game.as_str(),
            "seconds": self.seconds,
            "settings": settings,
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
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain([0]).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain([0]).collect();
    // Both files live in the saves directory, so this is a same-volume replacement. If Windows
    // cannot perform it, MoveFileExW leaves the previous report in place.
    let replaced = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            loop {
                let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
                let path =
                    std::env::temp_dir().join(format!("uu-benchmark-{}-{id}", std::process::id()));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create test directory: {error}"),
                }
            }
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
    fn settings_values_are_validated_and_normalized_before_reporting() {
        let environment = vec![
            ("BB_RESOLUTION".into(), "02560×01440".into()),
            ("BB_PRESENT_MODE".into(), "fifo".into()),
            ("BB_FULLSCREEN".into(), "01".into()),
            ("BB_FSR_SHARPNESS".into(), "0.40".into()),
            ("BB_GAME_RES".into(), "/home/user/saves".into()),
            ("BB_TOGGLE2".into(), "0x20".into()),
            ("PATH".into(), "/private/bin".into()),
        ];
        let config = Config::from_values(Some("90"), environment)
            .unwrap()
            .unwrap();
        assert_eq!(
            config.settings,
            BTreeMap::from([
                ("BB_RESOLUTION".into(), "2560x1440".into()),
                ("BB_PRESENT_MODE".into(), "Fifo".into()),
                ("BB_FULLSCREEN".into(), "1".into()),
                ("BB_FSR_SHARPNESS".into(), "0.4".into()),
                ("BB_TOGGLE2".into(), "32".into()),
            ])
        );

        for (key, path) in [
            ("BB_RESOLUTION", "/home/user/1920x1080"),
            ("BB_GAME_RES", "C:\\Users\\Alice\\saves"),
            ("BB_RENDER_RES", "1920x1080/../../save"),
            ("BB_OUTPUT_RES", "1920x1080\\private"),
            ("BB_VBLANK_HZ", "/home/user/60"),
            ("BB_FPS_LIMIT", "30/../../secret"),
            ("BB_UPSCALER", "/private/dlss"),
            ("BB_UPSCALE_QUALITY", "quality/secret"),
            ("BB_FSR_SHARPNESS", "/home/user/0.4"),
            ("BB_FRAME_GEN", "/private"),
            ("BB_PRESENT_MODE", "Fifo/secret"),
            ("BB_FULLSCREEN", "/private"),
            ("BB_TINY_READBACK", "1/secret"),
            ("BB_READBACK_SMALL_BUFFERS", "/home/user"),
            ("BB_TOGGLE", "/home/user/0"),
            ("BB_TOGGLE2", "0x../../secret"),
            ("BB_SCALAR_COPY64_SYNC", "/private"),
            ("BB_GPU_MERGE", "2"),
            ("BB_MAX_FRAMES_AHEAD", "999999999999999999999999999"),
            ("BB_DMEM_MB", "/private"),
            ("BB_GAME_FREE_MB", "/private"),
            ("BB_GAME_MOVIE_MB", "/private"),
            ("BB_GAME_RT_MB", "/private"),
            ("BB_GAME_RT2_MB", "/private"),
        ] {
            assert_eq!(normalize_report_setting(key, path), None, "{key}={path}");
        }
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
    fn failed_report_replacement_preserves_the_previous_report() {
        let dir = TempDir::new();
        let target = dir.0.join("benchmark.json");
        fs::write(&target, b"previous report").unwrap();
        let missing_temporary = dir.0.join("missing.tmp");

        assert!(replace_report(&missing_temporary, &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"previous report");
    }

    #[test]
    fn report_boundary_filters_settings_that_bypass_config() {
        let mut samples = Samples::default();
        samples.record_frame(16.0, 512).unwrap();
        let report = samples
            .finish(
                Game::U1,
                10,
                BTreeMap::from([
                    ("BB_RESOLUTION".into(), "1280x720".into()),
                    ("BB_RENDER_RES".into(), "/private/saves".into()),
                    ("BB_SAVE_DIR".into(), "/private/saves".into()),
                    ("PATH".into(), "/private/bin".into()),
                ]),
                Ended::Ok,
            )
            .unwrap();
        assert_eq!(
            report.settings,
            BTreeMap::from([("BB_RESOLUTION".into(), "1280x720".into()),])
        );

        let json: serde_json::Value = serde_json::from_slice(&report.to_json().unwrap()).unwrap();
        assert_eq!(
            json["settings"],
            serde_json::json!({
                "BB_RESOLUTION": "1280x720",
            })
        );

        let directly_constructed = Report {
            settings: BTreeMap::from([
                ("BB_RESOLUTION".into(), "/private/saves".into()),
                ("BB_SAVE_DIR".into(), "/private/saves".into()),
            ]),
            ..report
        };
        let json: serde_json::Value =
            serde_json::from_slice(&directly_constructed.to_json().unwrap()).unwrap();
        assert_eq!(json["settings"], serde_json::json!({}));
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
