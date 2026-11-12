//! Test fixtures: temporary folders and a synthetic dump (a `param.sfo` and three tiny fake
//! ELF headers; never a byte of a real game).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use uncharted_core::games::Game;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A folder under the system temp folder, removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("uu-cli-{}-{label}-{nonce}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A `param.sfo` holding `TITLE_ID` and `APP_VER` as UTF-8 strings.
pub fn sfo(title_id: &str, app_ver: &str) -> Vec<u8> {
    let pairs = [("TITLE_ID", title_id), ("APP_VER", app_ver)];
    let (mut keys, mut values, mut entries) = (Vec::new(), Vec::new(), Vec::new());
    for (key, value) in pairs {
        let key_offset = keys.len() as u16;
        keys.extend_from_slice(key.as_bytes());
        keys.push(0);
        let value_offset = values.len() as u32;
        values.extend_from_slice(value.as_bytes());
        values.push(0);
        let len = (value.len() + 1) as u32;
        entries.extend_from_slice(&key_offset.to_le_bytes());
        entries.extend_from_slice(&0x0204u16.to_le_bytes());
        entries.extend_from_slice(&len.to_le_bytes());
        entries.extend_from_slice(&len.to_le_bytes());
        entries.extend_from_slice(&value_offset.to_le_bytes());
    }
    let key_base = 20 + entries.len();
    let value_base = key_base + keys.len();
    let mut data = b"\0PSF".to_vec();
    data.extend_from_slice(&0x0000_0101u32.to_le_bytes());
    data.extend_from_slice(&(key_base as u32).to_le_bytes());
    data.extend_from_slice(&(value_base as u32).to_le_bytes());
    data.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
    data.extend(entries);
    data.extend(keys);
    data.extend(values);
    data
}

/// A complete, decrypted-looking collection dump at version `app_ver`.
pub fn dump(label: &str, app_ver: &str) -> TempDir {
    let dir = TempDir::new(label);
    for name in ["sce_sys", "u1data", "u2data", "u3data"] {
        fs::create_dir(dir.0.join(name)).unwrap();
    }
    for game in Game::ALL {
        fs::write(dir.0.join(game.executable()), b"\x7fELF\x02fake header").unwrap();
    }
    fs::write(dir.0.join("sce_sys/param.sfo"), sfo("CUSA02320", app_ver)).unwrap();
    dir
}
