//! Windows and the System V red zone. PS4 code (System V ABI) may keep data in the 128 bytes
//! below rsp in a function that calls nothing; signals on FreeBSD and Linux are delivered below
//! them. Windows puts an exception's frame right below rsp: with the thread's AVX state in use
//! (the upper halves of the YMM registers) it overwrites the stack from the 64-byte boundary at
//! or below rsp down, 72 to 128 of those bytes (Xeon E3-1240 v2, Windows 10; the
//! `windows_frame` test). Guest faults are routine here: the GPU code protects guest pages it
//! tracks and the access goes on after the fault. One inside such a function lost its data.
//! U1 1.02's constant table builder at 0x63df10 (U2 0xd645f0, U3 0x12d6cf0) keeps its entry
//! counters and store pointers there: an entry stayed unwritten, and a later draw called the
//! null member function its type picked ("execute access to 0", #0 guest offset 0x641532).
//!
//! Before the guest starts, the functions of each module come from its .eh_frame_hdr. In one
//! that uses memory below rsp (`[rsp-n]`, or `[rbp-n]` past its frame) and calls nothing, each
//! access that can fault (not the stack, fs/gs or read-only image data) runs from a thunk
//! between `lea rsp,[rsp-128]` and `lea rsp,[rsp+128]` (lea keeps the flags), so a fault's frame
//! lands below the data. A jump takes 5 bytes: a shorter access moves its neighbours along when
//! no direct branch lands between them, else it stays as it was (a breakpoint would be an
//! exception itself). Functions with indirect jumps, data in their code (jump tables), calls or
//! exception tables are left alone. Runs after every other code patch: a jump out of the image
//! there (a hook, a rewritten instruction) is not moved, and its thunk may come back to the
//! instruction after its padding.
//!
//! Piece 3.14, the port of `src/red_zone.c` (the decoder is iced-x86 here, Zydis there). It
//! exports the C symbols of `src/red_zone.h`. `BB_RED_ZONE=0` turns it off, `BB_RED_ZONE=list`
//! prints each function and access. The thunk memory code also runs on Linux (the tests do).

use crate::patch_util::{self, host, Bits, Decoded, Image};
use iced_x86::{
    FlowControl, InstructionInfoFactory, InstructionInfoOptions, Mnemonic, OpAccess, OpKind,
    Register,
};
use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError, RwLock};

/// Instructions per region, bytes a region's thunk may take, thunk memory per allocation.
const REGION_ITEMS: usize = 6;
const REGION_MAX: usize = 256;
const CHUNK: usize = 1 << 20;
/// Thunk memory reserved before the GPU starts (`red_zone_reserve`): committed chunk by chunk.
const RESERVE: usize = 16 << 20;

/// A loaded segment: offset in the image, size, ELF flags (1 execute, 2 write, 4 read).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RedZoneSegment {
    pub address: u64,
    pub size: u64,
    pub flags: u64,
}

struct Function {
    start: u64,
    end: u64,
    lsda: bool,
}

/// A thunk's code range and the guest offset of the instructions it runs.
struct Region {
    thunk: u64,
    end: u64,
    site: u64,
    seen: AtomicBool,
}

/// What keeps data below rsp and was not rewritten (functions left alone, accesses left as they
/// were), sorted by start: a fault there is reported.
struct Unprotected {
    start: u64,
    end: u64,
    why: &'static str,
}

#[derive(Default)]
struct State {
    chunk: u64,
    chunk_used: usize,
    reserved: u64,
    reserved_used: usize,
    chunks_outside: usize,
}

static STATE: Mutex<State> = Mutex::new(State {
    chunk: 0,
    chunk_used: CHUNK,
    reserved: 0,
    reserved_used: 0,
    chunks_outside: 0,
});
static REGIONS: RwLock<Vec<Region>> = RwLock::new(Vec::new());
static UNPROTECTED: RwLock<Vec<Unprotected>> = RwLock::new(Vec::new());
static IMAGE_BASE: AtomicU64 = AtomicU64::new(0);
static IMAGE_SIZE: AtomicU64 = AtomicU64::new(0);
static FAULTS_KEPT: AtomicU64 = AtomicU64::new(0);
static UNPROTECTED_FAULTS: AtomicU64 = AtomicU64::new(0);

fn mode_is(value: &str) -> bool {
    std::env::var_os("BB_RED_ZONE").is_some_and(|m| m == value)
}

fn segment_of(segments: &[RedZoneSegment], offset: u64) -> Option<&RedZoneSegment> {
    segments
        .iter()
        .find(|s| offset.wrapping_sub(s.address) < s.size)
}

/// .eh_frame (DWARF CFI with the GNU 'z' augmentation): a function's length and whether it has
/// an exception table (LSDA).
fn uleb(data: &[u8], p: &mut usize) -> u64 {
    let mut v = 0u64;
    let mut shift = 0;
    while *p < data.len() && shift < 64 {
        let c = data[*p];
        *p += 1;
        v |= u64::from(c & 0x7f) << shift;
        if c & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    v
}

fn encoded_size(encoding: u8) -> usize {
    match encoding & 0x0f {
        0x00 | 0x04 | 0x0c => 8,
        0x02 | 0x0a => 2,
        0x03 | 0x0b => 4,
        _ => 0,
    }
}

fn read_value(data: &[u8], at: usize, size: usize) -> Option<u64> {
    let mut v = [0u8; 8];
    v[..size].copy_from_slice(data.get(at..at.checked_add(size)?)?);
    Some(u64::from_le_bytes(v))
}

struct Cie {
    fde_encoding: u8,
    lsda_encoding: u8,
    augmented: bool,
    lsda: bool,
}

fn parse_cie(image: &Image, offset: u64) -> Option<Cie> {
    let header = image.vec(offset, 8)?;
    let length = u32::from_le_bytes(header[..4].try_into().ok()?);
    let id = u32::from_le_bytes(header[4..].try_into().ok()?);
    if length == 0 || length == u32::MAX || id != 0 {
        return None;
    }
    let data = image.vec(offset, 4 + u64::from(length))?;
    let end = data.len();
    let mut p = 8;
    let version = *data.get(p)?;
    p += 1;
    let n = data[p..].iter().position(|&b| b == 0)?;
    let augmentation = data[p..p + n].to_vec();
    if p + n >= end || augmentation.contains(&b'e') {
        return None; // "eh": old GCC data
    }
    p += n + 1;
    uleb(&data, &mut p); // code and data alignment
    uleb(&data, &mut p);
    if version == 1 {
        p += 1;
    } else {
        uleb(&data, &mut p); // return address register
    }
    let mut cie = Cie {
        fde_encoding: 0,
        lsda_encoding: 0xff,
        augmented: augmentation.first() == Some(&b'z'),
        lsda: false,
    };
    if !cie.augmented {
        return Some(cie);
    }
    uleb(&data, &mut p);
    for &a in &augmentation[1..] {
        if p >= end {
            return None;
        }
        match a {
            b'R' => {
                cie.fde_encoding = data[p];
                p += 1;
            }
            b'L' => {
                cie.lsda = true;
                cie.lsda_encoding = data[p];
                p += 1;
            }
            b'P' => {
                let size = encoded_size(data[p]);
                p += 1;
                if size == 0 {
                    return None;
                }
                p += size;
            }
            b'S' | b'B' => {}
            _ => return None,
        }
    }
    Some(cie)
}

/// The length of the function an FDE describes and whether it has an exception table.
fn parse_fde(image: &Image, offset: u64) -> Option<(u64, bool)> {
    if offset + 8 > image.size() {
        return None;
    }
    let header = image.vec(offset, 8)?;
    let length = u32::from_le_bytes(header[..4].try_into().ok()?);
    let back = u32::from_le_bytes(header[4..].try_into().ok()?);
    if length == 0 || length == u32::MAX || back == 0 {
        return None;
    }
    let data = image.vec(offset, 4 + u64::from(length))?;
    if u64::from(back) > offset + 4 {
        return None;
    }
    let cie = parse_cie(image, offset + 4 - u64::from(back))?;
    let size = encoded_size(cie.fde_encoding);
    let end = data.len();
    let mut p = 8 + size;
    if size == 0 || p + size > end {
        return None;
    }
    let range = read_value(&data, p, size)?;
    p += size;
    let mut lsda = false;
    if cie.augmented && cie.lsda && cie.lsda_encoding != 0xff {
        uleb(&data, &mut p);
        let lsda_size = encoded_size(cie.lsda_encoding);
        lsda = lsda_size == 0 || p + lsda_size > end || read_value(&data, p, lsda_size)? != 0;
    }
    Some((range, lsda))
}

/// Every module's .eh_frame_hdr: version 1, pcrel sdata4 .eh_frame pointer, udata4 count and a
/// table of datarel sdata4 pairs (function start, FDE), sorted by start.
fn find_functions(image: &Image, segments: &[RedZoneSegment]) -> Vec<Function> {
    const HEADER: u32 = 0x3b03_1b01; // 01 1b 03 3b
    let mut functions: Vec<Function> = Vec::new();
    for s in segments.iter().filter(|s| s.flags & 4 != 0) {
        let end = s.address + s.size;
        let mut at = (s.address + 3) & !3;
        while at + 12 <= end {
            let here = at;
            at += 4;
            if image.u32_at(here) != Some(HEADER) {
                continue;
            }
            let Some(entries) = image.u32_at(here + 8) else {
                continue;
            };
            let entries = u64::from(entries);
            if entries == 0 || here + 12 + entries * 8 > end {
                continue;
            }
            let first = functions.len();
            let mut previous = 0u64;
            let mut good = true;
            for i in 0..entries {
                let (Some(location), Some(fde)) = (
                    image.u32_at(here + 12 + 8 * i),
                    image.u32_at(here + 16 + 8 * i),
                ) else {
                    good = false;
                    break;
                };
                let start = here.wrapping_add(location as i32 as i64 as u64);
                let code = segment_of(segments, start);
                let parsed = code.and_then(|c| {
                    parse_fde(image, here.wrapping_add(fde as i32 as i64 as u64)).map(|f| (c, f))
                });
                let Some((code, (range, lsda))) = parsed else {
                    good = false;
                    break;
                };
                if code.flags & 1 == 0 || start < previous {
                    good = false;
                    break;
                }
                previous = start;
                let limit = code.address + code.size;
                functions.push(Function {
                    start,
                    end: if range < limit - start {
                        start + range
                    } else {
                        limit
                    },
                    lsda,
                });
            }
            if !good {
                functions.truncate(first); // not a header after all
                continue;
            }
            at = here + 12 + entries * 8; // the loop's step is added by `at += 4` above
        }
    }
    functions
}

/// rel32 reach: every byte of `[at, at+length)` within 2 GiB (less a margin) of every byte of
/// the image.
fn in_reach(base: u64, size: u64, at: u64, length: u64) -> bool {
    const REACH: u64 = 0x7ff0_0000;
    if at >= base {
        return at + length - base < REACH;
    }
    at >= 0x10000 && base + size - at < REACH
}

/// First free range of `length` in reach: above the image (from 64 MiB past its end, as before,
/// then right after it), then below it. Reserved only when `commit` is false.
fn find_near(base: u64, size: u64, length: usize, commit: bool) -> Option<u64> {
    let len = length as u64;
    let starts = [
        (base + size + (64 << 20) + 0xffff) & !0xffff,
        (base + size + 0xffff) & !0xffff,
    ];
    for (s, &first) in starts.iter().enumerate() {
        let mut at = first;
        while in_reach(base, size, at, len) {
            if s == 1 && at >= starts[0] {
                break;
            }
            if let Some(p) = host::map(at, length, commit) {
                return Some(p);
            }
            at += 1 << 16;
        }
    }
    if base >= len + 0x10000 {
        let mut at = (base - len) & !0xffff;
        while in_reach(base, size, at, len) {
            if let Some(p) = host::map(at, length, commit) {
                return Some(p);
            }
            if at < (1 << 16) + 0x10000 {
                break;
            }
            at -= 1 << 16;
        }
    }
    None
}

/// Right after the image is mapped, before the GPU starts: reserves thunk memory within rel32
/// reach of the image while the address space around it is still empty. Started after the GPU,
/// the search found no free 1 MiB block in some boots (the driver's allocations had taken the
/// range) and thousands of accesses stayed as they were: their data was lost on faults (crashes
/// and hangs after checkpoints). Reserved only; committed as `red_zone_patch` uses it.
///
/// # Safety
/// `image` must be the mapped image of `image_size` bytes (it is not dereferenced here).
#[no_mangle]
pub unsafe extern "C" fn red_zone_reserve(image: *mut u8, image_size: u64) {
    if mode_is("0") || image.is_null() {
        return;
    }
    let mut st = lock();
    if st.reserved != 0 {
        return;
    }
    match find_near(image as usize as u64, image_size, RESERVE, false) {
        Some(p) => {
            st.reserved = p;
            st.reserved_used = 0;
        }
        None => println!(
            "Red zone: no {} MiB free near the image before the GPU starts; thunk memory is looked for later",
            RESERVE >> 20
        ),
    }
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Thunk memory: within rel32 reach of the whole image.
fn near_image(st: &mut State, image: &Image, size: usize) -> Option<u64> {
    if st.reserved != 0 && st.reserved_used + size <= RESERVE {
        let at = st.reserved + st.reserved_used as u64;
        if host::commit(at, size) {
            st.reserved_used += size;
            return Some(at);
        }
    }
    let p = find_near(image.addr(), image.size(), size, true)?;
    st.chunks_outside += 1;
    Some(p)
}

fn thunk_reserve(st: &mut State, image: &Image) -> Option<u64> {
    if st.chunk_used + REGION_MAX > CHUNK {
        st.chunk = near_image(st, image, CHUNK)?;
        st.chunk_used = 0;
    }
    Some(st.chunk + st.chunk_used as u64)
}

/// A decoded instruction and what it does with memory and rsp. `access`: a memory access that
/// can fault, run with rsp lowered; `rsp`: the instruction uses rsp.
#[derive(Clone)]
struct Item {
    d: Decoded,
    access: bool,
    rsp: bool,
}

impl Item {
    fn len(&self) -> u64 {
        self.d.len() as u64
    }
}

fn is_rsp(r: Register) -> bool {
    r != Register::None && r.full_register() == Register::RSP
}

/// Moving an instruction into a thunk: no branches, RIP-relative operands are re-aimed. The
/// instructions cpu_patch.rs rewrites stay in the image, where its handlers find them.
fn movable(it: &Item) -> bool {
    patch_util::movable(&it.d.ins)
        && !matches!(
            it.d.ins.mnemonic(),
            Mnemonic::Lzcnt
                | Mnemonic::Tzcnt
                | Mnemonic::Andn
                | Mnemonic::Bextr
                | Mnemonic::Blsi
                | Mnemonic::Blsmsk
                | Mnemonic::Blsr
                | Mnemonic::Movbe
        )
}

struct Patcher<'a> {
    image: Image,
    segments: &'a [RedZoneSegment],
    info: InstructionInfoFactory,
    listing: bool,
    regions: Vec<Region>,
    unprotected: Vec<Unprotected>,
    stats: Stats,
}

#[derive(Default)]
struct Stats {
    functions: u32,
    red: u32,
    calls: u32,
    indirect: u32,
    lsda: u32,
    undecodable: u32,
    accesses: u32,
    jumps: u32,
    moved: u32,
    left: u32,
}

impl Patcher<'_> {
    /// What the item does with memory and rsp; `red`: it uses memory below rsp (or below its
    /// frame under rbp: `frame` bytes pushed or reserved after `mov rbp, rsp`, -1 without a
    /// frame pointer).
    fn classify(&mut self, d: Decoded, frame: i64, red: &mut bool) -> Item {
        let ins = d.ins;
        let ignored = matches!(
            ins.mnemonic(),
            Mnemonic::Nop
                | Mnemonic::Prefetch
                | Mnemonic::Prefetchnta
                | Mnemonic::Prefetcht0
                | Mnemonic::Prefetcht1
                | Mnemonic::Prefetcht2
                | Mnemonic::Prefetchw
                | Mnemonic::Prefetchwt1
        );
        let mut item = Item {
            d,
            access: false,
            rsp: false,
        };
        let info = self
            .info
            .info_options(&ins, InstructionInfoOptions::NO_MEMORY_USAGE);
        if info.used_registers().iter().any(|u| is_rsp(u.register())) {
            item.rsp = true;
        }
        for i in 0..ins.op_count() {
            let kind = ins.op_kind(i);
            if kind != OpKind::Memory {
                if matches!(
                    kind,
                    OpKind::MemorySegRSI
                        | OpKind::MemorySegESI
                        | OpKind::MemoryESRDI
                        | OpKind::MemoryESEDI
                ) && !ignored
                    && !matches!(ins.segment_prefix(), Register::FS | Register::GS)
                {
                    item.access = true;
                }
                continue;
            }
            let (base, index) = (ins.memory_base(), ins.memory_index());
            let disp = ins.memory_displacement64() as i64;
            if is_rsp(base) || is_rsp(index) {
                item.rsp = true;
                if disp < 0 {
                    *red = true;
                }
                continue;
            }
            if frame >= 0 && base == Register::RBP && disp.checked_neg().is_some_and(|n| n > frame)
            {
                *red = true;
            }
            // `lea` computes an address: no access.
            if ins.mnemonic() == Mnemonic::Lea
                || ignored
                || matches!(ins.memory_segment(), Register::FS | Register::GS)
            {
                continue;
            }
            if ins.is_ip_rel_memory_operand() {
                let target = ins.memory_displacement64().wrapping_sub(self.image.addr());
                if let Some(data) = segment_of(self.segments, target) {
                    if data.flags & 2 == 0 {
                        continue; // read-only image data
                    }
                }
            }
            item.access = true;
        }
        item
    }

    /// The thunk for `items`: the accesses between `lea rsp,[rsp-128]` and `lea rsp,[rsp+128]`,
    /// the others as they were, then a jump back after the last.
    fn emit_thunk(&mut self, st: &mut State, items: &[Item]) -> Option<u64> {
        const LOWER: [u8; 5] = [0x48, 0x8d, 0x64, 0x24, 0x80];
        const RAISE: [u8; 8] = [0x48, 0x8d, 0xa4, 0x24, 0x80, 0, 0, 0];
        let t = thunk_reserve(st, &self.image)?;
        let mut code: Vec<u8> = Vec::with_capacity(REGION_MAX);
        let mut lowered = false;
        for it in items {
            if it.access != lowered {
                code.extend_from_slice(if lowered { &RAISE } else { &LOWER });
                lowered = !lowered;
            }
            code.extend_from_slice(&it.d.relocated(t + code.len() as u64)?);
        }
        if lowered {
            code.extend_from_slice(&RAISE);
        }
        let last = items.last()?;
        let back = last.d.at + last.len();
        let rel = i32::try_from(back as i64 - (t as i64 + code.len() as i64 + 5)).ok()?;
        code.push(0xe9);
        code.extend_from_slice(&rel.to_le_bytes());
        if st.chunk_used + code.len() > CHUNK {
            return None;
        }
        patch_util::write_code(t, &code);
        self.regions.push(Region {
            thunk: t,
            end: t + code.len() as u64,
            site: items[0].d.at - self.image.addr(),
            seen: AtomicBool::new(false),
        });
        st.chunk_used += (code.len() + 15) & !15;
        Some(t)
    }

    fn note_unprotected(&mut self, start: u64, end: u64, why: &'static str) {
        self.unprotected.push(Unprotected { start, end, why });
    }

    /// One function: decode it, and if it keeps data below rsp, send its accesses to thunks.
    fn patch_function(&mut self, st: &mut State, f: &Function, targets: &Bits) {
        let mut items: Vec<Item> = Vec::new();
        let (mut red, mut calls, mut indirect) = (false, false, false);
        let mut undecoded = false;
        let mut frame: i64 = -1; // bytes below rbp, -1: no frame pointer
        let mut at = f.start;
        while at < f.end {
            let Some(d) = patch_util::decode(&self.image, at, f.end) else {
                undecoded = true; // data: a jump table after the code
                break;
            };
            let ins = d.ins;
            let rsp_operand =
                |i: u32| ins.op_kind(i) == OpKind::Register && ins.op_register(i) == Register::RSP;
            // push rbp; mov rbp, rsp opens a frame; pushes and sub rsp, imm after it extend it.
            if ins.mnemonic() == Mnemonic::Mov
                && ins.op_count() == 2
                && ins.op_kind(0) == OpKind::Register
                && ins.op_register(0) == Register::RBP
                && rsp_operand(1)
            {
                frame = 0;
            } else if frame >= 0 && ins.mnemonic() == Mnemonic::Push {
                frame = frame.saturating_add(8);
            } else if frame >= 0
                && ins.mnemonic() == Mnemonic::Sub
                && rsp_operand(0)
                && ins.op_count() == 2
                && !matches!(ins.op_kind(1), OpKind::Register | OpKind::Memory)
            {
                frame = frame.saturating_add(ins.immediate(1) as i64);
            } else if frame >= 0
                && ins.mnemonic() != Mnemonic::Sub
                && ins.mnemonic() != Mnemonic::Push
            {
                // rsp moved some other way (and rsp, -32): the frame's size is not known.
                let info = self
                    .info
                    .info_options(&ins, InstructionInfoOptions::NO_MEMORY_USAGE);
                for i in 0..ins.op_count() {
                    if rsp_operand(i)
                        && matches!(
                            info.op_access(i),
                            OpAccess::Write
                                | OpAccess::ReadWrite
                                | OpAccess::CondWrite
                                | OpAccess::ReadCondWrite
                        )
                    {
                        frame = i64::MAX;
                    }
                }
            }
            let len = d.len() as u64;
            let item = self.classify(d, if frame == i64::MAX { -1 } else { frame }, &mut red);
            match ins.flow_control() {
                FlowControl::Call | FlowControl::IndirectCall => calls = true,
                FlowControl::IndirectBranch => indirect = true,
                _ => {}
            }
            items.push(item);
            at += len;
        }
        if !red {
            return;
        }
        self.stats.red += 1;
        if f.lsda || calls || indirect || undecoded {
            let why = if f.lsda {
                "exception table"
            } else if calls {
                "calls"
            } else if indirect {
                "indirect jump"
            } else {
                "data in its code"
            };
            if self.listing {
                println!(
                    "Red zone: function {:#x}-{:#x}: left alone ({why})",
                    f.start, f.end
                );
            }
            if f.lsda {
                self.stats.lsda += 1;
            } else if calls {
                self.stats.calls += 1;
            } else if indirect {
                self.stats.indirect += 1;
            } else {
                self.stats.undecodable += 1;
            }
            self.note_unprotected(f.start, f.end, why);
            return;
        }
        let (accesses, left) = (self.stats.accesses, self.stats.left);
        let base = self.image.addr();
        let offset = |it: &Item| it.d.at - base;
        let n = items.len();
        let mut covered = 0u64;
        let mut i = 0;
        while i < n {
            let cur = i;
            i += 1;
            if !items[cur].access || offset(&items[cur]) < covered {
                continue;
            }
            let here = offset(&items[cur]);
            if items[cur].rsp || !movable(&items[cur]) {
                let (why, note) = if items[cur].rsp {
                    ("uses rsp", "an access that uses rsp")
                } else {
                    ("not movable", "an access that cannot move")
                };
                if self.listing {
                    println!("Red zone:   access at {here:#x} left ({why})");
                }
                self.stats.left += 1;
                self.note_unprotected(here, here + items[cur].len(), note);
                continue;
            }
            // The access and the instructions after it, else the one or two before it.
            let (mut a, mut b) = (cur, cur + 1);
            let mut length = items[cur].len();
            while length < 5
                && b < n
                && b - a < REGION_ITEMS
                && movable(&items[b])
                && !targets.get(offset(&items[b]))
            {
                length += items[b].len();
                b += 1;
            }
            // An access right after comes along: its own region could start no earlier.
            while length >= 5
                && b < n
                && b - a < REGION_ITEMS
                && items[b].access
                && !items[b].rsp
                && movable(&items[b])
                && !targets.get(offset(&items[b]))
            {
                length += items[b].len();
                b += 1;
            }
            if length < 5 {
                b = cur + 1;
                length = items[cur].len();
                if !targets.get(here)
                    && cur > 0
                    && offset(&items[cur - 1]) >= covered
                    && movable(&items[cur - 1])
                {
                    a = cur - 1;
                    length += items[a].len();
                    if length < 5
                        && !targets.get(offset(&items[a]))
                        && a > 0
                        && offset(&items[a - 1]) >= covered
                        && movable(&items[a - 1])
                    {
                        a -= 1;
                        length += items[a].len();
                    }
                }
            }
            if length < 5 {
                if self.listing {
                    println!("Red zone:   access at {here:#x} left (no room for a jump)");
                }
                self.stats.left += 1;
                self.note_unprotected(
                    here,
                    here + items[cur].len(),
                    "an access without room for a jump",
                );
                continue;
            }
            for it in &mut items[a..b] {
                if it.rsp {
                    it.access = false; // moved, not under the lowered rsp
                }
            }
            let Some(thunk) = self.emit_thunk(st, &items[a..b]) else {
                self.stats.left += 1;
                self.note_unprotected(
                    here,
                    here + items[cur].len(),
                    "an access without thunk memory",
                );
                continue;
            };
            let site = offset(&items[a]);
            let to = thunk as i64 - (base as i64 + site as i64 + 5);
            let mut patch = vec![0xcc; length as usize];
            patch[0] = 0xe9;
            patch[1..5].copy_from_slice(&(to as i32).to_le_bytes());
            self.image.write(site, &patch);
            self.stats.accesses += items[a..b].iter().filter(|it| it.access).count() as u32;
            self.stats.jumps += 1;
            self.stats.moved += (b - a - 1) as u32;
            covered = site + length;
            i = b;
        }
        if self.listing {
            println!(
                "Red zone: function {:#x}-{:#x}: {} accesses lowered, {} left",
                f.start,
                f.end,
                self.stats.accesses - accesses,
                self.stats.left - left
            );
        }
    }

    /// Marks the targets of direct branches in `[start, end)`; `stop`: at the first byte that
    /// does not decode, else skip it. A jump out of the image (another patch) may come back after
    /// the nops or breakpoints that pad it.
    fn branch_targets(&self, start: u64, end: u64, stop: bool, targets: &mut Bits) {
        let size = self.image.size();
        let mut at = start;
        while at < end {
            let Some(d) = patch_util::decode(&self.image, at, end) else {
                if stop {
                    return;
                }
                at += 1;
                continue;
            };
            if let Some(target) = patch_util::direct_branch(&d.ins) {
                let target = target.wrapping_sub(self.image.addr());
                if target < size {
                    targets.set(target);
                } else if d.ins.mnemonic() == Mnemonic::Jmp {
                    let mut after = at + d.len() as u64;
                    while after < end && matches!(self.image.byte(after), Some(0x90 | 0xcc)) {
                        after += 1;
                    }
                    targets.set(after);
                }
            }
            at += d.len() as u64;
        }
    }
}

/// Before guest code runs (the image still writable, after every other code patch): the memory
/// accesses of functions that keep data below rsp run from thunks with rsp 128 bytes lower.
/// `BB_RED_ZONE=0` turns it off, `BB_RED_ZONE=list` prints each function and access.
///
/// # Safety
/// `image` must be `image_size` bytes of readable and writable memory and `segments` must point
/// at `count` entries.
#[no_mangle]
pub unsafe extern "C" fn red_zone_patch(
    image: *mut u8,
    image_size: u64,
    segments: *const RedZoneSegment,
    count: usize,
) {
    if mode_is("0") {
        println!("Red zone: BB_RED_ZONE=0, guest functions left as they are");
        return;
    }
    if image.is_null() {
        return;
    }
    let segments: &[RedZoneSegment] = if segments.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: the caller's contract.
        unsafe { std::slice::from_raw_parts(segments, count) }
    };
    // SAFETY: the caller's contract.
    let image = unsafe { Image::new(image, image_size) };
    patch(image, segments);
}

fn patch(image: Image, segments: &[RedZoneSegment]) {
    IMAGE_BASE.store(image.addr(), Ordering::Relaxed);
    IMAGE_SIZE.store(image.size(), Ordering::Relaxed);
    let functions = find_functions(&image, segments);
    let mut p = Patcher {
        image,
        segments,
        info: InstructionInfoFactory::new(),
        listing: mode_is("list"),
        regions: Vec::new(),
        unprotected: Vec::new(),
        stats: Stats::default(),
    };
    if functions.is_empty() {
        println!("Red zone: no function table found, guest functions left as they are");
        return;
    }
    // Direct branch targets: a sweep over each executable segment, and one from each function's
    // start (data after a function, such as a jump table, can put the first out of step).
    let mut targets = Bits::new(image.size());
    for s in segments.iter().filter(|s| s.flags & 1 != 0) {
        p.branch_targets(s.address, s.address + s.size, false, &mut targets);
    }
    for f in &functions {
        p.branch_targets(f.start, f.end, true, &mut targets);
    }
    p.stats.functions = functions.len() as u32;
    let mut st = lock();
    for f in &functions {
        p.patch_function(&mut st, f, &targets);
    }
    let s = &p.stats;
    println!(
        "Red zone: {} of {} guest functions keep data below rsp; {} memory accesses in them run with rsp \
         128 bytes lower ({} jumps, {} moved neighbours), {} left as they were; skipped: {} with calls, \
         {} with indirect jumps, {} with exception tables, {} with data in their code",
        s.red, s.functions, s.accesses, s.jumps, s.moved, s.left, s.calls, s.indirect, s.lsda, s.undecodable
    );
    if st.reserved != 0 {
        println!(
            "Red zone: thunk memory {} of {} MiB reserved before the GPU at {:#x}, {} MiB found later",
            st.reserved_used >> 20,
            RESERVE >> 20,
            st.reserved,
            st.chunks_outside * (CHUNK >> 20)
        );
    } else {
        println!(
            "Red zone: thunk memory found after the GPU started: {} MiB",
            st.chunks_outside * (CHUNK >> 20)
        );
    }
    drop(st);
    let Patcher {
        mut regions,
        mut unprotected,
        ..
    } = p;
    regions.sort_unstable_by_key(|r| r.thunk);
    unprotected.sort_unstable_by_key(|u| u.start);
    REGIONS
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .extend(regions);
    UNPROTECTED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .extend(unprotected);
}

fn region_of(regions: &[Region], address: u64) -> Option<&Region> {
    let low = regions.partition_point(|r| r.end <= address);
    regions.get(low).filter(|r| address >= r.thunk)
}

/// 1 when `address` is in a thunk: `*site` is the guest offset of the instructions it runs.
///
/// # Safety
/// `site` must be valid for writes.
#[no_mangle]
pub unsafe extern "C" fn red_zone_site(address: u64, site: *mut u64) -> c_int {
    let regions = REGIONS.read().unwrap_or_else(PoisonError::into_inner);
    match region_of(&regions, address) {
        Some(r) if !site.is_null() => {
            // SAFETY: the caller's contract.
            unsafe { *site = r.site };
            1
        }
        _ => 0,
    }
}

/// The exception handler resolved a fault at `rip` (a guest page tracked by the GPU code). A
/// fault in a thunk: its data was kept (each thunk's first, then counts). A fault at an access
/// left as it was, or in a function left alone: its data may be lost.
#[no_mangle]
pub extern "C" fn red_zone_fault_handled(rip: u64) {
    {
        let regions = REGIONS.read().unwrap_or_else(PoisonError::into_inner);
        if let Some(r) = region_of(&regions, rip) {
            let n = FAULTS_KEPT.fetch_add(1, Ordering::Relaxed) + 1;
            if !r.seen.swap(true, Ordering::Relaxed) || n == 1000 || n == 10000 || n == 100_000 {
                println!(
                    "Red zone: guest page fault {n} inside a function that keeps data below rsp (guest offset {:#x}); the data was kept",
                    r.site
                );
            }
            return;
        }
    }
    let base = IMAGE_BASE.load(Ordering::Relaxed);
    let size = IMAGE_SIZE.load(Ordering::Relaxed);
    let offset = rip.wrapping_sub(base);
    if base == 0 || offset >= size {
        return;
    }
    let unprotected = UNPROTECTED.read().unwrap_or_else(PoisonError::into_inner);
    let low = unprotected.partition_point(|u| u.end <= offset);
    let Some(u) = unprotected.get(low).filter(|u| offset >= u.start) else {
        return;
    };
    let n = UNPROTECTED_FAULTS.fetch_add(1, Ordering::Relaxed) + 1;
    if n <= 50 || n == 100 || n == 1000 || n == 10000 {
        println!(
            "Red zone: guest page fault {n} at guest offset {offset:#x}, in a function that keeps data below rsp, at {} (not rewritten): its data may be lost",
            u.why
        );
    }
}

#[cfg(test)]
mod tests;
