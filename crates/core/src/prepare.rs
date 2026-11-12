//! Preparing a game folder into an install: the boot image, linked modules, the content
//! profile, patches and the render size. Replaces `scripts/prepare.py` (SELF/ELF parsing, the
//! memory image, import tables) and the orchestration in `run.py` (`prepare`, `locate`,
//! `patch_files_for`). The install is what `bundle` serves to other devices (SPEC.md M11).
//! The bounded SELF/plain-ELF parser follows the inherited contract at
//! `Supermedo/uncharted_pc` `scripts/prepare.py`, commit
//! `8b66edc0192ea246ca63b360857402a0aa55626d`; tests use authored synthetic inputs only.

use crate::content_profile::{self, Sku};
use crate::error::{Error, Result};
use crate::games::{self, Game};
use crate::patches::{self, Selection};
use crate::sfo::{self, SfoValue};
use crate::{bundle, link, render_size};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The first eight bytes of a freshly prepared `boot.bin`.
pub const BOOT_MAGIC: [u8; 8] = *b"BBPROBE2";
/// The memory image of an executable may not exceed this (512 MiB).
pub const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;

/// The NID of a symbol name: the first 8 bytes of SHA-1 over the ASCII name plus the fixed salt
/// (`518d64a635ded8c1e6b039b1c3e55230`), reversed, standard base64 without padding, with `/`
/// written as `-`. Same as `nid()` in `prepare.py`.
pub fn nid(name: &str) -> String {
    const SALT: [u8; 16] = [
        0x51, 0x8d, 0x64, 0xa6, 0x35, 0xde, 0xd8, 0xc1, 0xe6, 0xb0, 0x39, 0xb1, 0xc3, 0xe5, 0x52,
        0x30,
    ];
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut input = name.as_bytes().to_vec();
    input.extend_from_slice(&SALT);
    let mut digest: Vec<u8> = sha1(&input)[..8].to_vec();
    digest.reverse();
    let mut out = String::new();
    for chunk in digest.chunks(3) {
        let mut block = [0u8; 3];
        block[..chunk.len()].copy_from_slice(chunk);
        let bits = u32::from(block[0]) << 16 | u32::from(block[1]) << 8 | u32::from(block[2]);
        // Unpadded: a chunk of n bytes gives n + 1 characters.
        for i in 0..=chunk.len() {
            let index = (bits >> (18 - 6 * i)) & 63;
            out.push(ALPHABET[index as usize] as char);
        }
    }
    out.replace('/', "-")
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

    // The PS4 SELF magic `4f 15 3d 1d` (`parse_self` in `prepare.py`); not `00 15 3d 1d`.
    let is_self = executable.starts_with(b"\x4f\x15=\x1d");
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
        // Only loadable segments map memory; PT_SCE_DYNLIBDATA and similar file-only
        // segments carry `p_memsz` 0 in real executables (`prepare()` in `prepare.py`).
        if matches!(ph.kind, PT_LOAD | PT_SCE_RELRO) && ph.filesz > ph.memsz {
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

/// Where the third-party patches come from (`patch_arguments` in `run.py`): the patch XML
/// folder (`BB_PATCHES_DIR`) and the player's choice in it (`patches.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchSource {
    /// Every `*.xml` directly in this folder is read, in name order.
    pub dir: PathBuf,
    /// The player's choice; a patch it does not name keeps its file's `isEnabled`.
    pub selection: Selection,
}

/// What the caller asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareOptions {
    /// The SKU `content.bin` reports (`BB_CONTENT_SKU`, full by default).
    pub sku: Sku,
    /// Third-party patches the player left enabled apply on top (`patches.bin`); `None` writes
    /// no patch file, as `run.py` does without `BB_PATCHES_DIR`.
    pub patches: Option<PatchSource>,
    /// The render size, or `None` for the game's own 1080p (`render_size`).
    pub render_size: Option<(u32, u32)>,
}

impl Default for PrepareOptions {
    fn default() -> Self {
        PrepareOptions {
            sku: Sku::Full,
            patches: None,
            render_size: None,
        }
    }
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

impl InstallManifest {
    /// `install.json`: `game` (its key), `title_id`, `app_ver` and `files`, indented two
    /// spaces, ending in a newline.
    pub fn to_json(&self) -> String {
        let value = serde_json::json!({
            "game": self.game.key(),
            "title_id": self.title_id,
            "app_ver": self.app_ver,
            "files": self.files,
        });
        let mut text = serde_json::to_string_pretty(&value).expect("a JSON value serializes");
        text.push('\n');
        text
    }
}

/// The manifest `prepare` writes last into the install folder; an install without it is
/// unfinished. It is not listed in its own `files`.
pub const MANIFEST_FILE: &str = "install.json";
/// The boot image `prepare` writes before linking (starts with [`BOOT_MAGIC`]).
pub const BOOT_FILE: &str = "boot.bin";
/// The executable's analysis (`analysis.json` in `prepare.py`): imports, relocation counts,
/// the libc evidence. Hashes and counts only, no bytes of the executable.
pub const ANALYSIS_FILE: &str = "analysis.json";
/// `content_profile::ContentProfile::to_bin`, read by the runtime.
pub const CONTENT_FILE: &str = "content.bin";
/// `content_profile::ContentProfile::to_json`.
pub const CONTENT_JSON_FILE: &str = "content-profile.json";
/// `patches::to_patches_bin` of the selected third-party patches.
pub const PATCHES_FILE: &str = "patches.bin";

/// Prepare `game` from the dump folder `dump_dir` into `install_dir`: gate on the revision
/// (`games::check_revision`), build and link the boot image, write the content profile,
/// compile patches and the render size. Returns the manifest.
///
/// The gate runs before anything is written: a revision the port does not support is
/// `Error::WrongRevision` and leaves `install_dir` as it was (not even created). Everything
/// else that can be refused from the dump alone (the executable, `libc.prx`, the content
/// profile, the patches) is checked before the first write too.
///
/// The install holds only files made here, never a copy of a dump file (SPEC.md M9):
/// `boot.bin`, `analysis.json`, the linked images and their reports, `content.bin`,
/// `content-profile.json`, `patches.bin`, `boot-res-<W>x<H>.bin` and its `.json`, then
/// `install.json`. Unlike `prepare.py`, no `eboot.elf` or `entry.bin` is written, and every
/// name is checked against `bundle::is_forbidden` before the manifest is. `install.json` is
/// written last, so an install without it is unfinished. Device-owned folders
/// (`bundle::DEVICE_OWNED`) are left alone.
pub fn prepare(
    dump_dir: &Path,
    install_dir: &Path,
    game: Game,
    options: &PrepareOptions,
) -> Result<InstallManifest> {
    prepare_with(dump_dir, install_dir, game, options, &games::SUPPORTED)
}

/// [`prepare`] against an explicit revision table (tests open the gate with it).
fn prepare_with(
    dump_dir: &Path,
    install_dir: &Path,
    game: Game,
    options: &PrepareOptions,
    supported: &[(Game, &[&str])],
) -> Result<InstallManifest> {
    let sfo_bytes = read_dump(dump_dir, "sce_sys/param.sfo")?;
    let values = sfo::parse(&sfo_bytes)?;
    let app_ver = sfo::app_version(&values)
        .ok_or_else(|| Error::BadDump("param.sfo has no APP_VER or VERSION.".into()))?
        .to_owned();
    // The revision gate (SPEC.md M11) comes before any other read or write.
    games::check_revision_in(game, &app_ver, supported)?;
    let title_id = sfo::text(&values, sfo::TITLE_ID)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::BadDump("param.sfo has no TITLE_ID.".into()))?
        .to_owned();

    let exe = executable_in(dump_dir, game);
    let source = read_dump(dump_dir, &exe)?;
    let image = load_image(&source)?;
    let libc = inspect_libc(&read_dump(dump_dir, "sce_module/libc.prx")?)?;
    let boot = build_boot(&image, libc.init_env_is_ret)?;
    let profile = content_profile::from_sfo(&values, options.sku)?;
    let patch_file = match &options.patches {
        Some(patch_source) => Some(compile_patches(
            patch_source,
            &image.program_headers,
            &patches::Target {
                title_id: title_id.clone(),
                executable: game.executable().to_owned(),
                app_ver: app_ver.clone(),
            },
        )?),
        None => None,
    };
    let analysis = analysis_json(dump_dir, &source, &values, &image, &boot, &libc)?;

    fs::create_dir_all(install_dir)?;
    remove_if_present(&install_dir.join(MANIFEST_FILE))?;
    for entry in fs::read_dir(install_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry.file_name().to_string_lossy().starts_with("boot-res-")
        {
            fs::remove_file(entry.path())?;
        }
    }

    let mut files = Vec::new();
    put(install_dir, &mut files, BOOT_FILE, &boot.file)?;
    put(install_dir, &mut files, ANALYSIS_FILE, analysis.as_bytes())?;
    link::link_modules(dump_dir, install_dir, &link::DEFAULT_MODULES, &exe)?;
    files.extend([link::LINKED_FILE.to_owned(), link::REPORT_FILE.to_owned()]);
    // The libc-only path (`run.sh`) always links the folder's `eboot.bin`, so its image is
    // only made when that is the executable being prepared.
    if exe == "eboot.bin" {
        link::link_libc(dump_dir, install_dir)?;
        files.extend([link::LIBC_IMAGE.to_owned(), link::LIBC_REPORT.to_owned()]);
    }
    put(install_dir, &mut files, CONTENT_FILE, &profile.to_bin())?;
    put(
        install_dir,
        &mut files,
        CONTENT_JSON_FILE,
        profile.to_json().as_bytes(),
    )?;
    if let Some(bytes) = &patch_file {
        put(install_dir, &mut files, PATCHES_FILE, bytes)?;
    }
    if let Some(size) = options
        .render_size
        .filter(|&size| size != render_size::OUTPUT_SIZE)
    {
        let linked = fs::read(install_dir.join(link::LINKED_FILE))?;
        let patched = render_size::patch_image(&linked, game, size)?;
        let stem = format!("boot-res-{}x{}", size.0, size.1);
        let meta = serde_json::json!({
            "render": format!("{}x{}", patched.render.0, patched.render.1),
            "extra_mib": patched.extra_mib,
            "requested": format!("{}x{}", size.0, size.1),
        });
        put(
            install_dir,
            &mut files,
            &format!("{stem}.bin"),
            &patched.image,
        )?;
        put(
            install_dir,
            &mut files,
            &format!("{stem}.json"),
            meta.to_string().as_bytes(),
        )?;
    }

    files.sort();
    if let Some(name) = files.iter().find(|name| bundle::is_forbidden(name)) {
        return Err(Error::Prepare(format!(
            "{name} may not be part of an install"
        )));
    }
    let manifest = InstallManifest {
        game,
        title_id,
        app_ver,
        files,
    };
    fs::write(install_dir.join(MANIFEST_FILE), manifest.to_json())?;
    Ok(manifest)
}

/// Write one install file and list it.
fn put(install_dir: &Path, files: &mut Vec<String>, name: &str, bytes: &[u8]) -> Result<()> {
    fs::write(install_dir.join(name), bytes)?;
    files.push(name.to_owned());
    Ok(())
}

/// The folder a game's dump lives in and the executable file to use there: `BB_GAME_DIR_U1` ..
/// `_U3` first, else the shared `BB_GAME_DIR` (`locate` in `run.py`), read through `env` so
/// tests can fake it. A game dumped on its own may hold its executable as that folder's
/// `eboot.bin`, which is then the file returned. Neither variable set is `Error::Env(..)`.
/// An empty value counts as not set.
pub fn locate(game: Game, env: impl Fn(&str) -> Option<String>) -> Result<(PathBuf, String)> {
    let own_key = format!("BB_GAME_DIR_{}", game.key().to_ascii_uppercase());
    let set = |key: &str| env(key).filter(|value| !value.is_empty());
    let own = set(&own_key);
    let has_own = own.is_some();
    let folder = match own.or_else(|| set("BB_GAME_DIR")) {
        Some(folder) => PathBuf::from(folder),
        None => {
            return Err(Error::Env(format!(
                "{own_key} or BB_GAME_DIR (the game folder, the one with eboot.bin)"
            )))
        }
    };
    let name = game.executable();
    // Only a game's own folder: in a collection folder eboot.bin is Uncharted 1, and an
    // incomplete dump without big2/big3 would start Uncharted 1 instead.
    if has_own && uses_own_eboot(&folder, name) {
        return Ok((folder, "eboot.bin".into()));
    }
    Ok((folder, name.into()))
}

/// The collection's own title ids (`COLLECTION_TITLE_IDS` in `prepare.py`): there
/// `eboot.bin` is Uncharted 1.
const COLLECTION_TITLE_IDS: [&str; 3] = ["CUSA02343", "CUSA02320", "CUSA02344"];

/// `collection_folder` in `prepare.py`: the folder holds the collection (its `eboot.bin` is
/// Uncharted 1), not a game dumped on its own. An unreadable `param.sfo` is not a collection.
fn collection_folder(folder: &Path) -> bool {
    if folder.join("u1data").is_dir() {
        return true;
    }
    fs::read(folder.join("sce_sys").join("param.sfo"))
        .ok()
        .and_then(|bytes| sfo::parse(&bytes).ok())
        .is_some_and(|values| {
            sfo::text(&values, sfo::TITLE_ID).is_some_and(|id| COLLECTION_TITLE_IDS.contains(&id))
        })
}

/// A game dumped on its own whose executable is the folder's `eboot.bin`.
fn uses_own_eboot(folder: &Path, name: &str) -> bool {
    !folder.join(name).is_file() && folder.join("eboot.bin").is_file() && !collection_folder(folder)
}

/// The executable `prepare` reads in `folder`: the game's own name, else the folder's
/// `eboot.bin` for a game dumped on its own (the rule [`locate`] applies).
fn executable_in(folder: &Path, game: Game) -> String {
    let name = game.executable();
    if uses_own_eboot(folder, name) {
        "eboot.bin".into()
    } else {
        name.into()
    }
}

/// Read a dump file; a missing or unreadable one is `BadDump` naming it (never its bytes).
fn read_dump(dump_dir: &Path, rel: &str) -> Result<Vec<u8>> {
    fs::read(dump_dir.join(rel)).map_err(|e| Error::BadDump(format!("{rel}: {e}")))
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// The third-party patches for `target` (`patches.py --external-only`): every `*.xml` in the
/// folder in name order (one that does not read or parse is skipped, as the script does), those
/// that apply to this dump, the player's selection, compiled against the executable's loadable
/// segments. A missing folder is no patches.
///
/// The segments are `(vaddr, vaddr + memsz)` of each non-empty `PT_LOAD`, as `eboot_segments`
/// in `scripts/patches.py` reads them (a patch address minus `EBOOT_BASE` must fall inside
/// one). `patches::eboot_segments` instead subtracts `EBOOT_BASE` from the vaddr, which refuses
/// an executable linked at vaddr 0.
fn compile_patches(
    source: &PatchSource,
    program_headers: &[ProgramHeader],
    target: &patches::Target,
) -> Result<Vec<u8>> {
    let mut names = Vec::new();
    if source.dir.is_dir() {
        for entry in fs::read_dir(&source.dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type()?.is_file() && name.ends_with(".xml") {
                names.push(name);
            }
        }
    }
    names.sort();
    let mut all = Vec::new();
    for name in names {
        let Ok(xml) = fs::read_to_string(source.dir.join(&name)) else {
            continue;
        };
        if let Ok(found) = patches::parse(&name, &xml) {
            all.extend(found);
        }
    }
    let found = patches::applicable(&all, target, true);
    let selected = patches::select(&found, &source.selection);
    let segments: Vec<(u64, u64)> = program_headers
        .iter()
        .filter(|ph| ph.kind == PT_LOAD && ph.memsz > 0)
        .map(|ph| (ph.vaddr, ph.vaddr + ph.memsz))
        .collect();
    let (writes, _notes) = patches::compile(&selected, &segments)?;
    Ok(patches::to_patches_bin(&writes))
}

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_SCE_DYNLIBDATA: u32 = 0x6100_0000;
const PT_SCE_RELRO: u32 = 0x6100_0010;
const DT_NEEDED: u64 = 1;
const DT_SCE_JMPREL: u64 = 0x6100_0029;
const DT_SCE_PLTRELSZ: u64 = 0x6100_002d;
const DT_SCE_RELA: u64 = 0x6100_002f;
const DT_SCE_RELASZ: u64 = 0x6100_0031;
const DT_SCE_STRTAB: u64 = 0x6100_0035;
const DT_SCE_STRSZ: u64 = 0x6100_0037;
const DT_SCE_SYMTAB: u64 = 0x6100_0039;
const DT_SCE_SYMTABSZ: u64 = 0x6100_003f;
const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;
const SHN_ABS: u16 = 0xfff1;
/// An import name in `boot.bin` is a 128-byte field, NUL included.
const NAME_FIELD: usize = 128;

fn fail(message: impl Into<String>) -> Error {
    Error::Prepare(message.into())
}

fn bytes_at(data: &[u8], offset: u64, size: u64) -> Result<&[u8]> {
    let range = usize::try_from(offset)
        .ok()
        .zip(usize::try_from(size).ok())
        .and_then(|(start, len)| Some(start..start.checked_add(len)?));
    range.and_then(|range| data.get(range)).ok_or_else(|| {
        fail(format!(
            "out of bounds: {offset:#x}+{size:#x} / {:#x}",
            data.len()
        ))
    })
}

fn word_at(data: &[u8], offset: u64) -> Result<u64> {
    let bytes = bytes_at(data, offset, 8)?;
    Ok(u64::from_le_bytes(bytes.try_into().expect("eight bytes")))
}

/// The SCE dynamic section of a loaded executable or module: its tags in order, the
/// dynamic-library data blob they index into, and the string table in that blob.
struct Dynamic<'a> {
    entries: Vec<(u64, u64)>,
    blob: &'a [u8],
    strings: &'a [u8],
}

impl<'a> Dynamic<'a> {
    /// `stop_at_null`: `prepare()` stops at the first `DT_NULL`, `inspect_libc` reads the whole
    /// segment. A tag given twice takes its last value either way (a Python `dict`).
    fn read(image: &'a Image, stop_at_null: bool) -> Result<Dynamic<'a>> {
        let dynamic = image
            .program_headers
            .iter()
            .find(|ph| ph.kind == PT_DYNAMIC)
            .ok_or_else(|| fail("no PT_DYNAMIC segment"))?;
        let mut entries = Vec::new();
        for at in (0..dynamic.filesz / 16).map(|i| dynamic.offset + i * 16) {
            let tag = word_at(&image.elf, at)?;
            if tag == 0 && stop_at_null {
                break;
            }
            entries.push((tag, word_at(&image.elf, at + 8)?));
        }
        let lib = image
            .program_headers
            .iter()
            .find(|ph| ph.kind == PT_SCE_DYNLIBDATA)
            .ok_or_else(|| fail("no PT_SCE_DYNLIBDATA segment"))?;
        let mut dynamic = Dynamic {
            entries,
            blob: bytes_at(&image.elf, lib.offset, lib.filesz)?,
            strings: &[],
        };
        dynamic.strings = dynamic.table(DT_SCE_STRTAB, DT_SCE_STRSZ)?;
        Ok(dynamic)
    }

    fn tag(&self, tag: u64) -> Result<u64> {
        self.entries
            .iter()
            .rev()
            .find(|(t, _)| *t == tag)
            .map(|(_, value)| *value)
            .ok_or_else(|| fail(format!("missing dynamic tag {tag:#x}")))
    }

    fn table(&self, offset_tag: u64, size_tag: u64) -> Result<&'a [u8]> {
        bytes_at(self.blob, self.tag(offset_tag)?, self.tag(size_tag)?)
    }

    fn string(&self, offset: u64) -> Result<String> {
        bytes_at(self.strings, offset, 1)?;
        let start = offset as usize;
        let len = self.strings[start..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| fail("unterminated dynamic string"))?;
        let text = &self.strings[start..start + len];
        if !text.is_ascii() {
            return Err(fail("non-ASCII dynamic string"));
        }
        Ok(String::from_utf8_lossy(text).into_owned())
    }

    fn symbols(&self) -> Result<Vec<DynSymbol>> {
        let table = self.table(DT_SCE_SYMTAB, DT_SCE_SYMTABSZ)?;
        if table.len() % 24 != 0 {
            return Err(fail("bad symbol table size"));
        }
        table
            .as_chunks::<24>()
            .0
            .iter()
            .map(|entry| {
                let name = u32::from_le_bytes(entry[..4].try_into().expect("four bytes"));
                Ok(DynSymbol {
                    name: self.string(u64::from(name))?,
                    kind: entry[4] & 15,
                    section: u16::from_le_bytes([entry[6], entry[7]]),
                    value: u64::from_le_bytes(entry[8..16].try_into().expect("eight bytes")),
                    size: u64::from_le_bytes(entry[16..].try_into().expect("eight bytes")),
                })
            })
            .collect()
    }
}

struct DynSymbol {
    name: String,
    kind: u8,
    section: u16,
    value: u64,
    size: u64,
}

/// `boot.bin` and what `analysis.json` says about it.
struct Boot {
    file: Vec<u8>,
    needed: Vec<String>,
    relocation_counts: BTreeMap<u32, u64>,
    imports: Vec<String>,
}

/// The `BBPROBE2` boot image of `prepare()` in `prepare.py`: header (magic, image size, entry,
/// segment, relocation and import counts, the libc `_init_env` flag), the loadable segments
/// (vaddr, memsz, flags), the import names (128-byte fields), the relocations (target, kind,
/// value, addend; kind 0 base-relative, 1 function import, 2 data import), then the memory
/// image with absolute symbols already written.
fn build_boot(image: &Image, init_env_is_ret: bool) -> Result<Boot> {
    let loads: Vec<&ProgramHeader> = image
        .program_headers
        .iter()
        .filter(|ph| matches!(ph.kind, PT_LOAD | PT_SCE_RELRO))
        .collect();
    let mapped = |address: u64| {
        loads.iter().any(|ph| {
            ph.vaddr <= address
                && address
                    .checked_add(8)
                    .zip(ph.vaddr.checked_add(ph.memsz))
                    .is_some_and(|(end, limit)| end <= limit)
        })
    };
    let dynamic = Dynamic::read(image, true)?;
    let symbols = dynamic.symbols()?;
    let mut memory = image.bytes.clone();
    let mut relocations: Vec<(u64, u64, i64, i64)> = Vec::new();
    let mut counts = BTreeMap::<u32, u64>::new();
    let mut imports: Vec<usize> = Vec::new();
    for (offset_tag, size_tag) in [
        (DT_SCE_JMPREL, DT_SCE_PLTRELSZ),
        (DT_SCE_RELA, DT_SCE_RELASZ),
    ] {
        let table = dynamic.table(offset_tag, size_tag)?;
        if table.len() % 24 != 0 {
            return Err(fail("bad relocation table size"));
        }
        for entry in table.as_chunks::<24>().0 {
            let target = u64::from_le_bytes(entry[..8].try_into().expect("eight bytes"));
            let info = u64::from_le_bytes(entry[8..16].try_into().expect("eight bytes"));
            let addend = i64::from_le_bytes(entry[16..].try_into().expect("eight bytes"));
            let kind = (info & 0xffff_ffff) as u32;
            let index = (info >> 32) as usize;
            *counts.entry(kind).or_default() += 1;
            if !mapped(target) {
                return Err(fail(format!(
                    "relocation outside mapped segments: {target:#x}"
                )));
            }
            match kind {
                8 => relocations.push((target, 0, addend, 0)),
                1 | 6 | 7 => {
                    let symbol = symbols
                        .get(index)
                        .ok_or_else(|| fail("relocation symbol out of range"))?;
                    let value = i128::from(symbol.value) + i128::from(addend);
                    if symbol.section == SHN_ABS {
                        let value = u64::try_from(value)
                            .map_err(|_| fail("absolute relocation out of range"))?;
                        slice_mut(&mut memory, target, 8)?.copy_from_slice(&value.to_le_bytes());
                    } else if symbol.section != 0 {
                        let value = i64::try_from(value)
                            .map_err(|_| fail("relocation value out of range"))?;
                        relocations.push((target, 0, value, 0));
                    } else {
                        let slot = match imports.iter().position(|&i| i == index) {
                            Some(slot) => slot,
                            None => {
                                imports.push(index);
                                imports.len() - 1
                            }
                        };
                        let import_kind = if symbol.kind == STT_OBJECT { 2 } else { 1 };
                        if (import_kind == 1 && addend != 0) || !(0..4096).contains(&addend) {
                            return Err(fail("unsupported import addend"));
                        }
                        relocations.push((target, import_kind, slot as i64, addend));
                    }
                }
                _ => return Err(fail(format!("unsupported relocation {kind}"))),
            }
        }
    }
    let names: Vec<&str> = imports.iter().map(|&i| symbols[i].name.as_str()).collect();

    let mut file = BOOT_MAGIC.to_vec();
    for value in [
        memory.len() as u64,
        image.entry,
        loads.len() as u64,
        relocations.len() as u64,
        names.len() as u64,
        u64::from(init_env_is_ret),
    ] {
        file.extend_from_slice(&value.to_le_bytes());
    }
    for ph in &loads {
        for value in [ph.vaddr, ph.memsz, u64::from(ph.flags)] {
            file.extend_from_slice(&value.to_le_bytes());
        }
    }
    for name in &names {
        if name.len() >= NAME_FIELD {
            return Err(fail("import name too long"));
        }
        let mut field = [0u8; NAME_FIELD];
        field[..name.len()].copy_from_slice(name.as_bytes());
        file.extend_from_slice(&field);
    }
    for (target, kind, value, addend) in &relocations {
        file.extend_from_slice(&target.to_le_bytes());
        file.extend_from_slice(&kind.to_le_bytes());
        file.extend_from_slice(&value.to_le_bytes());
        file.extend_from_slice(&addend.to_le_bytes());
    }
    file.extend_from_slice(&memory);

    let needed = dynamic
        .entries
        .iter()
        .filter(|(tag, _)| *tag == DT_NEEDED)
        .map(|(_, value)| dynamic.string(*value))
        .collect::<Result<Vec<_>>>()?;
    Ok(Boot {
        file,
        needed,
        relocation_counts: counts,
        imports: names.into_iter().map(str::to_owned).collect(),
    })
}

/// What `inspect_libc` in `prepare.py` proves about the dump's `libc.prx`.
#[derive(Debug, Clone, PartialEq)]
struct LibcEvidence {
    /// The `_init_env` export is exactly `RET` (`c3`); never assumed.
    init_env_is_ret: bool,
    report: serde_json::Value,
}

/// `inspect_libc` in `prepare.py`: find the `_init_env` and `std::ios_base::Init` function
/// exports by NID and record their code, so `boot.bin` only claims `_init_env` is `RET` when
/// this dump's libc says so.
fn inspect_libc(source: &[u8]) -> Result<LibcEvidence> {
    let image = load_image(source)?;
    let dynamic = Dynamic::read(&image, false)?;
    let init_env = nid("_init_env");
    let ios_base_init = nid("_ZNSt8ios_base4InitC1Ev");
    let mut report = serde_json::Map::new();
    report.insert("sha256".into(), bundle::sha256_hex(source).into());
    report.insert("init_env_is_ret".into(), false.into());
    let mut init_env_is_ret = false;
    for symbol in dynamic.symbols()? {
        let key = symbol.name.split('#').next().unwrap_or("");
        if (key != init_env && key != ios_base_init)
            || symbol.section == 0
            || symbol.kind != STT_FUNC
        {
            continue;
        }
        let segment = image
            .program_headers
            .iter()
            .find(|ph| {
                ph.kind == PT_LOAD
                    && ph.flags & 1 != 0
                    && ph.vaddr <= symbol.value
                    && symbol.value - ph.vaddr < ph.filesz
            })
            .ok_or_else(|| fail("libc export outside its executable segments"))?;
        let code = bytes_at(
            &image.elf,
            segment.offset + (symbol.value - segment.vaddr),
            symbol.size,
        )?;
        if key == init_env {
            init_env_is_ret = code == [0xc3];
            report.insert("symbol".into(), symbol.name.clone().into());
            report.insert("address".into(), format!("{:#x}", symbol.value).into());
            report.insert("size".into(), symbol.size.into());
            report.insert("bytes".into(), hex(code).into());
            report.insert("init_env_is_ret".into(), init_env_is_ret.into());
        } else {
            report.insert(
                "ios_base_init".into(),
                serde_json::json!({
                    "symbol": symbol.name,
                    "address": format!("{:#x}", symbol.value),
                    "size": symbol.size,
                    "code_sha256": bundle::sha256_hex(code),
                    "name": "_ZNSt8ios_base4InitC1Ev",
                }),
            );
        }
    }
    Ok(LibcEvidence {
        init_env_is_ret,
        report: serde_json::Value::Object(report),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The symbol names `analysis.json` names when an import's NID is one of them
/// (`candidates` in `prepare.py`).
const NAME_HINTS: &[&str] = &[
    "_init_env",
    "atexit",
    "exit",
    "_exit",
    "__cxa_atexit",
    "__cxa_finalize",
    "__cxa_guard_acquire",
    "__cxa_guard_release",
    "__cxa_guard_abort",
    "__stack_chk_guard",
    "__stack_chk_fail",
    "malloc",
    "free",
    "memcpy",
    "memmove",
    "memset",
    "memcmp",
    "strlen",
    "printf",
    "_ZNSt8ios_base4InitC1Ev",
    "sceKernelLoadStartModule",
    "sceVideoOutOpen",
    "sceKernelCreateSema",
    "sceKernelWaitSema",
    "sceKernelPollSema",
    "sceKernelSignalSema",
    "sceKernelCancelSema",
    "sceKernelDeleteSema",
    "sceAppContentInitialize",
    "sceAppContentAppParamGetInt",
    "sceAppContentGetAddcontInfoList",
    "pthread_cond_init",
    "sceSysmoduleLoadModule",
    "pthread_mutexattr_init",
    "gettimeofday",
    "scePthreadMutexattrInit",
    "scePthreadMutexattrSettype",
    "scePthreadMutexattrDestroy",
    "scePthreadMutexInit",
    "scePthreadMutexLock",
    "scePthreadMutexTrylock",
    "scePthreadMutexUnlock",
    "scePthreadMutexDestroy",
    "scePthreadRwlockInit",
    "scePthreadRwlockDestroy",
    "scePthreadRwlockRdlock",
    "scePthreadRwlockWrlock",
    "scePthreadRwlockTryrdlock",
    "scePthreadRwlockTrywrlock",
    "scePthreadRwlockUnlock",
    "scePthreadRwlockTimedrdlock",
    "scePthreadRwlockTimedwrlock",
    "sceKernelGetDirectMemorySize",
    "sceKernelAllocateDirectMemory",
    "sceKernelMapDirectMemory",
    "sceKernelReleaseDirectMemory",
    "sceKernelMunmap",
];

/// `analysis.json` (the report of `prepare.py`): the source's hash and size, `param.sfo`, the
/// entry, image size, program headers, needed modules, relocation counts, imports with name
/// hints, the libc evidence, the bundled module names and the resource counts. Text keeps its
/// UTF-8 (`ensure_ascii=False`), so a title's trademark sign stays as it is.
fn analysis_json(
    dump_dir: &Path,
    source: &[u8],
    values: &BTreeMap<String, SfoValue>,
    image: &Image,
    boot: &Boot,
    libc: &LibcEvidence,
) -> Result<String> {
    let sfo: serde_json::Map<String, serde_json::Value> = values
        .iter()
        .map(|(key, value)| {
            let value = match value {
                SfoValue::Int(n) => serde_json::Value::from(*n),
                SfoValue::Text(text) => serde_json::Value::from(text.as_str()),
            };
            (key.clone(), value)
        })
        .collect();
    let program_headers: Vec<serde_json::Value> = image
        .program_headers
        .iter()
        .map(|ph| {
            serde_json::json!({
                "type": ph.kind,
                "flags": ph.flags,
                "offset": ph.offset,
                "vaddr": ph.vaddr,
                "filesz": ph.filesz,
                "memsz": ph.memsz,
                "align": ph.align,
            })
        })
        .collect();
    let known: BTreeMap<String, &str> = NAME_HINTS.iter().map(|name| (nid(name), *name)).collect();
    let hints: serde_json::Map<String, serde_json::Value> = boot
        .imports
        .iter()
        .filter_map(|name| {
            let key = name.split('#').next().unwrap_or("");
            known.get(key).map(|hint| (name.clone(), (*hint).into()))
        })
        .collect();
    let counts: serde_json::Map<String, serde_json::Value> = boot
        .relocation_counts
        .iter()
        .map(|(kind, count)| (kind.to_string(), (*count).into()))
        .collect();
    let mut bundled = Vec::new();
    if let Ok(entries) = fs::read_dir(dump_dir.join("sce_module")) {
        for entry in entries {
            bundled.push(entry?.file_name().to_string_lossy().into_owned());
        }
    }
    bundled.sort();
    let mut resources = BTreeMap::<String, u64>::new();
    let mut resource_bytes = 0u64;
    let root = dump_dir.join("dvdroot_ps4");
    if root.is_dir() {
        for top in fs::read_dir(&root)? {
            let top = top?;
            let name = top.file_name().to_string_lossy().into_owned();
            let mut stack = vec![top.path()];
            while let Some(path) = stack.pop() {
                let meta = fs::metadata(&path)?;
                if meta.is_dir() {
                    for child in fs::read_dir(&path)? {
                        stack.push(child?.path());
                    }
                } else if meta.is_file() {
                    *resources.entry(name.clone()).or_default() += 1;
                    resource_bytes += meta.len();
                }
            }
        }
    }
    let report = serde_json::json!({
        "source_sha256": bundle::sha256_hex(source),
        "source_bytes": source.len(),
        "sfo": sfo,
        "entry": format!("{:#x}", image.entry),
        "image_bytes": image.bytes.len(),
        "program_headers": program_headers,
        "needed": boot.needed,
        "relocation_counts": counts,
        "import_count": boot.imports.len(),
        "imports": boot.imports,
        "import_name_hints": hints,
        "libc_evidence": libc.report,
        "bundled_modules": bundled,
        "resources": resources,
        "resource_bytes": resource_bytes,
        "status": "Prepared only; execution and Vulkan are tested separately.",
    });
    let mut text = serde_json::to_string_pretty(&report).expect("a JSON value serializes");
    text.push('\n');
    Ok(text)
}

/// SHA-1 (FIPS 180-4), for [`nid`] only.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64).wrapping_mul(8).to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (word, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*bytes);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d, e]) {
            *s = s.wrapping_add(v);
        }
    }
    let mut digest = [0u8; 20];
    for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *out = word.to_be_bytes();
    }
    digest
}

#[cfg(test)]
mod tests;

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
        data[..4].copy_from_slice(b"\x4f\x15=\x1d");
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
    fn accepts_file_only_segments_with_zero_memory_size() {
        // A PT_SCE_DYNLIBDATA header as real executables carry it: file bytes, p_memsz 0.
        let mut fixture = vec![0; 0x1110];
        elf_header(&mut fixture, 0, 2, 0);
        program_header(&mut fixture, 64, 1, 0x1000, 0, 16, 32);
        program_header(&mut fixture, 120, 0x6100_0000, 0x1010, 0, 0x100, 0);
        fixture[0x1010] = 0xaa;
        let image = load_image(&fixture).unwrap();
        assert_eq!(image.elf[0x1010], 0xaa);
        assert_eq!(image.bytes.len(), 32);
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
