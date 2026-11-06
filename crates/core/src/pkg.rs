//! The `.pkg` extractor: entry table, bounded copies, overlap refusal. Unpacks the container
//! only; a package from PSN or a disc is encrypted (drm type 0x0F), and decrypting it needs
//! console keys, which are out of scope and never in this crate (SPEC.md M9). Encrypted entries
//! are counted and skipped; delta patches are refused rather than half-applied. Everything is
//! big-endian.

use crate::error::{Error, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAGIC: [u8; 4] = *b"\x7fCNT";
pub const HEADER_SIZE: u64 = 0x1000;
pub const ENTRY_SIZE: u64 = 0x20;
pub const ENTRY_NAMES_ID: u32 = 0x200;
pub const STRUCTURAL_ID_MAX: u32 = 0x1000;
pub const CHUNK_SIZE: usize = 1 << 20;
pub const FLAG_ENCRYPTED: u32 = 0x8000_0000;
pub const CONTENT_TYPE_DELTA_PATCH: u32 = 0x1E;
const CONTENT_FLAG_DELTA_PATCH: u32 = 0x4100_0000;
const MAX_ENTRY_COUNT: u32 = 1 << 20;
const MAX_NAMES_SIZE: u64 = 256 << 20;
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

/// Random access to a package, by seeking: a retail package is tens of gigabytes.
pub trait Source {
    fn size(&self) -> u64;
    /// Fill `buf` from `offset`; reading past the end is `Error::Pkg("package is cut short")`.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub content_id: String,
    pub content_type: u32,
    pub content_flags: u32,
    pub drm_type: u32,
    pub entry_count: u32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    pub name: Option<String>,
    pub flags: u32,
    pub offset: u64,
    pub size: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub header: Header,
    pub entries: Vec<Entry>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub extracted: u32,
    pub skipped_encrypted: u32,
}

fn pkg(message: impl Into<String>) -> Error {
    Error::Pkg(message.into())
}
fn read(source: &dyn Source, offset: u64, buf: &mut [u8]) -> Result<()> {
    let len = u64::try_from(buf.len()).map_err(|_| pkg("read size overflow"))?;
    let end = offset
        .checked_add(len)
        .ok_or_else(|| pkg("package offset overflow"))?;
    if end > source.size() {
        return Err(pkg("package is cut short"));
    }
    source.read_at(offset, buf).map_err(|e| match e {
        Error::Pkg(_) => e,
        _ => pkg("package read failed"),
    })
}
fn be32(bytes: &[u8], at: usize) -> Result<u32> {
    let slice = bytes
        .get(
            at..at
                .checked_add(4)
                .ok_or_else(|| pkg("header offset overflow"))?,
        )
        .ok_or_else(|| pkg("truncated package header"))?;
    Ok(u32::from_be_bytes(
        slice
            .try_into()
            .map_err(|_| pkg("truncated package header"))?,
    ))
}

/// Read the header and entry table, refusing malformed bounds and overlapping bodies.
pub fn analyse(source: &dyn Source) -> Result<Info> {
    if source.size() < HEADER_SIZE {
        return Err(pkg("package is cut short"));
    }
    let mut header_bytes = vec![0; HEADER_SIZE as usize];
    read(source, 0, &mut header_bytes)?;
    if header_bytes[..4] != MAGIC {
        return Err(pkg("not a PS4 package (bad magic)"));
    }
    let entry_count = be32(&header_bytes, 0x10)?;
    if entry_count == 0 || entry_count > MAX_ENTRY_COUNT {
        return Err(pkg("unreasonable package entry count"));
    }
    let table_offset = u64::from(be32(&header_bytes, 0x18)?);
    let table_size = u64::from(entry_count)
        .checked_mul(ENTRY_SIZE)
        .ok_or_else(|| pkg("entry table size overflow"))?;
    let table_end = table_offset
        .checked_add(table_size)
        .ok_or_else(|| pkg("entry table offset overflow"))?;
    if table_offset < HEADER_SIZE || table_end > source.size() {
        return Err(pkg("entry table is outside package"));
    }
    let table_len = usize::try_from(table_size).map_err(|_| pkg("entry table too large"))?;
    let mut table = Vec::new();
    table
        .try_reserve_exact(table_len)
        .map_err(|_| pkg("entry table allocation failed"))?;
    table.resize(table_len, 0);
    read(source, table_offset, &mut table)?;
    let mut raw = Vec::with_capacity(entry_count as usize);
    for record in table.as_chunks::<{ ENTRY_SIZE as usize }>().0 {
        let id = be32(record, 0)?;
        let name_offset = be32(record, 4)?;
        let flags = be32(record, 8)?;
        let offset = u64::from(be32(record, 16)?);
        let size = u64::from(be32(record, 20)?);
        let end = offset
            .checked_add(size)
            .ok_or_else(|| pkg("entry range overflow"))?;
        if end > source.size() {
            return Err(pkg("entry is outside package"));
        }
        if size != 0 && (offset < HEADER_SIZE || (offset < table_end && end > table_offset)) {
            return Err(pkg("entry overlaps package header or entry table"));
        }
        raw.push((id, name_offset, flags, offset, size));
    }
    let names = raw.iter().find(|e| e.0 == ENTRY_NAMES_ID);
    let mut names_blob = Vec::new();
    if let Some((_, _, _, offset, size)) = names {
        if *size > MAX_NAMES_SIZE {
            return Err(pkg("package names table is too large"));
        }
        let names_len = usize::try_from(*size).map_err(|_| pkg("names table too large"))?;
        names_blob
            .try_reserve_exact(names_len)
            .map_err(|_| pkg("names table allocation failed"))?;
        names_blob.resize(names_len, 0);
        read(source, *offset, &mut names_blob)?;
    }
    let entries: Vec<Entry> = raw
        .into_iter()
        .map(|(id, name_offset, flags, offset, size)| {
            let start = name_offset as usize;
            let name = if start < names_blob.len() {
                let tail = &names_blob[start..];
                let end = tail
                    .iter()
                    .position(|b| *b == 0)
                    .ok_or_else(|| pkg("unterminated package entry name"))?;
                if end == 0 {
                    None
                } else {
                    Some(
                        std::str::from_utf8(&tail[..end])
                            .map_err(|_| pkg("invalid UTF-8 package entry name"))?
                            .to_owned(),
                    )
                }
            } else {
                None
            };
            Ok(Entry {
                id,
                name,
                flags,
                offset,
                size,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut spans = Vec::new();
    for entry in entries.iter().filter(|entry| entry.size != 0) {
        let end = entry
            .offset
            .checked_add(entry.size)
            .ok_or_else(|| pkg("entry range overflow"))?;
        spans.push((entry.offset, end));
    }
    spans.sort_unstable();
    if spans.windows(2).any(|w| w[1].0 < w[0].1) {
        return Err(pkg("package entries overlap"));
    }
    let header = Header {
        content_id: String::from_utf8_lossy(
            header_bytes[0x40..0x64]
                .split(|b| *b == 0)
                .next()
                .unwrap_or(&[]),
        )
        .into_owned(),
        content_type: be32(&header_bytes, 0x74)?,
        content_flags: be32(&header_bytes, 0x78)?,
        drm_type: be32(&header_bytes, 0x70)?,
        entry_count,
    };
    if header.content_type == CONTENT_TYPE_DELTA_PATCH
        || header.content_flags & CONTENT_FLAG_DELTA_PATCH == CONTENT_FLAG_DELTA_PATCH
    {
        return Err(pkg("delta patch packages are not supported"));
    }
    Ok(Info { header, entries })
}

/// A package path made safe to join under an output folder.
pub fn safe_relative(name: &str) -> Result<String> {
    if name.is_empty() {
        return Err(pkg("empty package path"));
    }
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') || normalized.starts_with("//") {
        return Err(pkg("absolute package path"));
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() {
            return Err(pkg("empty package path component"));
        }
        if part == "." {
            continue;
        }
        if part == ".." || part.contains(':') || part.contains('\0') || part.ends_with(['.', ' ']) {
            return Err(pkg("unsafe package path"));
        }
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(pkg("reserved Windows package path"));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(pkg("empty package path"));
    }
    Ok(parts.join("/"))
}

fn ensure_directory(path: &Path) -> Result<()> {
    if path.exists() {
        let meta =
            fs::symlink_metadata(path).map_err(|_| pkg("cannot inspect extraction directory"))?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(pkg("unsafe extraction directory"));
        }
    } else {
        fs::create_dir(path).map_err(|_| pkg("cannot create extraction directory"))?;
    }
    Ok(())
}
fn safe_target(root: &Path, relative: &str) -> Result<PathBuf> {
    let mut current = root.to_path_buf();
    let parts: Vec<_> = Path::new(relative).components().collect();
    for (i, component) in parts.iter().enumerate() {
        let Component::Normal(part) = component else {
            return Err(pkg("unsafe extraction path"));
        };
        current.push(part);
        if i + 1 < parts.len() {
            ensure_directory(&current)?;
        }
    }
    if let Ok(meta) = fs::symlink_metadata(&current) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(pkg("extraction target is not a regular file"));
        }
    }
    Ok(current)
}

fn validate_target(root: &Path, relative: &str) -> Result<()> {
    let mut current = root.to_path_buf();
    let parts: Vec<_> = Path::new(relative).components().collect();
    for (i, component) in parts.iter().enumerate() {
        let Component::Normal(part) = component else {
            return Err(pkg("unsafe extraction path"));
        };
        current.push(part);
        if let Ok(meta) = fs::symlink_metadata(&current) {
            if meta.file_type().is_symlink() {
                return Err(pkg("symlink in extraction path"));
            }
            if i + 1 < parts.len() && !meta.is_dir() {
                return Err(pkg("file blocks extraction directory"));
            }
            if i + 1 == parts.len() && !meta.is_file() {
                return Err(pkg("extraction target is not a regular file"));
            }
        }
    }
    Ok(())
}

fn create_staging_file(parent: &Path) -> Result<(PathBuf, File)> {
    for _ in 0..128 {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".pkg-extract-{}-{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(pkg("cannot create staged extraction file")),
        }
    }
    Err(pkg("cannot allocate a unique staged extraction file"))
}

/// Extract every unencrypted named game file under `out`.
pub fn extract(source: &dyn Source, out: &Path) -> Result<Report> {
    let info = analyse(source)?;
    let mut report = Report::default();
    let mut destinations = Vec::new();
    for entry in &info.entries {
        if entry.id < STRUCTURAL_ID_MAX {
            continue;
        }
        let Some(name) = entry.name.as_deref() else {
            continue;
        };
        let Ok(relative) = safe_relative(name) else {
            continue;
        };
        if entry.flags & FLAG_ENCRYPTED != 0 {
            report.skipped_encrypted = report
                .skipped_encrypted
                .checked_add(1)
                .ok_or_else(|| pkg("encrypted entry count overflow"))?;
            continue;
        }
        // Existing symlinks are rejected even for nested components; components created above
        // are checked again by safe_target when a later entry traverses the same path.
        destinations.push((entry, relative));
    }
    if destinations.is_empty() {
        return Ok(report);
    }
    let mut paths: Vec<String> = destinations
        .iter()
        .map(|(_, path)| path.to_lowercase())
        .collect();
    paths.sort_unstable();
    if paths.windows(2).any(|pair| {
        pair[1] == pair[0]
            || (pair[1].starts_with(&pair[0])
                && pair[1].as_bytes().get(pair[0].len()) == Some(&b'/'))
    }) {
        return Err(pkg(
            "package paths collide on a case-insensitive filesystem",
        ));
    }
    if out.exists() {
        let meta = fs::symlink_metadata(out).map_err(|_| pkg("cannot inspect output folder"))?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(pkg("unsafe output folder"));
        }
    } else {
        fs::create_dir_all(out).map_err(|_| pkg("cannot create output folder"))?;
    }
    let root = fs::canonicalize(out).map_err(|_| pkg("cannot resolve output folder"))?;
    for (_, relative) in &destinations {
        validate_target(&root, relative)?;
    }
    for (entry, relative) in destinations {
        let target = safe_target(&root, &relative)?;
        let parent = target
            .parent()
            .ok_or_else(|| pkg("extraction target has no parent"))?;
        let (staged_path, mut file) = create_staging_file(parent)?;
        let copied = (|| -> Result<()> {
            let mut done = 0u64;
            let mut chunk = vec![0u8; CHUNK_SIZE];
            while done < entry.size {
                let count = usize::try_from((entry.size - done).min(CHUNK_SIZE as u64))
                    .map_err(|_| pkg("entry chunk size overflow"))?;
                let offset = entry
                    .offset
                    .checked_add(done)
                    .ok_or_else(|| pkg("entry offset overflow"))?;
                read(source, offset, &mut chunk[..count])?;
                file.write_all(&chunk[..count])
                    .map_err(|_| pkg("cannot write staged extraction file"))?;
                done = done
                    .checked_add(count as u64)
                    .ok_or_else(|| pkg("entry copy size overflow"))?;
            }
            file.flush()
                .map_err(|_| pkg("cannot finish staged extraction file"))?;
            Ok(())
        })();
        drop(file);
        if let Err(error) = copied {
            let _ = fs::remove_file(&staged_path);
            return Err(error);
        }
        if fs::rename(&staged_path, &target).is_err() {
            let _ = fs::remove_file(&staged_path);
            return Err(pkg("cannot replace extraction target"));
        }
        report.extracted = report
            .extracted
            .checked_add(1)
            .ok_or_else(|| pkg("extracted entry count overflow"))?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Bytes(Vec<u8>);
    impl Source for Bytes {
        fn size(&self) -> u64 {
            self.0.len() as u64
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
            let start = usize::try_from(offset).map_err(|_| pkg("test offset"))?;
            let end = start
                .checked_add(buf.len())
                .ok_or_else(|| pkg("test range"))?;
            let data = self
                .0
                .get(start..end)
                .ok_or_else(|| pkg("package is cut short"))?;
            buf.copy_from_slice(data);
            Ok(())
        }
    }
    fn put32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    fn fixture(files: &[(&str, &[u8], u32)], content_type: u32) -> Bytes {
        let mut names = Vec::new();
        let mut name_offsets = Vec::new();
        for (name, _, _) in files {
            name_offsets.push(names.len() as u32);
            names.extend_from_slice(name.as_bytes());
            names.push(0);
        }
        let table_offset = HEADER_SIZE as usize + names.len();
        let count = files.len() + 1;
        let bodies_start = table_offset + count * ENTRY_SIZE as usize;
        let mut bytes = vec![0; bodies_start];
        bytes[..4].copy_from_slice(&MAGIC);
        put32(&mut bytes, 0x10, count as u32);
        put32(&mut bytes, 0x18, table_offset as u32);
        put32(&mut bytes, 0x70, 0);
        put32(&mut bytes, 0x74, content_type);
        put32(&mut bytes, 0x78, 0);
        bytes[0x40..0x4b].copy_from_slice(b"EP9000-test");
        bytes[HEADER_SIZE as usize..table_offset].copy_from_slice(&names);
        let mut records = Vec::new();
        records.push((
            ENTRY_NAMES_ID,
            0,
            0,
            HEADER_SIZE as u32,
            names.len() as u32,
            &[][..],
        ));
        for (i, (_, data, flags)) in files.iter().enumerate() {
            let offset = bytes.len() as u32;
            bytes.extend_from_slice(data);
            records.push((
                0x1000,
                name_offsets[i],
                *flags,
                offset,
                data.len() as u32,
                *data,
            ));
        }
        for (i, (id, name_offset, flags, offset, size, _)) in records.iter().enumerate() {
            let at = table_offset + i * ENTRY_SIZE as usize;
            put32(&mut bytes, at, *id);
            put32(&mut bytes, at + 4, *name_offset);
            put32(&mut bytes, at + 8, *flags);
            put32(&mut bytes, at + 16, *offset);
            put32(&mut bytes, at + 20, *size);
        }
        let _ = bodies_start;
        Bytes(bytes)
    }
    fn temp_path() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("uncharted-pkg-{}-{n}", std::process::id()))
    }

    #[test]
    fn analyses_and_extracts_bounded_plain_files() {
        let src = fixture(
            &[
                ("sce_sys/param.sfo", b"metadata", 0),
                ("eboot.bin", b"executable", 0),
            ],
            0x1a,
        );
        let info = analyse(&src).unwrap();
        assert_eq!(info.header.content_id, "EP9000-test");
        assert_eq!(info.entries.len(), 3);
        let out = temp_path();
        let report = extract(&src, &out).unwrap();
        assert_eq!(report.extracted, 2);
        assert_eq!(fs::read(out.join("eboot.bin")).unwrap(), b"executable");
        fs::remove_dir_all(out).unwrap();
    }
    #[test]
    fn refuses_truncation_and_overlapping_entries() {
        let src = fixture(&[("a", b"1234", 0), ("b", b"5678", 0)], 0x1a);
        let mut cut = src.0.clone();
        cut.truncate(cut.len() - 1);
        assert!(analyse(&Bytes(cut)).is_err());
        let mut overlap = src.0.clone();
        let table = be32(&overlap, 0x18).unwrap() as usize;
        let a = be32(&overlap, table + ENTRY_SIZE as usize + 16).unwrap();
        put32(&mut overlap, table + 2 * ENTRY_SIZE as usize + 16, a);
        assert!(analyse(&Bytes(overlap)).is_err());
    }
    #[test]
    fn rejects_delta_and_unsafe_paths_and_skips_encrypted_entries() {
        assert!(analyse(&fixture(&[("a", b"x", 0)], CONTENT_TYPE_DELTA_PATCH)).is_err());
        for path in ["../x", "/x", "C:\\x", "a//b", "a/../b"] {
            assert!(safe_relative(path).is_err(), "{path}");
        }
        let src = fixture(&[("secret", b"ciphertext", FLAG_ENCRYPTED)], 0x1a);
        let out = temp_path();
        let report = extract(&src, &out).unwrap();
        assert_eq!(report.skipped_encrypted, 1);
        assert_eq!(report.extracted, 0);
        assert!(!out.exists());
    }

    #[test]
    fn malformed_and_truncated_names_fail_closed() {
        let src = fixture(&[("a", b"x", 0)], 0x1a);
        let table = be32(&src.0, 0x18).unwrap() as usize;
        let names_offset = be32(&src.0, table + 16).unwrap() as usize;
        let mut image = src.0.clone();
        image[names_offset] = 0xff;
        assert!(analyse(&Bytes(image)).is_err());
        let mut image = src.0.clone();
        image[names_offset + 1] = b'x';
        assert!(analyse(&Bytes(image)).is_err());
    }
    #[test]
    fn rejects_paths_that_alias_after_normalization() {
        let src = fixture(&[("a\\b", b"x", 0), ("a/b", b"y", 0)], 0x1a);
        let out = temp_path();
        assert!(extract(&src, &out).is_err());
        assert!(!out.exists());
    }

    #[test]
    fn extracts_empty_files_and_replaces_older_nonempty_contents() {
        let out = temp_path();
        let fresh = fixture(&[("fresh.bin", b"", 0)], 0x1a);
        let report = extract(&fresh, &out).unwrap();
        assert_eq!(report.extracted, 1);
        assert_eq!(fs::metadata(out.join("fresh.bin")).unwrap().len(), 0);
        let overlay = fixture(&[("stale.bin", b"older bytes", 0)], 0x1a);
        extract(&overlay, &out).unwrap();
        let replacement = fixture(&[("stale.bin", b"", 0)], 0x1a);
        assert_eq!(extract(&replacement, &out).unwrap().extracted, 1);
        assert_eq!(fs::metadata(out.join("stale.bin")).unwrap().len(), 0);
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn rejects_bad_header_counts_and_table_bounds() {
        let src = fixture(&[("a", b"x", 0)], 0x1a);
        for count in [0, MAX_ENTRY_COUNT + 1] {
            let mut image = src.0.clone();
            put32(&mut image, 0x10, count);
            assert!(analyse(&Bytes(image)).is_err());
        }
        let mut image = src.0.clone();
        put32(&mut image, 0x18, u32::MAX);
        assert!(analyse(&Bytes(image)).is_err());
        let mut image = src.0.clone();
        let table = be32(&image, 0x18).unwrap() as usize;
        put32(&mut image, table + ENTRY_SIZE as usize + 20, u32::MAX);
        assert!(analyse(&Bytes(image)).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_extraction_directories() {
        use std::os::unix::fs::symlink;
        let src = fixture(&[("nested/file.bin", b"payload", 0)], 0x1a);
        let out = temp_path();
        let outside = temp_path();
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, out.join("nested")).unwrap();
        assert!(extract(&src, &out).is_err());
        assert!(!outside.join("file.bin").exists());
        fs::remove_dir_all(out).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replaces_a_hard_link_without_modifying_its_other_name() {
        let src = fixture(&[("file.bin", b"replacement", 0)], 0x1a);
        let out = temp_path();
        let outside = temp_path();
        fs::create_dir_all(&out).unwrap();
        fs::write(&outside, b"keep me").unwrap();
        fs::hard_link(&outside, out.join("file.bin")).unwrap();
        assert_eq!(extract(&src, &out).unwrap().extracted, 1);
        assert_eq!(fs::read(&outside).unwrap(), b"keep me");
        assert_eq!(fs::read(out.join("file.bin")).unwrap(), b"replacement");
        fs::remove_dir_all(out).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn rejects_file_directory_collisions_before_creating_output() {
        let src = fixture(&[("a", b"x", 0), ("a/b", b"y", 0)], 0x1a);
        let out = temp_path();
        assert!(extract(&src, &out).is_err());
        assert!(!out.exists());
    }
}
