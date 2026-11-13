//! Linking the boot image: `link_modules` resolves the imports of the executable and of the
//! modules it loads (`libc.prx`, `libSceFios2.prx`) into the image; `link_libc` does the same
//! for the older libc-only path. Both read the freshly prepared `boot.bin`
//! (`prepare::BOOT_MAGIC`) and write the linked image next to it. Replace
//! `scripts/link_modules.py` and `scripts/link_libc.py`; their Python tests are the expected
//! results.
//!
//! The module reader (`Module::read`: dynamic tags, the SCE string, symbol and relocation
//! tables, library and module identities) and `patch_fs_loads` are the same in both scripts and
//! are shared here. Symbols match by identity: the NID together with the library and module
//! names and versions the dynamic tags give, never by the dump-local one-letter ID or the NID
//! alone.
//!
//! `link_modules` places every module after the previous one, 64 KiB aligned, after the boot
//! image. The imports of all modules share one import table, so the host runtime can override
//! any function and otherwise binds the native export. Imports of `libSceLibcInternal` are
//! served by `libc.prx`'s same-NID export when there is exactly one. It writes
//! `boot-linked.bin` (`BBPROBE5`, read by `src/probe.c`) and `link.json` (the report).

use crate::error::{Error, Result};
use crate::prepare::{load_image, ProgramHeader, BOOT_MAGIC, MAX_IMAGE_BYTES};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The modules linked by default (`DEFAULT_MODULES` in `scripts/link_modules.py`).
pub const DEFAULT_MODULES: [&str; 2] = ["libc.prx", "libSceFios2.prx"];

/// The first eight bytes of the image `link_modules` writes.
pub const LINKED_MAGIC: [u8; 8] = *b"BBPROBE5";
/// The file `link_modules` writes next to `boot.bin`.
pub const LINKED_FILE: &str = "boot-linked.bin";
/// The report `link_modules` writes next to `boot.bin`.
pub const REPORT_FILE: &str = "link.json";

/// The first eight bytes of the image `link_libc` writes.
pub const LIBC_MAGIC: [u8; 8] = *b"BBPROBE4";
/// The image `link_libc` writes under `out`; the original `boot.bin` is left as it was.
pub const LIBC_IMAGE: &str = "boot-libc.bin";
/// The report `link_libc` writes under `out`.
pub const LIBC_REPORT: &str = "libc-link.json";

/// What a link produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    /// The linked boot image written under `out`.
    pub image: PathBuf,
    /// The modules that were linked in, by file name.
    pub modules: Vec<String>,
}

/// Link `modules` (file names under `<game>/sce_module/`) and the executable `exe` into the
/// boot image in `out`. A `boot.bin` that is not `BBPROBE2` is
/// `Error::Prepare("linker expects freshly prepared BBPROBE2")`.
pub fn link_modules(game: &Path, out: &Path, modules: &[&str], exe: &str) -> Result<Linked> {
    let main = Module::read(&read_input(&game.join(exe), exe)?).map_err(|e| prefixed(exe, e))?;
    let boot = Boot::parse(&read_input(
        &out.join("boot.bin"),
        "boot.bin (prepare the executable first)",
    )?)?;
    let mut loaded = Vec::with_capacity(modules.len());
    for &filename in modules {
        let what = format!("sce_module/{filename}");
        let source = read_input(&game.join("sce_module").join(filename), &what)?;
        loaded.push((
            filename,
            Module::read(&source).map_err(|e| prefixed(filename, e))?,
        ));
    }
    let (bytes, report) = link_modules_with(&main, boot, &loaded)?;
    let image = out.join(LINKED_FILE);
    fs::write(&image, bytes)?;
    let mut text = serde_json::to_string_pretty(&report)
        .map_err(|e| Error::Io(format!("{REPORT_FILE}: {e}")))?;
    text.push('\n');
    fs::write(out.join(REPORT_FILE), text)?;
    Ok(Linked {
        image,
        modules: modules.iter().map(|m| m.to_string()).collect(),
    })
}

/// Link `<game>/eboot.bin` against `<game>/sce_module/libc.prx` only, into a separate,
/// reproducible probe image `<out>/boot-libc.bin` (`BBPROBE4`) with its report
/// `<out>/libc-link.json`. `<out>/boot.bin` must be a freshly prepared `BBPROBE2` image and is
/// only read. Same output bytes as `link()` in `scripts/link_libc.py`.
pub fn link_libc(game: &Path, out: &Path) -> Result<Linked> {
    let main = read_linkable(&read_file(&game.join("eboot.bin"))?)?;
    let libc = read_linkable(&read_file(&game.join("sce_module").join("libc.prx"))?)?;
    link_libc_with(&main, &libc, out)
}

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_TLS: u32 = 7;
const PT_SCE_DYNLIBDATA: u32 = 0x6100_0000;
const PT_SCE_PROCPARAM: u32 = 0x6100_0001;
const PT_SCE_RELRO: u32 = 0x6100_0010;

const DT_INIT: u64 = 12;
const DT_SCE_MODULE_INFO: u64 = 0x6100_000d;
const DT_SCE_NEEDED_MODULE: u64 = 0x6100_000f;
const DT_SCE_EXPORT_LIB: u64 = 0x6100_0013;
const DT_SCE_IMPORT_LIB: u64 = 0x6100_0015;
// The PLT (jump slot) relocation table comes first, then the other relocations, in both
// scripts. Values as in the public PS4 ELF documentation (OpenOrbis "PS4 ELF Specification",
// GPCS4 `elf-sce.h`, shadPS4 `elf.h`); `dt_sce_tags_have_the_documented_values` pins them.
const DT_SCE_JMPREL: u64 = 0x6100_0029;
const DT_SCE_PLTRELSZ: u64 = 0x6100_002d;
const DT_SCE_RELA: u64 = 0x6100_002f;
const DT_SCE_RELASZ: u64 = 0x6100_0031;
const DT_SCE_STRTAB: u64 = 0x6100_0035;
const DT_SCE_STRSZ: u64 = 0x6100_0037;
const DT_SCE_SYMTAB: u64 = 0x6100_0039;
const DT_SCE_SYMTABSZ: u64 = 0x6100_003f;

const SHN_ABS: u16 = 0xfff1;
const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;
const STT_SECTION: u8 = 3;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;

const R_X86_64_64: u32 = 1;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;
const R_X86_64_DTPMOD64: u32 = 16;

/// Relocation kinds in the boot images (`src/probe.c`): 0 a plain address, 1 a function import
/// (a call trap), 2 a data import (a data trap page).
const KIND_ADDRESS: u64 = 0;
const KIND_FUNCTION: u64 = 1;
const KIND_DATA: u64 = 2;

/// The boot-image import kind of a symbol: data for STT_OBJECT, a function otherwise.
fn import_kind(symbol: &Symbol) -> u64 {
    if symbol.kind == STT_OBJECT {
        KIND_DATA
    } else {
        KIND_FUNCTION
    }
}

/// libc is TLS module 2 in the probe (the executable is 1).
const TLS_MODULE_ID: u64 = 2;
/// The largest executable TLS block the probe runtime accepts.
const MAX_MAIN_TLS: u64 = 1024 * 1024;
/// `mov rax, fs:[0]`: the executable's initial-exec thread pointer load.
const FS_LOAD: [u8; 9] = [0x64, 0x48, 0x8b, 0x04, 0x25, 0x00, 0x00, 0x00, 0x00];
/// The GS segment prefix that replaces FS in `FS_LOAD`.
const GS_PREFIX: u8 = 0x65;
/// Import names are stored in fixed 128-byte fields, NUL padded.
const NAME_FIELD: usize = 128;

fn fail(message: impl Into<String>) -> Error {
    Error::Prepare(message.into())
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))
}

/// Read an input of `link_modules`; a missing one is a plain `Prepare` error naming `what`.
fn read_input(path: &Path, what: &str) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            fail(format!("{what} is missing"))
        } else {
            Error::Io(format!("{what}: {e}"))
        }
    })
}

fn prefixed(what: &str, error: Error) -> Error {
    match error {
        Error::Prepare(m) => Error::Prepare(format!("{what}: {m}")),
        other => other,
    }
}

/// Round up to the next 64 KiB.
fn align_64k(value: u64) -> u64 {
    value.saturating_add(0xffff) & !0xffff
}

/// Library/module codes are assigned per executable. The host runtime spells imports with the
/// codes of the executable it was first written for (Bloodborne CUSA03173); `link_modules`
/// rewrites every game's imports to that spelling by library name, and to `NID#library`
/// otherwise.
const RUNTIME_SPELLING: &[(&str, &str, &str)] = &[
    ("libSceAjm", "N", "O"),
    ("libSceAppContent", "c", "d"),
    ("libSceAudioIn", "Y", "Z"),
    ("libSceAudioOut", "G", "H"),
    ("libSceAvPlayer", "m", "n"),
    ("libSceCommonDialog", "Z", "a"),
    ("libSceFios2", "F", "G"),
    ("libSceGameLiveStreaming", "e", "f"),
    ("libSceGnmDriver", "B", "C"),
    ("libSceHttp", "K", "L"),
    ("libSceImeDialog", "b", "c"),
    ("libSceMouse", "U", "V"),
    ("libSceMsgDialog", "j", "k"),
    ("libSceNet", "E", "F"),
    ("libSceNetCtl", "J", "K"),
    ("libSceNpAuth", "X", "Y"),
    ("libSceNpCommerce", "k", "l"),
    ("libSceNpCommon", "Q", "R"),
    ("libSceNpManager", "R", "S"),
    ("libSceNpMatching2", "P", "Q"),
    ("libSceNpProfileDialog", "h", "i"),
    ("libSceNpScore", "o", "p"),
    ("libSceNpSignaling", "T", "U"),
    ("libSceNpTrophy", "l", "m"),
    ("libSceNpUtility", "S", "T"),
    ("libSceNpWebApi", "W", "X"),
    ("libScePad", "A", "B"),
    ("libScePlayGo", "n", "o"),
    ("libScePosix", "I", "J"),
    ("libSceRtc", "H", "I"),
    ("libSceSaveData", "O", "P"),
    ("libSceSaveDataDialog", "d", "e"),
    ("libSceScreenShot", "g", "h"),
    ("libSceSsl", "L", "M"),
    ("libSceSysmodule", "M", "N"),
    ("libSceSystemService", "V", "W"),
    ("libSceUserService", "D", "E"),
    ("libSceVideoOut", "C", "D"),
    ("libSceVideoRecording", "f", "g"),
    ("libSceVoice", "i", "j"),
    ("libSceWebBrowserDialog", "a", "b"),
    ("libc", "q", "q"),
    ("libkernel", "p", "J"),
];
/// The module a library lives in when it is not the library's own name.
const RUNTIME_MODULE: &[(&str, &str)] = &[
    ("libSceAppContent", "libSceAppContentUtil"),
    ("libScePosix", "libkernel"),
];

/// A library or module as the dynamic tags name it: its name and version.
type Named = (String, u16);

/// What a symbol is, independent of the dump-local IDs in its spelled name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Identity {
    nid: String,
    library: Named,
    module: Named,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Symbol {
    name: String,
    kind: u8,
    binding: u8,
    section: u16,
    value: u64,
    size: u64,
    /// `None` for an unnamed symbol and for a plain name without two `#`.
    identity: Option<Identity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Relocation {
    target: u64,
    kind: u32,
    symbol: u32,
    addend: i64,
}

/// An executable or PRX read for linking: what `module()` returns in both scripts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Module {
    elf: Vec<u8>,
    program_headers: Vec<ProgramHeader>,
    /// The dynamic tags; a repeated tag keeps its last value.
    tags: HashMap<u64, u64>,
    symbols: Vec<Symbol>,
    relocations: Vec<Relocation>,
    /// Dump-local library IDs to the library they name, in tag order.
    libraries: Vec<(String, Named)>,
    /// Dump-local module IDs to the module they name, in tag order.
    modules: Vec<(String, Named)>,
    /// Lowercase hex SHA-256 of the source file.
    sha256: String,
}

/// The ID spelling the dynamic tags use in symbol names (`encode_id` in `link_libc.py`).
fn encode_id(mut value: u64) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-";
    let mut digits = vec![ALPHABET[(value & 63) as usize]];
    value >>= 6;
    while value != 0 {
        digits.push(ALPHABET[(value & 63) as usize]);
        value >>= 6;
    }
    digits.reverse();
    String::from_utf8(digits).expect("ASCII alphabet")
}

fn span<'a>(data: &'a [u8], offset: u64, size: u64, what: &str) -> Result<&'a [u8]> {
    let start = usize::try_from(offset).map_err(|_| fail(format!("{what} out of bounds")))?;
    let len = usize::try_from(size).map_err(|_| fail(format!("{what} out of bounds")))?;
    start
        .checked_add(len)
        .and_then(|end| data.get(start..end))
        .ok_or_else(|| fail(format!("{what} out of bounds")))
}

fn u16_at(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("four bytes"))
}

fn u64_at(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().expect("eight bytes"))
}

/// The fixed-size records of `table`, `size` bytes each; a short last record is an error.
fn records<'a>(
    table: &'a [u8],
    size: usize,
    what: &'a str,
) -> impl Iterator<Item = Result<&'a [u8]>> {
    (0..table.len()).step_by(size).map(move |at| {
        table
            .get(at..at + size)
            .ok_or_else(|| fail(format!("{what} out of bounds")))
    })
}

impl Module {
    /// Read a SELF or plain ELF (through `prepare::load_image`) for linking.
    fn read(source: &[u8]) -> Result<Module> {
        let image = load_image(source)?;
        let elf = image.elf;
        let program_headers = image.program_headers;

        let dynamic_segment = program_headers
            .iter()
            .find(|ph| ph.kind == PT_DYNAMIC)
            .ok_or_else(|| fail("missing dynamic segment"))?;
        let mut dynamic = Vec::new();
        let end = dynamic_segment
            .offset
            .checked_add(dynamic_segment.filesz)
            .ok_or_else(|| fail("dynamic table out of bounds"))?;
        let mut at = dynamic_segment.offset;
        while at < end {
            let entry = span(&elf, at, 16, "dynamic table")?;
            let tag = u64_at(entry, 0);
            if tag == 0 {
                break;
            }
            dynamic.push((tag, u64_at(entry, 8)));
            at += 16;
        }
        let tags: HashMap<u64, u64> = dynamic.iter().copied().collect();
        let tag = |key: u64| {
            tags.get(&key)
                .copied()
                .ok_or_else(|| fail(format!("missing dynamic tag {key:#x}")))
        };

        let dynlib = program_headers
            .iter()
            .find(|ph| ph.kind == PT_SCE_DYNLIBDATA)
            .ok_or_else(|| fail("missing dynamic library data"))?;
        let blob = span(&elf, dynlib.offset, dynlib.filesz, "dynamic library data")?;
        let strings = span(
            blob,
            tag(DT_SCE_STRTAB)?,
            tag(DT_SCE_STRSZ)?,
            "string table",
        )?;
        let string = |offset: u64| -> Result<String> {
            let start = usize::try_from(offset)
                .ok()
                .filter(|start| *start < strings.len())
                .ok_or_else(|| fail("string out of bounds"))?;
            let len = strings[start..]
                .iter()
                .position(|byte| *byte == 0)
                .ok_or_else(|| fail("unterminated string"))?;
            let text = &strings[start..start + len];
            if !text.is_ascii() {
                return Err(fail("non-ASCII string"));
            }
            Ok(String::from_utf8(text.to_vec()).expect("ASCII"))
        };

        let mut libraries: Vec<(String, Named)> = Vec::new();
        let mut modules: Vec<(String, Named)> = Vec::new();
        for (tag, value) in &dynamic {
            let table = match *tag {
                DT_SCE_EXPORT_LIB | DT_SCE_IMPORT_LIB => &mut libraries,
                DT_SCE_MODULE_INFO | DT_SCE_NEEDED_MODULE => &mut modules,
                _ => continue,
            };
            let key = encode_id(value >> 48);
            let named = (
                string(value & 0xffff_ffff)?,
                ((value >> 32) & 0xffff) as u16,
            );
            match table.iter().find(|(known, _)| *known == key) {
                Some((_, known)) if *known != named => {
                    return Err(fail("conflicting module/library IDs"))
                }
                Some(_) => {}
                None => table.push((key, named)),
            }
        }

        let mut module = Module {
            elf: Vec::new(),
            program_headers: Vec::new(),
            tags: HashMap::new(),
            symbols: Vec::new(),
            relocations: Vec::new(),
            libraries,
            modules,
            sha256: crate::bundle::sha256_hex(source),
        };
        let table = span(
            blob,
            tag(DT_SCE_SYMTAB)?,
            tag(DT_SCE_SYMTABSZ)?,
            "symbol table",
        )?;
        for record in records(table, 24, "symbol table") {
            let record = record?;
            let info = record[4];
            let name = string(u64::from(u32_at(record, 0)))?;
            let identity = if name.is_empty() {
                None
            } else {
                module.identity(&name)?
            };
            module.symbols.push(Symbol {
                name,
                kind: info & 15,
                binding: info >> 4,
                section: u16_at(record, 6),
                value: u64_at(record, 8),
                size: u64_at(record, 16),
                identity,
            });
        }
        // The jump slots first, then the other relocations, as both scripts read them.
        for (offset, size) in [
            (DT_SCE_JMPREL, DT_SCE_PLTRELSZ),
            (DT_SCE_RELA, DT_SCE_RELASZ),
        ] {
            let table = span(blob, tag(offset)?, tag(size)?, "relocation table")?;
            for record in records(table, 24, "relocation table") {
                let record = record?;
                let info = u64_at(record, 8);
                module.relocations.push(Relocation {
                    target: u64_at(record, 0),
                    kind: (info & 0xffff_ffff) as u32,
                    symbol: (info >> 32) as u32,
                    addend: u64_at(record, 16) as i64,
                });
            }
        }
        module.elf = elf;
        module.program_headers = program_headers;
        module.tags = tags;
        Ok(module)
    }

    /// The identity of a spelled name `<nid>#<library id>#<module id>`, or `None` for a plain
    /// name (such as `module_start`). An ID the dynamic tags do not define is an error.
    fn identity(&self, name: &str) -> Result<Option<Identity>> {
        let parts: Vec<&str> = name.split('#').collect();
        let [nid, library, module] = parts[..] else {
            return Ok(None);
        };
        let find = |table: &[(String, Named)], key: &str| {
            table
                .iter()
                .find(|(known, _)| known == key)
                .map(|(_, named)| named.clone())
                .ok_or_else(|| {
                    fail(format!(
                        "symbol {name} names an unknown library or module ID"
                    ))
                })
        };
        Ok(Some(Identity {
            nid: nid.to_string(),
            library: find(&self.libraries, library)?,
            module: find(&self.modules, module)?,
        }))
    }

    /// The first program header of `kind`.
    fn first(&self, kind: u32) -> Option<&ProgramHeader> {
        self.program_headers.iter().find(|ph| ph.kind == kind)
    }

    /// `identity` for the libc path, where every name must be spelled with its IDs.
    fn spelled_identity(&self, name: &str) -> Result<Identity> {
        self.identity(name)?
            .ok_or_else(|| fail("unsupported symbol encoding"))
    }
}

/// `Module::read` for `link_libc`, which refuses named symbols without library and module IDs.
fn read_linkable(source: &[u8]) -> Result<Module> {
    let module = Module::read(source)?;
    if module
        .symbols
        .iter()
        .any(|symbol| !symbol.name.is_empty() && symbol.identity.is_none())
    {
        return Err(fail("unsupported symbol encoding"));
    }
    Ok(module)
}

/// Rewrite each `mov rax, fs:[0]` in the executable segments of `program_headers` (placed at
/// `base` in `image`) to GS: glibc owns FS on Linux, and the runtime points GS at each guest
/// thread's TCB. Only this exact 9-byte instruction is rewritten. Returns how many were.
fn patch_fs_loads(image: &mut [u8], program_headers: &[ProgramHeader], base: u64) -> usize {
    let len = image.len() as u64;
    let mut patched = 0;
    for ph in program_headers {
        if ph.kind != PT_LOAD || ph.flags & 1 == 0 {
            continue;
        }
        let start = base.saturating_add(ph.vaddr).min(len) as usize;
        let end = base
            .saturating_add(ph.vaddr)
            .saturating_add(ph.filesz)
            .min(len) as usize;
        let mut at = start;
        while at + FS_LOAD.len() <= end {
            if image[at..at + FS_LOAD.len()] == FS_LOAD {
                image[at] = GS_PREFIX;
                patched += 1;
                at += FS_LOAD.len();
            } else {
                at += 1;
            }
        }
    }
    patched
}

/// A boot-image relocation: target, kind (`KIND_ADDRESS`, `KIND_FUNCTION`, `KIND_DATA`), value
/// (an address, or an import index) and addend.
type BootRelocation = (u64, u64, i64, i64);

fn to_i64(value: i128) -> Result<i64> {
    i64::try_from(value).map_err(|_| fail("relocation value out of range"))
}

fn put_u64(image: &mut [u8], at: u64, value: u64) -> Result<()> {
    let start = usize::try_from(at).map_err(|_| fail("relocation outside image"))?;
    start
        .checked_add(8)
        .and_then(|end| image.get_mut(start..end))
        .ok_or_else(|| fail("relocation outside image"))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

/// The import table: names in boot order, and each identity's import index. A repeated
/// identity keeps its first place and its last index, like the Python dict.
struct Imports {
    names: Vec<String>,
    order: Vec<Identity>,
    index: HashMap<Identity, usize>,
}

/// `link()` in `scripts/link_libc.py`, given the two modules already read.
fn link_libc_with(main: &Module, libc: &Module, out: &Path) -> Result<Linked> {
    let raw = read_file(&out.join("boot.bin"))?;
    let header = span(&raw, 0, 56, "boot header")?;
    if header[..8] != BOOT_MAGIC {
        return Err(fail("linker expects freshly prepared BBPROBE2"));
    }
    let field = |i: usize| u64_at(header, 8 + i * 8);
    let (size, entry, segment_count, reloc_count, name_count, flags) =
        (field(0), field(1), field(2), field(3), field(4), field(5));
    let table = |count: u64, each: u64| {
        count
            .checked_mul(each)
            .ok_or_else(|| fail("boot tables out of bounds"))
    };
    let mut at = 56u64;
    let mut segments: Vec<(u64, u64, u64)> = Vec::new();
    for record in records(
        span(&raw, at, table(segment_count, 24)?, "boot segments")?,
        24,
        "boot segments",
    ) {
        let record = record?;
        segments.push((u64_at(record, 0), u64_at(record, 8), u64_at(record, 16)));
    }
    at += table(segment_count, 24)?;
    let mut imports = Imports {
        names: Vec::new(),
        order: Vec::new(),
        index: HashMap::new(),
    };
    let name_bytes = table(name_count, NAME_FIELD as u64)?;
    for record in records(
        span(&raw, at, name_bytes, "boot names")?,
        NAME_FIELD,
        "boot names",
    ) {
        let record = record?;
        let len = record.iter().position(|b| *b == 0).unwrap_or(NAME_FIELD);
        let name =
            String::from_utf8(record[..len].to_vec()).map_err(|_| fail("invalid import name"))?;
        imports.names.push(name);
    }
    at = at
        .checked_add(name_bytes)
        .ok_or_else(|| fail("boot tables out of bounds"))?;
    let mut relocs: Vec<BootRelocation> = Vec::new();
    let reloc_bytes = table(reloc_count, 32)?;
    for record in records(
        span(&raw, at, reloc_bytes, "boot relocations")?,
        32,
        "boot relocations",
    ) {
        let record = record?;
        relocs.push((
            u64_at(record, 0),
            u64_at(record, 8),
            u64_at(record, 16) as i64,
            u64_at(record, 24) as i64,
        ));
    }
    at = at
        .checked_add(reloc_bytes)
        .ok_or_else(|| fail("boot tables out of bounds"))?;
    let mut image = span(&raw, at, size, "boot image")?.to_vec();
    if at + size != raw.len() as u64 {
        return Err(fail("unexpected boot trailer"));
    }

    let base = size
        .checked_add(65535)
        .ok_or_else(|| fail("linked image exceeds probe limit"))?
        & !65535;
    let loads: Vec<&ProgramHeader> = libc
        .program_headers
        .iter()
        .filter(|ph| matches!(ph.kind, PT_LOAD | PT_SCE_RELRO))
        .collect();
    let mut libsize = 0u64;
    for ph in &loads {
        let end = ph
            .vaddr
            .checked_add(ph.memsz)
            .ok_or_else(|| fail("linked image exceeds probe limit"))?;
        libsize = libsize.max(end);
    }
    if loads.is_empty() {
        return Err(fail("libc has no load segments"));
    }
    if base.saturating_add(libsize) > MAX_IMAGE_BYTES {
        return Err(fail("linked image exceeds probe limit"));
    }
    image.resize((base + libsize) as usize, 0);
    let mapped = |address: u64, size: u64| {
        loads.iter().any(|ph| {
            ph.vaddr <= address
                && address
                    .checked_add(size)
                    .is_some_and(|end| end <= ph.vaddr + ph.memsz)
        })
    };
    for ph in &loads {
        if ph.filesz > ph.memsz {
            return Err(fail("invalid libc segment size"));
        }
        let source = span(&libc.elf, ph.offset, ph.filesz, "libc segment")?;
        let start = (base + ph.vaddr) as usize;
        image[start..start + source.len()].copy_from_slice(source);
        segments.push((base + ph.vaddr, ph.memsz, u64::from(ph.flags)));
    }

    // Keep the executable's own spelling for the host resolver. A new import gets that
    // spelling only when library and module, names and versions, both match the executable's.
    for (i, name) in imports.names.iter().enumerate() {
        let identity = main.spelled_identity(name)?;
        if !imports.index.contains_key(&identity) {
            imports.order.push(identity.clone());
        }
        imports.index.insert(identity, i);
    }
    let invert = |table: &[(String, Named)]| {
        let mut inverse: HashMap<Named, String> = HashMap::new();
        for (key, named) in table {
            inverse.insert(named.clone(), key.clone());
        }
        inverse
    };
    let main_libraries = invert(&main.libraries);
    let main_modules = invert(&main.modules);
    let imported = |imports: &mut Imports, symbol: &Symbol| -> Result<usize> {
        let identity = symbol
            .identity
            .as_ref()
            .ok_or_else(|| fail("unsupported libc import"))?;
        if let Some(index) = imports.index.get(identity) {
            return Ok(*index);
        }
        let name = match (
            main_libraries.get(&identity.library),
            main_modules.get(&identity.module),
        ) {
            (Some(library), Some(module)) => format!("{}#{library}#{module}", identity.nid),
            _ => format!("libc:{}", symbol.name),
        };
        let index = imports.names.len();
        imports.names.push(name);
        imports.order.push(identity.clone());
        imports.index.insert(identity.clone(), index);
        Ok(index)
    };

    let symbol_of = |relocation: &Relocation| {
        libc.symbols
            .get(relocation.symbol as usize)
            .ok_or_else(|| fail("libc relocation symbol out of range"))
    };
    let mut tls_relocations = 0u64;
    for relocation in &libc.relocations {
        if !mapped(relocation.target, 8) {
            return Err(fail(format!(
                "libc relocation outside image {:#x}",
                relocation.target
            )));
        }
        let target = base + relocation.target;
        let addend = relocation.addend;
        match relocation.kind {
            R_X86_64_RELATIVE => {
                relocs.push((
                    target,
                    KIND_ADDRESS,
                    to_i64(i128::from(base) + i128::from(addend))?,
                    0,
                ));
            }
            R_X86_64_DTPMOD64 => {
                // The descriptor offset is already in the adjacent word; __tls_get_addr uses a
                // separate TLS block for libc.
                let symbol = symbol_of(relocation)?;
                if !symbol.name.is_empty() || symbol.kind != STT_SECTION || addend != 0 {
                    return Err(fail("unsupported external TLS relocation"));
                }
                put_u64(&mut image, target, TLS_MODULE_ID)?;
                tls_relocations += 1;
            }
            R_X86_64_64 | R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                let symbol = symbol_of(relocation)?;
                let value = i128::from(symbol.value) + i128::from(addend);
                if symbol.section == SHN_ABS {
                    let value =
                        u64::try_from(value).map_err(|_| fail("relocation value out of range"))?;
                    put_u64(&mut image, target, value)?;
                } else if symbol.section != 0 {
                    relocs.push((target, KIND_ADDRESS, to_i64(i128::from(base) + value)?, 0));
                } else {
                    let kind = import_kind(symbol);
                    if (kind == KIND_FUNCTION && addend != 0) || !(0..4096).contains(&addend) {
                        return Err(fail("unsupported libc import addend"));
                    }
                    let index = imported(&mut imports, symbol)?;
                    relocs.push((target, kind, index as i64, addend));
                }
            }
            kind => return Err(fail(format!("unsupported libc relocation type {kind}"))),
        }
    }

    let mut exports: HashMap<&Identity, &Symbol> = HashMap::new();
    for symbol in &libc.symbols {
        let Some(identity) = &symbol.identity else {
            continue;
        };
        if symbol.section == 0
            || !matches!(symbol.binding, 1 | 2)
            || !matches!(symbol.kind, STT_FUNC | STT_OBJECT)
        {
            continue;
        }
        if !mapped(symbol.value, symbol.size.max(1)) {
            return Err(fail("export outside libc load segments"));
        }
        if exports.insert(identity, symbol).is_some() {
            return Err(fail("ambiguous libc export"));
        }
    }
    let mut bindings: Vec<(u64, u64, u64)> = Vec::new();
    for identity in &imports.order {
        if let Some(symbol) = exports.get(identity) {
            let address = base
                .checked_add(symbol.value)
                .ok_or_else(|| fail("export outside libc load segments"))?;
            bindings.push((imports.index[identity] as u64, address, import_kind(symbol)));
        }
    }

    let fs_loads_patched = patch_fs_loads(&mut image, &main.program_headers, 0);
    let main_tls = main
        .program_headers
        .iter()
        .find(|ph| ph.kind == PT_TLS)
        .map(|ph| (ph.vaddr, ph.filesz, ph.memsz, ph.align));
    if let Some((vaddr, filesz, memsz, _)) = main_tls {
        if filesz > memsz || memsz > MAX_MAIN_TLS || vaddr.saturating_add(filesz) > size {
            return Err(fail("unsupported eboot TLS layout"));
        }
    }
    let main_tls = main_tls.unwrap_or((0, 0, 0, 0));
    let tls = libc
        .program_headers
        .iter()
        .find(|ph| ph.kind == PT_TLS)
        .ok_or_else(|| fail("libc has no TLS segment"))?;
    if tls.filesz > tls.memsz || !mapped(tls.vaddr, tls.memsz) {
        return Err(fail("unsupported TLS layout"));
    }
    let procparam = main
        .program_headers
        .iter()
        .find(|ph| ph.kind == PT_SCE_PROCPARAM)
        .ok_or_else(|| fail("eboot has no process parameters"))?;
    let init = libc
        .tags
        .get(&DT_INIT)
        .and_then(|init| base.checked_add(*init))
        .ok_or_else(|| fail("libc has no init address"))?;
    let metadata = [
        base,
        libsize,
        init,
        base + tls.vaddr,
        tls.memsz,
        tls.filesz,
        bindings.len() as u64,
        procparam.vaddr,
    ];

    let mut data = Vec::with_capacity(image.len() + 4096);
    data.extend_from_slice(&LIBC_MAGIC);
    let counts = [
        image.len() as u64,
        entry,
        segments.len() as u64,
        relocs.len() as u64,
        imports.names.len() as u64,
        flags,
    ];
    let main_tls_words = [main_tls.0, main_tls.1, main_tls.2, main_tls.3];
    let words = counts
        .iter()
        .chain(&metadata)
        .chain(&main_tls_words)
        .copied()
        .chain(bindings.iter().flat_map(|(a, b, c)| [*a, *b, *c]))
        .chain(segments.iter().flat_map(|(a, b, c)| [*a, *b, *c]));
    for word in words {
        data.extend_from_slice(&word.to_le_bytes());
    }
    for name in &imports.names {
        if name.len() >= NAME_FIELD {
            return Err(fail("name too long"));
        }
        let mut field = [0u8; NAME_FIELD];
        field[..name.len()].copy_from_slice(name.as_bytes());
        data.extend_from_slice(&field);
    }
    for (target, kind, value, addend) in &relocs {
        data.extend_from_slice(&target.to_le_bytes());
        data.extend_from_slice(&kind.to_le_bytes());
        data.extend_from_slice(&value.to_le_bytes());
        data.extend_from_slice(&addend.to_le_bytes());
    }
    data.extend_from_slice(&image);

    let mut relocation_counts: Vec<(String, Json)> = Vec::new();
    for relocation in &libc.relocations {
        let key = relocation.kind.to_string();
        match relocation_counts
            .iter_mut()
            .find(|(known, _)| *known == key)
        {
            Some((_, Json::Int(count))) => *count += 1,
            _ => relocation_counts.push((key, Json::Int(1))),
        }
    }
    let hex = |value: u64| Json::Str(format!("{value:#x}"));
    let int = |value: u64| Json::Int(i128::from(value));
    let report = Json::Object(vec![
        ("base".into(), hex(base)),
        ("size".into(), int(libsize)),
        ("sha256".into(), Json::Str(libc.sha256.clone())),
        ("init".into(), hex(init)),
        ("bindings".into(), int(bindings.len() as u64)),
        ("tls_module_id".into(), int(TLS_MODULE_ID)),
        ("fs_loads_patched".into(), int(fs_loads_patched as u64)),
        (
            "main_tls".into(),
            Json::Object(vec![
                ("vaddr".into(), int(main_tls.0)),
                ("filesz".into(), int(main_tls.1)),
                ("memsz".into(), int(main_tls.2)),
                ("align".into(), int(main_tls.3)),
            ]),
        ),
        ("tls_relocations".into(), int(tls_relocations)),
        ("tls_template_bytes".into(), int(tls.filesz)),
        ("tls_memory_bytes".into(), int(tls.memsz)),
        (
            "imports".into(),
            Json::List(imports.names.iter().cloned().map(Json::Str).collect()),
        ),
        ("relocation_counts".into(), Json::Object(relocation_counts)),
        (
            "symbol_bindings".into(),
            Json::List(
                bindings
                    .iter()
                    .map(|(index, address, kind)| {
                        Json::Object(vec![
                            (
                                "import_name".into(),
                                Json::Str(imports.names[*index as usize].clone()),
                            ),
                            ("address".into(), hex(*address)),
                            ("kind".into(), int(*kind)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]);
    let mut text = String::new();
    report.write(&mut text, 0);
    text.push('\n');

    let image_path = out.join(LIBC_IMAGE);
    let report_path = out.join(LIBC_REPORT);
    fs::write(&image_path, &data)
        .map_err(|e| Error::Io(format!("{}: {e}", image_path.display())))?;
    fs::write(&report_path, text)
        .map_err(|e| Error::Io(format!("{}: {e}", report_path.display())))?;
    Ok(Linked {
        image: image_path,
        modules: vec!["libc.prx".to_string()],
    })
}

/// The import name the host runtime knows an identity by.
fn runtime_name(identity: &Identity) -> String {
    let library = identity.library.0.as_str();
    let home = RUNTIME_MODULE
        .iter()
        .find(|(l, _)| *l == library)
        .map_or(library, |(_, m)| *m);
    match RUNTIME_SPELLING.iter().find(|(l, _, _)| *l == library) {
        Some((_, lib, module)) if identity.module.0 == home => {
            format!("{}#{lib}#{module}", identity.nid)
        }
        _ => format!("{}#{library}", identity.nid),
    }
}

/// A freshly prepared `boot.bin` (`BBPROBE2`): the executable's memory image, its segments,
/// import names and relocations.
#[derive(Debug, Clone)]
struct Boot {
    entry: u64,
    flags: u64,
    segments: Vec<[u64; 3]>,
    names: Vec<String>,
    relocations: Vec<(u64, u64, i64, i64)>,
    image: Vec<u8>,
}

impl Boot {
    fn parse(raw: &[u8]) -> Result<Boot> {
        if raw.get(..8) != Some(&BOOT_MAGIC[..]) {
            return Err(fail("linker expects freshly prepared BBPROBE2"));
        }
        let field = |i: u64| span(raw, 8 + i * 8, 8, "boot header").map(|w| u64_at(w, 0));
        let size = field(0)?;
        let entry = field(1)?;
        let (segment_count, relocation_count, name_count) = (field(2)?, field(3)?, field(4)?);
        let flags = field(5)?;
        let table = |pos: u64, count: u64, width: u64| -> Result<&[u8]> {
            span(
                raw,
                pos,
                count
                    .checked_mul(width)
                    .ok_or_else(|| fail("boot tables out of bounds"))?,
                "boot tables",
            )
        };
        let mut pos = 56u64;
        let segments = table(pos, segment_count, 24)?
            .as_chunks::<24>()
            .0
            .iter()
            .map(|c| {
                [0, 8, 16].map(|at| u64::from_le_bytes(c[at..at + 8].try_into().expect("eight")))
            })
            .collect();
        pos += segment_count * 24;
        let names = table(pos, name_count, NAME_FIELD as u64)?
            .as_chunks::<NAME_FIELD>()
            .0
            .iter()
            .map(|c| {
                let len = c.iter().position(|&b| b == 0).unwrap_or(NAME_FIELD);
                String::from_utf8(c[..len].to_vec()).map_err(|_| fail("invalid import name"))
            })
            .collect::<Result<Vec<_>>>()?;
        pos += name_count * NAME_FIELD as u64;
        let relocations = table(pos, relocation_count, 32)?
            .as_chunks::<32>()
            .0
            .iter()
            .map(|c| {
                let word = |at: usize| u64::from_le_bytes(c[at..at + 8].try_into().expect("eight"));
                (word(0), word(8), word(16) as i64, word(24) as i64)
            })
            .collect();
        pos += relocation_count * 32;
        let image = span(raw, pos, size, "boot image")?.to_vec();
        if pos + size != raw.len() as u64 {
            return Err(fail("unexpected boot trailer"));
        }
        Ok(Boot {
            entry,
            flags,
            segments,
            names,
            relocations,
            image,
        })
    }
}

/// `link_modules`' import table in first-seen order: an identity keeps its first position, and a later
/// name with the same identity takes over its index (the scripts' dict).
#[derive(Default)]
struct ImportTable {
    order: Vec<(Option<Identity>, usize)>,
    position: HashMap<Option<Identity>, usize>,
}

impl ImportTable {
    fn set(&mut self, key: Option<Identity>, index: usize) {
        match self.position.get(&key) {
            Some(&at) => self.order[at].1 = index,
            None => {
                self.position.insert(key.clone(), self.order.len());
                self.order.push((key, index));
            }
        }
    }

    fn get(&self, key: &Option<Identity>) -> Option<usize> {
        self.position.get(key).map(|&at| self.order[at].1)
    }
}

/// One module's row of the linked image's module table.
struct Placed {
    file: String,
    base: u64,
    size: u64,
    init: u64,
    tls_address: u64,
    tls_memsz: u64,
    tls_filesz: u64,
    tls_module: u64,
    tls_relocations: u64,
    exports: u64,
    sha256: String,
}

/// An export: its address in the linked image and its import kind.
#[derive(Clone)]
struct Export {
    address: u64,
    kind: u64,
}

fn q(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn signed(value: i128, what: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| fail(format!("{what}: relocation value out of range")))
}

/// `link()` in `scripts/link_modules.py`: link `main` (whose prepared image is `boot`) with
/// `modules`, in order. Returns the `BBPROBE5` bytes and the `link.json` report.
fn link_modules_with(
    main: &Module,
    boot: Boot,
    modules: &[(&str, Module)],
) -> Result<(Vec<u8>, serde_json::Value)> {
    let Boot {
        entry,
        flags,
        mut segments,
        names: boot_names,
        mut relocations,
        mut image,
    } = boot;
    let size = image.len() as u64;

    let mut imports = ImportTable::default();
    let mut names = Vec::with_capacity(boot_names.len());
    for (index, name) in boot_names.iter().enumerate() {
        let identity = main.identity(name)?;
        names.push(identity.as_ref().map_or_else(|| name.clone(), runtime_name));
        imports.set(identity, index);
    }

    let mut exports: HashMap<Identity, Export> = HashMap::new();
    let mut by_nid: HashMap<(String, String), Vec<Export>> = HashMap::new();
    let mut table = Vec::new();
    let mut base = align_64k(size);
    let mut fs_patched = patch_fs_loads(&mut image, &main.program_headers, 0) as u64;
    let mut tls_module = 2u64;
    for (filename, m) in modules {
        let filename = *filename;
        let err = |m: &str| fail(format!("{filename}: {m}"));
        let loads: Vec<&ProgramHeader> = m
            .program_headers
            .iter()
            .filter(|p| matches!(p.kind, PT_LOAD | PT_SCE_RELRO))
            .collect();
        let modsize = loads
            .iter()
            .map(|p| p.vaddr.saturating_add(p.memsz))
            .max()
            .ok_or_else(|| err("no load segments"))?;
        if base.saturating_add(modsize) > MAX_IMAGE_BYTES {
            return Err(fail("linked image exceeds probe limit"));
        }
        image.resize((base + modsize) as usize, 0);
        let mapped = |address: u64, length: u64| {
            loads.iter().any(|p| {
                p.vaddr <= address
                    && address
                        .checked_add(length)
                        .is_some_and(|end| end <= p.vaddr.saturating_add(p.memsz))
            })
        };
        for p in &loads {
            if p.filesz > p.memsz {
                return Err(err("invalid segment size"));
            }
            let source = span(&m.elf, p.offset, p.filesz, "segment")?;
            let start = (base + p.vaddr) as usize;
            image[start..start + source.len()].copy_from_slice(source);
            segments.push([base + p.vaddr, p.memsz, u64::from(p.flags)]);
        }
        let tls = m.first(PT_TLS);
        let mut module_id = 0;
        if let Some(tls) = tls {
            if tls.filesz > tls.memsz || !mapped(tls.vaddr, tls.memsz) {
                return Err(err("unsupported TLS layout"));
            }
            module_id = tls_module;
            tls_module += 1;
        }
        let symbol = |id: u32| {
            m.symbols
                .get(id as usize)
                .ok_or_else(|| err("relocation names a missing symbol"))
        };
        let mut tls_relocations = 0;
        for r in &m.relocations {
            if !mapped(r.target, 8) {
                return Err(err(&format!("relocation outside image {:#x}", r.target)));
            }
            let target = base + r.target;
            match r.kind {
                R_X86_64_RELATIVE => {
                    let value = signed(i128::from(base) + i128::from(r.addend), filename)?;
                    relocations.push((target, KIND_ADDRESS, value, 0));
                }
                R_X86_64_DTPMOD64 => {
                    let s = symbol(r.symbol)?;
                    if !s.name.is_empty()
                        || s.kind != STT_SECTION
                        || r.addend != 0
                        || module_id == 0
                    {
                        return Err(err("unsupported external TLS relocation"));
                    }
                    put_u64(&mut image, target, module_id)?;
                    tls_relocations += 1;
                }
                R_X86_64_64 | R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                    let s = symbol(r.symbol)?;
                    if s.section == SHN_ABS {
                        let value = u64::try_from(i128::from(s.value) + i128::from(r.addend))
                            .map_err(|_| err("relocation value out of range"))?;
                        put_u64(&mut image, target, value)?;
                    } else if s.section != 0 {
                        let value = i128::from(base) + i128::from(s.value) + i128::from(r.addend);
                        relocations.push((target, KIND_ADDRESS, signed(value, filename)?, 0));
                    } else if s.identity.is_none() && s.binding == STB_WEAK {
                        put_u64(&mut image, target, 0)?; // weak undefined (module_start/stop)
                    } else {
                        let kind = import_kind(s);
                        if (kind == KIND_FUNCTION && r.addend != 0)
                            || !(0..4096).contains(&r.addend)
                        {
                            return Err(err("unsupported import addend"));
                        }
                        let key = s
                            .identity
                            .clone()
                            .ok_or_else(|| err(&format!("unsupported import {:?}", s.name)))?;
                        let key = Some(key);
                        let index = match imports.get(&key) {
                            Some(index) => index,
                            None => {
                                names.push(runtime_name(key.as_ref().expect("identity")));
                                imports.set(key, names.len() - 1);
                                names.len() - 1
                            }
                        };
                        relocations.push((target, kind, index as i64, r.addend));
                    }
                }
                kind => return Err(err(&format!("unsupported relocation type {kind}"))),
            }
        }
        let mut count = 0;
        for s in &m.symbols {
            let Some(identity) = &s.identity else {
                continue;
            };
            if s.section == 0
                || !matches!(s.binding, STB_GLOBAL | STB_WEAK)
                || !matches!(s.kind, STT_OBJECT | STT_FUNC)
            {
                continue;
            }
            if !mapped(s.value, s.size.max(1)) {
                return Err(err("export outside load segments"));
            }
            if exports.contains_key(identity) {
                return Err(err("ambiguous export"));
            }
            let export = Export {
                address: base + s.value,
                kind: import_kind(s),
            };
            exports.insert(identity.clone(), export.clone());
            by_nid
                .entry((identity.nid.clone(), identity.library.0.clone()))
                .or_default()
                .push(export);
            count += 1;
        }
        fs_patched += patch_fs_loads(&mut image, &m.program_headers, base) as u64;
        let init = m.tags.get(&DT_INIT).copied().unwrap_or(0);
        table.push(Placed {
            file: filename.to_string(),
            base,
            size: modsize,
            init: base
                .checked_add(init)
                .ok_or_else(|| err("invalid init address"))?,
            tls_address: tls.map_or(0, |t| base + t.vaddr),
            tls_memsz: tls.map_or(0, |t| t.memsz),
            tls_filesz: tls.map_or(0, |t| t.filesz),
            tls_module: module_id,
            tls_relocations,
            exports: count,
            sha256: m.sha256.clone(),
        });
        base = align_64k(base + modsize);
    }

    let mut bindings = Vec::new();
    let mut unresolved = Vec::new();
    for (identity, index) in &imports.order {
        let mut found = identity.as_ref().and_then(|i| exports.get(i)).cloned();
        if let Some(i) = identity
            .as_ref()
            .filter(|i| i.library.0 == "libSceLibcInternal")
        {
            if found.is_none() {
                found = match by_nid.get(&(i.nid.clone(), "libc".to_string())) {
                    Some(candidates) if candidates.len() == 1 => Some(candidates[0].clone()),
                    _ => None,
                };
            }
        }
        match found {
            Some(export) => bindings.push([*index as u64, export.address, export.kind]),
            None => unresolved.push(names[*index].clone()),
        }
    }

    let main_tls = main.first(PT_TLS);
    let main_tls_values = main_tls.map_or([0; 4], |t| [t.vaddr, t.filesz, t.memsz, t.align]);
    if let Some(t) = main_tls {
        if t.filesz > t.memsz
            || t.memsz > MAX_MAIN_TLS
            || t.vaddr.checked_add(t.filesz).is_none_or(|end| end > size)
        {
            return Err(fail("unsupported eboot TLS layout"));
        }
    }
    let procparam = main
        .first(PT_SCE_PROCPARAM)
        .ok_or_else(|| fail("executable has no process parameters"))?;

    let mut bytes = Vec::with_capacity(image.len() + 4096);
    bytes.extend_from_slice(&LINKED_MAGIC);
    for value in [
        image.len() as u64,
        entry,
        segments.len() as u64,
        relocations.len() as u64,
        names.len() as u64,
        flags,
        procparam.vaddr,
    ] {
        q(&mut bytes, value);
    }
    for value in main_tls_values {
        q(&mut bytes, value);
    }
    q(&mut bytes, table.len() as u64);
    for t in &table {
        for value in [
            t.base,
            t.size,
            t.init,
            t.tls_address,
            t.tls_memsz,
            t.tls_filesz,
            t.tls_module,
        ] {
            q(&mut bytes, value);
        }
    }
    q(&mut bytes, bindings.len() as u64);
    for value in bindings.iter().flatten() {
        q(&mut bytes, *value);
    }
    for value in segments.iter().flatten() {
        q(&mut bytes, *value);
    }
    for name in &names {
        if name.len() >= NAME_FIELD {
            return Err(fail("name too long"));
        }
        let mut field = [0u8; NAME_FIELD];
        field[..name.len()].copy_from_slice(name.as_bytes());
        bytes.extend_from_slice(&field);
    }
    for &(target, kind, value, addend) in &relocations {
        for word in [target, kind, value as u64, addend as u64] {
            q(&mut bytes, word);
        }
    }
    bytes.extend_from_slice(&image);

    let hex = |v: u64| format!("{v:#x}");
    let report = serde_json::json!({
        "modules": table.iter().map(|t| serde_json::json!({
            "file": t.file,
            "base": hex(t.base),
            "size": t.size,
            "init": hex(t.init),
            "tls_address": hex(t.tls_address),
            "tls_memsz": t.tls_memsz,
            "tls_filesz": t.tls_filesz,
            "tls_module": t.tls_module,
            "tls_relocations": t.tls_relocations,
            "exports": t.exports,
            "sha256": t.sha256,
        })).collect::<Vec<_>>(),
        "bindings": bindings.len(),
        "imports": names.len(),
        "fs_loads_patched": fs_patched,
        "main_tls": {
            "vaddr": main_tls_values[0],
            "filesz": main_tls_values[1],
            "memsz": main_tls_values[2],
            "align": main_tls_values[3],
        },
        "unresolved_imports": unresolved,
    });
    Ok((bytes, report))
}

/// The few JSON shapes the link reports hold, written like Python's `json.dumps(indent=2)`
/// (insertion order, ASCII only), so the reports match the scripts' byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Json {
    Int(i128),
    Str(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn write(&self, out: &mut String, indent: usize) {
        let pad = |out: &mut String, depth: usize| out.extend(std::iter::repeat_n(' ', depth));
        match self {
            Json::Int(value) => out.push_str(&value.to_string()),
            Json::Str(value) => write_json_string(out, value),
            Json::List(items) if items.is_empty() => out.push_str("[]"),
            Json::Object(items) if items.is_empty() => out.push_str("{}"),
            Json::List(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    pad(out, indent + 2);
                    item.write(out, indent + 2);
                }
                out.push('\n');
                pad(out, indent);
                out.push(']');
            }
            Json::Object(items) => {
                out.push_str("{\n");
                for (i, (key, value)) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    pad(out, indent + 2);
                    write_json_string(out, key);
                    out.push_str(": ");
                    value.write(out, indent + 2);
                }
                out.push('\n');
                pad(out, indent);
                out.push('}');
            }
        }
    }
}

fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (' '..='~').contains(&c) => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
pub(crate) mod tests;
