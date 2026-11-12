//! `tests/test_link_libc.py` ported one test per test, on the same synthetic fixtures, plus the
//! module reader and an end-to-end link of authored SELF-free ELF files. Expected image hashes
//! and reports were produced by `scripts/link_libc.py` on the same inputs, so they check the
//! output byte for byte. No game binaries.

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
        (DT_SCE_RELA, relocations_at as u64),
        (DT_SCE_RELASZ, 24 * relocations.len() as u64),
        (DT_SCE_JMPREL, end),
        (DT_SCE_PLTRELSZ, 0),
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
