//! libSceNpTrophy on the game's own trophy pack (roadmap 3.05, ADR 0002). Replaces
//! `src/runtime_trophy.c` and exports its two C symbols, [`runtime_trophy_configure`] and
//! [`runtime_trophy_resolve`]; the nine `sceNpTrophy*` guest entries resolve through the NIDs in
//! [`crate::abi::NID_TABLE`].
//!
//! run.py has `scripts/trophies.py` leave the texts and icons of the running game's set in
//! `<out>/trophy/` (`trophies.txt`, `TROPxxx.PNG`); `probe.c` gives that folder to
//! [`runtime_trophy_configure`]. Unlocks are kept in `<user>/trophy/<NP communication id>.log`,
//! one `id unix-time` per line, which the launcher's Trophies page reads too. An unlock shows the
//! game's trophy notice (the overlay in bbgpu) and earns the platinum when it was the last of the
//! others. Without the set (a dump without `sce_sys/trophy`, or a probe started by hand) every
//! call is accepted and only logged.
//!
//! [`Trophies`] is the whole library as plain Rust over byte slices, with the folder, the notice
//! and the clock behind [`Host`]; the guest entry points at the bottom hold one in a global and
//! only turn pointers into slices. This module calls no C function: the loader hands it
//! `runtime_file_user_dir` and `bbgpu_trophy_notify` through [`runtime_trophy_bind`], so the
//! Rust tests link without the C side.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The library's error codes (only those the C file returned).
pub const INVALID_ARGUMENT: i32 = 0x8055_1604_u32 as i32;
pub const INVALID_HANDLE: i32 = 0x8055_1608_u32 as i32;
pub const INVALID_CONTEXT: i32 = 0x8055_1609_u32 as i32;
pub const INVALID_TROPHY_ID: i32 = 0x8055_160A_u32 as i32;
pub const ALREADY_UNLOCKED: i32 = 0x8055_160C_u32 as i32;
pub const PLATINUM_CANNOT_UNLOCK: i32 = 0x8055_160D_u32 as i32;

/// Trophy ids are `0..MAX_TROPHIES`; the unlock-state call reports them as 128 flags.
pub const MAX_TROPHIES: usize = 128;
const MAX_CONTEXTS: usize = 16;

/// `SceNpTrophyGameDetails`, `SceNpTrophyGameData`, `SceNpTrophyDetails` and `SceNpTrophyData`:
/// each starts with its size as a 64-bit field the game fills in.
pub const GAME_DETAILS_SIZE: usize = 0x4A0;
pub const GAME_DATA_SIZE: usize = 0x20;
pub const TROPHY_DETAILS_SIZE: usize = 0x498;
pub const TROPHY_DATA_SIZE: usize = 24;
/// Without a set an output structure is zeroed after its size field, when that size is above 8
/// and under this.
const CLEAR_LIMIT: u64 = 4096;

/// Microseconds from 0001-01-01 to the Unix epoch (`SceRtcTick`).
const RTC_UNIX_EPOCH_US: u64 = 62_135_596_800_000_000;
/// A `trophies.txt`, unlock log or icon this large or larger is not read.
const MAX_FILE: u64 = 64 << 20;
/// The C buffers the set's id, title and detail were copied into (with their NUL).
const ID_CAPACITY: usize = 32;
const TITLE_CAPACITY: usize = 256;
const DETAIL_CAPACITY: usize = 1024;

/// What the library needs from the rest of the runtime. The loader's is [`Bound`]; the tests
/// use a fake.
pub trait Host {
    /// The `--user` folder (`runtime_file_user_dir`); unlocks go to `<it>/trophy/<id>.log`.
    fn user_dir(&self) -> Vec<u8>;
    /// Whether an unlock shows the in-game notice (`BB_TROPHY_POPUP` is not `0...`).
    fn popup(&self) -> bool;
    /// Shows the notice (`bbgpu_trophy_notify`): grade 1 platinum, 2 gold, 3 silver, 4 bronze;
    /// `png` is the trophy's icon file, `None` when it could not be read.
    fn notify(&self, grade: i32, game: &[u8], name: &[u8], png: Option<&[u8]>);
    /// Unix time in seconds, the time an unlock is logged with.
    fn now(&self) -> i64;
}

/// One trophy of the set, from a `trophy` line of `trophies.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trophy {
    /// `P`, `G`, `S` or `B` (anything else counts as bronze); 0 when the field was empty.
    pub grade: u8,
    pub hidden: bool,
    pub name: Vec<u8>,
    pub detail: Vec<u8>,
    /// Unix time of the unlock, 0 while locked (an unlock logged at time 0 is kept as 1).
    pub when: i64,
}

/// The running game's set: `trophies.txt` and the unlocks from its log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Set {
    dir: Vec<u8>,
    id: Vec<u8>,
    title: Vec<u8>,
    detail: Vec<u8>,
    groups: i32,
    /// The platinum's id, `None` in a set without one.
    platinum: Option<usize>,
    trophies: Vec<Option<Trophy>>,
}

impl Set {
    /// Reads `trophies.txt` (`text`) of the set in `dir`, as `scripts/trophies.py` writes it:
    /// one record per line, fields split by tabs, `\n`, `\t` and `\\` escaped in the texts.
    /// Unknown records and trophy ids outside `0..MAX_TROPHIES` are skipped; the id is empty
    /// when no `npcommid` record names one.
    pub fn parse(dir: &[u8], text: &[u8]) -> Self {
        let mut set = Self {
            dir: dir.to_vec(),
            id: Vec::new(),
            title: Vec::new(),
            detail: Vec::new(),
            groups: 0,
            platinum: None,
            trophies: vec![None; MAX_TROPHIES],
        };
        for line in lines(until_nul(text)) {
            let field: Vec<&[u8]> = line.split(|&b| b == b'\t').take(7).collect();
            match (field[0], field.len()) {
                (b"npcommid", 2..) => set.id = truncated(field[1], ID_CAPACITY),
                (b"title", 2..) => set.title = truncated(&unescape(field[1]), TITLE_CAPACITY),
                (b"detail", 2..) => set.detail = truncated(&unescape(field[1]), DETAIL_CAPACITY),
                (b"groups", 2..) => set.groups = atoi(field[1]),
                (b"trophy", 6..) => {
                    let Some(id) = trophy_index(atoi(field[1])) else {
                        continue;
                    };
                    let grade = field[2].first().copied().unwrap_or(0);
                    set.trophies[id] = Some(Trophy {
                        grade,
                        hidden: field[3].first() == Some(&b'1'),
                        name: unescape(field[4]),
                        detail: unescape(field[5]),
                        when: 0,
                    });
                    if grade == b'P' {
                        set.platinum = Some(id);
                    }
                }
                _ => {}
            }
        }
        set
    }

    /// Takes the unlocks from the log (`text`): lines `id unix-time`; for a trophy listed more
    /// than once the earliest time wins. Lines that do not parse, and ids not in the set, are
    /// skipped.
    pub fn load_unlocks(&mut self, text: &[u8]) {
        for line in lines(until_nul(text)) {
            let Some((id, rest)) = scan_int(line) else {
                continue;
            };
            let Some((when, _)) = scan_int(rest) else {
                continue;
            };
            let Some(trophy) = usize::try_from(id)
                .ok()
                .filter(|&id| id < MAX_TROPHIES)
                .and_then(|id| self.trophies[id].as_mut())
            else {
                continue;
            };
            if trophy.when == 0 || when < trophy.when {
                trophy.when = if when != 0 { when } else { 1 };
            }
        }
    }

    pub fn id(&self) -> &[u8] {
        &self.id
    }

    pub fn trophy(&self, id: usize) -> Option<&Trophy> {
        self.trophies.get(id)?.as_ref()
    }

    /// `<user>/trophy/<id>.log`.
    pub fn log_path(&self, user_dir: &[u8]) -> PathBuf {
        path(&[user_dir, b"/trophy/", &self.id, b".log"].concat())
    }

    fn present(&self) -> impl Iterator<Item = (usize, &Trophy)> {
        self.trophies
            .iter()
            .enumerate()
            .filter_map(|(id, trophy)| Some((id, trophy.as_ref()?)))
    }

    /// Marks a trophy earned, appends it to the log and shows the notice.
    fn earn(&mut self, id: usize, when: i64, host: &dyn Host) {
        let Some(trophy) = self.trophies[id].as_mut() else {
            return;
        };
        trophy.when = when;
        append_unlock(&host.user_dir(), &self.id, id, when);
        let trophy = &self.trophies[id].as_ref().expect("present above");
        println!(
            "Runtime: trophy {id} unlocked ({}: {})",
            lossy(&self.id),
            lossy(&trophy.name)
        );
        if host.popup() {
            let icon = path(&[&self.dir[..], format!("/TROP{id:03}.PNG").as_bytes()].concat());
            let png = read_file(&icon);
            host.notify(
                grade_number(trophy.grade),
                &self.title,
                &trophy.name,
                png.as_deref(),
            );
        }
    }
}

/// The library's state: the set, the open contexts and the id counter that contexts and
/// handles share.
#[derive(Debug)]
pub struct Trophies {
    set: Option<Set>,
    /// Ids of the open contexts, 0 for a free slot (a 17th context is not tracked).
    contexts: [i32; MAX_CONTEXTS],
    next_id: i32,
}

impl Default for Trophies {
    fn default() -> Self {
        Self::new()
    }
}

impl Trophies {
    pub const fn new() -> Self {
        Self {
            set: None,
            contexts: [0; MAX_CONTEXTS],
            next_id: 1,
        }
    }

    pub fn set(&self) -> Option<&Set> {
        self.set.as_ref()
    }

    /// `runtime_trophy_configure`: loads `<dir>/trophies.txt` and the unlock log. A missing file
    /// leaves the state as it was; a set without an id leaves no set.
    pub fn configure(&mut self, dir: &[u8], host: &dyn Host) {
        let Some(text) = read_file(&path(&[dir, b"/trophies.txt"].concat())) else {
            println!(
                "Trophies: no set in {} (accepted and logged only)",
                lossy(dir)
            );
            return;
        };
        self.set = None;
        let mut set = Set::parse(dir, &text);
        if set.id.is_empty() {
            println!("Trophies: {} has no set id", lossy(dir));
            return;
        }
        if let Some(log) = read_file(&set.log_path(&host.user_dir())) {
            set.load_unlocks(&log);
        }
        let earned = set.present().filter(|(_, t)| t.when != 0).count();
        println!(
            "Trophies: {} \"{}\", {} trophies, {earned} earned",
            lossy(&set.id),
            lossy(&set.title),
            set.present().count()
        );
        self.set = Some(set);
    }

    fn new_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// `sceNpTrophyCreateContext`.
    pub fn create_context(&mut self, ctx: Option<&mut i32>, user: i32, label: u32) -> i32 {
        let Some(ctx) = ctx else {
            return INVALID_ARGUMENT;
        };
        let id = self.new_id();
        if let Some(slot) = self.contexts.iter_mut().find(|slot| **slot == 0) {
            *slot = id;
        }
        println!("Runtime: trophy context {id} (user {user}, service label {label})");
        *ctx = id;
        0
    }

    /// `sceNpTrophyCreateHandle`.
    pub fn create_handle(&mut self, handle: Option<&mut i32>) -> i32 {
        let Some(handle) = handle else {
            return INVALID_ARGUMENT;
        };
        *handle = self.new_id();
        0
    }

    /// `sceNpTrophyRegisterContext`: always succeeds.
    pub fn register(&self, ctx: i32) -> i32 {
        let note = if self.set.is_some() { "" } else { " (no set)" };
        println!("Runtime: trophy context {ctx} registered{note}");
        0
    }

    /// `sceNpTrophyDestroyContext` and `sceNpTrophyDestroyHandle`: always succeed.
    pub fn destroy(&mut self, id: i32) -> i32 {
        if let Some(slot) = self.contexts.iter_mut().find(|slot| **slot == id) {
            *slot = 0;
        }
        0
    }

    /// `sceNpTrophyUnlockTrophy`. `platinum` gets the platinum's id when this unlock earned it,
    /// else -1. Without a set every unlock is accepted and only logged.
    pub fn unlock(
        &mut self,
        ctx: i32,
        handle: i32,
        id: i32,
        platinum: Option<&mut i32>,
        host: &dyn Host,
    ) -> i32 {
        let mut earned_platinum = -1;
        let result = self.unlock_inner(ctx, handle, id, &mut earned_platinum, host);
        if let Some(platinum) = platinum {
            *platinum = earned_platinum;
        }
        result
    }

    fn unlock_inner(
        &mut self,
        ctx: i32,
        handle: i32,
        id: i32,
        platinum: &mut i32,
        host: &dyn Host,
    ) -> i32 {
        let Some(set) = self.set.as_mut() else {
            println!("Runtime: trophy {id} unlocked");
            return 0;
        };
        if let Err(code) = check(ctx, handle) {
            return code;
        }
        let Some(index) = trophy_index(id).filter(|&i| set.trophies[i].is_some()) else {
            return INVALID_TROPHY_ID;
        };
        let trophy = set.trophies[index].as_ref().expect("checked above");
        if trophy.grade == b'P' {
            return PLATINUM_CANNOT_UNLOCK;
        }
        if trophy.when != 0 {
            return ALREADY_UNLOCKED;
        }
        let now = host.now();
        set.earn(index, now, host);
        let rest = set
            .present()
            .filter(|(_, t)| t.grade != b'P' && t.when == 0)
            .count();
        if let Some(p) = set.platinum {
            if rest == 0 && set.trophies[p].as_ref().is_some_and(|t| t.when == 0) {
                set.earn(p, now, host);
                *platinum = p as i32;
            }
        }
        0
    }

    /// `sceNpTrophyGetGameInfo`. Each output is the structure as the game passed it, its size
    /// field first: `details` must say [`GAME_DETAILS_SIZE`] and `data` [`GAME_DATA_SIZE`].
    /// Without a set both are zeroed after their size field and the call succeeds.
    pub fn game_info(
        &self,
        ctx: i32,
        handle: i32,
        details: Option<&mut [u8]>,
        data: Option<&mut [u8]>,
    ) -> i32 {
        if let Err(code) = check(ctx, handle) {
            return code;
        }
        if details.is_none() && data.is_none() {
            return INVALID_ARGUMENT;
        }
        let Some(set) = &self.set else {
            clear_output(details);
            clear_output(data);
            return 0;
        };
        let (mut total, mut earned, mut all, mut got) = ([0u32; 4], [0u32; 4], 0u32, 0u32);
        for (_, trophy) in set.present() {
            let grade = grade_index(trophy.grade);
            total[grade] += 1;
            all += 1;
            if trophy.when != 0 {
                earned[grade] += 1;
                got += 1;
            }
        }
        if let Some(details) = details {
            let Some(d) = sized(details, GAME_DETAILS_SIZE) else {
                return INVALID_ARGUMENT;
            };
            let values = [
                set.groups as u32,
                all,
                total[0],
                total[1],
                total[2],
                total[3],
            ];
            put_u32s(&mut d[8..32], &values);
            d[32..GAME_DETAILS_SIZE].fill(0);
            put_text(&mut d[32..160], &set.title);
            put_text(&mut d[160..GAME_DETAILS_SIZE], &set.detail);
        }
        if let Some(data) = data {
            let Some(d) = sized(data, GAME_DATA_SIZE) else {
                return INVALID_ARGUMENT;
            };
            let percent = (got * 100).checked_div(all).unwrap_or(0);
            let values = [got, earned[0], earned[1], earned[2], earned[3], percent];
            put_u32s(&mut d[8..32], &values);
        }
        0
    }

    /// `sceNpTrophyGetTrophyInfo`: `details` gets [`TROPHY_DETAILS_SIZE`] bytes (id, grade,
    /// group -1, hidden, name, description) and `data` [`TROPHY_DATA_SIZE`] (id, unlocked, the
    /// unlock time as an `SceRtcTick`); both must hold that many. A hidden trophy tells nothing
    /// until it is earned. Without a set both are zeroed after their size field.
    pub fn trophy_info(
        &self,
        ctx: i32,
        handle: i32,
        id: i32,
        details: Option<&mut [u8]>,
        data: Option<&mut [u8]>,
    ) -> i32 {
        if let Err(code) = check(ctx, handle) {
            return code;
        }
        if details.is_none() && data.is_none() {
            return INVALID_ARGUMENT;
        }
        let Some(set) = &self.set else {
            clear_output(details);
            clear_output(data);
            return 0;
        };
        let Some(trophy) = trophy_index(id).and_then(|i| set.trophy(i)) else {
            return INVALID_TROPHY_ID;
        };
        if details
            .as_ref()
            .is_some_and(|d| d.len() < TROPHY_DETAILS_SIZE)
            || data.as_ref().is_some_and(|d| d.len() < TROPHY_DATA_SIZE)
        {
            return INVALID_ARGUMENT;
        }
        if let Some(d) = details {
            d[8..TROPHY_DETAILS_SIZE].fill(0);
            // id, grade, group (-1: no groups), as the guest's int32s.
            let head = [id as u32, grade_number(trophy.grade) as u32, u32::MAX];
            put_u32s(&mut d[8..20], &head);
            d[20] = u8::from(trophy.hidden);
            if !trophy.hidden || trophy.when != 0 {
                put_text(&mut d[24..152], &trophy.name);
                put_text(&mut d[152..TROPHY_DETAILS_SIZE], &trophy.detail);
            }
        }
        if let Some(d) = data {
            d[8..TROPHY_DATA_SIZE].fill(0);
            d[8..12].copy_from_slice(&id.to_ne_bytes());
            d[12] = u8::from(trophy.when != 0);
            let tick = if trophy.when != 0 {
                RTC_UNIX_EPOCH_US.wrapping_add((trophy.when as u64).wrapping_mul(1_000_000))
            } else {
                0
            };
            d[16..24].copy_from_slice(&tick.to_ne_bytes());
        }
        0
    }

    /// `sceNpTrophyGetTrophyUnlockState`: bit `n % 32` of `flags[n / 32]` is trophy `n` earned,
    /// and `count` the number of trophies in the set (0 and no flags without a set).
    pub fn unlock_state(
        &self,
        ctx: i32,
        handle: i32,
        flags: Option<&mut [u32; 4]>,
        count: Option<&mut u32>,
    ) -> i32 {
        let (Some(flags), Some(count)) = (flags, count) else {
            return INVALID_ARGUMENT;
        };
        if let Err(code) = check(ctx, handle) {
            return code;
        }
        *flags = [0; 4];
        *count = 0;
        for (id, trophy) in self.set.iter().flat_map(Set::present) {
            *count += 1;
            if trophy.when != 0 {
                flags[id >> 5] |= 1 << (id & 31);
            }
        }
        0
    }
}

/// The context and handle checks every query makes (any positive value passes).
fn check(ctx: i32, handle: i32) -> Result<(), i32> {
    if ctx <= 0 {
        Err(INVALID_CONTEXT)
    } else if handle <= 0 {
        Err(INVALID_HANDLE)
    } else {
        Ok(())
    }
}

fn trophy_index(id: i32) -> Option<usize> {
    usize::try_from(id).ok().filter(|&id| id < MAX_TROPHIES)
}

/// The grade as the notice and `SceNpTrophyDetails` number it.
fn grade_number(grade: u8) -> i32 {
    match grade {
        b'P' => 1,
        b'G' => 2,
        b'S' => 3,
        _ => 4,
    }
}

/// The grade's slot in the per-grade counts of `SceNpTrophyGameDetails`.
fn grade_index(grade: u8) -> usize {
    match grade {
        b'P' => 0,
        b'G' => 1,
        b'S' => 2,
        _ => 3,
    }
}

/// The size field the game put at the start of an output structure.
fn guest_size(out: &[u8]) -> u64 {
    out.get(..8)
        .map_or(0, |b| u64::from_ne_bytes(b.try_into().expect("8 bytes")))
}

/// `out` when its size field says `size` and it holds that many bytes.
fn sized(out: &mut [u8], size: usize) -> Option<&mut [u8]> {
    if guest_size(out) == size as u64 && out.len() >= size {
        Some(&mut out[..size])
    } else {
        None
    }
}

/// Without a set: the structure the game passed, zeroed after its size field.
fn clear_output(out: Option<&mut [u8]>) {
    let Some(out) = out else {
        return;
    };
    let size = guest_size(out);
    if size > 8 && size < CLEAR_LIMIT {
        let end = (size as usize).min(out.len());
        out[8..end].fill(0);
    }
}

/// `values` one after another at the start of `out`, in the guest's byte order.
fn put_u32s(out: &mut [u8], values: &[u32]) {
    for (index, value) in values.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
    }
}

/// `text` into a zeroed C string field, cut to leave room for its NUL.
fn put_text(field: &mut [u8], text: &[u8]) {
    let length = text.len().min(field.len().saturating_sub(1));
    field[..length].copy_from_slice(&text[..length]);
}

/// The text before its first NUL, as the C string functions saw the file.
fn until_nul(text: &[u8]) -> &[u8] {
    text.iter()
        .position(|&b| b == 0)
        .map_or(text, |end| &text[..end])
}

/// The non-empty lines of `text`, split at CR and LF.
fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split(|&b| b == b'\r' || b == b'\n')
        .filter(|line| !line.is_empty())
}

/// The first `capacity - 1` bytes of `text` (a C buffer of `capacity` bytes with its NUL).
fn truncated(text: &[u8], capacity: usize) -> Vec<u8> {
    text[..text.len().min(capacity - 1)].to_vec()
}

/// `\n` and `\t` to a newline and a tab; a backslash before any other byte keeps that byte.
fn unescape(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.iter();
    while let Some(&b) = bytes.next() {
        if b != b'\\' {
            out.push(b);
            continue;
        }
        match bytes.next() {
            Some(b'n') => out.push(b'\n'),
            Some(b't') => out.push(b'\t'),
            Some(&other) => out.push(other),
            None => out.push(b'\\'),
        }
    }
    out
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// A decimal integer after optional white space and sign, and the rest of the text after it;
/// `None` without a digit (`sscanf`'s `%d`). Saturates instead of overflowing.
fn scan_int(text: &[u8]) -> Option<(i64, &[u8])> {
    let start = text
        .iter()
        .position(|&b| !is_space(b))
        .unwrap_or(text.len());
    let mut rest = &text[start..];
    let negative = match rest.first() {
        Some(b'-') => {
            rest = &rest[1..];
            true
        }
        Some(b'+') => {
            rest = &rest[1..];
            false
        }
        _ => false,
    };
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let magnitude = rest[..digits].iter().fold(0i64, |value, &d| {
        value.saturating_mul(10).saturating_add(i64::from(d - b'0'))
    });
    let value = if negative {
        magnitude.saturating_neg()
    } else {
        magnitude
    };
    Some((value, &rest[digits..]))
}

/// `atoi`: the leading integer, 0 without one.
fn atoi(text: &[u8]) -> i32 {
    scan_int(text).map_or(0, |(value, _)| {
        value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    })
}

fn lossy(text: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(text)
}

/// A path from the bytes the C side passed (UTF-8 on Windows, see `src/compat_win.h`).
fn path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// The whole file, or `None` when it cannot be read or is [`MAX_FILE`] or larger.
fn read_file(path: &std::path::Path) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    if length >= MAX_FILE {
        return None;
    }
    let mut data = Vec::with_capacity(length as usize);
    file.read_to_end(&mut data).ok()?;
    Some(data)
}

/// Appends `id when` to `<user>/trophy/<set id>.log`, making the `trophy` folder (not the user
/// folder). Best effort: a failure leaves the unlock unsaved, as in the C file.
fn append_unlock(user_dir: &[u8], set_id: &[u8], id: usize, when: i64) {
    let _ = fs::create_dir(path(&[user_dir, b"/trophy"].concat()));
    let log = path(&[user_dir, b"/trophy/", set_id, b".log"].concat());
    if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(log) {
        let _ = file.write_all(format!("{id} {when}\n").as_bytes());
    }
}

// ---- the C side ----

/// `const char *runtime_file_user_dir(void)`.
pub type UserDirFn = unsafe extern "C" fn() -> *const c_char;
/// `void bbgpu_trophy_notify(int grade, const char *game, const char *name, const void *png,
/// uint64_t png_size)`.
pub type NotifyFn = unsafe extern "C" fn(c_int, *const c_char, *const c_char, *const c_void, u64);

/// The loader's [`Host`]: the functions [`runtime_trophy_bind`] was given, `BB_TROPHY_POPUP`
/// read once, and the system clock.
#[derive(Debug, Clone, Copy)]
pub struct Bound {
    user_dir: Option<UserDirFn>,
    notify: Option<NotifyFn>,
}

impl Host for Bound {
    fn user_dir(&self) -> Vec<u8> {
        // runtime_file.c's default when nothing was bound or it returned null.
        let default = b"user".to_vec();
        let Some(user_dir) = self.user_dir else {
            return default;
        };
        // SAFETY: the loader bound runtime_file_user_dir, which takes nothing and returns null
        // or a NUL-terminated string that stays valid.
        let dir = unsafe { user_dir() };
        if dir.is_null() {
            return default;
        }
        // SAFETY: non-null and NUL-terminated (above).
        unsafe { CStr::from_ptr(dir) }.to_bytes().to_vec()
    }

    fn popup(&self) -> bool {
        static POPUP: OnceLock<bool> = OnceLock::new();
        *POPUP.get_or_init(|| env_bytes("BB_TROPHY_POPUP").is_none_or(|v| v.first() != Some(&b'0')))
    }

    fn notify(&self, grade: i32, game: &[u8], name: &[u8], png: Option<&[u8]>) {
        let Some(notify) = self.notify else {
            return;
        };
        // The texts come from a file cut at its first NUL, so they hold none.
        let game = CString::new(game).unwrap_or_default();
        let name = CString::new(name).unwrap_or_default();
        let (icon, size) = png.map_or((std::ptr::null(), 0), |png| {
            (png.as_ptr().cast::<c_void>(), png.len() as u64)
        });
        // SAFETY: the loader bound bbgpu_trophy_notify; both strings are NUL-terminated and the
        // icon is null or `size` readable bytes, all alive for the call (it copies what it keeps).
        unsafe { notify(grade, game.as_ptr(), name.as_ptr(), icon, size) };
    }

    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
    }
}

fn env_bytes(name: &str) -> Option<Vec<u8>> {
    let value = std::env::var_os(name)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(value.into_vec())
    }
    #[cfg(not(unix))]
    {
        Some(value.to_string_lossy().into_owned().into_bytes())
    }
}

struct Global {
    trophies: Trophies,
    host: Bound,
}

static STATE: Mutex<Global> = Mutex::new(Global {
    trophies: Trophies::new(),
    host: Bound {
        user_dir: None,
        notify: None,
    },
});

/// The library's one lock, held for every call (and while the notice is shown, as in C).
fn state() -> MutexGuard<'static, Global> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Called once by the loader before [`runtime_trophy_configure`]: the user folder and the
/// in-game notice. Either may be null (then the folder is `user` and no notice is shown).
#[no_mangle]
pub extern "C" fn runtime_trophy_bind(user_dir: Option<UserDirFn>, notify: Option<NotifyFn>) {
    state().host = Bound { user_dir, notify };
}

/// The folder run.py prepared for the running game (`--trophies`, `scripts/trophies.py`).
///
/// # Safety
/// `dir` is null (ignored) or a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn runtime_trophy_configure(dir: *const c_char) {
    if dir.is_null() {
        return;
    }
    // SAFETY: non-null, and the caller guarantees a NUL-terminated string.
    let dir = unsafe { CStr::from_ptr(dir) }.to_bytes();
    let mut guard = state();
    let Global { trophies, host } = &mut *guard;
    trophies.configure(dir, host);
}

/// The address of the guest entry point for the import `name` (`NID#library#module`), 0 when
/// it is not one of this library's.
///
/// # Safety
/// `name` is null or a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn runtime_trophy_resolve(name: *const c_char) -> usize {
    if name.is_null() {
        return 0;
    }
    // SAFETY: non-null, and the caller guarantees a NUL-terminated string.
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return 0;
    };
    crate::abi::nid_name(name).and_then(export).unwrap_or(0)
}

/// The entry point for a library symbol name.
fn export(symbol: &str) -> Option<usize> {
    Some(match symbol {
        "sceNpTrophyCreateContext" => trophy_context as *const () as usize,
        "sceNpTrophyCreateHandle" => trophy_handle as *const () as usize,
        "sceNpTrophyRegisterContext" => trophy_register as *const () as usize,
        "sceNpTrophyUnlockTrophy" => trophy_unlock as *const () as usize,
        "sceNpTrophyGetGameInfo" => trophy_game_info as *const () as usize,
        "sceNpTrophyGetTrophyInfo" => trophy_info as *const () as usize,
        "sceNpTrophyDestroyContext" | "sceNpTrophyDestroyHandle" => {
            trophy_destroy as *const () as usize
        }
        "sceNpTrophyGetTrophyUnlockState" => trophy_unlock_state as *const () as usize,
        _ => return None,
    })
}

/// Guest code calls with the System V ABI (`ABI` in `src/runtime.h`). Only x86-64 has guest
/// code; elsewhere the entry points are plain C functions, so the module builds and tests the
/// same on every target.
macro_rules! guest {
    ($(#[$meta:meta])* unsafe fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> i32 $body:block) => {
        $(#[$meta])*
        #[cfg(target_arch = "x86_64")]
        unsafe extern "sysv64" fn $name($($arg: $ty),*) -> i32 $body
        $(#[$meta])*
        #[cfg(not(target_arch = "x86_64"))]
        unsafe extern "C" fn $name($($arg: $ty),*) -> i32 $body
    };
}

/// A guest output structure: as many bytes as its size field claims, or at least `minimum`
/// (the bytes the call writes whatever the claim; a claim outside `8..4096` counts as 8).
///
/// # Safety
/// `out` is null or points to a structure the game made, with its 64-bit size field first,
/// writable for the length returned and not otherwise borrowed during the call.
unsafe fn guest_struct<'a>(out: *mut c_void, minimum: usize) -> Option<&'a mut [u8]> {
    if out.is_null() {
        return None;
    }
    // SAFETY: non-null, and every such structure starts with its 8-byte size.
    let size = unsafe { out.cast::<u64>().read_unaligned() };
    let claimed = if (8..CLEAR_LIMIT).contains(&size) {
        size as usize
    } else {
        8
    };
    // SAFETY: the caller guarantees the structure is writable for this length.
    Some(unsafe { std::slice::from_raw_parts_mut(out.cast::<u8>(), claimed.max(minimum)) })
}

guest! {
    /// # Safety
    /// `ctx` is null or writable.
    unsafe fn trophy_context(ctx: *mut i32, user: i32, label: u32, _options: u64) -> i32 {
        // SAFETY: null or writable (the caller's contract).
        let ctx = unsafe { ctx.as_mut() };
        state().trophies.create_context(ctx, user, label)
    }
}

guest! {
    /// # Safety
    /// `handle` is null or writable.
    unsafe fn trophy_handle(handle: *mut i32) -> i32 {
        // SAFETY: null or writable (the caller's contract).
        let handle = unsafe { handle.as_mut() };
        state().trophies.create_handle(handle)
    }
}

guest! {
    /// # Safety
    /// No pointers.
    unsafe fn trophy_register(ctx: i32, _handle: i32, _options: u64) -> i32 {
        let result = state().trophies.register(ctx);
        start_test_unlocks();
        result
    }
}

guest! {
    /// # Safety
    /// No pointers.
    unsafe fn trophy_destroy(id: i32) -> i32 {
        state().trophies.destroy(id)
    }
}

guest! {
    /// # Safety
    /// `platinum` is null or writable.
    unsafe fn trophy_unlock(ctx: i32, handle: i32, id: i32, platinum: *mut i32) -> i32 {
        // SAFETY: null or writable (the caller's contract).
        let platinum = unsafe { platinum.as_mut() };
        let mut guard = state();
        let Global { trophies, host } = &mut *guard;
        trophies.unlock(ctx, handle, id, platinum, host)
    }
}

guest! {
    /// # Safety
    /// `details` and `data` are null or `SceNpTrophyGameDetails` / `SceNpTrophyGameData`.
    unsafe fn trophy_game_info(ctx: i32, handle: i32, details: *mut c_void, data: *mut c_void) -> i32 {
        // SAFETY: null or the game's structures (the caller's contract); nothing else borrows them.
        let (details, data) = unsafe { (guest_struct(details, 8), guest_struct(data, 8)) };
        state().trophies.game_info(ctx, handle, details, data)
    }
}

guest! {
    /// # Safety
    /// `details` and `data` are null or `SceNpTrophyDetails` / `SceNpTrophyData`.
    unsafe fn trophy_info(ctx: i32, handle: i32, id: i32, details: *mut c_void, data: *mut c_void) -> i32 {
        // SAFETY: null or the game's structures (the caller's contract); nothing else borrows them.
        let (details, data) = unsafe {
            (guest_struct(details, TROPHY_DETAILS_SIZE), guest_struct(data, TROPHY_DATA_SIZE))
        };
        state().trophies.trophy_info(ctx, handle, id, details, data)
    }
}

guest! {
    /// # Safety
    /// `flags` is null or four writable words, `count` null or writable.
    unsafe fn trophy_unlock_state(ctx: i32, handle: i32, flags: *mut u32, count: *mut u32) -> i32 {
        // SAFETY: null or writable for the lengths above (the caller's contract).
        let (flags, count) = unsafe { (flags.cast::<[u32; 4]>().as_mut(), count.as_mut()) };
        state().trophies.unlock_state(ctx, handle, flags, count)
    }
}

/// `BB_TROPHY_TEST=<id>[,<id>...]`: earns those trophies 12 s after the game registers its set,
/// then one every 7 s, to see the notice and the lists without playing for them. Once per run.
fn start_test_unlocks() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    let Some(list) = env_bytes("BB_TROPHY_TEST").filter(|list| !list.is_empty()) else {
        return;
    };
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("trophy-test".into())
        .spawn(move || {
            let mut pause = Duration::from_secs(12);
            for id in list.split(|&b| b == b',').filter(|id| !id.is_empty()) {
                std::thread::sleep(pause);
                let mut guard = state();
                let Global { trophies, host } = &mut *guard;
                let result = trophies.unlock(1, 1, atoi(id), None, host);
                drop(guard);
                println!("Runtime: BB_TROPHY_TEST unlock {} -> {result}", lossy(id));
                pause = Duration::from_secs(7);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("uu-trophy-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join("set")).unwrap();
            fs::create_dir_all(path.join("user")).unwrap();
            Self(path)
        }

        fn set(&self) -> Vec<u8> {
            self.0.join("set").to_str().unwrap().as_bytes().to_vec()
        }

        fn log(&self) -> PathBuf {
            self.0.join("user/trophy/NPWR00000_00.log")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    type Notice = (i32, Vec<u8>, Vec<u8>, Option<Vec<u8>>);

    struct FakeHost {
        user: Vec<u8>,
        popup: bool,
        now: Cell<i64>,
        notices: RefCell<Vec<Notice>>,
    }

    impl FakeHost {
        fn new(dir: &TempDir) -> Self {
            Self {
                user: dir.0.join("user").to_str().unwrap().as_bytes().to_vec(),
                popup: true,
                now: Cell::new(1_700_000_000),
                notices: RefCell::new(Vec::new()),
            }
        }
    }

    impl Host for FakeHost {
        fn user_dir(&self) -> Vec<u8> {
            self.user.clone()
        }
        fn popup(&self) -> bool {
            self.popup
        }
        fn notify(&self, grade: i32, game: &[u8], name: &[u8], png: Option<&[u8]>) {
            let notice = (grade, game.to_vec(), name.to_vec(), png.map(<[u8]>::to_vec));
            self.notices.borrow_mut().push(notice);
        }
        fn now(&self) -> i64 {
            self.now.get()
        }
    }

    /// A made-up set in the layout scripts/trophies.py writes (tests/test_trophies.py's
    /// fake pack: a platinum, a bronze and a hidden gold), plus a silver.
    const SET: &str = "npcommid\tNPWR00000_00\n\
        title\tUncharted\\u2122 Test\n\
        detail\tA made-up\\tset\n\
        groups\t1\n\
        trophy\t000\tP\t0\tPlatinum\tAll of them\n\
        trophy\t001\tB\t0\tFirst\tDo the first thing\n\
        trophy\t002\tG\t1\tSecret\tDo\\ta\\nthing\n\
        trophy\t003\tS\t0\tThird\tAnother\r\n\
        trophy\t200\tB\t0\tOut of range\tskipped\n\
        unknown\trecord\n";

    fn configured(dir: &TempDir, host: &FakeHost) -> Trophies {
        fs::write(dir.0.join("set/trophies.txt"), SET).unwrap();
        fs::write(dir.0.join("set/TROP001.PNG"), b"\x89PNG fake icon").unwrap();
        let mut trophies = Trophies::new();
        trophies.configure(&dir.set(), host);
        trophies
    }

    fn details(size: usize) -> Vec<u8> {
        let mut out = vec![0xAA; size];
        out[..8].copy_from_slice(&(size as u64).to_ne_bytes());
        out
    }

    fn u32_at(out: &[u8], offset: usize) -> u32 {
        u32::from_ne_bytes(out[offset..offset + 4].try_into().unwrap())
    }

    fn c_text(field: &[u8]) -> &[u8] {
        until_nul(field)
    }

    #[test]
    fn parse_reads_the_set_records_and_unescapes_the_texts() {
        let set = Set::parse(b"/made/up", SET.as_bytes());
        assert_eq!(set.id(), b"NPWR00000_00");
        // `\u` is not an escape: the backslash goes and the `u` stays.
        assert_eq!(set.title, b"Unchartedu2122 Test");
        assert_eq!(set.detail, b"A made-up\tset");
        assert_eq!(set.groups, 1);
        assert_eq!(set.platinum, Some(0));
        assert_eq!(set.present().count(), 4);
        let secret = set.trophy(2).unwrap();
        assert_eq!((secret.grade, secret.hidden), (b'G', true));
        assert_eq!(secret.detail, b"Do\ta\nthing");
        assert_eq!(set.trophy(3).unwrap().detail, b"Another");
        assert!(set.trophy(200).is_none());
    }

    #[test]
    fn unescape_and_number_parsing_follow_the_c_library() {
        assert_eq!(unescape(b"a\\nb\\tc\\\\d\\qe\\"), b"a\nb\tc\\dqe\\");
        assert_eq!(atoi(b"  042xyz"), 42);
        assert_eq!(atoi(b"-7"), -7);
        assert_eq!(atoi(b"x"), 0);
        assert_eq!(atoi(b"99999999999"), i32::MAX);
        assert_eq!(scan_int(b" 5 1700"), Some((5, &b" 1700"[..])));
        assert_eq!(scan_int(b"+"), None);
        assert_eq!(truncated(b"abcdef", 4), b"abc");
        assert_eq!(until_nul(b"ab\0cd"), b"ab");
    }

    #[test]
    fn a_folder_without_trophies_txt_keeps_no_set_and_accepts_every_call() {
        let dir = TempDir::new("no-set");
        let host = FakeHost::new(&dir);
        let mut trophies = Trophies::new();
        trophies.configure(&dir.set(), &host);
        assert!(trophies.set().is_none());
        let mut platinum = 7;
        assert_eq!(trophies.unlock(0, 0, 999, Some(&mut platinum), &host), 0);
        assert_eq!(platinum, -1);
        let mut game = details(GAME_DETAILS_SIZE);
        assert_eq!(trophies.game_info(1, 1, Some(&mut game), None), 0);
        assert!(game[8..].iter().all(|&b| b == 0));
        assert_eq!(&game[..8], &(GAME_DETAILS_SIZE as u64).to_ne_bytes());
        let mut data = details(TROPHY_DATA_SIZE);
        assert_eq!(trophies.trophy_info(1, 1, 999, None, Some(&mut data)), 0);
        assert!(data[8..].iter().all(|&b| b == 0));
        let (mut flags, mut count) = ([9; 4], 9);
        assert_eq!(
            trophies.unlock_state(1, 1, Some(&mut flags), Some(&mut count)),
            0
        );
        assert_eq!((flags, count), ([0; 4], 0));
        assert!(!dir.log().exists());
    }

    #[test]
    fn a_set_without_an_id_is_no_set() {
        let dir = TempDir::new("no-id");
        let host = FakeHost::new(&dir);
        fs::write(
            dir.0.join("set/trophies.txt"),
            "title\tNo id\ntrophy\t1\tB\t0\tA\tB\n",
        )
        .unwrap();
        let mut trophies = Trophies::new();
        trophies.configure(&dir.set(), &host);
        assert!(trophies.set().is_none());
    }

    #[test]
    fn clear_output_zeroes_after_the_size_field_only_for_sizes_from_9_to_4095() {
        for (size, cleared) in [
            (0u64, false),
            (8, false),
            (9, true),
            (4095, true),
            (4096, false),
        ] {
            let mut out = vec![0xAA; 4200];
            out[..8].copy_from_slice(&size.to_ne_bytes());
            clear_output(Some(&mut out));
            assert_eq!(out[8] == 0, cleared, "size {size}");
            assert_eq!(out[4100], 0xAA);
        }
    }

    #[test]
    fn an_unlock_is_logged_announced_and_kept_after_a_reload() {
        let dir = TempDir::new("persist");
        let host = FakeHost::new(&dir);
        let mut trophies = configured(&dir, &host);
        let mut platinum = 0;
        assert_eq!(trophies.unlock(1, 2, 1, Some(&mut platinum), &host), 0);
        assert_eq!(platinum, -1);
        assert_eq!(fs::read_to_string(dir.log()).unwrap(), "1 1700000000\n");
        let notices = host.notices.borrow().clone();
        let title = unescape(b"Uncharted\\u2122 Test");
        assert_eq!(
            notices,
            [(
                4,
                title,
                b"First".to_vec(),
                Some(b"\x89PNG fake icon".to_vec())
            )]
        );

        let reloaded = configured(&dir, &host);
        let set = reloaded.set().unwrap();
        assert_eq!(set.trophy(1).unwrap().when, 1_700_000_000);
        assert_eq!(set.trophy(3).unwrap().when, 0);
    }

    #[test]
    fn a_second_unlock_is_already_unlocked_and_not_logged_again() {
        let dir = TempDir::new("duplicate");
        let host = FakeHost::new(&dir);
        let mut trophies = configured(&dir, &host);
        assert_eq!(trophies.unlock(1, 1, 3, None, &host), 0);
        assert_eq!(trophies.unlock(1, 1, 3, None, &host), ALREADY_UNLOCKED);
        assert_eq!(fs::read_to_string(dir.log()).unwrap(), "3 1700000000\n");
        assert_eq!(host.notices.borrow().len(), 1);
    }

    #[test]
    fn the_last_other_trophy_earns_the_platinum_which_cannot_be_unlocked_itself() {
        let dir = TempDir::new("platinum");
        let host = FakeHost::new(&dir);
        let mut trophies = configured(&dir, &host);
        let mut platinum = 0;
        assert_eq!(
            trophies.unlock(1, 1, 0, Some(&mut platinum), &host),
            PLATINUM_CANNOT_UNLOCK
        );
        assert_eq!(platinum, -1);
        assert_eq!(trophies.unlock(1, 1, 1, Some(&mut platinum), &host), 0);
        assert_eq!(trophies.unlock(1, 1, 2, Some(&mut platinum), &host), 0);
        assert_eq!(platinum, -1);
        host.now.set(1_700_000_500);
        assert_eq!(trophies.unlock(1, 1, 3, Some(&mut platinum), &host), 0);
        assert_eq!(platinum, 0);
        assert_eq!(
            fs::read_to_string(dir.log()).unwrap(),
            "1 1700000000\n2 1700000000\n3 1700000500\n0 1700000500\n"
        );
        let grades: Vec<i32> = host.notices.borrow().iter().map(|n| n.0).collect();
        assert_eq!(grades, [4, 2, 3, 1]);
        // The secret gold's icon is missing: its notice has none.
        assert_eq!(host.notices.borrow()[1].3, None);
        assert_eq!(
            trophies.unlock(1, 1, 0, None, &host),
            PLATINUM_CANNOT_UNLOCK
        );
    }

    #[test]
    fn invalid_arguments_return_the_library_errors() {
        let dir = TempDir::new("invalid");
        let host = FakeHost::new(&dir);
        let mut trophies = configured(&dir, &host);
        assert_eq!(trophies.unlock(0, 1, 1, None, &host), INVALID_CONTEXT);
        assert_eq!(trophies.unlock(1, -1, 1, None, &host), INVALID_HANDLE);
        for id in [-1, 4, 128, 200] {
            assert_eq!(trophies.unlock(1, 1, id, None, &host), INVALID_TROPHY_ID);
        }
        assert_eq!(trophies.create_context(None, 1, 0), INVALID_ARGUMENT);
        assert_eq!(trophies.create_handle(None), INVALID_ARGUMENT);
        assert_eq!(trophies.game_info(1, 1, None, None), INVALID_ARGUMENT);
        assert_eq!(trophies.game_info(0, 1, None, None), INVALID_CONTEXT);
        assert_eq!(trophies.trophy_info(1, 1, 1, None, None), INVALID_ARGUMENT);
        assert_eq!(trophies.trophy_info(1, 0, 1, None, None), INVALID_HANDLE);
        let mut data = details(TROPHY_DATA_SIZE);
        assert_eq!(
            trophies.trophy_info(1, 1, 9, None, Some(&mut data)),
            INVALID_TROPHY_ID
        );
        let mut wrong = details(GAME_DETAILS_SIZE);
        wrong[..8].copy_from_slice(&0x400u64.to_ne_bytes());
        assert_eq!(
            trophies.game_info(1, 1, Some(&mut wrong), None),
            INVALID_ARGUMENT
        );
        let mut count = 0;
        assert_eq!(
            trophies.unlock_state(1, 1, None, Some(&mut count)),
            INVALID_ARGUMENT
        );
        assert_eq!(
            trophies.unlock_state(0, 1, Some(&mut [0; 4]), Some(&mut count)),
            INVALID_CONTEXT
        );
        assert!(!dir.log().exists());
    }

    #[test]
    fn contexts_and_handles_share_one_counter_and_destroy_frees_a_slot() {
        let mut trophies = Trophies::new();
        let (mut ctx, mut handle) = (0, 0);
        assert_eq!(trophies.create_context(Some(&mut ctx), 1, 0), 0);
        assert_eq!(trophies.create_handle(Some(&mut handle)), 0);
        assert_eq!((ctx, handle), (1, 2));
        assert_eq!(trophies.contexts[0], 1);
        assert_eq!(trophies.register(ctx), 0);
        assert_eq!(trophies.destroy(ctx), 0);
        assert_eq!(trophies.contexts[0], 0);
        assert_eq!(trophies.destroy(handle), 0);
    }

    #[test]
    fn game_info_counts_by_grade_and_writes_the_texts() {
        let dir = TempDir::new("game-info");
        let host = FakeHost::new(&dir);
        let mut trophies = configured(&dir, &host);
        trophies.unlock(1, 1, 2, None, &host);
        let mut game = details(GAME_DETAILS_SIZE);
        let mut data = details(GAME_DATA_SIZE);
        assert_eq!(
            trophies.game_info(1, 1, Some(&mut game), Some(&mut data)),
            0
        );
        let counts: Vec<u32> = (0..6).map(|i| u32_at(&game, 8 + i * 4)).collect();
        assert_eq!(counts, [1, 4, 1, 1, 1, 1]);
        assert_eq!(c_text(&game[32..160]), unescape(b"Uncharted\\u2122 Test"));
        assert_eq!(c_text(&game[160..]), b"A made-up\tset");
        let earned: Vec<u32> = (0..6).map(|i| u32_at(&data, 8 + i * 4)).collect();
        assert_eq!(earned, [1, 0, 1, 0, 0, 25]);
    }

    #[test]
    fn trophy_info_hides_a_hidden_trophy_until_it_is_earned() {
        let dir = TempDir::new("trophy-info");
        let host = FakeHost::new(&dir);
        let mut trophies = configured(&dir, &host);
        let mut d = details(TROPHY_DETAILS_SIZE);
        let mut data = details(TROPHY_DATA_SIZE);
        assert_eq!(
            trophies.trophy_info(1, 1, 2, Some(&mut d), Some(&mut data)),
            0
        );
        assert_eq!(
            (u32_at(&d, 8), u32_at(&d, 12), u32_at(&d, 16)),
            (2, 2, u32::MAX)
        );
        assert_eq!(d[20], 1);
        assert!(d[24..].iter().all(|&b| b == 0));
        assert_eq!((u32_at(&data, 8), data[12]), (2, 0));
        assert_eq!(&data[16..24], &[0; 8]);

        trophies.unlock(1, 1, 2, None, &host);
        assert_eq!(
            trophies.trophy_info(1, 1, 2, Some(&mut d), Some(&mut data)),
            0
        );
        assert_eq!(c_text(&d[24..152]), b"Secret");
        assert_eq!(c_text(&d[152..]), b"Do\ta\nthing");
        assert_eq!(data[12], 1);
        let tick = u64::from_ne_bytes(data[16..24].try_into().unwrap());
        assert_eq!(tick, RTC_UNIX_EPOCH_US + 1_700_000_000 * 1_000_000);
    }

    #[test]
    fn unlock_state_sets_one_bit_per_earned_trophy() {
        let dir = TempDir::new("unlock-state");
        let host = FakeHost::new(&dir);
        let mut trophies = configured(&dir, &host);
        trophies.unlock(1, 1, 1, None, &host);
        trophies.unlock(1, 1, 3, None, &host);
        let (mut flags, mut count) = ([0; 4], 0);
        assert_eq!(
            trophies.unlock_state(1, 1, Some(&mut flags), Some(&mut count)),
            0
        );
        assert_eq!((flags, count), ([0b1010, 0, 0, 0], 4));
    }

    #[test]
    fn the_earliest_logged_unlock_wins_and_bad_lines_are_skipped() {
        let mut set = Set::parse(b"/made/up", SET.as_bytes());
        set.load_unlocks(b"1 500\n1 300\r\n1 400\n3 0\n2\nx 5\n9 100\n-1 100\n128 100\n");
        assert_eq!(set.trophy(1).unwrap().when, 300);
        assert_eq!(set.trophy(3).unwrap().when, 1);
        assert_eq!(set.trophy(2).unwrap().when, 0);
    }

    #[test]
    fn without_the_popup_an_unlock_shows_no_notice() {
        let dir = TempDir::new("no-popup");
        let mut host = FakeHost::new(&dir);
        host.popup = false;
        let mut trophies = configured(&dir, &host);
        assert_eq!(trophies.unlock(1, 1, 1, None, &host), 0);
        assert!(host.notices.borrow().is_empty());
        assert!(dir.log().exists());
    }

    #[test]
    fn the_guest_entries_resolve_by_nid_and_check_their_pointers() {
        let resolve = |name: &str| {
            let name = CString::new(name).unwrap();
            // SAFETY: a NUL-terminated string alive for the call.
            unsafe { runtime_trophy_resolve(name.as_ptr()) }
        };
        assert_eq!(
            resolve("XbkjbobZlCY#l#m"),
            trophy_context as *const () as usize
        );
        assert_eq!(
            resolve("28xmRUFao68#l#m"),
            trophy_unlock as *const () as usize
        );
        assert_eq!(
            resolve("E1Wrwd07Lr8#l#m"),
            trophy_destroy as *const () as usize
        );
        assert_eq!(
            resolve("GNcF4oidY0Y#l#m"),
            trophy_destroy as *const () as usize
        );
        assert_eq!(
            resolve("LHuSmO3SLd8#l#m"),
            trophy_unlock_state as *const () as usize
        );
        assert_eq!(resolve("XbkjbobZlCY#q#q"), 0);
        assert_eq!(resolve("bzQExy189ZI#q#q"), 0);
        // SAFETY: null is allowed.
        assert_eq!(unsafe { runtime_trophy_resolve(std::ptr::null()) }, 0);
        let nine: std::collections::HashSet<usize> = crate::abi::NID_TABLE
            .iter()
            .filter_map(|(nid, _)| Some(resolve(nid)).filter(|&f| f != 0))
            .collect();
        assert_eq!(
            nine.len(),
            8,
            "nine NIDs, the two destroys share one function"
        );

        // SAFETY: null pointers, which every entry rejects before touching them.
        unsafe {
            assert_eq!(
                trophy_context(std::ptr::null_mut(), 1, 0, 0),
                INVALID_ARGUMENT
            );
            assert_eq!(trophy_handle(std::ptr::null_mut()), INVALID_ARGUMENT);
            let null = std::ptr::null_mut();
            assert_eq!(trophy_game_info(1, 1, null, null), INVALID_ARGUMENT);
            assert_eq!(trophy_info(1, 1, 0, null, null), INVALID_ARGUMENT);
            assert_eq!(
                trophy_unlock_state(1, 1, null.cast(), null.cast()),
                INVALID_ARGUMENT
            );
        }
        let mut data = details(GAME_DATA_SIZE);
        // SAFETY: a structure of the claimed size; the global state has no set in the tests.
        let result =
            unsafe { trophy_game_info(0, 1, std::ptr::null_mut(), data.as_mut_ptr().cast()) };
        assert_eq!(result, INVALID_CONTEXT);
    }
}
