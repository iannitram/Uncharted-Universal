//! Trophy data read for the runtime: the game's `.trp` files, names and grades, and the unlock
//! log. Reading only; showing trophies in the launcher is after 1.0 (SPEC.md Won'ts). Replaces
//! `scripts/trophies.py` (its AES-128, the `.trp` reader, the `trophies.txt` cache the runtime
//! reads, the unlock log and the progress count).
//!
//! `sce_sys/trophy/trophy01.trp` (U1), `trophy02.trp` (U2) and `trophy03.trp` (U3) are PS4
//! trophy packs: a small container with the trophy icons (PNG) and the texts as ESFM entries,
//! which are AES-128-CBC encrypted. The key of a set is the set's NP communication id
//! (`NPWRxxxxx_00`, padded to a block) encrypted with the console's trophy key, the same way
//! shadPS4 reads them.
//!
//! The console's trophy key is a console secret (SPEC.md M9). It is NOT in this crate: the
//! player supplies it at run time, the way shadPS4 takes its `TrophyKey` setting, as 32 hex
//! digits in `BB_TROPHY_KEY` ([`TrophyKey::from_env`]) or in a file
//! ([`TrophyKey::from_file`]). [`TrophyKey`] and [`Aes128`] print as `TrophyKey(..)` and
//! `Aes128(..)`, and no error message holds a key or a decrypted byte.
//!
//! The cache (`<out>/trophies.txt` and the icons) is what `src/runtime_trophy.c` reads today;
//! its format is the Python's `FORMAT` 2, line for line. Unlocks are kept in
//! `<user dir>/trophy/<NPWR id>.log`, one `id unix-time` per line.
//!
//! The AES-128 here is the plain FIPS-197 cipher (byte state, table-free rounds); the S-box is
//! generated, as the Python generates it. It decrypts game metadata only; it is not meant to
//! be constant time.

use crate::error::{Error, Result};
use crate::games::Game;
use roxmltree::{Document, ParsingOptions};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// The first four bytes of a trophy pack (big-endian).
pub const TRP_MAGIC: u32 = 0xDCA2_4D00;
/// The entry flag of an encrypted ESFM entry.
pub const FLAG_ENCRYPTED: u32 = 3;
/// The `trophies.txt` format; bump it when the file changes, so older caches are rebuilt.
pub const FORMAT: u32 = 2;
/// The environment variable that holds the console's trophy key (32 hex digits).
pub const KEY_VAR: &str = "BB_TROPHY_KEY";
/// The cache file in the prepare folder.
pub const CACHE_FILE: &str = "trophies.txt";

const HEADER_SIZE: usize = 0x60;
const ENTRY_FIELDS: usize = 52;
const MAX_XML_BYTES: usize = 8 * 1024 * 1024;
const MAX_XML_NODES: u32 = 100_000;

/// The trophy file and the NP communication id of a game: `GAMES` in `trophies.py`. The id is
/// looked for in `npbind.dat` first, then here.
pub fn trp_file(game: Game) -> (&'static str, &'static str) {
    match game {
        Game::U1 => ("trophy01.trp", "NPWR09798_00"),
        Game::U2 => ("trophy02.trp", "NPWR09799_00"),
        Game::U3 => ("trophy03.trp", "NPWR09801_00"),
    }
}

/// A trophy's grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Grade {
    Platinum,
    Gold,
    Silver,
    Bronze,
}

impl Grade {
    /// Every grade, in the order the progress line counts them.
    pub const ALL: [Grade; 4] = [Grade::Platinum, Grade::Gold, Grade::Silver, Grade::Bronze];

    /// "Platinum", "Gold", "Silver", "Bronze" (`GRADE_NAMES`).
    pub fn name(self) -> &'static str {
        match self {
            Grade::Platinum => "Platinum",
            Grade::Gold => "Gold",
            Grade::Silver => "Silver",
            Grade::Bronze => "Bronze",
        }
    }

    /// "P", "G", "S", "B": the pack's `ttype` and the cache's grade field.
    pub fn letter(self) -> &'static str {
        match self {
            Grade::Platinum => "P",
            Grade::Gold => "G",
            Grade::Silver => "S",
            Grade::Bronze => "B",
        }
    }

    /// The grade of a `ttype` letter. Anything else counts as bronze, as the Python's
    /// `progress` and the runtime's `grade_number` both do.
    pub fn from_letter(letter: &str) -> Grade {
        match letter {
            "P" => Grade::Platinum,
            "G" => Grade::Gold,
            "S" => Grade::Silver,
            _ => Grade::Bronze,
        }
    }
}

/// One trophy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trophy {
    pub id: u32,
    pub grade: Grade,
    pub hidden: bool,
    /// The group id (`gid`), empty for the base game. The cache does not keep it.
    pub group: String,
    pub name: String,
    pub detail: String,
}

/// A game's trophy set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrophySet {
    pub npcommid: String,
    pub title: String,
    pub detail: String,
    /// How many `<group>` elements the pack's TROPCONF has.
    pub groups: u32,
    /// Sorted by id.
    pub trophies: Vec<Trophy>,
    /// The pack's `TROPnnn.PNG` and `ICON0.PNG`, by entry name. Empty when read from a cache.
    pub icons: BTreeMap<String, Vec<u8>>,
}

/// Trophies per grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GradeCounts {
    pub platinum: u32,
    pub gold: u32,
    pub silver: u32,
    pub bronze: u32,
}

impl GradeCounts {
    /// The count of one grade.
    pub fn get(&self, grade: Grade) -> u32 {
        match grade {
            Grade::Platinum => self.platinum,
            Grade::Gold => self.gold,
            Grade::Silver => self.silver,
            Grade::Bronze => self.bronze,
        }
    }

    fn add(&mut self, grade: Grade) {
        match grade {
            Grade::Platinum => self.platinum += 1,
            Grade::Gold => self.gold += 1,
            Grade::Silver => self.silver += 1,
            Grade::Bronze => self.bronze += 1,
        }
    }
}

/// How much of a set is earned, as the progress line shows (`progress` in `trophies.py`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    /// Trophies of the set that are unlocked.
    pub unlocked: u32,
    /// Trophies in the set.
    pub total: u32,
    /// `unlocked * 100 / total`, rounded down; 0 for an empty set.
    pub percent: u32,
    /// Unlocked trophies per grade.
    pub earned: GradeCounts,
    /// All trophies per grade.
    pub by_grade: GradeCounts,
}

// ---- AES-128 --------------------------------------------------------------------------------

const fn xtime(a: u8) -> u8 {
    (a << 1) ^ if a & 0x80 != 0 { 0x1B } else { 0 }
}

const fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut out = 0;
    while b != 0 {
        if b & 1 != 0 {
            out ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    out
}

/// The S-box, generated the way `_tables` in `trophies.py` does: walk the multiplicative group
/// with generator 3, take the inverse as `q`, and apply the affine map.
const fn make_sbox() -> [u8; 256] {
    let mut sbox = [0u8; 256];
    let mut p: u8 = 1;
    let mut q: u8 = 1;
    loop {
        p = p ^ (p << 1) ^ if p & 0x80 != 0 { 0x1B } else { 0 };
        q ^= q << 1;
        q ^= q << 2;
        q ^= q << 4;
        if q & 0x80 != 0 {
            q ^= 0x09;
        }
        let x = q ^ q.rotate_left(1) ^ q.rotate_left(2) ^ q.rotate_left(3) ^ q.rotate_left(4);
        sbox[p as usize] = x ^ 0x63;
        if p == 1 {
            break;
        }
    }
    sbox[0] = 0x63;
    sbox
}

const fn invert(sbox: &[u8; 256]) -> [u8; 256] {
    let mut inverse = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        inverse[sbox[i] as usize] = i as u8;
        i += 1;
    }
    inverse
}

const fn mul_table(by: u8) -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        table[i] = gmul(i as u8, by);
        i += 1;
    }
    table
}

const SBOX: [u8; 256] = make_sbox();
const INV_SBOX: [u8; 256] = invert(&SBOX);
const MUL9: [u8; 256] = mul_table(9);
const MUL11: [u8; 256] = mul_table(11);
const MUL13: [u8; 256] = mul_table(13);
const MUL14: [u8; 256] = mul_table(14);

/// AES-128 (FIPS-197) with its expanded key: one block at a time and CBC decryption are all
/// the trophy packs need. Prints as `Aes128(..)`: the round keys are never shown.
#[derive(Clone)]
pub struct Aes128 {
    round_keys: [[u8; 16]; 11],
}

impl fmt::Debug for Aes128 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Aes128(..)")
    }
}

impl Aes128 {
    /// Expand `key`.
    pub fn new(key: &[u8; 16]) -> Aes128 {
        let mut words = [[0u8; 4]; 44];
        for (i, word) in words.iter_mut().take(4).enumerate() {
            word.copy_from_slice(&key[4 * i..4 * i + 4]);
        }
        let mut rcon = 1u8;
        for i in 4..44 {
            let mut t = words[i - 1];
            if i % 4 == 0 {
                t = [
                    SBOX[t[1] as usize] ^ rcon,
                    SBOX[t[2] as usize],
                    SBOX[t[3] as usize],
                    SBOX[t[0] as usize],
                ];
                rcon = xtime(rcon);
            }
            let before = words[i - 4];
            words[i] = [0, 1, 2, 3].map(|j| before[j] ^ t[j]);
        }
        let mut round_keys = [[0u8; 16]; 11];
        for (round, round_key) in round_keys.iter_mut().enumerate() {
            for column in 0..4 {
                round_key[4 * column..4 * column + 4].copy_from_slice(&words[4 * round + column]);
            }
        }
        Aes128 { round_keys }
    }

    /// Encrypt one block.
    pub fn encrypt_block(&self, block: &[u8; 16]) -> [u8; 16] {
        let mut s = *block;
        add_round_key(&mut s, &self.round_keys[0]);
        for (round, round_key) in self.round_keys.iter().enumerate().skip(1) {
            for byte in s.iter_mut() {
                *byte = SBOX[*byte as usize];
            }
            s = shift_rows(&s);
            if round != 10 {
                mix_columns(&mut s);
            }
            add_round_key(&mut s, round_key);
        }
        s
    }

    /// Decrypt one block.
    pub fn decrypt_block(&self, block: &[u8; 16]) -> [u8; 16] {
        let mut s = *block;
        add_round_key(&mut s, &self.round_keys[10]);
        for (round, round_key) in self.round_keys.iter().enumerate().take(10).rev() {
            s = inv_shift_rows(&s);
            for byte in s.iter_mut() {
                *byte = INV_SBOX[*byte as usize];
            }
            add_round_key(&mut s, round_key);
            if round != 0 {
                inv_mix_columns(&mut s);
            }
        }
        s
    }

    /// CBC-decrypt the whole blocks of `data` with `iv`; a trailing partial block is dropped,
    /// as in the Python.
    pub fn cbc_decrypt(&self, data: &[u8], iv: &[u8; 16]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() - data.len() % 16);
        let mut previous = *iv;
        for block in data.as_chunks::<16>().0 {
            let plain = self.decrypt_block(block);
            out.extend(plain.iter().zip(previous).map(|(a, b)| a ^ b));
            previous = *block;
        }
        out
    }
}

fn add_round_key(state: &mut [u8; 16], key: &[u8; 16]) {
    for (byte, k) in state.iter_mut().zip(key) {
        *byte ^= k;
    }
}

/// Row `r` of column `c` is byte `4c + r`; row `r` moves `r` columns left.
fn shift_rows(s: &[u8; 16]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for c in 0..4 {
        for r in 0..4 {
            out[4 * c + r] = s[4 * ((c + r) % 4) + r];
        }
    }
    out
}

fn inv_shift_rows(s: &[u8; 16]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for c in 0..4 {
        for r in 0..4 {
            out[4 * c + r] = s[4 * ((c + 4 - r) % 4) + r];
        }
    }
    out
}

fn mix_columns(s: &mut [u8; 16]) {
    for column in s.as_chunks_mut::<4>().0 {
        let [a0, a1, a2, a3] = *column;
        let all = a0 ^ a1 ^ a2 ^ a3;
        column[0] = a0 ^ all ^ xtime(a0 ^ a1);
        column[1] = a1 ^ all ^ xtime(a1 ^ a2);
        column[2] = a2 ^ all ^ xtime(a2 ^ a3);
        column[3] = a3 ^ all ^ xtime(a3 ^ a0);
    }
}

fn inv_mix_columns(s: &mut [u8; 16]) {
    for column in s.as_chunks_mut::<4>().0 {
        let [a0, a1, a2, a3] = column.map(usize::from);
        column[0] = MUL14[a0] ^ MUL11[a1] ^ MUL13[a2] ^ MUL9[a3];
        column[1] = MUL9[a0] ^ MUL14[a1] ^ MUL11[a2] ^ MUL13[a3];
        column[2] = MUL13[a0] ^ MUL9[a1] ^ MUL14[a2] ^ MUL11[a3];
        column[3] = MUL11[a0] ^ MUL13[a1] ^ MUL9[a2] ^ MUL14[a3];
    }
}

// ---- The console's trophy key -----------------------------------------------------------------

/// The console's trophy key, supplied at run time (see the module docs). Holds only the
/// expanded cipher; prints as `TrophyKey(..)`.
#[derive(Clone)]
pub struct TrophyKey {
    master: Aes128,
}

impl fmt::Debug for TrophyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TrophyKey(..)")
    }
}

impl TrophyKey {
    /// The key from its 16 bytes.
    pub fn from_bytes(key: [u8; 16]) -> TrophyKey {
        TrophyKey {
            master: Aes128::new(&key),
        }
    }

    /// The key from 32 hex digits (surrounding whitespace ignored). Anything else is
    /// `Error::Trophies`, and the message never repeats the text.
    pub fn from_hex(text: &str) -> Result<TrophyKey> {
        let text = text.trim();
        let digits = text.as_bytes();
        if digits.len() != 32 || !digits.iter().all(u8::is_ascii_hexdigit) {
            return Err(Error::Trophies(
                "the trophy key must be 32 hex digits".into(),
            ));
        }
        let mut key = [0u8; 16];
        for (byte, pair) in key.iter_mut().zip(digits.as_chunks::<2>().0) {
            let pair = std::str::from_utf8(pair).expect("hex digits are ASCII");
            *byte = u8::from_str_radix(pair, 16).expect("checked hex digits");
        }
        Ok(TrophyKey::from_bytes(key))
    }

    /// The key from [`KEY_VAR`] (`BB_TROPHY_KEY`), read through `var` (pass
    /// `|k| std::env::var(k).ok()` for the real environment). Unset or empty is
    /// `Error::Trophies`.
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Result<TrophyKey> {
        match var(KEY_VAR).filter(|value| !value.trim().is_empty()) {
            Some(value) => TrophyKey::from_hex(&value),
            None => Err(Error::Trophies(format!(
                "no trophy key: set {KEY_VAR} to the console's trophy key (32 hex digits)"
            ))),
        }
    }

    /// The key from a text file holding its 32 hex digits.
    pub fn from_file(path: &Path) -> Result<TrophyKey> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            Error::Trophies(format!("the trophy key file cannot be read: {}", e.kind()))
        })?;
        TrophyKey::from_hex(&text)
    }

    /// The cipher of one set: its NP communication id, padded with zeros to a block, encrypted
    /// with the trophy key (`title_key` in `trophies.py`). An id longer than a block is
    /// `Error::Trophies`.
    pub fn title_key(&self, npcommid: &str) -> Result<Aes128> {
        let id = npcommid.as_bytes();
        if id.len() > 16 {
            return Err(Error::Trophies(format!(
                "{npcommid} is not an NP communication id"
            )));
        }
        let mut block = [0u8; 16];
        block[..id.len()].copy_from_slice(id);
        Ok(Aes128::new(&self.master.encrypt_block(&block)))
    }
}

// ---- The trophy pack --------------------------------------------------------------------------

/// One entry of a trophy pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrpEntry {
    /// [`FLAG_ENCRYPTED`] for an encrypted ESFM entry.
    pub flag: u32,
    pub data: Vec<u8>,
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "the trophy pack".into())
}

fn trophy_error(message: impl Into<String>) -> Error {
    Error::Trophies(message.into())
}

/// The entries of a `.trp` file, by name (`read_trp` in `trophies.py`).
pub fn read_trp(path: &Path) -> Result<BTreeMap<String, TrpEntry>> {
    let label = file_label(path);
    let data = std::fs::read(path)
        .map_err(|e| trophy_error(format!("{label} cannot be read: {}", e.kind())))?;
    parse_trp(&data).map_err(|message| trophy_error(format!("{label}: {message}")))
}

/// The entries of a trophy pack in memory. Not a pack, or an entry table past the end of the
/// file, is `Error::Trophies`; an entry whose data runs past the end is left out, as in the
/// Python.
pub fn parse_trp(data: &[u8]) -> std::result::Result<BTreeMap<String, TrpEntry>, String> {
    let be32 = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().expect("4 bytes"));
    let be64 = |at: usize| u64::from_be_bytes(data[at..at + 8].try_into().expect("8 bytes"));
    if data.len() < HEADER_SIZE || be32(0) != TRP_MAGIC {
        return Err("not a trophy pack".into());
    }
    let count = be32(16) as usize;
    let entry_size = be32(20) as usize;
    let table_end = count
        .checked_mul(entry_size)
        .and_then(|size| size.checked_add(HEADER_SIZE));
    if count > 0 && (entry_size < ENTRY_FIELDS || table_end.is_none_or(|end| end > data.len())) {
        return Err("the trophy pack's entry table is damaged".into());
    }
    let mut entries = BTreeMap::new();
    for i in 0..count {
        let at = HEADER_SIZE + i * entry_size;
        let raw_name = &data[at..at + 32];
        let raw_name = &raw_name[..raw_name.iter().position(|&b| b == 0).unwrap_or(32)];
        let name: String = raw_name
            .iter()
            .map(|&b| if b.is_ascii() { b as char } else { '\u{FFFD}' })
            .collect();
        let offset = be64(at + 32);
        let size = be64(at + 40);
        let flag = be32(at + 48);
        let end = offset.checked_add(size);
        if let Some(end) = end.filter(|&end| end <= data.len() as u64) {
            entries.insert(
                name,
                TrpEntry {
                    flag,
                    data: data[offset as usize..end as usize].to_vec(),
                },
            );
        }
    }
    Ok(entries)
}

fn first_block(raw: &[u8]) -> Option<[u8; 16]> {
    raw.get(..16).map(|b| b.try_into().expect("16 bytes"))
}

/// The XML text of an ESFM entry: the first block is the IV of the rest, and the text ends at
/// its last `>` (`decrypt_esfm`). Text that does not decrypt is `Error::Trophies`.
pub fn decrypt_esfm(raw: &[u8], key: &Aes128) -> Result<String> {
    let iv = first_block(raw).ok_or_else(|| trophy_error("trophy text is too short"))?;
    let plain = key.cbc_decrypt(&raw[16..], &iv);
    let end = plain
        .iter()
        .rposition(|&b| b == b'>')
        .ok_or_else(|| trophy_error("trophy text did not decrypt"))?;
    Ok(String::from_utf8_lossy(&plain[..=end]).into_owned())
}

/// True when `key` opens `raw` (an ESFM entry): its first plain block is all zero.
pub fn key_fits(raw: &[u8], key: &Aes128) -> bool {
    first_block(raw).is_some_and(|block| key.decrypt_block(&block) == [0u8; 16])
}

/// The `NPWRnnnnn_nn` ids in `bytes`, in order, without overlaps.
fn npwr_ids(bytes: &[u8]) -> Vec<String> {
    let fits = |w: &[u8]| {
        w.starts_with(b"NPWR")
            && w[4..9].iter().all(u8::is_ascii_digit)
            && w[9] == b'_'
            && w[10..12].iter().all(u8::is_ascii_digit)
    };
    let mut ids = Vec::new();
    let mut at = 0;
    while at + 12 <= bytes.len() {
        if fits(&bytes[at..at + 12]) {
            ids.push(String::from_utf8_lossy(&bytes[at..at + 12]).into_owned());
            at += 12;
        } else {
            at += 1;
        }
    }
    ids
}

/// The NP communication id whose key opens `raw` (an ESFM entry): the ids in
/// `<game_dir>/sce_sys/npbind.dat`, then `wanted`, then the collection's three
/// ([`trp_file`]), then every `NPWRnnnnn_00` (`find_npcommid`). None fits is `Error::Trophies`.
pub fn find_npcommid(raw: &[u8], game_dir: &Path, wanted: &str, key: &TrophyKey) -> Result<String> {
    let mut candidates = std::fs::read(game_dir.join("sce_sys").join("npbind.dat"))
        .map(|npbind| npwr_ids(&npbind))
        .unwrap_or_default();
    candidates.push(wanted.to_owned());
    candidates.extend(Game::ALL.iter().map(|&g| trp_file(g).1.to_owned()));
    let mut tried = Vec::new();
    for npcommid in candidates {
        if npcommid.is_empty() || tried.contains(&npcommid) {
            continue;
        }
        if key_fits(raw, &key.title_key(&npcommid)?) {
            return Ok(npcommid);
        }
        tried.push(npcommid);
    }
    for number in 0..100_000u32 {
        let npcommid = format!("NPWR{number:05}_00");
        if key_fits(raw, &key.title_key(&npcommid)?) {
            return Ok(npcommid);
        }
    }
    Err(trophy_error(format!(
        "no trophy key fits this pack (is {KEY_VAR} the console's trophy key?)"
    )))
}

/// One decrypted TROPCONF or TROP XML (`parse_xml`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrophyXml {
    pub npcommid: String,
    pub title: String,
    pub detail: String,
    pub groups: u32,
    pub trophies: BTreeMap<u32, Trophy>,
}

/// Parse a decrypted TROPCONF or TROP XML. A trophy without a numeric id is skipped. Bad XML
/// is `Error::Trophies`. `roxmltree` runs with DTDs off and a node limit, as in `patches`.
pub fn parse_xml(text: &str) -> Result<TrophyXml> {
    if text.len() > MAX_XML_BYTES {
        return Err(trophy_error("trophy text exceeds the 8 MiB limit"));
    }
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: MAX_XML_NODES,
        entity_resolver: None,
    };
    let document = Document::parse_with_options(text, options)
        .map_err(|e| trophy_error(format!("trophy text is not XML: {e}")))?;
    let root = document.root_element();
    let child_text = |node: roxmltree::Node, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    let mut xml = TrophyXml {
        npcommid: child_text(root, "npcommid"),
        title: child_text(root, "title-name"),
        detail: child_text(root, "title-detail"),
        groups: root.children().filter(|c| c.has_tag_name("group")).count() as u32,
        trophies: BTreeMap::new(),
    };
    for node in root.descendants().filter(|n| n.has_tag_name("trophy")) {
        let Some(id) = node
            .attribute("id")
            .and_then(|id| id.trim().parse::<u32>().ok())
        else {
            continue;
        };
        xml.trophies.insert(
            id,
            Trophy {
                id,
                grade: Grade::from_letter(node.attribute("ttype").unwrap_or("B")),
                hidden: node.attribute("hidden") == Some("yes"),
                group: node.attribute("gid").unwrap_or("").to_owned(),
                name: child_text(node, "name"),
                detail: child_text(node, "detail"),
            },
        );
    }
    Ok(xml)
}

fn is_icon(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".PNG") else {
        return false;
    };
    stem == "ICON0"
        || stem
            .strip_prefix("TROP")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The set in a trophy pack (`load_trp`). Texts are in language `lang` when the pack has
/// `TROP_<lang>.ESFM`, else the default `TROP.ESFM`. `wanted` is the id to try after
/// `npbind.dat`. A damaged or undecryptable pack is `Error::Trophies`.
pub fn load_trp(
    trp_path: &Path,
    game_dir: &Path,
    lang: u32,
    wanted: &str,
    key: &TrophyKey,
) -> Result<TrophySet> {
    let entries = read_trp(trp_path)?;
    let conf = entries
        .get("TROPCONF.ESFM")
        .ok_or_else(|| trophy_error(format!("{}: no TROPCONF.ESFM", file_label(trp_path))))?;
    let npcommid = find_npcommid(&conf.data, game_dir, wanted, key)?;
    let cipher = key.title_key(&npcommid)?;
    let mut structure = parse_xml(&decrypt_esfm(&conf.data, &cipher)?)?;
    let texts = entries
        .get(&format!("TROP_{lang:02}.ESFM"))
        .or_else(|| entries.get("TROP.ESFM"));
    if let Some(texts) = texts {
        let named = parse_xml(&decrypt_esfm(&texts.data, &cipher)?)?;
        structure.title = named.title;
        structure.detail = named.detail;
        for (id, trophy) in named.trophies {
            let entry = structure
                .trophies
                .entry(id)
                .or_insert_with(|| trophy.clone());
            entry.name = trophy.name;
            entry.detail = trophy.detail;
        }
    }
    Ok(TrophySet {
        npcommid,
        title: structure.title,
        detail: structure.detail,
        groups: structure.groups,
        trophies: structure.trophies.into_values().collect(),
        icons: entries
            .into_iter()
            .filter(|(name, _)| is_icon(name))
            .map(|(name, entry)| (name, entry.data))
            .collect(),
    })
}

/// The trophy pack of `game` in `game_dir`: `sce_sys/trophy/<trp_file>`, or, when the folder
/// holds that game alone (`own`), `trophy00.trp` after it (`trp_for`).
pub fn trp_for(game_dir: &Path, game: Game, own: bool) -> Option<PathBuf> {
    let folder = game_dir.join("sce_sys").join("trophy");
    let own_name = own.then_some("trophy00.trp");
    std::iter::once(trp_file(game).0)
        .chain(own_name)
        .map(|name| folder.join(name))
        .find(|path| path.is_file())
}

/// Load `game`'s set from its `.trp` file in the collection folder `game_dir`, in language
/// `lang` (1 = English). No pack, or a damaged or undecryptable one, is `Error::Trophies`.
pub fn load(game_dir: &Path, game: Game, lang: u32, key: &TrophyKey) -> Result<TrophySet> {
    let trp = trp_for(game_dir, game, false)
        .ok_or_else(|| trophy_error(format!("{}: no {}", game.key(), trp_file(game).0)))?;
    load_trp(&trp, game_dir, lang, trp_file(game).1, key)
}

// ---- The cache the runtime reads ----------------------------------------------------------------

/// A cache field: backslash, tab and newline escaped, carriage returns dropped.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// The inverse of [`escape`]: `\t` and `\n` are tab and newline, `\` before anything else is
/// that character, and a lone trailing `\` stays.
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Write `set` into `out`: `trophies.txt` with `stamp`, and the icons (`write_cache`).
pub fn write_cache(set: &TrophySet, out: &Path, stamp: &str) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut lines = vec![
        format!("format\t{FORMAT}"),
        format!("stamp\t{stamp}"),
        format!("npcommid\t{}", set.npcommid),
        format!("title\t{}", escape(&set.title)),
        format!("detail\t{}", escape(&set.detail)),
        format!("groups\t{}", set.groups),
    ];
    let mut trophies: Vec<&Trophy> = set.trophies.iter().collect();
    trophies.sort_by_key(|t| t.id);
    for t in trophies {
        lines.push(format!(
            "trophy\t{}\t{}\t{}\t{}\t{}",
            t.id,
            t.grade.letter(),
            if t.hidden { 1 } else { 0 },
            escape(&t.name),
            escape(&t.detail)
        ));
    }
    for (name, blob) in &set.icons {
        if is_icon(name) {
            std::fs::write(out.join(name), blob)?;
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(out.join(CACHE_FILE), text)?;
    Ok(())
}

/// The set in a cache folder, without icons, or `None` when there is none, it is of another
/// format, or it lists no trophy (`read_cache`).
pub fn read_cache(out: &Path) -> Option<TrophySet> {
    let text = std::fs::read_to_string(out.join(CACHE_FILE)).ok()?;
    let mut set = TrophySet::default();
    for line in text.lines() {
        let part: Vec<&str> = line.split('\t').collect();
        match part[0] {
            "format" if part.get(1) != Some(&FORMAT.to_string().as_str()) => return None,
            "npcommid" if part.len() > 1 => set.npcommid = unescape(part[1]),
            "title" if part.len() > 1 => set.title = unescape(part[1]),
            "detail" if part.len() > 1 => set.detail = unescape(part[1]),
            "groups" if part.len() > 1 => set.groups = part[1].parse().unwrap_or(0),
            "trophy" if part.len() >= 6 => {
                let Some(id) = digits(part[1]) else { continue };
                set.trophies.push(Trophy {
                    id,
                    grade: Grade::from_letter(part[2]),
                    hidden: part[3] == "1",
                    group: String::new(),
                    name: unescape(part[4]),
                    detail: unescape(part[5]),
                });
            }
            _ => {}
        }
    }
    (!set.trophies.is_empty()).then_some(set)
}

/// A number written as ASCII digits only (Python's `isdigit` then `int`).
fn digits<T: std::str::FromStr>(text: &str) -> Option<T> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn stamp_of(trp: &Path, lang: u32) -> Result<String> {
    let meta = std::fs::metadata(trp)?;
    let modified = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let resolved = std::fs::canonicalize(trp)?;
    Ok(format!(
        "{}|{}|{modified}|{lang}",
        resolved.display(),
        meta.len()
    ))
}

/// Write the cache of `game` into `out` (`prepare` in `trophies.py`). It is kept when the
/// pack's path, size and time and the language are those of the cache's stamp, else made
/// again. `Ok(None)`: the game has no trophy pack, and any cache is removed. A pack that cannot
/// be read is `Error::Trophies` and removes the cache too; trophies are optional, so the caller
/// reports it and starts the game anyway.
pub fn prepare(
    game_dir: &Path,
    game: Game,
    out: &Path,
    lang: u32,
    own: bool,
    key: &TrophyKey,
) -> Result<Option<PathBuf>> {
    let cache = out.join(CACHE_FILE);
    let remove_cache = || match std::fs::remove_file(&cache) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::from(e)),
        _ => Ok(()),
    };
    let Some(trp) = trp_for(game_dir, game, own) else {
        remove_cache()?;
        return Ok(None);
    };
    let stamp = stamp_of(&trp, lang)?;
    let current = std::fs::read_to_string(&cache)
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("stamp\t").map(str::to_owned))
        })
        .unwrap_or_default();
    if current == stamp && read_cache(out).is_some() {
        return Ok(Some(out.to_path_buf()));
    }
    let written = load_trp(&trp, game_dir, lang, trp_file(game).1, key)
        .and_then(|set| write_cache(&set, out, &stamp));
    match written {
        Ok(()) => Ok(Some(out.to_path_buf())),
        Err(e) => {
            remove_cache()?;
            Err(match e {
                Error::Trophies(_) => e,
                other => trophy_error(format!("{}: {other}", game.key())),
            })
        }
    }
}

// ---- Unlocks ------------------------------------------------------------------------------------

/// The runtime's unlock log of a set: `<user_dir>/trophy/<npcommid>.log`.
pub fn unlock_log(user_dir: &Path, npcommid: &str) -> PathBuf {
    user_dir.join("trophy").join(format!("{npcommid}.log"))
}

/// The trophies unlocked so far, each with its earliest unlock time (unix seconds), from the
/// set's unlock log under `user_dir`. No log is no unlocks; a line that is not two numbers
/// first is skipped.
pub fn read_unlocks(user_dir: &Path, npcommid: &str) -> BTreeMap<u32, u64> {
    let mut unlocked = BTreeMap::new();
    let Ok(bytes) = std::fs::read(unlock_log(user_dir, npcommid)) else {
        return unlocked;
    };
    for line in String::from_utf8_lossy(&bytes).lines() {
        let mut part = line.split_whitespace();
        let (Some(id), Some(when)) = (part.next(), part.next()) else {
            continue;
        };
        let (Some(id), Some(when)) = (digits::<u32>(id), digits::<u64>(when)) else {
            continue;
        };
        unlocked
            .entry(id)
            .and_modify(|earliest: &mut u64| *earliest = (*earliest).min(when))
            .or_insert(when);
    }
    unlocked
}

/// How much of `set` is earned with the trophies in `unlocked` (ids of other sets are ignored).
pub fn progress(set: &TrophySet, unlocked: &BTreeMap<u32, u64>) -> Progress {
    let mut progress = Progress {
        total: set.trophies.len() as u32,
        ..Progress::default()
    };
    for trophy in &set.trophies {
        progress.by_grade.add(trophy.grade);
        if unlocked.contains_key(&trophy.id) {
            progress.earned.add(trophy.grade);
            progress.unlocked += 1;
        }
    }
    progress.percent = (progress.unlocked * 100)
        .checked_div(progress.total)
        .unwrap_or(0);
    progress
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generated_sbox_is_the_standard_one() {
        assert_eq!(SBOX[0x00], 0x63);
        assert_eq!(SBOX[0x01], 0x7C);
        assert_eq!(SBOX[0x53], 0xED);
        assert_eq!(SBOX[0xFF], 0x16);
        assert_eq!(INV_SBOX[0x63], 0x00);
    }

    #[test]
    fn a_truncated_entry_table_is_damaged_not_a_panic() {
        let mut data = vec![0u8; HEADER_SIZE];
        data[..4].copy_from_slice(&TRP_MAGIC.to_be_bytes());
        data[16..20].copy_from_slice(&3u32.to_be_bytes());
        data[20..24].copy_from_slice(&64u32.to_be_bytes());
        assert!(parse_trp(&data).unwrap_err().contains("damaged"));
        data[20..24].copy_from_slice(&0u32.to_be_bytes());
        assert!(parse_trp(&data).is_err());
    }

    #[test]
    fn npwr_ids_are_found_in_order() {
        let bytes = b"\0NPWR00001_00xxNPWR1234_00NPWR99999_01";
        assert_eq!(npwr_ids(bytes), ["NPWR00001_00", "NPWR99999_01"]);
    }

    #[test]
    fn icon_names() {
        assert!(is_icon("TROP001.PNG"));
        assert!(is_icon("ICON0.PNG"));
        assert!(!is_icon("GR001.PNG"));
        assert!(!is_icon("TROP.PNG"));
        assert!(!is_icon("../TROP1.PNG"));
    }

    #[test]
    fn keys_never_print() {
        let key = TrophyKey::from_bytes([7; 16]);
        assert_eq!(format!("{key:?}"), "TrophyKey(..)");
        assert_eq!(
            format!("{:?}", key.title_key("NPWR00001_00").unwrap()),
            "Aes128(..)"
        );
        let err = TrophyKey::from_hex("0123456789abcdef0123456789abcdeZ").unwrap_err();
        assert!(!err.to_string().contains("0123"));
    }
}
