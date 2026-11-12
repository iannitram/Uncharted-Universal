//! `link_modules` against a hand-built synthetic game: a plain-ELF executable, `libc.prx` and
//! `libSceFios2.prx` with SCE dynamic tables, and the `BBPROBE2` boot image `prepare` writes.
//! No game data. `scripts/link_modules.py` has no tests of its own, so the expected values are
//! worked out by hand from the script (see `good_link_matches_the_hand_checked_image`); the
//! error cases port `tests/test_link_libc.py`. The fixture builders are `pub(crate)` so
//! `prepare`'s tests can build a whole synthetic dump from them.

use super::*;
use std::path::PathBuf;

pub(crate) struct Sym {
    pub(crate) name: &'static str,
    pub(crate) kind: u8,
    pub(crate) binding: u8,
    pub(crate) section: u16,
    pub(crate) value: u64,
    pub(crate) size: u64,
}

pub(crate) const fn sym(
    name: &'static str,
    kind: u8,
    binding: u8,
    section: u16,
    value: u64,
) -> Sym {
    Sym {
        name,
        kind,
        binding,
        section,
        value,
        size: if section != 0 && section != SHN_ABS {
            1
        } else {
            0
        },
    }
}

/// A plain ELF64 with one PT_LOAD at vaddr 0 and the SCE dynamic tables. File layout: headers
/// at 0, the load segment's bytes at 0x1000, PT_SCE_DYNLIBDATA (strings, symbols, PLT
/// relocations, relocations) at 0x2000 with `p_memsz` 0, PT_DYNAMIC at 0x3000.
#[derive(Default)]
pub(crate) struct Elf {
    pub(crate) code: Vec<u8>,
    pub(crate) memsz: u64,
    pub(crate) flags: u32,
    /// vaddr, filesz, memsz, align.
    pub(crate) tls: Option<[u64; 4]>,
    pub(crate) procparam: Option<u64>,
    /// Dynamic tag, ID, version, name.
    pub(crate) ids: Vec<(u64, u64, u64, &'static str)>,
    pub(crate) symbols: Vec<Sym>,
    pub(crate) jmprel: Vec<(u64, u32, u32, i64)>,
    pub(crate) rela: Vec<(u64, u32, u32, i64)>,
    pub(crate) init: Option<u64>,
}

fn put(data: &mut [u8], at: usize, bytes: &[u8]) {
    data[at..at + bytes.len()].copy_from_slice(bytes);
}

impl Elf {
    pub(crate) fn bytes(&self) -> Vec<u8> {
        let mut strings = vec![0u8];
        let mut intern = |s: &str| -> u64 {
            if s.is_empty() {
                return 0;
            }
            let at = strings.len() as u64;
            strings.extend_from_slice(s.as_bytes());
            strings.push(0);
            at
        };
        let id_names: Vec<u64> = self.ids.iter().map(|id| intern(id.3)).collect();
        let sym_names: Vec<u64> = self.symbols.iter().map(|s| intern(s.name)).collect();
        while strings.len() % 8 != 0 {
            strings.push(0);
        }
        let mut blob = strings.clone();
        let symtab = blob.len() as u64;
        for (s, name) in self.symbols.iter().zip(&sym_names) {
            blob.extend_from_slice(&(*name as u32).to_le_bytes());
            blob.push((s.binding << 4) | s.kind);
            blob.push(0);
            blob.extend_from_slice(&s.section.to_le_bytes());
            blob.extend_from_slice(&s.value.to_le_bytes());
            blob.extend_from_slice(&s.size.to_le_bytes());
        }
        let mut table = |relocs: &[(u64, u32, u32, i64)]| {
            let at = blob.len() as u64;
            for &(target, kind, symbol, addend) in relocs {
                blob.extend_from_slice(&target.to_le_bytes());
                blob.extend_from_slice(
                    &((u64::from(symbol) << 32) | u64::from(kind)).to_le_bytes(),
                );
                blob.extend_from_slice(&addend.to_le_bytes());
            }
            (at, blob.len() as u64 - at)
        };
        let jmprel = table(&self.jmprel);
        let rela = table(&self.rela);
        let mut dynamic = vec![
            (DT_SCE_STRTAB, 0),
            (DT_SCE_STRSZ, strings.len() as u64),
            (DT_SCE_SYMTAB, symtab),
            (DT_SCE_SYMTABSZ, self.symbols.len() as u64 * 24),
            (DT_SCE_JMPREL, jmprel.0),
            (DT_SCE_PLTRELSZ, jmprel.1),
            (DT_SCE_RELA, rela.0),
            (DT_SCE_RELASZ, rela.1),
        ];
        for (&(tag, id, version, _), name) in self.ids.iter().zip(&id_names) {
            dynamic.push((tag, (id << 48) | (version << 32) | name));
        }
        if let Some(init) = self.init {
            dynamic.push((DT_INIT, init));
        }
        dynamic.push((0, 0));
        assert!(self.code.len() <= 0x1000 && blob.len() <= 0x1000);

        // type, flags, offset, vaddr, filesz, memsz, align
        let mut headers = vec![
            (
                PT_LOAD,
                self.flags,
                0x1000,
                0,
                self.code.len() as u64,
                self.memsz,
                0x1000,
            ),
            (PT_SCE_DYNLIBDATA, 4, 0x2000, 0, blob.len() as u64, 0, 16),
            (
                PT_DYNAMIC,
                4,
                0x3000,
                0,
                dynamic.len() as u64 * 16,
                dynamic.len() as u64 * 16,
                8,
            ),
        ];
        if let Some([vaddr, filesz, memsz, align]) = self.tls {
            headers.push((PT_TLS, 4, 0x1000 + vaddr, vaddr, filesz, memsz, align));
        }
        if let Some(vaddr) = self.procparam {
            headers.push((PT_SCE_PROCPARAM, 4, 0x1000 + vaddr, vaddr, 0, 0x40, 8));
        }
        let mut data = vec![0u8; 0x3000 + dynamic.len() * 16];
        put(&mut data, 0, b"\x7fELF\x02\x01\x01");
        put(&mut data, 18, &62u16.to_le_bytes());
        put(&mut data, 32, &64u64.to_le_bytes());
        put(&mut data, 54, &56u16.to_le_bytes());
        put(&mut data, 56, &(headers.len() as u16).to_le_bytes());
        for (i, (kind, flags, offset, vaddr, filesz, memsz, align)) in headers.iter().enumerate() {
            let at = 64 + i * 56;
            put(&mut data, at, &kind.to_le_bytes());
            put(&mut data, at + 4, &flags.to_le_bytes());
            for (field, value) in [
                (8, offset),
                (16, vaddr),
                (32, filesz),
                (40, memsz),
                (48, align),
            ] {
                put(&mut data, at + field, &value.to_le_bytes());
            }
        }
        put(&mut data, 0x1000, &self.code);
        put(&mut data, 0x2000, &blob);
        for (i, (tag, value)) in dynamic.iter().enumerate() {
            put(&mut data, 0x3000 + i * 16, &tag.to_le_bytes());
            put(&mut data, 0x3000 + i * 16 + 8, &value.to_le_bytes());
        }
        data
    }
}

/// A `boot.bin` in `prepare`'s layout: header, segments, names, relocations, image.
fn boot(magic: &[u8; 8], image: &[u8], names: &[&str], relocs: &[(u64, u64, i64, i64)]) -> Vec<u8> {
    let mut data = magic.to_vec();
    for value in [
        image.len() as u64,
        0,
        1,
        relocs.len() as u64,
        names.len() as u64,
        1,
    ] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0u64, image.len() as u64, 5] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    for name in names {
        let mut field = [0u8; 128];
        field[..name.len()].copy_from_slice(name.as_bytes());
        data.extend_from_slice(&field);
    }
    for &(a, b, c, d) in relocs {
        for word in [a, b, c as u64, d as u64] {
            data.extend_from_slice(&word.to_le_bytes());
        }
    }
    data.extend_from_slice(image);
    data
}

const FS: [u8; 9] = FS_LOAD;

/// The executable: imports from libc, libSceFios2, a library nobody ships and
/// libSceLibcInternal, a TLS block and process parameters.
pub(crate) fn eboot() -> Elf {
    let mut code = FS.to_vec();
    code.push(0xc3);
    code.resize(0x40, 0);
    Elf {
        code,
        memsz: 0x1000,
        flags: 5,
        tls: Some([0x800, 0, 0x30, 16]),
        procparam: Some(0x100),
        ids: vec![
            (DT_SCE_IMPORT_LIB, 0, 1, "libc"),
            (DT_SCE_IMPORT_LIB, 1, 1, "libSceFios2"),
            (DT_SCE_IMPORT_LIB, 2, 1, "libSceUnknownX"),
            (DT_SCE_IMPORT_LIB, 3, 1, "libSceLibcInternal"),
            (DT_SCE_NEEDED_MODULE, 0, 1, "libc"),
            (DT_SCE_NEEDED_MODULE, 1, 1, "libSceFios2"),
            (DT_SCE_NEEDED_MODULE, 2, 1, "libSceUnknownX"),
            (DT_SCE_NEEDED_MODULE, 3, 1, "libSceLibcInternal"),
        ],
        ..Elf::default()
    }
}

const BOOT_NAMES: [&str; 4] = [
    "Nmemcpy#A#A",
    "Nopen#B#B",
    "Nmissing#C#C",
    "NinternalFn#D#D",
];

/// libc.prx: two exports, two imports from libkernel, a weak `module_start`, an absolute
/// symbol, a TLS block and one of every relocation type the linker handles.
pub(crate) fn libc() -> Elf {
    let mut code = vec![0u8; 0x100];
    code[0x10] = 0xc3;
    code[0x20] = 0xc3;
    code[0x60..0x68].fill(0xff);
    put(&mut code, 0x80, &FS);
    Elf {
        code,
        memsz: 0x1000,
        flags: 5,
        tls: Some([0x200, 4, 32, 16]),
        init: Some(0x30),
        ids: vec![
            (DT_SCE_EXPORT_LIB, 0, 1, "libc"),
            (DT_SCE_IMPORT_LIB, 1, 1, "libkernel"),
            (DT_SCE_MODULE_INFO, 0, 1, "libc"),
            (DT_SCE_NEEDED_MODULE, 1, 1, "libkernel"),
        ],
        symbols: vec![
            sym("", STT_SECTION, 0, 1, 0),
            sym("Nmemcpy#A#A", STT_FUNC, STB_GLOBAL, 1, 0x10),
            sym("NinternalFn#A#A", STT_FUNC, STB_GLOBAL, 1, 0x20),
            sym("Nkernelfn#B#B", STT_FUNC, STB_GLOBAL, 0, 0),
            sym("Ndata#B#B", STT_OBJECT, STB_GLOBAL, 0, 0),
            sym("module_start", STT_FUNC, STB_WEAK, 0, 0),
            sym("Nabs#A#A", STT_OBJECT, 0, SHN_ABS, 0x1234),
        ],
        jmprel: vec![(0x68, R_X86_64_JUMP_SLOT, 3, 0), (0x70, R_X86_64_64, 4, 8)],
        rela: vec![
            (0x40, R_X86_64_RELATIVE, 0, 0x30),
            (0x48, R_X86_64_DTPMOD64, 0, 0),
            (0x50, R_X86_64_64, 1, 4),
            (0x58, R_X86_64_64, 6, 4),
            (0x60, R_X86_64_GLOB_DAT, 5, 0),
        ],
        ..Elf::default()
    }
}

pub(crate) fn fios() -> Elf {
    Elf {
        code: (1u8..=16).collect(),
        memsz: 0x800,
        flags: 5,
        ids: vec![
            (DT_SCE_EXPORT_LIB, 0, 1, "libSceFios2"),
            (DT_SCE_MODULE_INFO, 0, 1, "libSceFios2"),
        ],
        symbols: vec![
            sym("", 0, 0, 0, 0),
            sym("Nopen#A#A", STT_FUNC, STB_GLOBAL, 1, 8),
        ],
        ..Elf::default()
    }
}

struct Game {
    root: PathBuf,
}

impl Game {
    fn new(name: &str, main: &Elf, modules: &[(&str, &Elf)], boot_bin: &[u8]) -> Game {
        let root =
            std::env::temp_dir().join(format!("uncharted-link-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("game/sce_module")).unwrap();
        std::fs::create_dir_all(root.join("out")).unwrap();
        std::fs::write(root.join("game/eboot.bin"), main.bytes()).unwrap();
        for (file, elf) in modules {
            std::fs::write(root.join("game/sce_module").join(file), elf.bytes()).unwrap();
        }
        std::fs::write(root.join("out/boot.bin"), boot_bin).unwrap();
        Game { root }
    }

    fn standard(name: &str) -> Game {
        Game::new(
            name,
            &eboot(),
            &[("libc.prx", &libc()), ("libSceFios2.prx", &fios())],
            &standard_boot(),
        )
    }

    fn link(&self, modules: &[&str]) -> Result<Linked> {
        link_modules(
            &self.root.join("game"),
            &self.root.join("out"),
            modules,
            "eboot.bin",
        )
    }

    fn output(&self) -> Vec<u8> {
        std::fs::read(self.root.join("out").join(LINKED_FILE)).unwrap()
    }

    fn report(&self) -> serde_json::Value {
        let text = std::fs::read_to_string(self.root.join("out").join(REPORT_FILE)).unwrap();
        assert!(text.ends_with("}\n"));
        serde_json::from_str(&text).unwrap()
    }

    fn has_output(&self) -> bool {
        self.root.join("out").join(LINKED_FILE).exists()
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn boot_image() -> Vec<u8> {
    let mut image = eboot().code;
    image.resize(0x1000, 0);
    image
}

fn standard_boot() -> Vec<u8> {
    boot(&BOOT_MAGIC, &boot_image(), &BOOT_NAMES, &[(0x200, 1, 0, 0)])
}

/// The `BBPROBE5` file read back.
#[derive(Debug)]
struct Probe5 {
    header: Vec<u64>,
    main_tls: Vec<u64>,
    table: Vec<Vec<u64>>,
    bindings: Vec<Vec<u64>>,
    segments: Vec<Vec<u64>>,
    names: Vec<String>,
    relocations: Vec<(u64, u64, i64, i64)>,
    image: Vec<u8>,
}

fn read_probe5(data: &[u8]) -> Probe5 {
    assert_eq!(&data[..8], b"BBPROBE5");
    let mut at = 8;
    let words = |at: &mut usize, n: usize| -> Vec<u64> {
        let out = (0..n).map(|i| word(data, *at + i * 8)).collect();
        *at += n * 8;
        out
    };
    let header = words(&mut at, 7);
    let main_tls = words(&mut at, 4);
    let modules = words(&mut at, 1)[0] as usize;
    let table = (0..modules).map(|_| words(&mut at, 7)).collect();
    let binding_count = words(&mut at, 1)[0] as usize;
    let bindings = (0..binding_count).map(|_| words(&mut at, 3)).collect();
    let segments = (0..header[2] as usize).map(|_| words(&mut at, 3)).collect();
    let mut names = Vec::new();
    for _ in 0..header[4] {
        let field = &data[at..at + 128];
        let len = field.iter().position(|&b| b == 0).unwrap();
        names.push(String::from_utf8(field[..len].to_vec()).unwrap());
        at += 128;
    }
    let mut relocations = Vec::new();
    for _ in 0..header[3] {
        let w = words(&mut at, 4);
        relocations.push((w[0], w[1], w[2] as i64, w[3] as i64));
    }
    let image = data[at..].to_vec();
    assert_eq!(image.len() as u64, header[0]);
    Probe5 {
        header,
        main_tls,
        table,
        bindings,
        segments,
        names,
        relocations,
        image,
    }
}

fn word(image: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(image[at..at + 8].try_into().unwrap())
}

fn prepare_error(result: Result<Linked>) -> String {
    match result {
        Err(Error::Prepare(message)) => message,
        other => panic!("expected Error::Prepare, got {other:?}"),
    }
}

/// Worked by hand from `scripts/link_modules.py`:
/// - boot image 0x1000 bytes, so libc.prx goes at align_64k(0x1000) = 0x10000 (size 0x1000)
///   and libSceFios2.prx at align_64k(0x11000) = 0x20000 (size 0x800); image 0x20800 bytes.
/// - libc has PT_TLS, so it is TLS module 2; Fios has none (module 0, TLS fields 0).
/// - import table: the four boot names in order, rewritten to the runtime spelling
///   (libc -> q#q, libSceFios2 -> F#G, others NID#library), then libc's imports in relocation
///   order (PLT table first): Nkernelfn and Ndata from libkernel (p#J).
/// - bindings: Nmemcpy (0) -> 0x10010 and Nopen (1) -> 0x20008, both functions; the
///   libSceLibcInternal import (3) -> libc's one same-NID export 0x10020. Nmissing,
///   Nkernelfn and Ndata stay unresolved, left to the host runtime.
/// - relocations: the boot one, then JUMP_SLOT (function, import 4), R_X86_64_64 to data
///   (kind 2, import 5, addend 8), RELATIVE (0x10000 + 0x30), R_X86_64_64 to a defined
///   symbol (0x10000 + 0x10 + 4). DTPMOD64 writes 2, the absolute symbol writes 0x1234 + 4,
///   the weak undefined `module_start` writes 0, all into the image.
/// - `mov rax, fs:[0]` rewritten at 0 (eboot) and 0x10080 (libc).
#[test]
fn good_link_matches_the_hand_checked_image() {
    let game = Game::standard("good");
    let linked = game.link(&DEFAULT_MODULES).unwrap();
    assert_eq!(linked.image, game.root.join("out/boot-linked.bin"));
    assert_eq!(linked.modules, ["libc.prx", "libSceFios2.prx"]);

    let probe = read_probe5(&game.output());
    assert_eq!(probe.header, [0x20800, 0, 3, 5, 6, 1, 0x100]);
    assert_eq!(probe.main_tls, [0x800, 0, 0x30, 16]);
    assert_eq!(
        probe.table,
        [
            vec![0x10000, 0x1000, 0x10030, 0x10200, 32, 4, 2],
            vec![0x20000, 0x800, 0x20000, 0, 0, 0, 0],
        ]
    );
    assert_eq!(
        probe.bindings,
        [
            vec![0, 0x10010, 1],
            vec![1, 0x20008, 1],
            vec![3, 0x10020, 1]
        ]
    );
    assert_eq!(
        probe.segments,
        [
            vec![0, 0x1000, 5],
            vec![0x10000, 0x1000, 5],
            vec![0x20000, 0x800, 5]
        ]
    );
    assert_eq!(
        probe.names,
        [
            "Nmemcpy#q#q",
            "Nopen#F#G",
            "Nmissing#libSceUnknownX",
            "NinternalFn#libSceLibcInternal",
            "Nkernelfn#p#J",
            "Ndata#p#J",
        ]
    );
    assert_eq!(
        probe.relocations,
        [
            (0x200, 1, 0, 0),
            (0x10068, 1, 4, 0),
            (0x10070, 2, 5, 8),
            (0x10040, 0, 0x10030, 0),
            (0x10050, 0, 0x10014, 0),
        ]
    );
    let image = &probe.image;
    assert_eq!(image[0], 0x65, "eboot fs: load rewritten to gs:");
    assert_eq!(&image[1..9], &FS[1..]);
    assert_eq!(image[0x10080], 0x65, "libc fs: load rewritten to gs:");
    assert_eq!(image[0x10010], 0xc3);
    assert_eq!(word(image, 0x10048), 2, "TLS module ID is a literal");
    assert_eq!(word(image, 0x10058), 0x1238, "absolute symbol plus addend");
    assert_eq!(word(image, 0x10060), 0, "weak undefined module_start");
    assert_eq!(
        &image[0x20000..0x20010],
        &(1u8..=16).collect::<Vec<_>>()[..]
    );
    assert!(image[0x20010..].iter().all(|&b| b == 0));

    let report = game.report();
    assert_eq!(report["bindings"], 3);
    assert_eq!(report["imports"], 6);
    assert_eq!(report["fs_loads_patched"], 2);
    assert_eq!(
        report["unresolved_imports"],
        serde_json::json!(["Nmissing#libSceUnknownX", "Nkernelfn#p#J", "Ndata#p#J"])
    );
    assert_eq!(
        report["main_tls"],
        serde_json::json!({"vaddr": 2048, "filesz": 0, "memsz": 48, "align": 16})
    );
    assert_eq!(
        report["modules"][0],
        serde_json::json!({
            "file": "libc.prx", "base": "0x10000", "size": 4096, "init": "0x10030",
            "tls_address": "0x10200", "tls_memsz": 32, "tls_filesz": 4, "tls_module": 2,
            "tls_relocations": 1, "exports": 2, "sha256": sha256_hex(&libc().bytes()),
        })
    );
    assert_eq!(report["modules"][1]["base"], "0x20000");
    assert_eq!(report["modules"][1]["tls_address"], "0x0");
    assert_eq!(report["modules"][1]["exports"], 1);
}

#[test]
fn wrong_boot_bin_is_refused_before_anything_is_written() {
    for magic in [b"BBPROBE1", b"BBPROBE5"] {
        let game = Game::new(
            "wrong-boot",
            &eboot(),
            &[("libc.prx", &libc()), ("libSceFios2.prx", &fios())],
            &boot(magic, &boot_image(), &BOOT_NAMES, &[]),
        );
        let error = game.link(&DEFAULT_MODULES).unwrap_err();
        assert_eq!(
            error,
            Error::Prepare("linker expects freshly prepared BBPROBE2".into())
        );
        assert_eq!(error.code(), "prepare");
        assert!(!game.has_output());
    }
}

#[test]
fn missing_boot_bin_or_trailer_is_refused() {
    let game = Game::standard("boot-trailer");
    let mut raw = standard_boot();
    raw.push(0);
    std::fs::write(game.root.join("out/boot.bin"), raw).unwrap();
    assert_eq!(
        prepare_error(game.link(&DEFAULT_MODULES)),
        "unexpected boot trailer"
    );
    std::fs::remove_file(game.root.join("out/boot.bin")).unwrap();
    assert!(prepare_error(game.link(&DEFAULT_MODULES)).starts_with("boot.bin"));
    assert!(!game.has_output());
}

#[test]
fn missing_module_is_named() {
    let game = Game::standard("missing-module");
    assert_eq!(
        prepare_error(game.link(&["libc.prx", "libSceNgs2.prx"])),
        "sce_module/libSceNgs2.prx is missing"
    );
    assert!(!game.has_output());
}

#[test]
fn unresolved_import_is_left_to_the_host_runtime() {
    // Same NID, wrong library version: no binding (test_same_nid_wrong_library_version_does_not_bind).
    let mut main = eboot();
    main.ids[0] = (DT_SCE_IMPORT_LIB, 0, 2, "libc");
    let game = Game::new(
        "unresolved",
        &main,
        &[("libc.prx", &libc()), ("libSceFios2.prx", &fios())],
        &standard_boot(),
    );
    game.link(&DEFAULT_MODULES).unwrap();
    let probe = read_probe5(&game.output());
    assert_eq!(probe.bindings, [vec![1, 0x20008, 1], vec![3, 0x10020, 1]]);
    assert_eq!(probe.names[0], "Nmemcpy#q#q");
    let report = game.report();
    assert_eq!(report["bindings"], 2);
    assert_eq!(report["unresolved_imports"][0], "Nmemcpy#q#q");
    assert_eq!(report["unresolved_imports"][1], "Nmissing#libSceUnknownX");
}

#[test]
fn libc_internal_import_needs_exactly_one_libc_candidate() {
    // Without libc.prx the libSceLibcInternal import has no candidate and stays unresolved.
    let game = Game::standard("internal");
    game.link(&["libSceFios2.prx"]).unwrap();
    let probe = read_probe5(&game.output());
    assert_eq!(probe.bindings, [vec![1, 0x10008, 1]]);
    assert_eq!(probe.table, [vec![0x10000, 0x800, 0x10000, 0, 0, 0, 0]]);
    assert_eq!(
        game.report()["unresolved_imports"],
        serde_json::json!([
            "Nmemcpy#q#q",
            "Nmissing#libSceUnknownX",
            "NinternalFn#libSceLibcInternal"
        ])
    );
}

fn libc_with(name: &str, edit: impl FnOnce(&mut Elf)) -> Game {
    let mut module = libc();
    edit(&mut module);
    Game::new(
        name,
        &eboot(),
        &[("libc.prx", &module), ("libSceFios2.prx", &fios())],
        &standard_boot(),
    )
}

#[test]
fn unsupported_tls_symbol_is_rejected() {
    let game = libc_with("libc-edit-1", |m| {
        m.rela[1] = (0x48, R_X86_64_DTPMOD64, 1, 0)
    });
    assert_eq!(
        prepare_error(game.link(&DEFAULT_MODULES)),
        "libc.prx: unsupported external TLS relocation"
    );
    assert!(!game.has_output());
}

#[test]
fn tls_relocation_without_a_tls_block_is_rejected() {
    let game = libc_with("libc-edit-2", |m| m.tls = None);
    assert_eq!(
        prepare_error(game.link(&DEFAULT_MODULES)),
        "libc.prx: unsupported external TLS relocation"
    );
}

#[test]
fn relocation_outside_segments_is_rejected() {
    let game = libc_with("libc-edit-3", |m| {
        m.rela.push((0xffc, R_X86_64_RELATIVE, 0, 0))
    });
    assert_eq!(
        prepare_error(game.link(&DEFAULT_MODULES)),
        "libc.prx: relocation outside image 0xffc"
    );
}

#[test]
fn unsupported_relocation_type_is_rejected() {
    let game = libc_with("libc-edit-4", |m| m.rela.push((0x78, 2, 1, 0)));
    assert_eq!(
        prepare_error(game.link(&DEFAULT_MODULES)),
        "libc.prx: unsupported relocation type 2"
    );
}

#[test]
fn function_import_with_addend_is_rejected() {
    let game = libc_with("libc-edit-5", |m| {
        m.jmprel[0] = (0x68, R_X86_64_JUMP_SLOT, 3, 8)
    });
    assert_eq!(
        prepare_error(game.link(&DEFAULT_MODULES)),
        "libc.prx: unsupported import addend"
    );
}

#[test]
fn same_export_twice_is_ambiguous() {
    let game = Game::standard("ambiguous");
    assert_eq!(
        prepare_error(game.link(&["libc.prx", "libc.prx"])),
        "libc.prx: ambiguous export"
    );
}

#[test]
fn fs_loads_outside_executable_segments_are_kept() {
    let mut main = eboot();
    main.flags = 6;
    let game = Game::new(
        "fs-kept",
        &main,
        &[("libc.prx", &libc()), ("libSceFios2.prx", &fios())],
        &standard_boot(),
    );
    game.link(&DEFAULT_MODULES).unwrap();
    let probe = read_probe5(&game.output());
    assert_eq!(probe.image[0], 0x64);
    assert_eq!(probe.image[0x10080], 0x65);
    assert_eq!(game.report()["fs_loads_patched"], 1);
}

#[test]
fn unknown_library_id_in_a_boot_name_is_refused() {
    let game = Game::new(
        "unknown-id",
        &eboot(),
        &[("libc.prx", &libc()), ("libSceFios2.prx", &fios())],
        &boot(&BOOT_MAGIC, &boot_image(), &["Nx#Z#A"], &[]),
    );
    assert_eq!(
        prepare_error(game.link(&DEFAULT_MODULES)),
        "symbol Nx#Z#A names an unknown library or module ID"
    );
}

#[test]
fn encode_id_matches_the_script() {
    assert_eq!(encode_id(0), "A");
    assert_eq!(encode_id(42), "q");
    assert_eq!(encode_id(63), "-");
    assert_eq!(encode_id(64), "BA");
    assert_eq!(encode_id(0xffff), "P--");
}
