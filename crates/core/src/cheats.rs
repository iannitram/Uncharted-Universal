//! Cheat patches ("health" and "ammo") for the three executables. A cheat is written only when
//! the executable's SHA-256 is one of the known builds AND the bytes at its address are the
//! expected ones; anything else is reported and skipped, never an error. Addresses are image
//! offsets (virtual addresses of the executable's load segments). Replaces `scripts/cheats.py`,
//! and the `BB_CHEATS` setting helpers of `run.py` (`game_cheats`) and the launcher
//! (`game_environment`).
//!
//! "Cheat file parse" in the code map: the cheat table itself (hash -> cheat -> address,
//! original bytes, replacement) is compiled into the crate as [`TABLE`], moved here from
//! `PATCHES` in `cheats.py`, and read by [`parse`].

use crate::bundle::sha256_hex;
use crate::error::{Error, Result};
use crate::games::Game;
use crate::patches::{ByteWrite, PATCHES_MAGIC};
use crate::prepare::{self, ProgramHeader};

/// A cheat the launcher offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cheat {
    Health,
    Ammo,
}

impl Cheat {
    /// Both cheats, in the launcher's order.
    pub const ALL: [Cheat; 2] = [Cheat::Health, Cheat::Ammo];

    /// "health" or "ammo": the name in the table, in `BB_CHEATS` and in the launcher's
    /// `<game>_infinite_<cheat>` settings.
    pub fn key(self) -> &'static str {
        match self {
            Cheat::Health => "health",
            Cheat::Ammo => "ammo",
        }
    }

    /// The inverse of [`Cheat::key`] (exact, lower case).
    pub fn from_key(key: &str) -> Option<Cheat> {
        Cheat::ALL.into_iter().find(|cheat| cheat.key() == key)
    }
}

/// Executable SHA-256 of the 01.00 builds, `V100` in `cheats.py` (hex).
pub const SHA256_V100: [(Game, &str); 3] = [
    (
        Game::U1,
        "8953b6ed5cf4d1d26aa3aa4cd1e0c72817ea155f6a5774fb17f6fec7d787f953",
    ),
    (
        Game::U2,
        "61c0456314a8de2dbd32e1c426f7828092de7d25a58b83386ced1a48d7765ebe",
    ),
    (
        Game::U3,
        "acc952e5a9ac7062f8ecfc52700faa599d5f29deb1e24f9e81e6de832326d1d0",
    ),
];

/// Executable SHA-256 of the 01.02 builds, `V102` in `cheats.py` (hex).
pub const SHA256_V102: [(Game, &str); 3] = [
    (
        Game::U1,
        "dc017b45da8978bb11a0a11d940e96d98fa07f3c066a3debba043714432212f8",
    ),
    (
        Game::U2,
        "5047b693b1c6fd06ccb1eacecacee60b6183bedc18d023cac276e1ec6009dfe2",
    ),
    (
        Game::U3,
        "02c111aa7e73e445d71a1ad861702811f31d471abc61caf01732d55a77eb1c38",
    ),
];

/// The cheat table, `PATCHES` in `cheats.py`: one entry per line as
/// `<game> <executable sha256> <cheat> <offset> <original hex> <replacement hex>`.
///
/// health: jump to the engine's own no-damage path. ammo: the clip decrement is shared with NPC
/// weapons, so U1/U2 drop the player Fire's call to it and U3 forces its player-held branch.
pub const TABLE: &str = "\
# game sha256 cheat offset original replacement
# 01.00
u1 8953b6ed5cf4d1d26aa3aa4cd1e0c72817ea155f6a5774fb17f6fec7d787f953 health 0x261edb 7535 eb35
u1 8953b6ed5cf4d1d26aa3aa4cd1e0c72817ea155f6a5774fb17f6fec7d787f953 ammo 0x71b003 e8c8acc2ff 9090909090
u2 61c0456314a8de2dbd32e1c426f7828092de7d25a58b83386ced1a48d7765ebe health 0x35de73 0f84bc010000 e9cf02000090
u2 61c0456314a8de2dbd32e1c426f7828092de7d25a58b83386ced1a48d7765ebe ammo 0xa4fd8d e87e8aa6ff 9090909090
u3 acc952e5a9ac7062f8ecfc52700faa599d5f29deb1e24f9e81e6de832326d1d0 health 0x33f87b 0f84ca020000 e96a0b000090
u3 acc952e5a9ac7062f8ecfc52700faa599d5f29deb1e24f9e81e6de832326d1d0 ammo 0x46a1f9 4c8d3dc0660501 41b401eb729090
# 01.02
u1 dc017b45da8978bb11a0a11d940e96d98fa07f3c066a3debba043714432212f8 health 0x26244b 7535 eb35
u1 dc017b45da8978bb11a0a11d940e96d98fa07f3c066a3debba043714432212f8 ammo 0x730f73 e8c854c1ff 9090909090
u2 5047b693b1c6fd06ccb1eacecacee60b6183bedc18d023cac276e1ec6009dfe2 health 0x7f0143 0f843c050000 e95f06000090
u2 5047b693b1c6fd06ccb1eacecacee60b6183bedc18d023cac276e1ec6009dfe2 ammo 0x961ac8 e853851800 9090909090
u3 02c111aa7e73e445d71a1ad861702811f31d471abc61caf01732d55a77eb1c38 health 0x9f9295 4180bfa7000000007436 e9320300009090909090
u3 02c111aa7e73e445d71a1ad861702811f31d471abc61caf01732d55a77eb1c38 ammo 0xd517ac 4c8d257d92aa0066418b442462 40b701e9110100009090909090
";

/// `PT_LOAD` and `PT_SCE_RELRO`: the segments `image_bytes` in `cheats.py` reads through.
const PT_LOAD: u32 = 1;
const PT_SCE_RELRO: u32 = 0x6100_0010;

/// What compiling cheats gave.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Compiled {
    pub writes: Vec<ByteWrite>,
    /// One plain sentence per cheat that was skipped (unknown build, bytes differ).
    pub skipped: Vec<String>,
}

/// One entry of the cheat table: a write to make when the executable has this hash and the
/// bytes at `offset` are `original`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub cheat: Cheat,
    pub game: Game,
    /// Executable SHA-256 (hex) this entry is for.
    pub sha256: String,
    pub offset: u64,
    pub original: Vec<u8>,
    pub replacement: Vec<u8>,
}

/// Parse the cheat table text (one entry per line, the [`TABLE`] format; blank lines and lines
/// starting with `#` are skipped) into entries; bad text is `Error::Trophies`. An entry's
/// original and replacement must be non-empty, the same length and differ; one executable may
/// list a cheat once.
pub fn parse(table: &str) -> Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    for (index, raw) in table.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let number = index + 1;
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [game, sha256, cheat, offset, original, replacement] = fields[..] else {
            return Err(table_error(number, "expected six fields"));
        };
        let game = Game::from_key(game)
            .ok_or_else(|| table_error(number, &format!("unknown game {game:?}")))?;
        if !is_sha256_hex(sha256) {
            return Err(table_error(number, "the hash is not a lower-case SHA-256"));
        }
        let cheat = Cheat::from_key(cheat)
            .ok_or_else(|| table_error(number, &format!("unknown cheat {cheat:?}")))?;
        let offset = offset
            .strip_prefix("0x")
            .and_then(|hex| u64::from_str_radix(hex, 16).ok())
            .ok_or_else(|| table_error(number, "the offset is not 0x-prefixed hex"))?;
        let original = decode_hex(original)
            .ok_or_else(|| table_error(number, "the original bytes are not hex"))?;
        let replacement = decode_hex(replacement)
            .ok_or_else(|| table_error(number, "the replacement bytes are not hex"))?;
        if original.is_empty() || original.len() != replacement.len() {
            return Err(table_error(
                number,
                "original and replacement must be the same non-zero length",
            ));
        }
        if original == replacement {
            return Err(table_error(number, "the replacement changes nothing"));
        }
        if offset.checked_add(original.len() as u64).is_none() {
            return Err(table_error(number, "the offset overflows"));
        }
        if entries
            .iter()
            .any(|entry| entry.sha256 == sha256 && entry.cheat == cheat)
        {
            return Err(table_error(
                number,
                &format!("{} is listed twice for this executable", cheat.key()),
            ));
        }
        entries.push(Entry {
            cheat,
            game,
            sha256: sha256.to_string(),
            offset,
            original,
            replacement,
        });
    }
    Ok(entries)
}

/// The built-in [`TABLE`], parsed.
pub fn table() -> Vec<Entry> {
    parse(TABLE).expect("the built-in cheat table parses")
}

/// Apply compiled cheat writes to an executable image in place (the bytes were checked when
/// compiling; a write outside `image` is `Error::Trophies`). Every write is checked before any
/// byte changes, so a refused set leaves `image` as it was.
pub fn apply(image: &mut [u8], compiled: &Compiled) -> Result<()> {
    let mut ranges = Vec::with_capacity(compiled.writes.len());
    for write in &compiled.writes {
        let range = usize::try_from(write.offset)
            .ok()
            .and_then(|start| Some(start..start.checked_add(write.bytes.len())?))
            .filter(|range| range.end <= image.len())
            .ok_or_else(|| {
                Error::Trophies(format!(
                    "a cheat write at {:#x} falls outside the executable",
                    write.offset
                ))
            })?;
        ranges.push(range);
    }
    for (range, write) in ranges.into_iter().zip(&compiled.writes) {
        image[range].copy_from_slice(&write.bytes);
    }
    Ok(())
}

/// Compile the `enabled` cheats for `executable` (the file's bytes, hashed here) against the
/// built-in [`TABLE`].
pub fn compile(executable: &[u8], enabled: &[Cheat]) -> Result<Compiled> {
    compile_with(&table(), executable, enabled)
}

/// [`compile`] against any `entries` (tests pass a table for a synthetic executable). Writes
/// come in the order of `enabled`; a repeated cheat is compiled once. An unknown executable, an
/// executable that does not load, a cheat the table lacks for this build, or unexpected bytes
/// at a cheat's address skip that cheat with a sentence in `skipped`; never an error.
pub fn compile_with(entries: &[Entry], executable: &[u8], enabled: &[Cheat]) -> Result<Compiled> {
    let mut compiled = Compiled::default();
    let mut wanted: Vec<Cheat> = Vec::new();
    for cheat in enabled {
        if !wanted.contains(cheat) {
            wanted.push(*cheat);
        }
    }
    if wanted.is_empty() {
        return Ok(compiled);
    }
    let sha256 = sha256_hex(executable);
    let build: Vec<&Entry> = entries.iter().filter(|e| e.sha256 == sha256).collect();
    if build.is_empty() {
        for cheat in &wanted {
            compiled.skipped.push(format!(
                "{}: this executable is not a supported version, so no cheat is applied.",
                cheat.key()
            ));
        }
        return Ok(compiled);
    }
    let image = match prepare::load_image(executable) {
        Ok(image) => image,
        Err(error) => {
            for cheat in &wanted {
                compiled
                    .skipped
                    .push(format!("{}: not applied ({error}).", cheat.key()));
            }
            return Ok(compiled);
        }
    };
    for cheat in wanted {
        let Some(entry) = build.iter().find(|entry| entry.cheat == cheat) else {
            compiled.skipped.push(format!(
                "{}: there is no such cheat for this executable.",
                cheat.key()
            ));
            continue;
        };
        let found = image_bytes(
            &image.elf,
            &image.program_headers,
            entry.offset,
            entry.original.len(),
        );
        if found != Some(entry.original.as_slice()) {
            compiled.skipped.push(format!(
                "{}: skipped, unexpected code at {:#x}.",
                cheat.key(),
                entry.offset
            ));
            continue;
        }
        compiled.writes.push(ByteWrite {
            offset: entry.offset,
            bytes: entry.replacement.clone(),
        });
    }
    Ok(compiled)
}

/// The `size` bytes at image offset `address`, read through the load segment that holds them
/// whole (`image_bytes` in `cheats.py`), or `None` when no file-backed segment does.
fn image_bytes<'a>(
    elf: &'a [u8],
    program_headers: &[ProgramHeader],
    address: u64,
    size: usize,
) -> Option<&'a [u8]> {
    let end = address.checked_add(size as u64)?;
    program_headers.iter().find_map(|ph| {
        if !matches!(ph.kind, PT_LOAD | PT_SCE_RELRO)
            || ph.vaddr > address
            || end > ph.vaddr.checked_add(ph.filesz)?
        {
            return None;
        }
        let start = usize::try_from(ph.offset.checked_add(address - ph.vaddr)?).ok()?;
        elf.get(start..start.checked_add(size)?)
    })
}

/// The `cheats.bin` for the runtime: the writes in the same layout as `patches::to_patches_bin`
/// (`BBPATCH2`, base, count, then offset, length and bytes per write, little-endian u64), with
/// base 0 because cheat offsets are already image offsets (`write_patch_file` in `cheats.py`).
pub fn to_cheats_bin(writes: &[ByteWrite]) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(&PATCHES_MAGIC);
    output.extend_from_slice(&0u64.to_le_bytes());
    output.extend_from_slice(&(writes.len() as u64).to_le_bytes());
    for write in writes {
        output.extend_from_slice(&write.offset.to_le_bytes());
        output.extend_from_slice(&(write.bytes.len() as u64).to_le_bytes());
        output.extend_from_slice(&write.bytes);
    }
    output
}

/// The `BB_CHEATS` value for the cheats switched on in the launcher ("u1:health,u2:ammo"), in
/// game then cheat order whatever the order of `enabled` (`game_environment` in the launcher).
pub fn setting(enabled: &[(Game, Cheat)]) -> String {
    let mut parts = Vec::new();
    for game in Game::ALL {
        for cheat in Cheat::ALL {
            if enabled.contains(&(game, cheat)) {
                parts.push(format!("{}:{}", game.key(), cheat.key()));
            }
        }
    }
    parts.join(",")
}

/// `BB_CHEATS` reduced to `game`'s own cheats, in the setting's order (`game_cheats` in
/// `run.py`). Unknown games or cheats in the setting are ignored.
pub fn for_game(setting: &str, game: Game) -> Vec<Cheat> {
    let mut cheats = Vec::new();
    for part in setting.split(',') {
        let Some((key, cheat)) = part.split_once(':') else {
            continue;
        };
        if Game::from_key(key) != Some(game) {
            continue;
        }
        if let Some(cheat) = Cheat::from_key(cheat) {
            if !cheats.contains(&cheat) {
                cheats.push(cheat);
            }
        }
    }
    cheats
}

fn table_error(line: usize, message: &str) -> Error {
    Error::Trophies(format!("cheat table line {line}: {message}"))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&value[at..at + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    //! Ported from `tests/test_cheats.py`. The executable is a synthetic one-segment SELF
    //! (`tiny_self` there) whose entry point is set inside the segment, because
    //! `prepare::load_image` refuses an unmapped entry where `prepare.py` did not check.

    use super::*;

    const ORIGINAL: &str = "7535";
    const PATCHED: &str = "eb35";
    const ADDRESS: u64 = 0x110;

    fn hex(value: &str) -> Vec<u8> {
        decode_hex(value).expect("test hex")
    }

    fn put_u16(data: &mut [u8], at: usize, value: u16) {
        data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(data: &mut [u8], at: usize, value: u32) {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u64(data: &mut [u8], at: usize, value: u64) {
        data[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    /// `tiny_self(code)`: a SELF with one plain segment, file offset 0x1000 and image offset
    /// 0x100, holding the 0x40 bytes of `code`.
    fn tiny_self(code: &[u8]) -> Vec<u8> {
        assert_eq!(code.len(), 0x40);
        let mut data = vec![0u8; 0x1000 + 0x40];
        data[..4].copy_from_slice(b"\x4f\x15\x3d\x1d");
        put_u16(&mut data, 24, 1);
        put_u64(&mut data, 32, 0x800);
        put_u64(&mut data, 40, 0x200);
        put_u64(&mut data, 48, 0x40);
        put_u64(&mut data, 56, 0x40);
        // ELF header at 64.
        data[64..73].copy_from_slice(b"\x7fELF\x02\x01\x01\x09\x00");
        put_u16(&mut data, 64 + 16, 0xfe10);
        put_u16(&mut data, 64 + 18, 62);
        put_u32(&mut data, 64 + 20, 1);
        put_u64(&mut data, 64 + 24, 0x100); // entry, inside the segment
        put_u64(&mut data, 64 + 32, 64); // phoff
        put_u16(&mut data, 64 + 52, 64);
        put_u16(&mut data, 64 + 54, 56);
        put_u16(&mut data, 64 + 56, 1);
        // Program header at 128: PT_LOAD, R+X, offset 0x1000, vaddr 0x100, 0x40 bytes.
        put_u32(&mut data, 128, 1);
        put_u32(&mut data, 132, 5);
        put_u64(&mut data, 136, 0x1000);
        put_u64(&mut data, 144, 0x100);
        put_u64(&mut data, 152, 0);
        put_u64(&mut data, 160, 0x40);
        put_u64(&mut data, 168, 0x40);
        put_u64(&mut data, 176, 0x1000);
        data[0x200..0x240].copy_from_slice(code);
        data
    }

    /// `code(at, data)`: the segment's 0x40 bytes with `data` at image offset `at`.
    fn code(at: u64, bytes: &[u8]) -> Vec<u8> {
        let mut code = vec![0u8; 0x40];
        let start = (at - 0x100) as usize;
        code[start..start + bytes.len()].copy_from_slice(bytes);
        code
    }

    fn entry(executable: &[u8], cheat: Cheat, offset: u64, original: &str, new: &str) -> Entry {
        Entry {
            cheat,
            game: Game::U1,
            sha256: sha256_hex(executable),
            offset,
            original: hex(original),
            replacement: hex(new),
        }
    }

    /// `run_cheats`: the `cheats.bin` for `enabled` with a table holding health (and `extra`),
    /// or `None` when nothing applies (`build` writes no file then).
    fn run_cheats(code: &[u8], enabled: &[Cheat], extra: Option<Entry>) -> Option<Vec<u8>> {
        let exe = tiny_self(code);
        let mut table = vec![entry(&exe, Cheat::Health, ADDRESS, ORIGINAL, PATCHED)];
        if let Some(mut extra) = extra {
            extra.sha256 = sha256_hex(&exe);
            table.push(extra);
        }
        let compiled = compile_with(&table, &exe, enabled).expect("compile never errors");
        (!compiled.writes.is_empty()).then(|| to_cheats_bin(&compiled.writes))
    }

    fn expected_bin(writes: &[(u64, &[u8])]) -> Vec<u8> {
        let mut blob = b"BBPATCH2".to_vec();
        blob.extend_from_slice(&0u64.to_le_bytes());
        blob.extend_from_slice(&(writes.len() as u64).to_le_bytes());
        for (offset, bytes) in writes {
            blob.extend_from_slice(&offset.to_le_bytes());
            blob.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            blob.extend_from_slice(bytes);
        }
        blob
    }

    // CheatTests (launcher settings and run.py's per-game filter).

    #[test]
    fn all_six_default_to_off() {
        let all: Vec<(Game, Cheat)> = Game::ALL
            .into_iter()
            .flat_map(|game| Cheat::ALL.into_iter().map(move |cheat| (game, cheat)))
            .collect();
        assert_eq!(all.len(), 6);
        assert_eq!(setting(&[]), "");
    }

    #[test]
    fn environment_lists_enabled_cheats_by_game() {
        assert_eq!(
            setting(&[(Game::U2, Cheat::Ammo), (Game::U1, Cheat::Health)]),
            "u1:health,u2:ammo"
        );
    }

    #[test]
    fn run_keeps_only_the_running_games_cheats() {
        let cheats = "u1:health,u2:ammo,u3:health,u3:ammo";
        assert_eq!(for_game(cheats, Game::U1), [Cheat::Health]);
        assert_eq!(for_game(cheats, Game::U2), [Cheat::Ammo]);
        assert_eq!(for_game(cheats, Game::U3), [Cheat::Health, Cheat::Ammo]);
        assert_eq!(for_game("", Game::U1), []);
        assert_eq!(
            Game::from_executable("big2-ps4_Shipping.elf").map(|g| for_game(cheats, g)),
            Some(vec![Cheat::Ammo])
        );
    }

    // PatchFileTests (cheats.py).

    #[test]
    fn expected_bytes_are_replaced_in_the_patch_file() {
        let blob = run_cheats(&code(ADDRESS, &hex(ORIGINAL)), &[Cheat::Health], None);
        assert_eq!(blob, Some(expected_bin(&[(ADDRESS, &hex(PATCHED))])));
    }

    #[test]
    fn unexpected_bytes_write_nothing() {
        let exe = tiny_self(&code(ADDRESS, b"t5"));
        let table = [entry(&exe, Cheat::Health, ADDRESS, ORIGINAL, PATCHED)];
        let compiled = compile_with(&table, &exe, &[Cheat::Health]).unwrap();
        assert!(compiled.writes.is_empty());
        assert_eq!(
            compiled.skipped,
            ["health: skipped, unexpected code at 0x110."]
        );
    }

    #[test]
    fn mismatched_cheat_does_not_drop_the_other() {
        let ammo = Entry {
            cheat: Cheat::Ammo,
            game: Game::U1,
            sha256: String::new(),
            offset: 0x120,
            original: hex("e800000000"),
            replacement: hex("9090909090"),
        };
        let blob = run_cheats(
            &code(ADDRESS, &hex(ORIGINAL)),
            &[Cheat::Health, Cheat::Ammo],
            Some(ammo),
        );
        assert_eq!(blob, Some(expected_bin(&[(ADDRESS, &hex(PATCHED))])));
    }

    #[test]
    fn disabled_cheat_and_unknown_cheat_write_nothing() {
        let code = code(ADDRESS, &hex(ORIGINAL));
        assert_eq!(run_cheats(&code, &[], None), None);
        assert_eq!(run_cheats(&code, &[Cheat::Ammo], None), None);
    }

    #[test]
    fn unknown_executable_is_refused() {
        let exe = tiny_self(&[0u8; 0x40]);
        let compiled = compile(&exe, &[Cheat::Health, Cheat::Ammo]).unwrap();
        assert!(compiled.writes.is_empty());
        assert_eq!(compiled.skipped.len(), 2);
        assert!(compiled.skipped[0].contains("not a supported version"));
    }

    /// `build` removed a stale `cheats.bin` when cheats are off; the Rust caller writes the file,
    /// so the contract here is that no cheat compiles to no writes, even for a missing (empty)
    /// executable, and never an error.
    #[test]
    fn nothing_enabled_compiles_to_nothing_for_any_executable() {
        assert_eq!(compile(&[], &[]), Ok(Compiled::default()));
        assert_eq!(compile(b"not an executable", &[]), Ok(Compiled::default()));
    }

    /// `run.py` calls the imported module, not a script path; here the table the module
    /// carries is the one `compile` uses.
    #[test]
    fn compile_uses_the_built_in_table() {
        assert_eq!(table(), parse(TABLE).unwrap());
        assert_eq!(table().len(), 12);
    }

    #[test]
    fn real_table_is_consistent() {
        let entries = table();
        let mut builds: Vec<&str> = entries.iter().map(|e| e.sha256.as_str()).collect();
        builds.dedup();
        assert_eq!(builds.len(), 6);
        for sha in &builds {
            let mut spans: Vec<(u64, u64)> = Vec::new();
            for entry in entries.iter().filter(|e| e.sha256 == *sha) {
                assert_eq!(entry.original.len(), entry.replacement.len());
                assert_ne!(entry.original, entry.replacement);
                spans.push((entry.offset, entry.offset + entry.original.len() as u64));
            }
            assert_eq!(spans.len(), 2, "{sha}");
            spans.sort_unstable();
            assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0), "{sha}");
        }
        for entry in &entries {
            let known = SHA256_V100
                .iter()
                .chain(SHA256_V102.iter())
                .any(|(game, sha)| *game == entry.game && *sha == entry.sha256);
            assert!(known, "{} {}", entry.game.key(), entry.sha256);
        }
    }

    // Rust-only: parse, apply and the image-offset mapping.

    #[test]
    fn table_matches_cheats_py() {
        let entries = table();
        let u3_102_ammo = entries
            .iter()
            .find(|e| e.sha256 == SHA256_V102[2].1 && e.cheat == Cheat::Ammo)
            .unwrap();
        assert_eq!(u3_102_ammo.game, Game::U3);
        assert_eq!(u3_102_ammo.offset, 0xd517ac);
        assert_eq!(u3_102_ammo.original, hex("4c8d257d92aa0066418b442462"));
        assert_eq!(u3_102_ammo.replacement, hex("40b701e9110100009090909090"));
        let u1_100_health = entries
            .iter()
            .find(|e| e.sha256 == SHA256_V100[0].1 && e.cheat == Cheat::Health)
            .unwrap();
        assert_eq!(
            (u1_100_health.offset, u1_100_health.replacement.as_slice()),
            (0x261edb, &[0xeb, 0x35][..])
        );
    }

    #[test]
    fn bad_table_text_is_a_trophies_error() {
        let sha = "a".repeat(64);
        for bad in [
            "u1 health 0x10 7535 eb35".to_string(),
            format!("u4 {sha} health 0x10 7535 eb35"),
            format!("u1 {} health 0x10 7535 eb35", "A".repeat(64)),
            format!("u1 {sha} speed 0x10 7535 eb35"),
            format!("u1 {sha} health 10 7535 eb35"),
            format!("u1 {sha} health 0x10 753 eb3"),
            format!("u1 {sha} health 0x10 7535 eb"),
            format!("u1 {sha} health 0x10 7535 7535"),
            format!("u1 {sha} health 0xffffffffffffffff 7535 eb35"),
            format!("u1 {sha} health 0x10 7535 eb35\nu1 {sha} health 0x20 7535 eb35"),
        ] {
            assert!(
                matches!(parse(&bad), Err(Error::Trophies(_))),
                "accepted {bad:?}"
            );
        }
        assert_eq!(parse("# only a comment\n\n"), Ok(Vec::new()));
    }

    #[test]
    fn apply_writes_in_place_and_refuses_outside_the_image() {
        let mut image = vec![0u8; 8];
        let compiled = Compiled {
            writes: vec![ByteWrite {
                offset: 2,
                bytes: vec![1, 2],
            }],
            skipped: Vec::new(),
        };
        apply(&mut image, &compiled).unwrap();
        assert_eq!(image, [0, 0, 1, 2, 0, 0, 0, 0]);

        let outside = Compiled {
            writes: vec![
                ByteWrite {
                    offset: 0,
                    bytes: vec![9],
                },
                ByteWrite {
                    offset: 7,
                    bytes: vec![9, 9],
                },
            ],
            skipped: Vec::new(),
        };
        assert!(matches!(
            apply(&mut image, &outside),
            Err(Error::Trophies(_))
        ));
        assert_eq!(image, [0, 0, 1, 2, 0, 0, 0, 0], "nothing written");
        let overflow = Compiled {
            writes: vec![ByteWrite {
                offset: u64::MAX,
                bytes: vec![9],
            }],
            skipped: Vec::new(),
        };
        assert!(apply(&mut image, &overflow).is_err());
    }

    #[test]
    fn compiled_writes_apply_to_the_memory_image() {
        let exe = tiny_self(&code(ADDRESS, &hex(ORIGINAL)));
        let table = [entry(&exe, Cheat::Health, ADDRESS, ORIGINAL, PATCHED)];
        let compiled = compile_with(&table, &exe, &[Cheat::Health, Cheat::Health]).unwrap();
        assert_eq!(compiled.writes.len(), 1, "a repeated cheat compiles once");
        let mut image = prepare::load_image(&exe).unwrap().bytes;
        apply(&mut image, &compiled).unwrap();
        assert_eq!(&image[0x110..0x112], &hex(PATCHED)[..]);
    }

    #[test]
    fn a_write_must_lie_inside_one_file_backed_segment() {
        let exe = tiny_self(&code(0x13e, &hex(ORIGINAL)));
        // Straddles the segment end (0x140): never read past filesz.
        let table = [entry(&exe, Cheat::Health, 0x13f, "0000", PATCHED)];
        let compiled = compile_with(&table, &exe, &[Cheat::Health]).unwrap();
        assert!(compiled.writes.is_empty());
        // Below the segment start.
        let table = [entry(&exe, Cheat::Health, 0xf0, "0000", PATCHED)];
        assert!(compile_with(&table, &exe, &[Cheat::Health])
            .unwrap()
            .writes
            .is_empty());
        // Last two bytes of the segment.
        let table = [entry(&exe, Cheat::Health, 0x13e, ORIGINAL, PATCHED)];
        assert_eq!(
            compile_with(&table, &exe, &[Cheat::Health]).unwrap().writes,
            [ByteWrite {
                offset: 0x13e,
                bytes: hex(PATCHED)
            }]
        );
    }

    #[test]
    fn a_known_hash_that_does_not_load_is_skipped_not_an_error() {
        let exe = b"not an executable".to_vec();
        let table = [entry(&exe, Cheat::Health, ADDRESS, ORIGINAL, PATCHED)];
        let compiled = compile_with(&table, &exe, &[Cheat::Health]).unwrap();
        assert!(compiled.writes.is_empty());
        assert_eq!(compiled.skipped.len(), 1);
        assert!(compiled.skipped[0].starts_with("health: not applied"));
    }

    #[test]
    fn empty_cheats_bin_is_the_bare_header() {
        assert_eq!(to_cheats_bin(&[]), expected_bin(&[]));
    }
}
