//! The hardware report that `crates/core::tune` reads (#171): `uncharted-run --probe`
//! prints one JSON object for the GPU the game will use. Replaces the device ranking of
//! `tools/gpu_capabilities.c` and adds what the launcher's choices need: vendor, generation,
//! video memory, integrated or hybrid graphics, CPU cores and the CPU check of `probe.c`.
//!
//! Vulkan is loaded at run time (`ash`, the `loaded` feature), so a machine without a Vulkan
//! loader gets an error message instead of a missing-library failure at start. No game files
//! are read and nothing leaves the machine.

use serde_json::json;

/// What the device type says about the GPU (`VkPhysicalDeviceType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Discrete,
    Integrated,
    Virtual,
    Cpu,
    Other,
}

/// One Vulkan physical device, as far as the report needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    /// PCI vendor id (`VkPhysicalDeviceProperties::vendorID`).
    pub vendor_id: u32,
    /// `deviceName`, as the driver reports it.
    pub name: String,
    pub kind: Kind,
    /// `apiVersion`.
    pub api_version: u32,
    /// The largest heap with `VK_MEMORY_HEAP_DEVICE_LOCAL_BIT`, in bytes.
    pub device_local_bytes: u64,
}

/// Vulkan 1.3, the version the renderer needs (`TargetVulkanApiVersion`).
pub const TARGET_API_VERSION: u32 = (1 << 22) | (3 << 12);

/// The device the game will use: the index `BB_GPU_ID` names when it reads as zero or more
/// (the game parses it with `strtol`, so text without digits is 0), else the game's ranking:
/// Vulkan 1.3 first, then discrete, then anything but a CPU renderer, then the larger
/// device-local heap; on a tie the first enumerated wins.
pub fn select(adapters: &[Adapter], gpu_id: Option<&str>) -> Result<usize, String> {
    if adapters.is_empty() {
        return Err("no Vulkan device".into());
    }
    if let Some(index) = gpu_id.map(strtol).filter(|n| *n >= 0) {
        return usize::try_from(index)
            .ok()
            .filter(|i| *i < adapters.len())
            .ok_or_else(|| {
                format!(
                    "BB_GPU_ID {index} is past the last Vulkan device ({} found)",
                    adapters.len()
                )
            });
    }
    let rank = |a: &Adapter| {
        (
            a.api_version >= TARGET_API_VERSION,
            a.kind == Kind::Discrete,
            a.kind != Kind::Cpu,
            a.device_local_bytes,
        )
    };
    let mut best = 0;
    for (i, adapter) in adapters.iter().enumerate().skip(1) {
        if rank(adapter) > rank(&adapters[best]) {
            best = i;
        }
    }
    Ok(best)
}

// C's strtol in base 10: leading white space, an optional sign, then digits; no digits is 0.
// Saturates instead of wrapping.
fn strtol(text: &str) -> i64 {
    let text = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let value = digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0i64, |n, d| {
            n.saturating_mul(10).saturating_add(i64::from(d - b'0'))
        });
    if negative {
        -value
    } else {
        value
    }
}

/// `amd`, `nvidia`, `intel` or `other`, from the PCI vendor id.
pub fn vendor(vendor_id: u32) -> &'static str {
    match vendor_id {
        0x1002 => "amd",
        0x10de => "nvidia",
        0x8086 => "intel",
        _ => "other",
    }
}

/// The generation names `crates/core::tune::parse_probe` accepts, from the vendor and the
/// driver's device name. A name it cannot place is `unknown`, which never implies RTX.
pub fn generation(vendor_id: u32, name: &str) -> &'static str {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_uppercase)
        .collect();
    let has = |word: &str| words.iter().any(|w| w == word);
    match vendor(vendor_id) {
        "nvidia" => nvidia_generation(&words, has("ADA"), has("BLACKWELL")),
        "amd" => amd_generation(name, &words),
        "intel" if has("ARC") => "intel_arc",
        "intel" => "older",
        _ => "unknown",
    }
}

// "GeForce RTX 3090 Ti", "RTX A4000" (Ampere), "RTX 4000 Ada Generation", "Quadro RTX 4000"
// and "TITAN RTX" (Turing), "RTX PRO 6000 Blackwell"; no RTX in the name is older (GTX, MX).
fn nvidia_generation(words: &[String], ada: bool, blackwell: bool) -> &'static str {
    let Some(rtx) = words.iter().position(|w| w == "RTX") else {
        return "older";
    };
    if blackwell {
        return "rtx50";
    }
    if ada {
        return "rtx40";
    }
    if words[..rtx].iter().any(|w| w == "QUADRO" || w == "TITAN") {
        return "rtx20";
    }
    let model = words.get(rtx + 1).map(String::as_str).unwrap_or("");
    if model.len() > 1 && model.starts_with('A') && model[1..].bytes().all(|b| b.is_ascii_digit()) {
        return "rtx30";
    }
    match (model.len(), model.as_bytes().first()) {
        (4, Some(b'2')) if is_number(model) => "rtx20",
        (4, Some(b'3')) if is_number(model) => "rtx30",
        (4, Some(b'4')) if is_number(model) => "rtx40",
        (4, Some(b'5')) if is_number(model) => "rtx50",
        _ => "unknown",
    }
}

fn is_number(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit())
}

// RADV names the chip: "AMD Radeon 780M Graphics (RADV PHOENIX)". Other drivers give the
// model: "AMD Radeon RX 7800 XT", "AMD Radeon(TM) 780M Graphics", "AMD Radeon(TM) 8060S".
fn amd_generation(name: &str, words: &[String]) -> &'static str {
    if let Some(chip) = name
        .split("(RADV ")
        .nth(1)
        .and_then(|r| r.split(')').next())
    {
        return radv_generation(&chip.to_ascii_uppercase());
    }
    if let Some(rx) = words.iter().position(|w| w == "RX") {
        let model = words.get(rx + 1).map(String::as_str).unwrap_or("");
        let digits: String = model.chars().take_while(char::is_ascii_digit).collect();
        return match (digits.len(), digits.as_bytes().first()) {
            (4, Some(b'6')) => "rdna2",
            (4, Some(b'7')) => "rdna3",
            (4, Some(b'9')) => "rdna4",
            (3, _) | (4, Some(b'5')) => "older",
            _ => "unknown",
        };
    }
    // Integrated: 610M, 660M, 680M and 820M are RDNA 2; 740M to 780M and 840M to 890M are
    // RDNA 3 and 3.5; 8040S to 8060S (Strix Halo) are RDNA 3.5. The Steam Deck is RDNA 2.
    for word in words {
        let (digits, suffix) = word.split_at(word.len() - 1);
        if !is_number(digits) {
            continue;
        }
        match (suffix, digits) {
            ("M", "610" | "660" | "680" | "820") => return "rdna2",
            ("M", d) if d.len() == 3 && (d.starts_with('7') || d.starts_with('8')) => {
                return "rdna3"
            }
            ("S", d) if d.len() == 4 && d.starts_with("80") => return "rdna3",
            _ => {}
        }
    }
    if words.windows(3).any(|w| w[0] == "CUSTOM" && w[1] == "GPU") {
        return "rdna2";
    }
    "unknown"
}

fn radv_generation(chip: &str) -> &'static str {
    let chip = chip.split("_R").next().unwrap_or(chip); // GFX1103_R1
    match chip {
        "NAVI21" | "NAVI22" | "NAVI23" | "NAVI24" | "VANGOGH" | "REMBRANDT"
        | "RAPHAEL_MENDOCINO" | "MENDOCINO" | "SIENNA_CICHLID" | "NAVY_FLOUNDER"
        | "DIMGREY_CAVEFISH" | "BEIGE_GOBY" | "YELLOW_CARP" => "rdna2",
        "NAVI31" | "NAVI32" | "NAVI33" | "PHOENIX" | "PHOENIX2" | "HAWK_POINT1" | "HAWK_POINT2"
        | "STRIX1" | "STRIX_HALO" | "KRACKAN1" => "rdna3",
        "NAVI44" | "NAVI48" => "rdna4",
        "NAVI10" | "NAVI12" | "NAVI14" | "RAVEN" | "RAVEN2" | "RENOIR" | "VEGA10" | "VEGA12"
        | "VEGA20" | "VEGAM" | "POLARIS10" | "POLARIS11" | "POLARIS12" | "TONGA" | "FIJI"
        | "CARRIZO" | "STONEY" | "BONAIRE" | "HAWAII" | "KAVERI" | "KABINI" => "older",
        gfx if gfx.starts_with("GFX103") => "rdna2",
        gfx if gfx.starts_with("GFX11") => "rdna3",
        gfx if gfx.starts_with("GFX12") => "rdna4",
        gfx if gfx.starts_with("GFX10") || gfx.starts_with("GFX9") || gfx.starts_with("GFX8") => {
            "older"
        }
        _ => "unknown",
    }
}

/// The CPU instructions the game cannot run without (`check_cpu` in `src/probe.c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuFeatures {
    /// AVX, with the OS saving the YMM state.
    pub avx: bool,
    pub bmi1: bool,
    pub movbe: bool,
    pub lzcnt: bool,
    pub popcnt: bool,
    pub sse42: bool,
}

/// Whether the game can run on a CPU with `features`: AVX, POPCNT and SSE4.2 always; BMI1,
/// LZCNT and MOVBE only when `rewrite` is off (`BB_CPU_PATCH=0`), except MOVBE on Windows,
/// where the fault handler emulates it.
pub fn cpu_supported(features: CpuFeatures, rewrite: bool, windows: bool) -> bool {
    let always = features.avx && features.popcnt && features.sse42;
    always && (rewrite || (features.bmi1 && features.lzcnt && (features.movbe || windows)))
}

/// The CPU this runs on: its logical cores and whether the game can run on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cpu {
    pub cores: u32,
    pub supported: bool,
}

/// Read the CPU this process runs on. On a CPU that is not x86-64 the game's code is
/// translated (FEX on Android, the Apple route), so the check does not apply.
pub fn cpu() -> Cpu {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let rewrite = std::env::var_os("BB_CPU_PATCH").is_none_or(|v| v != "0");
    Cpu {
        cores: u32::try_from(cores).unwrap_or(u32::MAX),
        supported: this_cpu_supported(rewrite),
    }
}

#[cfg(target_arch = "x86_64")]
fn this_cpu_supported(rewrite: bool) -> bool {
    use std::arch::is_x86_feature_detected as has;
    // std's "avx" also checks that the OS saves the YMM state (XGETBV), as probe.c does.
    let features = CpuFeatures {
        avx: has!("avx"),
        bmi1: has!("bmi1"),
        movbe: has!("movbe"),
        lzcnt: has!("lzcnt"),
        popcnt: has!("popcnt"),
        sse42: has!("sse4.2"),
    };
    cpu_supported(features, rewrite, cfg!(windows))
}

#[cfg(not(target_arch = "x86_64"))]
fn this_cpu_supported(_rewrite: bool) -> bool {
    true
}

/// The report for `adapters[selected]`, with the keys and values `parse_probe` reads.
/// `hybrid_graphics` looks at every adapter, not only the selected one. `refresh_hz` is null:
/// Vulkan cannot tell without a window, and the launcher knows its own display.
pub fn report(adapters: &[Adapter], selected: usize, cpu: Cpu) -> String {
    let gpu = &adapters[selected];
    let name = match gpu.name.trim() {
        "" => "Unknown GPU",
        name => name,
    };
    let any = |kind| adapters.iter().any(|a| a.kind == kind);
    json!({
        "vendor": vendor(gpu.vendor_id),
        "name": name,
        "generation": generation(gpu.vendor_id, name),
        "vram_mib": u32::try_from(gpu.device_local_bytes >> 20).unwrap_or(u32::MAX),
        "shared_memory": gpu.kind != Kind::Discrete,
        "integrated_gpu": gpu.kind == Kind::Integrated,
        "hybrid_graphics": any(Kind::Integrated) && any(Kind::Discrete),
        "refresh_hz": null,
        "cpu_cores": cpu.cores.max(1),
        "cpu_supported": cpu.supported,
    })
    .to_string()
}

/// Enumerate the Vulkan devices, select the game's, and return its report. `gpu_id` is
/// `BB_GPU_ID`. Errors are one line for the player: no loader, no device, a bad index.
pub fn run(gpu_id: Option<&str>) -> Result<String, String> {
    let adapters = enumerate()?;
    let selected = select(&adapters, gpu_id)?;
    Ok(report(&adapters, selected, cpu()))
}

/// Every Vulkan physical device, in the loader's order (the order `BB_GPU_ID` counts in).
pub fn enumerate() -> Result<Vec<Adapter>, String> {
    use ash::vk;
    // SAFETY: loading the Vulkan loader runs its initialisers; nothing else in this process
    // depends on it, and the entry points stay valid while `entry` lives (this function).
    let entry = unsafe { ash::Entry::load() }.map_err(|e| format!("no Vulkan loader ({e})"))?;
    let app = vk::ApplicationInfo::default()
        .application_name(c"uncharted-run probe")
        .api_version(vk::API_VERSION_1_3);
    let create = vk::InstanceCreateInfo::default().application_info(&app);
    // SAFETY: `create` and the `app` it points to outlive the call; no allocation callbacks.
    let instance = unsafe { entry.create_instance(&create, None) }
        .map_err(|e| format!("cannot create a Vulkan instance ({e})"))?;
    // SAFETY: `instance` is valid until `destroy_instance` below, and the physical devices it
    // returns are only queried before then.
    let adapters = unsafe { instance.enumerate_physical_devices() }
        .map_err(|e| format!("cannot list Vulkan devices ({e})"))
        .map(|devices| {
            devices
                .into_iter()
                .map(|device| {
                    // SAFETY: `device` came from this live instance.
                    let (properties, memory) = unsafe {
                        (
                            instance.get_physical_device_properties(device),
                            instance.get_physical_device_memory_properties(device),
                        )
                    };
                    Adapter {
                        vendor_id: properties.vendor_id,
                        name: properties
                            .device_name_as_c_str()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        kind: match properties.device_type {
                            vk::PhysicalDeviceType::DISCRETE_GPU => Kind::Discrete,
                            vk::PhysicalDeviceType::INTEGRATED_GPU => Kind::Integrated,
                            vk::PhysicalDeviceType::VIRTUAL_GPU => Kind::Virtual,
                            vk::PhysicalDeviceType::CPU => Kind::Cpu,
                            _ => Kind::Other,
                        },
                        api_version: properties.api_version,
                        device_local_bytes: memory
                            .memory_heaps_as_slice()
                            .iter()
                            .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
                            .map(|h| h.size)
                            .max()
                            .unwrap_or(0),
                    }
                })
                .collect()
        });
    // SAFETY: nothing created from `instance` outlives this point.
    unsafe { instance.destroy_instance(None) };
    adapters
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const GIB: u64 = 1 << 30;

    fn adapter(vendor_id: u32, name: &str, kind: Kind, api: u32, local: u64) -> Adapter {
        Adapter {
            vendor_id,
            name: name.into(),
            kind,
            api_version: api,
            device_local_bytes: local,
        }
    }

    fn radeon_780m() -> Adapter {
        adapter(
            0x1002,
            "AMD Radeon 780M Graphics (RADV PHOENIX)",
            Kind::Integrated,
            TARGET_API_VERSION,
            4 * GIB,
        )
    }

    fn rtx_4060_laptop() -> Adapter {
        adapter(
            0x10de,
            "NVIDIA GeForce RTX 4060 Laptop GPU",
            Kind::Discrete,
            TARGET_API_VERSION,
            8188 << 20,
        )
    }

    fn llvmpipe() -> Adapter {
        adapter(
            0x10005,
            "llvmpipe (LLVM 17.0.6, 256 bits)",
            Kind::Cpu,
            TARGET_API_VERSION,
            64 * GIB,
        )
    }

    const VULKAN_1_2: u32 = (1 << 22) | (2 << 12);

    #[test]
    fn probe_selection_follows_the_game_ranking() {
        let discrete_wins = [radeon_780m(), rtx_4060_laptop()];
        assert_eq!(select(&discrete_wins, None), Ok(1));
        let old_api = Adapter {
            api_version: VULKAN_1_2,
            ..rtx_4060_laptop()
        };
        assert_eq!(select(&[old_api, radeon_780m()], None), Ok(1), "1.3 first");
        assert_eq!(
            select(&[llvmpipe(), radeon_780m()], None),
            Ok(1),
            "a CPU renderer loses before its memory counts"
        );
        let small = adapter(0x1002, "small", Kind::Discrete, TARGET_API_VERSION, 8 * GIB);
        let big = adapter(0x1002, "big", Kind::Discrete, TARGET_API_VERSION, 16 * GIB);
        assert_eq!(select(&[small.clone(), big.clone()], None), Ok(1));
        assert_eq!(select(&[big.clone(), small], None), Ok(0));
        assert_eq!(
            select(&[big.clone(), big], None),
            Ok(0),
            "a tie keeps the first"
        );
        assert_eq!(select(&[llvmpipe()], None), Ok(0));
        assert!(select(&[], None).is_err());
    }

    #[test]
    fn probe_bb_gpu_id_reads_like_strtol() {
        let two = [radeon_780m(), rtx_4060_laptop()];
        for (text, want) in [
            ("0", Ok(0)),
            ("1", Ok(1)),
            (" 1", Ok(1)),
            ("+1", Ok(1)),
            ("1x", Ok(1)),
            ("abc", Ok(0)),
            ("", Ok(0)),
            ("-1", Ok(1)),
            ("-7", Ok(1)),
        ] {
            assert_eq!(select(&two, Some(text)), want, "BB_GPU_ID={text:?}");
        }
        for out in ["2", "99999999999999999999"] {
            let error = select(&two, Some(out)).unwrap_err();
            assert!(error.contains("BB_GPU_ID"), "{error}");
        }
    }

    #[test]
    fn probe_vendor_from_the_pci_id() {
        assert_eq!(vendor(0x1002), "amd");
        assert_eq!(vendor(0x10de), "nvidia");
        assert_eq!(vendor(0x8086), "intel");
        assert_eq!(vendor(0x10005), "other");
        assert_eq!(vendor(0x106b), "other");
    }

    #[test]
    fn probe_generation_from_driver_device_names() {
        let (amd, nvidia, intel) = (0x1002, 0x10de, 0x8086);
        for (vendor_id, name, want) in [
            (nvidia, "NVIDIA GeForce RTX 3090 Ti", "rtx30"),
            (nvidia, "NVIDIA GeForce RTX 4070", "rtx40"),
            (nvidia, "NVIDIA GeForce RTX 4060 Laptop GPU", "rtx40"),
            (nvidia, "NVIDIA GeForce RTX 5080", "rtx50"),
            (nvidia, "NVIDIA GeForce RTX 2060 SUPER", "rtx20"),
            (nvidia, "NVIDIA TITAN RTX", "rtx20"),
            (nvidia, "Quadro RTX 4000", "rtx20"),
            (nvidia, "NVIDIA RTX A4000", "rtx30"),
            (nvidia, "NVIDIA RTX 4000 Ada Generation", "rtx40"),
            (
                nvidia,
                "NVIDIA RTX PRO 6000 Blackwell Workstation Edition",
                "rtx50",
            ),
            (nvidia, "NVIDIA GeForce GTX 1660 SUPER", "older"),
            (nvidia, "NVIDIA GeForce GTX 1080 Ti", "older"),
            (nvidia, "NVIDIA GeForce RTX 6090", "unknown"),
            (amd, "AMD Radeon RX 6800 XT (RADV NAVI21)", "rdna2"),
            (amd, "AMD Custom GPU 0405 (RADV VANGOGH)", "rdna2"),
            (amd, "AMD Radeon 680M (RADV REMBRANDT)", "rdna2"),
            (amd, "AMD Radeon 780M Graphics (RADV PHOENIX)", "rdna3"),
            (amd, "AMD Radeon 740M Graphics (RADV PHOENIX2)", "rdna3"),
            (amd, "AMD Radeon Graphics (RADV STRIX_HALO)", "rdna3"),
            (amd, "AMD Radeon 890M Graphics (RADV GFX1150)", "rdna3"),
            (amd, "AMD Radeon Graphics (RADV GFX1103_R1)", "rdna3"),
            (amd, "AMD Radeon RX 7900 XTX (RADV NAVI31)", "rdna3"),
            (amd, "AMD Radeon RX 9070 XT (RADV GFX1201)", "rdna4"),
            (amd, "AMD Radeon RX 9060 XT (RADV NAVI44)", "rdna4"),
            (amd, "AMD Radeon RX 580 Series (RADV POLARIS10)", "older"),
            (amd, "AMD Radeon RX 5700 XT (RADV NAVI10)", "older"),
            (amd, "AMD Radeon Graphics (RADV SOMETHING_NEW)", "unknown"),
            (amd, "AMD Radeon RX 6700 XT", "rdna2"),
            (amd, "AMD Radeon RX 6800S", "rdna2"),
            (amd, "AMD Radeon RX 7800 XT", "rdna3"),
            (amd, "AMD Radeon RX 7600M XT", "rdna3"),
            (amd, "AMD Radeon RX 9070", "rdna4"),
            (amd, "AMD Radeon RX 5600 XT", "older"),
            (amd, "AMD Radeon RX 580 2048SP", "older"),
            (amd, "AMD Radeon(TM) 680M", "rdna2"),
            (amd, "AMD Radeon(TM) 610M", "rdna2"),
            (amd, "AMD Radeon(TM) 820M Graphics", "rdna2"),
            (amd, "AMD Radeon 780M Graphics", "rdna3"),
            (amd, "AMD Radeon(TM) 890M Graphics", "rdna3"),
            (amd, "AMD Radeon(TM) 8060S Graphics", "rdna3"),
            (amd, "AMD Custom GPU 0405", "rdna2"),
            (amd, "AMD Radeon(TM) Graphics", "unknown"),
            (intel, "Intel(R) Arc(TM) A770 Graphics", "intel_arc"),
            (intel, "Intel(R) Arc(tm) A750 Graphics (DG2)", "intel_arc"),
            (intel, "Intel(R) Arc(TM) B580 Graphics", "intel_arc"),
            (intel, "Intel(R) Arc(TM) Graphics", "intel_arc"),
            (intel, "Intel(R) UHD Graphics 770", "older"),
            (intel, "Intel(R) Iris(R) Xe Graphics", "older"),
            (intel, "Intel(R) Graphics (Search)", "older"),
            (0x10005, "llvmpipe (LLVM 17.0.6, 256 bits)", "unknown"),
            (0x106b, "Apple M2", "unknown"),
        ] {
            assert_eq!(generation(vendor_id, name), want, "{name}");
        }
    }

    #[test]
    fn probe_cpu_check_matches_check_cpu() {
        let all = CpuFeatures {
            avx: true,
            bmi1: true,
            movbe: true,
            lzcnt: true,
            popcnt: true,
            sse42: true,
        };
        for windows in [false, true] {
            assert!(cpu_supported(all, true, windows));
            assert!(cpu_supported(all, false, windows));
            for hard in [
                CpuFeatures { avx: false, ..all },
                CpuFeatures {
                    popcnt: false,
                    ..all
                },
                CpuFeatures {
                    sse42: false,
                    ..all
                },
            ] {
                assert!(!cpu_supported(hard, true, windows), "{hard:?}");
            }
            // Rewritten before the game starts unless BB_CPU_PATCH=0.
            let old = CpuFeatures {
                bmi1: false,
                lzcnt: false,
                movbe: false,
                ..all
            };
            assert!(cpu_supported(old, true, windows));
            assert!(!cpu_supported(
                CpuFeatures { bmi1: false, ..all },
                false,
                windows
            ));
            assert!(!cpu_supported(
                CpuFeatures {
                    lzcnt: false,
                    ..all
                },
                false,
                windows
            ));
        }
        // MOVBE alone (AMD FX): emulated by the Windows fault handler, required on Linux.
        let no_movbe = CpuFeatures {
            movbe: false,
            ..all
        };
        assert!(cpu_supported(no_movbe, false, true));
        assert!(!cpu_supported(no_movbe, false, false));
    }

    #[test]
    fn probe_this_cpu_has_cores() {
        assert!(cpu().cores >= 1);
    }

    fn parsed(adapters: &[Adapter], selected: usize) -> Value {
        let cpu = Cpu {
            cores: 16,
            supported: true,
        };
        serde_json::from_str(&report(adapters, selected, cpu)).unwrap()
    }

    #[test]
    fn probe_report_has_exactly_the_keys_tune_reads() {
        let got = parsed(&[radeon_780m()], 0);
        let mut keys: Vec<_> = got.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "cpu_cores",
                "cpu_supported",
                "generation",
                "hybrid_graphics",
                "integrated_gpu",
                "name",
                "refresh_hz",
                "shared_memory",
                "vendor",
                "vram_mib",
            ]
        );
        assert_eq!(
            got,
            json!({
                "vendor": "amd", "name": "AMD Radeon 780M Graphics (RADV PHOENIX)",
                "generation": "rdna3", "vram_mib": 4096, "shared_memory": true,
                "integrated_gpu": true, "hybrid_graphics": false, "refresh_hz": null,
                "cpu_cores": 16, "cpu_supported": true,
            })
        );
    }

    #[test]
    fn probe_report_on_a_hybrid_laptop_describes_the_selected_gpu() {
        let laptop = [radeon_780m(), rtx_4060_laptop()];
        let discrete = parsed(&laptop, 1);
        assert_eq!(discrete["vendor"], "nvidia");
        assert_eq!(discrete["generation"], "rtx40");
        assert_eq!(discrete["vram_mib"], 8188);
        assert_eq!(discrete["shared_memory"], false);
        assert_eq!(discrete["integrated_gpu"], false);
        assert_eq!(discrete["hybrid_graphics"], true);

        let integrated = parsed(&laptop, 0);
        assert_eq!(
            integrated["vram_mib"], 4096,
            "never the other adapter's heap"
        );
        assert_eq!(integrated["integrated_gpu"], true);
        assert_eq!(integrated["hybrid_graphics"], true);
    }

    #[test]
    fn probe_report_rounds_memory_down_and_never_leaves_a_name_empty() {
        let odd = adapter(
            0x8086,
            "  ",
            Kind::Integrated,
            TARGET_API_VERSION,
            (2 << 30) - 1,
        );
        let got = parsed(&[odd], 0);
        assert_eq!(got["vram_mib"], 2047);
        assert_eq!(got["name"], "Unknown GPU");
        let cpu_renderer = parsed(&[llvmpipe()], 0);
        assert_eq!(cpu_renderer["vendor"], "other");
        assert_eq!(cpu_renderer["shared_memory"], true);
        assert_eq!(cpu_renderer["integrated_gpu"], false);
        let huge = adapter(0x10de, "RTX", Kind::Discrete, TARGET_API_VERSION, u64::MAX);
        assert_eq!(parsed(&[huge], 0)["vram_mib"], u32::MAX);
    }

    /// On a machine with a Vulkan driver this prints a real report; without one (most CI
    /// runners) it must explain in one line rather than crash.
    #[test]
    fn probe_live_run_reports_or_explains() {
        match run(None) {
            Ok(json) => {
                let got: Value = serde_json::from_str(&json).unwrap();
                assert_eq!(got.as_object().unwrap().len(), 10, "{json}");
                eprintln!("probe report: {json}");
            }
            Err(message) => {
                assert!(!message.is_empty() && !message.contains('\n'), "{message}");
                eprintln!("probe error: {message}");
            }
        }
    }
}
