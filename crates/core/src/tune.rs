//! The settings picker for the three display cards (SPEC.md M5, issue #174): Handheld 30,
//! Balanced 60 and Max quality. It turns the hardware probe (#171) and the benchmark results
//! (#173) into the settings the Settings page offers (`app/src/vm/settings`, ROADMAP 2.06).
//! A pure function: no files, no clock. Values are the strings the settings store keeps
//! (`frame_rate` "30" or "60", `render_resolution` "WxH", `upscale_preset` one of `off`,
//! `quality`, `balanced`, `performance`, `ultra`, `anti_aliasing` and `frame_generation` an
//! upscaler name or `off`).
//!
//! # Hardware report (#171)
//! `parse_probe` reads one JSON object describing the GPU selected for the game, rather
//! than the GPU driving the launcher window. The future runtime probe owns Vulkan device
//! enumeration, generation detection, `BB_GPU_ID`, refresh-rate and CPU feature queries.
//! No game files are needed. `choices` supplies gates; DLSS/FSR feature rules belong to #147.

use crate::error::{Error, Result};
use serde_json::Value;

/// The GPU vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Amd,
    Nvidia,
    Intel,
    Other,
}

/// The generation identified by the runtime; unknown hardware never implies RTX support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generation {
    Rdna2,
    Rdna3,
    Rdna4,
    Rtx20,
    Rtx30,
    Rtx40,
    Rtx50,
    IntelArc,
    Older,
    Unknown,
}

/// What the hardware probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub vendor: Vendor,
    /// An RTX card (DLSS is available).
    pub rtx: bool,
    /// Video memory in MiB.
    pub vram_mib: u32,
    /// The GPU shares system memory (an APU).
    pub shared_memory: bool,
    /// The display's refresh rate, when known.
    pub refresh_hz: Option<u32>,
}

/// Local hardware data from `uncharted-run --probe` (the runtime loader is still planned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareReport {
    pub device: Device,
    pub name: String,
    pub generation: Generation,
    /// The selected GPU is integrated. Shared memory alone does not establish this.
    pub integrated_gpu: bool,
    /// Both an integrated and a discrete GPU were enumerated, regardless of selection.
    pub hybrid_graphics: bool,
    pub cpu_cores: u32,
    /// The runtime's CPU instruction check, including OS support for AVX state.
    /// This is independent of any instructions the runtime can rewrite or emulate.
    pub cpu_supported: bool,
}

/// Parse the flat report object: `vendor` (`amd`, `nvidia`, `intel`, `other`), `name`,
/// `generation` (`rdna2`/`rdna3`/`rdna4`, `rtx20`/`rtx30`/`rtx40`/`rtx50`, `intel_arc`,
/// `older`, `unknown`), `vram_mib` (dedicated or shared GPU heap, never total system RAM),
/// `shared_memory`, `integrated_gpu`, `hybrid_graphics`, `refresh_hz` (positive integer or
/// null), `cpu_cores` (positive integer) and `cpu_supported` (boolean). All keys are required;
/// extra keys are ignored for future probe versions. Invalid reports give a fixed error
/// without echoing their contents. RTX support is derived, never trusted as a separate flag.
pub fn parse_probe(json: &str) -> Result<HardwareReport> {
    let damaged = || Error::Launch("The hardware probe report is damaged".into());
    let report: Value = serde_json::from_str(json).map_err(|_| damaged())?;
    let text = |key: &str| report[key].as_str().ok_or_else(damaged);
    let boolean = |key: &str| report[key].as_bool().ok_or_else(damaged);
    let number = |key: &str| {
        report[key]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(damaged)
    };
    let vendor = match text("vendor")? {
        "amd" => Vendor::Amd,
        "nvidia" => Vendor::Nvidia,
        "intel" => Vendor::Intel,
        "other" => Vendor::Other,
        _ => return Err(damaged()),
    };
    let generation = match text("generation")? {
        "rdna2" => Generation::Rdna2,
        "rdna3" => Generation::Rdna3,
        "rdna4" => Generation::Rdna4,
        "rtx20" => Generation::Rtx20,
        "rtx30" => Generation::Rtx30,
        "rtx40" => Generation::Rtx40,
        "rtx50" => Generation::Rtx50,
        "intel_arc" => Generation::IntelArc,
        "older" => Generation::Older,
        "unknown" => Generation::Unknown,
        _ => return Err(damaged()),
    };
    let generation_vendor = match generation {
        Generation::Rdna2 | Generation::Rdna3 | Generation::Rdna4 => Vendor::Amd,
        Generation::Rtx20 | Generation::Rtx30 | Generation::Rtx40 | Generation::Rtx50 => {
            Vendor::Nvidia
        }
        Generation::IntelArc => Vendor::Intel,
        Generation::Older | Generation::Unknown => vendor,
    };
    let name = text("name")?.trim().to_string();
    let cpu_cores = number("cpu_cores")?;
    if vendor != generation_vendor || name.is_empty() || cpu_cores == 0 {
        return Err(damaged());
    }
    let refresh_hz = match report.get("refresh_hz") {
        Some(Value::Null) => None,
        Some(_) => Some(number("refresh_hz")?).filter(|n| *n > 0),
        None => return Err(damaged()),
    };
    if report["refresh_hz"] != Value::Null && refresh_hz.is_none() {
        return Err(damaged());
    }
    Ok(HardwareReport {
        device: Device {
            vendor,
            rtx: matches!(
                generation,
                Generation::Rtx20 | Generation::Rtx30 | Generation::Rtx40 | Generation::Rtx50
            ),
            vram_mib: number("vram_mib")?,
            shared_memory: boolean("shared_memory")?,
            refresh_hz,
        },
        name,
        generation,
        integrated_gpu: boolean("integrated_gpu")?,
        hybrid_graphics: boolean("hybrid_graphics")?,
        cpu_cores,
        cpu_supported: boolean("cpu_supported")?,
    })
}

/// Necessary hardware gates for the Display page, before #147's upscaler feature rules.
/// A true gate permits further checks; it does not promise driver or asset availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choices {
    pub frame_generation: bool,
    pub above_1080p: bool,
    pub dlss: bool,
    pub integrated_gpu_warning: Option<&'static str>,
}

/// Gate against the selected GPU's memory, never a different adapter's larger heap.
pub fn choices(report: &HardwareReport) -> Choices {
    Choices {
        frame_generation: report.device.vram_mib >= 6144,
        above_1080p: report.device.vram_mib >= 6144,
        dlss: report.device.vendor == Vendor::Nvidia && report.device.rtx,
        integrated_gpu_warning: (report.integrated_gpu && report.hybrid_graphics).then_some(
            "The game would use the integrated GPU. Select the discrete GPU for better performance.",
        ),
    }
}

/// Average frames per second the benchmark measured on each card; `None` for a card not
/// benchmarked.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bench {
    pub handheld30: Option<f64>,
    pub balanced60: Option<f64>,
    pub max: Option<f64>,
}

/// The settings of one card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub frame_rate: String,
    pub render_resolution: String,
    /// The upscaler's name: `dlss` on an RTX card, else `fsr3`.
    pub anti_aliasing: String,
    pub upscale_preset: String,
    pub frame_generation: String,
    /// The benchmark was below target and the preset was moved one step down.
    pub stepped_down: bool,
}

/// The three cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cards {
    pub handheld30: Card,
    pub balanced60: Card,
    pub max: Card,
}

/// Pick the settings of the three cards for `device`, stepping a card's preset down when
/// `bench` shows it missed its frame-rate target (30 for `handheld30`, 60 for the others).
pub fn pick(device: &Device, bench: &Bench) -> Cards {
    let anti_aliasing = if device.rtx { "dlss" } else { "fsr3" };

    // Create base cards with default values
    let mut handheld30 = Card {
        frame_rate: "30".to_string(),
        render_resolution: "1280x720".to_string(),
        anti_aliasing: anti_aliasing.to_string(),
        upscale_preset: "quality".to_string(),
        frame_generation: "off".to_string(),
        stepped_down: false,
    };

    let mut balanced60 = Card {
        frame_rate: "60".to_string(),
        render_resolution: "1920x1080".to_string(),
        anti_aliasing: anti_aliasing.to_string(),
        upscale_preset: if device.shared_memory {
            "balanced"
        } else {
            "quality"
        }
        .to_string(),
        frame_generation: "off".to_string(),
        stepped_down: false,
    };

    // Determine max resolution based on VRAM
    let max_resolution = if !device.shared_memory {
        if device.vram_mib >= 10240 {
            "3840x2160"
        } else if device.vram_mib >= 6144 {
            "2560x1440"
        } else {
            "1920x1080"
        }
    } else {
        "1920x1080"
    };

    let mut max = Card {
        frame_rate: "60".to_string(),
        render_resolution: max_resolution.to_string(),
        anti_aliasing: anti_aliasing.to_string(),
        upscale_preset: "quality".to_string(),
        frame_generation: if device.rtx {
            "dlss"
        } else if device.vram_mib >= 6144 && device.refresh_hz.unwrap_or(0) >= 120 {
            "fsr3"
        } else {
            "off"
        }
        .to_string(),
        stepped_down: false,
    };

    // Handle low memory case
    let low_memory = device.vram_mib < 6144;
    if low_memory {
        handheld30.frame_generation = "off".to_string();
        balanced60.frame_generation = "off".to_string();
        max.frame_generation = "off".to_string();

        // Cap resolutions to 1920x1080 for balanced60 and max cards only
        balanced60.render_resolution = "1920x1080".to_string();
        max.render_resolution = "1920x1080".to_string();
    }

    // Process benchmark results and step presets down if needed
    let step_down_preset = |preset: &str| -> String {
        const STEPS: [&str; 5] = ["off", "quality", "balanced", "performance", "ultra"];
        match STEPS.iter().position(|step| *step == preset) {
            Some(at) => STEPS[(at + 1).min(STEPS.len() - 1)].to_string(),
            None => preset.to_string(),
        }
    };

    // Check handheld30 benchmark
    if let Some(fps) = bench.handheld30 {
        if fps < 0.95 * 30.0 {
            handheld30.upscale_preset = step_down_preset(&handheld30.upscale_preset);
            handheld30.stepped_down = true;
        }
    }

    // Check balanced60 benchmark
    if let Some(fps) = bench.balanced60 {
        if fps < 0.95 * 60.0 {
            balanced60.upscale_preset = step_down_preset(&balanced60.upscale_preset);
            balanced60.stepped_down = true;
        }
    }

    // Check max benchmark
    if let Some(fps) = bench.max {
        if fps < 0.95 * 60.0 {
            max.upscale_preset = step_down_preset(&max.upscale_preset);
            max.stepped_down = true;
        }
    }

    Cards {
        handheld30,
        balanced60,
        max,
    }
}
