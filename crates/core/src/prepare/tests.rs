//! `tests/test_prepare.py` ported (the SELF reader on its fixtures, the libc `RET` contract,
//! `nid`, the UTF-8 title in `analysis.json`), then `prepare` and `locate` end to end on a
//! hand-built synthetic dump: a plain-ELF `eboot.bin` with imports and every relocation kind
//! `boot.bin` carries, `libc.prx` and `libSceFios2.prx` from `link`'s fixtures, a `param.sfo` and
//! a patch folder. No game data. The revision gate is opened with an explicit table because
//! `games::SUPPORTED` is still empty.

use super::*;
use crate::link::tests::link_modules::{eboot, fios, libc, sym, Elf};
use std::collections::BTreeSet;

const STT_SECTION: u8 = 3;
const STB_GLOBAL: u8 = 1;
const R_X86_64_64: u32 = 1;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;

fn put_u16(data: &mut [u8], at: usize, value: u16) {
    data[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put_u32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put_u64(data: &mut [u8], at: usize, value: u64) {
    data[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn word(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}

/// `'<16sHHIQQQIHHHHHH'` at `at`: the ELF header of the Python fixtures.
fn elf_header(data: &mut [u8], at: usize, kind: u16, phnum: u16) {
    data[at..at + 8].copy_from_slice(b"\x7fELF\x02\x01\x01\x09");
    put_u16(data, at + 16, kind);
    put_u16(data, at + 18, 62);
    put_u32(data, at + 20, 1);
    put_u64(data, at + 32, 64);
    put_u16(data, at + 52, 64);
    put_u16(data, at + 54, 56);
    put_u16(data, at + 56, phnum);
}

/// `'<IIQQQQQQ'` at `at`: kind, flags, offset, vaddr, paddr 0, filesz, memsz, align.
#[allow(clippy::too_many_arguments)]
fn program_header(
    data: &mut [u8],
    at: usize,
    kind: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
) {
    put_u32(data, at, kind);
    put_u32(data, at + 4, flags);
    put_u64(data, at + 8, offset);
    put_u64(data, at + 16, vaddr);
    put_u64(data, at + 32, filesz);
    put_u64(data, at + 40, memsz);
    put_u64(data, at + 48, align);
}

/// A SELF descriptor `'<QQQQ'`: flags, file offset, size, memory size.
fn descriptor(data: &mut [u8], at: usize, flags: u64, offset: u64, size: u64) {
    put_u64(data, at, flags);
    put_u64(data, at + 8, offset);
    put_u64(data, at + 16, size);
    put_u64(data, at + 24, size);
}

const SELF: &[u8; 4] = b"\x4f\x15=\x1d";

/// `fixture()` in `test_prepare.py`: one blocked segment, file offset 0x1000, vaddr 0.
fn fixture() -> Vec<u8> {
    let mut data = vec![0u8; 0x210];
    data[..4].copy_from_slice(SELF);
    put_u16(&mut data, 24, 1);
    descriptor(&mut data, 32, 0x800, 0x200, 16);
    elf_header(&mut data, 64, 0xfe10, 1);
    program_header(&mut data, 128, 1, 5, 0x1000, 0, 16, 32, 0x1000);
    data[0x200..].copy_from_slice(&(0u8..16).collect::<Vec<_>>());
    data
}

/// `make_test_libc()` / the `libc()` of the RET test: `_init_env` (by NID) at vaddr 0, its one
/// code byte `instruction`.
fn python_libc(instruction: u8) -> Vec<u8> {
    let mut data = vec![0u8; 0x320];
    data[..4].copy_from_slice(SELF);
    put_u16(&mut data, 24, 2);
    descriptor(&mut data, 32, 0x800, 0x200, 16);
    descriptor(&mut data, 64, 0x10_0800, 0x220, 256);
    elf_header(&mut data, 96, 0xfe18, 3);
    program_header(&mut data, 160, 1, 5, 0x1000, 0, 16, 16, 0x1000);
    program_header(&mut data, 216, 0x6100_0000, 4, 0x2000, 0, 256, 0, 16);
    program_header(&mut data, 272, 2, 4, 0x20b0, 0, 80, 80, 8);
    data[0x200] = instruction;
    let name = b"bzQExy189ZI#C#A\0";
    data[0x220..0x220 + name.len()].copy_from_slice(name);
    // '<IBBHQQ': name 0, info 0x12 (global FUNC), other 0, section 1, value 0, size 1.
    data[0x264] = 0x12;
    put_u16(&mut data, 0x266, 1);
    put_u64(&mut data, 0x270, 1);
    for (i, (tag, value)) in [
        (0x6100_0035u64, 0u64),
        (0x6100_0037, 32),
        (0x6100_0039, 64),
        (0x6100_003f, 24),
        (0, 0),
    ]
    .into_iter()
    .enumerate()
    {
        put_u64(&mut data, 0x2d0 + 16 * i, tag);
        put_u64(&mut data, 0x2d8 + 16 * i, value);
    }
    data
}

/// `make_test_eboot()`: three blocked segments, empty symbol and relocation tables.
fn python_eboot() -> Vec<u8> {
    let mut data = vec![0u8; 0x600];
    data[..4].copy_from_slice(SELF);
    put_u16(&mut data, 24, 3);
    descriptor(&mut data, 32, 0x800, 0x200, 16);
    descriptor(&mut data, 64, 0x10_0800, 0x220, 0x200);
    descriptor(&mut data, 96, 0x20_0800, 0x420, 0x100);
    elf_header(&mut data, 128, 0xfe18, 3);
    program_header(&mut data, 192, 1, 5, 0x1000, 0, 16, 16, 0x1000);
    program_header(&mut data, 248, 0x6100_0000, 4, 0x2000, 0, 0x200, 0, 16);
    program_header(&mut data, 304, 2, 4, 0x2200, 0, 0x100, 0x100, 8);
    data[0x200] = 0xc3;
    let tags = [
        (0x6100_0035u64, 0u64),
        (0x6100_0037, 32),
        (0x6100_0039, 32),
        (0x6100_003f, 0),
        (0x6100_0029, 32),
        (0x6100_002d, 0),
        (0x6100_002f, 32),
        (0x6100_0031, 0),
        (0, 0),
    ];
    for (i, (tag, value)) in tags.into_iter().enumerate() {
        put_u64(&mut data, 0x420 + 16 * i, tag);
        put_u64(&mut data, 0x428 + 16 * i, value);
    }
    data
}

enum Value<'a> {
    Text(&'a str),
    Int(u32),
}

/// A `param.sfo` (`make_test_sfo`, with any number of keys): text is format 0x204, integers
/// 0x404.
fn param_sfo(entries: &[(&str, Value)]) -> Vec<u8> {
    let mut keys = Vec::new();
    let mut values = Vec::new();
    let mut table = Vec::new();
    for (key, value) in entries {
        let (format, bytes) = match value {
            Value::Text(text) => {
                let mut bytes = text.as_bytes().to_vec();
                bytes.push(0);
                (0x0204u16, bytes)
            }
            Value::Int(n) => (0x0404, n.to_le_bytes().to_vec()),
        };
        table.extend_from_slice(&(keys.len() as u16).to_le_bytes());
        table.extend_from_slice(&format.to_le_bytes());
        table.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        table.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        table.extend_from_slice(&(values.len() as u32).to_le_bytes());
        keys.extend_from_slice(key.as_bytes());
        keys.push(0);
        values.extend_from_slice(&bytes);
    }
    let key_base = 20 + table.len();
    let value_base = key_base + keys.len();
    let mut data = b"\0PSF".to_vec();
    for word in [0x101, key_base, value_base, entries.len()] {
        data.extend_from_slice(&(word as u32).to_le_bytes());
    }
    data.extend_from_slice(&table);
    data.extend_from_slice(&keys);
    data.extend_from_slice(&values);
    data
}

// ---- The SELF reader (`SelfTests`) ----

#[test]
fn libc_ret_contract_is_verified_from_symbol_and_code() {
    let proof = inspect_libc(&python_libc(0xc3)).unwrap();
    assert!(proof.init_env_is_ret);
    assert_eq!(proof.report["bytes"], "c3");
    assert_eq!(proof.report["symbol"], "bzQExy189ZI#C#A");
    assert_eq!(proof.report["address"], "0x0");
    assert_eq!(
        proof.report["sha256"],
        bundle::sha256_hex(&python_libc(0xc3))
    );
    let proof = inspect_libc(&python_libc(0x90)).unwrap();
    assert!(!proof.init_env_is_ret);
    assert_eq!(proof.report["init_env_is_ret"], false);
    assert_eq!(nid("_init_env"), "bzQExy189ZI");
}

#[test]
fn nid_is_sha1_of_the_salted_name_reversed_in_base64() {
    // FIPS 180-4 test vectors for the SHA-1 underneath.
    assert_eq!(
        hex(&sha1(b"abc")),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(
        hex(&sha1(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )),
        "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
    );
    assert_eq!(nid("_init_env"), "bzQExy189ZI");
    for name in NAME_HINTS {
        let id = nid(name);
        assert_eq!(id.len(), 11, "{name}");
        assert!(!id.contains('/') && !id.contains('='), "{name}");
    }
}

#[test]
fn file_offset_is_not_virtual_address() {
    let image = load_image(&fixture()).unwrap();
    assert_eq!(image.elf[0x1000..0x1010], (0u8..16).collect::<Vec<_>>()[..]);
    assert_eq!(image.program_headers[0].vaddr, 0);
    assert_eq!(image.bytes[..16], (0u8..16).collect::<Vec<_>>()[..]);
}

#[test]
fn plain_elf_matches_self() {
    // Executables dumped already decrypted are a bare ELF64, with no SELF wrapper.
    let wrapped = load_image(&fixture()).unwrap();
    let mut plain = fixture()[64..].to_vec();
    plain.resize(0x1000, 0);
    plain.extend(0u8..16);
    let plain = load_image(&plain).unwrap();
    assert_eq!(plain.elf[0x1000..0x1010], wrapped.elf[0x1000..0x1010]);
    assert_eq!(plain.program_headers, wrapped.program_headers);
    assert_eq!(plain.bytes, wrapped.bytes);
}

#[test]
fn plain_elf_missing_required_segment_rejected() {
    assert!(matches!(
        load_image(&fixture()[64..0x1d0]),
        Err(Error::Prepare(message)) if message.contains("required segment 0")
    ));
}

#[test]
fn unknown_magic_rejected() {
    let mut data = b"MZ".to_vec();
    data.resize(302, 0);
    assert!(matches!(
        load_image(&data),
        Err(Error::Prepare(message)) if message == "expected PS4 SELF or ELF64"
    ));
    // The old reader's wrong SELF magic (`00 15 3d 1d`) is not a SELF.
    let mut data = fixture();
    data[0] = 0;
    assert!(matches!(
        load_image(&data),
        Err(Error::Prepare(message)) if message == "expected PS4 SELF or ELF64"
    ));
}

#[test]
fn truncated_payload_rejected() {
    let mut data = fixture();
    data.pop();
    assert!(matches!(load_image(&data), Err(Error::Prepare(_))));
}

#[test]
fn encrypted_or_compressed_payload_rejected() {
    for flag in [2, 8] {
        let mut data = fixture();
        put_u64(&mut data, 32, 0x800 | flag);
        assert!(matches!(
            load_image(&data),
            Err(Error::Prepare(message)) if message.contains("encrypted/compressed")
        ));
    }
}

#[test]
fn missing_required_segment_rejected() {
    let mut data = fixture();
    put_u64(&mut data, 32, 0);
    assert!(matches!(
        load_image(&data),
        Err(Error::Prepare(message)) if message.contains("required segment")
    ));
}

#[test]
fn bad_program_header_index_rejected() {
    let mut data = fixture();
    put_u64(&mut data, 32, 0x10_0800);
    assert!(matches!(
        load_image(&data),
        Err(Error::Prepare(message)) if message.contains("segment index")
    ));
}

// ---- A temporary folder, removed on drop ----

struct Temp {
    root: PathBuf,
}

impl Temp {
    fn new(name: &str) -> Temp {
        let root =
            std::env::temp_dir().join(format!("uncharted-prepare-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Temp { root }
    }

    fn dump(&self) -> PathBuf {
        self.root.join("dump")
    }

    fn install(&self) -> PathBuf {
        self.root.join("install")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

// ---- `PrepareEncodingTests`: analysis.json keeps the title's UTF-8 ----

#[test]
fn analysis_json_keeps_a_trademark_title_in_utf8() {
    let temp = Temp::new("utf8-title");
    let dump = temp.dump();
    write(&dump.join("sce_module/libc.prx"), &python_libc(0xc3));
    fs::create_dir_all(dump.join("dvdroot_ps4")).unwrap();
    let title = "UNCHARTED: The Nathan Drake Collection\u{2122}";
    let values = sfo::parse(&param_sfo(&[("TITLE", Value::Text(title))])).unwrap();
    let source = python_eboot();
    let image = load_image(&source).unwrap();
    let libc = inspect_libc(&python_libc(0xc3)).unwrap();
    let boot = build_boot(&image, libc.init_env_is_ret).unwrap();
    let text = analysis_json(&dump, &source, &values, &image, &boot, &libc).unwrap();
    assert!(text.contains(title), "the title is not escaped");
    assert!(text.ends_with("}\n"));
    let analysis: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(analysis["sfo"]["TITLE"], title);
    assert_eq!(analysis["libc_evidence"]["init_env_is_ret"], true);
    assert_eq!(analysis["bundled_modules"], serde_json::json!(["libc.prx"]));
    assert_eq!(analysis["import_count"], 0);
    assert_eq!(analysis["entry"], "0x0");
    assert_eq!(analysis["source_sha256"], bundle::sha256_hex(&source));

    // boot.bin of the same fixture: header, one segment, no names or relocations, the image.
    assert_eq!(boot.file[..8], BOOT_MAGIC);
    let header: Vec<u64> = (0..6).map(|i| word(&boot.file, 8 + i * 8)).collect();
    assert_eq!(header, [16, 0, 1, 0, 0, 1]);
    assert_eq!(
        [
            word(&boot.file, 56),
            word(&boot.file, 64),
            word(&boot.file, 72)
        ],
        [0, 16, 5]
    );
    assert_eq!(boot.file[80..], image.bytes[..]);
    assert_eq!(boot.file[80], 0xc3);
}

// ---- The synthetic dump `prepare` runs on ----

const APP_VER: &str = "01.02";
const OPEN: [(Game, &[&str]); 3] = [
    (Game::U1, &[APP_VER]),
    (Game::U2, &[APP_VER]),
    (Game::U3, &[]),
];

/// `link`'s executable with a symbol table and relocations: two function imports (one bound
/// twice), a data import with an addend, a local symbol, an absolute symbol and a base-relative
/// relocation.
fn main_elf() -> Elf {
    let mut elf = eboot();
    elf.symbols = vec![
        sym("", STT_SECTION, 0, 0, 0),
        sym("Nmemcpy#A#A", STT_FUNC, STB_GLOBAL, 0, 0),
        sym("Nopen#B#B", STT_FUNC, STB_GLOBAL, 0, 0),
        sym("Nmissing#C#C", STT_OBJECT, STB_GLOBAL, 0, 0),
        sym("Nlocal#A#A", STT_FUNC, STB_GLOBAL, 1, 0x20),
        sym("Nabs#A#A", STT_OBJECT, 0, SHN_ABS, 0x1234),
    ];
    elf.jmprel = vec![
        (0x200, R_X86_64_JUMP_SLOT, 1, 0),
        (0x208, R_X86_64_JUMP_SLOT, 2, 0),
    ];
    elf.rela = vec![
        (0x210, R_X86_64_RELATIVE, 0, 0x30),
        (0x218, R_X86_64_64, 3, 8),
        (0x220, R_X86_64_64, 4, 4),
        (0x228, R_X86_64_64, 5, 2),
        (0x230, R_X86_64_GLOB_DAT, 1, 0),
    ];
    elf
}

/// `link`'s libc with `_init_env` exported (by NID) at a `RET` byte, and every name in the
/// NID encoding `link_libc` reads.
fn libc_elf() -> Elf {
    let mut elf = libc();
    elf.code[0x98] = 0xc3;
    let init_env: &'static str = Box::leak(format!("{}#A#A", nid("_init_env")).into_boxed_str());
    for symbol in &mut elf.symbols {
        if symbol.name == "module_start" {
            symbol.name = "Nstart#A#A";
        }
    }
    elf.symbols
        .push(sym(init_env, STT_FUNC, STB_GLOBAL, 1, 0x98));
    elf
}

const PATCH_XML: &str = r#"<?xml version="1.0"?>
<Patches><TitleID><ID>CUSA02320</ID></TitleID>
<Metadata Name="Nop" Author="test" AppVer="01.02" AppElf="eboot.bin" isEnabled="true">
<PatchList><Line Type="bytes" Address="0x00400030" Value="90 90"/></PatchList></Metadata>
<Metadata Name="Off" Author="test" AppVer="01.02" AppElf="eboot.bin" isEnabled="false">
<PatchList><Line Type="bytes" Address="0x00400038" Value="cc"/></PatchList></Metadata>
<Metadata Name="Other revision" Author="test" AppVer="01.00" AppElf="eboot.bin" isEnabled="true">
<PatchList><Line Type="bytes" Address="0x00400040" Value="cc"/></PatchList></Metadata>
</Patches>"#;

/// A dump folder of `game` with its executable named `exe`.
fn make_dump(dump: &Path, exe: &str, title_id: &str) {
    write(&dump.join(exe), &main_elf().bytes());
    write(&dump.join("sce_module/libc.prx"), &libc_elf().bytes());
    write(&dump.join("sce_module/libSceFios2.prx"), &fios().bytes());
    write(
        &dump.join("sce_sys/param.sfo"),
        &param_sfo(&[
            ("APP_VER", Value::Text(APP_VER)),
            ("TITLE", Value::Text("UNCHARTED\u{2122}")),
            ("TITLE_ID", Value::Text(title_id)),
            ("USER_DEFINED_PARAM_2", Value::Int(7)),
        ]),
    );
    write(&dump.join("dvdroot_ps4/u1data/a.psarc"), b"resource");
    write(&dump.join("dvdroot_ps4/u1data/sub/b.psarc"), b"more");
}

fn with_patches(dir: PathBuf) -> PrepareOptions {
    PrepareOptions {
        patches: Some(PatchSource {
            dir,
            selection: Selection::default(),
        }),
        ..PrepareOptions::default()
    }
}

/// Every file under `dir`, relative with `/`, sorted.
fn listing(dir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            } else {
                let rel = entry.path().strip_prefix(dir).unwrap().to_owned();
                out.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out
}

#[test]
fn prepares_a_whole_install_and_writes_the_manifest_last() {
    let temp = Temp::new("whole");
    let (dump, install) = (temp.dump(), temp.install());
    make_dump(&dump, "eboot.bin", "CUSA02320");
    let patch_dir = temp.root.join("patches");
    write(&patch_dir.join("nop.xml"), PATCH_XML.as_bytes());
    write(&patch_dir.join("zz-broken.xml"), b"<Patches><Metadata>");
    write(&patch_dir.join("readme.txt"), b"not a patch");

    let manifest =
        prepare_with(&dump, &install, Game::U1, &with_patches(patch_dir), &OPEN).unwrap();
    assert_eq!(manifest.game, Game::U1);
    assert_eq!(manifest.title_id, "CUSA02320");
    assert_eq!(manifest.app_ver, APP_VER);
    assert_eq!(
        manifest.files,
        [
            "analysis.json",
            "boot-libc.bin",
            "boot-linked.bin",
            "boot.bin",
            "content-profile.json",
            "content.bin",
            "libc-link.json",
            "link.json",
            "patches.bin",
        ]
    );
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(install.join(MANIFEST_FILE)).unwrap()).unwrap();
    assert_eq!(json["game"], "u1");
    assert_eq!(json["title_id"], "CUSA02320");
    assert_eq!(json["app_ver"], APP_VER);
    assert_eq!(json["files"], serde_json::json!(manifest.files));
    assert_eq!(
        fs::read_to_string(install.join(MANIFEST_FILE)).unwrap(),
        manifest.to_json()
    );

    // boot.bin: the header, the one load segment, the imports in first-use order, the
    // relocations (absolute symbols already written into the image), then the image.
    let boot = fs::read(install.join(BOOT_FILE)).unwrap();
    assert_eq!(boot[..8], BOOT_MAGIC);
    let header: Vec<u64> = (0..6).map(|i| word(&boot, 8 + i * 8)).collect();
    assert_eq!(header, [0x1000, 0, 1, 6, 3, 1]);
    assert_eq!(
        [word(&boot, 56), word(&boot, 64), word(&boot, 72)],
        [0, 0x1000, 5]
    );
    let names: Vec<String> = (0..3)
        .map(|i| {
            let field = &boot[80 + i * 128..80 + (i + 1) * 128];
            let len = field.iter().position(|&b| b == 0).unwrap();
            String::from_utf8(field[..len].to_vec()).unwrap()
        })
        .collect();
    assert_eq!(names, ["Nmemcpy#A#A", "Nopen#B#B", "Nmissing#C#C"]);
    let at = 80 + 3 * 128;
    let relocations: Vec<[u64; 4]> = (0..6)
        .map(|i| {
            let base = at + i * 32;
            [0, 8, 16, 24].map(|o| word(&boot, base + o))
        })
        .collect();
    assert_eq!(
        relocations,
        [
            [0x200, 1, 0, 0],
            [0x208, 1, 1, 0],
            [0x210, 0, 0x30, 0],
            [0x218, 2, 2, 8],
            [0x220, 0, 0x24, 0],
            [0x230, 1, 0, 0],
        ]
    );
    let image = &boot[at + 6 * 32..];
    assert_eq!(image.len(), 0x1000);
    assert_eq!(word(image, 0x228), 0x1236);
    assert_eq!(image[..0x40], main_elf().code[..]);

    let analysis: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(install.join(ANALYSIS_FILE)).unwrap()).unwrap();
    assert_eq!(analysis["sfo"]["TITLE"], "UNCHARTED\u{2122}");
    assert_eq!(
        analysis["relocation_counts"],
        serde_json::json!({"1": 3, "6": 1, "7": 2, "8": 1})
    );
    assert_eq!(analysis["imports"], serde_json::json!(names));
    assert_eq!(analysis["libc_evidence"]["init_env_is_ret"], true);
    assert_eq!(analysis["libc_evidence"]["bytes"], "c3");
    assert_eq!(
        analysis["bundled_modules"],
        serde_json::json!(["libSceFios2.prx", "libc.prx"])
    );
    assert_eq!(analysis["resources"], serde_json::json!({"u1data": 2}));
    assert_eq!(analysis["resource_bytes"], 12);
    assert_eq!(
        analysis["source_sha256"],
        bundle::sha256_hex(&main_elf().bytes())
    );

    // The content profile from param.sfo: full SKU, parameter 1 from the title number.
    let content = fs::read(install.join(CONTENT_FILE)).unwrap();
    assert_eq!(content[..8], *b"BBCONT01");
    let profile: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(install.join(CONTENT_JSON_FILE)).unwrap())
            .unwrap();
    assert_eq!(profile["sku"], "full");
    assert_eq!(profile["user_params"], serde_json::json!([2320, 7, 0, 0]));

    // Only the enabled patch for this revision: one write at 0x30. The broken file is skipped.
    let expected = patches::to_patches_bin(&[patches::ByteWrite {
        offset: 0x30,
        bytes: vec![0x90, 0x90],
    }]);
    assert_eq!(fs::read(install.join(PATCHES_FILE)).unwrap(), expected);

    // The linked images come from the boot.bin written here.
    assert_eq!(
        fs::read(install.join(link::LINKED_FILE)).unwrap()[..8],
        *b"BBPROBE5"
    );
    assert_eq!(
        fs::read(install.join(link::LIBC_IMAGE)).unwrap()[..8],
        link::LIBC_MAGIC
    );
}

/// SPEC.md M9: list the install folder itself. It holds exactly the manifest's files and
/// `install.json`, none of them a name the bundle refuses, none named or byte-equal to a file
/// of the dump.
#[test]
fn the_install_folder_holds_no_game_files() {
    let temp = Temp::new("no-game-files");
    let (dump, install) = (temp.dump(), temp.install());
    make_dump(&dump, "eboot.bin", "CUSA02320");
    let patch_dir = temp.root.join("patches");
    write(&patch_dir.join("nop.xml"), PATCH_XML.as_bytes());
    let manifest =
        prepare_with(&dump, &install, Game::U1, &with_patches(patch_dir), &OPEN).unwrap();

    let mut expected: BTreeSet<String> = manifest.files.iter().cloned().collect();
    expected.insert(MANIFEST_FILE.to_owned());
    let listed = listing(&install);
    assert_eq!(listed, expected);
    for name in &listed {
        assert!(!bundle::is_forbidden(name), "{name}");
        assert!(
            !name.ends_with(".elf") && !name.starts_with("entry"),
            "{name}"
        );
    }
    let dump_files = listing(&dump);
    let dump_names: BTreeSet<&str> = dump_files
        .iter()
        .map(|rel| rel.rsplit('/').next().unwrap())
        .collect();
    let dump_bytes: Vec<Vec<u8>> = dump_files
        .iter()
        .map(|rel| fs::read(dump.join(rel)).unwrap())
        .collect();
    for name in &listed {
        assert!(!dump_names.contains(name.as_str()), "{name}");
        let bytes = fs::read(install.join(name)).unwrap();
        assert!(
            !dump_bytes.contains(&bytes),
            "{name} is a copy of a dump file"
        );
    }
}

#[test]
fn a_refused_revision_writes_nothing() {
    let temp = Temp::new("refused");
    let (dump, install) = (temp.dump(), temp.install());
    make_dump(&dump, "eboot.bin", "CUSA02320");
    // Supported for U1 only: U3 (and any other APP_VER) is refused before any write.
    let refused = prepare_with(&dump, &install, Game::U3, &PrepareOptions::default(), &OPEN);
    assert_eq!(
        refused,
        Err(Error::WrongRevision {
            game: "u3".into(),
            found: APP_VER.into(),
        })
    );
    assert!(!install.exists());
    // The shipped table is still empty: the public `prepare` refuses every revision.
    assert!(matches!(
        prepare(&dump, &install, Game::U1, &PrepareOptions::default()),
        Err(Error::WrongRevision { .. })
    ));
    assert!(!install.exists());
    // A refusal never touches an existing install either.
    write(&install.join(MANIFEST_FILE), b"{}");
    write(&install.join("boot-res-1x1.bin"), b"old");
    assert!(prepare(&dump, &install, Game::U1, &PrepareOptions::default()).is_err());
    assert_eq!(
        listing(&install),
        BTreeSet::from([MANIFEST_FILE.to_owned(), "boot-res-1x1.bin".to_owned()])
    );
}

#[test]
fn a_bad_dump_is_refused_before_anything_is_written() {
    let temp = Temp::new("bad-dump");
    let (dump, install) = (temp.dump(), temp.install());
    fs::create_dir_all(&dump).unwrap();
    assert!(matches!(
        prepare_with(&dump, &install, Game::U1, &PrepareOptions::default(), &OPEN),
        Err(Error::BadDump(message)) if message.starts_with("sce_sys/param.sfo")
    ));
    assert!(!install.exists());

    // An executable the reader refuses, after the gate opened: still nothing written.
    make_dump(&dump, "eboot.bin", "CUSA02320");
    write(&dump.join("eboot.bin"), b"MZ not an executable");
    assert!(matches!(
        prepare_with(&dump, &install, Game::U1, &PrepareOptions::default(), &OPEN),
        Err(Error::Prepare(message)) if message == "expected PS4 SELF or ELF64"
    ));
    assert!(!install.exists());

    // No libc.prx: BadDump, nothing written.
    make_dump(&dump, "eboot.bin", "CUSA02320");
    fs::remove_file(dump.join("sce_module/libc.prx")).unwrap();
    assert!(matches!(
        prepare_with(&dump, &install, Game::U1, &PrepareOptions::default(), &OPEN),
        Err(Error::BadDump(message)) if message.starts_with("sce_module/libc.prx")
    ));
    assert!(!install.exists());

    // No TITLE_ID.
    make_dump(&dump, "eboot.bin", "");
    assert!(matches!(
        prepare_with(&dump, &install, Game::U1, &PrepareOptions::default(), &OPEN),
        Err(Error::BadDump(_))
    ));
    assert!(!install.exists());
}

#[test]
fn a_render_size_the_image_cannot_take_leaves_no_manifest() {
    let temp = Temp::new("render-size");
    let (dump, install) = (temp.dump(), temp.install());
    make_dump(&dump, "eboot.bin", "CUSA02320");
    let options = PrepareOptions {
        render_size: Some((1280, 720)),
        ..PrepareOptions::default()
    };
    // The synthetic image has none of the game's viewport code.
    assert!(matches!(
        prepare_with(&dump, &install, Game::U1, &options, &OPEN),
        Err(Error::RenderSize(_))
    ));
    assert!(!install.join(MANIFEST_FILE).exists());

    // The game's own output size is no render-size file at all.
    let options = PrepareOptions {
        render_size: Some(render_size::OUTPUT_SIZE),
        ..PrepareOptions::default()
    };
    let manifest = prepare_with(&dump, &install, Game::U1, &options, &OPEN).unwrap();
    assert!(manifest.files.iter().all(|f| !f.starts_with("boot-res-")));
    assert!(!manifest.files.contains(&PATCHES_FILE.to_owned()));
}

#[test]
fn a_new_prepare_clears_stale_outputs_and_keeps_device_folders() {
    let temp = Temp::new("stale");
    let (dump, install) = (temp.dump(), temp.install());
    make_dump(&dump, "eboot.bin", "CUSA02320");
    write(&install.join("boot-res-2560x1440.bin"), b"old");
    write(&install.join("boot-res-2560x1440.json"), b"{}");
    write(&install.join("saves/slot0.dat"), b"save");
    let options = PrepareOptions {
        sku: Sku::Trial,
        ..PrepareOptions::default()
    };
    let manifest = prepare_with(&dump, &install, Game::U1, &options, &OPEN).unwrap();
    assert!(!install.join("boot-res-2560x1440.bin").exists());
    assert!(!install.join("boot-res-2560x1440.json").exists());
    assert_eq!(fs::read(install.join("saves/slot0.dat")).unwrap(), b"save");
    assert!(!manifest.files.iter().any(|f| f.starts_with("saves")));
    let profile = fs::read_to_string(install.join(CONTENT_JSON_FILE)).unwrap();
    assert!(profile.contains("\"sku\": \"trial\""));
}

#[test]
fn a_game_with_its_own_executable_skips_the_libc_only_image() {
    let temp = Temp::new("u2-own-exe");
    let (dump, install) = (temp.dump(), temp.install());
    make_dump(&dump, Game::U2.executable(), "CUSA02320");
    let manifest =
        prepare_with(&dump, &install, Game::U2, &PrepareOptions::default(), &OPEN).unwrap();
    assert!(manifest.files.contains(&link::LINKED_FILE.to_owned()));
    assert!(!manifest.files.contains(&link::LIBC_IMAGE.to_owned()));
    assert!(!install.join(link::LIBC_IMAGE).exists());
}

#[test]
fn a_game_dumped_on_its_own_uses_its_eboot() {
    let temp = Temp::new("u2-eboot");
    let (dump, install) = (temp.dump(), temp.install());
    // Uncharted 2 dumped on its own: its executable is the folder's eboot.bin.
    make_dump(&dump, "eboot.bin", "CUSA07737");
    let manifest =
        prepare_with(&dump, &install, Game::U2, &PrepareOptions::default(), &OPEN).unwrap();
    assert!(manifest.files.contains(&link::LIBC_IMAGE.to_owned()));
    // The collection's eboot.bin is Uncharted 1, never Uncharted 2.
    make_dump(&dump, "eboot.bin", "CUSA02320");
    assert!(matches!(
        prepare_with(&dump, &install, Game::U2, &PrepareOptions::default(), &OPEN),
        Err(Error::BadDump(message)) if message.starts_with("big2-ps4_Shipping.elf")
    ));
}

// ---- locate ----

fn env_of(pairs: &[(&str, String)]) -> impl Fn(&str) -> Option<String> {
    let map: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();
    move |key| map.get(key).cloned()
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn locate_prefers_the_games_own_folder_then_the_shared_one() {
    let temp = Temp::new("locate");
    let own = temp.root.join("u2");
    let shared = temp.root.join("collection");
    fs::create_dir_all(&own).unwrap();
    fs::create_dir_all(&shared).unwrap();
    let env = env_of(&[
        ("BB_GAME_DIR_U2", path_text(&own)),
        ("BB_GAME_DIR", path_text(&shared)),
    ]);
    assert_eq!(
        locate(Game::U2, &env).unwrap(),
        (own.clone(), "big2-ps4_Shipping.elf".to_owned())
    );
    assert_eq!(
        locate(Game::U3, &env).unwrap(),
        (shared.clone(), "big3-ps4_Shipping.elf".to_owned())
    );
    assert_eq!(
        locate(Game::U1, &env).unwrap(),
        (shared.clone(), "eboot.bin".to_owned())
    );
    // An empty value is not set.
    let env = env_of(&[
        ("BB_GAME_DIR_U3", String::new()),
        ("BB_GAME_DIR", path_text(&shared)),
    ]);
    assert_eq!(locate(Game::U3, &env).unwrap().0, shared);
}

#[test]
fn locate_without_a_folder_is_an_env_error() {
    let env = env_of(&[("BB_GAME_DIR", String::new())]);
    assert_eq!(
        locate(Game::U3, env),
        Err(Error::Env(
            "BB_GAME_DIR_U3 or BB_GAME_DIR (the game folder, the one with eboot.bin)".into()
        ))
    );
    assert!(matches!(locate(Game::U1, |_| None), Err(Error::Env(_))));
}

#[test]
fn locate_uses_the_eboot_of_a_game_dumped_on_its_own() {
    let temp = Temp::new("locate-eboot");
    let own = temp.root.join("u3");
    write(&own.join("eboot.bin"), b"x");
    let env = env_of(&[("BB_GAME_DIR_U3", path_text(&own))]);
    assert_eq!(
        locate(Game::U3, &env).unwrap(),
        (own.clone(), "eboot.bin".to_owned())
    );
    // Its own executable, when there, wins.
    write(&own.join("big3-ps4_Shipping.elf"), b"x");
    assert_eq!(locate(Game::U3, &env).unwrap().1, "big3-ps4_Shipping.elf");

    // A collection folder keeps the name: Uncharted 1's data next to it ...
    let collection = temp.root.join("collection");
    write(&collection.join("eboot.bin"), b"x");
    fs::create_dir_all(collection.join("u1data")).unwrap();
    let env = env_of(&[("BB_GAME_DIR_U2", path_text(&collection))]);
    assert_eq!(locate(Game::U2, &env).unwrap().1, "big2-ps4_Shipping.elf");
    // ... or the collection's title id.
    fs::remove_dir(collection.join("u1data")).unwrap();
    write(
        &collection.join("sce_sys/param.sfo"),
        &param_sfo(&[("TITLE_ID", Value::Text("CUSA02344"))]),
    );
    assert_eq!(locate(Game::U2, &env).unwrap().1, "big2-ps4_Shipping.elf");
    // The shared folder never swaps in eboot.bin: there it is Uncharted 1.
    let alone = temp.root.join("alone");
    write(&alone.join("eboot.bin"), b"x");
    let env = env_of(&[("BB_GAME_DIR", path_text(&alone))]);
    assert_eq!(locate(Game::U2, &env).unwrap().1, "big2-ps4_Shipping.elf");
}
