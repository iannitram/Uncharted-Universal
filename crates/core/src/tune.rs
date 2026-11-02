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
    not_built!("tune::pick")
}
