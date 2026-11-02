//! The settings picker for the three display cards (SPEC.md M5, issue #174): Handheld 30,
//! Balanced 60 and Max quality. It turns the hardware probe (#171) and the benchmark results
//! (#173) into the settings the Settings page offers (`app/src/vm/settings`, ROADMAP 2.06).
//! A pure function: no files, no clock. Values are the strings the settings store keeps
//! (`frame_rate` "30" or "60", `render_resolution` "WxH", `upscale_preset` one of `off`,
//! `quality`, `balanced`, `performance`, `ultra`, `anti_aliasing` and `frame_generation` an
//! upscaler name or `off`).

/// The GPU vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Amd,
    Nvidia,
    Intel,
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
