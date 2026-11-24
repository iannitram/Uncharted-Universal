//! Scripted and agent-driven playthrough runner and per-build health reports (#15).
//!
//! Pure-Rust route definition (`Route`, `Section`, `ScriptStep`), recovery-capable step runner
//! (`run_route` over [`PlaytestHost`]), frame/freeze/crash/visual-checkpoint checks, and
//! structured per-build reports (`BuildReport`) in JSON and Markdown.

use crate::error::{Error, Result};
use crate::games::Game;
use crate::logs::{self, Ended, PlaythroughRun, Where};
use serde_json::{json, Value};

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_STALL_THRESHOLD_MS: f64 = 40.0;
const DEFAULT_MAX_STALLS: usize = 5;
const DEFAULT_MAX_FROZEN_FRAMES: usize = 3;
const DEFAULT_MAX_MEAN_DIFF: f64 = 12.0;
const DEFAULT_MIN_LUMA: f64 = 2.0;
const MAX_AGENT_STEPS: usize = 16;
const MAX_LOG_EXCERPTS: usize = 10;
const FREEZE_DIFF_EPSILON: f64 = 0.05;

/// Known `BB_PAD_FILE` button/token names (`crates/runtime/src/pad/read.rs`).
const PAD_BUTTON_TOKENS: &[&str] = &[
    "cross",
    "circle",
    "square",
    "triangle",
    "l1",
    "r1",
    "l2",
    "r2",
    "l3",
    "r3",
    "options",
    "touchpad",
    "touchpad_left",
    "touchpad_right",
    "up",
    "down",
    "left",
    "right",
    "replay",
];

const PAD_AXIS_PREFIXES: &[&str] = &["lx=", "ly=", "rx=", "ry="];
const PAD_MOUSE_PREFIXES: &[&str] = &["mx=", "my="];

/// Validate that every whitespace-separated token in `tokens` is a supported `BB_PAD_FILE` token.
pub fn validate_pad_tokens(tokens: &str) -> Result<()> {
    for token in tokens.split_whitespace() {
        if PAD_BUTTON_TOKENS.contains(&token) {
            continue;
        }
        if let Some(val) = PAD_AXIS_PREFIXES.iter().find_map(|p| token.strip_prefix(p)) {
            let parsed: i32 = val
                .parse()
                .map_err(|_| Error::Launch(format!("invalid pad axis value in token '{token}'")))?;
            if !(0..=255).contains(&parsed) {
                return Err(Error::Launch(format!(
                    "pad axis token '{token}' must be in 0..=255"
                )));
            }
            continue;
        }
        if let Some(val) = PAD_MOUSE_PREFIXES
            .iter()
            .find_map(|p| token.strip_prefix(p))
        {
            let parsed: f32 = val.parse().map_err(|_| {
                Error::Launch(format!("invalid pad mouse value in token '{token}'"))
            })?;
            if !parsed.is_finite() {
                return Err(Error::Launch(format!(
                    "pad mouse token '{token}' must be finite"
                )));
            }
            continue;
        }
        return Err(Error::Launch(format!("unknown pad token '{token}'")));
    }
    Ok(())
}

/// One timed scripted input step written to `BB_PAD_FILE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptStep {
    pub tokens: String,
    pub hold_ms: u64,
    pub note: Option<String>,
}

impl ScriptStep {
    pub fn new(tokens: impl Into<String>, hold_ms: u64) -> Self {
        Self {
            tokens: tokens.into(),
            hold_ms,
            note: None,
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// Expected visual checkpoint at the end of a section.
#[derive(Debug, Clone, PartialEq)]
pub struct Checkpoint {
    pub image_path: Option<String>,
    pub max_mean_diff: f64,
    pub min_luma: f64,
}

impl Default for Checkpoint {
    fn default() -> Self {
        Self {
            image_path: None,
            max_mean_diff: DEFAULT_MAX_MEAN_DIFF,
            min_luma: DEFAULT_MIN_LUMA,
        }
    }
}

/// Performance and health thresholds for a single section.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionBudget {
    pub timeout_ms: u64,
    pub min_avg_fps: Option<f64>,
    pub max_frame_ms: Option<f64>,
    pub stall_threshold_ms: f64,
    pub max_stalls: usize,
    pub max_frozen_frames: usize,
    pub require_audio: bool,
    pub expected_trophies: Vec<String>,
}

impl Default for SectionBudget {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            min_avg_fps: None,
            max_frame_ms: None,
            stall_threshold_ms: DEFAULT_STALL_THRESHOLD_MS,
            max_stalls: DEFAULT_MAX_STALLS,
            max_frozen_frames: DEFAULT_MAX_FROZEN_FRAMES,
            require_audio: false,
            expected_trophies: Vec::new(),
        }
    }
}

/// Recovery actions when a section fails or desyncs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryPolicy {
    pub max_retries: u32,
    pub nudge_tokens: Vec<String>,
    pub reload_save: bool,
    pub agent_takeover: bool,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 1,
            nudge_tokens: Vec::new(),
            reload_save: true,
            agent_takeover: false,
        }
    }
}

/// One playable section/sequence in a route.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub save: Option<String>,
    pub steps: Vec<ScriptStep>,
    pub replay: Option<String>,
    pub checkpoint: Option<Checkpoint>,
    pub budget: SectionBudget,
    pub recovery: RecoveryPolicy,
}

impl Section {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            save: None,
            steps: Vec::new(),
            replay: None,
            checkpoint: None,
            budget: SectionBudget::default(),
            recovery: RecoveryPolicy::default(),
        }
    }
}

/// A full test route for one game.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub game: Game,
    pub name: String,
    pub sections: Vec<Section>,
}

impl Route {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::Launch("route name must not be empty".into()));
        }
        if self.sections.is_empty() {
            return Err(Error::Launch(
                "route must contain at least one section".into(),
            ));
        }
        for section in &self.sections {
            if section.id.trim().is_empty() {
                return Err(Error::Launch("section id must not be empty".into()));
            }
            for step in &section.steps {
                validate_pad_tokens(&step.tokens)?;
            }
            for nudge in &section.recovery.nudge_tokens {
                validate_pad_tokens(nudge)?;
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> String {
        let sections: Vec<Value> = self
            .sections
            .iter()
            .map(|s| {
                let steps: Vec<Value> = s
                    .steps
                    .iter()
                    .map(|st| {
                        json!({
                            "tokens": st.tokens,
                            "hold_ms": st.hold_ms,
                            "note": st.note,
                        })
                    })
                    .collect();
                let checkpoint = s.checkpoint.as_ref().map(|c| {
                    json!({
                        "image_path": c.image_path,
                        "max_mean_diff": c.max_mean_diff,
                        "min_luma": c.min_luma,
                    })
                });
                json!({
                    "id": s.id,
                    "title": s.title,
                    "save": s.save,
                    "steps": steps,
                    "replay": s.replay,
                    "checkpoint": checkpoint,
                    "budget": {
                        "timeout_ms": s.budget.timeout_ms,
                        "min_avg_fps": s.budget.min_avg_fps,
                        "max_frame_ms": s.budget.max_frame_ms,
                        "stall_threshold_ms": s.budget.stall_threshold_ms,
                        "max_stalls": s.budget.max_stalls,
                        "max_frozen_frames": s.budget.max_frozen_frames,
                        "require_audio": s.budget.require_audio,
                        "expected_trophies": s.budget.expected_trophies,
                    },
                    "recovery": {
                        "max_retries": s.recovery.max_retries,
                        "nudge_tokens": s.recovery.nudge_tokens,
                        "reload_save": s.recovery.reload_save,
                        "agent_takeover": s.recovery.agent_takeover,
                    }
                })
            })
            .collect();
        serde_json::to_string_pretty(&json!({
            "game": self.game.key(),
            "name": self.name,
            "sections": sections,
        }))
        .unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let invalid = |msg: &str| Error::Launch(format!("invalid playtest route: {msg}"));
        let v: Value = serde_json::from_str(text).map_err(|e| invalid(&e.to_string()))?;
        let game_key = v["game"]
            .as_str()
            .ok_or_else(|| invalid("missing 'game'"))?;
        let game = Game::from_key(game_key).ok_or_else(|| invalid("unknown 'game'"))?;
        let name = v["name"]
            .as_str()
            .ok_or_else(|| invalid("missing 'name'"))?
            .to_owned();
        let arr = v["sections"]
            .as_array()
            .ok_or_else(|| invalid("missing 'sections' array"))?;
        let mut sections = Vec::with_capacity(arr.len());
        for item in arr {
            let id = item["id"]
                .as_str()
                .ok_or_else(|| invalid("section missing 'id'"))?
                .to_owned();
            let title = item["title"].as_str().unwrap_or(&id).to_owned();
            let save = item["save"].as_str().map(str::to_owned);
            let replay = item["replay"].as_str().map(str::to_owned);
            let mut steps = Vec::new();
            if let Some(step_arr) = item["steps"].as_array() {
                for st in step_arr {
                    let tokens = st["tokens"]
                        .as_str()
                        .ok_or_else(|| invalid("step missing 'tokens'"))?
                        .to_owned();
                    let hold_ms = st["hold_ms"].as_u64().unwrap_or(0);
                    let note = st["note"].as_str().map(str::to_owned);
                    steps.push(ScriptStep {
                        tokens,
                        hold_ms,
                        note,
                    });
                }
            }
            let checkpoint = item.get("checkpoint").and_then(|c| {
                if c.is_null() {
                    return None;
                }
                Some(Checkpoint {
                    image_path: c["image_path"].as_str().map(str::to_owned),
                    max_mean_diff: c["max_mean_diff"].as_f64().unwrap_or(DEFAULT_MAX_MEAN_DIFF),
                    min_luma: c["min_luma"].as_f64().unwrap_or(DEFAULT_MIN_LUMA),
                })
            });
            let budget = if let Some(b) = item.get("budget").and_then(Value::as_object) {
                SectionBudget {
                    timeout_ms: b
                        .get("timeout_ms")
                        .and_then(Value::as_u64)
                        .unwrap_or(DEFAULT_TIMEOUT_MS),
                    min_avg_fps: b.get("min_avg_fps").and_then(Value::as_f64),
                    max_frame_ms: b.get("max_frame_ms").and_then(Value::as_f64),
                    stall_threshold_ms: b
                        .get("stall_threshold_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(DEFAULT_STALL_THRESHOLD_MS),
                    max_stalls: b
                        .get("max_stalls")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize)
                        .unwrap_or(DEFAULT_MAX_STALLS),
                    max_frozen_frames: b
                        .get("max_frozen_frames")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize)
                        .unwrap_or(DEFAULT_MAX_FROZEN_FRAMES),
                    require_audio: b
                        .get("require_audio")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    expected_trophies: b
                        .get("expected_trophies")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                }
            } else {
                SectionBudget::default()
            };
            let recovery = if let Some(r) = item.get("recovery").and_then(Value::as_object) {
                RecoveryPolicy {
                    max_retries: r
                        .get("max_retries")
                        .and_then(Value::as_u64)
                        .map(|n| n as u32)
                        .unwrap_or(1),
                    nudge_tokens: r
                        .get("nudge_tokens")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                    reload_save: r
                        .get("reload_save")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    agent_takeover: r
                        .get("agent_takeover")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                }
            } else {
                RecoveryPolicy::default()
            };
            sections.push(Section {
                id,
                title,
                save,
                steps,
                replay,
                checkpoint,
                budget,
                recovery,
            });
        }
        let route = Self {
            game,
            name,
            sections,
        };
        route.validate()?;
        Ok(route)
    }
}

/// Compact RGB8 frame buffer for synthetic tests and PNG checkpoint comparisons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbFrame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl RgbFrame {
    pub fn solid(width: u32, height: u32, r: u8, g: u8, b: u8) -> Self {
        let len = (width as usize) * (height as usize) * 3;
        let mut rgb = Vec::with_capacity(len);
        for _ in 0..(width as usize * height as usize) {
            rgb.push(r);
            rgb.push(g);
            rgb.push(b);
        }
        Self { width, height, rgb }
    }

    pub fn from_png_bytes(bytes: &[u8]) -> Result<Self> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| Error::Launch(format!("failed to decode checkpoint image: {e}")))?;
        let rgb8 = img.to_rgb8();
        let (width, height) = rgb8.dimensions();
        Ok(Self {
            width,
            height,
            rgb: rgb8.into_raw(),
        })
    }

    pub fn mean_luma(&self) -> f64 {
        if self.rgb.is_empty() {
            return 0.0;
        }
        let mut sum = 0.0;
        let pixels = self.rgb.len() / 3;
        for chunk in self.rgb.as_chunks::<3>().0 {
            sum += 0.2126 * f64::from(chunk[0])
                + 0.7152 * f64::from(chunk[1])
                + 0.0722 * f64::from(chunk[2]);
        }
        sum / (pixels.max(1) as f64)
    }

    pub fn mean_abs_diff(&self, other: &Self) -> Option<f64> {
        if self.width != other.width || self.height != other.height {
            return None;
        }
        if self.rgb.is_empty() {
            return Some(0.0);
        }
        let sum: u64 = self
            .rgb
            .iter()
            .zip(other.rgb.iter())
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum();
        Some((sum as f64) / (self.rgb.len() as f64))
    }
}

/// Telemetry returned by [`PlaytestHost::apply_input`] for one step.
#[derive(Debug, Clone, Default)]
pub struct StepOutcome {
    pub elapsed_ms: u64,
    pub frame_times_ms: Vec<f64>,
    pub log_lines: Vec<String>,
    pub process_alive: bool,
    pub exit_code: Option<i32>,
    pub screenshot: Option<RgbFrame>,
    pub screenshot_path: Option<String>,
    pub audio_started: bool,
    pub trophies_unlocked: Vec<String>,
}

/// Decision returned by a vision/AI agent during closed-loop takeover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentAction {
    Continue { tokens: String, hold_ms: u64 },
    SectionComplete,
    Abort { reason: String },
}

/// Host/emulator interface driven by [`run_route`].
pub trait PlaytestHost {
    fn load_save(&mut self, section: &Section, save: Option<&str>) -> Result<()>;
    fn apply_input(&mut self, tokens: &str, hold_ms: u64) -> StepOutcome;
    fn load_checkpoint_image(&mut self, path: &str) -> Result<RgbFrame>;

    fn agent_decide(
        &mut self,
        _section: &Section,
        _last: &StepOutcome,
        _attempt: u32,
    ) -> Option<AgentAction> {
        None
    }
}

/// Classification of a section health or correctness failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Crash,
    Hang,
    FrameFreeze,
    LowFps,
    FrameStall,
    VisualBlack,
    CheckpointMismatch,
    AudioMissing,
    TrophyMissing,
    Aborted,
}

impl FailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Crash => "crash",
            Self::Hang => "hang",
            Self::FrameFreeze => "frame_freeze",
            Self::LowFps => "low_fps",
            Self::FrameStall => "frame_stall",
            Self::VisualBlack => "visual_black",
            Self::CheckpointMismatch => "checkpoint_mismatch",
            Self::AudioMissing => "audio_missing",
            Self::TrophyMissing => "trophy_missing",
            Self::Aborted => "aborted",
        }
    }

    pub fn from_str_key(key: &str) -> Option<Self> {
        match key {
            "crash" => Some(Self::Crash),
            "hang" => Some(Self::Hang),
            "frame_freeze" => Some(Self::FrameFreeze),
            "low_fps" => Some(Self::LowFps),
            "frame_stall" => Some(Self::FrameStall),
            "visual_black" => Some(Self::VisualBlack),
            "checkpoint_mismatch" => Some(Self::CheckpointMismatch),
            "audio_missing" => Some(Self::AudioMissing),
            "trophy_missing" => Some(Self::TrophyMissing),
            "aborted" => Some(Self::Aborted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionFailure {
    pub kind: FailureKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionStatus {
    Pass,
    Fail,
}

impl SectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SectionResult {
    pub id: String,
    pub title: String,
    pub status: SectionStatus,
    pub attempts: u32,
    pub recoveries: Vec<String>,
    pub duration_ms: u64,
    pub frames: usize,
    pub avg_fps: Option<f64>,
    pub min_fps: Option<f64>,
    pub max_frame_ms: Option<f64>,
    pub stall_count: usize,
    pub frozen_frames: usize,
    pub failures: Vec<SectionFailure>,
    pub log_excerpts: Vec<String>,
    pub screenshot_paths: Vec<String>,
    pub trophies_unlocked: Vec<String>,
    pub checkpoint_diff: Option<f64>,
}

/// Structured per-build playthrough health report.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildReport {
    pub build: String,
    pub game: Game,
    pub route_name: String,
    pub at_unix: u64,
    pub sections: Vec<SectionResult>,
}

impl BuildReport {
    pub fn passed(&self) -> usize {
        self.sections
            .iter()
            .filter(|s| s.status == SectionStatus::Pass)
            .count()
    }

    pub fn failed(&self) -> usize {
        self.sections
            .iter()
            .filter(|s| s.status == SectionStatus::Fail)
            .count()
    }

    pub fn overall_pass(&self) -> bool {
        self.sections
            .iter()
            .all(|s| s.status == SectionStatus::Pass)
    }

    pub fn broken_sections(&self) -> Vec<&SectionResult> {
        self.sections
            .iter()
            .filter(|s| s.status == SectionStatus::Fail)
            .collect()
    }

    pub fn overall_avg_fps(&self) -> Option<f64> {
        let mut total_fps = 0.0;
        let mut count = 0usize;
        for s in &self.sections {
            if let Some(fps) = s.avg_fps {
                total_fps += fps;
                count += 1;
            }
        }
        (count > 0).then(|| ((total_fps / count as f64) * 10.0).round() / 10.0)
    }

    pub fn to_playthrough_run(&self, preset: &str) -> PlaythroughRun {
        let ended = if self
            .sections
            .iter()
            .any(|s| s.failures.iter().any(|f| f.kind == FailureKind::Crash))
        {
            Ended::Crash
        } else {
            Ended::Ok
        };
        let last_section = self
            .broken_sections()
            .first()
            .copied()
            .or_else(|| self.sections.last());
        let where_ = last_section.map(|s| Where {
            title: self.route_name.clone(),
            subtitle: s.title.clone(),
        });
        logs::record(
            &self.build,
            self.game,
            where_.as_ref(),
            preset,
            self.overall_avg_fps(),
            ended,
            self.at_unix,
        )
    }

    pub fn to_json(&self) -> String {
        let sections: Vec<Value> = self
            .sections
            .iter()
            .map(|s| {
                let failures: Vec<Value> = s
                    .failures
                    .iter()
                    .map(|f| {
                        json!({
                            "kind": f.kind.as_str(),
                            "message": f.message,
                        })
                    })
                    .collect();
                json!({
                    "id": s.id,
                    "title": s.title,
                    "status": s.status.as_str(),
                    "attempts": s.attempts,
                    "recoveries": s.recoveries,
                    "duration_ms": s.duration_ms,
                    "frames": s.frames,
                    "avg_fps": s.avg_fps,
                    "min_fps": s.min_fps,
                    "max_frame_ms": s.max_frame_ms,
                    "stall_count": s.stall_count,
                    "frozen_frames": s.frozen_frames,
                    "failures": failures,
                    "log_excerpts": s.log_excerpts,
                    "screenshot_paths": s.screenshot_paths,
                    "trophies_unlocked": s.trophies_unlocked,
                    "checkpoint_diff": s.checkpoint_diff,
                })
            })
            .collect();
        serde_json::to_string_pretty(&json!({
            "build": self.build,
            "game": self.game.key(),
            "route_name": self.route_name,
            "at_unix": self.at_unix,
            "overall_pass": self.overall_pass(),
            "passed": self.passed(),
            "failed": self.failed(),
            "sections": sections,
        }))
        .unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let invalid = |msg: &str| Error::Launch(format!("invalid build report: {msg}"));
        let v: Value = serde_json::from_str(text).map_err(|e| invalid(&e.to_string()))?;
        let build = v["build"]
            .as_str()
            .ok_or_else(|| invalid("missing 'build'"))?
            .to_owned();
        let game_key = v["game"]
            .as_str()
            .ok_or_else(|| invalid("missing 'game'"))?;
        let game = Game::from_key(game_key).ok_or_else(|| invalid("unknown 'game'"))?;
        let route_name = v["route_name"]
            .as_str()
            .ok_or_else(|| invalid("missing 'route_name'"))?
            .to_owned();
        let at_unix = v["at_unix"].as_u64().unwrap_or(0);
        let arr = v["sections"]
            .as_array()
            .ok_or_else(|| invalid("missing 'sections'"))?;
        let mut sections = Vec::with_capacity(arr.len());
        for s in arr {
            let status = match s["status"].as_str() {
                Some("pass") => SectionStatus::Pass,
                Some("fail") => SectionStatus::Fail,
                _ => return Err(invalid("invalid section 'status'")),
            };
            let mut failures = Vec::new();
            if let Some(farr) = s["failures"].as_array() {
                for f in farr {
                    let kind = f["kind"]
                        .as_str()
                        .and_then(FailureKind::from_str_key)
                        .ok_or_else(|| invalid("invalid failure 'kind'"))?;
                    let message = f["message"].as_str().unwrap_or("").to_owned();
                    failures.push(SectionFailure { kind, message });
                }
            }
            let str_vec = |key: &str| -> Vec<String> {
                s[key]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            sections.push(SectionResult {
                id: s["id"].as_str().unwrap_or("").to_owned(),
                title: s["title"].as_str().unwrap_or("").to_owned(),
                status,
                attempts: s["attempts"].as_u64().unwrap_or(1) as u32,
                recoveries: str_vec("recoveries"),
                duration_ms: s["duration_ms"].as_u64().unwrap_or(0),
                frames: s["frames"].as_u64().unwrap_or(0) as usize,
                avg_fps: s["avg_fps"].as_f64(),
                min_fps: s["min_fps"].as_f64(),
                max_frame_ms: s["max_frame_ms"].as_f64(),
                stall_count: s["stall_count"].as_u64().unwrap_or(0) as usize,
                frozen_frames: s["frozen_frames"].as_u64().unwrap_or(0) as usize,
                failures,
                log_excerpts: str_vec("log_excerpts"),
                screenshot_paths: str_vec("screenshot_paths"),
                trophies_unlocked: str_vec("trophies_unlocked"),
                checkpoint_diff: s["checkpoint_diff"].as_f64(),
            });
        }
        Ok(Self {
            build,
            game,
            route_name,
            at_unix,
            sections,
        })
    }

    pub fn to_markdown(&self) -> String {
        let verdict = if self.overall_pass() { "PASS" } else { "FAIL" };
        let mut out = format!(
            "# Playtest Report: {} ({}) — {}\n\n\
             - **Build**: `{}`\n\
             - **Route**: `{}`\n\
             - **Sections**: {}/{} passed\n\n\
             | Section | Status | Attempts | Avg FPS | Max Frame (ms) | Stalls | Failures |\n\
             | --- | --- | ---: | ---: | ---: | ---: | --- |\n",
            self.game.name(),
            self.game.key(),
            verdict,
            self.build,
            self.route_name,
            self.passed(),
            self.sections.len(),
        );
        for s in &self.sections {
            let avg = s
                .avg_fps
                .map(|v| format!("{v:.1}"))
                .unwrap_or_else(|| "-".into());
            let max_ms = s
                .max_frame_ms
                .map(|v| format!("{v:.1}"))
                .unwrap_or_else(|| "-".into());
            let fail_summary = if s.failures.is_empty() {
                "-".into()
            } else {
                s.failures
                    .iter()
                    .map(|f| f.kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            out.push_str(&format!(
                "| {} (`{}`) | {} | {} | {} | {} | {} | {} |\n",
                s.title,
                s.id,
                s.status.as_str().to_uppercase(),
                s.attempts,
                avg,
                max_ms,
                s.stall_count,
                fail_summary
            ));
        }
        let broken = self.broken_sections();
        if !broken.is_empty() {
            out.push_str("\n## Broken Sections\n");
            for s in broken {
                out.push_str(&format!("\n### {} (`{}`)\n", s.title, s.id));
                for f in &s.failures {
                    out.push_str(&format!("- **{}**: {}\n", f.kind.as_str(), f.message));
                }
                if !s.recoveries.is_empty() {
                    out.push_str(&format!("- **Recoveries**: {}\n", s.recoveries.join("; ")));
                }
                if !s.log_excerpts.is_empty() {
                    out.push_str("- **Log excerpts**:\n");
                    for line in &s.log_excerpts {
                        out.push_str(&format!("  - `{line}`\n"));
                    }
                }
                if !s.screenshot_paths.is_empty() {
                    out.push_str(&format!(
                        "- **Screenshots**: {}\n",
                        s.screenshot_paths.join(", ")
                    ));
                }
            }
        }
        out
    }
}

/// Run an entire [`Route`] against `host` and produce a [`BuildReport`].
pub fn run_route(
    host: &mut dyn PlaytestHost,
    route: &Route,
    build: &str,
    at_unix: u64,
) -> BuildReport {
    let mut sections = Vec::with_capacity(route.sections.len());
    for section in &route.sections {
        sections.push(run_section(host, section));
    }
    BuildReport {
        build: build.to_owned(),
        game: route.game,
        route_name: route.name.clone(),
        at_unix,
        sections,
    }
}

fn run_section(host: &mut dyn PlaytestHost, section: &Section) -> SectionResult {
    let max_attempts = section.recovery.max_retries.saturating_add(1);
    let mut recoveries = Vec::new();
    let mut final_result = None;

    for attempt in 1..=max_attempts {
        if attempt == 1 || section.recovery.reload_save {
            if let Err(err) = host.load_save(section, section.save.as_deref()) {
                return SectionResult {
                    id: section.id.clone(),
                    title: section.title.clone(),
                    status: SectionStatus::Fail,
                    attempts: attempt,
                    recoveries,
                    duration_ms: 0,
                    frames: 0,
                    avg_fps: None,
                    min_fps: None,
                    max_frame_ms: None,
                    stall_count: 0,
                    frozen_frames: 0,
                    failures: vec![SectionFailure {
                        kind: FailureKind::Aborted,
                        message: format!("failed to load save: {err}"),
                    }],
                    log_excerpts: Vec::new(),
                    screenshot_paths: Vec::new(),
                    trophies_unlocked: Vec::new(),
                    checkpoint_diff: None,
                };
            }
        }

        let mut res = execute_attempt(host, section, attempt);
        res.attempts = attempt;
        res.recoveries = recoveries.clone();

        let can_retry =
            attempt < max_attempts && !res.failures.iter().any(|f| f.kind == FailureKind::Aborted);
        if res.status == SectionStatus::Pass || !can_retry {
            final_result = Some(res);
            break;
        }

        let reasons: Vec<&str> = res.failures.iter().map(|f| f.kind.as_str()).collect();
        recoveries.push(format!(
            "attempt {attempt} failed ({}); retrying{}",
            reasons.join(", "),
            if section.recovery.reload_save {
                " with save reload"
            } else {
                ""
            }
        ));
        final_result = Some(res);
    }

    final_result.expect("at least one attempt always runs")
}

fn execute_attempt(host: &mut dyn PlaytestHost, section: &Section, attempt: u32) -> SectionResult {
    let mut total_ms = 0u64;
    let mut frame_times = Vec::new();
    let mut log_excerpts = Vec::new();
    let mut screenshot_paths = Vec::new();
    let mut trophies_unlocked = Vec::new();
    let mut audio_started = false;
    let mut frozen_frames = 0usize;
    let mut prev_shot: Option<RgbFrame> = None;
    let mut last_outcome = StepOutcome {
        process_alive: true,
        ..StepOutcome::default()
    };
    let mut failures = Vec::new();

    for step in &section.steps {
        let outcome = host.apply_input(&step.tokens, step.hold_ms);
        record_step_telemetry(
            &outcome,
            &mut total_ms,
            &mut frame_times,
            &mut log_excerpts,
            &mut screenshot_paths,
            &mut trophies_unlocked,
            &mut audio_started,
            &mut frozen_frames,
            &mut prev_shot,
        );
        last_outcome = outcome;

        if check_fatal_step(
            &last_outcome,
            total_ms,
            section.budget.timeout_ms,
            &mut failures,
        ) {
            break;
        }
    }

    // If no fatal error yet and frozen/stalled mid-section, try configured nudges.
    if failures.is_empty()
        && frozen_frames > section.budget.max_frozen_frames
        && !section.recovery.nudge_tokens.is_empty()
    {
        for nudge in &section.recovery.nudge_tokens {
            let outcome = host.apply_input(nudge, 500);
            record_step_telemetry(
                &outcome,
                &mut total_ms,
                &mut frame_times,
                &mut log_excerpts,
                &mut screenshot_paths,
                &mut trophies_unlocked,
                &mut audio_started,
                &mut frozen_frames,
                &mut prev_shot,
            );
            last_outcome = outcome;
            if check_fatal_step(
                &last_outcome,
                total_ms,
                section.budget.timeout_ms,
                &mut failures,
            ) {
                break;
            }
        }
    }

    // Optional closed-loop vision/agent takeover.
    if failures.is_empty() && section.recovery.agent_takeover {
        for _ in 0..MAX_AGENT_STEPS {
            let Some(action) = host.agent_decide(section, &last_outcome, attempt) else {
                break;
            };
            match action {
                AgentAction::SectionComplete => break,
                AgentAction::Abort { reason } => {
                    failures.push(SectionFailure {
                        kind: FailureKind::Aborted,
                        message: reason,
                    });
                    break;
                }
                AgentAction::Continue { tokens, hold_ms } => {
                    let outcome = host.apply_input(&tokens, hold_ms);
                    record_step_telemetry(
                        &outcome,
                        &mut total_ms,
                        &mut frame_times,
                        &mut log_excerpts,
                        &mut screenshot_paths,
                        &mut trophies_unlocked,
                        &mut audio_started,
                        &mut frozen_frames,
                        &mut prev_shot,
                    );
                    last_outcome = outcome;
                    if check_fatal_step(
                        &last_outcome,
                        total_ms,
                        section.budget.timeout_ms,
                        &mut failures,
                    ) {
                        break;
                    }
                }
            }
        }
    }

    let (avg_fps, min_fps, max_frame_ms, stall_count) =
        summarize_frames(&frame_times, section.budget.stall_threshold_ms);

    if frozen_frames > section.budget.max_frozen_frames {
        failures.push(SectionFailure {
            kind: FailureKind::FrameFreeze,
            message: format!(
                "{frozen_frames} consecutive frozen frames (max {})",
                section.budget.max_frozen_frames
            ),
        });
    }
    if let Some(min_avg) = section.budget.min_avg_fps {
        match avg_fps {
            Some(fps) if fps < min_avg => failures.push(SectionFailure {
                kind: FailureKind::LowFps,
                message: format!("avg FPS {fps:.1} below budget {min_avg:.1}"),
            }),
            None => failures.push(SectionFailure {
                kind: FailureKind::Hang,
                message: "no frames recorded".into(),
            }),
            _ => {}
        }
    }
    if let Some(max_allowed_ms) = section.budget.max_frame_ms {
        if let Some(max_seen) = max_frame_ms {
            if max_seen > max_allowed_ms {
                failures.push(SectionFailure {
                    kind: FailureKind::FrameStall,
                    message: format!(
                        "max frame time {max_seen:.1}ms exceeded {max_allowed_ms:.1}ms"
                    ),
                });
            }
        }
    }
    if stall_count > section.budget.max_stalls {
        failures.push(SectionFailure {
            kind: FailureKind::FrameStall,
            message: format!(
                "{stall_count} frame stalls > {:.1}ms (max {})",
                section.budget.stall_threshold_ms, section.budget.max_stalls
            ),
        });
    }
    if section.budget.require_audio && !audio_started {
        failures.push(SectionFailure {
            kind: FailureKind::AudioMissing,
            message: "audio output never started".into(),
        });
    }
    for expected in &section.budget.expected_trophies {
        if !trophies_unlocked.iter().any(|t| t == expected) {
            failures.push(SectionFailure {
                kind: FailureKind::TrophyMissing,
                message: format!("expected trophy '{expected}' did not unlock"),
            });
        }
    }

    let checkpoint_diff = evaluate_checkpoint(
        host,
        section.checkpoint.as_ref(),
        prev_shot.as_ref(),
        &mut failures,
    );

    let status = if failures.is_empty() {
        SectionStatus::Pass
    } else {
        SectionStatus::Fail
    };

    SectionResult {
        id: section.id.clone(),
        title: section.title.clone(),
        status,
        attempts: attempt,
        recoveries: Vec::new(),
        duration_ms: total_ms,
        frames: frame_times.len(),
        avg_fps,
        min_fps,
        max_frame_ms,
        stall_count,
        frozen_frames,
        failures,
        log_excerpts,
        screenshot_paths,
        trophies_unlocked,
        checkpoint_diff,
    }
}

#[allow(clippy::too_many_arguments)]
fn record_step_telemetry(
    outcome: &StepOutcome,
    total_ms: &mut u64,
    frame_times: &mut Vec<f64>,
    log_excerpts: &mut Vec<String>,
    screenshot_paths: &mut Vec<String>,
    trophies_unlocked: &mut Vec<String>,
    audio_started: &mut bool,
    frozen_frames: &mut usize,
    prev_shot: &mut Option<RgbFrame>,
) {
    *total_ms = total_ms.saturating_add(outcome.elapsed_ms);
    frame_times.extend(outcome.frame_times_ms.iter().copied());
    *audio_started |= outcome.audio_started;
    if let Some(path) = &outcome.screenshot_path {
        screenshot_paths.push(path.clone());
    }
    for trophy in &outcome.trophies_unlocked {
        if !trophies_unlocked.contains(trophy) {
            trophies_unlocked.push(trophy.clone());
        }
    }
    for line in &outcome.log_lines {
        if log_excerpts.len() < MAX_LOG_EXCERPTS && is_notable_log(line) {
            log_excerpts.push(line.clone());
        }
    }
    if let Some(shot) = &outcome.screenshot {
        if let Some(prev) = prev_shot.as_ref() {
            if prev
                .mean_abs_diff(shot)
                .is_some_and(|d| d < FREEZE_DIFF_EPSILON)
            {
                *frozen_frames += 1;
            }
        }
        *prev_shot = Some(shot.clone());
    }
}

fn check_fatal_step(
    outcome: &StepOutcome,
    total_ms: u64,
    timeout_ms: u64,
    failures: &mut Vec<SectionFailure>,
) -> bool {
    let joined = outcome.log_lines.join("\n");
    if let Some(crash) = logs::crash_line(&joined) {
        failures.push(SectionFailure {
            kind: FailureKind::Crash,
            message: crash,
        });
        return true;
    }
    if let Some(line) = outcome.log_lines.iter().find(|l| is_crash_or_assert(l)) {
        failures.push(SectionFailure {
            kind: FailureKind::Crash,
            message: line.clone(),
        });
        return true;
    }
    if !outcome.process_alive {
        failures.push(SectionFailure {
            kind: FailureKind::Crash,
            message: format!("process exited unexpectedly (code {:?})", outcome.exit_code),
        });
        return true;
    }
    if outcome.frame_times_ms.is_empty() && outcome.elapsed_ms > 0 {
        failures.push(SectionFailure {
            kind: FailureKind::Hang,
            message: format!("no frames produced during {}ms step", outcome.elapsed_ms),
        });
        return true;
    }
    if total_ms > timeout_ms {
        failures.push(SectionFailure {
            kind: FailureKind::Hang,
            message: format!("section exceeded timeout ({total_ms}ms > {timeout_ms}ms)"),
        });
        return true;
    }
    false
}

fn evaluate_checkpoint(
    host: &mut dyn PlaytestHost,
    checkpoint: Option<&Checkpoint>,
    last_shot: Option<&RgbFrame>,
    failures: &mut Vec<SectionFailure>,
) -> Option<f64> {
    let cp = checkpoint?;
    let Some(shot) = last_shot else {
        failures.push(SectionFailure {
            kind: FailureKind::CheckpointMismatch,
            message: "checkpoint configured but no screenshot was captured".into(),
        });
        return None;
    };
    let luma = shot.mean_luma();
    if luma < cp.min_luma {
        failures.push(SectionFailure {
            kind: FailureKind::VisualBlack,
            message: format!(
                "screenshot too dark (mean luma {luma:.2} < {:.2})",
                cp.min_luma
            ),
        });
    }
    let path = cp.image_path.as_deref()?;
    match host.load_checkpoint_image(path) {
        Ok(expected) => match shot.mean_abs_diff(&expected) {
            Some(diff) => {
                if diff > cp.max_mean_diff {
                    failures.push(SectionFailure {
                        kind: FailureKind::CheckpointMismatch,
                        message: format!(
                            "checkpoint '{path}' mismatch: mean diff {diff:.2} > {:.2}",
                            cp.max_mean_diff
                        ),
                    });
                }
                Some(diff)
            }
            None => {
                failures.push(SectionFailure {
                    kind: FailureKind::CheckpointMismatch,
                    message: format!(
                        "checkpoint '{path}' size {}x{} != {}x{}",
                        expected.width, expected.height, shot.width, shot.height
                    ),
                });
                None
            }
        },
        Err(err) => {
            failures.push(SectionFailure {
                kind: FailureKind::CheckpointMismatch,
                message: format!("failed to load checkpoint '{path}': {err}"),
            });
            None
        }
    }
}

fn summarize_frames(
    frame_times: &[f64],
    stall_threshold_ms: f64,
) -> (Option<f64>, Option<f64>, Option<f64>, usize) {
    if frame_times.is_empty() {
        return (None, None, None, 0);
    }
    let total_ms: f64 = frame_times.iter().copied().sum();
    let max_ms = frame_times.iter().copied().fold(0.0_f64, f64::max);
    let stall_count = frame_times
        .iter()
        .filter(|&&ms| ms > stall_threshold_ms)
        .count();
    let avg_fps = (total_ms > 0.0)
        .then(|| (((frame_times.len() as f64) * 1000.0 / total_ms) * 10.0).round() / 10.0);
    let min_fps = (max_ms > 0.0).then(|| ((1000.0 / max_ms) * 10.0).round() / 10.0);
    (avg_fps, min_fps, Some(max_ms), stall_count)
}

fn is_crash_or_assert(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("assert")
        || lower.contains("sigsegv")
        || lower.contains("unhandled exception")
        || lower.contains("fatal")
}

fn is_notable_log(line: &str) -> bool {
    is_crash_or_assert(line)
        || line.contains("Crash:")
        || line.to_ascii_lowercase().contains("fault")
}
