//! PS4 virtual memory (roadmap 3.12; replaces `src/runtime_memory.c` on Linux): the direct
//! (physical) pool and its blocks, flexible memory, reserved ranges, protection changes and
//! queries, batch maps, and the GPU library's view of the region table.
//!
//! [`Space`] is the region table and the allocators over a [`Host`] that makes the host
//! mappings. It builds on every target, and its tests run on every CI runner with a fake host.
//! [`Memory`] is the table lock: map, unmap and protect take it exclusively, lookups (the GPU's
//! per-draw queries, `sceKernelVirtualQuery`, GPU page protection) share it, and a per-thread
//! cache of recently found regions holds for one table generation. GPU notifications are
//! queued under the lock and run after it is dropped.
//!
//! On Linux (and Android) `linux` adds the host: one sparse memfd backs the whole direct pool
//! and the flexible span after it, so any mapping of any physical range aliases the same
//! storage, and a second view of it lets the host write past guest and GPU page protection.
//! It also exports the C symbols `runtime_memory.c` exported. Windows keeps
//! `src/runtime_memory.c` (views of a pagefile-backed section in one placeholder) until the
//! Windows shims move (3.15); macOS and iOS build the table only, as a CI build check.
//!
//! Guest addresses are placed below 1 TiB (the PS4 user range): GPU descriptors encode 40-bit
//! addresses, so host-default `0x7f...` addresses would not fit. The port changes no behaviour:
//! the same error codes, search order, block layout and log lines as the C.

#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod linux;

use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{PoisonError, RwLock};

/// `sceKernelGetDirectMemorySize` on a retail PS4: 5056 MiB. `BB_DMEM_MB` raises it (the
/// resolution patches above 1080p need about 4 GiB more; `run.sh` sets it).
pub const RETAIL_POOL: u64 = 5056 << 20;
/// The largest pool `BB_DMEM_MB` may ask for.
pub const MAX_POOL: u64 = 16384 << 20;
/// The flexible memory budget.
pub const FLEXIBLE_SIZE: u64 = 448 << 20;
/// Flexible memory's backing: `[pool, pool + FLEX_SPAN)` after the direct pool.
pub const FLEX_SPAN: u64 = 1 << 30;
/// The PS4 page.
pub const PAGE: u64 = 16384;
/// The PS4 user range, where guest mappings are placed.
pub const USER_MIN: u64 = 0x10_0000_0000;
pub const USER_MAX: u64 = 0xfc_0000_0000;
/// Host-owned memory the guest can see (image, stacks, trampolines) lives in
/// `[LOW_MIN, USER_MIN)`: PS4 code packs pointers into 40-bit fields.
pub const LOW_MIN: u64 = 0x08_0000_0000;
/// The main executable where the PS4 loads it, else as low as possible below 2 GiB:
/// Uncharted's `TGenericOffsetPtr` tables cover pointers into the executable as `[0, 2 GiB)`.
pub const IMAGE_MIN: u64 = 0x40_0000;
pub const IMAGE_LIMIT: u64 = 0x8000_0000;
/// Where `runtime_map_guest_memory` asks for its flexible memory: high, because games claim
/// virtual ranges from `USER_MIN` up without reserving them.
pub const GUEST_SCRATCH: u64 = 0xf0_0000_0000;
/// Direct-memory blocks the table holds at once.
pub const BLOCK_LIMIT: usize = 4096;

/// The PS4 kernel's error codes.
pub const INVALID: i32 = 0x8002_0016_u32 as i32;
pub const NO_MEMORY: i32 = 0x8002_000c_u32 as i32;
pub const ACCESS: i32 = 0x8002_000d_u32 as i32;
pub const NO_SPACE: i32 = 0x8002_0023_u32 as i32;

/// Mapping flags the guest passes.
pub const MAP_FIXED: i32 = 0x10;
pub const MAP_NO_OVERWRITE: i32 = 0x80;

/// Host protection bits (the Linux `PROT_*` values; a [`Host`] maps them to its own).
pub const HOST_READ: i32 = 1;
pub const HOST_WRITE: i32 = 2;
pub const HOST_EXEC: i32 = 4;

/// Guest protection: CPU read 1, write 2, execute 4; GPU read 0x10, write 0x20.
const PROT_MASK: i32 = 0x37;

/// Optimizations switched off at run time (diagnostics): the number in the file named by
/// `BB_TOGGLE_FILE`, re-read every 250 ms. Bit 1 is the region cache here; the other bits are
/// the GPU library's (`gpu/shim/bbport_toggles.h` lists them). The C `uint64_t`
/// `runtime_disabled_optimizations`, read with `__atomic_load_n`.
#[cfg_attr(
    any(target_os = "linux", target_os = "android"),
    export_name = "runtime_disabled_optimizations"
)]
pub static DISABLED_OPTIMIZATIONS: AtomicU64 = AtomicU64::new(0);
/// A second number in the file: more toggles (`BbToggle::Disabled2`), starting at `BB_TOGGLE2`.
#[cfg_attr(
    any(target_os = "linux", target_os = "android"),
    export_name = "runtime_disabled_optimizations2"
)]
pub static DISABLED_OPTIMIZATIONS2: AtomicU64 = AtomicU64::new(0);

/// A GPU notification for a guest range (`bbgpu`): mapped, about to be unmapped, or its
/// protection changed.
pub type GpuRange = extern "C" fn(address: usize, size: u64);

/// What a region of the table is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Reserved,
    Direct,
    Flexible,
}

/// One region of the guest address space, `[start, end)`. `phys` is the byte offset in the
/// backing for direct and flexible memory (flexible memory sits after the direct pool).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    pub kind: Kind,
    pub prot: i32,
    pub memory_type: i32,
    pub phys: u64,
}

/// An allocated direct-memory block, `[start, start + size)` of the pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub start: u64,
    pub size: u64,
    pub memory_type: i32,
}

impl Block {
    fn end(&self) -> u64 {
        self.start + self.size
    }

    fn contains(&self, phys: u64) -> bool {
        phys >= self.start && phys - self.start < self.size
    }
}

/// `sceKernelVirtualQuery`'s result: the PS4 layout (72 bytes). `flags`: flexible 1, direct 2,
/// stack 4, pooled 8, committed 16. `padding` is written as zeros, as the C `memset` did.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualQueryInfo {
    pub start: u64,
    pub end: u64,
    pub offset: u64,
    pub protection: i32,
    pub memory_type: i32,
    pub flags: u32,
    pub name: [u8; 32],
    pub padding: [u8; 4],
}

const _: () = assert!(std::mem::size_of::<VirtualQueryInfo>() == 72);

/// One `sceKernelBatchMap` entry: the PS4 layout (32 bytes). `start` is the guest's `void *`.
/// `operation`: 0 map direct, 1 unmap, 2 protect, 3 map flexible, 4 protect and set the type.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchEntry {
    pub start: u64,
    pub offset: u64,
    pub length: u64,
    pub prot: i8,
    pub memory_type: i8,
    pub reserved: i16,
    pub operation: i32,
}

const _: () = assert!(std::mem::size_of::<BatchEntry>() == 32);

/// Counters for `runtime_memory_report`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub allocations: usize,
    pub maps: usize,
    pub flexible_maps: usize,
    pub protects: usize,
    pub live_bytes: u64,
    pub flexible_bytes: u64,
}

/// The host side of the address space: the backing and the host mappings of guest ranges.
/// Protection is in [`HOST_READ`], [`HOST_WRITE`] and [`HOST_EXEC`] bits.
pub trait Host {
    /// Makes the backing for the direct pool and the flexible span after it (`bytes` in all),
    /// once; false when the host refuses.
    fn pool(&mut self, bytes: u64) -> bool;
    /// Commits backing pages before the guest uses them. Linux backing needs none; the
    /// Windows section (3.15) commits here.
    fn commit(&self, _phys: u64, _size: u64) -> bool {
        true
    }
    /// Maps `[address, address + size)`: the backing at `phys`, or inaccessible when
    /// `reserved`, replacing whatever the host had there.
    fn map(&self, address: u64, size: u64, prot: i32, reserved: bool, phys: u64) -> bool;
    fn unmap(&self, address: u64, size: u64) -> bool;
    /// Called under the shared lock too (GPU page tracking): it must be thread-safe.
    fn protect(&self, address: u64, size: u64, prot: i32) -> bool;
    /// Released backing reads as zero when it is used again.
    fn discard(&self, phys: u64, size: u64);
}

/// `n` rounded up to the power of two `a`, wrapping as the C `uint64_t` arithmetic did.
pub fn align_up(n: u64, a: u64) -> u64 {
    n.wrapping_add(a - 1) & !(a - 1)
}

/// Guest protection to host protection. The GPU read/write bits (0x10/0x20) need host access.
pub fn host_prot(prot: i32) -> i32 {
    let mut host = 0;
    if prot & 0x11 != 0 {
        host |= HOST_READ;
    }
    if prot & 0x22 != 0 {
        host |= HOST_WRITE | HOST_READ;
    }
    if prot & 4 != 0 {
        host |= HOST_EXEC | HOST_READ;
    }
    host
}

/// The pool size for a `BB_DMEM_MB` value: 5056 MiB unless it asks for 5056 to 16384 MiB.
pub fn pool_size(dmem_mb: Option<&str>) -> u64 {
    let mb = dmem_mb.map_or(0, |text| strtoull(text, 10));
    if !(RETAIL_POOL >> 20..=MAX_POOL >> 20).contains(&mb) {
        RETAIL_POOL
    } else {
        mb << 20
    }
}

/// C `strtoull` for `base` 10, or 0 (a `0x` prefix is hex, a leading `0` octal): leading
/// white space and a sign, then as many digits as there are; overflow gives `u64::MAX`.
pub fn strtoull(text: &str, base: u32) -> u64 {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, text) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (base, digits) = if base != 0 {
        (base, text)
    } else if (text.starts_with("0x") || text.starts_with("0X"))
        && text[2..].starts_with(|c: char| c.is_ascii_hexdigit())
    {
        (16, &text[2..])
    } else if text.starts_with('0') {
        (8, text)
    } else {
        (10, text)
    };
    let mut value = 0u64;
    for digit in digits.chars().map_while(|c| c.to_digit(base)) {
        match value
            .checked_mul(u64::from(base))
            .and_then(|v| v.checked_add(u64::from(digit)))
        {
            Some(next) => value = next,
            None => return u64::MAX,
        }
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

/// The toggle file as `fscanf(f, "%llu %llu")` reads it: missing numbers are 0.
pub fn parse_toggles(text: &str) -> (u64, u64) {
    fn number(text: &str) -> Option<(u64, &str)> {
        let text = text.trim_start();
        let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
        let digits = unsigned.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let end = text.len() - unsigned.len() + digits;
        Some((strtoull(&text[..end], 10), &text[end..]))
    }
    match number(text) {
        None => (0, 0),
        Some((first, rest)) => (first, number(rest).map_or(0, |(second, _)| second)),
    }
}

/// `%p` as glibc prints it: `(nil)` for null.
pub fn pointer(address: u64) -> String {
    if address == 0 {
        "(nil)".to_string()
    } else {
        format!("{address:#x}")
    }
}

/// `%#x`: no prefix for zero.
fn alternate_hex(value: u64) -> String {
    if value == 0 {
        "0".to_string()
    } else {
        format!("{value:#x}")
    }
}

/// A line on the runtime's standard output. On Linux it goes through C `printf`, so it keeps
/// its place among the C runtime's lines in a redirected log.
pub(crate) fn say(line: &str) {
    #[cfg(all(any(target_os = "linux", target_os = "android"), not(test)))]
    if let Ok(text) = std::ffi::CString::new(line) {
        // SAFETY: both pointers are NUL-terminated strings that outlive the call, and the
        // format consumes exactly one string argument.
        unsafe { libc::printf(c"%s\n".as_ptr(), text.as_ptr()) };
        return;
    }
    println!("{line}");
}

/// The window `sceKernelAllocateDirectMemory` searches: the games search up to the retail pool
/// size, and a raised pool (`BB_DMEM_MB`) extends that search so the larger render target
/// heaps fit (scripts/render_res.py). Returns the possibly raised `high`.
pub fn search_high(pool: u64, high: i64) -> u64 {
    let high = high as u64;
    if (RETAIL_POOL..pool).contains(&high) {
        pool
    } else {
        high
    }
}

/// The checks `sceKernelAllocateDirectMemory` makes before taking the lock: `(low, end,
/// alignment)` for [`Space::allocate_direct`], or the error. `NO_SPACE` from here is the
/// refusal the runtime logs.
pub fn direct_window(
    pool: u64,
    low: i64,
    high: i64,
    size: u64,
    alignment: u64,
    memory_type: i32,
) -> Result<(u64, u64, u64), i32> {
    let alignment = if alignment == 0 { PAGE } else { alignment };
    if low < 0
        || high < 0
        || size == 0
        || !size.is_multiple_of(PAGE)
        || !valid_alignment(pool, alignment)
        || !(0..=10).contains(&memory_type)
    {
        return Err(INVALID);
    }
    let end = search_high(pool, high).min(pool);
    let low = low as u64;
    if low >= end || size > end - low {
        return Err(NO_SPACE);
    }
    Ok((low, end, alignment))
}

/// The log line for a refused direct allocation.
pub fn refusal_line(pool: u64, low: i64, high: i64, size: u64) -> String {
    format!(
        "Runtime: direct allocation of {size} bytes in [{}, {}) refused: no space (pool {pool} bytes)",
        alternate_hex(low as u64),
        alternate_hex(search_high(pool, high)),
    )
}

fn valid_alignment(pool: u64, alignment: u64) -> bool {
    alignment >= PAGE && alignment <= pool && alignment.is_power_of_two()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hook {
    Map,
    Unmap,
    Invalidate,
}

/// The region table, the direct-memory blocks and the flexible allocator over a [`Host`].
/// Every method is the C's "locked" form: [`Memory`] holds the lock around it.
pub struct Space<H> {
    host: H,
    pool_size: u64,
    pooled: bool,
    trace: bool,
    blocks: Vec<Block>,
    vmas: Vec<Vma>,
    flex: Vec<u64>,
    hooks: [Option<GpuRange>; 3],
    pending: VecDeque<(Hook, u64, u64)>,
    stats: Stats,
    queries: AtomicUsize,
}

impl<H: Host> Space<H> {
    /// An empty address space with a direct pool of `pool_size` bytes.
    pub fn new(pool_size: u64, host: H) -> Self {
        Self {
            host,
            pool_size,
            pooled: false,
            trace: false,
            blocks: Vec::new(),
            vmas: Vec::new(),
            flex: vec![0; (FLEX_SPAN / PAGE / 64) as usize],
            hooks: [None; 3],
            pending: VecDeque::new(),
            stats: Stats::default(),
            queries: AtomicUsize::new(0),
        }
    }

    /// With `BB_MEMORY_LOG=1`'s trace of reservations and batch entries.
    pub fn with_trace(mut self, trace: bool) -> Self {
        self.trace = trace;
        self
    }

    pub fn trace(&self) -> bool {
        self.trace
    }

    pub fn pool_size(&self) -> u64 {
        self.pool_size
    }

    pub fn host(&self) -> &H {
        &self.host
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    pub fn regions(&self) -> &[Vma] {
        &self.vmas
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    fn ensure_pool(&mut self) -> bool {
        if !self.pooled {
            self.pooled = self.host.pool(self.pool_size + FLEX_SPAN);
        }
        self.pooled
    }

    /// First region with `end > address`.
    fn vma_index(&self, address: u64) -> usize {
        self.vmas.partition_point(|v| v.end <= address)
    }

    fn split_at(&mut self, address: u64) {
        let i = self.vma_index(address);
        let Some(&left) = self.vmas.get(i) else {
            return;
        };
        if left.start >= address {
            return;
        }
        // Direct and flexible mappings both sit on the backing at a byte offset: the right half
        // continues from where the left half stops (write_backing and drop_range read phys).
        let mut right = left;
        right.start = address;
        if left.kind != Kind::Reserved {
            right.phys += address - left.start;
        }
        self.vmas[i].end = address;
        self.vmas.insert(i + 1, right);
    }

    /// `[start, end)` split on its edges; returns the first index inside it.
    fn carve(&mut self, start: u64, end: u64) -> usize {
        self.split_at(start);
        self.split_at(end);
        self.vma_index(start)
    }

    fn covered(&self, start: u64, end: u64, allow_reserved: bool) -> bool {
        let mut i = self.vma_index(start);
        let mut at = start;
        while at < end {
            match self.vmas.get(i) {
                Some(v) if v.start <= at && (allow_reserved || v.kind != Kind::Reserved) => {
                    at = v.end;
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    fn overlaps(&self, start: u64, end: u64, ignore_reserved: bool) -> bool {
        self.vmas[self.vma_index(start)..]
            .iter()
            .take_while(|v| v.start < end)
            .any(|v| !ignore_reserved || v.kind != Kind::Reserved)
    }

    /// First fit in the PS4 user range, starting from the hint; 0 when nothing fits.
    fn find_free(&self, hint: u64, size: u64, alignment: u64) -> u64 {
        let mut at = align_up(hint.max(USER_MIN), alignment);
        let mut i = self.vma_index(at);
        while at.wrapping_add(size) <= USER_MAX {
            match self.vmas.get(i) {
                Some(v) if v.start < at.wrapping_add(size) => at = align_up(v.end, alignment),
                _ => return at,
            }
            i += 1;
        }
        0
    }

    fn flex_test(&self, page: u64) -> bool {
        (self.flex[(page / 64) as usize] >> (page % 64)) & 1 != 0
    }

    fn flex_set(&mut self, first: u64, count: u64, used: bool) {
        for page in first..first + count {
            let (word, bit) = ((page / 64) as usize, page % 64);
            if used {
                self.flex[word] |= 1 << bit;
            } else {
                self.flex[word] &= !(1 << bit);
            }
        }
    }

    fn flex_alloc(&mut self, size: u64) -> Option<u64> {
        let pages = size / PAGE;
        let mut run = 0;
        for page in 0..FLEX_SPAN / PAGE {
            run = if self.flex_test(page) { 0 } else { run + 1 };
            if run == pages {
                let first = page + 1 - pages;
                self.flex_set(first, pages, true);
                return Some(self.pool_size + first * PAGE);
            }
        }
        None
    }

    fn flex_free(&mut self, phys: u64, size: u64) {
        self.flex_set((phys - self.pool_size) / PAGE, size / PAGE, false);
        self.host.discard(phys, size);
    }

    fn drop_range(&mut self, start: u64, end: u64) {
        let first = self.carve(start, end);
        let mut n = 0;
        while let Some(&v) = self.vmas.get(first + n) {
            if v.end > end || v.start < start {
                break;
            }
            if v.kind == Kind::Flexible {
                self.stats.flexible_bytes -= v.end - v.start;
                self.flex_free(v.phys, v.end - v.start);
            }
            n += 1;
        }
        self.vmas.drain(first..first + n);
    }

    fn queue_hook(&mut self, hook: Hook, address: u64, size: u64) {
        // One unmap can cover many mappings (games map video memory in 1 MiB pieces).
        if self.hooks[hook as usize].is_some() {
            self.pending.push_back((hook, address, size));
        }
    }

    /// Places a host mapping; with `MAP_FIXED` it replaces what the guest had there.
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        want: u64,
        size: u64,
        prot: i32,
        flags: i32,
        alignment: u64,
        kind: Kind,
        memory_type: i32,
        phys: u64,
    ) -> Result<u64, i32> {
        let address = if flags & MAP_FIXED != 0 {
            if want == 0 || !want.is_multiple_of(PAGE) {
                return Err(INVALID);
            }
            if flags & MAP_NO_OVERWRITE != 0 && self.overlaps(want, want.wrapping_add(size), true) {
                return Err(NO_MEMORY);
            }
            want
        } else {
            match self.find_free(want, size, alignment) {
                0 => return Err(NO_MEMORY),
                address => address,
            }
        };
        let reserved = kind == Kind::Reserved;
        if !self
            .host
            .map(address, size, host_prot(prot), reserved, phys)
        {
            return Err(NO_MEMORY);
        }
        let end = address.wrapping_add(size);
        self.drop_range(address, end);
        let at = self.vma_index(address);
        self.vmas.insert(
            at,
            Vma {
                start: address,
                end,
                kind,
                prot,
                memory_type,
                phys,
            },
        );
        if kind == Kind::Flexible {
            self.stats.flexible_bytes += size;
        }
        if !reserved {
            self.queue_hook(Hook::Map, address, size);
        }
        Ok(address)
    }

    fn block_type(&self, phys: u64) -> i32 {
        self.blocks
            .iter()
            .find(|b| b.contains(phys))
            .map_or(-1, |b| b.memory_type)
    }

    fn direct_range_allocated(&self, phys: u64, size: u64) -> bool {
        let mut at = phys;
        while at < phys.wrapping_add(size) {
            match self.blocks.iter().find(|b| b.contains(at)) {
                Some(b) => at = b.end(),
                None => return false,
            }
        }
        true
    }

    /// `sceKernelAllocateDirectMemory` after [`direct_window`]: the first `alignment`-aligned
    /// free range of `size` bytes in `[low, end)` of the pool.
    pub fn allocate_direct(
        &mut self,
        low: u64,
        end: u64,
        size: u64,
        alignment: u64,
        memory_type: i32,
    ) -> Result<u64, i32> {
        if !self.ensure_pool() {
            return Err(NO_MEMORY);
        }
        let mut start = align_up(low, alignment);
        let slot = self.blocks.len() < BLOCK_LIMIT;
        loop {
            if !slot || start > end || size > end - start {
                return Err(NO_SPACE);
            }
            match self
                .blocks
                .iter()
                .find(|b| start < b.end() && b.start < start + size)
            {
                Some(b) => start = align_up(b.end(), alignment),
                None => break,
            }
        }
        // Claimed before the commit (a host may drop the lock while it commits).
        self.blocks.push(Block {
            start,
            size,
            memory_type,
        });
        if !self.host.commit(start, size) {
            self.blocks.pop();
            return Err(NO_MEMORY);
        }
        self.stats.allocations += 1;
        self.stats.live_bytes += size;
        Ok(start)
    }

    /// `sceKernelMapDirectMemory`: maps `size` bytes of allocated pool at `physical`.
    pub fn map_direct(
        &mut self,
        want: u64,
        size: u64,
        prot: i32,
        flags: i32,
        physical: i64,
        alignment: u64,
    ) -> Result<u64, i32> {
        let alignment = if alignment == 0 { PAGE } else { alignment };
        if physical < 0
            || !(physical as u64).is_multiple_of(PAGE)
            || size == 0
            || !size.is_multiple_of(PAGE)
            || !valid_alignment(self.pool_size, alignment)
            || prot & !PROT_MASK != 0
        {
            return Err(INVALID);
        }
        let phys = physical as u64;
        if !self.direct_range_allocated(phys, size) {
            return Err(INVALID);
        }
        let memory_type = self.block_type(phys);
        let address = self.place(
            want,
            size,
            prot,
            flags,
            alignment,
            Kind::Direct,
            memory_type,
            phys,
        )?;
        self.stats.maps += 1;
        Ok(address)
    }

    /// `sceKernelMunmap`: unmaps `[start, start + size)` rounded up to pages.
    pub fn unmap(&mut self, start: u64, size: u64) -> i32 {
        if !start.is_multiple_of(PAGE) || size == 0 {
            return INVALID;
        }
        let end = start.wrapping_add(align_up(size, PAGE));
        let first = self.vma_index(start);
        let mut i = first;
        while let Some(&v) = self.vmas.get(i) {
            if v.start >= end {
                break;
            }
            if v.kind != Kind::Reserved {
                let (a, b) = (v.start.max(start), v.end.min(end));
                self.queue_hook(Hook::Unmap, a, b - a);
            }
            i += 1;
        }
        for v in &self.vmas[first..i] {
            let (a, b) = (v.start.max(start), v.end.min(end));
            if !self.host.unmap(a, b - a) {
                return INVALID;
            }
        }
        self.drop_range(start, end);
        0
    }

    /// `sceKernelReleaseDirectMemory`: frees `[start, start + size)` of the pool. Mappings of
    /// the released pages disappear with them, and the pages read as zero when reused.
    pub fn release_direct(&mut self, start: u64, size: u64) -> i32 {
        if !start.is_multiple_of(PAGE) || !size.is_multiple_of(PAGE) {
            return INVALID;
        }
        if size == 0 {
            return 0;
        }
        if !self.direct_range_allocated(start, size) {
            return INVALID;
        }
        let stop = start.wrapping_add(size);
        let mut i = 0;
        while let Some(&v) = self.vmas.get(i) {
            let length = v.end - v.start;
            if v.kind == Kind::Direct && v.phys < stop && start < v.phys + length {
                let (a, b) = (v.phys.max(start), (v.phys + length).min(stop));
                self.unmap(v.start + (a - v.phys), b - a);
                i = self.vma_index(v.start);
                continue;
            }
            i += 1;
        }
        let mut j = 0;
        while let Some(&b) = self.blocks.get(j) {
            if b.start >= stop || start >= b.end() {
                j += 1;
                continue;
            }
            let (a, e) = (b.start.max(start), b.end().min(stop));
            let left = Block {
                size: a - b.start,
                ..b
            };
            let right = Block {
                start: e,
                size: b.end() - e,
                ..b
            };
            if left.size != 0 {
                self.blocks[j] = left;
                j += 1;
            } else {
                self.blocks.remove(j);
            }
            // The C table had a fixed number of slots: a right part with no slot left is lost.
            if right.size != 0 && self.blocks.len() < BLOCK_LIMIT {
                self.blocks.push(right);
            }
            self.stats.live_bytes -= e - a;
        }
        self.host.discard(start, size);
        0
    }

    /// `sceKernelMapFlexibleMemory`: `size` bytes of the flexible budget, mapped.
    pub fn map_flexible(
        &mut self,
        want: u64,
        size: u64,
        prot: i32,
        flags: i32,
    ) -> Result<u64, i32> {
        if size == 0 || !size.is_multiple_of(PAGE) || prot & !PROT_MASK != 0 {
            return Err(INVALID);
        }
        if self.stats.flexible_bytes.wrapping_add(size) > FLEXIBLE_SIZE || !self.ensure_pool() {
            return Err(NO_MEMORY);
        }
        let phys = self.flex_alloc(size).ok_or(NO_MEMORY)?;
        if !self.host.commit(phys, size) {
            self.flex_set((phys - self.pool_size) / PAGE, size / PAGE, false);
            return Err(NO_MEMORY);
        }
        match self.place(want, size, prot, flags, PAGE, Kind::Flexible, 0, phys) {
            Ok(address) => {
                self.stats.flexible_maps += 1;
                Ok(address)
            }
            Err(error) => {
                self.flex_free(phys, size);
                Err(error)
            }
        }
    }

    /// `sceKernelReserveVirtualRange`: an inaccessible range no other placement takes.
    pub fn reserve(
        &mut self,
        want: u64,
        size: u64,
        flags: i32,
        alignment: u64,
    ) -> Result<u64, i32> {
        let alignment = if alignment == 0 { PAGE } else { alignment };
        if size == 0 || !size.is_multiple_of(PAGE) || !valid_alignment(self.pool_size, alignment) {
            return Err(INVALID);
        }
        if flags & MAP_FIXED != 0 && self.overlaps(want, want.wrapping_add(size), false) {
            return Err(NO_MEMORY);
        }
        if self.trace {
            say(&format!(
                "Memory: reserve want={} size=0x{size:x} flags=0x{:x} align=0x{alignment:x}",
                pointer(want),
                flags as u32
            ));
        }
        self.place(want, size, 0, flags, alignment, Kind::Reserved, 0, 0)
    }

    /// `sceKernelMprotect` (`memory_type` < 0) and batch protection: the whole range must be
    /// mapped.
    pub fn protect(&mut self, start: u64, size: u64, prot: i32, memory_type: i32) -> i32 {
        let end = start.wrapping_add(align_up(size, PAGE));
        let start = start & !(PAGE - 1);
        if prot & !PROT_MASK != 0 || !self.covered(start, end, false) {
            return INVALID;
        }
        if !self
            .host
            .protect(start, end.wrapping_sub(start), host_prot(prot))
        {
            return INVALID;
        }
        let mut i = self.carve(start, end);
        while let Some(v) = self.vmas.get_mut(i) {
            if v.start >= end {
                break;
            }
            v.prot = prot;
            if memory_type >= 0 {
                v.memory_type = memory_type;
            }
            i += 1;
        }
        self.queue_hook(Hook::Invalidate, start, end.wrapping_sub(start));
        self.stats.protects += 1;
        0
    }

    /// `sceKernelQueryMemoryProtection`: the whole contiguous run of mappings with the
    /// protection of the one at `address`, as `(start, end, prot)`.
    pub fn query_protection(&self, address: u64) -> Result<(u64, u64, u32), i32> {
        let i = self.vma_index(address);
        let Some(v) = self.vmas.get(i) else {
            return Err(ACCESS);
        };
        if v.start > address || v.kind == Kind::Reserved {
            return Err(ACCESS);
        }
        let joins = |a: &Vma, b: &Vma| {
            a.end == b.start && a.kind != Kind::Reserved && b.kind != Kind::Reserved
        };
        let (mut first, mut last) = (i, i);
        while first > 0
            && joins(&self.vmas[first - 1], &self.vmas[first])
            && self.vmas[first - 1].prot == v.prot
        {
            first -= 1;
        }
        while last + 1 < self.vmas.len()
            && joins(&self.vmas[last], &self.vmas[last + 1])
            && self.vmas[last + 1].prot == v.prot
        {
            last += 1;
        }
        self.queries.fetch_add(1, Ordering::Relaxed);
        Ok((self.vmas[first].start, self.vmas[last].end, v.prot as u32))
    }

    /// `sceKernelVirtualQuery`: the region at `address`, or with flag 1 the next one after it.
    pub fn virtual_query(&self, address: u64, flags: i32) -> Result<VirtualQueryInfo, i32> {
        let Some(&v) = self.vmas.get(self.vma_index(address)) else {
            return Err(ACCESS);
        };
        if v.start > address && flags & 1 == 0 {
            return Err(ACCESS);
        }
        self.queries.fetch_add(1, Ordering::Relaxed);
        let (flags, label): (u32, &[u8]) = match v.kind {
            Kind::Reserved => (0, b"reserved"),
            Kind::Direct => (2 | 16, b"direct"),
            Kind::Flexible => (1 | 16, b"flexible"),
        };
        let mut name = [0u8; 32];
        name[..label.len()].copy_from_slice(label);
        Ok(VirtualQueryInfo {
            start: v.start,
            end: v.end,
            offset: if v.kind == Kind::Direct { v.phys } else { 0 },
            protection: v.prot,
            memory_type: v.memory_type,
            flags,
            name,
            padding: [0; 4],
        })
    }

    /// `sceKernelGetDirectMemoryType`: the block holding `phys`, as `(type, start, end)`.
    pub fn direct_memory_type(&self, phys: u64) -> Result<(i32, u64, u64), i32> {
        self.blocks
            .iter()
            .find(|b| b.contains(phys))
            .map(|b| (b.memory_type, b.start, b.end()))
            .ok_or(ACCESS)
    }

    /// One `sceKernelBatchMap2` entry; the address the entry reports afterwards.
    pub fn batch_entry(&mut self, entry: &BatchEntry, flags: i32) -> Result<u64, i32> {
        let prot = i32::from(entry.prot as u8);
        let (start, length) = (entry.start, entry.length);
        let done = |code: i32| if code == 0 { Ok(start) } else { Err(code) };
        match entry.operation {
            0 => self.map_direct(start, length, prot, flags, entry.offset as i64, 0),
            1 => done(self.unmap(start, length)),
            2 => done(self.protect(start, length, prot, -1)),
            3 => self.map_flexible(start, length, prot, flags),
            4 => done(self.protect(start, length, prot, i32::from(entry.memory_type))),
            _ => Err(INVALID),
        }
    }

    /// The region holding `address`, if any.
    pub fn containing(&self, address: u64) -> Option<Vma> {
        self.vmas
            .get(self.vma_index(address))
            .filter(|v| v.start <= address)
            .copied()
    }

    /// Mapped length from `address`, up to `size`, across adjacent mappings.
    pub fn clamp(&self, address: u64, size: u64) -> u64 {
        let mut length = 0;
        for v in &self.vmas[self.vma_index(address)..] {
            if length >= size || v.start > address + length || v.kind == Kind::Reserved {
                break;
            }
            length = v.end - address;
        }
        length.min(size)
    }

    /// The region around `address` as `(start, end, mapped)`: a mapping (reserved ranges are
    /// not mapped) or the gap before the next one; `None` past the last region.
    pub fn region(&self, address: u64) -> Option<(u64, u64, bool)> {
        let i = self.vma_index(address);
        let v = self.vmas.get(i)?;
        if v.start <= address {
            Some((v.start, v.end, v.kind != Kind::Reserved))
        } else {
            let start = if i > 0 { self.vmas[i - 1].end } else { 0 };
            Some((start, v.start, false))
        }
    }

    /// Whether `[address, address + size)` is all mapped (reserved ranges are not).
    pub fn is_mapped(&self, address: u64, size: u64) -> bool {
        self.covered(address, address.wrapping_add(size), false)
    }

    /// GPU page tracking: restricts host access, never beyond the guest's own protection.
    pub fn gpu_protect(&self, address: u64, size: u64, read: bool, write: bool) {
        let end = address.wrapping_add(size);
        for v in &self.vmas[self.vma_index(address)..] {
            if v.start >= end {
                break;
            }
            if v.kind == Kind::Reserved {
                continue;
            }
            let (a, b) = (v.start.max(address), v.end.min(end));
            let guest = host_prot(v.prot);
            let mut want = guest & HOST_EXEC;
            if read {
                want |= HOST_READ;
            }
            if write {
                want |= HOST_WRITE | HOST_READ;
            }
            self.host.protect(a, b - a, guest & want);
        }
    }

    /// Walks `[address, address + size)` through the backing: `copy(phys, offset, n)` for each
    /// mapped piece, `offset` from `address`. False at the first byte with no backing (the
    /// pieces before it are already passed on).
    pub fn for_each_backing(
        &self,
        address: u64,
        size: u64,
        mut copy: impl FnMut(u64, u64, u64),
    ) -> bool {
        let end = address.wrapping_add(size);
        let mut at = address;
        while at < end {
            match self.containing(at) {
                Some(v) if v.kind != Kind::Reserved => {
                    let n = v.end.min(end) - at;
                    copy(v.phys + (at - v.start), at - address, n);
                    at += n;
                }
                _ => return false,
            }
        }
        true
    }

    /// The GPU library's notifications; every mapping the guest already has is announced.
    pub fn set_hooks(
        &mut self,
        map: Option<GpuRange>,
        unmap: Option<GpuRange>,
        invalidate: Option<GpuRange>,
    ) {
        self.hooks = [map, unmap, invalidate];
        for i in 0..self.vmas.len() {
            let v = self.vmas[i];
            if v.kind != Kind::Reserved {
                self.queue_hook(Hook::Map, v.start, v.end - v.start);
            }
        }
    }

    /// The unmap notification, called before an unmap takes the lock.
    pub fn unmap_hook(&self) -> Option<GpuRange> {
        self.hooks[Hook::Unmap as usize]
    }

    /// The oldest queued notification: its hook (now; it may have been cleared) and range.
    pub fn next_hook(&mut self) -> Option<(Option<GpuRange>, u64, u64)> {
        let (hook, address, size) = self.pending.pop_front()?;
        Some((self.hooks[hook as usize], address, size))
    }

    pub fn pending_hooks(&self) -> usize {
        self.pending.len()
    }

    /// Drops the notifications queued after the first `n` (sent already).
    pub fn forget_hooks_after(&mut self, n: usize) {
        self.pending.truncate(n);
    }

    /// The two `runtime_memory_report` lines.
    pub fn report(&self) -> [String; 2] {
        let s = self.stats;
        [
            format!(
                "Runtime: direct memory allocations={}, maps={}, live={}, budget={}",
                s.allocations, s.maps, s.live_bytes, self.pool_size
            ),
            format!(
                "Runtime: flexible maps={}, in use={}, protects={}, queries={}, regions={}",
                s.flexible_maps,
                s.flexible_bytes,
                s.protects,
                self.queries.load(Ordering::Relaxed),
                self.vmas.len()
            ),
        ]
    }
}

thread_local! {
    /// How deep this thread holds a [`Memory`] lock exclusively; nested calls skip relocking.
    static EXCLUSIVE_DEPTH: Cell<u32> = const { Cell::new(0) };
    /// Recently found regions for the GPU's per-draw queries.
    static REGION_CACHE: RefCell<RegionCache> = const { RefCell::new(RegionCache::EMPTY) };
}

#[derive(Debug, Clone, Copy)]
struct CachedRegion {
    owner: u64,
    generation: u64,
    start: u64,
    end: u64,
    mapped: bool,
}

struct RegionCache {
    entries: [CachedRegion; 8],
    next: usize,
}

impl RegionCache {
    const EMPTY: RegionCache = RegionCache {
        entries: [CachedRegion {
            owner: 0,
            generation: 0,
            start: 0,
            end: 0,
            mapped: false,
        }; 8],
        next: 0,
    };
}

/// A [`Space`] under the region table lock.
///
/// A thread that holds it exclusively skips relocking, and its shared lookups then read the
/// table directly; that happens only from a fault handler that interrupted the holder between
/// two table changes (a guest batch entry read). The generation is odd while a writer holds
/// the lock; a cached region is valid for one even value. Guest-visible results are written
/// after the lock is dropped: a write to a GPU-tracked page faults, and the fault handler
/// waits for locks the GPU thread may hold while it waits for this one.
pub struct Memory<H> {
    id: u64,
    lock: RwLock<()>,
    generation: AtomicU64,
    space: UnsafeCell<Space<H>>,
}

// SAFETY: the space is reached only through `write` (the exclusive lock) and `read` (the shared
// lock). What changes under the shared lock is atomic (the query counter) or a host call that
// must be thread-safe (`Host::protect`); `H: Send + Sync` covers both.
unsafe impl<H: Send + Sync> Sync for Memory<H> {}

impl<H: Host> Memory<H> {
    pub fn new(space: Space<H>) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            lock: RwLock::new(()),
            generation: AtomicU64::new(0),
            space: UnsafeCell::new(space),
        }
    }

    /// Runs `f` holding the lock exclusively.
    pub fn write<R>(&self, f: impl FnOnce(&mut Space<H>) -> R) -> R {
        let depth = EXCLUSIVE_DEPTH.get();
        let guard = (depth == 0).then(|| {
            let guard = self.lock.write().unwrap_or_else(PoisonError::into_inner);
            self.generation.fetch_add(1, Ordering::AcqRel);
            guard
        });
        EXCLUSIVE_DEPTH.set(depth + 1);
        // SAFETY: this thread holds the exclusive lock (taken above, or by the caller up the
        // stack, which is paused in a fault handler between table changes; see the type).
        let result = f(unsafe { &mut *self.space.get() });
        EXCLUSIVE_DEPTH.set(depth);
        if let Some(guard) = guard {
            self.generation.fetch_add(1, Ordering::Release);
            drop(guard);
        }
        result
    }

    /// Runs `f` holding the lock shared (or exclusively already, on this thread).
    pub fn read<R>(&self, f: impl FnOnce(&Space<H>) -> R) -> R {
        let guard = (EXCLUSIVE_DEPTH.get() == 0)
            .then(|| self.lock.read().unwrap_or_else(PoisonError::into_inner));
        // SAFETY: this thread holds the lock shared, or exclusively (see `write`).
        let result = f(unsafe { &*self.space.get() });
        drop(guard);
        result
    }

    /// The table generation: odd while a writer holds the lock.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn cached(&self, address: u64) -> Option<CachedRegion> {
        if DISABLED_OPTIMIZATIONS.load(Ordering::Relaxed) & 1 != 0 {
            return None;
        }
        let generation = self.generation();
        if generation & 1 != 0 {
            return None;
        }
        let owner = self.id;
        REGION_CACHE.with(|cache| {
            cache.borrow().entries.iter().copied().find(|c| {
                c.owner == owner
                    && c.generation == generation
                    && c.end != 0
                    && c.start <= address
                    && address < c.end
            })
        })
    }

    /// Called under the lock after a lookup found `vma`.
    fn remember(&self, vma: Option<Vma>) {
        let generation = self.generation();
        let Some(v) = vma else {
            return;
        };
        if generation & 1 != 0 {
            return;
        }
        let region = CachedRegion {
            owner: self.id,
            generation,
            start: v.start,
            end: v.end,
            mapped: v.kind != Kind::Reserved,
        };
        REGION_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let at = cache.next % 8;
            cache.entries[at] = region;
            cache.next = cache.next.wrapping_add(1);
        });
    }

    /// `runtime_memory_clamp`: mapped length from `address`, up to `size`.
    pub fn clamp(&self, address: u64, size: u64) -> u64 {
        if let Some(c) = self.cached(address) {
            if c.mapped && size <= c.end - address {
                return size;
            }
        }
        self.read(|s| {
            self.remember(s.containing(address));
            s.clamp(address, size)
        })
    }

    /// `runtime_memory_region`: [`Space::region`], through the cache.
    pub fn region(&self, address: u64) -> Option<(u64, u64, bool)> {
        if let Some(c) = self.cached(address) {
            return Some((c.start, c.end, c.mapped));
        }
        self.read(|s| {
            self.remember(s.containing(address));
            s.region(address)
        })
    }

    /// `runtime_memory_is_mapped`, through the cache.
    pub fn is_mapped(&self, address: u64, size: u64) -> bool {
        if let Some(c) = self.cached(address) {
            if c.mapped && size <= c.end - address {
                return true;
            }
        }
        self.read(|s| s.is_mapped(address, size))
    }

    /// Runs the queued GPU notifications, each outside the lock.
    pub fn flush_hooks(&self) {
        while let Some((hook, address, size)) = self.write(Space::next_hook) {
            if let Some(hook) = hook {
                hook(address as usize, size);
            }
        }
    }
}

/// Guest import NIDs (`src/import_names.inc`) of the functions this module exports, with the
/// symbol each names; `runtime_memory_resolve` answers exactly these. A test checks the list
/// against the file. `sceKernelMapNamedDirectMemory` and `sceKernelBatchMap2` are exported but
/// have no NID there yet, as in the C.
pub const NIDS: &[(&str, &str)] = &[
    ("pO96TwzOm5E#p#J", "sceKernelGetDirectMemorySize"),
    ("rTXw65xmLIA#p#J", "sceKernelAllocateDirectMemory"),
    ("L-Q3LEjIbgA#p#J", "sceKernelMapDirectMemory"),
    ("MBuItvba6z8#p#J", "sceKernelReleaseDirectMemory"),
    ("WFcfL2lzido#p#J", "sceKernelQueryMemoryProtection"),
    ("rVjRvHJ0X6c#p#J", "sceKernelVirtualQuery"),
    ("BC+OG5m9+bw#p#J", "sceKernelGetDirectMemoryType"),
    ("2SKEx6bSq-4#p#J", "sceKernelBatchMap"),
    ("cQke9UuBQOk#p#J", "sceKernelMunmap"),
    ("7oxv3PPCumo#p#J", "sceKernelReserveVirtualRange"),
    ("vSMAm3cxYTY#p#J", "sceKernelMprotect"),
    ("IWIBBdTHit4#p#J", "sceKernelMapFlexibleMemory"),
    ("teiItL2boFw#p#J", "sceKernelReleaseFlexibleMemory"),
    ("mL8NDH86iQI#p#J", "sceKernelMapNamedFlexibleMemory"),
    ("UqDGjXA5yUM#p#J", "munmap"),
];

/// Every guest symbol this module exports (the C `exports` table).
pub const SYMBOLS: &[&str] = &[
    "sceKernelGetDirectMemorySize",
    "sceKernelAllocateDirectMemory",
    "sceKernelMapDirectMemory",
    "sceKernelMapNamedDirectMemory",
    "sceKernelReleaseDirectMemory",
    "sceKernelMunmap",
    "munmap",
    "sceKernelMapFlexibleMemory",
    "sceKernelMapNamedFlexibleMemory",
    "sceKernelReleaseFlexibleMemory",
    "sceKernelReserveVirtualRange",
    "sceKernelMprotect",
    "sceKernelQueryMemoryProtection",
    "sceKernelVirtualQuery",
    "sceKernelGetDirectMemoryType",
    "sceKernelBatchMap",
    "sceKernelBatchMap2",
];

/// The symbol a guest import NID names, when this module exports it.
pub fn nid_symbol(nid: &[u8]) -> Option<&'static str> {
    NIDS.iter()
        .find(|(n, _)| n.as_bytes() == nid)
        .map(|&(_, symbol)| symbol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const MIB: u64 = 1 << 20;
    const POOL: u64 = RETAIL_POOL;

    /// Accepts every host call and records the ones a test asks about.
    #[derive(Default)]
    struct Fake {
        refuse_map: bool,
        calls: Mutex<Vec<String>>,
    }

    impl Fake {
        fn log(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }
        fn calls(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.lock().unwrap())
        }
    }

    impl Host for Fake {
        fn pool(&mut self, bytes: u64) -> bool {
            self.log(format!("pool {bytes:#x}"));
            true
        }
        fn map(&self, address: u64, size: u64, prot: i32, reserved: bool, phys: u64) -> bool {
            self.log(format!(
                "map {address:#x} {size:#x} prot={prot} reserved={reserved} phys={phys:#x}"
            ));
            !self.refuse_map
        }
        fn unmap(&self, address: u64, size: u64) -> bool {
            self.log(format!("unmap {address:#x} {size:#x}"));
            true
        }
        fn protect(&self, address: u64, size: u64, prot: i32) -> bool {
            self.log(format!("protect {address:#x} {size:#x} prot={prot}"));
            true
        }
        fn discard(&self, phys: u64, size: u64) {
            self.log(format!("discard {phys:#x} {size:#x}"));
        }
    }

    fn space() -> Space<Fake> {
        Space::new(POOL, Fake::default())
    }

    /// `sceKernelAllocateDirectMemory` as the guest calls it, before and under the lock.
    fn allocate(
        s: &mut Space<Fake>,
        low: i64,
        high: i64,
        size: u64,
        alignment: u64,
        memory_type: i32,
    ) -> Result<u64, i32> {
        let (low, end, alignment) =
            direct_window(s.pool_size(), low, high, size, alignment, memory_type)?;
        s.allocate_direct(low, end, size, alignment, memory_type)
    }

    /// `test_runtime.c` `direct_memory`: the layout and error codes (the shared backing and
    /// zero-on-reuse are the Linux tests in `memory::linux`).
    #[test]
    fn direct_memory_allocate_map_unmap_release_as_the_c_test() {
        let mut s = space();
        let (pool, length) = (536_870_912i64, 16384u64);
        assert_eq!(allocate(&mut s, 0, pool, 1, 0, 0), Err(INVALID));
        assert_eq!(allocate(&mut s, 0, 8192, length, 0, 0), Err(NO_SPACE));
        let a = allocate(&mut s, 0, pool, length, 2 * MIB, 0).unwrap();
        let b = allocate(&mut s, 0, pool, length, 2 * MIB, 0).unwrap();
        let c = allocate(&mut s, 0, pool, length, 16384, 0).unwrap();
        assert_eq!((a, b, c), (0, 2 * MIB, 16384));
        assert_eq!(s.map_direct(0, length * 2, 3, 0, b as i64, 0), Err(INVALID));
        let x = s.map_direct(0, length, 3, 0, a as i64, 2 * MIB).unwrap();
        assert_eq!(x % (2 * MIB), 0);
        let y = s.map_direct(0, length, 3, 0, a as i64, 0).unwrap();
        assert_ne!(x, y);
        assert_eq!(s.unmap(x + 4096, length), INVALID);
        assert_eq!((s.unmap(x, length), s.unmap(y, length)), (0, 0));
        for start in [a, b, c] {
            assert_eq!(s.release_direct(start, length), 0);
        }
        assert_eq!(allocate(&mut s, 0, pool, length, 0, 0), Ok(0));
        s.map_direct(0, length, 3, 0, 0, 0).unwrap();
        assert_eq!(s.release_direct(0, length), 0);
        assert!(s.regions().is_empty(), "release also unmaps the mapping");
        assert_eq!(s.stats().live_bytes, 0);
    }

    #[test]
    fn allocation_checks_arguments_before_the_pool() {
        let pool = POOL as i64;
        for (low, high, size, alignment, memory_type) in [
            (-1, pool, PAGE, 0, 0),
            (0, -1, PAGE, 0, 0),
            (0, pool, 0, 0, 0),
            (0, pool, PAGE + 1, 0, 0),
            (0, pool, PAGE, 4096, 0),
            (0, pool, PAGE, 3 * PAGE, 0),
            (0, pool, PAGE, 2 * POOL, 0),
            (0, pool, PAGE, 0, -1),
            (0, pool, PAGE, 0, 11),
        ] {
            assert_eq!(
                direct_window(POOL, low, high, size, alignment, memory_type),
                Err(INVALID),
                "{low} {high} {size} {alignment} {memory_type}"
            );
        }
        assert_eq!(direct_window(POOL, pool, pool, PAGE, 0, 0), Err(NO_SPACE));
        assert_eq!(
            direct_window(POOL, 0, pool * 2, PAGE, 0, 10),
            Ok((0, POOL, PAGE))
        );
    }

    #[test]
    fn a_raised_pool_extends_a_search_up_to_the_retail_size() {
        let raised = 6000 * MIB;
        let retail = RETAIL_POOL as i64;
        assert_eq!(search_high(raised, retail), raised);
        assert_eq!(search_high(raised, retail - 1), RETAIL_POOL - 1);
        assert_eq!(search_high(POOL, retail), RETAIL_POOL);
        let mut s = Space::new(raised, Fake::default());
        let size = 1024 * MIB;
        let first = allocate(&mut s, 0, retail, 4500 * MIB, 0, 0).unwrap();
        assert_eq!(first, 0);
        // Only 556 MiB of the retail window is left; the raise lets 1 GiB fit above it.
        assert_eq!(allocate(&mut s, 0, retail, size, 0, 0), Ok(4500 * MIB));
        assert_eq!(
            refusal_line(raised, 0, retail, PAGE),
            format!("Runtime: direct allocation of 16384 bytes in [0, {raised:#x}) refused: no space (pool {raised} bytes)")
        );
    }

    #[test]
    fn allocation_steps_past_every_overlapping_block() {
        let mut s = space();
        let high = POOL as i64;
        assert_eq!(allocate(&mut s, 0, high, 4 * PAGE, 0, 1), Ok(0));
        assert_eq!(allocate(&mut s, 0, high, PAGE, 0, 2), Ok(4 * PAGE));
        assert_eq!(s.release_direct(PAGE, PAGE), 0);
        // The hole at one page fits one page, not two.
        assert_eq!(allocate(&mut s, 0, high, 2 * PAGE, 0, 3), Ok(5 * PAGE));
        assert_eq!(allocate(&mut s, 0, high, PAGE, 0, 4), Ok(PAGE));
        assert_eq!(
            allocate(&mut s, (3 * PAGE) as i64, high, PAGE, 0, 5),
            Ok(7 * PAGE)
        );
        assert_eq!(s.direct_memory_type(PAGE), Ok((4, PAGE, 2 * PAGE)));
        assert_eq!(
            s.direct_memory_type(5 * PAGE + 1),
            Ok((3, 5 * PAGE, 7 * PAGE))
        );
        assert_eq!(s.direct_memory_type(8 * PAGE), Err(ACCESS));
    }

    #[test]
    fn the_block_table_holds_4096_blocks() {
        let mut s = space();
        let high = POOL as i64;
        // Each search starts at its own page, so the test does not walk every block each time.
        for i in 0..BLOCK_LIMIT as u64 {
            assert_eq!(
                allocate(&mut s, (i * PAGE) as i64, high, PAGE, 0, 0),
                Ok(i * PAGE)
            );
        }
        let next = (BLOCK_LIMIT as u64 * PAGE) as i64;
        assert_eq!(allocate(&mut s, next, high, PAGE, 0, 0), Err(NO_SPACE));
        // Releasing a whole block frees its slot.
        assert_eq!(s.release_direct(PAGE, PAGE), 0);
        assert_eq!(allocate(&mut s, PAGE as i64, high, PAGE, 0, 0), Ok(PAGE));
        // Releasing the middle of a block needs a second slot: with none left, the part after
        // the released range is lost (the C table did the same).
        assert_eq!(s.release_direct(0, 4 * PAGE), 0);
        let wide = allocate(&mut s, 0, high, 4 * PAGE, 0, 0).unwrap();
        for i in 0..3 {
            allocate(&mut s, next + i * PAGE as i64, high, PAGE, 0, 0).unwrap();
        }
        assert_eq!(s.blocks().len(), BLOCK_LIMIT);
        assert_eq!(s.release_direct(wide + PAGE, PAGE), 0);
        assert_eq!(s.direct_memory_type(wide), Ok((0, wide, wide + PAGE)));
        assert_eq!(s.direct_memory_type(wide + 2 * PAGE), Err(ACCESS));
    }

    #[test]
    fn releasing_the_middle_of_a_block_keeps_both_ends() {
        let mut s = space();
        let start = allocate(&mut s, 0, POOL as i64, 8 * PAGE, 0, 7).unwrap();
        let address = s.map_direct(0, 8 * PAGE, 3, 0, start as i64, 0).unwrap();
        s.host().calls();
        assert_eq!(s.release_direct(start + 2 * PAGE, 3 * PAGE), 0);
        assert_eq!(s.direct_memory_type(start), Ok((7, 0, 2 * PAGE)));
        assert_eq!(s.direct_memory_type(start + 3 * PAGE), Err(ACCESS));
        assert_eq!(
            s.direct_memory_type(start + 6 * PAGE),
            Ok((7, 5 * PAGE, 8 * PAGE))
        );
        assert_eq!(s.stats().live_bytes, 5 * PAGE);
        // The mapping of the released pages is gone; both ends stay mapped on their pages.
        assert!(s.is_mapped(address, 2 * PAGE));
        assert!(!s.is_mapped(address + 2 * PAGE, PAGE));
        let right = s.virtual_query(address + 5 * PAGE, 0).unwrap();
        assert_eq!((right.start, right.offset), (address + 5 * PAGE, 5 * PAGE));
        assert_eq!(
            s.host().calls(),
            [
                format!("unmap {:#x} {:#x}", address + 2 * PAGE, 3 * PAGE),
                format!("discard {:#x} {:#x}", 2 * PAGE, 3 * PAGE),
            ]
        );
        assert_eq!(s.release_direct(start + 2 * PAGE, PAGE), INVALID);
        assert_eq!(s.release_direct(start + 1, PAGE), INVALID);
        assert_eq!(s.release_direct(start, 0), 0);
    }

    #[test]
    fn placement_is_first_fit_from_the_hint_in_the_user_range() {
        let mut s = space();
        let phys = allocate(&mut s, 0, POOL as i64, 4 * MIB, 0, 0).unwrap() as i64;
        let first = s.map_direct(0, MIB, 3, 0, phys, 0).unwrap();
        assert_eq!(first, USER_MIN);
        assert_eq!(s.map_direct(0, MIB, 3, 0, phys, 0), Ok(USER_MIN + MIB));
        // A hint inside a mapping moves past it; alignment rounds the next address up.
        assert_eq!(
            s.map_direct(USER_MIN + PAGE, MIB, 3, 0, phys, 4 * MIB),
            Ok(USER_MIN + 4 * MIB)
        );
        // The gap below the aligned mapping takes an unaligned request that fits it.
        assert_eq!(
            s.map_direct(0, 2 * MIB, 3, 0, phys, 0),
            Ok(USER_MIN + 2 * MIB)
        );
        assert_eq!(
            s.map_direct(0x90_0000_0000, MIB, 3, 0, phys, 0),
            Ok(0x90_0000_0000)
        );
        // Nothing fits past the user range.
        assert_eq!(
            s.map_direct(USER_MAX - MIB, 2 * MIB, 3, 0, phys, 0),
            Err(NO_MEMORY)
        );
    }

    #[test]
    fn fixed_mappings_replace_and_no_overwrite_refuses() {
        let mut s = space();
        let phys = allocate(&mut s, 0, POOL as i64, 4 * MIB, 0, 0).unwrap() as i64;
        let at = USER_MIN + 16 * MIB;
        assert_eq!(
            s.map_direct(at + 1, PAGE, 3, MAP_FIXED, phys, 0),
            Err(INVALID)
        );
        assert_eq!(s.map_direct(0, PAGE, 3, MAP_FIXED, phys, 0), Err(INVALID));
        assert_eq!(s.map_direct(at, 4 * PAGE, 1, MAP_FIXED, phys, 0), Ok(at));
        let both = MAP_FIXED | MAP_NO_OVERWRITE;
        assert_eq!(
            s.map_direct(at + PAGE, PAGE, 3, both, phys, 0),
            Err(NO_MEMORY)
        );
        // MAP_FIXED alone replaces the middle page, splitting the mapping around it.
        assert_eq!(
            s.map_direct(at + PAGE, PAGE, 3, MAP_FIXED, phys + 3 * PAGE as i64, 0),
            Ok(at + PAGE)
        );
        let pieces: Vec<_> = s
            .regions()
            .iter()
            .map(|v| (v.start - at, v.end - at, v.prot, v.phys))
            .collect();
        assert_eq!(
            pieces,
            [
                (0, PAGE, 1, 0),
                (PAGE, 2 * PAGE, 3, 3 * PAGE),
                (2 * PAGE, 4 * PAGE, 1, 2 * PAGE),
            ]
        );
        // A reserved range does not block MAP_NO_OVERWRITE; it is replaced.
        let reserved = s.reserve(at + 8 * PAGE, 4 * PAGE, MAP_FIXED, 0).unwrap();
        assert_eq!(s.map_direct(reserved, PAGE, 3, both, phys, 0), Ok(reserved));
        let refusing = Fake {
            refuse_map: true,
            ..Fake::default()
        };
        let mut host_fail = Space::new(POOL, refusing);
        let p = allocate_on(&mut host_fail, PAGE);
        assert_eq!(host_fail.map_direct(0, PAGE, 3, 0, p, 0), Err(NO_MEMORY));
        assert!(host_fail.regions().is_empty());
    }

    fn allocate_on(s: &mut Space<Fake>, size: u64) -> i64 {
        let (low, end, alignment) =
            direct_window(s.pool_size(), 0, POOL as i64, size, 0, 0).unwrap();
        s.allocate_direct(low, end, size, alignment, 0).unwrap() as i64
    }

    #[test]
    fn mapping_checks_protection_and_the_allocated_range() {
        let mut s = space();
        let phys = allocate_on(&mut s, 2 * PAGE);
        assert_eq!(s.map_direct(0, PAGE, 0x40, 0, phys, 0), Err(INVALID));
        assert_eq!(s.map_direct(0, PAGE, 3, 0, -(PAGE as i64), 0), Err(INVALID));
        assert_eq!(s.map_direct(0, PAGE, 3, 0, phys + 1, 0), Err(INVALID));
        assert_eq!(s.map_direct(0, 3 * PAGE, 3, 0, phys, 0), Err(INVALID));
        assert_eq!(
            s.map_direct(0, PAGE, 3, 0, phys, 2 * PAGE + 1),
            Err(INVALID)
        );
        // Two adjacent blocks map as one range.
        let next = allocate_on(&mut s, PAGE);
        assert_eq!(next, 2 * PAGE as i64);
        let address = s.map_direct(0, 3 * PAGE, 0x33, 0, phys, 0).unwrap();
        let info = s.virtual_query(address, 0).unwrap();
        assert_eq!(
            (info.protection, info.memory_type, info.flags),
            (0x33, 0, 18)
        );
        assert_eq!(&info.name[..7], b"direct\0");
        assert_eq!(
            s.host().calls().last().unwrap(),
            &format!(
                "map {address:#x} {:#x} prot=3 reserved=false phys=0x0",
                3 * PAGE
            )
        );
    }

    #[test]
    fn protection_splits_queries_join_runs() {
        let mut s = space();
        let phys = allocate_on(&mut s, 8 * PAGE);
        let a = s.map_direct(0, 4 * PAGE, 3, 0, phys, 0).unwrap();
        let b = s
            .map_direct(
                a + 4 * PAGE,
                4 * PAGE,
                3,
                MAP_FIXED,
                phys + 4 * PAGE as i64,
                0,
            )
            .unwrap();
        assert_eq!(b, a + 4 * PAGE);
        // One run across the two mappings with the same protection.
        assert_eq!(s.query_protection(a + PAGE), Ok((a, a + 8 * PAGE, 3)));
        // The size is rounded up to pages: pages 1 and 2.
        assert_eq!(s.protect(a + PAGE, 2 * PAGE - 5, 1, -1), 0);
        assert_eq!(s.query_protection(a), Ok((a, a + PAGE, 3)));
        assert_eq!(
            s.query_protection(a + PAGE),
            Ok((a + PAGE, a + 3 * PAGE, 1))
        );
        assert_eq!(
            s.query_protection(a + 3 * PAGE),
            Ok((a + 3 * PAGE, a + 8 * PAGE, 3))
        );
        assert_eq!(s.regions().len(), 4);
        assert_eq!(s.protect(a, PAGE, 0x80, -1), INVALID);
        assert_eq!(s.protect(a + 7 * PAGE, 2 * PAGE, 1, -1), INVALID);
        assert_eq!(s.protect(a + 6 * PAGE, PAGE, 3, 9), 0);
        assert_eq!(s.virtual_query(a + 6 * PAGE, 0).unwrap().memory_type, 9);
        assert_eq!(s.stats().protects, 2);
        assert_eq!(s.query_protection(a + 8 * PAGE), Err(ACCESS));
    }

    #[test]
    fn reserved_ranges_are_not_mapped_memory() {
        let mut s = space();
        assert_eq!(s.reserve(0, PAGE + 1, 0, 0), Err(INVALID));
        assert_eq!(s.reserve(0, PAGE, 0, PAGE / 2), Err(INVALID));
        let r = s.reserve(0, 16 * PAGE, 0, 64 * PAGE).unwrap();
        assert_eq!(r, USER_MIN);
        assert_eq!(
            s.host().calls(),
            [format!(
                "map {r:#x} {:#x} prot=0 reserved=true phys=0x0",
                16 * PAGE
            )]
        );
        assert_eq!(s.reserve(r + PAGE, PAGE, MAP_FIXED, 0), Err(NO_MEMORY));
        assert_eq!(s.query_protection(r), Err(ACCESS));
        assert_eq!(s.protect(r, PAGE, 3, -1), INVALID);
        assert!(!s.is_mapped(r, PAGE));
        assert_eq!(s.clamp(r, PAGE), 0);
        assert_eq!(s.region(r), Some((r, r + 16 * PAGE, false)));
        let info = s.virtual_query(r + PAGE, 0).unwrap();
        assert_eq!((info.flags, info.offset), (0, 0));
        assert_eq!(&info.name[..9], b"reserved\0");
        // Placement skips the reserved range.
        let phys = allocate_on(&mut s, PAGE);
        assert_eq!(s.map_direct(0, PAGE, 3, 0, phys, 0), Ok(r + 16 * PAGE));
        // Unmapping it releases it.
        assert_eq!(s.unmap(r, 16 * PAGE), 0);
        assert_eq!(s.virtual_query(r, 0), Err(ACCESS));
        assert_eq!(s.virtual_query(r, 1).unwrap().start, r + 16 * PAGE);
    }

    #[test]
    fn virtual_query_finds_the_next_region_with_flag_one() {
        let mut s = space();
        let phys = allocate_on(&mut s, PAGE);
        let at = s.map_direct(USER_MIN + MIB, PAGE, 1, 0, phys, 0).unwrap();
        assert_eq!(s.virtual_query(USER_MIN, 0), Err(ACCESS));
        let next = s.virtual_query(USER_MIN, 1).unwrap();
        assert_eq!((next.start, next.end), (at, at + PAGE));
        assert_eq!(s.virtual_query(at + PAGE, 1), Err(ACCESS));
        let s2 = space();
        assert_eq!(s2.virtual_query(0, 1), Err(ACCESS));
    }

    #[test]
    fn flexible_memory_has_a_448_mib_budget() {
        let mut s = space();
        assert_eq!(s.map_flexible(0, 0, 3, 0), Err(INVALID));
        assert_eq!(s.map_flexible(0, PAGE, 0x100, 0), Err(INVALID));
        let a = s.map_flexible(0, 400 * MIB, 3, 0).unwrap();
        assert_eq!(s.map_flexible(0, 64 * MIB, 3, 0), Err(NO_MEMORY));
        let b = s.map_flexible(0, 48 * MIB, 3, 0).unwrap();
        assert_eq!(b, a + 400 * MIB);
        assert_eq!(s.stats().flexible_bytes, FLEXIBLE_SIZE);
        let info = s.virtual_query(b, 0).unwrap();
        assert_eq!((info.flags, info.offset), (17, 0));
        assert_eq!(&info.name[..9], b"flexible\0");
        assert_eq!(s.unmap(a, 400 * MIB), 0);
        assert_eq!(s.stats().flexible_bytes, 48 * MIB);
        // The freed backing is used again, from the start of the flexible span.
        let c = s.map_flexible(0, MIB, 3, 0).unwrap();
        assert_eq!(s.containing(c).unwrap().phys, POOL);
        assert_eq!(s.stats().flexible_maps, 3);
    }

    /// #34: the right half of a split flexible mapping continues on the backing where the left
    /// half stops, and unmapping the middle frees only its pages.
    #[test]
    fn split_flexible_mappings_keep_their_backing_offsets() {
        let mut s = space();
        let a = s.map_flexible(0, 4 * PAGE, 3, 0).unwrap();
        assert_eq!(s.unmap(a + PAGE, 2 * PAGE), 0);
        assert_eq!(s.stats().flexible_bytes, 2 * PAGE);
        let mut pieces = Vec::new();
        assert!(
            s.for_each_backing(a + 3 * PAGE, PAGE, |phys, offset, n| pieces
                .push((phys, offset, n)))
        );
        assert_eq!(pieces, [(POOL + 3 * PAGE, 0, PAGE)]);
        // The two freed pages are the next flexible backing.
        let b = s.map_flexible(0, 2 * PAGE, 3, 0).unwrap();
        assert_eq!(s.containing(b).unwrap().phys, POOL + PAGE);
    }

    #[test]
    fn backing_walks_stop_at_the_first_unmapped_byte() {
        let mut s = space();
        let phys = allocate_on(&mut s, 4 * PAGE);
        let a = s
            .map_direct(0, 2 * PAGE, 3, 0, phys + 2 * PAGE as i64, 0)
            .unwrap();
        let b = s
            .map_direct(a + 2 * PAGE, PAGE, 3, MAP_FIXED, phys, 0)
            .unwrap();
        let mut pieces = Vec::new();
        assert!(s.for_each_backing(a + PAGE, 2 * PAGE, |p, o, n| pieces.push((p, o, n))));
        assert_eq!(pieces, [(3 * PAGE, 0, PAGE), (0, PAGE, PAGE)]);
        pieces.clear();
        assert!(!s.for_each_backing(b, 2 * PAGE, |p, o, n| pieces.push((p, o, n))));
        assert_eq!(pieces, [(0, 0, PAGE)], "the mapped part is passed on first");
        assert!(!s.for_each_backing(b + PAGE, 1, |_, _, _| panic!("nothing mapped")));
    }

    #[test]
    fn clamp_region_and_is_mapped_cross_adjacent_mappings_only() {
        let mut s = space();
        let phys = allocate_on(&mut s, 4 * PAGE);
        let a = s
            .map_direct(USER_MIN + MIB, 2 * PAGE, 3, MAP_FIXED, phys, 0)
            .unwrap();
        s.map_direct(a + 2 * PAGE, PAGE, 1, MAP_FIXED, phys, 0)
            .unwrap();
        s.reserve(a + 3 * PAGE, PAGE, MAP_FIXED, 0).unwrap();
        s.map_direct(a + 5 * PAGE, PAGE, 3, MAP_FIXED, phys, 0)
            .unwrap();
        assert_eq!(s.clamp(a, 10 * PAGE), 3 * PAGE);
        assert_eq!(s.clamp(a + PAGE, PAGE / 2), PAGE / 2);
        assert_eq!(s.clamp(a - PAGE, PAGE), 0);
        assert!(s.is_mapped(a, 3 * PAGE));
        assert!(!s.is_mapped(a, 4 * PAGE));
        assert!(!s.is_mapped(a + 4 * PAGE, PAGE));
        assert_eq!(s.region(0), Some((0, a, false)));
        assert_eq!(s.region(a + PAGE), Some((a, a + 2 * PAGE, true)));
        assert_eq!(
            s.region(a + 4 * PAGE),
            Some((a + 4 * PAGE, a + 5 * PAGE, false))
        );
        assert_eq!(s.region(a + 6 * PAGE), None);
    }

    #[test]
    fn gpu_protection_never_exceeds_the_guest_protection() {
        let mut s = space();
        let phys = allocate_on(&mut s, 2 * PAGE);
        let a = s.map_direct(0, PAGE, 0x37, 0, phys, 0).unwrap();
        s.map_direct(a + PAGE, PAGE, 1, MAP_FIXED, phys, 0).unwrap();
        s.reserve(a + 2 * PAGE, PAGE, MAP_FIXED, 0).unwrap();
        s.host().calls();
        s.gpu_protect(a, 3 * PAGE, true, false);
        s.gpu_protect(a, 2 * PAGE, false, false);
        s.gpu_protect(a + PAGE, PAGE, true, true);
        assert_eq!(
            s.host().calls(),
            [
                format!("protect {a:#x} {PAGE:#x} prot=5"),
                format!("protect {:#x} {PAGE:#x} prot=1", a + PAGE),
                format!("protect {a:#x} {PAGE:#x} prot=4"),
                format!("protect {:#x} {PAGE:#x} prot=0", a + PAGE),
                format!("protect {:#x} {PAGE:#x} prot=1", a + PAGE),
            ]
        );
    }

    #[test]
    fn host_protection_follows_the_guest_bits() {
        assert_eq!(host_prot(0), 0);
        assert_eq!(host_prot(1), HOST_READ);
        assert_eq!(host_prot(0x10), HOST_READ);
        assert_eq!(host_prot(2), HOST_READ | HOST_WRITE);
        assert_eq!(host_prot(0x20), HOST_READ | HOST_WRITE);
        assert_eq!(host_prot(4), HOST_READ | HOST_EXEC);
        assert_eq!(host_prot(0x37), HOST_READ | HOST_WRITE | HOST_EXEC);
    }

    #[test]
    fn batch_entries_run_in_order_and_stop_at_the_first_error() {
        let mut s = space();
        let phys = allocate_on(&mut s, 4 * PAGE);
        let at = USER_MIN + 64 * MIB;
        let entry = |operation, start, offset, length, prot, memory_type| BatchEntry {
            start,
            offset,
            length,
            prot,
            memory_type,
            reserved: 0,
            operation,
        };
        let map = entry(0, at, phys as u64, 2 * PAGE, 3, 0);
        assert_eq!(s.batch_entry(&map, MAP_FIXED), Ok(at));
        assert_eq!(s.batch_entry(&entry(2, at, 0, PAGE, 1, 0), 0), Ok(at));
        assert_eq!(
            s.batch_entry(&entry(4, at + PAGE, 0, PAGE, 1, 5), 0),
            Ok(at + PAGE)
        );
        assert_eq!(s.virtual_query(at + PAGE, 0).unwrap().memory_type, 5);
        let flexible = s.batch_entry(&entry(3, 0, 0, PAGE, 3, 0), 0).unwrap();
        assert!(s.virtual_query(flexible, 0).unwrap().flags & 1 != 0);
        assert_eq!(s.batch_entry(&entry(1, at, 0, 2 * PAGE, 0, 0), 0), Ok(at));
        assert!(!s.is_mapped(at, PAGE));
        assert_eq!(s.batch_entry(&entry(5, at, 0, PAGE, 0, 0), 0), Err(INVALID));
        // The protection byte is read unsigned: 0xb7 has bits outside the mask.
        assert_eq!(
            s.batch_entry(&entry(3, 0, 0, PAGE, 0xb7_u8 as i8, 0), 0),
            Err(INVALID)
        );
    }

    extern "C" fn record(address: usize, size: u64) {
        HOOK_CALLS.lock().unwrap().push((address as u64, size));
    }
    extern "C" fn record_unmap(address: usize, size: u64) {
        HOOK_CALLS.lock().unwrap().push((!(address as u64), size));
    }
    static HOOK_CALLS: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

    #[test]
    fn gpu_notifications_run_after_the_lock_in_order() {
        let memory = Memory::new(space());
        let phys = memory.write(|s| allocate_on(s, 4 * PAGE));
        let a = memory.write(|s| s.map_direct(0, 2 * PAGE, 3, 0, phys, 0).unwrap());
        memory.write(|s| s.reserve(0, PAGE, 0, 0).unwrap());
        memory.write(|s| s.set_hooks(Some(record), Some(record_unmap), Some(record)));
        memory.flush_hooks();
        assert_eq!(
            std::mem::take(&mut *HOOK_CALLS.lock().unwrap()),
            [(a, 2 * PAGE)]
        );
        memory.write(|s| {
            assert_eq!(s.protect(a, PAGE, 1, -1), 0);
            assert_eq!(s.unmap(a, 2 * PAGE), 0);
        });
        assert_eq!(memory.write(|s| s.pending_hooks()), 3);
        memory.flush_hooks();
        assert_eq!(
            std::mem::take(&mut *HOOK_CALLS.lock().unwrap()),
            [(a, PAGE), (!a, PAGE), (!(a + PAGE), PAGE)]
        );
        // `sceKernelMunmap` sends its notification first and drops the queued ones.
        let b = memory.write(|s| s.map_direct(0, PAGE, 3, 0, phys, 0).unwrap());
        memory.write(|s| {
            let before = s.pending_hooks();
            assert_eq!(s.unmap(b, PAGE), 0);
            s.forget_hooks_after(before);
        });
        memory.flush_hooks();
        assert_eq!(
            std::mem::take(&mut *HOOK_CALLS.lock().unwrap()),
            [(b, PAGE)]
        );
        assert!(memory.read(|s| s.unmap_hook()).is_some());
    }

    #[test]
    fn the_region_cache_holds_for_one_table_generation() {
        let memory = Memory::new(space());
        let phys = memory.write(|s| allocate_on(s, 2 * PAGE));
        let a = memory.write(|s| s.map_direct(0, 2 * PAGE, 3, 0, phys, 0).unwrap());
        assert_eq!(memory.generation() % 2, 0);
        assert_eq!(memory.clamp(a, 4 * PAGE), 2 * PAGE);
        assert!(memory.cached(a + PAGE).is_some());
        assert_eq!(memory.region(a + PAGE), Some((a, a + 2 * PAGE, true)));
        assert!(memory.is_mapped(a, 2 * PAGE));
        memory.write(|s| assert_eq!(s.unmap(a, 2 * PAGE), 0));
        assert!(memory.cached(a).is_none(), "a table change invalidates it");
        assert!(!memory.is_mapped(a, PAGE));
        assert_eq!(memory.region(a), None);
        let other = Memory::new(space());
        assert!(other.cached(a).is_none());
    }

    #[test]
    fn a_thread_holding_the_lock_reads_without_relocking() {
        let memory = Memory::new(space());
        let regions = memory.write(|_| memory.read(|s| s.regions().len()));
        assert_eq!(regions, 0);
        assert_eq!(
            memory.write(|_| memory.write(|_| memory.generation())) % 2,
            1
        );
        assert_eq!(memory.generation(), 4);
    }

    /// #121: the U-boat and Cartagena faults read 0x11383f88 bytes (about 275 MiB) past the end
    /// of the second direct block, which ends at 0x11bec00000 and is mapped in 1 MiB steps.
    /// The block edge holds: its last byte is mapped and backed at the block's end, the first
    /// byte past it is not, and the fault address is outside every mapping.
    #[test]
    fn the_second_direct_block_edge_of_issue_121() {
        let mut s = space();
        let first = allocate_on(&mut s, 256 * MIB);
        let second = allocate_on(&mut s, 64 * MIB);
        assert_eq!((first, second), (0, 256 * MIB as i64));
        let end = 0x11_bec0_0000u64;
        let fault = 0x11_cff8_3f88u64;
        assert_eq!(fault - end, 0x1138_3f88);
        let first_at = s.map_direct(0, 256 * MIB, 0x33, 0, first, 0).unwrap();
        assert_eq!(first_at, USER_MIN);
        let base = end - 64 * MIB;
        for step in 0..64 {
            let at = base + step * MIB;
            let phys = second + (step * MIB) as i64;
            assert_eq!(s.map_direct(at, MIB, 0x33, MAP_FIXED, phys, 0), Ok(at));
        }
        assert!(s.is_mapped(base, 64 * MIB));
        assert!(s.is_mapped(end - 1, 1));
        assert!(!s.is_mapped(end, 1));
        assert_eq!(s.clamp(end - 8, 64), 8);
        assert_eq!(s.query_protection(end - 1), Ok((base, end, 0x33)));
        let mut last = Vec::new();
        assert!(s.for_each_backing(end - 8, 8, |p, o, n| last.push((p, o, n))));
        assert_eq!(last, [(second as u64 + 64 * MIB - 8, 0, 8)]);
        assert_eq!(s.region(end), None);
        assert_eq!(s.virtual_query(fault, 0), Err(ACCESS));
        assert!(!s.is_mapped(fault, 8));
        // Unmapping and mapping again the last step leaves the same edge.
        assert_eq!(s.unmap(end - MIB, MIB), 0);
        assert!(!s.is_mapped(end - 1, 1));
        let phys = second + 63 * MIB as i64;
        assert_eq!(
            s.map_direct(end - MIB, MIB, 0x33, MAP_FIXED, phys, 0),
            Ok(end - MIB)
        );
        assert!(s.is_mapped(base, 64 * MIB) && !s.is_mapped(end, 1));
        assert_eq!(
            s.direct_memory_type(second as u64 + 64 * MIB - 1),
            Ok((0, 256 * MIB, 320 * MIB))
        );
        assert_eq!(s.direct_memory_type(second as u64 + 64 * MIB), Err(ACCESS));
    }

    #[test]
    fn the_report_lines_match_the_c() {
        let mut s = space();
        let phys = allocate_on(&mut s, PAGE);
        s.map_direct(0, PAGE, 3, 0, phys, 0).unwrap();
        s.query_protection(USER_MIN).unwrap();
        assert_eq!(
            s.report(),
            [
                format!("Runtime: direct memory allocations=1, maps=1, live=16384, budget={POOL}"),
                "Runtime: flexible maps=0, in use=0, protects=0, queries=1, regions=1".to_string(),
            ]
        );
    }

    #[test]
    fn the_pool_size_comes_from_bb_dmem_mb() {
        assert_eq!(pool_size(None), RETAIL_POOL);
        assert_eq!(pool_size(Some("")), RETAIL_POOL);
        assert_eq!(pool_size(Some("abc")), RETAIL_POOL);
        assert_eq!(pool_size(Some("5055")), RETAIL_POOL);
        assert_eq!(pool_size(Some("9000")), 9000 * MIB);
        assert_eq!(pool_size(Some(" 9000MB")), 9000 * MIB);
        assert_eq!(pool_size(Some("16384")), 16384 * MIB);
        assert_eq!(pool_size(Some("16385")), RETAIL_POOL);
    }

    #[test]
    fn numbers_parse_as_c_does() {
        assert_eq!(strtoull("0x1f", 0), 31);
        assert_eq!(strtoull("017", 0), 15);
        assert_eq!(strtoull("17", 0), 17);
        assert_eq!(strtoull("0x", 0), 0);
        assert_eq!(strtoull("-1", 0), u64::MAX);
        assert_eq!(strtoull("99999999999999999999", 10), u64::MAX);
        assert_eq!(parse_toggles("5 7\n"), (5, 7));
        assert_eq!(parse_toggles(" 5"), (5, 0));
        assert_eq!(parse_toggles("5 x"), (5, 0));
        assert_eq!(parse_toggles("x 5"), (0, 0));
        assert_eq!(parse_toggles(""), (0, 0));
        assert_eq!(pointer(0), "(nil)");
        assert_eq!(pointer(0x1000), "0x1000");
        assert_eq!(alternate_hex(0), "0");
        assert_eq!(alternate_hex(16), "0x10");
    }

    #[test]
    fn the_guest_layouts_are_the_ps4_ones() {
        use std::mem::offset_of;
        assert_eq!(offset_of!(VirtualQueryInfo, offset), 16);
        assert_eq!(offset_of!(VirtualQueryInfo, protection), 24);
        assert_eq!(offset_of!(VirtualQueryInfo, flags), 32);
        assert_eq!(offset_of!(VirtualQueryInfo, name), 36);
        assert_eq!(offset_of!(BatchEntry, prot), 24);
        assert_eq!(offset_of!(BatchEntry, memory_type), 25);
        assert_eq!(offset_of!(BatchEntry, operation), 28);
    }

    /// The NID list is `src/import_names.inc`'s entries for the exported symbols, all of them.
    #[test]
    fn the_nids_are_the_import_name_tables() {
        let table = include_str!("../../../src/import_names.inc");
        let mut expected: Vec<(&str, &str)> = table
            .lines()
            .filter_map(|line| {
                let line = line.trim().strip_prefix("{\"")?;
                let (nid, rest) = line.split_once("\",\"")?;
                let symbol = rest.strip_suffix("\"},")?;
                SYMBOLS.contains(&symbol).then_some((nid, symbol))
            })
            .collect();
        let mut listed = NIDS.to_vec();
        expected.sort();
        listed.sort();
        assert_eq!(listed, expected);
        assert_eq!(nid_symbol(b"cQke9UuBQOk#p#J"), Some("sceKernelMunmap"));
        assert_eq!(nid_symbol(b"cQke9UuBQOk#q#q"), None);
        assert!(NIDS.iter().all(|(_, symbol)| SYMBOLS.contains(symbol)));
    }
}
