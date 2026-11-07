//! Preparing a game folder into an install: the boot image, linked modules, the content
//! profile, patches and the render size. Replaces `scripts/prepare.py` (SELF/ELF parsing, the
//! memory image, import tables) and the orchestration in `run.py` (`prepare`, `locate`,
//! `patch_files_for`). The install is what `bundle` serves to other devices (SPEC.md M11).
//! The bounded SELF/plain-ELF parser follows the inherited contract at
//! `Supermedo/uncharted_pc` `scripts/prepare.py`, commit
//! `8b66edc0192ea246ca63b360857402a0aa55626d`; tests use authored synthetic inputs only.

use crate::error::{not_built, Error, Result};
use crate::games::Game;
use std::path::{Path, PathBuf};

/// The first eight bytes of a freshly prepared `boot.bin`.
pub const BOOT_MAGIC: [u8; 8] = *b"BBPROBE2";
/// The memory image of an executable may not exceed this (512 MiB).
pub const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;

/// The NID of a symbol name: the first 8 bytes of SHA-1 over the ASCII name plus the fixed salt
/// (`518d64a635ded8c1e6b039b1c3e55230`), reversed, standard base64 without padding, with `/`
/// written as `-`. Same as `nid()` in `prepare.py`.
pub fn nid(name: &str) -> String {
    not_built!("prepare::nid")
}

/// The loadable view of a SELF or plain ELF: what `parse_self` / `parse_plain_elf` return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// The memory image, `vaddr` 0 first.
    pub bytes: Vec<u8>,
    pub entry: u64,
    /// A normalized ELF file image. For a SELF, the ELF header and available loadable
    /// segments are copied into their ELF file offsets. Linker ports consume this together
    /// with `program_headers` rather than re-parsing unvalidated source bytes.
    pub elf: Vec<u8>,
    /// Validated ELF program headers shared by preparation and the linker ports.
    pub program_headers: Vec<ProgramHeader>,
}

/// A validated ELF64 program header. File ranges are known to fit `Image::elf` and memory
/// ranges are known to fit the bounded `Image::bytes` address space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramHeader {
    pub kind: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

/// Read an executable (SELF or plain ELF) into its memory image. A missing required segment is
/// `Error::Prepare("required segment <n> is unavailable")`; an oversize or unmapped entry is
/// `Error::Prepare("invalid memory image")`.
pub fn load_image(executable: &[u8]) -> Result<Image> {
    const ELF_HEADER_SIZE: usize = 64;
    const PROGRAM_HEADER_SIZE: usize = 56;
    const MAX_PHNUM: usize = 255;
    const PT_LOAD: u32 = 1;
    const PT_DYNAMIC: u32 = 2;
    const PT_SCE_DYNLIBDATA: u32 = 0x6100_0000;
    const PT_SCE_RELRO: u32 = 0x6100_0010;

    #[derive(Debug, Clone, Copy)]
    struct Header {
        entry: u64,
        phoff: u64,
        phnum: usize,
    }

    fn fail(message: impl Into<String>) -> Error {
        Error::Prepare(message.into())
    }

    fn slice(data: &[u8], offset: u64, size: u64) -> Result<&[u8]> {
        let start = usize::try_from(offset).map_err(|_| fail("invalid ELF bounds"))?;
        let len = usize::try_from(size).map_err(|_| fail("invalid ELF bounds"))?;
        let end = start
            .checked_add(len)
            .ok_or_else(|| fail("invalid ELF bounds"))?;
        data.get(start..end)
            .ok_or_else(|| fail("invalid ELF bounds"))
    }

    fn u16le(data: &[u8], offset: usize) -> Result<u16> {
        let bytes = data
            .get(offset..offset + 2)
            .ok_or_else(|| fail("invalid ELF header"))?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32le(data: &[u8], offset: usize) -> Result<u32> {
        let bytes = data
            .get(offset..offset + 4)
            .ok_or_else(|| fail("invalid ELF header"))?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("four byte range"),
        ))
    }

    fn u64le(data: &[u8], offset: usize) -> Result<u64> {
        let bytes = data
            .get(offset..offset + 8)
            .ok_or_else(|| fail("invalid ELF header"))?;
        Ok(u64::from_le_bytes(
            bytes.try_into().expect("eight byte range"),
        ))
    }

    fn parse_headers(data: &[u8], base: usize) -> Result<(Header, Vec<ProgramHeader>)> {
        let header = data
            .get(
                base..base
                    .checked_add(ELF_HEADER_SIZE)
                    .ok_or_else(|| fail("invalid ELF bounds"))?,
            )
            .ok_or_else(|| fail("expected little-endian x86-64 ELF"))?;
        if &header[..7] != b"\x7fELF\x02\x01\x01" || u16le(header, 18)? != 62 {
            return Err(fail("expected little-endian x86-64 ELF"));
        }
        let phoff = u64le(header, 32)?;
        let phentsize = u16le(header, 54)? as usize;
        let phnum = u16le(header, 56)? as usize;
        if phentsize != PROGRAM_HEADER_SIZE || phnum == 0 || phnum > MAX_PHNUM {
            return Err(fail("unsupported program headers"));
        }
        let table_start = (base as u64)
            .checked_add(phoff)
            .ok_or_else(|| fail("invalid ELF bounds"))?;
        let table_size = (phnum as u64)
            .checked_mul(PROGRAM_HEADER_SIZE as u64)
            .ok_or_else(|| fail("invalid ELF bounds"))?;
        let table = slice(data, table_start, table_size)?;
        let mut program_headers = Vec::with_capacity(phnum);
        for i in 0..phnum {
            let offset = i * PROGRAM_HEADER_SIZE;
            let ph = &table[offset..offset + PROGRAM_HEADER_SIZE];
            program_headers.push(ProgramHeader {
                kind: u32le(ph, 0)?,
                flags: u32le(ph, 4)?,
                offset: u64le(ph, 8)?,
                vaddr: u64le(ph, 16)?,
                filesz: u64le(ph, 32)?,
                memsz: u64le(ph, 40)?,
                align: u64le(ph, 48)?,
            });
        }
        Ok((
            Header {
                entry: u64le(header, 24)?,
                phoff,
                phnum,
            },
            program_headers,
        ))
    }

    fn checked_end(offset: u64, size: u64) -> Result<u64> {
        offset
            .checked_add(size)
            .ok_or_else(|| fail("invalid memory image"))
    }

    let is_self = executable.starts_with(b"\x00\x15=\x1d");
    let base = if is_self {
        let count = u16le(executable, 24)? as u64;
        let table_bytes = count
            .checked_mul(32)
            .ok_or_else(|| fail("invalid SELF header"))?;
        usize::try_from(
            32u64
                .checked_add(table_bytes)
                .ok_or_else(|| fail("invalid SELF header"))?,
        )
        .map_err(|_| fail("invalid SELF header"))?
    } else if executable.starts_with(b"\x7fELF") {
        0
    } else {
        return Err(fail("expected PS4 SELF or ELF64"));
    };

    if is_self {
        let descriptor_size = base
            .checked_sub(32)
            .ok_or_else(|| fail("invalid SELF header"))?;
        let _ = slice(executable, 32, descriptor_size as u64)?;
    }
    let (header, program_headers) = parse_headers(executable, base)?;
    for ph in &program_headers {
        if ph.filesz > ph.memsz {
            return Err(fail("segment file size exceeds memory size"));
        }
        let _ = checked_end(ph.offset, ph.filesz)?;
    }

    let elf_header_end = header
        .phoff
        .checked_add((header.phnum as u64) * (PROGRAM_HEADER_SIZE as u64))
        .and_then(|end| end.checked_add(ELF_HEADER_SIZE as u64))
        .ok_or_else(|| fail("invalid ELF bounds"))?;
    let max_file_end = program_headers
        .iter()
        .map(|ph| checked_end(ph.offset, ph.filesz))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(0)
        .max(elf_header_end);
    if max_file_end > MAX_IMAGE_BYTES {
        return Err(fail("probe image exceeds 512 MiB limit"));
    }
    let header_bytes = slice(executable, base as u64, elf_header_end)?;
    let mut covered = Vec::<(u64, u64)>::new();
    covered.push((0, elf_header_end));
    let mut copies = Vec::<(u64, u64, u64)>::new();
    if is_self {
        let count = (base - 32) / 32;
        for descriptor_index in 0..count {
            let at = 32 + descriptor_index * 32;
            let flags = u64le(executable, at)?;
            let offset = u64le(executable, at + 8)?;
            let size = u64le(executable, at + 16)?;
            let mem = u64le(executable, at + 24)?;
            if flags & 0x800 == 0 {
                continue;
            }
            if flags & 10 != 0 {
                return Err(fail("encrypted/compressed SELF segment is unsupported"));
            }
            let index = ((flags >> 20) & 4095) as usize;
            let ph = program_headers
                .get(index)
                .ok_or_else(|| fail("invalid segment index"))?;
            if size != ph.filesz || size != mem {
                return Err(fail("unsupported blocked segment layout"));
            }
            let _ = slice(executable, offset, size)?;
            let _ = checked_end(ph.offset, size)?;
            copies.push((offset, ph.offset, size));
            covered.push((ph.offset, checked_end(ph.offset, size)?));
        }
    } else {
        for (index, ph) in program_headers.iter().enumerate() {
            if ph.filesz == 0 {
                continue;
            }
            match slice(executable, ph.offset, ph.filesz) {
                Ok(_) => {
                    copies.push((ph.offset, ph.offset, ph.filesz));
                    covered.push((ph.offset, checked_end(ph.offset, ph.filesz)?));
                }
                Err(_)
                    if matches!(
                        ph.kind,
                        PT_LOAD | PT_DYNAMIC | PT_SCE_DYNLIBDATA | PT_SCE_RELRO
                    ) =>
                {
                    return Err(fail(format!("required segment {index} is unavailable")));
                }
                Err(_) => {}
            }
        }
    }

    for (index, ph) in program_headers.iter().enumerate() {
        if ph.filesz == 0 {
            continue;
        }
        let end = checked_end(ph.offset, ph.filesz)?;
        let is_covered = covered
            .iter()
            .any(|(start, finish)| *start <= ph.offset && end <= *finish);
        if !is_covered
            && matches!(
                ph.kind,
                PT_LOAD | PT_DYNAMIC | PT_SCE_DYNLIBDATA | PT_SCE_RELRO
            )
        {
            return Err(fail(format!("required segment {index} is unavailable")));
        }
    }

    let loads: Vec<&ProgramHeader> = program_headers
        .iter()
        .filter(|ph| matches!(ph.kind, PT_LOAD | PT_SCE_RELRO))
        .collect();
    if loads.is_empty() {
        return Err(fail("invalid memory image"));
    }
    let image_size = loads
        .iter()
        .map(|ph| checked_end(ph.vaddr, ph.memsz))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    if image_size > MAX_IMAGE_BYTES {
        return Err(fail("invalid memory image"));
    }
    let entry_mapped = loads.iter().any(|ph| {
        ph.vaddr
            .checked_add(ph.memsz)
            .is_some_and(|end| ph.vaddr <= header.entry && header.entry < end)
    });
    if !entry_mapped {
        return Err(fail("invalid memory image"));
    }

    // All source ranges, destination ranges, segment sizes and the eventual memory image
    // are validated before either output buffer is allocated.
    let elf_len = usize::try_from(max_file_end).map_err(|_| fail("invalid ELF bounds"))?;
    let image_len = usize::try_from(image_size).map_err(|_| fail("invalid memory image"))?;
    let mut elf = vec![0; elf_len];
    elf[..header_bytes.len()].copy_from_slice(header_bytes);
    for (source_offset, destination_offset, size) in copies {
        let source = slice(executable, source_offset, size)?;
        slice_mut(&mut elf, destination_offset, size)?.copy_from_slice(source);
    }
    let mut bytes = vec![0; image_len];
    for ph in loads {
        if ph.filesz > 0 {
            let source = slice(&elf, ph.offset, ph.filesz)?;
            slice_mut(&mut bytes, ph.vaddr, ph.filesz)?.copy_from_slice(source);
        }
    }
    Ok(Image {
        bytes,
        entry: header.entry,
        elf,
        program_headers,
    })
}

fn slice_mut(data: &mut [u8], offset: u64, size: u64) -> Result<&mut [u8]> {
    let start = usize::try_from(offset).map_err(|_| Error::Prepare("invalid ELF bounds".into()))?;
    let len = usize::try_from(size).map_err(|_| Error::Prepare("invalid ELF bounds".into()))?;
    let end = start
        .checked_add(len)
        .ok_or_else(|| Error::Prepare("invalid ELF bounds".into()))?;
    data.get_mut(start..end)
        .ok_or_else(|| Error::Prepare("invalid ELF bounds".into()))
}

/// What the caller asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareOptions {
    /// Third-party patches the player left enabled (`patches::Selection`) apply on top.
    pub apply_patches: bool,
    /// The render size, or `None` for the game's own 1080p (`render_size`).
    pub render_size: Option<(u32, u32)>,
}

/// What an install holds, to be written as `install.json` and hashed into a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallManifest {
    pub game: Game,
    pub title_id: String,
    pub app_ver: String,
    /// Files under the install folder, relative with `/`. Never a file of the dump.
    pub files: Vec<String>,
}

/// Prepare `game` from the dump folder `dump_dir` into `install_dir`: gate on the revision
/// (`games::check_revision`), build and link the boot image, write the content profile,
/// compile patches and the render size. Returns the manifest.
pub fn prepare(
    dump_dir: &Path,
    install_dir: &Path,
    game: Game,
    options: &PrepareOptions,
) -> Result<InstallManifest> {
    not_built("prepare::prepare")
}

/// The folder a game's dump lives in and the executable file to use there: `BB_GAME_DIR_U1` ..
/// `_U3` first, else the shared `BB_GAME_DIR` (`locate` in `run.py`), read through `env` so
/// tests can fake it. A game dumped on its own may hold its executable as that folder's
/// `eboot.bin`, which is then the file returned. Neither variable set is `Error::Env(..)`.
pub fn locate(game: Game, env: impl Fn(&str) -> Option<String>) -> Result<(PathBuf, String)> {
    not_built("prepare::locate")
}

#[cfg(test)]
mod image_tests {
    use super::{load_image, Error};

    fn put_u16(data: &mut [u8], at: usize, value: u16) {
        data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(data: &mut [u8], at: usize, value: u32) {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u64(data: &mut [u8], at: usize, value: u64) {
        data[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn elf_header(data: &mut [u8], at: usize, phnum: u16, entry: u64) {
        data[at..at + 7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        put_u16(data, at + 18, 62);
        put_u64(data, at + 24, entry);
        put_u64(data, at + 32, 64);
        put_u16(data, at + 54, 56);
        put_u16(data, at + 56, phnum);
    }

    fn program_header(
        data: &mut [u8],
        at: usize,
        kind: u32,
        offset: u64,
        vaddr: u64,
        filesz: u64,
        memsz: u64,
    ) {
        put_u32(data, at, kind);
        put_u32(data, at + 4, 5);
        put_u64(data, at + 8, offset);
        put_u64(data, at + 16, vaddr);
        put_u64(data, at + 32, filesz);
        put_u64(data, at + 40, memsz);
    }

    fn self_fixture() -> Vec<u8> {
        let mut data = vec![0; 0x210];
        data[..4].copy_from_slice(b"\x00\x15=\x1d");
        put_u16(&mut data, 24, 1);
        put_u64(&mut data, 32, 0x800);
        put_u64(&mut data, 40, 0x200);
        put_u64(&mut data, 48, 16);
        put_u64(&mut data, 56, 16);
        elf_header(&mut data, 64, 1, 0);
        program_header(&mut data, 128, 1, 0x1000, 0, 16, 32);
        data[0x200..].copy_from_slice(&(0u8..16).collect::<Vec<_>>());
        data
    }

    fn plain_fixture() -> Vec<u8> {
        let mut data = vec![0; 0x1010];
        elf_header(&mut data, 0, 1, 0);
        program_header(&mut data, 64, 1, 0x1000, 0, 16, 32);
        data[0x1000..].copy_from_slice(&(0u8..16).collect::<Vec<_>>());
        data
    }

    fn is_prepare_error(result: Result<super::Image, Error>) -> bool {
        matches!(result, Err(Error::Prepare(_)))
    }

    #[test]
    fn self_and_plain_elf_produce_the_same_memory_image_and_entry() {
        let wrapped = load_image(&self_fixture()).unwrap();
        let plain = load_image(&plain_fixture()).unwrap();
        assert_eq!(wrapped.bytes, plain.bytes);
        assert_eq!(wrapped.entry, plain.entry);
        assert_eq!(
            wrapped.bytes,
            (0u8..16)
                .chain(std::iter::repeat_n(0, 16))
                .collect::<Vec<_>>()
        );
        assert_eq!(wrapped.program_headers, plain.program_headers);
    }

    #[test]
    fn rejects_wrong_class_endianness_machine_and_program_header_contract() {
        let mut fixture = plain_fixture();
        fixture[4] = 1;
        assert!(is_prepare_error(load_image(&fixture)));
        let mut fixture = plain_fixture();
        fixture[5] = 2;
        assert!(is_prepare_error(load_image(&fixture)));
        let mut fixture = plain_fixture();
        put_u16(&mut fixture, 18, 3);
        assert!(is_prepare_error(load_image(&fixture)));
        let mut fixture = plain_fixture();
        put_u16(&mut fixture, 54, 32);
        assert!(is_prepare_error(load_image(&fixture)));
        let mut fixture = plain_fixture();
        put_u16(&mut fixture, 56, 0);
        assert!(is_prepare_error(load_image(&fixture)));
        let mut fixture = plain_fixture();
        put_u64(&mut fixture, 32, u64::MAX - 31);
        assert!(is_prepare_error(load_image(&fixture)));
        let mut fixture = plain_fixture();
        put_u64(&mut fixture, 64 + 8, super::MAX_IMAGE_BYTES + 1);
        assert!(
            matches!(load_image(&fixture), Err(Error::Prepare(message)) if message == "probe image exceeds 512 MiB limit")
        );
    }

    #[test]
    fn rejects_truncated_required_segment_and_invalid_memory_extents() {
        let mut fixture = plain_fixture();
        fixture.truncate(0x100f);
        assert!(
            matches!(load_image(&fixture), Err(Error::Prepare(message)) if message == "required segment 0 is unavailable")
        );
        let mut fixture = plain_fixture();
        put_u64(&mut fixture, 64 + 32, 33);
        assert!(
            matches!(load_image(&fixture), Err(Error::Prepare(message)) if message == "segment file size exceeds memory size")
        );
        let mut fixture = plain_fixture();
        put_u64(&mut fixture, 64 + 40, u64::MAX);
        assert!(is_prepare_error(load_image(&fixture)));
        let mut fixture = plain_fixture();
        put_u64(&mut fixture, 64 + 16, u64::MAX - 7);
        put_u64(&mut fixture, 64 + 40, 16);
        assert!(is_prepare_error(load_image(&fixture)));
    }

    #[test]
    fn preserves_self_payload_refusals() {
        let mut fixture = self_fixture();
        put_u64(&mut fixture, 32, 0);
        assert!(
            matches!(load_image(&fixture), Err(Error::Prepare(message)) if message == "required segment 0 is unavailable")
        );

        let mut fixture = self_fixture();
        put_u64(&mut fixture, 32, 0x802);
        assert!(
            matches!(load_image(&fixture), Err(Error::Prepare(message)) if message == "encrypted/compressed SELF segment is unsupported")
        );

        let mut fixture = self_fixture();
        put_u64(&mut fixture, 32, 0x200800);
        assert!(
            matches!(load_image(&fixture), Err(Error::Prepare(message)) if message == "invalid segment index")
        );

        let mut fixture = self_fixture();
        put_u64(&mut fixture, 48, 15);
        assert!(
            matches!(load_image(&fixture), Err(Error::Prepare(message)) if message == "unsupported blocked segment layout")
        );
    }

    #[test]
    fn rejects_entry_not_mapped_by_a_loadable_segment() {
        let mut fixture = plain_fixture();
        put_u64(&mut fixture, 24, 32);
        assert!(
            matches!(load_image(&fixture), Err(Error::Prepare(message)) if message == "invalid memory image")
        );
    }
}
