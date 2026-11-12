//! `tests/test_link_libc.py` ported one test per test, on the same synthetic fixtures, plus the
//! module reader and an end-to-end link of authored SELF-free ELF files. Expected image hashes
//! and reports were produced by `scripts/link_libc.py` on the same inputs, so they check the
//! output byte for byte. No game binaries.
//!
//! The `link_modules` tests (#283) are in the `link_modules` module below, on their own
//! hand-built synthetic game.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn ph(
    kind: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
) -> ProgramHeader {
    ProgramHeader {
        kind,
        flags,
        offset,
        vaddr,
        filesz,
        memsz,
        align,
    }
}

fn named(name: &str, version: u16) -> Named {
    (name.to_string(), version)
}

fn identity(nid: &str, library: Named, module: Named) -> Identity {
    Identity {
        nid: nid.to_string(),
        library,
        module,
    }
}

/// `package()` in `tests/test_probe.py`: a `BBPROBE2` boot image with one segment.
fn package(code: &[u8], names: &[&str], capabilities: u64) -> Vec<u8> {
    let mut image = code.to_vec();
    image.resize(image.len().max(4096), 0);
    let mut data = b"BBPROBE2".to_vec();
    for word in [
        image.len() as u64,
        0,
        1,
        0,
        names.len() as u64,
        capabilities,
        0,
        image.len() as u64,
        5,
    ] {
        data.extend_from_slice(&word.to_le_bytes());
    }
    for name in names {
        let mut field = [0u8; 128];
        field[..name.len()].copy_from_slice(name.as_bytes());
        data.extend_from_slice(&field);
    }
    data.extend_from_slice(&image);
    data
}

fn temp_dir(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "uncharted-link-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn empty_module() -> Module {
    Module {
        elf: Vec::new(),
        program_headers: Vec::new(),
        tags: HashMap::new(),
        symbols: Vec::new(),
        relocations: Vec::new(),
        libraries: Vec::new(),
        modules: Vec::new(),
        sha256: String::new(),
    }
}

fn symbol(
    name: &str,
    kind: u8,
    binding: u8,
    section: u16,
    value: u64,
    size: u64,
    identity: Option<Identity>,
) -> Symbol {
    Symbol {
        name: name.to_string(),
        kind,
        binding,
        section,
        value,
        size,
        identity,
    }
}

/// `LinkTests.fixture` in `tests/test_link_libc.py`.
fn fixture(version: u16, relocations: &[(u64, u32, u32, i64)]) -> (Module, Module) {
    let lib_key = identity("fixture", named("libc", version), named("libc", 1));
    let mut main = empty_module();
    main.libraries = vec![("q".into(), named("libc", 1))];
    main.modules = vec![("q".into(), named("libc", 1))];
    main.program_headers = vec![ph(0x6100_0001, 0, 0, 256, 0, 0, 0)];

    let mut elf = vec![0x31, 0xc0, 0xc3];
    elf.extend_from_slice(&[0; 13]);
    elf.push(0xc3);
    elf.extend_from_slice(&[0; 111]);
    let mut libc = empty_module();
    libc.program_headers = vec![
        ph(1, 5, 0, 0, elf.len() as u64, 4096, 0),
        ph(7, 0, 0, 128, 4, 32, 0),
    ];
    libc.elf = elf;
    libc.tags = HashMap::from([(12, 0)]);
    libc.symbols = vec![
        symbol("", 3, 0, 1, 0, 0, None),
        symbol("fixture#C#A", 2, 1, 1, 16, 1, Some(lib_key)),
    ];
    libc.relocations = relocations
        .iter()
        .map(|&(target, kind, symbol, addend)| Relocation {
            target,
            kind,
            symbol,
            addend,
        })
        .collect();
    libc.sha256 = "fixture".into();
    (main, libc)
}

/// `LinkTests.run_link`: the boot image, the linked image and its report.
fn run_link(main: &Module, libc: &Module, code: &[u8]) -> Result<(Vec<u8>, String)> {
    let out = temp_dir("run");
    fs::write(out.join("boot.bin"), package(code, &["fixture#q#q"], 1)).unwrap();
    let linked = link_libc_with(main, libc, &out);
    let result = linked.map(|linked| {
        assert_eq!(linked.image, out.join(LIBC_IMAGE));
        assert_eq!(linked.modules, ["libc.prx"]);
        (
            fs::read(out.join(LIBC_IMAGE)).unwrap(),
            fs::read_to_string(out.join(LIBC_REPORT)).unwrap(),
        )
    });
    fs::remove_dir_all(&out).unwrap();
    result
}

fn image_of(data: &[u8]) -> &[u8] {
    let size = u64_at(data, 8) as usize;
    &data[data.len() - size..]
}

fn prepare_message(result: Result<(Vec<u8>, String)>) -> String {
    match result {
        Err(Error::Prepare(message)) => message,
        other => panic!("expected a prepare error, got {other:?}"),
    }
}

const FIRST_REPORT: &str = r#"{
  "base": "0x10000",
  "size": 4096,
  "sha256": "fixture",
  "init": "0x10000",
  "bindings": 1,
  "tls_module_id": 2,
  "fs_loads_patched": 0,
  "main_tls": {
    "vaddr": 0,
    "filesz": 0,
    "memsz": 0,
    "align": 0
  },
  "tls_relocations": 0,
  "tls_template_bytes": 4,
  "tls_memory_bytes": 32,
  "imports": [
    "fixture#q#q"
  ],
  "relocation_counts": {},
  "symbol_bindings": [
    {
      "import_name": "fixture#q#q",
      "address": "0x10010",
      "kind": 1
    }
  ]
}
"#;

#[test]
fn different_local_ids_bind_same_identity() {
    let (main, libc) = fixture(1, &[]);
    let (data, report) = run_link(&main, &libc, &[0xc3]).unwrap();
    assert_eq!(&data[..8], b"BBPROBE4");
    assert_eq!(report, FIRST_REPORT);
    assert_eq!(data.len(), 69984);
    assert_eq!(
        sha256_of(&data),
        "e4e0b3760a6f465e4c2bc2b18ad3e4d3f6d6289074352fe48ce1e7838239b460"
    );
}

#[test]
fn same_nid_wrong_library_version_does_not_bind() {
    let (main, libc) = fixture(2, &[]);
    let (data, report) = run_link(&main, &libc, &[0xc3]).unwrap();
    assert!(report.contains("\n  \"bindings\": 0,\n"));
    assert!(report.contains("\n  \"symbol_bindings\": []\n"));
    assert_eq!(
        sha256_of(&data),
        "a7a23f4b8db2999cfed07679b81e1b93aabe87b51270c3b979598ba019622b69"
    );
}

#[test]
fn tls_module_id_is_literal_not_image_pointer() {
    let (main, libc) = fixture(1, &[(64, 16, 0, 0)]);
    let (data, report) = run_link(&main, &libc, &[0xc3]).unwrap();
    assert_eq!(u64_at(image_of(&data), 65536 + 64), 2);
    assert!(report.contains("\n  \"tls_relocations\": 1,\n"));
    assert!(report.contains("\n  \"relocation_counts\": {\n    \"16\": 1\n  },\n"));
    assert_eq!(
        sha256_of(&data),
        "713cb67c1399e3fd8e46082b27429d6d900bc9d2763f359efea861792045613f"
    );
}

#[test]
fn unsupported_tls_symbol_rejected() {
    let (main, libc) = fixture(1, &[(64, 16, 1, 0)]);
    let message = prepare_message(run_link(&main, &libc, &[0xc3]));
    assert!(message.contains("unsupported external TLS"), "{message}");
}

#[test]
fn relocation_outside_segments_rejected() {
    let (main, libc) = fixture(1, &[(4092, 8, 0, 0)]);
    let message = prepare_message(run_link(&main, &libc, &[0xc3]));
    assert_eq!(message, "libc relocation outside image 0xffc");
}

#[test]
fn fs_thread_pointer_loads_are_rewritten_to_gs() {
    let (mut main, libc) = fixture(1, &[]);
    let mut code = FS_LOAD.to_vec();
    code.push(0x90);
    code.extend_from_slice(&FS_LOAD);
    code.push(0xc3);
    main.program_headers.push(ph(1, 5, 0, 0, 64, 64, 0));
    main.program_headers.push(ph(7, 0, 0, 2048, 0, 48, 16));
    let (data, report) = run_link(&main, &libc, &code).unwrap();
    let image = image_of(&data);
    assert_eq!(image[0], 0x65);
    assert_eq!(image[10], 0x65);
    assert!(report.contains("\n  \"fs_loads_patched\": 2,\n"));
    assert!(report.contains(
        "\n  \"main_tls\": {\n    \"vaddr\": 2048,\n    \"filesz\": 0,\n    \"memsz\": 48,\n    \"align\": 16\n  },\n"
    ));
    let main_tls: Vec<u64> = (0..4).map(|i| u64_at(&data, 56 + 64 + i * 8)).collect();
    assert_eq!(main_tls, [2048, 0, 48, 16]);
    assert_eq!(
        sha256_of(&data),
        "c9848b0867ee9ed67005888e093476ea3fc53b7747a268458d1e51ca53b9dfbc"
    );
}

#[test]
fn fs_loads_outside_executable_segments_are_kept() {
    let (mut main, libc) = fixture(1, &[]);
    main.program_headers.push(ph(1, 6, 0, 0, 64, 64, 0));
    let (data, report) = run_link(&main, &libc, &FS_LOAD).unwrap();
    assert_eq!(image_of(&data)[0], 0x64);
    assert!(report.contains("\n  \"fs_loads_patched\": 0,\n"));
    assert_eq!(
        sha256_of(&data),
        "364fe576eb7513e2c9b37e10d8d3fdb3f5d2e7641a65c6ef8bc0d2a291f0e487"
    );
}

#[test]
fn boot_image_that_is_not_freshly_prepared_is_refused_and_left_alone() {
    let (main, libc) = fixture(1, &[]);
    let out = temp_dir("magic");
    let mut boot = package(&[0xc3], &["fixture#q#q"], 1);
    boot[7] = b'1';
    fs::write(out.join("boot.bin"), &boot).unwrap();
    let result = link_libc_with(&main, &libc, &out);
    assert_eq!(
        result,
        Err(Error::Prepare(
            "linker expects freshly prepared BBPROBE2".into()
        ))
    );
    assert!(!out.join(LIBC_IMAGE).exists());
    assert_eq!(fs::read(out.join("boot.bin")).unwrap(), boot);

    let mut trailing = package(&[0xc3], &["fixture#q#q"], 1);
    trailing.push(0);
    fs::write(out.join("boot.bin"), &trailing).unwrap();
    let result = link_libc_with(&main, &libc, &out);
    assert_eq!(
        result,
        Err(Error::Prepare("unexpected boot trailer".into()))
    );
    fs::remove_dir_all(&out).unwrap();
}

#[test]
fn encode_id_matches_the_script() {
    let cases = [
        (0, "A"),
        (1, "B"),
        (63, "-"),
        (64, "BA"),
        (65, "BB"),
        (4095, "--"),
        (65535, "P--"),
    ];
    for (value, expected) in cases {
        assert_eq!(encode_id(value), expected, "{value}");
    }
}

fn sha256_of(data: &[u8]) -> String {
    crate::bundle::sha256_hex(data)
}

fn put(data: &mut [u8], at: usize, bytes: &[u8]) {
    data[at..at + bytes.len()].copy_from_slice(bytes);
}

/// An authored plain ELF with SCE dynamic data: code at file 0x1000 (vaddr 0), the dynamic
/// table at 0x400 and the dynamic library data at 0x600. `extra` program headers have file
/// offset 0. Same bytes as the generator the expected values were made with.
fn synthetic(
    code: &[u8],
    strtab: &[u8],
    symbols: &[(u32, u8, u16, u64, u64)],
    relocations: &[(u64, u64, i64)],
    ids: &[(u64, u64)],
    extra: &[(u32, u32, u64, u64, u64, u64)],
) -> Vec<u8> {
    let mut headers = vec![
        (1, 5, 0x1000, 0, code.len() as u64, 0x1000, 0x1000),
        (2, 4, 0x400, 0, 0x200, 0x200, 8),
        (0x6100_0000, 4, 0x600, 0, 0x400, 0x400, 8),
    ];
    headers.extend(
        extra
            .iter()
            .map(|&(kind, flags, vaddr, filesz, memsz, align)| {
                (kind, flags, 0, vaddr, filesz, memsz, align)
            }),
    );
    let mut data = vec![0u8; 0x1000 + code.len()];
    put(&mut data, 0, b"\x7fELF\x02\x01\x01");
    put(&mut data, 18, &62u16.to_le_bytes());
    put(&mut data, 32, &64u64.to_le_bytes());
    put(&mut data, 54, &56u16.to_le_bytes());
    put(&mut data, 56, &(headers.len() as u16).to_le_bytes());
    for (i, (kind, flags, offset, vaddr, filesz, memsz, align)) in headers.into_iter().enumerate() {
        let at = 64 + i * 56;
        put(&mut data, at, &u32::to_le_bytes(kind));
        put(&mut data, at + 4, &u32::to_le_bytes(flags));
        for (j, word) in [offset, vaddr, 0, filesz, memsz, align]
            .into_iter()
            .enumerate()
        {
            put(&mut data, at + 8 + j * 8, &word.to_le_bytes());
        }
    }
    let blob = 0x600;
    let symbols_at = (strtab.len() + 7) & !7;
    let relocations_at = symbols_at + 24 * symbols.len();
    put(&mut data, blob, strtab);
    for (i, &(name, info, section, value, size)) in symbols.iter().enumerate() {
        let at = blob + symbols_at + 24 * i;
        put(&mut data, at, &name.to_le_bytes());
        data[at + 4] = info;
        put(&mut data, at + 6, &section.to_le_bytes());
        put(&mut data, at + 8, &value.to_le_bytes());
        put(&mut data, at + 16, &size.to_le_bytes());
    }
    for (i, &(target, info, addend)) in relocations.iter().enumerate() {
        let at = blob + relocations_at + 24 * i;
        put(&mut data, at, &target.to_le_bytes());
        put(&mut data, at + 8, &info.to_le_bytes());
        put(&mut data, at + 16, &addend.to_le_bytes());
    }
    let end = (relocations_at + 24 * relocations.len()) as u64;
    let mut tags = vec![
        (DT_SCE_STRTAB, 0),
        (DT_SCE_STRSZ, strtab.len() as u64),
        (DT_SCE_SYMTAB, symbols_at as u64),
        (DT_SCE_SYMTABSZ, 24 * symbols.len() as u64),
        // The relocations sit in the jump-slot table (0x61000029, read first) and the other
        // table is empty: the bytes the expected hashes were made from.
        (DT_SCE_JMPREL, relocations_at as u64),
        (DT_SCE_PLTRELSZ, 24 * relocations.len() as u64),
        (DT_SCE_RELA, end),
        (DT_SCE_RELASZ, 0),
    ];
    tags.extend_from_slice(ids);
    // An entry after the terminator is never read.
    tags.extend_from_slice(&[(DT_INIT, 0x10), (0, 0), (0x7fff, 1)]);
    for (i, (tag, value)) in tags.into_iter().enumerate() {
        put(&mut data, 0x400 + 16 * i, &tag.to_le_bytes());
        put(&mut data, 0x400 + 16 * i + 8, &value.to_le_bytes());
    }
    put(&mut data, 0x1000, code);
    data
}

fn id_tag(tag: u64, id: u64, version: u64, name: u64) -> (u64, u64) {
    (tag, (id << 48) | (version << 32) | name)
}

fn synthetic_eboot(code: &[u8]) -> Vec<u8> {
    synthetic(
        code,
        b"\0libc\0fixture#q#q\0",
        &[(6, 0x12, 0, 0, 0)],
        &[],
        &[
            id_tag(DT_SCE_IMPORT_LIB, 42, 1, 1),
            id_tag(DT_SCE_NEEDED_MODULE, 42, 1, 1),
        ],
        &[(0x6100_0001, 4, 0x80, 0, 0x40, 8), (7, 4, 0x800, 0, 48, 16)],
    )
}

/// libc with an export, a function and a data import, an absolute and a local symbol, and
/// one relocation of each supported kind.
fn synthetic_libc() -> Vec<u8> {
    let mut code = vec![0x31, 0xc0, 0xc3];
    code.extend_from_slice(&[0; 13]);
    code.push(0xc3);
    synthetic(
        &code,
        b"\0libc\0libSceOther\0fixture#C#A\0imp#C#A\0obj#B#A\0",
        &[
            (0, 0x03, 1, 0, 0),
            (18, 0x12, 1, 0x10, 1),
            (30, 0x11, 0, 0, 0),
            (38, 0x12, 0, 0, 0),
            (0, 0x00, SHN_ABS, 0x1234, 0),
            (0, 0x02, 1, 0x20, 0),
        ],
        &[
            (0x40, 16, 0),
            (0x08, 8, 0x30),
            (0x48, (2 << 32) | 7, 4),
            (0x50, (3 << 32) | 1, 0),
            (0x58, (4 << 32) | 1, 1),
            (0x60, (5 << 32) | 6, 2),
        ],
        &[
            id_tag(DT_SCE_EXPORT_LIB, 2, 1, 1),
            id_tag(DT_SCE_IMPORT_LIB, 1, 1, 6),
            id_tag(DT_SCE_MODULE_INFO, 0, 1, 1),
        ],
        &[(7, 4, 0x80, 4, 32, 16)],
    )
}

#[test]
fn module_reader_matches_the_script() {
    let source = synthetic_libc();
    let module = Module::read(&source).unwrap();
    assert_eq!(
        module.sha256,
        "9f81cb2329923b253230a454a54d507d223c17d46d65bea746b54a6466250cda"
    );
    assert_eq!(
        module.libraries,
        [
            ("C".to_string(), named("libc", 1)),
            ("B".to_string(), named("libSceOther", 1)),
        ]
    );
    assert_eq!(module.modules, [("A".to_string(), named("libc", 1))]);
    assert_eq!(module.tags.len(), 12);
    assert_eq!(module.tags[&DT_INIT], 16);
    let libc = || Some(named("libc", 1));
    let expected = [
        symbol("", 3, 0, 1, 0, 0, None),
        symbol(
            "fixture#C#A",
            2,
            1,
            1,
            16,
            1,
            Some(identity("fixture", libc().unwrap(), libc().unwrap())),
        ),
        symbol(
            "imp#C#A",
            1,
            1,
            0,
            0,
            0,
            Some(identity("imp", libc().unwrap(), libc().unwrap())),
        ),
        symbol(
            "obj#B#A",
            2,
            1,
            0,
            0,
            0,
            Some(identity("obj", named("libSceOther", 1), libc().unwrap())),
        ),
        symbol("", 0, 0, SHN_ABS, 0x1234, 0, None),
        symbol("", 2, 0, 1, 32, 0, None),
    ];
    assert_eq!(module.symbols, expected);
    let relocations: Vec<_> = module
        .relocations
        .iter()
        .map(|r| (r.target, r.kind, r.symbol, r.addend))
        .collect();
    assert_eq!(
        relocations,
        [
            (64, 16, 0, 0),
            (8, 8, 0, 48),
            (72, 7, 2, 4),
            (80, 1, 3, 0),
            (88, 1, 4, 1),
            (96, 6, 5, 2)
        ]
    );
}

#[test]
fn module_reader_refuses_conflicting_ids_and_plain_libc_names() {
    let conflicting = synthetic(
        &[0xc3],
        b"\0libc\0libkernel\0",
        &[],
        &[],
        &[
            id_tag(DT_SCE_EXPORT_LIB, 2, 1, 1),
            id_tag(DT_SCE_IMPORT_LIB, 2, 1, 6),
        ],
        &[],
    );
    assert_eq!(
        Module::read(&conflicting),
        Err(Error::Prepare("conflicting module/library IDs".into()))
    );

    let plain = synthetic(
        &[0xc3],
        b"\0module_start\0",
        &[(1, 0x12, 1, 0, 0)],
        &[],
        &[],
        &[],
    );
    let module = Module::read(&plain).unwrap();
    assert_eq!(module.symbols[0].identity, None);
    assert_eq!(
        read_linkable(&plain),
        Err(Error::Prepare("unsupported symbol encoding".into()))
    );

    let unknown = synthetic(&[0xc3], b"\0x#C#A\0", &[(1, 0x12, 1, 0, 0)], &[], &[], &[]);
    assert!(matches!(Module::read(&unknown), Err(Error::Prepare(_))));
}

/// The SCE relocation tags as the public PS4 ELF documentation gives them: OpenOrbis
/// `docs/MD/OELF Specification/PS4 ELF Specification.md` (DT_SCE_JMPREL 0x61000029,
/// DT_SCE_PLTRELSZ 0x6100002D, DT_SCE_RELA 0x6100002F, DT_SCE_RELASZ 0x61000031), GPCS4
/// `GPCS4/Loader/elf-sce.h` and shadPS4 `src/core/loader/elf.h` agree.
#[test]
fn dt_sce_tags_have_the_documented_values() {
    assert_eq!(DT_SCE_JMPREL, 0x6100_0029);
    assert_eq!(DT_SCE_PLTRELSZ, 0x6100_002d);
    assert_eq!(DT_SCE_RELA, 0x6100_002f);
    assert_eq!(DT_SCE_RELASZ, 0x6100_0031);
}

/// Both scripts read `(0x61000029, 0x6100002d)` then `(0x6100002f, 0x61000031)`: the jump
/// slots first, then the other relocations. Written with the raw tag numbers, so a swapped
/// constant cannot hide behind a fixture built from the same constant.
#[test]
fn module_reader_reads_jump_slots_then_other_relocations_by_raw_tag() {
    let mut source = synthetic(
        &[0xc3],
        b" ",
        &[(0, 0x03, 1, 0, 0)],
        &[(0x08, 8, 0x30), (0x10, 7, 0)],
        &[],
        &[],
    );
    // `synthetic` writes both relocations as one table; split it: the second record (the jump
    // slot) under 0x61000029, the first under 0x6100002f.
    let tag_at = |data: &[u8], tag: u64| {
        (0..32)
            .map(|i| 0x400 + 16 * i)
            .find(|&at| u64_at(data, at) == tag)
            .unwrap()
    };
    let relocations_at = u64_at(&source, tag_at(&source, 0x6100_0029) + 8);
    for (tag, offset) in [
        (0x6100_0029u64, relocations_at + 24),
        (0x6100_002d, 24),
        (0x6100_002f, relocations_at),
        (0x6100_0031, 24),
    ] {
        let at = tag_at(&source, tag) + 8;
        put(&mut source, at, &offset.to_le_bytes());
    }
    let module = Module::read(&source).unwrap();
    let read: Vec<_> = module
        .relocations
        .iter()
        .map(|r| (r.target, r.kind))
        .collect();
    assert_eq!(
        read,
        [(0x10, R_X86_64_JUMP_SLOT), (0x08, R_X86_64_RELATIVE)]
    );
}

#[test]
fn links_authored_eboot_and_libc_files_like_the_script() {
    let root = temp_dir("game");
    let game = root.join("game");
    let out = root.join("out");
    fs::create_dir_all(game.join("sce_module")).unwrap();
    fs::create_dir_all(&out).unwrap();
    let mut code = FS_LOAD.to_vec();
    code.push(0xc3);
    fs::write(game.join("eboot.bin"), synthetic_eboot(&code)).unwrap();
    fs::write(game.join("sce_module").join("libc.prx"), synthetic_libc()).unwrap();
    let boot = package(&code, &["fixture#q#q"], 1);
    fs::write(out.join("boot.bin"), &boot).unwrap();

    let linked = link_libc(&game, &out).unwrap();
    assert_eq!(linked.image, out.join(LIBC_IMAGE));
    let data = fs::read(&linked.image).unwrap();
    let report = fs::read_to_string(out.join(LIBC_REPORT)).unwrap();
    assert_eq!(
        fs::read(out.join("boot.bin")).unwrap(),
        boot,
        "boot.bin stays usable"
    );
    assert_eq!(data.len(), 70368);
    assert_eq!(
        sha256_of(&data),
        "ab760159083bced1dd66d2d00e07d7662d5d1f89329677c32868861ccfabea9c"
    );
    assert_eq!(report, E2E_REPORT);
    // Reproducible: a second link writes the same bytes.
    link_libc(&game, &out).unwrap();
    assert_eq!(fs::read(&linked.image).unwrap(), data);
    fs::remove_dir_all(&root).unwrap();
}

const E2E_REPORT: &str = r#"{
  "base": "0x10000",
  "size": 4096,
  "sha256": "9f81cb2329923b253230a454a54d507d223c17d46d65bea746b54a6466250cda",
  "init": "0x10010",
  "bindings": 1,
  "tls_module_id": 2,
  "fs_loads_patched": 1,
  "main_tls": {
    "vaddr": 2048,
    "filesz": 0,
    "memsz": 48,
    "align": 16
  },
  "tls_relocations": 1,
  "tls_template_bytes": 4,
  "tls_memory_bytes": 32,
  "imports": [
    "fixture#q#q",
    "imp#q#q",
    "libc:obj#B#A"
  ],
  "relocation_counts": {
    "16": 1,
    "8": 1,
    "7": 1,
    "1": 2,
    "6": 1
  },
  "symbol_bindings": [
    {
      "import_name": "fixture#q#q",
      "address": "0x10010",
      "kind": 1
    }
  ]
}
"#;

#[test]
fn json_strings_escape_like_python() {
    let mut out = String::new();
    write_json_string(&mut out, "a\"b\\c\n\u{1}\u{7f}\u{e9}\u{1f600}");
    assert_eq!(out, "\"a\\\"b\\\\c\\n\\u0001\\u007f\\u00e9\\ud83d\\ude00\"");
}

mod link_modules {
    //! `link_modules` against a hand-built synthetic game: a plain-ELF executable, `libc.prx` and
    //! `libSceFios2.prx` with SCE dynamic tables, and the `BBPROBE2` boot image `prepare` writes.
    //! No game data. `scripts/link_modules.py` has no tests of its own, so the expected values are
    //! worked out by hand from the script (see `good_link_matches_the_hand_checked_image`); the
    //! error cases port `tests/test_link_libc.py`.

    use crate::link::*;
    use std::path::PathBuf;

    struct Sym {
        name: &'static str,
        kind: u8,
        binding: u8,
        section: u16,
        value: u64,
        size: u64,
    }

    const fn sym(name: &'static str, kind: u8, binding: u8, section: u16, value: u64) -> Sym {
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
    struct Elf {
        code: Vec<u8>,
        memsz: u64,
        flags: u32,
        /// vaddr, filesz, memsz, align.
        tls: Option<[u64; 4]>,
        procparam: Option<u64>,
        /// Dynamic tag, ID, version, name.
        ids: Vec<(u64, u64, u64, &'static str)>,
        symbols: Vec<Sym>,
        jmprel: Vec<(u64, u32, u32, i64)>,
        rela: Vec<(u64, u32, u32, i64)>,
        init: Option<u64>,
    }

    fn put(data: &mut [u8], at: usize, bytes: &[u8]) {
        data[at..at + bytes.len()].copy_from_slice(bytes);
    }

    impl Elf {
        fn bytes(&self) -> Vec<u8> {
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
            for (i, (kind, flags, offset, vaddr, filesz, memsz, align)) in
                headers.iter().enumerate()
            {
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
    fn boot(
        magic: &[u8; 8],
        image: &[u8],
        names: &[&str],
        relocs: &[(u64, u64, i64, i64)],
    ) -> Vec<u8> {
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
    fn eboot() -> Elf {
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
    fn libc() -> Elf {
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

    fn fios() -> Elf {
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
                "tls_relocations": 1, "exports": 2, "sha256": crate::bundle::sha256_hex(&libc().bytes()),
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
}
