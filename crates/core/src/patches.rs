//! Third-party patches (shadPS4/GoldHEN XML): parse, pick the ones for this executable and
//! revision, apply, list for About. Replaces `scripts/patches.py` (the external-patch half; its
//! Bloodborne-only built-in database `patches/Bloodborne.xml`, FPS presets and effect switches
//! are not carried over). Patches are byte writes at the addresses of ONE build, so they are
//! keyed by title id, executable (`AppElf`) and `APP_VER`, and never applied to another build.
//!
//! Only literal writes are supported (`bytes`, `bytes16/32/64`, `float32/64`, `utf8`,
//! `utf16`); a pattern ("mask") patch is skipped whole, never half-applied. XML is parsed by
//! `roxmltree` 0.21.1 (MIT option), with DTD and external-entity resolution disabled and bounds
//! on input size, node count and nesting depth.

use crate::error::{Error, Result};
use crate::games::Game;
use roxmltree::{Document, Node, ParsingOptions};

/// Patch addresses are PS4 virtual addresses with the executable at this base.
pub const EBOOT_BASE: u64 = 0x40_0000;
/// The first eight bytes of `patches.bin`.
pub const PATCHES_MAGIC: [u8; 8] = *b"BBPATCH2";

const MAX_XML_BYTES: usize = 8 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 64;
const MAX_XML_NODES: u32 = 100_000;
/// `apply_patches` in `src/probe.c` reads a fixed 4096-byte entry buffer.
const MAX_PATCH_WRITE_BYTES: usize = 4096;

/// One byte write, at an offset into the executable's memory image (`address - EBOOT_BASE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteWrite {
    pub offset: u64,
    pub bytes: Vec<u8>,
}

/// What a patch is for. A patch applies only when all three match the dump.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    /// `param.sfo` `TITLE_ID` ("CUSA02320"); a patch file may list several.
    pub title_id: String,
    /// `AppElf` ("eboot.bin" when the file does not say): `Game::executable`.
    pub executable: String,
    /// `AppVer` ("01.02"): `param.sfo` `APP_VER`, falling back to `VERSION` only when missing.
    pub app_ver: String,
}

/// One patch of a patch file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// "<file name>/<patch name>": the launcher's selection key (`patches.json`).
    pub key: String,
    pub name: String,
    pub author: String,
    pub note: String,
    /// The title ids the file lists; empty when it lists none.
    pub title_ids: Vec<String>,
    pub executable: String,
    pub app_ver: String,
    /// `isEnabled` in the file.
    pub enabled_by_default: bool,
    pub writes: Vec<ByteWrite>,
}

/// The player's choice (`patches.json`): `enabled` and `disabled` override `isEnabled` per key.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
}

/// Parse one patch XML file (`file_name` forms the keys). Bad XML is `Error::Patch(..)`; a
/// patch with an unsupported line type is kept with `writes` empty and a note, so About can
/// list it as unavailable.
pub fn parse(file_name: &str, xml: &str) -> Result<Vec<Patch>> {
    if xml.len() > MAX_XML_BYTES {
        return Err(patch_error("XML patch file exceeds the 8 MiB limit"));
    }
    validate_xml_character_references(xml)?;
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: MAX_XML_NODES,
        entity_resolver: None,
    };
    let document = Document::parse_with_options(xml, options)
        .map_err(|error| patch_error(error.to_string()))?;
    let root = document.root_element();
    check_xml_depth(root)?;

    let title_ids = root
        .descendants()
        .filter(|node| node.has_tag_name("ID"))
        .filter_map(|node| node.text())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let metadata = root
        .descendants()
        .filter(|node| node.has_tag_name("Metadata"))
        .collect::<Vec<_>>();
    let mut patches = Vec::with_capacity(metadata.len());
    for meta in metadata {
        let name = meta
            .attribute("Name")
            .ok_or_else(|| patch_error("patch metadata is missing Name"))?
            .to_owned();
        let key = format!("{file_name}/{name}");
        let mut writes = Vec::new();
        let mut unavailable = None;
        for line in meta.descendants().filter(|node| node.has_tag_name("Line")) {
            match encode_line(line) {
                Ok(write) => writes.push(write),
                Err(message) => {
                    unavailable = Some(message);
                    break;
                }
            }
        }
        if let Some(reason) = unavailable {
            writes.clear();
            patches.push(Patch {
                key,
                name,
                author: meta.attribute("Author").unwrap_or_default().to_owned(),
                note: append_unavailable(meta.attribute("Note").unwrap_or_default(), &reason),
                title_ids: title_ids.clone(),
                executable: meta.attribute("AppElf").unwrap_or("eboot.bin").to_owned(),
                app_ver: meta.attribute("AppVer").unwrap_or_default().to_owned(),
                enabled_by_default: attr_true(meta, "isEnabled"),
                writes,
            });
        } else {
            patches.push(Patch {
                key,
                name,
                author: meta.attribute("Author").unwrap_or_default().to_owned(),
                note: meta.attribute("Note").unwrap_or_default().to_owned(),
                title_ids: title_ids.clone(),
                executable: meta.attribute("AppElf").unwrap_or("eboot.bin").to_owned(),
                app_ver: meta.attribute("AppVer").unwrap_or_default().to_owned(),
                enabled_by_default: attr_true(meta, "isEnabled"),
                writes,
            });
        }
    }
    Ok(patches)
}

/// `roxmltree` normalizes attribute values and validates XML grammar. It replaces invalid Unicode
/// scalar references (including surrogates and values above U+10FFFF) with U+FFFD, so this focused
/// pass rejects numeric references outside XML 1.0 Char before the parser sees them. It skips
/// contexts where `&#...;` is ordinary comment/CDATA/PI text.
fn validate_xml_character_references(xml: &str) -> Result<()> {
    let mut pos = 0;
    while pos < xml.len() {
        let rest = &xml[pos..];
        let skip = if rest.starts_with("<!--") {
            Some(("<!--", "-->"))
        } else if rest.starts_with("<![CDATA[") {
            Some(("<![CDATA[", "]]>"))
        } else if rest.starts_with("<?") {
            Some(("<?", "?>"))
        } else {
            None
        };
        if let Some((opening, closing)) = skip {
            let after_opening = pos + opening.len();
            let end = xml[after_opening..]
                .find(closing)
                .map(|offset| after_opening + offset + closing.len())
                .ok_or_else(|| {
                    patch_error("unterminated XML comment, CDATA or processing instruction")
                })?;
            pos = end;
            continue;
        }
        if let Some(reference) = rest.strip_prefix("&#") {
            let end = reference
                .find(';')
                .ok_or_else(|| patch_error("unterminated XML character reference"))?;
            let digits = &reference[..end];
            let (number, radix) = if let Some(hex) = digits.strip_prefix('x') {
                (hex, 16)
            } else {
                (digits, 10)
            };
            if number.is_empty() || !number.chars().all(|ch| ch.is_digit(radix)) {
                return Err(patch_error("invalid XML character reference"));
            }
            let value = u32::from_str_radix(number, radix)
                .map_err(|_| patch_error("invalid XML character reference"))?;
            if !is_xml10_char(value) {
                return Err(patch_error(
                    "XML character reference names a character forbidden by XML 1.0",
                ));
            }
            pos += 2 + end + 1;
        } else {
            let ch = rest.chars().next().expect("position is before end");
            pos += ch.len_utf8();
        }
    }
    Ok(())
}

fn is_xml10_char(value: u32) -> bool {
    matches!(value, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

fn check_xml_depth(root: Node<'_, '_>) -> Result<()> {
    let mut pending = vec![(root, 1usize)];
    while let Some((node, depth)) = pending.pop() {
        if depth > MAX_XML_DEPTH {
            return Err(patch_error("XML patch file is nested too deeply"));
        }
        for child in node.children().filter(Node::is_element) {
            pending.push((child, depth + 1));
        }
    }
    Ok(())
}

fn append_unavailable(note: &str, reason: &str) -> String {
    if note.is_empty() {
        format!("Unavailable: {reason}")
    } else {
        format!("{note} (Unavailable: {reason})")
    }
}

fn attr_true(element: Node<'_, '_>, name: &str) -> bool {
    element
        .attribute(name)
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn encode_line(line: Node<'_, '_>) -> std::result::Result<ByteWrite, String> {
    let kind = line
        .attribute("Type")
        .ok_or_else(|| "line is missing Type".to_owned())?;
    if !matches!(
        kind,
        "bytes" | "bytes16" | "bytes32" | "bytes64" | "float32" | "float64" | "utf8" | "utf16"
    ) {
        return Err(format!("unsupported patch type {kind:?}"));
    }
    let address = line
        .attribute("Address")
        .ok_or_else(|| "line is missing Address".to_owned())?;
    let address = parse_u64(address).map_err(|_| format!("invalid address {address:?}"))?;
    let offset = address
        .checked_sub(EBOOT_BASE)
        .ok_or_else(|| "address is below EBOOT_BASE".to_owned())?;
    let value = line
        .attribute("Value")
        .ok_or_else(|| "line is missing Value".to_owned())?;
    let bytes = match kind {
        "bytes" => decode_hex(value)?,
        "bytes16" | "bytes32" | "bytes64" => {
            let width = kind[5..]
                .parse::<usize>()
                .map_err(|_| "invalid integer width".to_owned())?
                / 8;
            let number =
                parse_u64(value).map_err(|_| format!("invalid integer value {value:?}"))?;
            let max = if width == 8 {
                u64::MAX
            } else {
                (1u64 << (width * 8)) - 1
            };
            if number > max {
                return Err(format!("integer value does not fit {kind}"));
            }
            number.to_le_bytes()[..width].to_vec()
        }
        "float32" => {
            let parsed = value.parse::<f32>().ok().filter(|value| value.is_finite());
            let value = parsed.ok_or_else(|| format!("invalid float32 value {value:?}"))?;
            value.to_le_bytes().to_vec()
        }
        "float64" => {
            let parsed = value.parse::<f64>().ok().filter(|value| value.is_finite());
            let value = parsed.ok_or_else(|| format!("invalid float64 value {value:?}"))?;
            value.to_le_bytes().to_vec()
        }
        "utf8" => {
            let mut bytes = value.as_bytes().to_vec();
            bytes.push(0);
            bytes
        }
        "utf16" => {
            let mut bytes = value
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            bytes.extend_from_slice(&[0, 0]);
            bytes
        }
        unsupported => return Err(format!("unsupported patch type {unsupported:?}")),
    };
    if bytes.is_empty() {
        return Err("literal patch line has no bytes".to_owned());
    }
    if bytes.len() > MAX_PATCH_WRITE_BYTES {
        return Err(format!(
            "literal write exceeds the {MAX_PATCH_WRITE_BYTES}-byte loader limit"
        ));
    }
    Ok(ByteWrite { offset, bytes })
}

fn parse_u64(value: &str) -> std::result::Result<u64, ()> {
    let trimmed = value.trim();
    let (digits, radix) = match trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        Some(hex) => (hex, 16),
        None => (trimmed, 10),
    };
    // `from_str_radix` accepts a leading sign; a literal must be plain digits.
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_digit(radix)) {
        return Err(());
    }
    u64::from_str_radix(digits, radix).map_err(|_| ())
}

fn decode_hex(value: &str) -> std::result::Result<Vec<u8>, String> {
    let digits = value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if digits.len() % 2 != 0 {
        return Err("bytes value has an odd number of hexadecimal digits".to_owned());
    }
    if !digits.iter().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("bytes value has a non-hexadecimal character".to_owned());
    }
    digits
        .as_slice()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let hex = pair.iter().collect::<String>();
            u8::from_str_radix(&hex, 16).map_err(|_| format!("invalid hexadecimal byte {hex:?}"))
        })
        .collect()
}

fn patch_error(message: impl Into<String>) -> Error {
    Error::Patch(message.into())
}

/// The patches that may be written to this dump: the file lists no title ids or lists
/// `target.title_id`, and `executable` and `app_ver` match. A scoped patch is skipped whenever
/// the target title id is unknown; a missing identity never authorizes a guess. The final
/// argument remains for API compatibility; scoped patches always require an exact title id.
pub fn applicable<'a>(patches: &'a [Patch], target: &Target, _fail_closed: bool) -> Vec<&'a Patch> {
    // An unknown executable or revision matches nothing: a missing identity never authorizes a guess.
    if target.executable.is_empty() || target.app_ver.is_empty() {
        return Vec::new();
    }
    patches
        .iter()
        .filter(|patch| {
            if patch.executable != target.executable || patch.app_ver != target.app_ver {
                return false;
            }
            if patch.title_ids.is_empty() {
                return true;
            }
            // A scoped patch must never be authorized by an unknown title id.
            if target.title_id.is_empty() {
                return false;
            }
            patch.title_ids.iter().any(|id| id == &target.title_id)
        })
        .collect()
}

/// The selected patches among `found`: `Selection` first, else `enabled_by_default`.
pub fn select<'a>(found: &[&'a Patch], selection: &Selection) -> Vec<&'a Patch> {
    found
        .iter()
        .copied()
        .filter(|patch| {
            selection.enabled.iter().any(|key| key == &patch.key)
                || (!selection.disabled.iter().any(|key| key == &patch.key)
                    && patch.enabled_by_default)
        })
        .collect()
}

/// The writes of `selected`, checked to lie inside the executable's loadable `segments`
/// (`(start, end)` offsets from [`eboot_segments`]), contain no empty writes, and fit the
/// loader's per-write limit; an invalid patch is skipped whole and named in the returned notes.
pub fn compile(
    selected: &[&Patch],
    segments: &[(u64, u64)],
) -> Result<(Vec<ByteWrite>, Vec<String>)> {
    let mut writes = Vec::new();
    let mut notes = Vec::new();
    for patch in selected {
        if patch.writes.is_empty() {
            notes.push(format!(
                "{}: patch has no supported literal writes",
                patch.key
            ));
            continue;
        }
        let oversized = patch
            .writes
            .iter()
            .any(|write| write.bytes.len() > MAX_PATCH_WRITE_BYTES);
        let empty_write = patch.writes.iter().any(|write| write.bytes.is_empty());
        let in_bounds = patch.writes.iter().all(|write| {
            let Some(len) = u64::try_from(write.bytes.len()).ok() else {
                return false;
            };
            let Some(end) = write.offset.checked_add(len) else {
                return false;
            };
            segments
                .iter()
                .any(|(start, stop)| start <= &write.offset && end <= *stop && start < stop)
        });
        if !oversized && !empty_write && in_bounds {
            writes.extend(patch.writes.iter().cloned());
        } else if oversized {
            notes.push(format!(
                "{}: a write exceeds the {MAX_PATCH_WRITE_BYTES}-byte loader limit",
                patch.key
            ));
        } else if empty_write {
            notes.push(format!(
                "{}: a write has zero bytes, which the loader rejects",
                patch.key
            ));
        } else {
            notes.push(format!(
                "{}: one or more writes fall outside the executable",
                patch.key
            ));
        }
    }
    Ok((writes, notes))
}

/// The loadable segments of an ELF image as `(offset, offset + memsz)` relative to `EBOOT_BASE`.
pub fn eboot_segments(elf: &[u8]) -> Result<Vec<(u64, u64)>> {
    const ELF_HEADER: usize = 64;
    const PROGRAM_HEADER: usize = 56;
    if elf.len() < ELF_HEADER
        || &elf[..4] != b"\x7fELF"
        || elf[4] != 2
        || elf[5] != 1
        || elf[6] != 1
    {
        return Err(patch_error("invalid ELF64 little-endian executable header"));
    }
    let phoff = read_u64(elf, 32)?;
    let phentsize = read_u16(elf, 54)? as usize;
    let phnum = read_u16(elf, 56)? as usize;
    if phentsize < PROGRAM_HEADER || phnum == 0 {
        return Err(patch_error("ELF has no valid program-header table"));
    }
    let table_start = usize::try_from(phoff)
        .map_err(|_| patch_error("ELF program-header offset is too large"))?;
    let table_bytes = phentsize
        .checked_mul(phnum)
        .ok_or_else(|| patch_error("ELF program-header table overflows"))?;
    let table_end = table_start
        .checked_add(table_bytes)
        .ok_or_else(|| patch_error("ELF program-header table overflows"))?;
    if table_end > elf.len() {
        return Err(patch_error("ELF program-header table is truncated"));
    }
    let mut segments = Vec::new();
    for index in 0..phnum {
        let at = table_start + index * phentsize;
        if read_u32(elf, at)? != 1 {
            continue;
        } // PT_LOAD
        let file_offset = read_u64(elf, at + 8)?;
        let vaddr = read_u64(elf, at + 16)?;
        let filesz = read_u64(elf, at + 32)?;
        let memsz = read_u64(elf, at + 40)?;
        if filesz > memsz {
            return Err(patch_error("ELF loadable segment filesz exceeds memsz"));
        }
        let file_end = file_offset
            .checked_add(filesz)
            .ok_or_else(|| patch_error("ELF loadable segment file range overflows"))?;
        if file_end > elf.len() as u64 {
            return Err(patch_error("ELF loadable segment exceeds the file"));
        }
        let absolute_end = vaddr
            .checked_add(memsz)
            .ok_or_else(|| patch_error("ELF loadable segment address range overflows"))?;
        let start = vaddr
            .checked_sub(EBOOT_BASE)
            .ok_or_else(|| patch_error("ELF loadable segment starts below EBOOT_BASE"))?;
        let end = absolute_end
            .checked_sub(EBOOT_BASE)
            .ok_or_else(|| patch_error("ELF loadable segment address range overflows"))?;
        if memsz == 0 {
            continue;
        }
        segments.push((start, end));
    }
    if segments.is_empty() {
        return Err(patch_error("ELF has no non-empty loadable segments"));
    }
    segments.sort_unstable();
    Ok(segments)
}

fn read_u16(data: &[u8], at: usize) -> Result<u16> {
    let bytes = data
        .get(at..at + 2)
        .ok_or_else(|| patch_error("truncated ELF header"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}
fn read_u32(data: &[u8], at: usize) -> Result<u32> {
    let bytes = data
        .get(at..at + 4)
        .ok_or_else(|| patch_error("truncated ELF program header"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
fn read_u64(data: &[u8], at: usize) -> Result<u64> {
    let bytes = data
        .get(at..at + 8)
        .ok_or_else(|| patch_error("truncated ELF header"))?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

/// `patches.bin` for the loader: `PATCHES_MAGIC`, `EBOOT_BASE` and the count (little-endian
/// u64), then per write its offset, length and bytes. Pass only writes accepted by [`compile`];
/// this serializer does not truncate or filter them. An empty `writes` list is valid.
pub fn to_patches_bin(writes: &[ByteWrite]) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(&PATCHES_MAGIC);
    output.extend_from_slice(&EBOOT_BASE.to_le_bytes());
    output.extend_from_slice(&(writes.len() as u64).to_le_bytes());
    for write in writes {
        output.extend_from_slice(&write.offset.to_le_bytes());
        output.extend_from_slice(&(write.bytes.len() as u64).to_le_bytes());
        output.extend_from_slice(&write.bytes);
    }
    output
}

/// One line of the About page's patch list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AboutEntry {
    pub key: String,
    pub name: String,
    pub author: String,
    /// Switched on for this dump (selection first, else `enabled_by_default`).
    pub enabled: bool,
    /// False when the patch is for another build, or has no literal writes (see [`parse`]).
    pub available: bool,
    /// One plain sentence when `available` is false, else empty.
    pub why_not: String,
}

/// The patches for About: every parsed patch with its name, author and whether it applies to
/// this dump, so unavailable ones are listed too.
pub fn about_list(patches: &[Patch], target: &Target, selection: &Selection) -> Vec<AboutEntry> {
    patches
        .iter()
        .map(|patch| {
            let identity_matches = applicable(std::slice::from_ref(patch), target, true).len() == 1;
            let has_writes = !patch.writes.is_empty();
            let available = identity_matches
                && has_writes
                && !patch.note.starts_with("Unavailable:")
                && !patch.note.contains("(Unavailable:");
            let enabled = selection.enabled.iter().any(|key| key == &patch.key)
                || (!selection.disabled.iter().any(|key| key == &patch.key)
                    && patch.enabled_by_default);
            let why_not = if available {
                String::new()
            } else if !identity_matches {
                "This patch is for a different title, executable or revision.".to_owned()
            } else if let Some(reason) = patch.note.split("Unavailable:").nth(1) {
                reason.trim_end_matches(')').trim().to_owned()
            } else {
                "This patch has no supported literal writes.".to_owned()
            };
            AboutEntry {
                key: patch.key.clone(),
                name: patch.name.clone(),
                author: patch.author.clone(),
                enabled,
                available,
                why_not,
            }
        })
        .collect()
}

/// The game a patch's `AppElf` belongs to, if any.
pub fn game_of(patch: &Patch) -> Option<Game> {
    Game::from_executable(&patch.executable)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0"?>
<Patches><TitleID><ID>CUSA02320</ID><ID>CUSA02343</ID></TitleID>
<Metadata Name="Literals" Author="A &amp; B" AppVer="01.02" isEnabled="true">
<PatchList>
<Line Type="bytes" Address="0x00401000" Value="01 02"/>
<Line Type="bytes16" Address="0x00401002" Value="0x1234"/>
<Line Type="bytes32" Address="0x00401004" Value="305419896"/>
<Line Type="bytes64" Address="0x00401008" Value="0x0102030405060708"/>
<Line Type="float32" Address="0x00401010" Value="1.5"/>
<Line Type="float64" Address="0x00401014" Value="-2.25"/>
<Line Type="utf8" Address="0x0040101c" Value="A&amp;B"/>
<Line Type="utf16" Address="0x00401020" Value="Hi"/>
</PatchList></Metadata></Patches>"#;

    fn target() -> Target {
        Target {
            title_id: "CUSA02320".into(),
            executable: "eboot.bin".into(),
            app_ver: "01.02".into(),
        }
    }

    #[test]
    fn parses_all_literal_types_and_entities() {
        let parsed = parse("community.xml", XML).unwrap();
        assert_eq!(parsed.len(), 1);
        let patch = &parsed[0];
        assert_eq!(patch.key, "community.xml/Literals");
        assert_eq!(patch.author, "A & B");
        assert_eq!(patch.title_ids, ["CUSA02320", "CUSA02343"]);
        assert_eq!(
            patch
                .writes
                .iter()
                .map(|w| w.bytes.clone())
                .collect::<Vec<_>>(),
            vec![
                vec![1, 2],
                0x1234u16.to_le_bytes().to_vec(),
                0x12345678u32.to_le_bytes().to_vec(),
                0x0102030405060708u64.to_le_bytes().to_vec(),
                1.5f32.to_le_bytes().to_vec(),
                (-2.25f64).to_le_bytes().to_vec(),
                b"A&B\0".to_vec(),
                vec![b'H', 0, b'i', 0, 0, 0],
            ]
        );
    }

    #[test]
    fn malformed_xml_is_patch_error() {
        assert!(matches!(
            parse("bad.xml", "<Patches><Metadata></Patches>"),
            Err(Error::Patch(_))
        ));
        assert!(matches!(
            parse("bad.xml", "<Patches>&unknown;</Patches>"),
            Err(Error::Patch(_))
        ));
        assert!(matches!(
            parse(
                "bad.xml",
                "<Patches><!--bad--comment--><Metadata Name=\"P\" AppVer=\"1\"/></Patches>"
            ),
            Err(Error::Patch(_))
        ));
        assert!(matches!(
            parse("bad.xml", "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"<\"/></Metadata></Patches>"),
            Err(Error::Patch(_))
        ));
        assert!(matches!(
            parse("bad.xml", "<!DOCTYPE Patches [<!ENTITY value \"x\">]><Patches><Metadata Name=\"P\" AppVer=\"1\"/></Patches>"),
            Err(Error::Patch(_))
        ));
        assert!(matches!(
            parse("bad.xml", "<!DOCTYPE Patches SYSTEM \"file:///etc/passwd\"><Patches><Metadata Name=\"P\" AppVer=\"1\"/></Patches>"),
            Err(Error::Patch(_))
        ));
    }

    #[test]
    fn xml_node_and_depth_limits_are_enforced() {
        let at_limit = format!(
            "<Patches>{}{}</Patches>",
            "<N>".repeat(MAX_XML_DEPTH - 1),
            "</N>".repeat(MAX_XML_DEPTH - 1)
        );
        assert!(parse("depth.xml", &at_limit).is_ok());
        let over_limit = format!(
            "<Patches>{}{}</Patches>",
            "<N>".repeat(MAX_XML_DEPTH),
            "</N>".repeat(MAX_XML_DEPTH)
        );
        assert!(matches!(
            parse("depth.xml", &over_limit),
            Err(Error::Patch(_))
        ));

        let too_many_nodes = format!(
            "<Patches>{}</Patches>",
            "<N/>".repeat(MAX_XML_NODES as usize)
        );
        assert!(matches!(
            parse("nodes.xml", &too_many_nodes),
            Err(Error::Patch(_))
        ));
    }

    #[test]
    fn unsupported_or_malformed_line_discards_every_write() {
        let xml = r#"<Patches><Metadata Name="Mask" AppVer="01.02"><Line Type="bytes" Address="0x401000" Value="90"/><Line Type="mask" Address="0x401001" Value="90 ??"/></Metadata></Patches>"#;
        let patch = &parse("m.xml", xml).unwrap()[0];
        assert!(patch.writes.is_empty());
        assert!(patch.note.contains("unsupported patch type"));
        assert!(
            !about_list(
                std::slice::from_ref(patch),
                &Target {
                    title_id: String::new(),
                    executable: "eboot.bin".into(),
                    app_ver: "01.02".into()
                },
                &Selection::default()
            )[0]
            .available
        );
    }

    #[test]
    fn selection_and_target_require_exact_identity() {
        let patches = parse("community.xml", XML).unwrap();
        let target = target();
        assert_eq!(applicable(&patches, &target, true).len(), 1);
        let mut wrong = target.clone();
        wrong.title_id = "CUSA00000".into();
        assert!(applicable(&patches, &wrong, true).is_empty());
        wrong = target.clone();
        wrong.executable = "big2-ps4_Shipping.elf".into();
        assert!(applicable(&patches, &wrong, true).is_empty());
        wrong = target.clone();
        wrong.app_ver = "01.00".into();
        assert!(applicable(&patches, &wrong, true).is_empty());
        wrong = target.clone();
        wrong.title_id.clear();
        assert!(applicable(&patches, &wrong, true).is_empty());
        assert!(applicable(&patches, &wrong, false).is_empty());
        let found = applicable(&patches, &target, true);
        assert_eq!(
            select(
                &found,
                &Selection {
                    enabled: vec![],
                    disabled: vec![patches[0].key.clone()]
                }
            )
            .len(),
            0
        );
        assert_eq!(
            select(
                &found,
                &Selection {
                    enabled: vec![patches[0].key.clone()],
                    disabled: vec![patches[0].key.clone()]
                }
            )
            .len(),
            1
        );
    }

    #[test]
    fn compile_refuses_entire_patch_for_bounds_or_overflow() {
        let patch = Patch {
            key: "x/P".into(),
            name: "P".into(),
            author: String::new(),
            note: String::new(),
            title_ids: vec![],
            executable: "eboot.bin".into(),
            app_ver: "01.02".into(),
            enabled_by_default: true,
            writes: vec![
                ByteWrite {
                    offset: 0,
                    bytes: vec![1],
                },
                ByteWrite {
                    offset: 9,
                    bytes: vec![2],
                },
            ],
        };
        let (writes, notes) = compile(&[&patch], &[(0, 8)]).unwrap();
        assert!(writes.is_empty());
        assert_eq!(notes.len(), 1);
        let overflowing = Patch {
            writes: vec![ByteWrite {
                offset: u64::MAX,
                bytes: vec![1],
            }],
            ..patch
        };
        assert!(compile(&[&overflowing], &[(0, u64::MAX)])
            .unwrap()
            .0
            .is_empty());
    }

    fn synthetic_elf(
        vaddr: u64,
        filesz: u64,
        memsz: u64,
        file_offset: u64,
        file_len: usize,
    ) -> Vec<u8> {
        let segment_end = if file_offset <= 1024 && filesz <= 1024 {
            (file_offset + filesz) as usize
        } else {
            0
        };
        let mut elf = vec![0u8; file_len.max(120).max(segment_end)];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[32..40].copy_from_slice(&64u64.to_le_bytes());
        elf[54..56].copy_from_slice(&56u16.to_le_bytes());
        elf[56..58].copy_from_slice(&1u16.to_le_bytes());
        elf[64..68].copy_from_slice(&1u32.to_le_bytes());
        elf[72..80].copy_from_slice(&file_offset.to_le_bytes());
        elf[80..88].copy_from_slice(&vaddr.to_le_bytes());
        elf[96..104].copy_from_slice(&filesz.to_le_bytes());
        elf[104..112].copy_from_slice(&memsz.to_le_bytes());
        elf
    }

    #[test]
    fn elf_segments_are_base_relative_and_validate_ranges() {
        assert_eq!(
            eboot_segments(&synthetic_elf(EBOOT_BASE + 0x1000, 0x20, 0x40, 120, 120)).unwrap(),
            vec![(0x1000, 0x1040)]
        );
        assert!(matches!(
            eboot_segments(b"not an ELF"),
            Err(Error::Patch(_))
        ));
        assert!(eboot_segments(&synthetic_elf(EBOOT_BASE + 0x1000, 0x20, 0x10, 120, 120)).is_err());
        assert!(eboot_segments(&synthetic_elf(
            EBOOT_BASE + 0x1000,
            0x20,
            0x20,
            u64::MAX - 1,
            120
        ))
        .is_err());
        assert!(eboot_segments(&synthetic_elf(u64::MAX - 2, 1, 8, 120, 120)).is_err());
        let segments =
            eboot_segments(&synthetic_elf(EBOOT_BASE + 0x1000, 0x20, 0x40, 120, 120)).unwrap();
        let patch = Patch {
            key: "x/P".into(),
            name: "P".into(),
            author: String::new(),
            note: String::new(),
            title_ids: vec![],
            executable: "eboot.bin".into(),
            app_ver: "01.02".into(),
            enabled_by_default: true,
            writes: vec![ByteWrite {
                offset: 0x103e,
                bytes: vec![1, 2],
            }],
        };
        assert_eq!(compile(&[&patch], &segments).unwrap().0.len(), 1);
        let outside = Patch {
            writes: vec![ByteWrite {
                offset: 0x1040,
                bytes: vec![1],
            }],
            ..patch
        };
        assert!(compile(&[&outside], &segments).unwrap().0.is_empty());
    }

    #[test]
    fn rejects_xml_10_forbidden_raw_and_numeric_characters() {
        for xml in [
            "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&#x1;\"/></Metadata></Patches>",
            "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&#1;\"/></Metadata></Patches>",
            "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&#xD800;\"/></Metadata></Patches>",
            "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&#xDFFF;\"/></Metadata></Patches>",
            "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&#55296;\"/></Metadata></Patches>",
            "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&#x110000;\"/></Metadata></Patches>",
            "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&#xFFFFFFFF;\"/></Metadata></Patches>",
            "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&#xFFFE;\"/></Metadata></Patches>",
            "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&#X41;\"/></Metadata></Patches>",
        ] {
            assert!(matches!(parse("bad.xml", xml), Err(Error::Patch(_))), "accepted malformed XML: {xml}");
        }
        let raw_forbidden = "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"x\u{1}\"/></Metadata></Patches>";
        assert!(matches!(
            parse("bad.xml", raw_forbidden),
            Err(Error::Patch(_))
        ));

        for reference in [
            "&#x9;",
            "&#xA;",
            "&#xD;",
            "&#x20;",
            "&#xD7FF;",
            "&#xE000;",
            "&#xFFFD;",
            "&#x10000;",
            "&#x10FFFF;",
            "&#x41;",
            "&#65;",
        ] {
            let xml = format!(
                "<Patches><Metadata Name=\"Valid\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"{reference}\"/></Metadata></Patches>"
            );
            assert_eq!(parse("valid.xml", &xml).unwrap()[0].writes.len(), 1);
        }

        let escaped = "<Patches><Metadata Name=\"Escaped\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&amp;#xD800;\"/></Metadata></Patches>";
        assert_eq!(
            parse("escaped.xml", escaped).unwrap()[0].writes[0].bytes,
            b"&#xD800;\0"
        );

        let ignored_contexts = "<Patches><!-- &#xD800; --><?pi &#xD800;?><![CDATA[&#xD800;]]><Metadata Name=\"P\" AppVer=\"1\"/></Patches>";
        assert!(parse("contexts.xml", ignored_contexts).is_ok());
    }

    #[test]
    fn literal_write_limit_matches_loader_and_discards_oversized_patch_whole() {
        let line = |kind: &str, value: &str| {
            format!("<Line Type=\"{kind}\" Address=\"0x401000\" Value=\"{value}\"/>")
        };
        let exact_bytes = "AA".repeat(MAX_PATCH_WRITE_BYTES);
        let too_many_bytes = "AA".repeat(MAX_PATCH_WRITE_BYTES + 1);
        let exact_utf8 = "a".repeat(MAX_PATCH_WRITE_BYTES - 1);
        let oversized_utf8 = "a".repeat(MAX_PATCH_WRITE_BYTES);
        let exact_utf16 = "a".repeat((MAX_PATCH_WRITE_BYTES - 2) / 2);
        let oversized_utf16 = "a".repeat(MAX_PATCH_WRITE_BYTES / 2);
        for (kind, value, expected) in [
            ("bytes", exact_bytes.as_str(), MAX_PATCH_WRITE_BYTES),
            ("utf8", exact_utf8.as_str(), MAX_PATCH_WRITE_BYTES),
            ("utf16", exact_utf16.as_str(), MAX_PATCH_WRITE_BYTES),
        ] {
            let xml = format!(
                "<Patches><Metadata Name=\"P\" AppVer=\"1\">{}</Metadata></Patches>",
                line(kind, value)
            );
            assert_eq!(
                parse("limits.xml", &xml).unwrap()[0].writes[0].bytes.len(),
                expected
            );
        }
        for (kind, value) in [
            ("bytes", too_many_bytes.as_str()),
            ("utf8", oversized_utf8.as_str()),
            ("utf16", oversized_utf16.as_str()),
        ] {
            let xml = format!(
                "<Patches><Metadata Name=\"P\" AppVer=\"1\">{}{}</Metadata></Patches>",
                line("bytes", "90"),
                line(kind, value)
            );
            let patch = &parse("limits.xml", &xml).unwrap()[0];
            assert!(patch.writes.is_empty());
            assert!(patch.note.contains("4096-byte loader limit"));
        }

        let patch = Patch {
            key: "limits/P".into(),
            name: "P".into(),
            author: String::new(),
            note: String::new(),
            title_ids: vec![],
            executable: "eboot.bin".into(),
            app_ver: "1".into(),
            enabled_by_default: true,
            writes: vec![
                ByteWrite {
                    offset: 0,
                    bytes: vec![1],
                },
                ByteWrite {
                    offset: 1,
                    bytes: vec![2; MAX_PATCH_WRITE_BYTES + 1],
                },
            ],
        };
        let (writes, notes) = compile(&[&patch], &[(0, 8192)]).unwrap();
        assert!(writes.is_empty());
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("4096-byte loader limit"));

        let exact = Patch {
            writes: vec![ByteWrite {
                offset: 0,
                bytes: vec![2; MAX_PATCH_WRITE_BYTES],
            }],
            ..patch
        };
        let (writes, notes) = compile(&[&exact], &[(0, MAX_PATCH_WRITE_BYTES as u64)]).unwrap();
        assert!(notes.is_empty());
        assert_eq!(writes[0].bytes.len(), MAX_PATCH_WRITE_BYTES);
    }

    #[test]
    fn attribute_whitespace_normalizes_raw_chars_but_preserves_numeric_references() {
        let raw = "a\tb\nc\r\nd\re";
        let xml = format!("<Patches><Metadata Name=\"Whitespace\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"{raw}\"/><Line Type=\"utf8\" Address=\"0x401100\" Value=\"a&#x9;b&#xA;c&#xD;d\"/></Metadata></Patches>");
        let patches = parse("whitespace.xml", &xml).unwrap();
        assert_eq!(patches[0].writes[0].bytes, b"a b c d e\0");
        assert_eq!(patches[0].writes[1].bytes, b"a\tb\nc\rd\0");

        let raw_crlf = "<Patches><Metadata Name=\"CRLF\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"a\r\nb\"/></Metadata></Patches>";
        assert_eq!(
            parse("crlf.xml", raw_crlf).unwrap()[0].writes[0].bytes,
            b"a b\0"
        );
    }

    #[test]
    fn compile_refuses_empty_write_and_prior_valid_writes_as_one_patch() {
        let patch = Patch {
            key: "empty/P".into(),
            name: "P".into(),
            author: String::new(),
            note: String::new(),
            title_ids: vec![],
            executable: "eboot.bin".into(),
            app_ver: "1".into(),
            enabled_by_default: true,
            writes: vec![
                ByteWrite {
                    offset: 0,
                    bytes: vec![0x90],
                },
                ByteWrite {
                    offset: 1,
                    bytes: Vec::new(),
                },
            ],
        };
        let (writes, notes) = compile(&[&patch], &[(0, 8)]).unwrap();
        assert!(writes.is_empty());
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("zero bytes"));
    }

    #[test]
    fn patches_bin_has_loader_header_and_empty_list() {
        let empty = to_patches_bin(&[]);
        assert_eq!(&empty[..8], &PATCHES_MAGIC);
        assert_eq!(&empty[8..16], &EBOOT_BASE.to_le_bytes());
        assert_eq!(&empty[16..24], &0u64.to_le_bytes());
        let blob = to_patches_bin(&[ByteWrite {
            offset: 3,
            bytes: vec![0xaa, 0xbb],
        }]);
        assert_eq!(&blob[16..24], &1u64.to_le_bytes());
        assert_eq!(&blob[24..32], &3u64.to_le_bytes());
        assert_eq!(&blob[32..40], &2u64.to_le_bytes());
        assert_eq!(&blob[40..], &[0xaa, 0xbb]);
    }

    #[test]
    fn missing_revision_or_executable_never_matches() {
        let unversioned = parse(
            "nover.xml",
            "<Patches><Metadata Name=\"NoVer\"><Line Type=\"bytes\" Address=\"0x401000\" Value=\"90\"/></Metadata></Patches>",
        )
        .unwrap();
        let unknown_revision = Target {
            title_id: String::new(),
            executable: "eboot.bin".into(),
            app_ver: String::new(),
        };
        assert!(applicable(&unversioned, &unknown_revision, true).is_empty());
        assert!(applicable(
            &unversioned,
            &Target {
                app_ver: String::new(),
                ..target()
            },
            true
        )
        .is_empty());
    }

    #[test]
    fn unscoped_patch_applies_only_to_its_executable_and_revision() {
        let unscoped = parse(
            "u.xml",
            "<Patches><Metadata Name=\"U\" AppVer=\"01.02\"><Line Type=\"bytes\" Address=\"0x401000\" Value=\"90\"/></Metadata></Patches>",
        )
        .unwrap();
        assert_eq!(applicable(&unscoped, &target(), true).len(), 1);
        assert!(applicable(
            &unscoped,
            &Target {
                executable: "big2-ps4_Shipping.elf".into(),
                ..target()
            },
            true
        )
        .is_empty());
    }

    #[test]
    fn integer_literals_must_fit_their_width() {
        for (kind, value, fits) in [
            ("bytes16", "0xFFFF", true),
            ("bytes16", "0x10000", false),
            ("bytes32", "0xFFFFFFFF", true),
            ("bytes32", "0x100000000", false),
            ("bytes64", "0xFFFFFFFFFFFFFFFF", true),
            ("bytes64", "0x10000000000000000", false),
        ] {
            let xml = format!(
                "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"{kind}\" Address=\"0x401000\" Value=\"{value}\"/></Metadata></Patches>"
            );
            let patch = &parse("w.xml", &xml).unwrap()[0];
            assert_eq!(!patch.writes.is_empty(), fits, "{kind} {value}");
        }
    }

    #[test]
    fn numeric_literals_reject_signs_and_non_finite_floats() {
        assert!(decode_hex("+f").is_err());
        assert!(decode_hex("0x").is_err());
        assert!(parse_u64("+1").is_err());
        assert!(parse_u64("0x").is_err());
        assert!(parse_u64("-1").is_err());
        let signed_entity = "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"utf8\" Address=\"0x401000\" Value=\"&#+65;\"/></Metadata></Patches>";
        assert!(matches!(
            parse("bad.xml", signed_entity),
            Err(Error::Patch(_))
        ));
        let infinite = "<Patches><Metadata Name=\"P\" AppVer=\"1\"><Line Type=\"float32\" Address=\"0x401000\" Value=\"inf\"/></Metadata></Patches>";
        assert!(parse("f.xml", infinite).unwrap()[0].writes.is_empty());
    }

    #[test]
    fn elf_loadable_segment_must_lie_in_file_and_above_base() {
        assert!(
            eboot_segments(&synthetic_elf(EBOOT_BASE + 0x1000, 0x20, 0x40, 2000, 120)).is_err()
        );
        assert!(eboot_segments(&synthetic_elf(0x1000, 0, 0x40, 0, 120)).is_err());
    }
}
