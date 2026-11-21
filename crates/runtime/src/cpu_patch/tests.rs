//! `cpu_patch` on a CPU that has BMI1, LZCNT and MOVBE (the port of `tests/test_cpu_patch.c`):
//! each case runs natively, then rewritten (`BB_CPU_PATCH=all`), from the same registers, flags
//! and memory; the results must match in every register, the defined flags and memory.
//! x86-64 Linux and Windows.

use super::*;
use crate::patch_util::host;
use std::sync::atomic::{AtomicU32, Ordering};

/// Instruction bytes from the assembler: `Snip` names a start and an end in `cpt_table`.
macro_rules! snippets {
    ($($name:ident = $text:literal;)*) => {
        #[allow(non_camel_case_types, dead_code, clippy::enum_variant_names)]
        #[derive(Clone, Copy)]
        enum Snip { $($name),* }
        std::arch::global_asm!(
            ".pushsection .data",
            $( concat!("cpt_", stringify!($name), ":"), $text, concat!("cpt_", stringify!($name), "_end:"), )*
            ".p2align 3",
            ".globl cpt_table",
            "cpt_table:",
            $( concat!(".quad cpt_", stringify!($name), ", cpt_", stringify!($name), "_end"), )*
            ".popsection",
            options(att_syntax)
        );
    };
}

snippets! {
    none = "";
    jmp0 = "jmp 1f\n1:";
    nop1 = "nop";
    push2 = "push $0x7a5a3c1e\npush $-0x2b4c6d8f";
    pop2 = "lea 16(%rsp),%rsp";
    load_rip = "mov 0(%rip),%r8";
    lz32 = "lzcnt %ecx,%eax";
    lz64 = "lzcnt %rcx,%rax";
    lz_same = "lzcnt %eax,%eax";
    lz16 = "lzcnt %cx,%ax";
    lz_mem = "lzcnt (%r12),%esi";
    lz_rip = "lzcnt 0(%rip),%r9";
    lz_pair = "lzcnt %ecx,%eax\nlzcnt %edx,%ebx";
    lz_high = "lzcnt %r14d,%r15d";
    tz32 = "tzcnt %ecx,%eax";
    tz64 = "tzcnt %r8,%r9";
    tz16 = "tzcnt %si,%di";
    tz_mem = "tzcnt 8(%r12,%r13,8),%rbx";
    tz_rsp = "tzcnt 8(%rsp),%rbp";
    andn64 = "andn %rsi,%rdi,%rax";
    andn32 = "andn %esi,%edi,%eax";
    andn_d_s2 = "andn %rax,%rdi,%rax";
    andn_d_s1 = "andn %esi,%eax,%eax";
    andn_all = "andn %eax,%eax,%eax";
    andn_hi32 = "andn %r10d,%r11d,%r10d";
    andn_mem = "andn (%r12),%rdi,%rax";
    andn_mem_d_s1 = "andn 16(%r12),%rax,%rax";
    andn_mem_index = "andn (%r12,%r13,8),%r13,%r13";
    andn_rsp = "andn 8(%rsp),%rbx,%rcx";
    andn_rip = "andn 0(%rip),%rbx,%rdx";
    blsr64 = "blsr %rsi,%rax";
    rdtsc_plain = "rdtsc";
    blsr_same = "blsr %eax,%eax";
    blsr_mem = "blsr (%r12),%rdx";
    blsr_index = "blsr 4(%r12,%r13,4),%r13d";
    blsmsk64 = "blsmsk %rsi,%rax";
    blsmsk_same = "blsmsk %eax,%eax";
    blsmsk_mem = "blsmsk (%r12),%edx";
    blsi64 = "blsi %rsi,%rax";
    blsi_same = "blsi %r9d,%r9d";
    blsi_mem = "blsi 8(%r12),%r11";
    bextr64 = "bextr %rdx,%rsi,%rax";
    bextr32_mem = "bextr %ecx,(%r12),%eax";
    bextr_same = "bextr %eax,%eax,%eax";
    movbe_l32 = "movbe (%r12),%eax";
    movbe_l64 = "movbe (%r12),%rax";
    movbe_l16 = "movbe (%r12),%ax";
    movbe_s32 = "movbe %ecx,8(%r12)";
    movbe_s64 = "movbe %rcx,(%r12,%r13,8)";
    movbe_s16 = "movbe %cx,(%r12)";
    movbe_rip = "movbe 0(%rip),%r10";
    movbe_index = "movbe %r13,(%r12,%r13,8)";
}

extern "C" {
    static cpt_table: [usize; 2];
}

fn snip(s: Snip) -> &'static [u8] {
    // SAFETY: `cpt_table` holds a start and an end address per snippet, in `Snip` order, into
    // the program's data.
    unsafe {
        let table = std::ptr::addr_of!(cpt_table) as *const usize;
        let start = *table.add(2 * s as usize) as *const u8;
        let end = *table.add(2 * s as usize + 1) as *const u8;
        std::slice::from_raw_parts(start, end.offset_from(start) as usize)
    }
}

const CFLAG: u64 = 0x1;
const PF: u64 = 0x4;
const AF: u64 = 0x10;
const ZFLAG: u64 = 0x40;
const SF: u64 = 0x80;
const OFLAG: u64 = 0x800;
const ALL: u64 = CFLAG | PF | AF | ZFLAG | SF | OFLAG;
const COUNT: u64 = CFLAG | ZFLAG;
const LOGIC: u64 = SF | ZFLAG | CFLAG | OFLAG;
const BEXTR_FLAGS: u64 = ZFLAG | CFLAG | OFLAG;

/// `expect`: J jump at the instruction, P jump at the one before, B breakpoint, - any. `rip`:
/// the last instruction of the body (1) or of post (2) reads the data at +16 RIP-relative.
struct Case {
    name: &'static str,
    pre: Snip,
    body: Snip,
    post: Snip,
    flags: u64,
    expect: char,
    rip: usize,
}

use Snip::*;
const fn case(
    name: &'static str,
    pre: Snip,
    body: Snip,
    post: Snip,
    flags: u64,
    expect: char,
    rip: usize,
) -> Case {
    Case {
        name,
        pre,
        body,
        post,
        flags,
        expect,
        rip,
    }
}

const CASES: &[Case] = &[
    case("lzcnt r32", none, lz32, none, COUNT, 'J', 0),
    case("lzcnt r64", none, lz64, none, COUNT, 'J', 0),
    case("lzcnt same", none, lz_same, none, COUNT, 'J', 0),
    case("lzcnt r16", none, lz16, none, COUNT, 'J', 0),
    case("lzcnt mem", none, lz_mem, none, COUNT, 'J', 0),
    case("lzcnt rip", none, lz_rip, none, COUNT, 'J', 1),
    case("lzcnt pair", none, lz_pair, none, COUNT, 'J', 0),
    case("lzcnt r14d", none, lz_high, none, COUNT, 'J', 0),
    case("lzcnt breakpoint", jmp0, lz32, jmp0, COUNT, 'B', 0),
    case("lzcnt previous", nop1, lz32, jmp0, COUNT, 'P', 0),
    case("lzcnt rip neighbour", none, lz32, load_rip, COUNT, 'J', 2),
    case("tzcnt r32", none, tz32, none, COUNT, 'J', 0),
    case("tzcnt r64", none, tz64, none, COUNT, 'J', 0),
    case("tzcnt r16", none, tz16, none, COUNT, 'J', 0),
    case("tzcnt mem", none, tz_mem, none, COUNT, 'J', 0),
    case("tzcnt rsp", push2, tz_rsp, pop2, COUNT, 'J', 0),
    case("tzcnt breakpoint", jmp0, tz32, jmp0, COUNT, 'B', 0),
    case("andn r64", none, andn64, none, LOGIC, 'J', 0),
    case("andn r32", none, andn32, none, LOGIC, 'J', 0),
    case("andn d=s2", none, andn_d_s2, none, LOGIC, 'J', 0),
    case("andn d=s1", none, andn_d_s1, none, LOGIC, 'J', 0),
    case("andn all", none, andn_all, none, LOGIC, 'J', 0),
    case("andn r10d d=s2", none, andn_hi32, none, LOGIC, 'J', 0),
    case("andn mem", none, andn_mem, none, LOGIC, 'J', 0),
    case("andn mem d=s1", none, andn_mem_d_s1, none, LOGIC, 'J', 0),
    case(
        "andn mem index=d",
        none,
        andn_mem_index,
        none,
        LOGIC,
        'J',
        0,
    ),
    case("andn rsp", push2, andn_rsp, pop2, LOGIC, 'J', 0),
    case("andn rip", none, andn_rip, none, LOGIC, 'J', 1),
    case("blsr r64", none, blsr64, none, LOGIC, 'J', 0),
    case("blsr same", none, blsr_same, none, LOGIC, 'J', 0),
    case("blsr mem", none, blsr_mem, none, LOGIC, 'J', 0),
    case("blsr index=d", none, blsr_index, none, LOGIC, 'J', 0),
    case("blsmsk r64", none, blsmsk64, none, LOGIC, 'J', 0),
    case("blsmsk same", none, blsmsk_same, none, LOGIC, 'J', 0),
    case("blsmsk mem", none, blsmsk_mem, none, LOGIC, 'J', 0),
    case("blsi r64", none, blsi64, none, LOGIC, 'J', 0),
    case("blsi same", none, blsi_same, none, LOGIC, 'J', 0),
    case("blsi mem", none, blsi_mem, none, LOGIC, 'J', 0),
    case("bextr r64", none, bextr64, none, BEXTR_FLAGS, 'J', 0),
    case("bextr mem", none, bextr32_mem, none, BEXTR_FLAGS, 'J', 0),
    case("bextr same", none, bextr_same, none, BEXTR_FLAGS, 'J', 0),
    case("movbe load32", none, movbe_l32, none, ALL, 'J', 0),
    case("movbe load64", none, movbe_l64, none, ALL, 'J', 0),
    case("movbe load16", none, movbe_l16, none, ALL, 'J', 0),
    case("movbe store32", none, movbe_s32, none, ALL, 'J', 0),
    case("movbe store64", none, movbe_s64, none, ALL, 'J', 0),
    case("movbe store16", none, movbe_s16, none, ALL, 'J', 0),
    case("movbe rip", none, movbe_rip, none, ALL, 'J', 1),
    case("movbe index=src", none, movbe_index, none, ALL, 'J', 0),
];

const TRIALS: usize = 400;
const DATA: usize = 0x80000;
const CODE_END: usize = 0x40000;
const MISSED: usize = 0x60000;
const IMAGE_SIZE: usize = 0x100000;
const IMAGE_AT: u64 = 0x2000_0000;

/// push callee-saved (both conventions); registers and flags from `inp`; pre, body, post;
/// registers and flags to `outp`. Returns the code and the offset of the body in it.
fn emit(c: &Case, at: u64, inp: u64, outp: u64) -> (Vec<u8>, usize) {
    let mut p: Vec<u8> = vec![
        0x53, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57,
    ];
    p.extend_from_slice(&[0x48, 0xb8]);
    p.extend_from_slice(&inp.to_le_bytes());
    p.extend_from_slice(&[0xff, 0xb0, 0x80, 0, 0, 0, 0x9d]); // push [rax+128]; popfq
    for r in 1..16u8 {
        if r == 4 {
            continue;
        }
        p.extend_from_slice(&[
            0x48 | if r >= 8 { 4 } else { 0 },
            0x8b,
            0x40 | (r & 7) << 3,
            8 * r,
        ]);
    }
    p.extend_from_slice(&[0x48, 0x8b, 0x00]); // mov rax, [rax]
    let mut body_at = 0;
    for (i, part) in [c.pre, c.body, c.post].into_iter().enumerate() {
        if i == 1 {
            body_at = p.len();
        }
        p.extend_from_slice(snip(part));
        if c.rip != 0 && c.rip == i {
            // 1 body, 2 post
            let disp = (IMAGE_AT + DATA as u64 + 16) as i64 - (at as i64 + p.len() as i64);
            let n = p.len();
            p[n - 4..].copy_from_slice(&(disp as i32).to_le_bytes());
        }
    }
    p.push(0x9c); // pushfq
    p.extend_from_slice(&[0x48, 0xa3]); // mov [out], rax
    p.extend_from_slice(&outp.to_le_bytes());
    p.extend_from_slice(&[0x48, 0xb8]); // movabs rax, out
    p.extend_from_slice(&outp.to_le_bytes());
    for r in 1..16u8 {
        if r == 4 {
            continue;
        }
        p.extend_from_slice(&[
            0x48 | if r >= 8 { 4 } else { 0 },
            0x89,
            0x40 | (r & 7) << 3,
            8 * r,
        ]);
    }
    p.extend_from_slice(&[0x8f, 0x80, 0x80, 0, 0, 0]); // pop [rax+128]
    p.extend_from_slice(&[
        0x41, 0x5f, 0x41, 0x5e, 0x41, 0x5d, 0x41, 0x5c, 0x5f, 0x5e, 0x5d, 0x5b, 0xc3,
    ]);
    (p, body_at)
}

struct Rng(u64);

const EDGES: [u64; 14] = [0, 1, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 65, 255];

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn edge(&mut self) -> u64 {
        if !self.next().is_multiple_of(3) {
            EDGES[(self.next() % EDGES.len() as u64) as usize]
        } else {
            self.next() % 256
        }
    }

    fn value(&mut self) -> u64 {
        match self.next() % 10 {
            0 | 1 => 0,
            2 => 1 << (self.next() % 64),
            3 => !0,
            4 => self.next() & 0xffff_ffff,
            5 => {
                // BEXTR controls
                let (a, b) = (self.edge(), self.edge());
                a | b << 8
                    | if !self.next().is_multiple_of(2) {
                        0
                    } else {
                        self.next() << 16
                    }
            }
            6 => self.next() >> (self.next() % 64),
            _ => self.next(),
        }
    }
}

#[derive(Clone)]
struct Trial {
    inp: [u64; 17],
    out: [u64; 17],
    data: [u8; 128],
    after: [u8; 128],
}

struct Machine {
    image: *mut u8,
    inp: *mut u64,
    out: *mut u64,
}

impl Machine {
    fn run(&self, harness: u64, t: &mut Trial) {
        // SAFETY: `harness` is code `emit` wrote into the executable image; `inp` and `out` are
        // 17 qwords it reads and writes, and the data area is inside the image.
        unsafe {
            std::ptr::copy_nonoverlapping(t.inp.as_ptr(), self.inp, 17);
            std::ptr::copy_nonoverlapping(t.data.as_ptr(), self.image.add(DATA), 128);
            let f: unsafe extern "C" fn() = std::mem::transmute(harness as usize);
            f();
            std::ptr::copy_nonoverlapping(self.out, t.out.as_mut_ptr(), 17);
            std::ptr::copy_nonoverlapping(self.image.add(DATA), t.after.as_mut_ptr(), 128);
        }
    }
}

static TRAPS: AtomicU32 = AtomicU32::new(0);

#[cfg(target_os = "linux")]
mod trap {
    use super::*;

    extern "C" fn handler(_sig: i32, _info: *mut libc::siginfo_t, context: *mut libc::c_void) {
        // SAFETY: the kernel passes the interrupted context.
        unsafe {
            let uc = context as *mut libc::ucontext_t;
            let gregs = &mut (*uc).uc_mcontext.gregs;
            let mut rip = gregs[libc::REG_RIP as usize] as u64;
            if cpu_patch_breakpoint(&mut rip, rip - 1) == 0 {
                let m = b"unexpected breakpoint\n";
                libc::write(2, m.as_ptr().cast(), m.len());
                libc::_exit(1);
            }
            gregs[libc::REG_RIP as usize] = rip as i64;
        }
        TRAPS.fetch_add(1, Ordering::Relaxed);
    }

    pub fn install() {
        // SAFETY: a plain SIGTRAP handler for the rewritten breakpoints.
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = handler as *const () as usize;
            sa.sa_flags = libc::SA_SIGINFO;
            assert_eq!(libc::sigaction(libc::SIGTRAP, &sa, std::ptr::null_mut()), 0);
        }
    }
}

#[cfg(windows)]
mod trap {
    use super::*;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS,
    };

    const BREAKPOINT: i32 = 0x8000_0003_u32 as i32;
    const CONTINUE_EXECUTION: i32 = -1;
    const CONTINUE_SEARCH: i32 = 0;

    unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows passes the exception being dispatched.
        unsafe {
            if (*(*info).ExceptionRecord).ExceptionCode != BREAKPOINT {
                return CONTINUE_SEARCH;
            }
            let mut rip = (*(*info).ContextRecord).Rip;
            if cpu_patch_breakpoint(&mut rip, rip) == 0 {
                eprintln!("unexpected breakpoint");
                std::process::exit(1);
            }
            (*(*info).ContextRecord).Rip = rip;
        }
        TRAPS.fetch_add(1, Ordering::Relaxed);
        CONTINUE_EXECUTION
    }

    pub fn install() {
        // SAFETY: a vectored handler for the rewritten breakpoints.
        let h = unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
        assert!(!h.is_null());
    }
}

#[test]
fn rewritten_instructions_match_native() {
    if host_missing() != 0 {
        println!(
            "cpu_patch test: this CPU lacks BMI1, LZCNT or MOVBE, so there is nothing to compare"
        );
        return;
    }
    assert_eq!(
        host::map(IMAGE_AT, IMAGE_SIZE, true),
        Some(IMAGE_AT),
        "image address taken"
    );
    trap::install();
    let image = IMAGE_AT as usize as *mut u8;
    // SAFETY: the image was just mapped read-write-execute.
    unsafe { std::ptr::write_bytes(image, 0xcc, IMAGE_SIZE) };
    let inp = Box::leak(Box::new([0u64; 17])).as_mut_ptr();
    let out = Box::leak(Box::new([0u64; 17])).as_mut_ptr();
    let machine = Machine { image, inp, out };
    let put = |at: usize, code: &[u8]| patch_util::write_code(IMAGE_AT + at as u64, code);

    let mut harness: Vec<u64> = Vec::new();
    let mut body: Vec<usize> = Vec::new();
    let mut at = 0x1000usize;
    for c in CASES {
        let (code, body_at) = emit(c, IMAGE_AT + at as u64, inp as u64, out as u64);
        put(at, &code);
        harness.push(IMAGE_AT + at as u64);
        body.push(at + body_at);
        at = (at + code.len() + 15) & !15;
        assert!(at < CODE_END);
    }
    // Outside the swept range: an ANDN the scan missed, for cpu_patch_illegal.
    let missed = case("andn missed", none, andn_mem_index, none, LOGIC, '-', 0);
    let (code, body_at) = emit(&missed, IMAGE_AT + MISSED as u64, inp as u64, out as u64);
    put(MISSED, &code);
    harness.push(IMAGE_AT + MISSED as u64);
    body.push(MISSED + body_at);

    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut trials: Vec<Vec<Trial>> = Vec::new();
    for i in 0..=CASES.len() {
        let mut list = Vec::new();
        for k in 0..TRIALS {
            let mut t = Trial {
                inp: [0; 17],
                out: [0; 17],
                data: [0; 128],
                after: [0; 128],
            };
            for slot in t.inp.iter_mut().take(16) {
                *slot = rng.value();
            }
            t.inp[4] = 0;
            t.inp[12] = IMAGE_AT + DATA as u64;
            t.inp[13] = rng.next() % 4;
            t.inp[16] = 0x202 | (rng.next() & ALL);
            if i < CASES.len() && CASES[i].name.starts_with("bextr") {
                // every start x length edge
                let control = EDGES[k % EDGES.len()] | EDGES[k / EDGES.len() % EDGES.len()] << 8;
                t.inp[0] = control;
                t.inp[1] = control;
                t.inp[2] = control;
                t.inp[6] = if k & 1 != 0 {
                    !0u64 >> (k % 3)
                } else {
                    rng.next()
                };
            }
            for j in (0..128).step_by(8) {
                t.data[j..j + 8].copy_from_slice(&rng.value().to_le_bytes());
            }
            machine.run(harness[i], &mut t);
            list.push(t);
        }
        trials.push(list);
    }

    // BB_CPU_PATCH=0 leaves the image as it is.
    let ranges = [[0x1000u64, (CODE_END - 0x1000) as u64]];
    std::env::set_var("BB_CPU_PATCH", "0");
    // SAFETY: the image is mapped and writable and `ranges` has one pair.
    unsafe { cpu_patch_image(image, IMAGE_SIZE as u64, ranges.as_ptr(), 1) };
    for (i, c) in CASES.iter().enumerate() {
        assert_ne!(
            image_byte(body[i]),
            0xe9,
            "{}: rewritten with BB_CPU_PATCH=0",
            c.name
        );
        assert_ne!(
            image_byte(body[i]),
            0xcc,
            "{}: breakpoint with BB_CPU_PATCH=0",
            c.name
        );
    }

    std::env::set_var("BB_CPU_PATCH", "all");
    // SAFETY: as above.
    unsafe { cpu_patch_image(image, IMAGE_SIZE as u64, ranges.as_ptr(), 1) };
    for (i, c) in CASES.iter().enumerate() {
        let (at_body, before) = (image_byte(body[i]), image_byte(body[i] - 1));
        let bad = match c.expect {
            'J' => at_body != 0xe9,
            'B' => at_body != 0xcc,
            'P' => before != 0xe9,
            _ => false,
        };
        assert!(
            !bad,
            "{}: expected {}, found {at_body:02x} (before {before:02x})",
            c.name, c.expect
        );
    }
    // The missed ANDN faults on an old CPU and the handler moves rip to a thunk; here a jmp over
    // its first 5 bytes (it has 6) stands in for the fault.
    let site = IMAGE_AT + body[CASES.len()] as u64;
    let mut rip = site;
    // SAFETY: `rip` is a local.
    assert_eq!(unsafe { cpu_patch_illegal(&mut rip) }, 1);
    assert_ne!(rip, site);
    assert_eq!(image_byte(body[CASES.len()]), 0xc4);
    assert_eq!(snip(andn_mem_index).len(), 6);
    let to = rip as i64 - (site as i64 + 5);
    let mut jump = vec![0xe9];
    jump.extend_from_slice(&(to as i32).to_le_bytes());
    put(body[CASES.len()], &jump);
    // The same instruction again finds its thunk.
    let mut again = site;
    // SAFETY: `again` is a local.
    assert_eq!(unsafe { cpu_patch_illegal(&mut again) }, 1);
    assert_eq!(again, rip);
    // An address outside the image is not ours.
    let mut outside = IMAGE_AT + IMAGE_SIZE as u64 + 4;
    // SAFETY: `outside` is a local.
    assert_eq!(unsafe { cpu_patch_illegal(&mut outside) }, 0);

    let mut failures = 0;
    for i in 0..=CASES.len() {
        let (name, flags) = if i < CASES.len() {
            (CASES[i].name, CASES[i].flags)
        } else {
            (missed.name, missed.flags)
        };
        for (k, t) in trials[i].iter().enumerate() {
            let mut again = t.clone();
            machine.run(harness[i], &mut again);
            let mut bad = t.after != again.after || (t.out[16] ^ again.out[16]) & flags != 0;
            for r in 0..16 {
                if r != 4 && t.out[r] != again.out[r] {
                    bad = true;
                }
            }
            if bad {
                failures += 1;
                if failures <= 10 {
                    println!("FAIL {name} trial {k}");
                    for r in (0..17).filter(|&r| r != 4) {
                        println!(
                            "  {r:2} in {:016x} native {:016x} rewritten {:016x}{}",
                            t.inp[r],
                            t.out[r],
                            again.out[r],
                            if t.out[r] != again.out[r] { " <" } else { "" }
                        );
                    }
                }
            }
        }
    }
    assert_eq!(failures, 0, "rewritten results differ from native");
    println!(
        "cpu_patch: {} cases x {TRIALS} trials match ({} breakpoints taken)",
        CASES.len() + 1,
        TRAPS.load(Ordering::Relaxed)
    );
    assert!(TRAPS.load(Ordering::Relaxed) > 0, "no breakpoint was taken");
}

fn image_byte(offset: usize) -> u8 {
    // SAFETY: inside the test image.
    unsafe { *((IMAGE_AT as usize + offset) as *const u8) }
}

#[test]
fn tsc_scale_math_and_overflow_bounds() {
    assert_eq!(tsc_scale_factor(1_000, 0), 1u64 << 32);
    assert_eq!(tsc_scale_factor(1_000_000, 1_000_000), 1u64 << 32);
    assert_eq!(tsc_scale_factor(500_000, 1_000_000), 1u64 << 31);
    assert_eq!(scale_tsc(0x1_0000_0000, 1u64 << 31), 0x8000_0000);
    assert_eq!(scale_tsc(1_000_000_000_000, 1u64 << 32), 1_000_000_000_000);
    assert_eq!(scale_tsc(1_000_000_000_000, 1u64 << 33), 2_000_000_000_000);
}

#[test]
fn rdtsc_decode_describe_and_emulate_scales_edx_eax() {
    let mut bytes = [0x0f_u8, 0x31]; // rdtsc
    let image = unsafe { Image::new(bytes.as_mut_ptr(), bytes.len() as u64) };
    let item = decode(&image, NEED_RDTSC, 0, bytes.len() as u64).unwrap();
    assert!(item.rewrite);
    assert!(decode(&image, NEED_BMI1, 0, bytes.len() as u64).is_some_and(|i| !i.rewrite));

    let op = describe(&item.d).unwrap();
    assert_eq!(op.kind, RDTSC);

    set_tsc_scale(1u64 << 31); // 0.5x
    let mut regs = [0x1111_2222_3333_4444u64; 17];
    regs[16] = 0x246;
    #[cfg(target_arch = "x86_64")]
    let before = unsafe { core::arch::x86_64::_rdtsc() };
    #[cfg(not(target_arch = "x86_64"))]
    let before = 0u64;

    unsafe { emulate(regs.as_mut_ptr(), &op) };

    #[cfg(target_arch = "x86_64")]
    let after = unsafe { core::arch::x86_64::_rdtsc() };
    #[cfg(not(target_arch = "x86_64"))]
    let after = 0u64;

    set_tsc_scale(1u64 << 32);

    let got = (regs[2] << 32) | (regs[0] & 0xffff_ffff);
    assert!(
        got >= (before >> 1) && got <= (after >> 1) + 10_000,
        "scaled={got} before/2={} after/2={}",
        before >> 1,
        after >> 1
    );
    assert_eq!(regs[0] >> 32, 0, "EAX zero-extends into RAX");
    assert_eq!(regs[2] >> 32, 0, "EDX zero-extends into RDX");
    assert_eq!(regs[1], 0x1111_2222_3333_4444u64);
    assert_eq!(regs[16], 0x246);
}
