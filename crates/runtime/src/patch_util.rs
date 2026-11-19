//! What `cpu_patch` and `red_zone` (piece 3.14) share: the guest image as bytes that may be
//! rewritten, the instruction decoder, a bit set of branch targets and thunk memory within rel32
//! reach of the image. The C code used Zydis and `mmap`/`VirtualAlloc` for these.

use iced_x86::{Decoder, DecoderOptions, FlowControl, Instruction, Mnemonic};

/// The longest x86 instruction.
const MAX_LENGTH: usize = 15;

/// The loaded guest image, which the patchers read and rewrite (the caller keeps it writable and
/// guarantees `base..base+size` is mapped for as long as the patchers use it).
#[derive(Clone, Copy)]
pub struct Image {
    base: *mut u8,
    size: u64,
}

// The image is process memory the loader owns; the patchers run before guest code starts.
unsafe impl Send for Image {}
unsafe impl Sync for Image {}

impl Image {
    /// # Safety
    /// `base..base+size` must be readable and writable memory that stays mapped while this value
    /// is used.
    pub unsafe fn new(base: *mut u8, size: u64) -> Image {
        Image { base, size }
    }

    pub fn addr(&self) -> u64 {
        self.base as usize as u64
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    /// Copies `out.len()` bytes at `offset`; false past the end of the image.
    pub fn copy(&self, offset: u64, out: &mut [u8]) -> bool {
        let Some(end) = offset.checked_add(out.len() as u64) else {
            return false;
        };
        if end > self.size {
            return false;
        }
        // SAFETY: the range is inside the image, which `new` promised is mapped.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.base.add(offset as usize),
                out.as_mut_ptr(),
                out.len(),
            );
        }
        true
    }

    /// `len` bytes at `offset`, or None past the end of the image.
    pub fn vec(&self, offset: u64, len: u64) -> Option<Vec<u8>> {
        let mut out = vec![0u8; usize::try_from(len).ok()?];
        self.copy(offset, &mut out).then_some(out)
    }

    pub fn byte(&self, offset: u64) -> Option<u8> {
        let mut b = [0u8];
        self.copy(offset, &mut b).then_some(b[0])
    }

    pub fn u32_at(&self, offset: u64) -> Option<u32> {
        let mut b = [0u8; 4];
        self.copy(offset, &mut b).then(|| u32::from_le_bytes(b))
    }

    /// Writes `data` at `offset`; ignored past the end of the image.
    pub fn write(&self, offset: u64, data: &[u8]) {
        let in_range = offset
            .checked_add(data.len() as u64)
            .is_some_and(|end| end <= self.size);
        if !in_range {
            return;
        }
        // SAFETY: the range is inside the image, which `new` promised is mapped and writable.
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                self.base.add(offset as usize),
                data.len(),
            );
        }
    }
}

/// One decoded instruction with its bytes (the image may change after it was decoded).
#[derive(Clone)]
pub struct Decoded {
    /// Address of the instruction (not the image offset).
    pub at: u64,
    pub ins: Instruction,
    pub bytes: [u8; MAX_LENGTH],
    /// Offset of the displacement field inside the instruction.
    pub disp_offset: usize,
}

impl Decoded {
    pub fn len(&self) -> usize {
        self.ins.len()
    }

    /// The instruction's memory operand is RIP-relative.
    pub fn rip_relative(&self) -> bool {
        self.ins.is_ip_rel_memory_operand()
    }

    /// The bytes of the instruction.
    pub fn code(&self) -> &[u8] {
        &self.bytes[..self.ins.len()]
    }

    /// The instruction's bytes for a thunk at `at`, its RIP-relative displacement re-aimed from
    /// the instruction's own address. None when the new displacement does not reach.
    pub fn relocated(&self, at: u64) -> Option<Vec<u8>> {
        let mut code = self.code().to_vec();
        if self.rip_relative() {
            let field = code.get(self.disp_offset..self.disp_offset + 4)?;
            let disp = i32::from_le_bytes(field.try_into().ok()?);
            let target = (self.at + self.len() as u64).wrapping_add(disp as i64 as u64);
            let rel = target.wrapping_sub(at + self.len() as u64) as i64;
            let rel = i32::try_from(rel).ok()?;
            code[self.disp_offset..self.disp_offset + 4].copy_from_slice(&rel.to_le_bytes());
        }
        Some(code)
    }
}

/// Decodes the instruction at image offset `offset`, which may not run past `end`.
pub fn decode(image: &Image, offset: u64, end: u64) -> Option<Decoded> {
    let available = usize::try_from(end.checked_sub(offset)?)
        .unwrap_or(MAX_LENGTH)
        .min(MAX_LENGTH);
    if available == 0 {
        return None;
    }
    let mut bytes = [0u8; MAX_LENGTH];
    if !image.copy(offset, &mut bytes[..available]) {
        return None;
    }
    let at = image.addr() + offset;
    let mut decoder = Decoder::with_ip(64, &bytes[..available], at, DecoderOptions::NONE);
    let ins = decoder.decode();
    if ins.is_invalid() {
        return None;
    }
    let disp_offset = decoder.get_constant_offsets(&ins).displacement_offset();
    Some(Decoded {
        at,
        ins,
        bytes,
        disp_offset,
    })
}

/// A direct branch (jump, call, loop) and its target address.
pub fn direct_branch(ins: &Instruction) -> Option<u64> {
    use iced_x86::OpKind;
    if ins.flow_control() == FlowControl::Next || ins.op_count() == 0 {
        return None;
    }
    (ins.op0_kind() == OpKind::NearBranch64).then(|| ins.near_branch_target())
}

/// What may be copied into a thunk: no branches, interrupts or system instructions. (The caller
/// handles RIP-relative operands.)
pub fn movable(ins: &Instruction) -> bool {
    if ins.flow_control() != FlowControl::Next {
        return false;
    }
    !matches!(
        ins.mnemonic(),
        Mnemonic::Syscall
            | Mnemonic::Sysenter
            | Mnemonic::Sysexit
            | Mnemonic::Sysret
            | Mnemonic::Hlt
            | Mnemonic::Ud0
            | Mnemonic::Ud1
            | Mnemonic::Ud2
            | Mnemonic::Int3
            | Mnemonic::Int
            | Mnemonic::Int1
            | Mnemonic::Into
            | Mnemonic::Cli
            | Mnemonic::Sti
            | Mnemonic::In
            | Mnemonic::Out
            | Mnemonic::Insb
            | Mnemonic::Insw
            | Mnemonic::Insd
            | Mnemonic::Outsb
            | Mnemonic::Outsw
            | Mnemonic::Outsd
            | Mnemonic::Rdmsr
            | Mnemonic::Wrmsr
            | Mnemonic::Wbinvd
            | Mnemonic::Invd
            | Mnemonic::Invlpg
            | Mnemonic::Clts
            | Mnemonic::Swapgs
            | Mnemonic::Lgdt
            | Mnemonic::Lidt
            | Mnemonic::Lldt
            | Mnemonic::Ltr
    )
}

/// A bit per image byte.
pub struct Bits(Vec<u8>);

impl Bits {
    pub fn new(size: u64) -> Bits {
        Bits(vec![0; (size / 8 + 1) as usize])
    }

    pub fn get(&self, offset: u64) -> bool {
        self.0
            .get((offset >> 3) as usize)
            .is_some_and(|b| b >> (offset & 7) & 1 != 0)
    }

    pub fn set(&mut self, offset: u64) {
        if let Some(b) = self.0.get_mut((offset >> 3) as usize) {
            *b |= 1 << (offset & 7);
        }
    }
}

/// Executable memory at a chosen address (host calls).
#[cfg(target_os = "linux")]
pub mod host {
    /// Maps `len` bytes exactly at `at`, read-write-execute when `commit` is set, else reserved
    /// (no access). None when the range is taken.
    pub fn map(at: u64, len: usize, commit: bool) -> Option<u64> {
        let prot = if commit {
            libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC
        } else {
            libc::PROT_NONE
        };
        // SAFETY: an anonymous mapping that must not replace an existing one; a kernel that
        // ignores the flag may return another address, which is unmapped again.
        unsafe {
            let p = libc::mmap(
                at as usize as *mut libc::c_void,
                len,
                prot,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED_NOREPLACE,
                -1,
                0,
            );
            if p == libc::MAP_FAILED {
                return None;
            }
            if p as usize as u64 == at {
                return Some(at);
            }
            libc::munmap(p, len);
        }
        None
    }

    /// Makes part of a reserved range read-write-execute.
    pub fn commit(at: u64, len: usize) -> bool {
        // SAFETY: only called on ranges `map` returned.
        unsafe {
            libc::mprotect(
                at as usize as *mut libc::c_void,
                len,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
            ) == 0
        }
    }
}

/// Executable memory at a chosen address (host calls).
#[cfg(windows)]
pub mod host {
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE, PAGE_NOACCESS,
    };

    pub fn map(at: u64, len: usize, commit: bool) -> Option<u64> {
        let (kind, protect) = if commit {
            (MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE)
        } else {
            (MEM_RESERVE, PAGE_NOACCESS)
        };
        // SAFETY: a request for a free range; nothing existing is touched.
        let p = unsafe { VirtualAlloc(at as usize as *const _, len, kind, protect) };
        (!p.is_null()).then_some(p as usize as u64)
    }

    pub fn commit(at: u64, len: usize) -> bool {
        // SAFETY: only called on ranges `map` reserved.
        let p = unsafe {
            VirtualAlloc(
                at as usize as *const _,
                len,
                MEM_COMMIT,
                PAGE_EXECUTE_READWRITE,
            )
        };
        !p.is_null()
    }
}

/// Copies code into thunk memory.
pub fn write_code(at: u64, code: &[u8]) {
    // SAFETY: `at` is inside memory `host` mapped for thunks and the caller checked the room.
    unsafe {
        std::ptr::copy_nonoverlapping(code.as_ptr(), at as usize as *mut u8, code.len());
    }
}
