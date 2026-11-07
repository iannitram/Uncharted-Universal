//! Build bundles (SPEC.md M9, M11, M12; ADR 0004): a prepared install as a list of files with
//! their hashes, which a hub serves and a client checks before it installs. A bundle lists only
//! what is ours or made from ours: the runtime libraries, the settings, the shader translations.
//! What is derived from the game (the program built from the recompiled code, and the game's
//! data folder) is never in a bundle (SPEC.md M9, "Implementation decisions"). Issue #189's
//! separate paired transfer of those files is not implemented by this module.
//! `is_forbidden` is checked for every file, and a test enforces it. New module; the text
//! format and the flow are copied from iannitram/totk-native's `bundle.rs` (MIT).

use crate::error::{Error, Result};
use crate::platform::Os;
use std::path::{Path, PathBuf};

/// What a bundled file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Part of the prepared install.
    Install,
    /// A GPU-independent shader translation (a driver pipeline cache is never bundled).
    ShaderTranslation,
}

/// One file of a build, its path relative to the build folder (with `/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleFile {
    pub path: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    pub kind: Kind,
}

/// A finished build for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub target: Os,
    /// How many of our patches were applied (the per-game hand patches and settings).
    pub patch_level: u32,
    /// Names this exact set of files.
    pub build_id: String,
    /// Sorted by `path`.
    pub files: Vec<BundleFile>,
}

/// Top-level folders of an install that belong to the device, not the build: never bundled, and
/// kept from the old install by `install`.
pub const DEVICE_OWNED: &[&str] = &["saves", "logs", "mods", "user"];

/// True when a path (relative, with `/`) must never leave a hub: a `.pkg`, anything under
/// `DEVICE_OWNED`, a `PARAM.SFO`, `EBOOT.BIN` or `EBOOT.ELF` copied from a copy, any
/// file of the game folder itself, a `.prx`/`.sprx`/`.self`, or game data the runtime reads.
pub fn is_forbidden(rel_path: &str) -> bool {
    // Bundle paths are a portable wire format. Reject ambiguous and escaping paths before any
    // receiver joins them to its staging directory.
    if rel_path.is_empty()
        || rel_path.starts_with('/')
        || rel_path.starts_with('\\')
        || rel_path.contains('\\')
        || rel_path.contains(':')
        || rel_path.chars().any(char::is_control)
    {
        return true;
    }
    let components: Vec<String> = rel_path
        .split('/')
        .map(|part| part.to_ascii_lowercase())
        .collect();
    if components
        .iter()
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return true;
    }
    let name = components.last().map(String::as_str).unwrap_or("");
    let forbidden_suffix = [
        ".pkg", ".iso", ".elf", ".self", ".prx", ".sprx", ".nca", ".keys",
    ];
    if forbidden_suffix.iter().any(|suffix| name.ends_with(suffix)) {
        return true;
    }
    if name == "param.sfo"
        || name.starts_with("eboot.")
        || name == "big2-ps4_shipping.elf"
        || name == "big3-ps4_shipping.elf"
    {
        return true;
    }
    let forbidden_component = [
        "usrdir",
        "game",
        "recompiled",
        "gamedata",
        "sce_sys",
        "sce_module",
        "app0",
        "dvdroot_ps4",
    ];
    if components
        .iter()
        .any(|part| forbidden_component.contains(&part.as_str()))
    {
        return true;
    }
    match components.first() {
        Some(first) => DEVICE_OWNED.iter().any(|owned| first == owned),
        None => false,
    }
}

/// The bundle as text, one item per line, each ending in `\n`: `uncharted-bundle 1`,
/// `target <os key>`, `patches <patch_level>`, `build <build_id>`, then one
/// `file <kind> <size> <sha256> <path>` line per file in order.
pub fn to_text(b: &Bundle) -> String {
    let mut out = String::new();
    out.push_str("uncharted-bundle 1\n");
    out.push_str(&format!("target {}\n", b.target.key()));
    out.push_str(&format!("patches {}\n", b.patch_level));
    out.push_str(&format!("build {}\n", b.build_id));
    for file in &b.files {
        out.push_str(&file_line(file));
    }
    out
}

/// Reads `to_text`'s format back. Any malformed line is `Error::Hub("bad bundle line <n>: <line>")`;
/// a forbidden path is `Error::Hub("refusing to bundle <path>: game files, keys and saves never
/// leave a hub")`.
pub fn from_text(text: &str) -> Result<Bundle> {
    let mut lines = text.lines();
    let header = lines.next().unwrap_or("");
    if header != "uncharted-bundle 1" {
        return Err(bad_line(1, header));
    }

    let target_line = lines.next().unwrap_or("");
    let target = parse_target(target_line).ok_or_else(|| bad_line(2, target_line))?;

    let patches_line = lines.next().unwrap_or("");
    let patch_level = parse_patches(patches_line).ok_or_else(|| bad_line(3, patches_line))?;

    let build_line = lines.next().unwrap_or("");
    let build_id = parse_build(build_line).ok_or_else(|| bad_line(4, build_line))?;

    let mut files = Vec::new();
    for (offset, line) in lines.enumerate() {
        let number = offset + 5;
        let mut fields = line.splitn(5, ' ');
        let tag = fields.next().unwrap_or("");
        let kind = fields.next().unwrap_or("");
        let size = fields.next().unwrap_or("");
        let sha256 = fields.next().unwrap_or("");
        let path = fields.next().unwrap_or("");
        if tag != "file" || path.is_empty() {
            return Err(bad_line(number, line));
        }
        let kind = match kind {
            "install" => Kind::Install,
            "shader" => Kind::ShaderTranslation,
            _ => return Err(bad_line(number, line)),
        };
        let size = size.parse::<u64>().map_err(|_| bad_line(number, line))?;
        if !is_sha256_hex(sha256) {
            return Err(bad_line(number, line));
        }
        if is_forbidden(path) {
            return Err(refusal(path));
        }
        if files.iter().any(|file: &BundleFile| file.path == path) {
            return Err(bad_line(number, line));
        }
        files.push(BundleFile {
            path: path.to_string(),
            size,
            sha256: sha256.to_string(),
            kind,
        });
    }

    Ok(Bundle {
        target,
        patch_level,
        build_id,
        files,
    })
}

/// The bundle of an install folder: every file under `dir` (recursively), skipping
/// `DEVICE_OWNED`; a forbidden file is the `from_text` refusal error. `build_id` is the first 16
/// hex digits of the SHA-256 of all the file lines joined in order.
pub fn manifest(dir: &Path, target: Os, patch_level: u32) -> Result<Bundle> {
    let mut files = Vec::new();
    collect_files(dir, dir, true, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let mut file_text = String::new();
    for file in &files {
        file_text.push_str(&file_line(file));
    }
    let build_id = sha256_hex(file_text.as_bytes())[..16].to_string();

    Ok(Bundle {
        target,
        patch_level,
        build_id,
        files,
    })
}

/// Checks every file of `b` in `dir` for its size and hash; the first that is missing or differs
/// is `Error::Hub("<path> does not match the bundle")`.
pub fn verify(b: &Bundle, dir: &Path) -> Result<()> {
    for file in &b.files {
        if is_forbidden(&file.path) {
            return Err(refusal(&file.path));
        }
        let path = dir.join(&file.path);
        let matches = regular_file_in_tree(dir, &file.path)
            .and_then(|_| std::fs::metadata(&path))
            .map(|meta| meta.len() == file.size)
            .unwrap_or(false)
            && std::fs::read(&path)
                .map(|bytes| sha256_hex(&bytes) == file.sha256)
                .unwrap_or(false);
        if !matches {
            return Err(Error::Hub(format!(
                "{} does not match the bundle",
                file.path
            )));
        }
    }
    Ok(())
}

fn regular_file_in_tree(root: &Path, rel_path: &str) -> std::io::Result<()> {
    let mut current = root.to_path_buf();
    let parts: Vec<&str> = rel_path.split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        let metadata = std::fs::symlink_metadata(&current)?;
        let expected_type = if index + 1 == parts.len() {
            metadata.file_type().is_file()
        } else {
            metadata.file_type().is_dir()
        };
        if !expected_type {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bundle path contains a non-regular file",
            ));
        }
    }
    Ok(())
}

/// Installs a checked build: `verify(b, staged)` first (nothing changes if it fails); then moves
/// each `DEVICE_OWNED` entry the old `install_dir` has into `staged`, swaps the folders, and
/// deletes the old one. Saves are backed up by `update` before this runs (M19).
pub fn install(b: &Bundle, staged: &Path, install_dir: &Path) -> Result<()> {
    match std::fs::symlink_metadata(staged.join(".uu-transfer-incomplete")) {
        Ok(_) => {
            return Err(Error::Hub(
                "game-data transfer is incomplete; this install is not ready".into(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    verify(b, staged)?;

    for name in DEVICE_OWNED {
        let source = install_dir.join(name);
        if !source.exists() {
            continue;
        }
        let destination = staged.join(name);
        if destination.exists() {
            return Err(Error::Io(format!(
                "{} already exists in the staged build",
                destination.display()
            )));
        }
        std::fs::rename(&source, &destination)?;
    }

    let old = with_extension(install_dir, "old");
    std::fs::rename(install_dir, &old)?;
    std::fs::rename(staged, install_dir)?;
    std::fs::remove_dir_all(&old)?;
    Ok(())
}

fn with_extension(path: &Path, extension: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}

fn collect_files(
    root: &Path,
    current: &Path,
    top_level: bool,
    files: &mut Vec<BundleFile>,
) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if top_level
            && DEVICE_OWNED
                .iter()
                .any(|owned| owned.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files(root, &path, false, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let relative = relative.to_string_lossy().replace('\\', "/");
            if is_forbidden(&relative) {
                return Err(refusal(&relative));
            }
            let bytes = std::fs::read(&path)?;
            files.push(BundleFile {
                path: relative,
                size: bytes.len() as u64,
                sha256: sha256_hex(&bytes),
                kind: Kind::Install,
            });
        } else {
            return Err(Error::Hub("refusing to bundle a non-regular file".into()));
        }
    }
    Ok(())
}

fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Install => "install",
        Kind::ShaderTranslation => "shader",
    }
}

fn file_line(file: &BundleFile) -> String {
    format!(
        "file {} {} {} {}\n",
        kind_word(file.kind),
        file.size,
        file.sha256,
        file.path
    )
}

fn bad_line(number: usize, line: &str) -> Error {
    Error::Hub(format!("bad bundle line {number}: {line}"))
}

fn refusal(path: &str) -> Error {
    Error::Hub(format!(
        "refusing to bundle {path}: game files, keys and saves never leave a hub"
    ))
}

fn parse_target(line: &str) -> Option<Os> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "target" {
        return None;
    }
    let key = fields.next()?;
    if fields.next().is_some() {
        return None;
    }
    Os::from_key(key)
}

fn parse_patches(line: &str) -> Option<u32> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "patches" {
        return None;
    }
    let level = fields.next()?.parse().ok()?;
    if fields.next().is_some() {
        return None;
    }
    Some(level)
}

fn parse_build(line: &str) -> Option<String> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "build" {
        return None;
    }
    let id = fields.next()?;
    if fields.next().is_some() {
        return None;
    }
    Some(id.to_string())
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_length = (bytes.len() as u64).wrapping_mul(8);
    let mut data = bytes.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_length.to_be_bytes());

    for chunk in data.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    let mut out = String::with_capacity(64);
    for word in state {
        out.push_str(&format!("{word:08x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("uncharted-bundle-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(dir: &Path, relative: &str, bytes: &[u8]) {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn bundle_with_two_kinds() -> Bundle {
        Bundle {
            target: Os::Windows,
            patch_level: 3,
            build_id: "0123456789abcdef".to_string(),
            files: vec![
                BundleFile {
                    path: "lib/a.so".to_string(),
                    size: 3,
                    sha256: sha256_hex(b"abc"),
                    kind: Kind::Install,
                },
                BundleFile {
                    path: "shaders/s.bin".to_string(),
                    size: 0,
                    sha256: sha256_hex(b""),
                    kind: Kind::ShaderTranslation,
                },
            ],
        }
    }

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn forbidden_paths() {
        assert!(is_forbidden("game.pkg"));
        assert!(is_forbidden("disc.iso"));
        assert!(is_forbidden("EBOOT.BIN"));
        assert!(is_forbidden("PARAM.SFO"));
        assert!(is_forbidden("keys/system.keys"));
        assert!(is_forbidden("app0/data.bin"));
        assert!(is_forbidden("dvdroot_ps4/data.bin"));
        assert!(is_forbidden("x.elf"));
        assert!(is_forbidden("x.self"));
        assert!(is_forbidden("x.prx"));
        assert!(is_forbidden("x.sprx"));
        assert!(is_forbidden("usrdir/x"));
        assert!(is_forbidden("game/x"));
        assert!(is_forbidden("recompiled/x"));
        assert!(is_forbidden("gamedata/x"));
        assert!(is_forbidden("saves/x"));
        assert!(is_forbidden("USER/x"));
        assert!(is_forbidden("../outside"));
        assert!(is_forbidden("lib/../../outside"));
        assert!(is_forbidden("/absolute/path"));
        assert!(is_forbidden("C:/absolute/path"));
        assert!(is_forbidden("sce_sys/param.sfo"));
        assert!(is_forbidden("game/eboot.bin"));
        assert!(!is_forbidden("lib/libx.so"));
    }

    #[test]
    fn text_round_trip() {
        let bundle = bundle_with_two_kinds();
        assert_eq!(from_text(&to_text(&bundle)), Ok(bundle));
    }

    #[test]
    fn from_text_refuses_forbidden_paths() {
        let hash = sha256_hex(b"x");
        let text = format!(
            "uncharted-bundle 1\ntarget linux\npatches 0\nbuild abc\nfile install 1 {hash} game.pkg\n"
        );
        assert_eq!(
            from_text(&text),
            Err(Error::Hub(
                "refusing to bundle game.pkg: game files, keys and saves never leave a hub"
                    .to_string()
            ))
        );
    }

    #[test]
    fn from_text_reports_the_malformed_line_number() {
        let text = "uncharted-bundle 1\ntarget linux\npatches zero\nbuild abc\n";
        assert_eq!(
            from_text(text),
            Err(Error::Hub("bad bundle line 3: patches zero".to_string()))
        );
    }

    #[test]
    fn manifest_skips_device_owned_and_sorts() {
        let dir = temp_dir("manifest");
        write_file(&dir, "lib/a.so", b"aaa");
        write_file(&dir, "shaders/s.bin", b"bbbb");
        write_file(&dir, "saves/keep.sav", b"save");

        let bundle = manifest(&dir, Os::Linux, 2).unwrap();
        let paths: Vec<&str> = bundle.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["lib/a.so", "shaders/s.bin"]);
        assert_eq!(bundle.files[0].size, 3);
        assert_eq!(bundle.files[0].sha256, sha256_hex(b"aaa"));
        assert_eq!(bundle.files[1].size, 4);
        assert_eq!(bundle.files[1].sha256, sha256_hex(b"bbbb"));
        assert_eq!(bundle.files[0].kind, Kind::Install);

        let lines = format!(
            "file install 3 {} lib/a.so\nfile install 4 {} shaders/s.bin\n",
            sha256_hex(b"aaa"),
            sha256_hex(b"bbbb")
        );
        assert_eq!(bundle.build_id, sha256_hex(lines.as_bytes())[..16]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn manifest_refuses_nested_game_data() {
        for (name, relative) in [
            ("sfo", "install/sce_sys/param.sfo"),
            ("executable", "install/eboot.bin"),
            ("game-data", "install/gamedata/data.bin"),
        ] {
            let dir = temp_dir(name);
            write_file(&dir, relative, b"synthetic");
            assert!(matches!(
                manifest(&dir, Os::Linux, 0),
                Err(Error::Hub(message)) if message.contains("refusing to bundle")
            ));
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn verify_checks_size_and_hash() {
        let dir = temp_dir("verify");
        write_file(&dir, "lib/a.so", b"abc");
        write_file(&dir, "shaders/s.bin", b"");
        let bundle = manifest(&dir, Os::Linux, 0).unwrap();
        assert_eq!(verify(&bundle, &dir), Ok(()));

        write_file(&dir, "lib/a.so", b"changed");
        assert_eq!(
            verify(&bundle, &dir),
            Err(Error::Hub("lib/a.so does not match the bundle".to_string()))
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn install_keeps_device_owned_folders() {
        let root = temp_dir("install");
        let staged = root.join("staged");
        let install_dir = root.join("install");
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::create_dir_all(&install_dir).unwrap();
        write_file(&staged, "lib/a.so", b"abc");
        write_file(&install_dir, "saves/keep.sav", b"save");
        write_file(&install_dir, "lib/old.so", b"old");

        let bundle = manifest(&staged, Os::Linux, 0).unwrap();
        install(&bundle, &staged, &install_dir).unwrap();

        assert!(install_dir.join("saves/keep.sav").exists());
        assert_eq!(
            std::fs::read(install_dir.join("saves/keep.sav")).unwrap(),
            b"save"
        );
        assert!(install_dir.join("lib/a.so").exists());
        assert!(!install_dir.join("lib/old.so").exists());
        assert!(!staged.exists());
        assert!(!with_extension(&install_dir, "old").exists());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_failed_verify_leaves_the_install_untouched() {
        let root = temp_dir("install-failure");
        let staged = root.join("staged");
        let install_dir = root.join("install");
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::create_dir_all(&install_dir).unwrap();
        write_file(&staged, "lib/a.so", b"abc");
        write_file(&install_dir, "saves/keep.sav", b"save");
        write_file(&install_dir, "lib/old.so", b"old");

        let bundle = manifest(&staged, Os::Linux, 0).unwrap();
        write_file(&staged, "lib/a.so", b"changed");
        assert_eq!(
            install(&bundle, &staged, &install_dir),
            Err(Error::Hub("lib/a.so does not match the bundle".to_string()))
        );

        assert!(install_dir.join("saves/keep.sav").exists());
        assert!(install_dir.join("lib/old.so").exists());
        assert!(staged.exists());
        assert!(!with_extension(&install_dir, "old").exists());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn install_refuses_a_marked_incomplete_game_data_transfer() {
        let staged = temp_dir("incomplete-transfer");
        let install_dir = temp_dir("incomplete-install");
        let bundle = Bundle {
            target: Os::Linux,
            patch_level: 0,
            build_id: "empty".into(),
            files: Vec::new(),
        };
        write_file(&staged, ".uu-transfer-incomplete", b"incomplete");
        assert!(matches!(
            install(&bundle, &staged, &install_dir),
            Err(Error::Hub(message)) if message.contains("incomplete")
        ));
        assert!(staged.exists());
        std::fs::remove_dir_all(staged).unwrap();
        std::fs::remove_dir_all(install_dir).unwrap();
    }
}
