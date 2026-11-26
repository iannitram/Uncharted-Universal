//! The early check on the player's dump, before any prepare (SPEC.md M11; issue #172 folds in):
//! folder layout, whether it is decrypted, which games and revisions it holds. Only file names,
//! `param.sfo` and each executable's header (a SELF's entry table) are read; nothing is copied,
//! uploaded or logged (M9). Replaces the checks scattered through `run.py` (`locate`, `find_executable`) and `launcher/` start-up code.

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

/// SELF entry flag: the entry holds a segment (`prepare::load_image` skips entries without it).
const SELF_ENTRY_SEGMENT: u64 = 0x800;
/// SELF entry flags: encrypted (0x2) or compressed (0x8), which `prepare::load_image` refuses.
const SELF_ENTRY_ENCRYPTED_OR_COMPRESSED: u64 = 0x2 | 0x8;

/// After the 4-byte magic, read a SELF's entry table (count at offset 24, 32-byte entries from
/// offset 32) and say whether no segment entry is encrypted or compressed. A table cut short
/// counts as not plain.
fn self_is_plain(file: &mut File) -> bool {
    let mut head = [0; 28];
    if file.read_exact(&mut head).is_err() {
        return false;
    }
    // `head` starts at file offset 4, so the u16 entry count at offset 24 is head[20..22].
    let count = u16::from_le_bytes([head[20], head[21]]) as usize;
    let mut table = vec![0; count * 32];
    if file.read_exact(&mut table).is_err() {
        return false;
    }
    table.as_chunks::<32>().0.iter().all(|entry| {
        let flags = u64::from_le_bytes(entry[..8].try_into().expect("eight byte range"));
        flags & SELF_ENTRY_SEGMENT == 0 || flags & SELF_ENTRY_ENCRYPTED_OR_COMPRESSED == 0
    })
}

/// What `check` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpReport {
    /// `TITLE_ID` from `param.sfo` ("CUSA02320"), if readable.
    pub title_id: Option<String>,
    pub games: Vec<FoundGame>,
    /// False when an executable is still an encrypted container (a `.pkg` from PSN, or an
    /// encrypted SELF) instead of a decrypted dump.
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
            // A fake-signed SELF (fSELF) stores every segment plain, and `prepare::load_image`
            // reads it like an ELF; only a SELF with an encrypted or compressed segment fails.
            if !self_is_plain(&mut file) {
                decrypted = false;
                problems.push(format!(
                    "{} is still an encrypted PS4 SELF; use a decrypted ELF dump.",
                    game.executable()
                ));
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

    /// A SELF header with one 32-byte entry per flag value (count at offset 24, table at 32).
    fn tiny_self(entry_flags: &[u64]) -> Vec<u8> {
        let mut data = PS4_SELF_MAGIC.to_vec();
        data.resize(24, 0);
        data.extend_from_slice(&(entry_flags.len() as u16).to_le_bytes());
        data.resize(32, 0);
        for flags in entry_flags {
            data.extend_from_slice(&flags.to_le_bytes());
            data.extend_from_slice(&[0; 24]);
        }
        data
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
            [&PS4_SELF_MAGIC[..], b"encrypted"].concat(),
        )
        .unwrap();
        assert!(check(&self_dir.0)
            .unwrap()
            .problems
            .iter()
            .any(|p| p.contains("encrypted PS4 SELF")));

        let fself_dir = good("fself", "01.00");
        for game in Game::ALL {
            fs::write(
                fself_dir.0.join(game.executable()),
                tiny_self(&[0x800, 0x801, 0x0]),
            )
            .unwrap();
        }
        let report = check(&fself_dir.0).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert!(report.decrypted);

        for (label, flags) in [("enc", 0x802u64), ("cmp", 0x808u64)] {
            let dir = good(label, "01.00");
            fs::write(dir.0.join("eboot.bin"), tiny_self(&[0x800, flags])).unwrap();
            let report = check(&dir.0).unwrap();
            assert!(!report.decrypted, "{label}");
            assert_eq!(
                report.problems,
                ["eboot.bin is still an encrypted PS4 SELF; use a decrypted ELF dump."],
                "{label}"
            );
        }

        let cut = good("cut", "01.00");
        let mut short = tiny_self(&[0x800, 0x800]);
        short.truncate(32 + 40);
        fs::write(cut.0.join("eboot.bin"), short).unwrap();
        assert!(!check(&cut.0).unwrap().decrypted);

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
