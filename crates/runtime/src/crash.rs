//! One-line runtime crash records consumed by `crates/core::logs::crash_line` (#168).
//!
//! Emits at most one `Crash:` line per process, including async-signal-safe emission from the
//! loader's fault/watchdog handlers (`crates/runtime/src/loader/fault.rs`).

use std::fmt::{self, Write as FmtWrite};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

static REPORTED: AtomicBool = AtomicBool::new(false);

/// The failure category written to the `kind` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashKind {
    Assertion,
    Fault,
    Exception,
}

impl CrashKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Assertion => "assert",
            Self::Fault => "fault",
            Self::Exception => "exception",
        }
    }
}

/// Write one bounded `Crash:` record to stderr.
///
/// Only the caller-provided guest offset or source location and short diagnostic are emitted;
/// no paths to game data, dump contents, or guest buffers are read or serialized here.
///
/// Prints at most once per process: a fault inside the fault handler, or an assertion that
/// follows a fault, must not add a second line. Later calls return `Ok(false)`; the first
/// returns `Ok(true)` once the line is written.
pub fn report(kind: CrashKind, location: &str, text: &str) -> io::Result<bool> {
    write_once(&REPORTED, &mut io::stderr().lock(), kind, location, text)
}

/// Async-signal-safe counterpart to [`report`] sharing the same process-wide once guard.
///
/// Formats into a fixed stack buffer without heap allocation or Rust stderr locking and writes
/// directly to stderr (`fd 2` on Unix). Returns `true` if this call wrote the line.
pub fn report_signal_safe(kind: CrashKind, location: &str, text: &str) -> bool {
    if REPORTED.swap(true, Ordering::SeqCst) {
        return false;
    }
    let mut raw = [0u8; 1536];
    let len = {
        let mut buf = StackBuf::new(&mut raw);
        let _ = write_record(&mut buf, kind, location, text);
        buf.pos
    };
    write_stderr_raw(&raw[..len]);
    true
}

fn write_stderr_raw(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    #[cfg(unix)]
    {
        extern "C" {
            fn write(fd: std::ffi::c_int, buf: *const std::ffi::c_void, count: usize) -> isize;
        }
        // SAFETY: fd 2 is stderr and `bytes` is a valid slice for the duration of the call.
        unsafe {
            let _ = write(2, bytes.as_ptr().cast(), bytes.len());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = io::stderr().write_all(bytes);
    }
}

struct StackBuf<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> StackBuf<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn as_str(&self) -> &str {
        // Only valid UTF-8 is ever written via `FmtWrite` / `Write` below.
        std::str::from_utf8(&self.buf[..self.pos]).unwrap_or("")
    }
}

impl Write for StackBuf<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let avail = self.buf.len().saturating_sub(self.pos);
        let n = data.len().min(avail);
        self.buf[self.pos..self.pos + n].copy_from_slice(&data[..n]);
        self.pos += n;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl FmtWrite for StackBuf<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let avail = self.buf.len().saturating_sub(self.pos);
        if s.len() <= avail {
            self.buf[self.pos..self.pos + s.len()].copy_from_slice(s.as_bytes());
            self.pos += s.len();
            return Ok(());
        }
        // Truncate cleanly at a UTF-8 char boundary.
        let mut end = 0usize;
        for ch in s.chars() {
            let next = end + ch.len_utf8();
            if next > avail {
                break;
            }
            end = next;
        }
        self.buf[self.pos..self.pos + end].copy_from_slice(&s.as_bytes()[..end]);
        self.pos += end;
        Ok(())
    }
}

fn write_once(
    guard: &AtomicBool,
    writer: &mut impl Write,
    kind: CrashKind,
    location: &str,
    text: &str,
) -> io::Result<bool> {
    if guard.swap(true, Ordering::SeqCst) {
        return Ok(false);
    }
    write_record(writer, kind, location, text).map(|()| true)
}

fn write_record(
    writer: &mut impl Write,
    kind: CrashKind,
    location: &str,
    text: &str,
) -> io::Result<()> {
    writer.write_all(b"Crash: kind=")?;
    writer.write_all(kind.as_str().as_bytes())?;
    writer.write_all(b" where=")?;
    write_one_line(writer, location, 128)?;
    writer.write_all(b" text=")?;
    write_one_line(writer, text, 200)?;
    writer.write_all(b"\n")
}

fn write_one_line(writer: &mut impl Write, value: &str, max_chars: usize) -> io::Result<()> {
    let mut wrote = false;
    let mut utf8 = [0u8; 4];
    for ch in value.chars().take(max_chars) {
        let mapped = if ch.is_control() { ' ' } else { ch };
        let encoded = mapped.encode_utf8(&mut utf8);
        writer.write_all(encoded.as_bytes())?;
        wrote = true;
    }
    if !wrote {
        writer.write_all(b"unknown")?;
    }
    Ok(())
}

/// Decoded `MOVBE` memory-operand instruction for AMD FX emulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovbeAccess {
    pub len: usize,
    pub address: u64,
    pub reg: usize,
    pub width: u8,
    pub is_store: bool,
}

/// Decodes a `MOVBE` with a memory operand at `code` (`regs` in `Rax, Rcx, Rdx, Rbx, Rsp, Rbp,
/// Rsi, Rdi, R8..R15` order), returning `None` when `code` is not a memory-operand `MOVBE`.
pub fn movbe_decode(code: &[u8], regs: &[u64; 16], rip: u64) -> Option<MovbeAccess> {
    let mut n = 0usize;
    let mut size16 = false;
    while n < 3 && *code.get(n)? == 0x66 {
        size16 = true;
        n += 1;
    }
    let mut rex = 0u8;
    if (*code.get(n)? & 0xf0) == 0x40 {
        rex = *code.get(n)?;
        n += 1;
    }
    if *code.get(n)? != 0x0f || *code.get(n + 1)? != 0x38 {
        return None;
    }
    let op = *code.get(n + 2)?;
    if op != 0xf0 && op != 0xf1 {
        return None;
    }
    let is_store = op == 0xf1;
    n += 3;

    let modrm = *code.get(n)?;
    n += 1;
    let mod_bits = modrm >> 6;
    let rm = (modrm & 7) as usize;
    if mod_bits == 3 {
        return None;
    }
    let reg = (((modrm >> 3) & 7) | ((rex & 4) << 1)) as usize;

    let mut at = 0u64;
    let mut rip_relative = false;
    if rm == 4 {
        let sib = *code.get(n)?;
        n += 1;
        let index = (((sib >> 3) & 7) | ((rex & 2) << 2)) as usize;
        let base = ((sib & 7) | ((rex & 1) << 3)) as usize;
        if index != 4 {
            at = at.wrapping_add(regs[index].wrapping_shl(u32::from(sib >> 6)));
        }
        if (sib & 7) == 5 && mod_bits == 0 {
            let disp = i32::from_le_bytes(code.get(n..n + 4)?.try_into().ok()?);
            n += 4;
            at = at.wrapping_add(disp as i64 as u64);
        } else {
            at = at.wrapping_add(regs[base]);
        }
    } else if rm == 5 && mod_bits == 0 {
        let disp = i32::from_le_bytes(code.get(n..n + 4)?.try_into().ok()?);
        n += 4;
        at = disp as i64 as u64;
        rip_relative = true;
    } else {
        let base = rm | (((rex & 1) as usize) << 3);
        at = regs[base];
    }

    if mod_bits == 1 {
        let disp = *code.get(n)? as i8;
        n += 1;
        at = at.wrapping_add(disp as i64 as u64);
    } else if mod_bits == 2 {
        let disp = i32::from_le_bytes(code.get(n..n + 4)?.try_into().ok()?);
        n += 4;
        at = at.wrapping_add(disp as i64 as u64);
    }

    if rip_relative {
        at = at.wrapping_add(rip.wrapping_add(n as u64));
    }

    let width = if (rex & 8) != 0 {
        8
    } else if size16 {
        2
    } else {
        4
    };
    Some(MovbeAccess {
        len: n,
        address: at,
        reg,
        width,
        is_store,
    })
}

/// Formats a single-line Linux fault summary matching `src/probe.c`.
pub fn format_linux_fault(
    sig: i32,
    rip: u64,
    image_base: u64,
    fault_addr: u64,
    host_where: Option<(&str, u64, Option<&str>)>,
) -> String {
    if image_base != 0 && rip.wrapping_sub(image_base) < 0x1000_0000 {
        format!(
            "Guest fault (signal {sig}) at guest offset {:#x}, address {:#x}\n",
            rip.wrapping_sub(image_base),
            fault_addr
        )
    } else if let Some((fname, off, sname)) = host_where {
        format!(
            "Host fault (signal {sig}) in {fname}+{off:#x} ({}), address {:#x}\n",
            sname.unwrap_or("?"),
            fault_addr
        )
    } else {
        format!("Fault (signal {sig}) at RIP {rip:#x}, address {fault_addr:#x}\n")
    }
}

fn write_linux_fault_crash_fields(
    where_out: &mut impl FmtWrite,
    text_out: &mut impl FmtWrite,
    sig: i32,
    rip: u64,
    image_base: u64,
    fault_addr: u64,
    host_where: Option<(&str, u64, Option<&str>)>,
) {
    if image_base != 0 && rip.wrapping_sub(image_base) < 0x1000_0000 {
        let offset = rip.wrapping_sub(image_base);
        let _ = write!(where_out, "{offset:#x}");
        let _ = write!(
            text_out,
            "signal {sig} at guest offset {offset:#x}, address {fault_addr:#x}"
        );
    } else if let Some((fname, off, sname)) = host_where {
        // Strip host directories so no local user/install paths appear in Crash: (#168).
        let base = fname.rsplit(['/', '\\']).next().unwrap_or(fname);
        let _ = write!(where_out, "{base}+{off:#x}");
        let _ = write!(
            text_out,
            "signal {sig} ({}) at address {fault_addr:#x}",
            sname.unwrap_or("?")
        );
    } else {
        let _ = write!(where_out, "{rip:#x}");
        let _ = write!(
            text_out,
            "signal {sig} at RIP {rip:#x}, address {fault_addr:#x}"
        );
    }
}

/// Computes `(where, text)` fields for a Linux fault `Crash:` record.
pub fn linux_fault_crash_fields(
    sig: i32,
    rip: u64,
    image_base: u64,
    fault_addr: u64,
    host_where: Option<(&str, u64, Option<&str>)>,
) -> (String, String) {
    let mut where_str = String::new();
    let mut text_str = String::new();
    write_linux_fault_crash_fields(
        &mut where_str,
        &mut text_str,
        sig,
        rip,
        image_base,
        fault_addr,
        host_where,
    );
    (where_str, text_str)
}

/// Emits one async-signal-safe `Crash: kind=fault ...` line for a Linux signal fault (#168).
pub fn report_linux_fault_signal_safe(
    sig: i32,
    rip: u64,
    image_base: u64,
    fault_addr: u64,
    host_where: Option<(&str, u64, Option<&str>)>,
) -> bool {
    let mut where_raw = [0u8; 256];
    let mut text_raw = [0u8; 256];
    let mut where_buf = StackBuf::new(&mut where_raw);
    let mut text_buf = StackBuf::new(&mut text_raw);
    write_linux_fault_crash_fields(
        &mut where_buf,
        &mut text_buf,
        sig,
        rip,
        image_base,
        fault_addr,
        host_where,
    );
    report_signal_safe(CrashKind::Fault, where_buf.as_str(), text_buf.as_str())
}

fn write_windows_fault_crash_fields(
    where_out: &mut impl FmtWrite,
    text_out: &mut impl FmtWrite,
    code: u32,
    rip: u64,
    image_base: u64,
    access: Option<(&str, u64)>,
    host_where: Option<(&str, u64)>,
) {
    if image_base != 0 && rip.wrapping_sub(image_base) < 0x2000_0000 {
        let offset = rip.wrapping_sub(image_base);
        let _ = write!(where_out, "{offset:#x}");
        if let Some((kind, addr)) = access {
            let _ = write!(
                text_out,
                "{kind} access to {addr:#x} at guest offset {offset:#x}"
            );
        } else {
            let _ = write!(
                text_out,
                "exception {code:#010x} at guest offset {offset:#x}"
            );
        }
    } else if let Some((module, off)) = host_where {
        let base = module.rsplit(['/', '\\']).next().unwrap_or(module);
        let _ = write!(where_out, "{base}+{off:#x}");
        if let Some((kind, addr)) = access {
            let _ = write!(text_out, "{kind} access to {addr:#x}");
        } else {
            let _ = write!(text_out, "exception {code:#010x}");
        }
    } else {
        let _ = write!(where_out, "{rip:#x}");
        if let Some((kind, addr)) = access {
            let _ = write!(text_out, "{kind} access to {addr:#x} at RIP {rip:#x}");
        } else {
            let _ = write!(text_out, "exception {code:#010x} at RIP {rip:#x}");
        }
    }
}

/// Emits one `Crash: kind=fault ...` line for a Windows exception fault (#168).
pub fn report_windows_fault_signal_safe(
    code: u32,
    rip: u64,
    image_base: u64,
    access: Option<(&str, u64)>,
    host_where: Option<(&str, u64)>,
) -> bool {
    let mut where_raw = [0u8; 256];
    let mut text_raw = [0u8; 256];
    let mut where_buf = StackBuf::new(&mut where_raw);
    let mut text_buf = StackBuf::new(&mut text_raw);
    write_windows_fault_crash_fields(
        &mut where_buf,
        &mut text_buf,
        code,
        rip,
        image_base,
        access,
        host_where,
    );
    report_signal_safe(CrashKind::Fault, where_buf.as_str(), text_buf.as_str())
}

#[cfg(test)]
mod tests {
    use super::{
        format_linux_fault, linux_fault_crash_fields, movbe_decode, write_once, write_record,
        CrashKind,
    };
    use std::sync::atomic::AtomicBool;

    #[test]
    fn once_guard_prints_exactly_one_crash_line() {
        let guard = AtomicBool::new(false);
        let mut output = Vec::new();
        assert!(write_once(&guard, &mut output, CrashKind::Fault, "0x10", "read 0x20").unwrap());
        assert!(!write_once(&guard, &mut output, CrashKind::Assertion, "a.rs:1", "again").unwrap());
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output, "Crash: kind=fault where=0x10 text=read 0x20\n");
    }

    #[test]
    fn forced_fault_emits_one_bounded_parseable_crash_line() {
        let mut output = Vec::new();
        write_record(
            &mut output,
            CrashKind::Fault,
            "0x1234\nforged prefix",
            &format!("read access to 0x5678{}", "é".repeat(250)),
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("Crash: kind=fault where=0x1234 forged prefix text="));
        let text = lines[0].split_once(" text=").unwrap().1;
        assert_eq!(text.chars().count(), 200);
        assert!(!text.chars().any(char::is_control));
    }

    #[test]
    fn movbe_decode_handles_widths_sib_rip_and_rejects_register_mod() {
        let mut regs = [0u64; 16];
        regs[1] = 0x1000; // rcx
        regs[2] = 0x20; // rdx

        // 64-bit store: movbe [rcx + rdx*4 + 0x10], r8 -> 4c 0f 38 f1 44 91 10
        let store64 = [0x4c, 0x0f, 0x38, 0xf1, 0x44, 0x91, 0x10];
        let acc = movbe_decode(&store64, &regs, 0x5000).unwrap();
        assert_eq!(acc.len, 7);
        assert_eq!(acc.address, 0x1000 + 0x20 * 4 + 0x10);
        assert_eq!(acc.reg, 8);
        assert_eq!(acc.width, 8);
        assert!(acc.is_store);

        // 16-bit RIP-relative load: 66 0f 38 f0 05 20 00 00 00 -> movbe ax, [rip + 0x20]
        let load16_rip = [0x66, 0x0f, 0x38, 0xf0, 0x05, 0x20, 0x00, 0x00, 0x00];
        let acc = movbe_decode(&load16_rip, &regs, 0x4000).unwrap();
        assert_eq!(acc.len, 9);
        assert_eq!(acc.address, 0x4000 + 9 + 0x20);
        assert_eq!(acc.reg, 0);
        assert_eq!(acc.width, 2);
        assert!(!acc.is_store);

        // Register-only mod=3 must be rejected.
        let reg_only = [0x0f, 0x38, 0xf0, 0xc1];
        assert!(movbe_decode(&reg_only, &regs, 0).is_none());
    }

    #[test]
    fn format_linux_fault_distinguishes_guest_host_and_unknown() {
        assert_eq!(
            format_linux_fault(11, 0x1000_1234, 0x1000_0000, 0xdead, None),
            "Guest fault (signal 11) at guest offset 0x1234, address 0xdead\n"
        );
        assert_eq!(
            format_linux_fault(
                11,
                0x7fff_0100,
                0x1000_0000,
                0x10,
                Some(("libfoo.so", 0x100, Some("bar")))
            ),
            "Host fault (signal 11) in libfoo.so+0x100 (bar), address 0x10\n"
        );
        assert_eq!(
            format_linux_fault(4, 0x7fff_0100, 0x1000_0000, 0, None),
            "Fault (signal 4) at RIP 0x7fff0100, address 0x0\n"
        );
        // Zero image_base must not treat low host addresses as guest offsets.
        assert_eq!(
            format_linux_fault(11, 0x1000, 0, 0, None),
            "Fault (signal 11) at RIP 0x1000, address 0x0\n"
        );
    }

    #[test]
    fn linux_fault_crash_fields_match_known_issues_and_strip_host_paths() {
        let (loc, text) = linux_fault_crash_fields(11, 0x1074_d488, 0x1000_0000, 0x0, None);
        assert_eq!(loc, "0x74d488");
        assert_eq!(text, "signal 11 at guest offset 0x74d488, address 0x0");

        let (loc, text) = linux_fault_crash_fields(
            11,
            0x7fff_0100,
            0x1000_0000,
            0x10,
            Some(("/home/deck/dev/out/gpu/libbbgpu.so", 0x100, Some("draw"))),
        );
        assert_eq!(loc, "libbbgpu.so+0x100");
        assert_eq!(text, "signal 11 (draw) at address 0x10");

        let (loc, text) = linux_fault_crash_fields(4, 0x7fff_0100, 0, 0x20, None);
        assert_eq!(loc, "0x7fff0100");
        assert_eq!(text, "signal 4 at RIP 0x7fff0100, address 0x20");
    }
}
