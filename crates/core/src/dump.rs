//! The early check on the player's dump, before any prepare (SPEC.md M11; issue #172 folds in):
//! folder layout, whether it is decrypted, which games and revisions it holds. Only file names
//! and `param.sfo` are read; nothing is copied, uploaded or logged (M9). Replaces the checks
//! scattered through `run.py` (`locate`, `find_executable`) and `launcher/` start-up code.

use crate::error::{Error, Result};
use crate::games::Game;
use crate::sfo::{self, APP_VER, TITLE_ID};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

/// One game found in a dump folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundGame {
    pub game: Game,
    /// The folder holding that game's executable (a collection dump has all three in one folder;
    /// a game dumped on its own has its own: `BB_GAME_DIR_U1` .. `_U3` in `run.py`).
    pub folder: PathBuf,
    /// `APP_VER` from `sce_sys/param.sfo`, if readable.
    pub app_ver: Option<String>,
}

/// First four bytes of a PS4 SELF container (an executable that is not a plain ELF).
const PS4_SELF_MAGIC: [u8; 4] = [0x4f, 0x15, 0x3d, 0x1d];
/// SELF entry flags `prepare::load_image` refuses: encrypted (0x2) and deflated (0x8).
const SELF_ENTRY_ENCRYPTED_OR_COMPRESSED: u64 = 0x2 | 0x8;

/// What the SELF entry table says about one executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelfState {
    /// No entry is encrypted or compressed: a fake-signed SELF that prepare loads.
    Plain,
    /// At least one entry is encrypted or compressed.
    Encrypted,
    /// The header or entry table is cut short.
    Unreadable,
}

/// Read the rest of the SELF header (32 bytes in all; entry count at 24) and the entry table
/// after it (32 bytes per entry, flags first), as `prepare::load_image` does. `file` is just
/// past the 4-byte magic. Only the flags are looked at.
fn self_state(file: &mut File) -> std::io::Result<SelfState> {
    fn read_full(file: &mut File, buf: &mut [u8]) -> std::io::Result<bool> {
        match file.read_exact(buf) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
            Err(error) => Err(error),
        }
    }
    let mut header = [0u8; 28];
    if !read_full(file, &mut header)? {
        return Ok(SelfState::Unreadable);
    }
    let count = u16::from_le_bytes([header[20], header[21]]);
    let mut entry = [0u8; 32];
    for _ in 0..count {
        if !read_full(file, &mut entry)? {
            return Ok(SelfState::Unreadable);
        }
        let mut flags = [0u8; 8];
        flags.copy_from_slice(&entry[..8]);
        if u64::from_le_bytes(flags) & SELF_ENTRY_ENCRYPTED_OR_COMPRESSED != 0 {
            return Ok(SelfState::Encrypted);
        }
    }
    Ok(SelfState::Plain)
}

/// What `check` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpReport {
    /// `TITLE_ID` from `param.sfo` ("CUSA02320"), if readable.
    pub title_id: Option<String>,
    pub games: Vec<FoundGame>,
    /// False when an executable is still an encrypted container (a `.pkg` from PSN, or a
    /// SELF with an encrypted or compressed entry) instead of a decrypted dump. A SELF whose
    /// entries are all plain (fake-signed) counts as decrypted: `prepare::load_image` loads it.
    pub decrypted: bool,
    /// Plain sentences for the first-run page, in order; empty when all is well.
    pub problems: Vec<String>,
}

/// Check a dump folder. Every problem, even a folder that is no dump at all (no executable of
/// any game), is one plain sentence in `problems`. A
/// revision the port does not support is listed in `problems` and left to
/// `games::check_revision` to refuse at prepare.
pub fn check(folder: &Path) -> Result<DumpReport> {
    // A dump's collection family must be known before inspecting executable files. Refuse
    // missing or unusable SFO metadata before constructing a report, so callers can fail the
    // preparation gate without guessing which collection this folder belongs to.
    let param = folder.join("sce_sys").join("param.sfo");
    let data = fs::read(&param).map_err(|error| {
        Error::BadDump(
            match error.kind() {
                std::io::ErrorKind::NotFound => "The dump is missing sce_sys/param.sfo.",
                _ => "sce_sys/param.sfo is damaged or unreadable.",
            }
            .into(),
        )
    })?;
    let values = sfo::parse(&data)
        .map_err(|_| Error::BadDump("sce_sys/param.sfo is damaged or unreadable.".into()))?;
    let title_id = sfo::text(&values, TITLE_ID)
        .ok_or_else(|| Error::BadDump("param.sfo is missing the CUSA title ID.".into()))?
        .to_owned();
    if !crate::games::is_collection_title_id(&title_id) {
        return Err(Error::BadDump(
            "param.sfo has an unrecognized CUSA title ID.".into(),
        ));
    }
    let app_ver = sfo::text(&values, APP_VER).map(str::to_owned);

    let mut problems = Vec::new();
    let mut games = Vec::new();
    let mut decrypted = true;
    for game in Game::ALL {
        let path = folder.join(game.executable());
        if !path.is_file() {
            problems.push(format!("The dump is missing {}.", game.executable()));
            continue;
        }
        let mut header = [0; 4];
        let mut file = File::open(&path)?;
        let count = file.read(&mut header)?;
        if count == 4 && header == PS4_SELF_MAGIC {
            match self_state(&mut file)? {
                SelfState::Plain => {}
                SelfState::Encrypted => {
                    decrypted = false;
                    problems.push(format!(
                        "{} is still an encrypted PS4 SELF; use a decrypted dump.",
                        game.executable()
                    ));
                }
                SelfState::Unreadable => {
                    decrypted = false;
                    problems.push(format!(
                        "{} is a PS4 SELF whose header is cut short; dump it again.",
                        game.executable()
                    ));
                }
            }
        } else if count != 4 || &header != b"\x7fELF" {
            decrypted = false;
            problems.push(format!("{} is not a decrypted ELF.", game.executable()));
        }
        games.push(FoundGame {
            game,
            folder: folder.to_path_buf(),
            app_ver: None,
        });
    }
    if let Some(ver) = app_ver.as_deref() {
        if ver == "01.02" || ver == "1.02" {
            problems.push("This dump is version 1.02; the port expects version 1.00 and does not support the merged update.".into());
        } else if ver != "01.00" && ver != "1.00" {
            problems.push(format!(
                "This dump is version {ver}; the port expects version 1.00."
            ));
        }
    } else {
        problems.push("param.sfo is missing APP_VER.".into());
    }
    for name in ["sce_sys", "u1data", "u2data", "u3data"] {
        if !folder.join(name).is_dir() {
            problems.push(format!("The dump is missing the {name} folder."));
        }
    }
    for found in &mut games {
        found.app_ver = app_ver.clone();
    }
    Ok(DumpReport {
        title_id: Some(title_id),
        decrypted,
        games,
        problems,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(label: &str) -> Self {
            let nonce = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("uu-dump-{}-{label}-{nonce}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn put_sfo(dir: &Path, version: &str) {
        put_sfo_with_title(dir, "CUSA02320", version);
    }

    fn put_sfo_with_title(dir: &Path, title_id: &str, version: &str) {
        let pairs = [("TITLE_ID", title_id), ("APP_VER", version)];
        let mut keys = Vec::new();
        let mut vals = Vec::new();
        let mut entries = Vec::new();
        for (key, value) in pairs {
            let ko = keys.len() as u16;
            keys.extend_from_slice(key.as_bytes());
            keys.push(0);
            let vo = vals.len() as u32;
            vals.extend_from_slice(value.as_bytes());
            vals.push(0);
            entries.extend_from_slice(&ko.to_le_bytes());
            entries.extend_from_slice(&0x0204u16.to_le_bytes());
            entries.extend_from_slice(&((value.len() + 1) as u32).to_le_bytes());
            entries.extend_from_slice(&((value.len() + 1) as u32).to_le_bytes());
            entries.extend_from_slice(&vo.to_le_bytes());
        }
        let key_base = 20 + entries.len();
        let value_base = key_base + keys.len();
        let mut data = Vec::new();
        data.extend_from_slice(b"\0PSF");
        data.extend_from_slice(&0x00000101u32.to_le_bytes());
        data.extend_from_slice(&(key_base as u32).to_le_bytes());
        data.extend_from_slice(&(value_base as u32).to_le_bytes());
        data.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
        data.extend(entries);
        data.extend(keys);
        data.extend(vals);
        fs::write(dir.join("sce_sys/param.sfo"), data).unwrap();
    }

    fn good(label: &str, version: &str) -> TempDir {
        let dir = TempDir::new(label);
        fs::create_dir(dir.0.join("sce_sys")).unwrap();
        for name in ["u1data", "u2data", "u3data"] {
            fs::create_dir(dir.0.join(name)).unwrap();
        }
        for game in Game::ALL {
            fs::write(dir.0.join(game.executable()), b"\x7fELF\x02fake header").unwrap();
        }
        put_sfo(&dir.0, version);
        dir
    }

    /// A SELF header (magic, entry count at 24) and one 32-byte entry per flag value.
    fn self_with_flags(flags: &[u64]) -> Vec<u8> {
        let mut data = vec![0u8; 32];
        data[..4].copy_from_slice(&PS4_SELF_MAGIC);
        data[24..26].copy_from_slice(&(flags.len() as u16).to_le_bytes());
        for flag in flags {
            let mut entry = [0u8; 32];
            entry[..8].copy_from_slice(&flag.to_le_bytes());
            data.extend_from_slice(&entry);
        }
        data
    }

    #[test]
    fn a_self_with_no_encrypted_or_compressed_entry_counts_as_decrypted() {
        let dir = good("plain-self", "01.00");
        // Ten entries: signed (0x4) segment entries (0x800) and plain ones, none encrypted
        // (0x2) or deflated (0x8), as a fake-signed dump carries.
        let flags = [0x804, 0x1, 0x804, 0x1, 0x804, 0x1, 0x804, 0x1, 0x4, 0x0];
        for game in Game::ALL {
            fs::write(dir.0.join(game.executable()), self_with_flags(&flags)).unwrap();
        }
        let report = check(&dir.0).unwrap();
        assert!(report.decrypted);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }

    #[test]
    fn a_compressed_or_cut_short_self_is_a_problem() {
        let compressed = good("compressed-self", "01.00");
        fs::write(compressed.0.join("eboot.bin"), self_with_flags(&[0x808])).unwrap();
        let report = check(&compressed.0).unwrap();
        assert!(!report.decrypted);
        assert!(report
            .problems
            .iter()
            .any(|p| p == "eboot.bin is still an encrypted PS4 SELF; use a decrypted dump."));

        let short = good("short-self", "01.00");
        let mut data = self_with_flags(&[0x800, 0x800]);
        data.truncate(32 + 40);
        fs::write(short.0.join("eboot.bin"), data).unwrap();
        let report = check(&short.0).unwrap();
        assert!(!report.decrypted);
        assert!(report
            .problems
            .iter()
            .any(|p| p == "eboot.bin is a PS4 SELF whose header is cut short; dump it again."));

        let header_only = good("header-only-self", "01.00");
        fs::write(
            header_only.0.join("eboot.bin"),
            [&PS4_SELF_MAGIC[..], b"short"].concat(),
        )
        .unwrap();
        assert!(!check(&header_only.0).unwrap().decrypted);
    }

    #[test]
    fn accepts_a_complete_decrypted_dump() {
        let dir = good("good", "01.00");
        let report = check(&dir.0).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert!(report.decrypted);
        assert_eq!(report.title_id.as_deref(), Some("CUSA02320"));
        assert_eq!(report.games.len(), 3);
    }

    #[test]
    fn identifies_self_version_missing_data_and_wrong_folder() {
        let self_dir = good("self", "01.00");
        fs::write(
            self_dir.0.join("eboot.bin"),
            self_with_flags(&[0x800, 0x802]),
        )
        .unwrap();
        let report = check(&self_dir.0).unwrap();
        assert!(!report.decrypted);
        assert!(report
            .problems
            .iter()
            .any(|p| p == "eboot.bin is still an encrypted PS4 SELF; use a decrypted dump."));

        let version_dir = good("version", "01.02");
        assert!(check(&version_dir.0)
            .unwrap()
            .problems
            .iter()
            .any(|p| p.contains("version 1.02")));

        let missing = good("missing", "01.00");
        fs::remove_dir_all(missing.0.join("u3data")).unwrap();
        assert!(check(&missing.0)
            .unwrap()
            .problems
            .iter()
            .any(|p| p == "The dump is missing the u3data folder."));

        let wrong = TempDir::new("wrong");
        fs::create_dir(wrong.0.join("sce_sys")).unwrap();
        for name in ["u1data", "u2data", "u3data"] {
            fs::create_dir(wrong.0.join(name)).unwrap();
        }
        put_sfo(&wrong.0, "01.00");
        let report = check(&wrong.0).unwrap();
        assert!(report
            .problems
            .iter()
            .any(|p| p == "The dump is missing eboot.bin."));
    }

    #[test]
    fn refuses_missing_unreadable_and_unknown_title_metadata_before_reporting() {
        let missing = good("missing-sfo", "01.00");
        fs::remove_file(missing.0.join("sce_sys/param.sfo")).unwrap();
        assert_eq!(
            check(&missing.0),
            Err(crate::error::Error::BadDump(
                "The dump is missing sce_sys/param.sfo.".into()
            ))
        );

        let unreadable = good("unreadable-sfo", "01.00");
        fs::write(unreadable.0.join("sce_sys/param.sfo"), b"not an SFO").unwrap();
        assert_eq!(
            check(&unreadable.0),
            Err(crate::error::Error::BadDump(
                "sce_sys/param.sfo is damaged or unreadable.".into()
            ))
        );

        let unknown = good("unknown-title", "01.00");
        put_sfo_with_title(&unknown.0, "CUSA00000", "01.00");
        assert_eq!(
            check(&unknown.0),
            Err(crate::error::Error::BadDump(
                "param.sfo has an unrecognized CUSA title ID.".into()
            ))
        );
    }
}
