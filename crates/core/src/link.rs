//! Linking the boot image: `link_modules` resolves the imports of the executable and of the
//! modules it loads (`libc.prx`, `libSceFios2.prx`) into the image; `link_libc` does the same
//! for the older libc-only path. Both read the freshly prepared `boot.bin`
//! (`prepare::BOOT_MAGIC`) and write the linked image next to it. Replace
//! `scripts/link_modules.py` and `scripts/link_libc.py`; their Python tests are the expected
//! results.
//!
//! `link_modules` places every module after the previous one, 64 KiB aligned, after the boot
//! image. The imports of all modules share one import table, so the host runtime can override
//! any function and otherwise binds the native export. Exports are matched by (NID, library
//! identity, module identity); imports of `libSceLibcInternal` are served by `libc.prx`'s
//! same-NID export when there is exactly one. It writes `boot-linked.bin` (`BBPROBE5`, read
//! by `src/probe.c`) and `link.json` (the report). `link_libc` (roadmap 1.09) lives in
//! `link/libc.rs` with its own module reader, written in parallel with this one.

use crate::bundle::sha256_hex;
use crate::error::{Error, Result};
use crate::prepare::{self, ProgramHeader, BOOT_MAGIC, MAX_IMAGE_BYTES};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The modules linked by default (`DEFAULT_MODULES` in `scripts/link_modules.py`).
pub const DEFAULT_MODULES: [&str; 2] = ["libc.prx", "libSceFios2.prx"];

/// The first eight bytes of the image `link_modules` writes.
pub const LINKED_MAGIC: [u8; 8] = *b"BBPROBE5";
/// The file `link_modules` writes next to `boot.bin`.
pub const LINKED_FILE: &str = "boot-linked.bin";
/// The report `link_modules` writes next to `boot.bin`.
pub const REPORT_FILE: &str = "link.json";

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
    let main = Module::parse(&read(&game.join(exe), exe)?).map_err(|e| prefixed(exe, e))?;
    let boot = Boot::parse(&read(
        &out.join("boot.bin"),
        "boot.bin (prepare the executable first)",
    )?)?;
    let mut loaded = Vec::with_capacity(modules.len());
    for &filename in modules {
        let what = format!("sce_module/{filename}");
        let source = read(&game.join("sce_module").join(filename), &what)?;
        loaded.push((
            filename,
            Module::parse(&source).map_err(|e| prefixed(filename, e))?,
        ));
    }
    let (bytes, report) = link(&main, boot, &loaded)?;
    let image = out.join(LINKED_FILE);
    std::fs::write(&image, bytes)?;
    let mut text = serde_json::to_string_pretty(&report)
        .map_err(|e| Error::Io(format!("{REPORT_FILE}: {e}")))?;
    text.push('\n');
    std::fs::write(out.join(REPORT_FILE), text)?;
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
    libc::link_libc(game, out)
}

mod libc;
pub use libc::{LIBC_IMAGE, LIBC_MAGIC, LIBC_REPORT};

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
const DT_SCE_JMPREL: u64 = 0x6100_0029;
const DT_SCE_PLTRELSZ: u64 = 0x6100_002d;
const DT_SCE_RELA: u64 = 0x6100_002f;
const DT_SCE_RELASZ: u64 = 0x6100_0031;
const DT_SCE_STRTAB: u64 = 0x6100_0035;
const DT_SCE_STRSZ: u64 = 0x6100_0037;
const DT_SCE_SYMTAB: u64 = 0x6100_0039;
const DT_SCE_SYMTABSZ: u64 = 0x6100_003f;

const R_X86_64_64: u32 = 1;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;
const R_X86_64_DTPMOD64: u32 = 16;

const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;
const STT_SECTION: u8 = 3;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;
const SHN_ABS: u16 = 0xfff1;

/// Import kinds in the linked image: 0 is a plain address, 1 a function, 2 data.
const KIND_ADDRESS: u64 = 0;
const KIND_FUNCTION: u64 = 1;
const KIND_DATA: u64 = 2;

/// `mov rax, fs:[0]`: the initial-exec thread-pointer load.
const FS_LOAD: [u8; 9] = [0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0];
const MAX_MAIN_TLS: u64 = 1024 * 1024;
const NAME_BYTES: usize = 128;

/// Library/module codes are assigned per executable. The host runtime spells imports with the
/// codes of the executable it was first written for (Bloodborne CUSA03173); every game's
/// imports are rewritten to that spelling by library name, and to `NID#library` otherwise.
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

fn fail(message: impl Into<String>) -> Error {
    Error::Prepare(message.into())
}

fn prefixed(what: &str, error: Error) -> Error {
    match error {
        Error::Prepare(m) => Error::Prepare(format!("{what}: {m}")),
        other => other,
    }
}

fn read(path: &Path, what: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            fail(format!("{what} is missing"))
        } else {
            Error::Io(format!("{what}: {e}"))
        }
    })
}

/// `data[offset..offset + size]`, or a plain error (`span` in `prepare.py`).
fn span(data: &[u8], offset: u64, size: u64) -> Result<&[u8]> {
    let start = usize::try_from(offset).map_err(|_| fail("out of bounds"))?;
    let len = usize::try_from(size).map_err(|_| fail("out of bounds"))?;
    let end = start
        .checked_add(len)
        .ok_or_else(|| fail("out of bounds"))?;
    data.get(start..end).ok_or_else(|| fail("out of bounds"))
}

fn u64_at(data: &[u8], offset: u64) -> Result<u64> {
    Ok(u64::from_le_bytes(
        span(data, offset, 8)?.try_into().expect("eight bytes"),
    ))
}

fn put_u64(data: &mut [u8], offset: u64, value: u64) -> Result<()> {
    let start = usize::try_from(offset).map_err(|_| fail("out of bounds"))?;
    let slot = start
        .checked_add(8)
        .and_then(|end| data.get_mut(start..end))
        .ok_or_else(|| fail("out of bounds"))?;
    slot.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

/// Round up to the next 64 KiB.
fn align_64k(value: u64) -> u64 {
    value.saturating_add(0xffff) & !0xffff
}

/// A library or module ID as the dynamic table spells it in symbol names: base64-like digits,
/// most significant first (`encode_id` in `link_libc.py`).
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

/// A library or module: its name and version.
type Named = (String, u16);

/// What a symbol stands for across modules: NID, library and module.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Identity {
    nid: String,
    library: Named,
    module: Named,
}

#[derive(Debug, Clone)]
struct Symbol {
    name: String,
    kind: u8,
    binding: u8,
    section: u16,
    value: u64,
    size: u64,
    identity: Option<Identity>,
}

#[derive(Debug, Clone, Copy)]
struct Relocation {
    target: u64,
    kind: u32,
    symbol: u32,
    addend: i64,
}

/// An executable or module with its SCE dynamic tables read (`module()` in the scripts).
#[derive(Debug, Clone)]
struct Module {
    elf: Vec<u8>,
    program_headers: Vec<ProgramHeader>,
    tags: HashMap<u64, u64>,
    symbols: Vec<Symbol>,
    relocations: Vec<Relocation>,
    libraries: HashMap<String, Named>,
    modules: HashMap<String, Named>,
    sha256: String,
}

impl Module {
    fn parse(source: &[u8]) -> Result<Module> {
        let image = prepare::load_image(source)?;
        let elf = image.elf;
        let program_headers = image.program_headers;
        let first = |kind: u32, what: &str| {
            program_headers
                .iter()
                .find(|p| p.kind == kind)
                .cloned()
                .ok_or_else(|| fail(format!("no {what} segment")))
        };
        let dynamic_segment = first(PT_DYNAMIC, "dynamic")?;
        let mut dynamic = Vec::new();
        let end = dynamic_segment
            .offset
            .checked_add(dynamic_segment.filesz)
            .ok_or_else(|| fail("out of bounds"))?;
        let mut pos = dynamic_segment.offset;
        while pos < end {
            let tag = u64_at(&elf, pos)?;
            let value = u64_at(&elf, pos.saturating_add(8))?;
            if tag == 0 {
                break;
            }
            dynamic.push((tag, value));
            pos = pos.saturating_add(16);
        }
        let tags: HashMap<u64, u64> = dynamic.iter().copied().collect();
        let tag = |t: u64| {
            tags.get(&t)
                .copied()
                .ok_or_else(|| fail(format!("missing dynamic tag {t:#x}")))
        };
        let data_segment = first(PT_SCE_DYNLIBDATA, "dynamic library data")?;
        let blob = span(&elf, data_segment.offset, data_segment.filesz)?;
        let strings = span(blob, tag(DT_SCE_STRTAB)?, tag(DT_SCE_STRSZ)?)?;
        let string = |offset: u64| -> Result<String> {
            let rest = usize::try_from(offset)
                .ok()
                .and_then(|start| strings.get(start..))
                .filter(|rest| !rest.is_empty())
                .ok_or_else(|| fail("out of bounds"))?;
            let len = rest
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| fail("unterminated string"))?;
            let text = &rest[..len];
            if !text.is_ascii() {
                return Err(fail("non-ASCII string"));
            }
            Ok(String::from_utf8(text.to_vec()).expect("ASCII"))
        };

        let mut libraries = HashMap::new();
        let mut modules = HashMap::new();
        for &(t, value) in &dynamic {
            let table = match t {
                DT_SCE_EXPORT_LIB | DT_SCE_IMPORT_LIB => &mut libraries,
                DT_SCE_MODULE_INFO | DT_SCE_NEEDED_MODULE => &mut modules,
                _ => continue,
            };
            let key = encode_id(value >> 48);
            let named = (
                string(value & 0xffff_ffff)?,
                ((value >> 32) & 0xffff) as u16,
            );
            if table.get(&key).is_some_and(|known| *known != named) {
                return Err(fail("conflicting module/library IDs"));
            }
            table.insert(key, named);
        }

        let table = span(blob, tag(DT_SCE_SYMTAB)?, tag(DT_SCE_SYMTABSZ)?)?;
        let mut symbols = Vec::new();
        for pos in (0..table.len() as u64).step_by(24) {
            let entry = span(table, pos, 24)?;
            let name_offset = u32::from_le_bytes(entry[0..4].try_into().expect("four bytes"));
            let info = entry[4];
            let name = string(u64::from(name_offset))?;
            let identity = if name.is_empty() {
                None
            } else {
                identity_of(&name, &libraries, &modules)?
            };
            symbols.push(Symbol {
                name,
                kind: info & 15,
                binding: info >> 4,
                section: u16::from_le_bytes([entry[6], entry[7]]),
                value: u64_at(entry, 8)?,
                size: u64_at(entry, 16)?,
                identity,
            });
        }

        let mut relocations = Vec::new();
        for (offset_tag, size_tag) in [
            (DT_SCE_JMPREL, DT_SCE_PLTRELSZ),
            (DT_SCE_RELA, DT_SCE_RELASZ),
        ] {
            let table = span(blob, tag(offset_tag)?, tag(size_tag)?)?;
            for pos in (0..table.len() as u64).step_by(24) {
                let entry = span(table, pos, 24)?;
                let info = u64_at(entry, 8)?;
                relocations.push(Relocation {
                    target: u64_at(entry, 0)?,
                    kind: (info & 0xffff_ffff) as u32,
                    symbol: (info >> 32) as u32,
                    addend: u64_at(entry, 16)? as i64,
                });
            }
        }
        Ok(Module {
            elf,
            program_headers,
            tags,
            symbols,
            relocations,
            libraries,
            modules,
            sha256: sha256_hex(source),
        })
    }

    fn identity(&self, name: &str) -> Result<Option<Identity>> {
        identity_of(name, &self.libraries, &self.modules)
    }

    fn first(&self, kind: u32) -> Option<&ProgramHeader> {
        self.program_headers.iter().find(|p| p.kind == kind)
    }
}

/// `NID#library#module` read through the module's ID tables. Plain names such as
/// `module_start` are not linkable (`None`); an ID the tables do not hold is an error.
fn identity_of(
    name: &str,
    libraries: &HashMap<String, Named>,
    modules: &HashMap<String, Named>,
) -> Result<Option<Identity>> {
    let parts: Vec<&str> = name.split('#').collect();
    let [nid, library, module] = parts[..] else {
        return Ok(None);
    };
    let unknown = || {
        fail(format!(
            "symbol {name} names an unknown library or module ID"
        ))
    };
    Ok(Some(Identity {
        nid: nid.to_string(),
        library: libraries.get(library).cloned().ok_or_else(unknown)?,
        module: modules.get(module).cloned().ok_or_else(unknown)?,
    }))
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
        let field = |i: u64| u64_at(raw, 8 + i * 8);
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
                    .ok_or_else(|| fail("out of bounds"))?,
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
        let names = table(pos, name_count, NAME_BYTES as u64)?
            .as_chunks::<NAME_BYTES>()
            .0
            .iter()
            .map(|c| {
                let len = c.iter().position(|&b| b == 0).unwrap_or(NAME_BYTES);
                String::from_utf8(c[..len].to_vec()).map_err(|_| fail("invalid import name"))
            })
            .collect::<Result<Vec<_>>>()?;
        pos += name_count * NAME_BYTES as u64;
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
        let image = span(raw, pos, size)?.to_vec();
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

/// Rewrite initial-exec `mov rax, fs:[0]` to `gs:` in the executable load segments of a module
/// placed at `base`: glibc owns FS on Linux. Returns how many were rewritten.
fn patch_fs_loads(image: &mut [u8], program_headers: &[ProgramHeader], base: u64) -> u64 {
    let mut patched = 0;
    let len = image.len() as u64;
    for p in program_headers
        .iter()
        .filter(|p| p.kind == PT_LOAD && p.flags & 1 != 0)
    {
        let start = base.saturating_add(p.vaddr);
        let end = start.saturating_add(p.filesz).min(len);
        if start >= end {
            continue;
        }
        let (mut at, end) = (start as usize, end as usize);
        while at + FS_LOAD.len() <= end {
            if image[at..at + FS_LOAD.len()] == FS_LOAD {
                image[at] = 0x65;
                patched += 1;
                at += FS_LOAD.len();
            } else {
                at += 1;
            }
        }
    }
    patched
}

/// The import table in first-seen order: an identity keeps its first position, and a later
/// name with the same identity takes over its index (the scripts' dict).
#[derive(Default)]
struct Imports {
    order: Vec<(Option<Identity>, usize)>,
    position: HashMap<Option<Identity>, usize>,
}

impl Imports {
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

/// Link `main` (whose prepared image is `boot`) with `modules`, in order. Returns the
/// `BBPROBE5` bytes and the `link.json` report.
fn link(
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

    let mut imports = Imports::default();
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
    let mut fs_patched = patch_fs_loads(&mut image, &main.program_headers, 0);
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
            let source = span(&m.elf, p.offset, p.filesz)?;
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
                        let kind = if s.kind == STT_OBJECT {
                            KIND_DATA
                        } else {
                            KIND_FUNCTION
                        };
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
                kind: if s.kind == STT_OBJECT {
                    KIND_DATA
                } else {
                    KIND_FUNCTION
                },
            };
            exports.insert(identity.clone(), export.clone());
            by_nid
                .entry((identity.nid.clone(), identity.library.0.clone()))
                .or_default()
                .push(export);
            count += 1;
        }
        fs_patched += patch_fs_loads(&mut image, &m.program_headers, base);
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
        if name.len() >= NAME_BYTES {
            return Err(fail("name too long"));
        }
        let mut field = [0u8; NAME_BYTES];
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

#[cfg(test)]
mod tests;
