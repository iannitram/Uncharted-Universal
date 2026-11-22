//! Boot image parser, CLI option parser, and byte-pattern scanner for `uncharted-run`
//! (roadmap 3.16, #457: porting `src/probe.c` into Rust).
//!
//! Full guest execution (mmap/VirtualAlloc image mapping, signal/VEH fault handlers,
//! `bbgpu_init`, and `enter_on_stack`) remains in `src/probe.c` until C/GPU linkage moves
//! to `uncharted-run` in 3.17.

#[cfg(all(
    target_arch = "x86_64",
    any(target_os = "linux", target_os = "windows")
))]
pub mod fault;
#[cfg(all(
    target_arch = "x86_64",
    any(target_os = "linux", target_os = "windows")
))]
pub mod hooks;
#[cfg(all(
    target_arch = "x86_64",
    any(target_os = "linux", target_os = "windows")
))]
pub mod image;
pub mod patches;

const MAX_IMAGE_SIZE: u64 = 512 * 1024 * 1024;
const MAX_SEGMENTS: u64 = 64;
const MAX_RELOCS: u64 = 1_000_000;
const MAX_IMPORTS: u64 = 100_000;
const MAX_MODULES: u64 = 8;
const MAX_TLS_MEMSZ: u64 = 1024 * 1024;
const MAX_PATCH_FILES: usize = 4;

/// Parsed `uncharted-run` command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Probe,
    ForceFault,
    ControllerInfo,
    ControllerTest(i32),
    ControllerWatch(i32),
    Boot(BootOptions),
}

/// Options for running or inspecting a prepared `boot*.bin` image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootOptions {
    pub boot_file: String,
    pub cpu_only: bool,
    pub strict_imports: bool,
    pub list_missing: bool,
    pub content_profile: Option<String>,
    pub app0: Option<String>,
    pub user_dir: Option<String>,
    pub patch_files: Vec<String>,
    pub guest_argv_file: Option<String>,
    pub trophy_dir: Option<String>,
    pub timeout_seconds: u64,
}

impl BootOptions {
    fn new(boot_file: String) -> Self {
        Self {
            boot_file,
            cpu_only: false,
            strict_imports: false,
            list_missing: false,
            content_profile: None,
            app0: None,
            user_dir: None,
            patch_files: Vec::new(),
            guest_argv_file: None,
            trophy_dir: None,
            timeout_seconds: 10,
        }
    }
}

/// Parse `uncharted-run` CLI arguments (excluding argv[0]).
pub fn parse_args(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err(
            "usage: uncharted-run <boot.bin> [options] | --probe | --force-fault | --controller-info"
                .into(),
        );
    }
    if args == ["--probe"] {
        return Ok(Command::Probe);
    }
    if args == ["--force-fault"] {
        return Ok(Command::ForceFault);
    }
    if args == ["--controller-info"] {
        return Ok(Command::ControllerInfo);
    }
    if args.len() == 2 && args[0] == "--controller-test" {
        let strength = args[1]
            .parse::<i32>()
            .map_err(|_| format!("invalid --controller-test strength: {}", args[1]))?;
        return Ok(Command::ControllerTest(strength));
    }
    if args.len() == 2 && args[0] == "--controller-watch" {
        let secs = args[1]
            .parse::<i32>()
            .map_err(|_| format!("invalid --controller-watch seconds: {}", args[1]))?;
        return Ok(Command::ControllerWatch(secs));
    }
    if args[0].starts_with('-') {
        return Err(format!("unknown option: {}", args[0]));
    }

    let mut opts = BootOptions::new(args[0].clone());
    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        let next = |idx: &mut usize| -> Result<String, String> {
            *idx += 1;
            args.get(*idx)
                .cloned()
                .ok_or_else(|| format!("missing value for {flag}"))
        };
        match flag {
            "--cpu-only" => opts.cpu_only = true,
            "--strict-imports" => opts.strict_imports = true,
            "--list-missing" => opts.list_missing = true,
            "--content-profile" => opts.content_profile = Some(next(&mut i)?),
            "--app0" => opts.app0 = Some(next(&mut i)?),
            "--user" => opts.user_dir = Some(next(&mut i)?),
            "--patches" => {
                if opts.patch_files.len() >= MAX_PATCH_FILES {
                    return Err("too many --patches".into());
                }
                opts.patch_files.push(next(&mut i)?);
            }
            "--guest-argv" => opts.guest_argv_file = Some(next(&mut i)?),
            "--trophies" => opts.trophy_dir = Some(next(&mut i)?),
            "--timeout" => {
                let raw = next(&mut i)?;
                opts.timeout_seconds = raw
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --timeout: {raw}"))?;
            }
            other => return Err(format!("Unknown option: {other}")),
        }
        i += 1;
    }
    Ok(Command::Boot(opts))
}

/// A 28-byte `BBCONT01` content profile record (`crates/core::content_profile`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentProfile(pub [u32; 5]);

impl ContentProfile {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != 28 || &bytes[..8] != b"BBCONT01" {
            return Err("invalid content profile".into());
        }
        let mut values = [0u32; 5];
        for (i, slot) in values.iter_mut().enumerate() {
            let start = 8 + i * 4;
            *slot = u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap());
        }
        Ok(Self(values))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeFormat {
    V1,
    V2,
    V3,
    V4,
    V5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub address: u64,
    pub size: u64,
    pub flags: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reloc {
    pub target: u64,
    pub kind: u64,
    pub value: u64,
    pub addend: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkedModule {
    pub base: u64,
    pub size: u64,
    pub init: u64,
    pub tls_address: u64,
    pub tls_memsz: u64,
    pub tls_filesz: u64,
    pub tls_module: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TlsMetadata {
    pub address: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingImport {
    pub nid: String,
    pub symbol: Option<&'static str>,
    pub is_data: bool,
}

/// Validated `BBPROBE1`–`BBPROBE5` boot image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootImage {
    pub format: ProbeFormat,
    pub size: u64,
    pub entry: u64,
    pub capabilities: u64,
    pub procparam: u64,
    pub main_tls: TlsMetadata,
    pub modules: Vec<LinkedModule>,
    pub bindings: Vec<Option<(u64, u64)>>,
    pub segments: Vec<Segment>,
    pub import_names: Vec<String>,
    pub relocs: Vec<Reloc>,
    pub image_offset: usize,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| "boot file offset overflow".to_string())?;
        if end > self.bytes.len() {
            return Err("truncated boot file".into());
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u64(&mut self) -> Result<u64, String> {
        let slice = self.read_bytes(8)?;
        Ok(u64::from_le_bytes(slice.try_into().unwrap()))
    }
}

pub fn is_mapped(segments: &[Segment], address: u64, bytes: u64) -> bool {
    segments
        .iter()
        .any(|s| address >= s.address && s.size >= bytes && address - s.address <= s.size - bytes)
}

impl BootImage {
    /// Parse and validate a `BBPROBE1`–`BBPROBE5` boot image from `bytes`.
    pub fn parse(bytes: &[u8], page_size: u64) -> Result<Self, String> {
        if page_size == 0 || !page_size.is_power_of_two() {
            return Err("invalid host page size".into());
        }
        let mut cur = Cursor::new(bytes);
        let magic = cur.read_bytes(8)?;
        let format = match magic {
            b"BBPROBE1" => ProbeFormat::V1,
            b"BBPROBE2" => ProbeFormat::V2,
            b"BBPROBE3" => ProbeFormat::V3,
            b"BBPROBE4" => ProbeFormat::V4,
            b"BBPROBE5" => ProbeFormat::V5,
            _ => return Err("bad boot file signature".into()),
        };

        let size = cur.read_u64()?;
        let entry = cur.read_u64()?;
        let ns = cur.read_u64()?;
        let nr = cur.read_u64()?;
        let import_count = cur.read_u64()?;
        let capabilities = if format == ProbeFormat::V1 {
            0
        } else {
            cur.read_u64()?
        };
        if (capabilities & !1) != 0 {
            return Err("unknown runtime capabilities".into());
        }
        if size == 0
            || size > MAX_IMAGE_SIZE
            || entry >= size
            || ns == 0
            || ns > MAX_SEGMENTS
            || nr > MAX_RELOCS
            || import_count > MAX_IMPORTS
        {
            return Err("boot file limits exceeded".into());
        }

        let multi = format == ProbeFormat::V5;
        let linked = matches!(format, ProbeFormat::V3 | ProbeFormat::V4 | ProbeFormat::V5);
        let mut main_tls = TlsMetadata::default();
        let mut modules = Vec::new();
        let mut procparam = 0u64;
        let mut nb = 0u64;
        let mut bindings = vec![None; import_count as usize];

        if multi {
            procparam = cur.read_u64()?;
            main_tls = TlsMetadata {
                address: cur.read_u64()?,
                filesz: cur.read_u64()?,
                memsz: cur.read_u64()?,
                align: cur.read_u64()?,
            };
            let module_count = cur.read_u64()?;
            if module_count == 0 || module_count > MAX_MODULES {
                return Err("invalid module count".into());
            }
            for _ in 0..module_count {
                modules.push(LinkedModule {
                    base: cur.read_u64()?,
                    size: cur.read_u64()?,
                    init: cur.read_u64()?,
                    tls_address: cur.read_u64()?,
                    tls_memsz: cur.read_u64()?,
                    tls_filesz: cur.read_u64()?,
                    tls_module: cur.read_u64()?,
                });
            }
            nb = cur.read_u64()?;
        } else if linked {
            modules.push(LinkedModule {
                base: cur.read_u64()?,
                size: cur.read_u64()?,
                init: cur.read_u64()?,
                tls_address: cur.read_u64()?,
                tls_memsz: cur.read_u64()?,
                tls_filesz: cur.read_u64()?,
                tls_module: 2,
            });
            nb = cur.read_u64()?;
            procparam = cur.read_u64()?;
            if format == ProbeFormat::V4 {
                main_tls = TlsMetadata {
                    address: cur.read_u64()?,
                    filesz: cur.read_u64()?,
                    memsz: cur.read_u64()?,
                    align: cur.read_u64()?,
                };
            }
        }

        if linked {
            if main_tls.filesz > main_tls.memsz
                || main_tls.memsz > MAX_TLS_MEMSZ
                || main_tls.address > size
                || main_tls.filesz > size - main_tls.address
                || (main_tls.align & main_tls.align.wrapping_sub(1)) != 0
                || main_tls.align > 4096
            {
                return Err("invalid eboot TLS metadata".into());
            }
            for m in &modules {
                if m.base >= size
                    || m.size == 0
                    || m.size > size - m.base
                    || m.init < m.base
                    || m.init - m.base >= m.size
                    || (m.tls_module != 0
                        && (m.tls_address >= size
                            || m.tls_memsz > size - m.tls_address
                            || m.tls_memsz > MAX_TLS_MEMSZ
                            || m.tls_filesz > m.tls_memsz
                            || !(2..=7).contains(&m.tls_module)))
                {
                    return Err("invalid linked module metadata".into());
                }
            }
            if nb > import_count {
                return Err("invalid binding count".into());
            }
            for _ in 0..nb {
                let index = cur.read_u64()?;
                let address = cur.read_u64()?;
                let kind = cur.read_u64()?;
                let inside = modules
                    .iter()
                    .any(|m| address >= m.base && address - m.base < m.size);
                if index >= import_count
                    || !inside
                    || (kind != 1 && kind != 2)
                    || bindings[index as usize].is_some()
                {
                    return Err("invalid native binding".into());
                }
                bindings[index as usize] = Some((address, kind));
            }
        }

        let mut segments = Vec::with_capacity(ns as usize);
        for _ in 0..ns {
            let address = cur.read_u64()?;
            let seg_size = cur.read_u64()?;
            let flags = cur.read_u64()?;
            if address > size || seg_size > size - address || address % page_size != 0 || flags > 7
            {
                return Err("bad segment".into());
            }
            segments.push(Segment {
                address,
                size: seg_size,
                flags,
            });
        }

        let mut import_names = Vec::with_capacity(import_count as usize);
        for _ in 0..import_count {
            let raw = cur.read_bytes(128)?;
            let nul = raw
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| "unterminated import name".to_string())?;
            let name = std::str::from_utf8(&raw[..nul])
                .map_err(|_| "invalid UTF-8 in import name".to_string())?;
            import_names.push(name.to_owned());
        }

        let mut relocs = Vec::with_capacity(nr as usize);
        for _ in 0..nr {
            let target = cur.read_u64()?;
            let kind = cur.read_u64()?;
            let value = cur.read_u64()?;
            let addend = cur.read_u64()?;
            if !is_mapped(&segments, target, 8)
                || kind > 2
                || (kind != 0 && value >= import_count)
                || (kind != 2 && addend != 0)
                || addend >= page_size
            {
                return Err("bad relocation".into());
            }
            relocs.push(Reloc {
                target,
                kind,
                value,
                addend,
            });
        }

        for m in &modules {
            if !is_mapped(&segments, m.init, 1)
                || (m.tls_module != 0 && !is_mapped(&segments, m.tls_address, m.tls_memsz))
            {
                return Err("unmapped module metadata".into());
            }
        }
        if !modules.is_empty() && !is_mapped(&segments, procparam, 64) {
            return Err("unmapped procparam".into());
        }
        for &(address, kind) in bindings.iter().flatten() {
            if !is_mapped(&segments, address, 1) {
                return Err("unmapped native export".into());
            }
            if kind == 1 {
                let executable = segments.iter().any(|s| {
                    (s.flags & 1) != 0 && address >= s.address && address - s.address < s.size
                });
                if !executable {
                    return Err("native function is not executable".into());
                }
            }
        }

        let image_offset = cur.pos;
        if bytes.len() - image_offset != size as usize {
            return Err("incorrect memory image size".into());
        }

        Ok(Self {
            format,
            size,
            entry,
            capabilities,
            procparam,
            main_tls,
            modules,
            bindings,
            segments,
            import_names,
            relocs,
            image_offset,
        })
    }

    /// Return all imports referenced by relocations that neither `abi::runtime_resolve`
    /// nor (when `native_libc` is enabled) native module bindings satisfy.
    pub fn missing_imports(&self, strict_imports: bool) -> Vec<MissingImport> {
        let native_libc = matches!(
            self.format,
            ProbeFormat::V3 | ProbeFormat::V4 | ProbeFormat::V5
        ) && !strict_imports
            && (self.capabilities & 1) != 0;
        let mut kinds = vec![0u8; self.import_names.len()];
        for r in &self.relocs {
            if r.kind != 0 {
                kinds[r.value as usize] = r.kind as u8;
            }
        }
        let mut missing = Vec::new();
        for (idx, name) in self.import_names.iter().enumerate() {
            let kind = kinds[idx];
            if kind == 0 {
                continue;
            }
            let is_data = kind == 2;
            let resolved = std::ffi::CString::new(name.as_str())
                .ok()
                .and_then(|c| crate::abi::runtime_resolve(&c, is_data))
                .is_some();
            if resolved || (native_libc && self.bindings[idx].is_some()) {
                continue;
            }
            let symbol = crate::abi::nid_name(name.as_str());
            missing.push(MissingImport {
                nid: name.clone(),
                symbol,
                is_data,
            });
        }
        missing
    }
}

/// Parse a space-separated hex byte pattern with `"??"` wildcards and find all matches in `haystack`.
pub fn find_pattern(haystack: &[u8], pattern: &str) -> Result<Vec<usize>, String> {
    let mut parsed = Vec::new();
    for token in pattern.split_whitespace() {
        if token == "??" {
            parsed.push(None);
        } else {
            let byte = u8::from_str_radix(token, 16)
                .map_err(|_| format!("invalid pattern byte: {token}"))?;
            parsed.push(Some(byte));
        }
    }
    if parsed.is_empty() {
        return Err("empty pattern".into());
    }
    if haystack.len() < parsed.len() {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for offset in 0..=haystack.len() - parsed.len() {
        let matched = parsed
            .iter()
            .enumerate()
            .all(|(i, expected)| expected.is_none_or(|b| haystack[offset + i] == b));
        if matched {
            hits.push(offset);
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u64(buf: &mut Vec<u8>, v: u64) {
        buf.extend_from_slice(&v.to_le_bytes());
    }

    fn build_bbprobe2(size: u64, entry: u64, imports: &[&str], relocs: &[Reloc]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"BBPROBE2");
        push_u64(&mut buf, size);
        push_u64(&mut buf, entry);
        push_u64(&mut buf, 1); // ns
        push_u64(&mut buf, relocs.len() as u64);
        push_u64(&mut buf, imports.len() as u64);
        push_u64(&mut buf, 1); // capabilities
                               // segment 0: [0..size], rwx
        push_u64(&mut buf, 0);
        push_u64(&mut buf, size);
        push_u64(&mut buf, 7);
        for name in imports {
            let mut slot = [0u8; 128];
            let b = name.as_bytes();
            slot[..b.len()].copy_from_slice(b);
            buf.extend_from_slice(&slot);
        }
        for r in relocs {
            push_u64(&mut buf, r.target);
            push_u64(&mut buf, r.kind);
            push_u64(&mut buf, r.value);
            push_u64(&mut buf, r.addend);
        }
        buf.resize(buf.len() + size as usize, 0);
        buf
    }

    #[test]
    fn parse_args_handles_probe_fault_and_boot_options() {
        assert_eq!(parse_args(&["--probe".into()]).unwrap(), Command::Probe);
        assert_eq!(
            parse_args(&["--force-fault".into()]).unwrap(),
            Command::ForceFault
        );
        let cmd = parse_args(&[
            "boot.bin".into(),
            "--cpu-only".into(),
            "--list-missing".into(),
            "--user".into(),
            "/tmp/u".into(),
            "--patches".into(),
            "p1.bin".into(),
            "--timeout".into(),
            "30".into(),
        ])
        .unwrap();
        let Command::Boot(opts) = cmd else {
            panic!("expected boot command");
        };
        assert_eq!(opts.boot_file, "boot.bin");
        assert!(opts.cpu_only && opts.list_missing);
        assert_eq!(opts.user_dir.as_deref(), Some("/tmp/u"));
        assert_eq!(opts.patch_files, vec!["p1.bin"]);
        assert_eq!(opts.timeout_seconds, 30);

        assert!(parse_args(&[
            "boot.bin".into(),
            "--patches".into(),
            "1".into(),
            "--patches".into(),
            "2".into(),
            "--patches".into(),
            "3".into(),
            "--patches".into(),
            "4".into(),
            "--patches".into(),
            "5".into(),
        ])
        .is_err());
    }

    #[test]
    fn parse_bbprobe2_and_check_missing_imports() {
        let bytes = build_bbprobe2(
            4096,
            0x100,
            &["unknownNid#x#y", "_init_env"],
            &[Reloc {
                target: 0x20,
                kind: 1,
                value: 0,
                addend: 0,
            }],
        );
        let image = BootImage::parse(&bytes, 4096).unwrap();
        assert_eq!(image.format, ProbeFormat::V2);
        assert_eq!(image.entry, 0x100);
        let missing = image.missing_imports(false);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].nid, "unknownNid#x#y");
        assert!(!missing[0].is_data);
    }

    #[test]
    fn parse_rejects_bad_signature_and_out_of_bounds_reloc() {
        assert!(BootImage::parse(b"BADMAGIC", 4096).is_err());
        let bad_reloc = build_bbprobe2(
            4096,
            0x10,
            &[],
            &[Reloc {
                target: 4092, // crosses 4096 boundary for 8-byte write
                kind: 0,
                value: 0,
                addend: 0,
            }],
        );
        assert!(BootImage::parse(&bad_reloc, 4096).is_err());
    }

    #[test]
    fn content_profile_and_pattern_scanner() {
        let mut prof = Vec::from(&b"BBCONT01"[..]);
        for v in [1u32, 2, 3, 4, 5] {
            prof.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(ContentProfile::parse(&prof).unwrap().0, [1, 2, 3, 4, 5]);
        assert!(ContentProfile::parse(b"BBCONT01short").is_err());

        let hits = find_pattern(
            &[0x48, 0x8b, 0x05, 0x10, 0x48, 0x8b, 0xff, 0x10],
            "48 8b ?? 10",
        )
        .unwrap();
        assert_eq!(hits, vec![0, 4]);
    }
}
