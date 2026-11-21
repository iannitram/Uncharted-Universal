//! Guest hook/patch thunks, TCB load rewriting, and rel32-reachable thunk allocation for
//! [`super`] (moving `src/probe.c`'s patching helpers into Rust for 3.17).

use super::{find_pattern, Segment};
use crate::patch_util::{host, write_code};
use iced_x86::{Decoder, DecoderOptions};
use std::ffi::CStr;
use std::sync::atomic::Ordering;

/// Allocates `size` bytes of RWX thunk memory within `+-2 GiB` of `[image_base, image_base + image_size)`.
pub fn near_image_memory(image_base: u64, image_size: u64, size: usize) -> Option<u64> {
    let page_mask = 0xfffu64;
    let end_of_image = (image_size.max(1) + page_mask) & !page_mask;
    let base = (image_base + end_of_image + (32u64 << 20) + 0xffff) & !0xffffu64;
    let max_step = 1900u64 << 20;
    let mut step = 0u64;
    while step < max_step {
        if let Some(addr) = host::map(base + step, size, true) {
            return Some(addr);
        }
        if image_base > step + (2u64 << 20) {
            let below = (image_base - step - (2u64 << 20)) & !0xffffu64;
            if below != 0 {
                if let Some(addr) = host::map(below, size, true) {
                    return Some(addr);
                }
            }
        }
        step += 1u64 << 16;
    }
    None
}

/// Rewrites `mov rax, gs:[0]` (`65 48 8b 04 25 00 00 00 00`) in executable segments to read
/// `gs:[displacement]` (used on Windows where `displacement` is the TEB `TlsSlots` offset).
///
/// # Safety
/// Every executable segment `[image_base + seg.address, +seg.size)` must be mapped and writable.
pub unsafe fn patch_tcb_loads(image_base: u64, segments: &[Segment], displacement: u32) -> u64 {
    const LOAD: [u8; 9] = [0x65, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0];
    let disp_bytes = displacement.to_le_bytes();
    let mut patched = 0u64;
    for seg in segments {
        if (seg.flags & 1) == 0 || (seg.size as usize) < LOAD.len() {
            continue;
        }
        // SAFETY: caller guarantees the segment is mapped and writable.
        let bytes = unsafe {
            std::slice::from_raw_parts_mut((image_base + seg.address) as *mut u8, seg.size as usize)
        };
        let mut i = 0;
        while i + LOAD.len() <= bytes.len() {
            if bytes[i..i + LOAD.len()] == LOAD {
                bytes[i + 5..i + 9].copy_from_slice(&disp_bytes);
                patched += 1;
                i += LOAD.len();
            } else {
                i += 1;
            }
        }
    }
    patched
}

/// Returns the byte length of whole instructions (>= 5, <= 16) at the start of `code` when it
/// begins with a standard frame-pointer prologue (`push rbp; mov rbp, rsp`), or `0` otherwise.
pub fn prologue_bytes(code: &[u8]) -> usize {
    if code.len() < 5 || code[..4] != [0x55, 0x48, 0x89, 0xe5] {
        return 0;
    }
    let mut decoder = Decoder::with_ip(64, code, 0, DecoderOptions::NONE);
    let mut total = 0usize;
    while decoder.can_decode() && total < 5 {
        let instr = decoder.decode();
        if instr.is_invalid() {
            return 0;
        }
        total += instr.len();
        if total > 16 {
            return 0;
        }
    }
    if total >= 5 {
        total
    } else {
        0
    }
}

/// Emits a `RuntimeGuestCallback` thunk at `thunk` for `site` (`f = image_base + offset`) and
/// patches `site` with a `jmp rel32` + `nop`s. Returns the aligned byte size consumed in `thunk`.
///
/// # Safety
/// `thunk` must have at least 320 writable+executable bytes within `+-2 GiB` of `site`, and
/// `site` must have `moved >= 5` writable+executable bytes.
pub unsafe fn emit_callback_thunk(
    thunk: u64,
    site: u64,
    image_base: u64,
    moved: usize,
    callback: usize,
) -> usize {
    debug_assert!(moved >= 5);
    let mut k = 0usize;
    let mut buf = [0u8; 320];

    // Save flags + volatile/callee-saved GPRs in `RuntimeGuestRegs` order.
    const SAVE: [u8; 33] = [
        0x9c, 0x50, 0x51, 0x52, 0x41, 0x50, 0x41, 0x51, 0x41, 0x52, 0x41, 0x53, 0x53, 0x41, 0x54,
        0x41, 0x55, 0x41, 0x57, 0x48, 0x89, 0xe3, 0x48, 0x83, 0xe4, 0xf0, 0x48, 0x81, 0xec, 0xb0,
        0x00, 0x00, 0x00,
    ];
    buf[k..k + SAVE.len()].copy_from_slice(&SAVE);
    k += SAVE.len();

    for x in 0..8u32 {
        let disp = (0x30u32 + 16 * x).to_le_bytes();
        buf[k] = 0xf3;
        buf[k + 1] = 0x0f;
        buf[k + 2] = 0x7f;
        buf[k + 3] = 0x84 | ((x as u8) << 3);
        buf[k + 4] = 0x24;
        buf[k + 5..k + 9].copy_from_slice(&disp);
        k += 9;
    }

    // Win64 ABI args: rcx=rdi, rdx=rsi, r8=image_base, r9=r14, [rsp+0x20]=rbx (regs).
    buf[k..k + 6].copy_from_slice(&[0x48, 0x89, 0xf9, 0x48, 0x89, 0xf2]);
    k += 6;
    buf[k..k + 2].copy_from_slice(&[0x49, 0xb8]);
    buf[k + 2..k + 10].copy_from_slice(&image_base.to_le_bytes());
    k += 10;
    buf[k..k + 8].copy_from_slice(&[0x4d, 0x89, 0xf1, 0x48, 0x89, 0x5c, 0x24, 0x20]);
    k += 8;
    buf[k..k + 2].copy_from_slice(&[0x48, 0xb8]);
    buf[k + 2..k + 10].copy_from_slice(&(callback as u64).to_le_bytes());
    k += 10;
    buf[k..k + 2].copy_from_slice(&[0xff, 0xd0]);
    k += 2;

    for x in 0..8u32 {
        let disp = (0x30u32 + 16 * x).to_le_bytes();
        buf[k] = 0xf3;
        buf[k + 1] = 0x0f;
        buf[k + 2] = 0x6f;
        buf[k + 3] = 0x84 | ((x as u8) << 3);
        buf[k + 4] = 0x24;
        buf[k + 5..k + 9].copy_from_slice(&disp);
        k += 9;
    }

    const RESTORE: [u8; 22] = [
        0x48, 0x89, 0xdc, 0x41, 0x5f, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0x41, 0x5b, 0x41, 0x5a, 0x41,
        0x59, 0x41, 0x58, 0x5a, 0x59, 0x58, 0x9d,
    ];
    buf[k..k + RESTORE.len()].copy_from_slice(&RESTORE);
    k += RESTORE.len();

    // SAFETY: `site` is readable for `moved` bytes.
    let moved_bytes = unsafe { std::slice::from_raw_parts(site as *const u8, moved) };
    buf[k..k + moved].copy_from_slice(moved_bytes);
    k += moved;

    let back = ((site + moved as u64) as i64 - (thunk + k as u64 + 5) as i64) as i32;
    buf[k] = 0xe9;
    buf[k + 1..k + 5].copy_from_slice(&back.to_le_bytes());
    k += 5;

    write_code(thunk, &buf[..k]);

    let to = (thunk as i64 - (site + 5) as i64) as i32;
    // SAFETY: `site` is writable for `moved >= 5` bytes.
    unsafe {
        let dst = std::slice::from_raw_parts_mut(site as *mut u8, moved);
        dst[0] = 0xe9;
        dst[1..5].copy_from_slice(&to.to_le_bytes());
        dst[5..].fill(0x90);
    }

    (k + 15) & !15
}

/// Installs `runtime_guest_callbacks` into executable segments of the guest image.
///
/// # Safety
/// All executable segments in `segments` at `image_base` must be mapped and writable.
pub unsafe fn install_game_callbacks(image_base: u64, image_size: u64, segments: &[Segment]) {
    if std::env::var("BB_CALLBACKS").is_ok_and(|v| v.starts_with('0')) {
        println!("Callbacks: disabled (BB_CALLBACKS=0)");
        return;
    }
    let skip = std::env::var("BB_CALLBACKS_SKIP").ok();

    let mut count = 0usize;
    let ptr = crate::pad::hooks::runtime_guest_callbacks(&mut count);
    if count == 0 || ptr.is_null() {
        return;
    }
    // SAFETY: `runtime_guest_callbacks` returns a valid static slice of `count` entries.
    let callbacks = unsafe { std::slice::from_raw_parts(ptr, count) };

    let thunk_bytes = count * 320;
    let mut thunks: Option<u64> = None;
    let mut thunk_failed = false;
    let mut used = 0usize;
    let mut installed = vec![false; count];
    let mut hook_offsets = Vec::with_capacity(count);

    for (i, c) in callbacks.iter().enumerate() {
        // SAFETY: `name` and `pattern` are static NUL-terminated C strings.
        let name = unsafe { CStr::from_ptr(c.name) }.to_string_lossy();
        if skip
            .as_deref()
            .filter(|s| !s.is_empty())
            .is_some_and(|s| name.contains(s))
        {
            continue;
        }
        let pattern = unsafe { CStr::from_ptr(c.pattern) }.to_string_lossy();
        let mut found: Option<u64> = None;
        let mut matches = 0usize;
        for seg in segments {
            if (seg.flags & 1) == 0 {
                continue;
            }
            // SAFETY: caller guarantees executable segments are mapped.
            let hay = unsafe {
                std::slice::from_raw_parts(
                    (image_base + seg.address) as *const u8,
                    seg.size as usize,
                )
            };
            if let Ok(hits) = find_pattern(hay, &pattern) {
                for hit in hits {
                    if matches == 0 {
                        found = Some(seg.address + hit as u64);
                    }
                    matches += 1;
                }
            }
        }
        if matches > 1 {
            println!("Callbacks: {name}: {matches} matches, not installed");
            continue;
        }
        let Some(base_offset) = found else {
            continue;
        };
        let offset = base_offset + c.skip as u64;
        if hook_offsets.contains(&offset) {
            continue;
        }
        hook_offsets.push(offset);

        if c.callback.is_none() {
            if !c.patch.is_null() && c.moved > 0 {
                // SAFETY: `patch` has `c.moved` bytes and `image_base + offset` is writable.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        c.patch,
                        (image_base + offset) as *mut u8,
                        c.moved,
                    );
                }
                installed[i] = true;
                println!("Callbacks: {name} at {offset:#x} (patch)");
            }
            continue;
        }

        if thunks.is_none() && !thunk_failed {
            thunks = near_image_memory(image_base, image_size, thunk_bytes);
            if thunks.is_none() {
                println!(
                    "Callbacks: no thunk memory near the image: {name} and the ones after it not installed"
                );
                thunk_failed = true;
            }
        }
        let Some(thunk_base) = thunks else {
            continue;
        };
        if used + 320 > thunk_bytes {
            println!("Callbacks: thunk memory full at {name}, the rest not installed");
            break;
        }
        let cb = c.callback.unwrap() as usize;
        // SAFETY: `thunk_base + used` has 320 bytes and `image_base + offset` has `c.moved` bytes.
        let step = unsafe {
            emit_callback_thunk(
                thunk_base + used as u64,
                image_base + offset,
                image_base,
                c.moved,
                cb,
            )
        };
        used += step;
        installed[i] = true;
        if name.contains("(raw mouse)") {
            crate::pad::hooks::runtime_raw_mouse_hooks.store(1, Ordering::Relaxed);
        }
        println!("Callbacks: {name} at {offset:#x}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn patch_tcb_loads_rewrites_executable_segments_only() {
        let mut exec = [0x90, 0x65, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0, 0xc3];
        let mut data = [0x65, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0];
        let base = exec.as_mut_ptr() as u64;
        let data_off = (data.as_mut_ptr() as u64).wrapping_sub(base);
        let segs = [
            Segment {
                address: 0,
                size: exec.len() as u64,
                flags: 1,
            },
            Segment {
                address: data_off,
                size: data.len() as u64,
                flags: 0,
            },
        ];
        // SAFETY: both buffers are valid for their segment lengths.
        let count = unsafe { patch_tcb_loads(base, &segs, 0x1488) };
        assert_eq!(count, 1);
        assert_eq!(&exec[6..10], &0x1488u32.to_le_bytes());
        assert_eq!(&data[5..9], &[0, 0, 0, 0]);
    }

    #[test]
    fn prologue_bytes_handles_common_frame_prologues() {
        // push rbp; mov rbp, rsp; push rbx (5 bytes)
        assert_eq!(prologue_bytes(&[0x55, 0x48, 0x89, 0xe5, 0x53, 0x90]), 5);
        // push rbp; mov rbp, rsp; push r12 (6 bytes)
        assert_eq!(
            prologue_bytes(&[0x55, 0x48, 0x89, 0xe5, 0x41, 0x54, 0x90]),
            6
        );
        // push rbp; mov rbp, rsp; sub rsp, 0x20 (8 bytes)
        assert_eq!(
            prologue_bytes(&[0x55, 0x48, 0x89, 0xe5, 0x48, 0x83, 0xec, 0x20]),
            8
        );
        assert_eq!(prologue_bytes(&[0x90, 0x90, 0x90, 0x90, 0x90]), 0);
    }

    static SEEN_ARG0: AtomicU64 = AtomicU64::new(0);
    static SEEN_BASE: AtomicU64 = AtomicU64::new(0);

    unsafe extern "win64" fn test_cb(rdi: u64, _rsi: u64, image: u64, _r14: u64, _regs: *const u8) {
        SEEN_ARG0.store(rdi, Ordering::Relaxed);
        SEEN_BASE.store(image, Ordering::Relaxed);
    }

    #[test]
    fn callback_thunk_calls_host_and_returns() {
        let code_mem = host::map(0x2_0000_0000, 4096, true).expect("map test code");
        // Function at code_mem: `mov rax, rdi; add rax, 1; ret` (8 bytes: 48 89 f8 48 83 c0 01 c3)
        let func = [0x48u8, 0x89, 0xf8, 0x48, 0x83, 0xc0, 0x01, 0xc3];
        write_code(code_mem, &func);
        let thunk = near_image_memory(code_mem, 4096, 4096).expect("near_image_memory");
        assert!(thunk.abs_diff(code_mem) < (2u64 << 30));

        // SAFETY: `thunk` and `code_mem` are RWX and within rel32 reach.
        let used = unsafe {
            emit_callback_thunk(
                thunk,
                code_mem,
                0x1234_5678,
                7,
                test_cb as *const () as usize,
            )
        };
        assert!(used > 0);

        // SAFETY: `code_mem` is a valid SysV function `fn(u64) -> u64`.
        let f: unsafe extern "sysv64" fn(u64) -> u64 =
            unsafe { std::mem::transmute(code_mem as usize) };
        let res = unsafe { f(41) };
        assert_eq!(res, 42);
        assert_eq!(SEEN_ARG0.load(Ordering::Relaxed), 41);
        assert_eq!(SEEN_BASE.load(Ordering::Relaxed), 0x1234_5678);
    }
}
