//! `red_zone` (the port of `tests/test_red_zone.c`, without the game executable part, which
//! needs files this repository never holds):
//!
//! 1. On Windows, a leaf that keeps 16 qwords below rsp takes a write fault (a read-only page
//!    that the handler makes writable, as the GPU page tracking does) with the upper YMM halves
//!    in use: how many qwords Windows left, with the store as it is and with rsp lowered by 128
//!    around it.
//! 2. A synthetic leaf with an `.eh_frame_hdr`, built in memory, is patched and run with a
//!    fault in its store: the data below rsp survives, the store runs from a thunk with rsp 128
//!    bytes lower, and the handlers' lookups find the thunk.

use super::*;
use std::sync::atomic::AtomicU64;

const IMAGE_AT: u64 = 0x2800_0000;
const IMAGE_LEN: usize = 0x10000;
const CODE: u64 = 0x1000;
const HEADER: u64 = 0x2000;
const CIE: u64 = 0x2080;
const FDE: u64 = 0x2100;
/// The page the leaf's store faults on.
const GUARDED: u64 = 0x9000;
const PAGE: usize = 4096;

/// SysV leaf (rdi = target, rdx = value): r8 = rsp, data kept below rsp, a store through rdi
/// that faults, the data read back; returns rdi + rdx.
const LEAF: [u8; 30] = [
    0x49, 0x89, 0xe0, // mov r8, rsp
    0x48, 0x89, 0x7c, 0x24, 0xf8, // mov [rsp-8], rdi
    0x48, 0x89, 0x54, 0x24, 0xf0, // mov [rsp-16], rdx
    0xc7, 0x07, 0x34, 0x12, 0x00, 0x00, // mov dword [rdi], 0x1234
    0x48, 0x8b, 0x44, 0x24, 0xf8, // mov rax, [rsp-8]
    0x48, 0x03, 0x44, 0x24, 0xf0, // add rax, [rsp-16]
    0xc3, // ret
];
/// Offset of the faulting store in the leaf.
const STORE: u64 = 13;

static FAULTS: AtomicU64 = AtomicU64::new(0);
static FAULT_RIP: AtomicU64 = AtomicU64::new(0);
static FAULT_RSP: AtomicU64 = AtomicU64::new(0);
static FAULT_R8: AtomicU64 = AtomicU64::new(0);
/// The page the handler makes writable.
static WATCHED: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "linux")]
mod fault {
    use super::*;

    extern "C" fn handler(_sig: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void) {
        // SAFETY: the kernel passes the signal's info and the interrupted context.
        unsafe {
            let page = WATCHED.load(Ordering::Relaxed);
            let address = (*info).si_addr() as usize as u64;
            if address.wrapping_sub(page) >= PAGE as u64 {
                libc::_exit(1);
            }
            let gregs = &(*(context as *mut libc::ucontext_t)).uc_mcontext.gregs;
            FAULT_RIP.store(gregs[libc::REG_RIP as usize] as u64, Ordering::Relaxed);
            FAULT_RSP.store(gregs[libc::REG_RSP as usize] as u64, Ordering::Relaxed);
            FAULT_R8.store(gregs[libc::REG_R8 as usize] as u64, Ordering::Relaxed);
            FAULTS.fetch_add(1, Ordering::Relaxed);
            let rwx = libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC;
            libc::mprotect(page as usize as *mut libc::c_void, PAGE, rwx);
        }
    }

    pub fn install() {
        // SAFETY: a plain SIGSEGV handler for the guarded page.
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = handler as *const () as usize;
            sa.sa_flags = libc::SA_SIGINFO | libc::SA_NODEFER;
            assert_eq!(libc::sigaction(libc::SIGSEGV, &sa, std::ptr::null_mut()), 0);
        }
    }

    pub fn guard(page: u64) {
        WATCHED.store(page, Ordering::Relaxed);
        // SAFETY: the page is inside the test image.
        let r =
            unsafe { libc::mprotect(page as usize as *mut libc::c_void, PAGE, libc::PROT_READ) };
        assert_eq!(r, 0);
    }
}

#[cfg(windows)]
mod fault {
    use super::*;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS,
    };
    use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_READONLY, PAGE_READWRITE};

    const ACCESS_VIOLATION: i32 = 0xc000_0005_u32 as i32;

    unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows passes the exception being dispatched.
        unsafe {
            let record = &*(*info).ExceptionRecord;
            let page = WATCHED.load(Ordering::Relaxed);
            if record.ExceptionCode != ACCESS_VIOLATION
                || record.NumberParameters < 2
                || (record.ExceptionInformation[1] as u64).wrapping_sub(page) >= PAGE as u64
            {
                return 0;
            }
            let context = &*(*info).ContextRecord;
            FAULT_RIP.store(context.Rip, Ordering::Relaxed);
            FAULT_RSP.store(context.Rsp, Ordering::Relaxed);
            FAULT_R8.store(context.R8, Ordering::Relaxed);
            FAULTS.fetch_add(1, Ordering::Relaxed);
            let mut old = 0;
            VirtualProtect(page as usize as *const _, PAGE, PAGE_READWRITE, &mut old);
        }
        -1
    }

    pub fn install() {
        // SAFETY: a vectored handler for the guarded page.
        let h = unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
        assert!(!h.is_null());
    }

    pub fn guard(page: u64) {
        WATCHED.store(page, Ordering::Relaxed);
        let mut old = 0;
        // SAFETY: the page is inside committed test memory.
        let r = unsafe { VirtualProtect(page as usize as *const _, PAGE, PAGE_READONLY, &mut old) };
        assert_ne!(r, 0);
    }
}

/// Runs a System V function (rdi, rsi, rdx). On Windows with the upper YMM halves in use, as game
/// code does, and with rsp `shift` bytes below a 64-byte boundary at the call: a fault's frame
/// overwrites the stack from the 64-byte boundary at or below rsp down, so that decides what it
/// reaches.
#[cfg(target_os = "linux")]
fn call_leaf(function: u64, a: u64, b: u64, c: u64, _shift: u64) -> u64 {
    // SAFETY: `function` is the leaf the test wrote into the executable image.
    unsafe {
        let f: extern "C" fn(u64, u64, u64) -> u64 = std::mem::transmute(function as usize);
        f(a, b, c)
    }
}

#[cfg(windows)]
mod windows_call {
    use std::sync::atomic::AtomicU64;

    #[no_mangle]
    pub static RZT_STACK_SHIFT: AtomicU64 = AtomicU64::new(0);

    // MS ABI: rcx = function, rdx, r8, r9 = its arguments.
    std::arch::global_asm!(
        ".globl rzt_call_with_avx",
        "rzt_call_with_avx:",
        "  push %rdi",
        "  push %rsi",
        "  push %rbx",
        "  mov %rsp, %rbx",
        "  and $-64, %rsp",
        "  sub RZT_STACK_SHIFT(%rip), %rsp",
        "  mov %rcx, %rax",
        "  mov %rdx, %rdi",
        "  mov %r8, %rsi",
        "  mov %r9, %rdx",
        "  vpcmpeqd %xmm1, %xmm1, %xmm1",
        "  vinsertf128 $1, %xmm1, %ymm1, %ymm1",
        "  vmovaps %ymm1, %ymm2",
        "  call *%rax",
        "  vzeroupper",
        "  mov %rbx, %rsp",
        "  pop %rbx",
        "  pop %rsi",
        "  pop %rdi",
        "  ret",
        options(att_syntax)
    );

    extern "C" {
        fn rzt_call_with_avx(function: u64, a: u64, b: u64, c: u64) -> u64;
    }

    pub fn call(function: u64, a: u64, b: u64, c: u64, shift: u64) -> u64 {
        RZT_STACK_SHIFT.store(shift, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: `function` is a System V leaf the test wrote into the executable image.
        unsafe { rzt_call_with_avx(function, a, b, c) }
    }
}

#[cfg(windows)]
use windows_call::call as call_leaf;

/// The synthetic module: a leaf, its `.eh_frame_hdr` (one entry), CIE and FDE.
fn build_image(at: u64) -> (Image, [RedZoneSegment; 3]) {
    assert_eq!(
        host::map(at, IMAGE_LEN, true),
        Some(at),
        "image address taken"
    );
    // SAFETY: the image was just mapped read-write-execute.
    let image = unsafe { Image::new(at as usize as *mut u8, IMAGE_LEN as u64) };
    image.write(CODE, &LEAF);
    let mut hdr = vec![0x01, 0x1b, 0x03, 0x3b];
    hdr.extend_from_slice(&0i32.to_le_bytes()); // .eh_frame pointer (not read)
    hdr.extend_from_slice(&1u32.to_le_bytes()); // entries
    hdr.extend_from_slice(&((CODE as i64 - HEADER as i64) as i32).to_le_bytes());
    hdr.extend_from_slice(&((FDE as i64 - HEADER as i64) as i32).to_le_bytes());
    image.write(HEADER, &hdr);
    // CIE: id 0, version 1, "zR", code and data alignment, return address register 16,
    // augmentation data length 1 with pointer encoding udata4, nop padding.
    let mut cie = 16u32.to_le_bytes().to_vec();
    cie.extend_from_slice(&[0, 0, 0, 0, 1, b'z', b'R', 0, 1, 0x78, 16, 1, 0x03, 0, 0, 0]);
    image.write(CIE, &cie);
    // FDE: CIE pointer, pc_begin, pc_range (udata4), augmentation length 0, padding.
    let mut fde = 16u32.to_le_bytes().to_vec();
    fde.extend_from_slice(&((FDE + 4 - CIE) as u32).to_le_bytes());
    fde.extend_from_slice(&(CODE as u32).to_le_bytes());
    fde.extend_from_slice(&(LEAF.len() as u32).to_le_bytes());
    fde.extend_from_slice(&[0, 0, 0, 0]);
    image.write(FDE, &fde);
    let segments = [
        RedZoneSegment {
            address: CODE,
            size: 0x1000,
            flags: 5,
        },
        RedZoneSegment {
            address: HEADER,
            size: 0x1000,
            flags: 4,
        },
        RedZoneSegment {
            address: 0x8000,
            size: 0x2000,
            flags: 6,
        },
    ];
    (image, segments)
}

#[test]
fn synthetic_leaf_runs_from_a_thunk_with_rsp_lowered() {
    fault::install();
    let (image, segments) = build_image(IMAGE_AT);
    // The thunk memory is reserved first, as the loader does right after mapping the image.
    // SAFETY: the image is mapped.
    unsafe { red_zone_reserve(image.addr() as usize as *mut u8, image.size()) };
    assert_ne!(lock().reserved, 0, "no thunk memory reserved");

    let target = IMAGE_AT + GUARDED + 64;
    let run = || {
        // SAFETY: the target is a u32 slot in the test image.
        unsafe { *(target as usize as *mut u32) = 0 };
        fault::guard(IMAGE_AT + GUARDED);
        let faults = FAULTS.load(Ordering::Relaxed);
        let result = call_leaf(IMAGE_AT + CODE, target, 0, 7, 0);
        assert_eq!(
            FAULTS.load(Ordering::Relaxed),
            faults + 1,
            "the store did not fault once"
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { *(target as usize as *const u32) },
            0x1234,
            "the store was lost"
        );
        assert_eq!(result, target + 7, "the data below rsp was lost");
    };

    // As it is: the fault is in the image and the stack pointer is the leaf's.
    run();
    let rip = FAULT_RIP.load(Ordering::Relaxed);
    assert_eq!(rip, IMAGE_AT + CODE + STORE);
    assert_eq!(
        FAULT_R8.load(Ordering::Relaxed),
        FAULT_RSP.load(Ordering::Relaxed)
    );
    let mut site = 0;
    // SAFETY: `site` is a local.
    assert_eq!(unsafe { red_zone_site(rip, &mut site) }, 0);
    red_zone_fault_handled(rip);

    // SAFETY: the image and the segment list are valid.
    unsafe {
        red_zone_patch(
            image.addr() as usize as *mut u8,
            image.size(),
            segments.as_ptr(),
            3,
        )
    };
    assert_eq!(
        image.byte(CODE + STORE),
        Some(0xe9),
        "the store was not sent to a thunk"
    );
    assert_eq!(
        image.byte(CODE + STORE + 5),
        Some(0xcc),
        "the padding after the jump"
    );
    assert_eq!(
        image.byte(CODE),
        Some(0x49),
        "the code before the store moved"
    );
    assert_eq!(
        lock().reserved_used,
        CHUNK,
        "the thunk came from the reserved memory"
    );

    run();
    let rip = FAULT_RIP.load(Ordering::Relaxed);
    assert!(
        rip.wrapping_sub(IMAGE_AT) >= IMAGE_LEN as u64,
        "the fault is not in a thunk"
    );
    assert_eq!(
        FAULT_R8.load(Ordering::Relaxed) - FAULT_RSP.load(Ordering::Relaxed),
        128,
        "the fault did not run with rsp 128 bytes lower"
    );
    // SAFETY: `site` is a local.
    assert_eq!(unsafe { red_zone_site(rip, &mut site) }, 1);
    assert_eq!(site, CODE + STORE);
    red_zone_fault_handled(rip);
    assert_eq!(FAULTS_KEPT.load(Ordering::Relaxed), 1);
    // An address that is in no thunk.
    // SAFETY: `site` is a local.
    assert_eq!(unsafe { red_zone_site(IMAGE_AT, &mut site) }, 0);
}

#[test]
fn eh_frame_function_table() {
    // The synthetic header is found; without the read flag on its segment it is not looked for.
    let (image, segments) = build_image(0x2900_0000);
    let functions = find_functions(&image, &segments);
    assert_eq!(functions.len(), 1);
    assert_eq!(
        (functions[0].start, functions[0].end),
        (CODE, CODE + LEAF.len() as u64)
    );
    assert!(!functions[0].lsda);
    assert!(find_functions(&image, &segments[..1]).is_empty());
}

#[cfg(windows)]
mod windows_frame {
    use super::*;
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
    };

    // MS ABI: rcx = the store's target, edx = 1 to lower rsp around it. Returns the qwords of
    // [rsp-128, rsp) that still hold the pattern after the store.
    std::arch::global_asm!(
        ".globl rzt_leaf",
        "rzt_leaf:",
        "  movabs $0x1122334455667788, %rax",
        "  mov $1, %r9",
        ".Lrzt_fill:",
        "  mov %rsp, %r10",
        "  mov %r9, %r11",
        "  shl $3, %r11",
        "  sub %r11, %r10",
        "  mov %rax, (%r10)",
        "  inc %r9",
        "  cmp $17, %r9",
        "  jne .Lrzt_fill",
        "  vpcmpeqd %xmm0, %xmm0, %xmm0",
        "  vinsertf128 $1, %xmm0, %ymm0, %ymm0",
        "  vmovaps %ymm0, %ymm1",
        "  test %edx, %edx",
        "  jz .Lrzt_plain",
        "  lea -0x80(%rsp), %rsp",
        "  movl $1, (%rcx)",
        "  lea 0x80(%rsp), %rsp",
        "  jmp .Lrzt_count",
        ".Lrzt_plain:",
        "  movl $1, (%rcx)",
        ".Lrzt_count:",
        "  xor %eax, %eax",
        "  mov $1, %r9",
        ".Lrzt_check:",
        "  mov %rsp, %r10",
        "  mov %r9, %r11",
        "  shl $3, %r11",
        "  sub %r11, %r10",
        "  movabs $0x1122334455667788, %r11",
        "  cmp %r11, (%r10)",
        "  jne .Lrzt_next",
        "  inc %eax",
        ".Lrzt_next:",
        "  inc %r9",
        "  cmp $17, %r9",
        "  jne .Lrzt_check",
        "  vzeroupper",
        "  ret",
        options(att_syntax)
    );

    extern "C" {
        fn rzt_leaf(target: *mut u8, lower: i32) -> i32;
    }

    #[test]
    fn windows_exception_frame_keeps_the_data_below_a_lowered_rsp() {
        fault::install();
        // SAFETY: two committed pages; the second is the guarded one.
        let base = unsafe {
            VirtualAlloc(
                std::ptr::null(),
                2 * PAGE,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        } as usize as u64;
        assert_ne!(base, 0);
        let guarded = base + PAGE as u64;
        let mut kept = [16, 16];
        for (lower, kept) in kept.iter_mut().enumerate() {
            for _ in 0..200 {
                fault::guard(guarded);
                // SAFETY: the guarded page is committed; the leaf only stores 4 bytes there.
                let k = unsafe { rzt_leaf(guarded as usize as *mut u8, lower as i32) };
                *kept = (*kept).min(k);
            }
        }
        println!(
            "Windows exception frame with the AVX state in use: {} of the 16 qwords below rsp kept when the store faults as it is, {} with rsp lowered by 128",
            kept[0], kept[1]
        );
        assert_eq!(
            kept[1], 16,
            "a fault under a lowered rsp keeps the 128 bytes below the old rsp"
        );
    }
}
