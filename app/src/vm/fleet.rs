//! The Fleet page (SPEC.md M12, roadmap 2.08): each paired device's build, patch level, last
//! sync and free space, and the drift warning before play. Plain Rust, no Slint types: the
//! `.slint` file draws [`FleetRow`]s and the texts below.
//!
//! Records are read with `uncharted_core::fleet::from_line` and compared with `drift` and
//! `warning`, the same functions the `uncharted-universal fleet` command uses. Until the hub
//! keeps real records (roadmap 4.04) the app reads a fixture file of `to_line` lines: this
//! device first, then the paired devices (`app/fixtures/fleet.tsv`).

use crate::focus::{FocusRing, Nav};
use uncharted_core::fleet::{self, DeviceRecord};
use uncharted_core::platform::Os;
use uncharted_core::Result;

/// What the page shows when no device is paired yet.
pub const EMPTY: &str =
    "No paired devices yet. Pair this device with a build hub and the others show up here.";

/// How many characters of a build id a row shows.
const BUILD_CHARS: usize = 12;

/// One device as the page draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetRow {
    /// The device's name; this device's row says so.
    pub name: String,
    /// "<OS> · build <id> · <n> fixes".
    pub detail: String,
    /// "synced <date> UTC", or "never synced".
    pub sync: String,
    /// "<n> GB free" or "<n> MB free".
    pub free: String,
}

/// The Fleet page's state: the rows, the warning and which item has focus. The items are the
/// rows, then the Back button.
#[derive(Debug)]
pub struct FleetVm {
    rows: Vec<FleetRow>,
    paired: usize,
    warning: Option<String>,
    focus: FocusRing,
}

impl FleetVm {
    /// No records at all: no rows, the empty text, focus on Back.
    pub fn empty() -> Self {
        Self::new(None, Vec::new())
    }

    /// `here` is this device's own record, `paired` the devices it is paired with. The device
    /// that made the latest save is the one that synced last (until the save card names it);
    /// the warning is `fleet::warning` for this device against that one.
    pub fn new(here: Option<DeviceRecord>, paired: Vec<DeviceRecord>) -> Self {
        let warning = here.as_ref().and_then(|here| {
            let saved_by = paired.iter().fold(here, |latest, r| {
                if r.last_sync_unix > latest.last_sync_unix {
                    r
                } else {
                    latest
                }
            });
            fleet::warning(&fleet::drift(here, saved_by), saved_by)
        });
        let mut rows: Vec<FleetRow> = here.iter().map(|r| row(r, true)).collect();
        rows.extend(paired.iter().map(|r| row(r, false)));
        let focus = FocusRing::new(rows.len() + 1);
        FleetVm {
            rows,
            paired: paired.len(),
            warning,
            focus,
        }
    }

    /// Reads a records file: one `fleet::to_line` line per device, this device first. Blank
    /// lines are skipped; a Windows line ending is fine. Any other line is the `Error::Hub`
    /// from `fleet::from_line`.
    pub fn from_text(text: &str) -> Result<Self> {
        let mut records = text
            .lines()
            .filter(|line| !line.is_empty())
            .map(fleet::from_line)
            .collect::<Result<Vec<_>>>()?
            .into_iter();
        let here = records.next();
        Ok(Self::new(here, records.collect()))
    }

    pub fn rows(&self) -> &[FleetRow] {
        &self.rows
    }

    /// The drift warning, or `None` when this device has the build that made the latest save.
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    /// [`EMPTY`] when no device is paired, else `None`.
    pub fn empty_text(&self) -> Option<&str> {
        (self.paired == 0).then_some(EMPTY)
    }

    /// The focused item: a row index, or [`FleetVm::back_index`].
    pub fn focused(&self) -> usize {
        self.focus.index()
    }

    /// The Back button's item index, after the last row.
    pub fn back_index(&self) -> usize {
        self.rows.len()
    }

    /// Moves focus through the rows and Back; moves wrap around.
    pub fn on_nav(&mut self, nav: Nav) {
        self.focus.step(nav);
    }

    /// Focus item `index` (a click); `false`, and no change, past the Back button.
    pub fn focus_on(&mut self, index: usize) -> bool {
        self.focus.focus_on(index)
    }

    /// Puts focus back on the first item (each time the page opens).
    pub fn reset_focus(&mut self) {
        self.focus = FocusRing::new(self.rows.len() + 1);
    }
}

fn row(r: &DeviceRecord, here: bool) -> FleetRow {
    let name = if here {
        format!("{} (this device)", r.device)
    } else {
        r.device.clone()
    };
    let build: String = r.build_id.chars().take(BUILD_CHARS).collect();
    FleetRow {
        name,
        detail: format!(
            "{} · build {build} · {}",
            os_name(r.os),
            fixes(r.patch_level)
        ),
        sync: sync_text(r.last_sync_unix),
        free: free_text(r.free_bytes),
    }
}

fn os_name(os: Os) -> &'static str {
    match os {
        Os::Linux => "Linux",
        Os::Windows => "Windows",
        Os::Mac => "macOS",
        Os::Ios => "iOS",
        Os::Android => "Android",
    }
}

/// "1 fix", "7 fixes": the patch level in the warning's words ("of our fixes").
fn fixes(n: u32) -> String {
    if n == 1 {
        "1 fix".to_string()
    } else {
        format!("{n} fixes")
    }
}

/// "synced 2025-11-12 00:00 UTC"; 0 is "never synced".
fn sync_text(unix: u64) -> String {
    if unix == 0 {
        return "never synced".to_string();
    }
    let (y, m, d) = civil_from_days((unix / 86_400) as i64);
    let secs = unix % 86_400;
    format!(
        "synced {y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        secs / 3_600,
        secs % 3_600 / 60
    )
}

/// Decimal units, as drives are sold: one decimal of GB from 1 GB up, whole MB below.
fn free_text(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    if bytes >= GB {
        format!("{:.1} GB free", bytes as f64 / GB as f64)
    } else {
        format!("{} MB free", bytes / 1_000_000)
    }
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day) (H. Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../fixtures/fleet.tsv");

    fn record(device: &str, build_id: &str, patch_level: u32, last_sync_unix: u64) -> DeviceRecord {
        DeviceRecord {
            device: device.to_string(),
            os: Os::Linux,
            build_id: build_id.to_string(),
            patch_level,
            last_sync_unix,
            free_bytes: 64_000_000_000,
        }
    }

    #[test]
    fn fixture_rows_show_build_patch_level_sync_and_free_space() {
        let vm = FleetVm::from_text(FIXTURE).unwrap();
        assert_eq!(
            vm.rows(),
            [
                FleetRow {
                    name: "AMD handheld (this device)".to_string(),
                    detail: "Linux · build 5b0c1d2e3f4a · 7 fixes".to_string(),
                    sync: "synced 2025-11-12 00:00 UTC".to_string(),
                    free: "64.0 GB free".to_string(),
                },
                FleetRow {
                    name: "Strix Halo tablet".to_string(),
                    detail: "Linux · build 9e8d7c6b5a4f · 9 fixes".to_string(),
                    sync: "synced 2025-11-13 00:00 UTC".to_string(),
                    free: "812.0 GB free".to_string(),
                },
                FleetRow {
                    name: "RTX 3090 Ti desktop".to_string(),
                    detail: "Windows · build 5b0c1d2e3f4a · 7 fixes".to_string(),
                    sync: "never synced".to_string(),
                    free: "1500.0 GB free".to_string(),
                },
            ]
        );
        assert_eq!(vm.empty_text(), None);
    }

    #[test]
    fn fixture_warns_with_the_device_that_synced_last() {
        let vm = FleetVm::from_text(FIXTURE).unwrap();
        assert_eq!(
            vm.warning(),
            Some(
                "Strix Halo tablet made this save with 9 of our fixes; this device has 7. \
                 Update this device first, or play on and keep both saves."
            )
        );
    }

    #[test]
    fn warning_text_is_fleet_warning_for_a_different_build_at_the_same_level() {
        let vm = FleetVm::new(
            Some(record("AMD handheld", "aaa", 5, 100)),
            vec![record("Strix Halo tablet", "bbb", 5, 200)],
        );
        assert_eq!(
            vm.warning(),
            Some(
                "Strix Halo tablet made this save with a different build of the game (bbb); \
                 this device has aaa. Update this device first, or play on and keep both saves."
            )
        );
    }

    #[test]
    fn no_warning_when_this_device_synced_last_or_has_the_same_build() {
        let synced_last = FleetVm::new(
            Some(record("AMD handheld", "aaa", 5, 300)),
            vec![record("Strix Halo tablet", "bbb", 9, 200)],
        );
        assert_eq!(synced_last.warning(), None);

        let same_build = FleetVm::new(
            Some(record("AMD handheld", "aaa", 5, 100)),
            vec![record("Strix Halo tablet", "aaa", 5, 200)],
        );
        assert_eq!(same_build.warning(), None);
    }

    #[test]
    fn no_paired_devices_is_the_empty_state_with_this_device_listed() {
        let vm = FleetVm::from_text("AMD handheld\tlinux\tabc\t1\t0\t500000000\n").unwrap();
        assert_eq!(vm.empty_text(), Some(EMPTY));
        assert_eq!(vm.warning(), None);
        assert_eq!(vm.rows().len(), 1);
        assert_eq!(vm.rows()[0].name, "AMD handheld (this device)");
        assert_eq!(vm.rows()[0].detail, "Linux · build abc · 1 fix");
        assert_eq!(vm.rows()[0].free, "500 MB free");
    }

    #[test]
    fn no_records_is_the_empty_state_with_focus_on_back() {
        for vm in [FleetVm::empty(), FleetVm::from_text("").unwrap()] {
            assert!(vm.rows().is_empty());
            assert_eq!(vm.empty_text(), Some(EMPTY));
            assert_eq!(vm.warning(), None);
            assert_eq!(vm.focused(), vm.back_index());
        }
    }

    #[test]
    fn windows_line_endings_and_blank_lines_read() {
        let text = FIXTURE.replace('\n', "\r\n") + "\r\n";
        let vm = FleetVm::from_text(&text).unwrap();
        assert_eq!(vm.rows().len(), 3);
    }

    #[test]
    fn a_bad_line_is_the_core_hub_error() {
        let err = FleetVm::from_text("AMD handheld\tlinux\tabc\n").unwrap_err();
        assert_eq!(err.code(), "hub");
        assert_eq!(
            err,
            uncharted_core::Error::Hub("bad device record: AMD handheld\tlinux\tabc".to_string())
        );
    }

    #[test]
    fn focus_moves_through_rows_then_back_and_wraps() {
        let mut vm = FleetVm::from_text(FIXTURE).unwrap();
        assert_eq!(vm.focused(), 0);
        assert_eq!(vm.back_index(), 3);
        vm.on_nav(Nav::Down);
        vm.on_nav(Nav::Down);
        vm.on_nav(Nav::Down);
        assert_eq!(vm.focused(), vm.back_index());
        vm.on_nav(Nav::Down);
        assert_eq!(vm.focused(), 0);
        vm.on_nav(Nav::Up);
        assert_eq!(vm.focused(), 3);
        vm.reset_focus();
        assert_eq!(vm.focused(), 0);
    }

    #[test]
    fn sync_times_are_utc_dates() {
        assert_eq!(sync_text(1), "synced 1970-01-01 00:00 UTC");
        assert_eq!(sync_text(951_782_400), "synced 2000-02-29 00:00 UTC");
        assert_eq!(sync_text(1_762_958_730), "synced 2025-11-12 14:45 UTC");
        // The largest value a record can hold still formats.
        assert!(sync_text(u64::MAX).starts_with("synced "));
    }

    #[test]
    fn long_build_ids_are_cut_to_twelve_characters() {
        let vm = FleetVm::new(Some(record("Desktop", &"f".repeat(64), 0, 0)), Vec::new());
        assert_eq!(vm.rows()[0].detail, "Linux · build ffffffffffff · 0 fixes");
    }
}
