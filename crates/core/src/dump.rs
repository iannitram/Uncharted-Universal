//! The early check on the player's dump, before any prepare (SPEC.md M11; issue #172 folds in):
//! folder layout, whether it is decrypted, which games and revisions it holds. Only file names
//! and `param.sfo` are read; nothing is copied, uploaded or logged (M9). Replaces the checks
//! scattered through `run.py` (`locate`, `find_executable`) and `launcher/` start-up code.

use crate::error::Result;
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

/// Check a dump folder. A folder with no executable of any game is `Error::BadDump(..)`; a
/// revision the port does not support is listed in `problems` and left to
/// `games::check_revision` to refuse at prepare.
pub fn check(folder: &Path) -> Result<DumpReport> {
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
        let count = File::open(&path)?.read(&mut header)?;
        if count == 4 && &header == b"SCE\0" {
            decrypted = false;
            problems.push(format!(
                "{} is still an encrypted PS4 SELF; use a decrypted ELF dump.",
                game.executable()
            ));
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
    let param = folder.join("sce_sys").join("param.sfo");
    let mut title_id = None;
    let mut app_ver = None;
    if param.is_file() {
        match fs::read(&param)
            .map_err(crate::error::Error::from)
            .and_then(|data| sfo::parse(&data))
        {
            Ok(values) => {
                title_id = sfo::text(&values, TITLE_ID).map(str::to_owned);
                app_ver = sfo::text(&values, APP_VER).map(str::to_owned);
                if let Some(id) = title_id.as_deref() {
                    if !crate::games::is_collection_title_id(id) {
                        problems.push("param.sfo has an unrecognized CUSA title ID.".into());
                    }
                } else {
                    problems.push("param.sfo is missing the CUSA title ID.".into());
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
            }
            Err(_) => problems.push("sce_sys/param.sfo is damaged or unreadable.".into()),
        }
    } else {
        problems.push("The dump is missing sce_sys/param.sfo.".into());
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
        title_id,
        decrypted,
        games,
        problems,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
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
        let pairs = [("TITLE_ID", "CUSA02320"), ("APP_VER", version)];
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
        fs::write(self_dir.0.join("eboot.bin"), b"SCE\0encrypted").unwrap();
        assert!(check(&self_dir.0)
            .unwrap()
            .problems
            .iter()
            .any(|p| p.contains("encrypted PS4 SELF")));

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
        let report = check(&wrong.0).unwrap();
        assert!(report
            .problems
            .iter()
            .any(|p| p == "The dump is missing eboot.bin."));
    }
}
