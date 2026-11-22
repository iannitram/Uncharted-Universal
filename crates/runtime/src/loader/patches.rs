//! `BBPATCH2` binary patch parser, validator, and applicator for [`super`]
//! (porting `apply_patches` from `src/probe.c` into Rust for roadmap 3.17).

use super::{is_mapped, Reloc, Segment};
use std::path::Path;

const MAGIC: &[u8; 8] = b"BBPATCH2";
const MAX_PATCH_BYTES: usize = 4096;

/// One validated write entry inside a `BBPATCH2` patch file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchEntry {
    pub offset: u64,
    pub data: Vec<u8>,
}

/// Parsed `BBPATCH2` binary patch file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFile {
    pub base: u64,
    pub entries: Vec<PatchEntry>,
}

impl PatchFile {
    /// Parses and validates a `BBPATCH2` byte slice against `segments` and `relocs`.
    pub fn parse(bytes: &[u8], segments: &[Segment], relocs: &[Reloc]) -> Result<Self, String> {
        if bytes.len() < 24 || &bytes[..8] != MAGIC {
            return Err("invalid patch file".into());
        }
        let base = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let count = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let mut pos = 24usize;
        let mut entries = Vec::with_capacity(usize::try_from(count).unwrap_or(0).min(1024));

        for _ in 0..count {
            if pos + 16 > bytes.len() {
                return Err("bad patch entry".into());
            }
            let offset = u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap());
            let length = u64::from_le_bytes(bytes[pos + 8..pos + 16].try_into().unwrap());
            pos += 16;

            let len = usize::try_from(length).map_err(|_| "bad patch entry".to_string())?;
            if len == 0 || len > MAX_PATCH_BYTES || !is_mapped(segments, offset, length) {
                return Err("bad patch entry".into());
            }
            let end = pos
                .checked_add(len)
                .filter(|&e| e <= bytes.len())
                .ok_or_else(|| "bad patch entry".to_string())?;
            let mut data = bytes[pos..end].to_vec();
            pos = end;

            // Validate any overlapping relocations so invalid files fail before mutating memory.
            rebase_slots(base, 0, offset, &mut data, segments, relocs, false)?;
            entries.push(PatchEntry { offset, data });
        }

        Ok(Self { base, entries })
    }

    /// Applies all entries to `[image_base, image_base + ...)` after relocations, rebasing any
    /// relocated pointer slots from `self.base` to `image_base`.
    ///
    /// Returns `(total_bytes_written, rebased_pointer_count)`.
    ///
    /// # Safety
    /// Every segment covering an entry's `[offset, offset + data.len())` must be mapped and
    /// writable at `image_base + offset`.
    pub unsafe fn apply(
        &self,
        image_base: u64,
        segments: &[Segment],
        relocs: &[Reloc],
    ) -> Result<(u64, u64), String> {
        let mut total_bytes = 0u64;
        let mut rebased = 0u64;
        for entry in &self.entries {
            if !is_mapped(segments, entry.offset, entry.data.len() as u64) {
                return Err("bad patch entry".into());
            }
            let mut data = entry.data.clone();
            rebased += rebase_slots(
                self.base,
                image_base,
                entry.offset,
                &mut data,
                segments,
                relocs,
                true,
            )?;
            // SAFETY: caller guarantees the mapped image is writable at `image_base + offset`.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    (image_base + entry.offset) as *mut u8,
                    data.len(),
                );
            }
            total_bytes += data.len() as u64;
        }
        Ok((total_bytes, rebased))
    }
}

/// Reads and applies a `BBPATCH2` file at `path` to the loaded image at `image_base`.
///
/// # Safety
/// Same requirements as [`PatchFile::apply`].
pub unsafe fn apply_file(
    path: &Path,
    image_base: u64,
    segments: &[Segment],
    relocs: &[Reloc],
) -> Result<(u64, u64), String> {
    let bytes = std::fs::read(path).map_err(|_| "invalid patch file".to_string())?;
    let patch = PatchFile::parse(&bytes, segments, relocs)?;
    // SAFETY: forwarded from caller.
    unsafe { patch.apply(image_base, segments, relocs) }
}

fn rebase_slots(
    patch_base: u64,
    image_base: u64,
    offset: u64,
    data: &mut [u8],
    segments: &[Segment],
    relocs: &[Reloc],
    write_rebase: bool,
) -> Result<u64, String> {
    let end = offset
        .checked_add(data.len() as u64)
        .ok_or_else(|| "bad patch entry".to_string())?;
    let mut seen_slots = Vec::<u64>::new();
    let mut rebased = 0u64;

    for reloc in relocs {
        let Some(reloc_end) = reloc.target.checked_add(8) else {
            continue;
        };
        if !(reloc.target < end && offset < reloc_end) {
            continue;
        }
        if reloc.kind != 0 || reloc.target < offset || reloc_end > end {
            return Err("patch overlaps a relocation".into());
        }
        // A relocated slot rebased twice would turn an already-adjusted address into garbage.
        if seen_slots.contains(&reloc.target) {
            continue;
        }
        seen_slots.push(reloc.target);

        let slot_off = (reloc.target - offset) as usize;
        let value = u64::from_le_bytes(data[slot_off..slot_off + 8].try_into().unwrap());
        let relative = value
            .checked_sub(patch_base)
            .filter(|&rel| is_mapped(segments, rel, 1))
            .ok_or_else(|| "patch writes a pointer outside the image".to_string())?;

        if write_rebase {
            let rebased_ptr = image_base.wrapping_add(relative);
            data[slot_off..slot_off + 8].copy_from_slice(&rebased_ptr.to_le_bytes());
        }
        rebased += 1;
    }

    Ok(rebased)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bbpatch2(base: u64, entries: &[(u64, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&base.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u64).to_le_bytes());
        for (offset, data) in entries {
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&(data.len() as u64).to_le_bytes());
            out.extend_from_slice(data);
        }
        out
    }

    #[test]
    fn parses_and_applies_patch_with_rebased_reloc_dedup() {
        let segments = [Segment {
            address: 0,
            size: 0x2000,
            flags: 3,
        }];
        let patch_base = 0x4000_0000u64;
        let target_ptr = patch_base + 0x1234;
        let mut payload = [0u8; 16];
        payload[0..4].copy_from_slice(&[0x90, 0x90, 0x90, 0x90]);
        payload[8..16].copy_from_slice(&target_ptr.to_le_bytes());

        // Duplicate reloc at offset 8 must only be rebased once.
        let relocs = [
            Reloc {
                target: 0x108,
                kind: 0,
                value: 0x1234,
                addend: 0,
            },
            Reloc {
                target: 0x108,
                kind: 0,
                value: 0x1234,
                addend: 0,
            },
        ];

        let raw = make_bbpatch2(patch_base, &[(0x100, &payload)]);
        let patch = PatchFile::parse(&raw, &segments, &relocs).unwrap();

        let mut image = vec![0u8; 0x2000];
        let image_base = image.as_mut_ptr() as usize as u64;
        // SAFETY: `image` covers [0, 0x2000) and is writable.
        let (bytes_written, rebased) =
            unsafe { patch.apply(image_base, &segments, &relocs) }.unwrap();
        assert_eq!(bytes_written, 16);
        assert_eq!(rebased, 1);
        assert_eq!(&image[0x100..0x104], &[0x90, 0x90, 0x90, 0x90]);
        let got_ptr = u64::from_le_bytes(image[0x108..0x110].try_into().unwrap());
        assert_eq!(got_ptr, image_base + 0x1234);
    }

    #[test]
    fn rejects_invalid_magic_oob_and_bad_reloc_overlap() {
        let segments = [Segment {
            address: 0x1000,
            size: 0x1000,
            flags: 1,
        }];
        assert!(PatchFile::parse(b"NOTPATCH", &segments, &[]).is_err());

        let oob = make_bbpatch2(0x1000, &[(0x500, &[1, 2, 3])]);
        assert!(PatchFile::parse(&oob, &segments, &[]).is_err());

        let relocs = [Reloc {
            target: 0x1004,
            kind: 1,
            value: 0,
            addend: 0,
        }];
        let overlap = make_bbpatch2(0x1000, &[(0x1000, &[0u8; 16])]);
        assert!(PatchFile::parse(&overlap, &segments, &relocs).is_err());

        let bad_ptr_reloc = [Reloc {
            target: 0x1000,
            kind: 0,
            value: 0x1000,
            addend: 0,
        }];
        let bad_ptr = make_bbpatch2(0x1000, &[(0x1000, &[0u8; 8])]);
        assert!(PatchFile::parse(&bad_ptr, &segments, &bad_ptr_reloc).is_err());
    }
}
