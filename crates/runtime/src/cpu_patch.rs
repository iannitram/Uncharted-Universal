//! Instructions the PS4's CPU (AMD Jaguar) has and older PC CPUs lack: BMI1 (ANDN, BEXTR, BLSI,
//! BLSMSK, BLSR, TZCNT), LZCNT and MOVBE, all missing before Intel Haswell (Ivy Bridge and Sandy
//! Bridge Core i and Xeon E3/E5 v1/v2). Guest code runs natively, so there the first BMI1
//! instruction stops the game with an illegal instruction (0xc000001d), and LZCNT runs as BSR
//! (another result, no fault). Before the guest starts, the executable segments are disassembled
//! (a linear sweep; clang keeps jump tables out of code) and each such instruction jumps to a
//! thunk near the image that computes the same result and the same defined flags with older
//! instructions, then jumps back. A jump takes 5 bytes: a shorter instruction (lzcnt eax, ecx is
//! 4) moves its neighbours into the thunk when no direct branch lands between them, else it
//! becomes a breakpoint that the exception handler sends to the thunk. An illegal instruction the
//! sweep missed runs from a thunk too ([`cpu_patch_illegal`]). Nothing changes on a CPU with
//! them.
//!
//! Piece 3.14, the port of `src/cpu_patch.c` (the decoder is iced-x86 here, Zydis there). It
//! exports the C symbols of `src/cpu_patch.h`. `BB_CPU_PATCH=0` turns it off, `BB_CPU_PATCH=all`
//! rewrites every supported instruction (tests on a CPU that has them).

use crate::patch_util::{self, host, Bits, Decoded, Image};
use iced_x86::{Mnemonic, OpKind, Register};
use std::ffi::c_int;
use std::io::Write as _;
use std::sync::{Mutex, PoisonError, RwLock};

const NEED_BMI1: u32 = 1;
const NEED_LZCNT: u32 = 2;
const NEED_MOVBE: u32 = 4;

const LZCNT: i32 = 0;
const TZCNT: i32 = 1;
const ANDN: i32 = 2;
const BLSR: i32 = 3;
const BLSMSK: i32 = 4;
const BLSI: i32 = 5;
const BEXTR: i32 = 6;
const MOVBE_LOAD: i32 = 7;
const MOVBE_STORE: i32 = 8;
const KINDS: usize = 9;
const KIND_NAMES: [&str; KINDS] = [
    "LZCNT", "TZCNT", "ANDN", "BLSR", "BLSMSK", "BLSI", "BEXTR", "MOVBE", "MOVBE",
];

const CF: u64 = 0x1;
const ZF: u64 = 0x40;
const OF: u64 = 0x800;

/// Bytes a thunk may take per instruction (the C call is the largest), instructions per region,
/// and bytes per thunk memory chunk.
const ITEM_MAX: usize = 256;
const REGION_ITEMS: usize = 4;
const CHUNK: usize = 1 << 20;

/// A memory operand of an instruction run by C code (thunks for BEXTR and 16-bit MOVBE).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuPatchMem {
    pub base: c_int,
    pub index: c_int,
    pub scale: c_int,
    pub rip: c_int,
    pub disp: i64,
    pub absolute: u64,
}

/// An instruction run by Rust code (thunks for BEXTR and 16-bit MOVBE); public for the thunks.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuPatchOp {
    pub kind: c_int,
    pub width: c_int,
    pub reg: [c_int; 3],
    pub mem: CpuPatchMem,
}

struct State {
    image: Option<Image>,
    chunk: u64,
    chunk_used: usize,
    missed: Vec<(u64, u64)>,
    rewritten: [u32; KINDS],
}

static STATE: Mutex<State> = Mutex::new(State {
    image: None,
    chunk: 0,
    chunk_used: CHUNK,
    missed: Vec::new(),
    rewritten: [0; KINDS],
});
/// (address, thunk) of the breakpoints, sorted; written before guest code runs.
static BREAKPOINTS: RwLock<Vec<(u64, u64)>> = RwLock::new(Vec::new());

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn mode_is(value: &str) -> bool {
    std::env::var_os("BB_CPU_PATCH").is_some_and(|m| m == value)
}

fn host_missing() -> u32 {
    use std::arch::x86_64::{__cpuid, __cpuid_count};
    let mut missing = 0;
    let max = __cpuid(0).eax;
    let max_extended = __cpuid(0x8000_0000).eax;
    if max < 7 || __cpuid_count(7, 0).ebx & (1 << 3) == 0 {
        missing |= NEED_BMI1;
    }
    if max_extended < 0x8000_0001 || __cpuid(0x8000_0001).ecx & (1 << 5) == 0 {
        missing |= NEED_LZCNT;
    }
    if max < 1 || __cpuid(1).ecx & (1 << 22) == 0 {
        missing |= NEED_MOVBE;
    }
    missing
}

/// The rest of Jaguar's instruction set has no rewrite: name what is missing.
fn report_unsupported() {
    const LEAF1: [(u32, &str); 10] = [
        (0, "SSE3"),
        (1, "PCLMULQDQ"),
        (9, "SSSE3"),
        (19, "SSE4.1"),
        (20, "SSE4.2"),
        (23, "POPCNT"),
        (25, "AES"),
        (26, "XSAVE"),
        (28, "AVX"),
        (29, "F16C"),
    ];
    let ecx = std::arch::x86_64::__cpuid(1).ecx;
    let mut list = String::new();
    for (bit, name) in LEAF1 {
        if ecx & (1 << bit) == 0 {
            list.push(' ');
            list.push_str(name);
        }
    }
    if !list.is_empty() {
        println!("CPU: this processor lacks{list}, which the PS4 has; the game will likely stop with an illegal instruction");
    }
}

fn wanted(need: u32, m: Mnemonic) -> bool {
    match m {
        Mnemonic::Andn
        | Mnemonic::Bextr
        | Mnemonic::Blsi
        | Mnemonic::Blsmsk
        | Mnemonic::Blsr
        | Mnemonic::Tzcnt => need & NEED_BMI1 != 0,
        Mnemonic::Lzcnt => need & NEED_LZCNT != 0,
        Mnemonic::Movbe => need & NEED_MOVBE != 0,
        _ => false,
    }
}

/// Thunk memory: within rel32 reach of the whole image.
fn near_image(image: &Image, size: usize) -> Option<u64> {
    let base = image.addr();
    let mut at = (base + image.size() + (48 << 20) + 0xffff) & !0xffff;
    while at + size as u64 - base < 0x7ff0_0000 {
        if host::map(at, size, true).is_some() {
            return Some(at);
        }
        at += 1 << 16;
    }
    None
}

/// Room for one region's thunk; `thunk_commit` keeps what it used.
fn thunk_reserve(st: &mut State) -> Option<u64> {
    if st.chunk_used + REGION_ITEMS * ITEM_MAX + 16 > CHUNK {
        let image = st.image?;
        st.chunk = near_image(&image, CHUNK)?;
        st.chunk_used = 0;
    }
    Some(st.chunk + st.chunk_used as u64)
}

fn thunk_commit(st: &mut State, used: usize) {
    st.chunk_used += (used + 15) & !15;
}

/// x86-64 encoding into a buffer that will live at `base`. Registers are numbered as in ModRM
/// (rax 0 ... r15 15).
struct Code {
    base: u64,
    p: Vec<u8>,
}

impl Code {
    fn new(base: u64) -> Code {
        Code {
            base,
            p: Vec::with_capacity(REGION_ITEMS * ITEM_MAX),
        }
    }

    fn here(&self) -> u64 {
        self.base + self.p.len() as u64
    }

    fn b(&mut self, v: u32) {
        self.p.push(v as u8);
    }

    fn bytes(&mut self, v: &[u8]) {
        self.p.extend_from_slice(v);
    }

    fn d32(&mut self, v: u32) {
        self.p.extend_from_slice(&v.to_le_bytes());
    }

    fn d64(&mut self, v: u64) {
        self.p.extend_from_slice(&v.to_le_bytes());
    }

    fn prefixes(&mut self, w: i32, r: i32, x: i32, base: i32) {
        if w == 16 {
            self.b(0x66);
        }
        let rex = 0x40u32
            | if w == 64 { 8 } else { 0 }
            | ((r >> 3 & 1) as u32) << 2
            | ((x >> 3 & 1) as u32) << 1
            | (base >> 3 & 1) as u32;
        if rex != 0x40 {
            self.b(rex);
        }
    }

    /// `op` with ModRM register form; `reg` is a register or an opcode extension.
    fn rr(&mut self, w: i32, op: &[u8], reg: i32, rm: i32) {
        self.prefixes(w, reg, 0, rm);
        self.bytes(op);
        self.b(0xc0 | ((reg & 7) as u32) << 3 | (rm & 7) as u32);
    }

    /// `op` with a memory operand; `adjust` is added to an rsp base (pushes before it), `imm` is
    /// the number of immediate bytes that follow (RIP-relative displacements count from the end).
    fn rm(&mut self, w: i32, op: &[u8], reg: i32, m: &CpuPatchMem, adjust: i64, imm: i64) -> bool {
        let (base, index) = (m.base, m.index);
        let disp = m.disp + if base == 4 { adjust } else { 0 };
        self.prefixes(w, reg, index.max(0), base.max(0));
        self.bytes(op);
        if m.rip != 0 {
            self.b(((reg & 7) as u32) << 3 | 5);
            let rel = m.absolute as i64 - (self.here() as i64 + 4 + imm);
            let Ok(rel) = i32::try_from(rel) else {
                return false;
            };
            self.d32(rel as u32);
            return true;
        }
        let Ok(disp32) = i32::try_from(disp) else {
            return false;
        };
        let sib = index >= 0 || base < 0 || (base & 7) == 4;
        let mod_ = if base < 0 || (disp == 0 && (base & 7) != 5) {
            0
        } else if i8::try_from(disp).is_ok() {
            1
        } else {
            2
        };
        self.b(mod_ << 6 | ((reg & 7) as u32) << 3 | if sib { 4 } else { (base & 7) as u32 });
        if sib {
            let scale = match m.scale {
                8 => 3,
                4 => 2,
                2 => 1,
                _ => 0,
            };
            self.b(scale << 6
                | ((if index >= 0 { index } else { 4 } & 7) as u32) << 3
                | if base < 0 { 5 } else { (base & 7) as u32 });
        }
        if mod_ == 1 {
            self.b((disp & 0xff) as u32);
        } else if mod_ == 2 || base < 0 {
            self.d32(disp32 as u32);
        }
        true
    }

    fn jump8(&mut self, op: u32) -> usize {
        self.b(op);
        self.b(0);
        self.p.len() - 1
    }

    fn land(&mut self, at: usize) {
        self.p[at] = (self.p.len() - at - 1) as i8 as u8;
    }

    fn mov_imm(&mut self, w: i32, r: i32, v: i64) {
        if w == 64 && v < 0 {
            self.prefixes(64, 0, 0, r);
            self.b(0xc7);
            self.b(0xc0 | (r & 7) as u32);
            self.d32(v as u32);
            return;
        }
        self.prefixes(if w == 16 { 16 } else { 32 }, 0, 0, r);
        self.b(0xb8 | (r & 7) as u32);
        if w == 16 {
            self.b((v & 0xff) as u32);
            self.b((v >> 8 & 0xff) as u32);
        } else {
            self.d32(v as u32);
        }
    }

    fn push(&mut self, r: i32) {
        if r >= 8 {
            self.b(0x41);
        }
        self.b(0x50 + (r & 7) as u32);
    }

    fn pop(&mut self, r: i32) {
        if r >= 8 {
            self.b(0x41);
        }
        self.b(0x58 + (r & 7) as u32);
    }

    fn bswap(&mut self, w: i32, r: i32) {
        self.prefixes(w, 0, 0, r);
        self.b(0x0f);
        self.b(0xc8 + (r & 7) as u32);
    }

    /// A scratch register outside the red zone: lea rsp, [rsp-128]; push t ... pop t; lea rsp,
    /// [rsp+128] (neither touches the flags). Memory operands based on rsp move by `SAVED`.
    fn save(&mut self, t: i32) {
        self.bytes(&[0x48, 0x8d, 0x64, 0x24, 0x80]);
        self.push(t);
    }

    fn restore(&mut self, t: i32) {
        self.pop(t);
        self.bytes(&[0x48, 0x8d, 0xa4, 0x24, 0x80, 0, 0, 0]);
    }
}

const SAVED: i64 = 136;

fn scratch(op: &CpuPatchOp) -> i32 {
    let mut used = 1u32 << 4;
    for r in op.reg {
        if r >= 0 {
            used |= 1 << r;
        }
    }
    if op.mem.base >= 0 {
        used |= 1 << op.mem.base;
    }
    if op.mem.index >= 0 {
        used |= 1 << op.mem.index;
    }
    (0..16).find(|r| used >> r & 1 == 0).unwrap_or(-1)
}

/// LZCNT/TZCNT: BSR/BSF (ZF when the source is 0, the destination kept), then the count and its
/// flags (ZF for a zero result, CF for a zero source).
fn gen_count(c: &mut Code, op: &CpuPatchOp) -> bool {
    let (w, d, s) = (op.width, op.reg[0], op.reg[1]);
    let scan: &[u8] = if op.kind == LZCNT {
        &[0x0f, 0xbd]
    } else {
        &[0x0f, 0xbc]
    };
    if s >= 0 {
        c.rr(w, scan, d, s);
    } else if !c.rm(w, scan, d, &op.mem, 0, 0) {
        return false;
    }
    let zero = c.jump8(0x74);
    if op.kind == LZCNT {
        c.rr(w, &[0x83], 6, d); // xor d, w-1
        c.b((w - 1) as u32);
    } else {
        c.rr(w, &[0x85], d, d); // test d, d
    }
    let done = c.jump8(0xeb);
    c.land(zero);
    c.mov_imm(w, d, w as i64);
    c.rr(w, &[0x83], 7, d); // cmp d, w+1: CF, not ZF
    c.b((w + 1) as u32);
    c.land(done);
    true
}

/// ANDN d, s1, s2: d = ~s1 & s2; SF, ZF from the result, CF and OF clear (AND's flags).
fn gen_andn(c: &mut Code, op: &CpuPatchOp) -> bool {
    let (w, d, s1) = (op.width, op.reg[0], op.reg[1]);
    let mut s2 = op.reg[2];
    let mut t = -1;
    if s2 < 0 {
        t = scratch(op);
        c.save(t);
        if !c.rm(w, &[0x8b], t, &op.mem, SAVED, 0) {
            return false; // mov t, [m]
        }
        s2 = t;
    }
    if d == s1 && d == s2 {
        c.rr(32, &[0x31], d, d); // xor d, d
    } else if d == s2 {
        // not s1; and d, s1; not s1
        c.rr(64, &[0xf7], 2, s1);
        c.rr(w, &[0x21], s1, d);
        c.rr(64, &[0xf7], 2, s1);
    } else {
        // mov d, s1; not d; and d, s2
        if d != s1 {
            c.rr(w, &[0x89], s1, d);
        }
        c.rr(w, &[0xf7], 2, d);
        c.rr(w, &[0x21], s2, d);
    }
    if t >= 0 {
        c.restore(t);
    }
    true
}

/// BLSR (s & (s-1)), BLSMSK (s ^ (s-1)), BLSI (s & -s); CF: BLSR, BLSMSK for a zero source, BLSI
/// for a nonzero one; SF and ZF from the result (BLSMSK's ZF is always clear); OF clear.
fn gen_bls(c: &mut Code, op: &CpuPatchOp) -> bool {
    let (w, d) = (op.width, op.reg[0]);
    let mut s = op.reg[1];
    let mut t = -1;
    if s < 0 || s == d {
        t = scratch(op);
        c.save(t);
        if s < 0 {
            if !c.rm(w, &[0x8b], t, &op.mem, SAVED, 0) {
                return false;
            }
        } else {
            c.rr(64, &[0x89], s, t);
        }
        s = t;
    }
    let minus_one = CpuPatchMem {
        base: s,
        index: -1,
        scale: 1,
        rip: 0,
        disp: -1,
        absolute: 0,
    };
    c.rr(w, &[0x85], s, s); // test s, s
    let zero = c.jump8(0x74);
    if op.kind == BLSI {
        c.rr(w, &[0x89], s, d);
        c.rr(w, &[0xf7], 3, d);
        c.rr(w, &[0x21], s, d);
        c.b(0xf9);
    } else {
        c.rm(w, &[0x8d], d, &minus_one, 0, 0); // lea d, [s-1]
        c.rr(w, if op.kind == BLSR { &[0x21] } else { &[0x31] }, s, d);
    }
    let done = c.jump8(0xeb);
    c.land(zero);
    if op.kind == BLSMSK {
        c.mov_imm(w, d, -1);
        c.rr(w, &[0x85], d, d);
        c.b(0xf9);
    } else {
        c.rr(32, &[0x31], d, d);
        if op.kind == BLSR {
            c.b(0xf9);
        }
    }
    c.land(done);
    if t >= 0 {
        c.restore(t);
    }
    true
}

/// MOVBE: mov and bswap, no flags.
fn gen_movbe(c: &mut Code, op: &CpuPatchOp) -> bool {
    let w = op.width;
    if op.kind == MOVBE_LOAD {
        if !c.rm(w, &[0x8b], op.reg[0], &op.mem, 0, 0) {
            return false;
        }
        c.bswap(w, op.reg[0]);
        return true;
    }
    let t = scratch(op);
    c.save(t);
    c.rr(64, &[0x89], op.reg[1], t);
    c.bswap(w, t);
    if !c.rm(w, &[0x89], t, &op.mem, SAVED, 0) {
        return false;
    }
    c.restore(t);
    true
}

/// BEXTR and 16-bit MOVBE run in Rust: the thunk saves the registers and flags as {rax ... r15,
/// rflags} (the rsp slot unused), xmm0-5 and calls this with the Microsoft x64 convention.
fn load(address: u64, w: i32) -> u64 {
    let mut v = [0u8; 8];
    // SAFETY: the guest's own memory operand, read as the instruction would have.
    unsafe {
        std::ptr::copy_nonoverlapping(
            address as usize as *const u8,
            v.as_mut_ptr(),
            (w / 8) as usize,
        );
    }
    u64::from_le_bytes(v)
}

fn swap(v: u64, w: i32) -> u64 {
    match w {
        16 => u64::from((v as u16).swap_bytes()),
        32 => u64::from((v as u32).swap_bytes()),
        _ => v.swap_bytes(),
    }
}

/// # Safety
/// Called only by the thunks [`gen_call`] writes: `regs` is their register save area (17 qwords,
/// the guest's rsp 128 bytes plus the area above it) and `op` the leaked [`CpuPatchOp`].
unsafe extern "win64" fn emulate(regs: *mut u64, op: *const CpuPatchOp) {
    // SAFETY: see above.
    let (regs, op) = unsafe { (std::slice::from_raw_parts_mut(regs, 17), &*op) };
    let rsp = regs.as_ptr() as usize as u64 + 17 * 8 + 128;
    let w = op.width;
    let mask = if w == 64 { u64::MAX } else { (1u64 << w) - 1 };
    let m = &op.mem;
    let mut address = if m.rip != 0 {
        m.absolute
    } else {
        m.disp as u64
    };
    if m.rip == 0 && m.base >= 0 {
        address = address.wrapping_add(if m.base == 4 {
            rsp
        } else {
            regs[m.base as usize]
        });
    }
    if m.rip == 0 && m.index >= 0 {
        address = address.wrapping_add(regs[m.index as usize].wrapping_mul(m.scale as u64));
    }
    let reg = |i: usize| op.reg[i] as usize;
    if op.kind == BEXTR {
        let source = if op.reg[1] >= 0 {
            regs[reg(1)]
        } else {
            load(address, w)
        } & mask;
        let control = regs[reg(2)];
        let start = (control & 0xff) as u32;
        let length = (control >> 8 & 0xff) as u32;
        let mut r = if start >= w as u32 {
            0
        } else {
            source >> start
        };
        if length < w as u32 {
            r &= (1u64 << length) - 1;
        }
        regs[reg(0)] = r;
        regs[16] = (regs[16] & !(CF | ZF | OF)) | if r != 0 { 0 } else { ZF };
    } else if op.kind == MOVBE_LOAD {
        let v = swap(load(address, w), w);
        regs[reg(0)] = if w == 16 {
            (regs[reg(0)] & !0xffff) | v
        } else {
            v
        };
    } else {
        let v = swap(regs[reg(1)] & mask, w).to_le_bytes();
        // SAFETY: the guest's own memory operand, written as the instruction would have.
        unsafe {
            std::ptr::copy_nonoverlapping(
                v.as_ptr(),
                address as usize as *mut u8,
                (w / 8) as usize,
            );
        }
    }
}

fn gen_call(c: &mut Code, op: &CpuPatchOp) -> bool {
    // The thunk keeps the operation for as long as the process runs.
    let copy = Box::into_raw(Box::new(*op)) as usize as u64;
    const ENTER: [u8; 44] = [
        0x48, 0x8d, 0x64, 0x24, 0x80, 0x9c, // lea rsp, [rsp-128]; pushfq
        0x41, 0x57, 0x41, 0x56, 0x41, 0x55, 0x41, 0x54, 0x41, 0x53, 0x41, 0x52, 0x41, 0x51, 0x41,
        0x50, // push r15 ... r8
        0x57, 0x56, 0x55, 0x54, 0x53, 0x52, 0x51, 0x50, // push rdi ... rax
        0x48, 0x89, 0xe3, 0x48, 0x83, 0xe4, 0xf0, 0x48, 0x81, 0xec, 0x80, 0, 0,
        0, // mov rbx, rsp; and rsp, -16; sub rsp, 0x80
    ];
    const LEAVE: [u8; 40] = [
        0x48, 0x89, 0xdc, 0x58, 0x59, 0x5a, 0x5b, // mov rsp, rbx; pop rax rcx rdx rbx
        0x48, 0x8d, 0x64, 0x24, 0x08, 0x5d, 0x5e, 0x5f, // lea rsp, [rsp+8]; pop rbp rsi rdi
        0x41, 0x58, 0x41, 0x59, 0x41, 0x5a, 0x41, 0x5b, 0x41, 0x5c, 0x41, 0x5d, 0x41, 0x5e, 0x41,
        0x5f, // pop r8 ... r15
        0x9d, 0x48, 0x8d, 0xa4, 0x24, 0x80, 0, 0, 0, // popfq; lea rsp, [rsp+128]
    ];
    c.bytes(&ENTER);
    for x in 0..6u32 {
        c.bytes(&[0xf3, 0x0f, 0x7f, (0x84 | x << 3) as u8, 0x24]);
        c.d32(0x20 + 16 * x);
    }
    c.bytes(&[0x48, 0x89, 0xd9]); // mov rcx, rbx
    c.bytes(&[0x48, 0xba]); // movabs rdx, op
    c.d64(copy);
    c.bytes(&[0x48, 0xb8]); // movabs rax, emulate
    c.d64(emulate as *const () as usize as u64);
    c.bytes(&[0xff, 0xd0]); // call rax
    for x in 0..6u32 {
        c.bytes(&[0xf3, 0x0f, 0x6f, (0x84 | x << 3) as u8, 0x24]);
        c.d32(0x20 + 16 * x);
    }
    c.bytes(&LEAVE);
    true
}

/// A decoded instruction and whether it is one to rewrite.
#[derive(Clone)]
struct Item {
    d: Decoded,
    rewrite: bool,
}

impl Item {
    fn len(&self) -> u64 {
        self.d.len() as u64
    }
}

/// A 16, 32 or 64-bit register that is not rsp: its number. rsp operands are left alone.
fn gpr(r: Register) -> Option<i32> {
    if !(r.is_gpr16() || r.is_gpr32() || r.is_gpr64()) {
        return None;
    }
    let id = r.number() as i32;
    (id != 4).then_some(id)
}

fn memory(d: &Decoded) -> Option<CpuPatchMem> {
    let ins = &d.ins;
    if matches!(ins.segment_prefix(), Register::FS | Register::GS) {
        return None;
    }
    let mut m = CpuPatchMem {
        base: -1,
        index: -1,
        scale: 1,
        ..CpuPatchMem::default()
    };
    if ins.is_ip_rel_memory_operand() {
        m.rip = 1;
        m.absolute = ins.memory_displacement64();
        m.disp = m.absolute.wrapping_sub(d.at + d.len() as u64) as i64;
        return Some(m);
    }
    if ins.memory_base() != Register::None {
        if !ins.memory_base().is_gpr64() {
            return None;
        }
        m.base = ins.memory_base().number() as i32;
    }
    if ins.memory_index() != Register::None {
        if !ins.memory_index().is_gpr64() {
            return None;
        }
        m.index = ins.memory_index().number() as i32;
    }
    if ins.memory_index_scale() != 0 {
        m.scale = ins.memory_index_scale() as i32;
    }
    m.disp = ins.memory_displacement64() as i64;
    Some(m)
}

fn describe(d: &Decoded) -> Option<CpuPatchOp> {
    let ins = &d.ins;
    let (kind, operands) = match ins.mnemonic() {
        Mnemonic::Lzcnt => (LZCNT, 2u32),
        Mnemonic::Tzcnt => (TZCNT, 2u32),
        Mnemonic::Andn => (ANDN, 3u32),
        Mnemonic::Blsr => (BLSR, 2u32),
        Mnemonic::Blsmsk => (BLSMSK, 2u32),
        Mnemonic::Blsi => (BLSI, 2u32),
        Mnemonic::Bextr => (BEXTR, 3u32),
        Mnemonic::Movbe => (MOVBE_LOAD, 2u32),
        _ => return None,
    };
    if ins.op_count() != operands {
        return None;
    }
    let mut op = CpuPatchOp {
        kind,
        reg: [-1; 3],
        ..CpuPatchOp::default()
    };
    op.mem.base = -1;
    op.mem.index = -1;
    let width = (0..operands)
        .find(|&i| ins.op_kind(i) == OpKind::Register)
        .map(|i| ins.op_register(i).size() as i32 * 8)?;
    op.width = width;
    if !(width == 32 || width == 64 || (width == 16 && (kind <= TZCNT || kind == MOVBE_LOAD))) {
        return None;
    }
    let mut memory_at = None;
    for i in 0..operands {
        match ins.op_kind(i) {
            OpKind::Register => op.reg[i as usize] = gpr(ins.op_register(i))?,
            OpKind::Memory if memory_at.is_none() => {
                op.mem = memory(d)?;
                memory_at = Some(i);
            }
            _ => return None,
        }
    }
    if memory_at.is_none() {
        op.mem.base = -1;
        op.mem.index = -1;
    }
    let mut kind = kind;
    if kind == MOVBE_LOAD && memory_at == Some(0) {
        kind = MOVBE_STORE;
    }
    op.kind = kind;
    (op.reg[0] >= 0 || kind == MOVBE_STORE).then_some(op)
}

fn decode(image: &Image, need: u32, offset: u64, end: u64) -> Option<Item> {
    let d = patch_util::decode(image, offset, end)?;
    let rewrite = wanted(need, d.ins.mnemonic());
    let ok = if rewrite {
        describe(&d).is_some()
    } else {
        patch_util::movable(&d.ins)
    };
    ok.then_some(Item { d, rewrite })
}

fn emit_item(st: &mut State, c: &mut Code, it: &Item) -> bool {
    if !it.rewrite {
        let Some(code) = it.d.relocated(c.here()) else {
            return false;
        };
        c.bytes(&code);
        return true;
    }
    let Some(op) = describe(&it.d) else {
        return false;
    };
    let ok = match op.kind {
        LZCNT | TZCNT => gen_count(c, &op),
        ANDN => gen_andn(c, &op),
        BLSR | BLSMSK | BLSI => gen_bls(c, &op),
        MOVBE_LOAD | MOVBE_STORE if op.width != 16 => gen_movbe(c, &op),
        _ => gen_call(c, &op),
    };
    if ok {
        st.rewritten[op.kind as usize] += 1;
    }
    ok
}

/// The thunk for `items`: their code, then a jump to the end of the last. Returns the thunk and
/// the thunk addresses of the items. None when something cannot be encoded.
fn emit_thunk(st: &mut State, items: &[Item]) -> Option<(u64, Vec<u64>)> {
    let t = thunk_reserve(st)?;
    let mut c = Code::new(t);
    let saved = st.rewritten;
    let mut starts = Vec::with_capacity(items.len());
    for it in items {
        starts.push(c.here());
        if !emit_item(st, &mut c, it) {
            st.rewritten = saved;
            return None;
        }
    }
    let last = items.last()?;
    let back = last.d.at + last.len();
    let rel = back as i64 - (c.here() as i64 + 5);
    let fits = i32::try_from(rel);
    let room = CHUNK - st.chunk_used;
    let (Ok(rel), true) = (fits, c.p.len() + 5 <= room) else {
        st.rewritten = saved;
        return None;
    };
    c.b(0xe9);
    c.d32(rel as u32);
    patch_util::write_code(t, &c.p);
    thunk_commit(st, c.p.len());
    Some((t, starts))
}

#[derive(Clone, Copy)]
struct Site {
    offset: u64,
    previous: [u64; 2],
}

/// Rewrites the instructions this CPU lacks in the executable ranges (`{offset, size}` in the
/// image) before guest code runs; the image must still be writable. `BB_CPU_PATCH=0` turns it
/// off, `BB_CPU_PATCH=all` rewrites every supported instruction (tests on a CPU that has them).
///
/// # Safety
/// `image` must be `image_size` bytes of readable and writable memory and `ranges` must point at
/// `count` `{offset, size}` pairs.
#[no_mangle]
pub unsafe extern "C" fn cpu_patch_image(
    image: *mut u8,
    image_size: u64,
    ranges: *const [u64; 2],
    count: usize,
) {
    if image.is_null() {
        return;
    }
    let ranges: &[[u64; 2]] = if ranges.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: the caller's contract.
        unsafe { std::slice::from_raw_parts(ranges, count) }
    };
    // SAFETY: the caller's contract.
    let image = unsafe { Image::new(image, image_size) };
    patch_image(image, ranges);
}

fn patch_image(image: Image, ranges: &[[u64; 2]]) {
    if mode_is("0") {
        return;
    }
    let missing = host_missing();
    let all = mode_is("all");
    let need = if all {
        NEED_BMI1 | NEED_LZCNT | NEED_MOVBE
    } else {
        missing
    };
    if all {
        println!("CPU: BB_CPU_PATCH=all: rewriting BMI1, LZCNT and MOVBE instructions");
    }
    report_unsupported();
    let mut st = lock();
    st.image = Some(image);
    if need == 0 {
        return;
    }
    let size = image.size();
    let count = ranges.len();
    // Pass 1: direct branch targets and the instructions to rewrite (with the two before them).
    let mut targets = Bits::new(size);
    let mut sites: Vec<Site> = Vec::new();
    for range in ranges {
        let end = range[0].saturating_add(range[1]).min(size);
        let mut previous = [u64::MAX; 2];
        let mut at = range[0];
        while at < end {
            let Some(d) = patch_util::decode(&image, at, end) else {
                previous = [u64::MAX; 2];
                at += 1;
                continue;
            };
            if let Some(target) = patch_util::direct_branch(&d.ins) {
                let target = target.wrapping_sub(image.addr());
                if target < size {
                    targets.set(target);
                }
            }
            if wanted(need, d.ins.mnemonic()) {
                sites.push(Site {
                    offset: at,
                    previous,
                });
            }
            previous[1] = previous[0];
            previous[0] = at;
            at += d.len() as u64;
        }
    }
    // Pass 2: a jump to a thunk over 5+ bytes (the instruction, else with its neighbours), else
    // a breakpoint. Sites are in segment order; `covered` ends the last rewritten region.
    let (mut jumps, mut moved, mut traps, mut left) = (0u32, 0u32, 0u32, 0u32);
    let mut covered = 0u64;
    let mut r = 0usize;
    let mut breakpoints: Vec<(u64, u64)> = Vec::new();
    for (i, site) in sites.iter().enumerate() {
        let o = site.offset;
        let mut tried = 0;
        while tried < count && !(o >= ranges[r][0] && o - ranges[r][0] < ranges[r][1]) {
            r = (r + 1) % count;
            tried += 1;
        }
        if tried == count {
            continue;
        }
        if i > 0 && sites[i - 1].offset > o {
            covered = 0; // next segment
        }
        if o < covered {
            continue;
        }
        let start = ranges[r][0];
        let end = start.saturating_add(ranges[r][1]).min(size);
        let Some(first_item) = decode(&image, need, o, end) else {
            left += 1;
            continue;
        };
        let mut length = first_item.len();
        let mut items = vec![first_item];
        let mut first = o;
        // Short: the following instructions, else the one or two before.
        let mut k = o + length;
        while length < 5 && items.len() < REGION_ITEMS && k < end && !targets.get(k) {
            let Some(next) = decode(&image, need, k, end) else {
                break;
            };
            length += next.len();
            k += next.len();
            items.push(next);
        }
        if length < 5 {
            let site_item = items[0].clone();
            let (p0, p1) = (site.previous[0], site.previous[1]);
            length = site_item.len();
            items = vec![site_item.clone()];
            first = o;
            if !targets.get(o) && p0 != u64::MAX && p0 >= covered && p0 >= start {
                if let Some(b1) = decode(&image, need, p0, end) {
                    length += b1.len();
                    let b0 = if length < 5
                        && !targets.get(p0)
                        && p1 != u64::MAX
                        && p1 >= covered
                        && p1 >= start
                    {
                        decode(&image, need, p1, end)
                    } else {
                        None
                    };
                    if let Some(b0) = b0 {
                        length += b0.len();
                        items = vec![b0, b1, site_item.clone()];
                        first = p1;
                    } else {
                        items = vec![b1, site_item.clone()];
                        first = p0;
                    }
                }
            }
            if length < 5 {
                items = vec![site_item];
                first = o;
                length = items[0].len();
            }
        }
        let Some((thunk, starts)) = emit_thunk(&mut st, &items) else {
            left += 1;
            continue;
        };
        if length < 5 {
            breakpoints.push((image.addr() + o, starts[0]));
            image.write(o, &[0xcc]);
            traps += 1;
        } else {
            let f = image.addr() + first;
            let to = thunk as i64 - (f as i64 + 5);
            for (item, start) in items.iter().zip(&starts) {
                if item.d.at >= f + 5 {
                    breakpoints.push((item.d.at, *start));
                }
            }
            let mut patch = vec![0xcc; length as usize];
            patch[0] = 0xe9;
            patch[1..5].copy_from_slice(&(to as i32).to_le_bytes());
            image.write(first, &patch);
            jumps += 1;
            moved += (items.len() - 1) as u32;
        }
        covered = first + length;
    }
    {
        let mut known = BREAKPOINTS.write().unwrap_or_else(PoisonError::into_inner);
        known.extend(breakpoints);
        known.sort_unstable_by_key(|b| b.0);
    }
    let rewritten = st.rewritten;
    let total: u32 = rewritten.iter().sum();
    let mut kinds = String::new();
    for k in 0..KINDS {
        if rewritten[k] != 0 && (k as i32 != MOVBE_STORE || rewritten[MOVBE_LOAD as usize] == 0) {
            let n = if k as i32 == MOVBE_LOAD {
                rewritten[k] + rewritten[MOVBE_STORE as usize]
            } else {
                rewritten[k]
            };
            if !kinds.is_empty() {
                kinds.push_str(", ");
            }
            kinds.push_str(&format!("{} {n}", KIND_NAMES[k]));
        }
    }
    println!(
        "CPU: {}{}{}{}: {total} instructions rewritten ({}); {jumps} jumps with {moved} moved neighbours, {traps} breakpoints, {left} left as they were",
        if missing != 0 {
            "this processor lacks the PS4's"
        } else {
            "rewrite forced"
        },
        if missing & NEED_BMI1 != 0 { " BMI1" } else { "" },
        if missing & NEED_LZCNT != 0 { " LZCNT" } else { "" },
        if missing & NEED_MOVBE != 0 { " MOVBE" } else { "" },
        if kinds.is_empty() { "none" } else { &kinds },
    );
}

/// Exception handler for a breakpoint the rewrite put at `address`: 1 when handled (`*rip` moved
/// to its thunk).
///
/// # Safety
/// `rip` must be valid for reads and writes.
#[no_mangle]
pub unsafe extern "C" fn cpu_patch_breakpoint(rip: *mut u64, address: u64) -> c_int {
    if rip.is_null() {
        return 0;
    }
    let all = BREAKPOINTS.read().unwrap_or_else(PoisonError::into_inner);
    let low = all.partition_point(|b| b.0 < address);
    match all.get(low) {
        Some(&(a, thunk)) if a == address => {
            // SAFETY: the caller's contract.
            unsafe { *rip = thunk };
            1
        }
        _ => 0,
    }
}

/// Exception handler for an illegal instruction at `*rip` in the image that the scan did not
/// reach: 1 when handled (`*rip` moved to a thunk).
///
/// # Safety
/// `rip` must be valid for reads and writes.
#[no_mangle]
pub unsafe extern "C" fn cpu_patch_illegal(rip: *mut u64) -> c_int {
    if rip.is_null() {
        return 0;
    }
    // SAFETY: the caller's contract.
    let at = unsafe { *rip };
    match illegal(at) {
        Some(thunk) => {
            // SAFETY: the caller's contract.
            unsafe { *rip = thunk };
            1
        }
        None => 0,
    }
}

fn illegal(at: u64) -> Option<u64> {
    let mut st = lock();
    let image = st.image?;
    if at.wrapping_sub(image.addr()) >= image.size() || mode_is("0") {
        return None;
    }
    if let Some(&(_, thunk)) = st.missed.iter().find(|m| m.0 == at) {
        return Some(thunk);
    }
    let offset = at - image.addr();
    let every = NEED_BMI1 | NEED_LZCNT | NEED_MOVBE;
    let item = decode(&image, every, offset, image.size()).filter(|item| item.rewrite)?;
    let (thunk, starts) = emit_thunk(&mut st, std::slice::from_ref(&item))?;
    debug_assert_eq!(thunk, starts[0]);
    let thunk = starts[0];
    st.missed.push((at, thunk));
    println!(
        "CPU: {} at guest offset {offset:#x} runs from a thunk (not found by the scan)",
        format!("{:?}", item.d.ins.mnemonic()).to_lowercase()
    );
    let _ = std::io::stdout().flush();
    Some(thunk)
}

#[cfg(test)]
mod tests;
