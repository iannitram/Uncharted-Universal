//! Render-size patches by byte pattern: a render size other than the PS4's 1920x1080 (540p up,
//! 4K and beyond) for all three games, and the 720p Deck mode. Everything is found by byte
//! pattern, so 1.00 and 1.02 of every region are covered. Replaces `scripts/render_res.py`
//! (ported from BilalReloaded's port, GPL-2.0-or-later, credited there) and
//! `scripts/render_720p.py`; their Python tests are the expected results.
//!
//! Every patch is data ([`CheckedWrite`]: where, the bytes it expects to find, the bytes to
//! write). [`apply_checked`] refuses a write that falls outside the image or whose expected
//! bytes are not there, with a plain error, and then writes nothing.

use crate::error::{Error, Result};
use crate::games::Game;
use crate::patches::ByteWrite;
use std::collections::BTreeMap;

/// The PS4's output size.
pub const OUTPUT_SIZE: (u32, u32) = (1920, 1080);
/// Smallest and largest size `parse_size` accepts.
pub const MIN_SIZE: (u32, u32) = (640, 360);
pub const MAX_SIZE: (u32, u32) = (7680, 4320);

/// The upscaling presets (`UPSCALE_SCALES` in `run.py`): the game draws at the chosen
/// resolution divided by the scale and the upscaler fills the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpscaleQuality {
    Quality,
    Balanced,
    Performance,
    Ultra,
}

impl UpscaleQuality {
    /// 1.5, 1.7, 2.0, 3.0.
    pub fn scale(self) -> f64 {
        match self {
            UpscaleQuality::Quality => 1.5,
            UpscaleQuality::Balanced => 1.7,
            UpscaleQuality::Performance => 2.0,
            UpscaleQuality::Ultra => 3.0,
        }
    }
}

/// What a resolution patch did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patched {
    /// The patched memory image.
    pub image: Vec<u8>,
    /// The size the game renders at: smaller than asked when the game's memory layout cannot
    /// hold it (Drake's Fortune keeps its render targets in the GPU area).
    pub render: (u32, u32),
    /// Direct memory to add, in MiB (`BB_DMEM_MB`).
    pub extra_mib: u32,
}

/// One patch as data: at `offset` the image must hold `expect`, and `bytes` replace it (same
/// length). The offset is into the memory image, like [`ByteWrite`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedWrite {
    pub offset: u64,
    pub expect: Vec<u8>,
    pub bytes: Vec<u8>,
}

impl CheckedWrite {
    /// The plain write, as the patch engine takes it.
    pub fn to_byte_write(&self) -> ByteWrite {
        ByteWrite {
            offset: self.offset,
            bytes: self.bytes.clone(),
        }
    }
}

fn refuse(message: String) -> Error {
    Error::RenderSize(message)
}

/// Write one patch into `image` after checking it: inside the image, same length, and the
/// expected bytes are there. On a refusal nothing is written.
fn commit(image: &mut [u8], write: &CheckedWrite) -> Result<()> {
    if write.expect.len() != write.bytes.len() || write.bytes.is_empty() {
        return Err(refuse(format!(
            "patch at {:#x} is malformed (expected and new bytes differ in length)",
            write.offset
        )));
    }
    let range = usize::try_from(write.offset)
        .ok()
        .and_then(|start| start.checked_add(write.bytes.len()).map(|end| start..end))
        .filter(|range| range.end <= image.len())
        .ok_or_else(|| {
            refuse(format!(
                "patch at {:#x} lies outside the executable image; nothing was written",
                write.offset
            ))
        })?;
    if image[range.clone()] != write.expect[..] {
        return Err(refuse(format!(
            "the executable does not match at {:#x} (not the expected revision); nothing was written",
            write.offset
        )));
    }
    image[range].copy_from_slice(&write.bytes);
    Ok(())
}

/// Apply `writes` in order to a copy of `image`. A write outside the image, or one whose
/// expected bytes are not there, is `Error::RenderSize(..)` and nothing is returned.
pub fn apply_checked(image: &[u8], writes: &[CheckedWrite]) -> Result<Vec<u8>> {
    let mut patched = image.to_vec();
    for write in writes {
        commit(&mut patched, write)?;
    }
    Ok(patched)
}

/// Parse "WxH" (case-insensitive). Width must be a multiple of 8 and height of 4, within
/// `MIN_SIZE` .. `MAX_SIZE`; anything else is `Error::RenderSize("<w>x<h> is not supported ...")`.
pub fn parse_size(text: &str) -> Result<(u32, u32)> {
    let lower = text.to_lowercase();
    let parts: Vec<&str> = lower.split('x').collect();
    let numbers: Option<Vec<i64>> = if parts.len() == 2 {
        parts
            .iter()
            .map(|part| part.trim().parse::<i64>().ok())
            .collect()
    } else {
        None
    };
    let Some(numbers) = numbers else {
        return Err(refuse(format!(
            "Render resolution: '{text}' is not WIDTHxHEIGHT"
        )));
    };
    let (width, height) = (numbers[0], numbers[1]);
    let in_range = (i64::from(MIN_SIZE.0)..=i64::from(MAX_SIZE.0)).contains(&width)
        && (i64::from(MIN_SIZE.1)..=i64::from(MAX_SIZE.1)).contains(&height);
    if !in_range || width % 8 != 0 || height % 4 != 0 {
        return Err(refuse(format!(
            "Render resolution: {width}x{height} is not supported (width a multiple of 8, \
             height of 4, {}x{} .. {}x{})",
            MIN_SIZE.0, MIN_SIZE.1, MAX_SIZE.0, MAX_SIZE.1
        )));
    }
    Ok((width as u32, height as u32))
}

const MIB: i128 = 1 << 20;
/// Drake's Fortune above what its GPU area holds: these 16:9 sizes in turn.
const FALLBACK_SIZES: [(u32, u32); 4] = [(3200, 1800), (2880, 1620), (2560, 1440), (1920, 1080)];
/// Base registers of `movl $imm32, disp8(%reg)` (ModRM mod=01 reg=0), rsp left out.
const BASES: [u8; 7] = [0x47, 0x40, 0x41, 0x42, 0x43, 0x45, 0x46];

/// Why one size failed.
enum Fail {
    /// The size does not fit the memory layout: try a smaller one.
    TooLarge,
    Hard(Error),
}

impl From<Error> for Fail {
    fn from(error: Error) -> Self {
        Fail::Hard(error)
    }
}

fn hard(message: &str) -> Fail {
    Fail::Hard(refuse(message.to_owned()))
}

/// Non-overlapping occurrences of `needle` in `data[start..end]`, left to right.
fn find_all(data: &[u8], start: usize, end: usize, needle: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut at = start;
    while at + needle.len() <= end {
        if &data[at..at + needle.len()] == needle {
            found.push(at);
            at += needle.len();
        } else {
            at += 1;
        }
    }
    found
}

/// Replace every `old` in `data[range]` with `new` as checked writes.
fn replace_all(
    data: &mut [u8],
    range: (usize, usize),
    old: &[u8],
    new: &[u8],
    log: &mut Vec<CheckedWrite>,
) -> Result<()> {
    for at in find_all(data, range.0, range.1, old) {
        let write = CheckedWrite {
            offset: at as u64,
            expect: old.to_vec(),
            bytes: new.to_vec(),
        };
        commit(data, &write)?;
        log.push(write);
    }
    Ok(())
}

/// The memory table entry `(id, parent, size, alignment, 0)` by structure: its offset and size.
/// It must be found exactly once.
fn table_entry(
    data: &[u8],
    entry_id: u32,
    parent: u32,
    align: u64,
    min_size: u64,
) -> std::result::Result<(usize, u64), Fail> {
    let mut key = [0u8; 8];
    key[..4].copy_from_slice(&entry_id.to_le_bytes());
    key[4..].copy_from_slice(&parent.to_le_bytes());
    let mut found = Vec::new();
    for at in 0..data.len().saturating_sub(31) {
        if data[at..at + 8] != key {
            continue;
        }
        let word =
            |from: usize| u64::from_le_bytes(data[at + from..at + from + 8].try_into().unwrap());
        let (size, entry_align, pad) = (word(8), word(16), word(24));
        if entry_align == align && pad == 0 && size >= min_size {
            found.push((at, size));
        }
    }
    if found.len() != 1 {
        return Err(Fail::Hard(refuse(format!(
            "Resolution patch: memory table entry {entry_id:#x} found {} times",
            found.len()
        ))));
    }
    Ok(found[0])
}

fn ceil_to(value: i128, step: i128) -> i128 {
    (value + step - 1).div_euclid(step) * step
}

/// A heap grows with the pixel count plus the margin, in whole 64 MiB steps.
fn grown(old: i128, factor: f64, margin: f64, step: i128) -> i128 {
    if factor > 1.0 {
        let extra = (old as f64 * (factor * margin - 1.0)) as i128;
        old + ceil_to(extra, step)
    } else {
        old
    }
}

fn put_u64(value: i128) -> std::result::Result<[u8; 8], Fail> {
    u64::try_from(value)
        .map(u64::to_le_bytes)
        .map_err(|_| hard("Resolution patch: memory table size out of range"))
}

/// The memory table's new layout for one margin: heap, render target, CPU area, GPU area sizes
/// and the heap's context.
type Layout = (i128, i128, i128, i128, u32);

/// `resolution_patch` in `render_res.py`: patch `image` in place for one size and return the
/// extra direct memory in MiB. `move_heap` is false for Drake's Fortune.
fn resolution_patch(
    image: &mut [u8],
    width: u32,
    height: u32,
    move_heap: bool,
    log: &mut Vec<CheckedWrite>,
) -> std::result::Result<u32, Fail> {
    // mov [reg+8],1920; mov [reg+0xc],1080 (1.00 starts with rdi, 1.02 with rax); any byte
    // stands in for the register.
    let anchor = image.windows(14).position(|w| {
        w[0] == 0xc7
            && w[2] == 0x08
            && w[3..7] == [0x80, 0x07, 0x00, 0x00]
            && w[7] == 0xc7
            && w[9] == 0x0c
            && w[10..14] == [0x38, 0x04, 0x00, 0x00]
    });
    let Some(first) = anchor else {
        return Err(hard("Resolution patch: display settings code not found"));
    };
    let range = (first.saturating_sub(0x40), (first + 0x800).min(image.len()));
    let (w, h) = (u64::from(width), u64::from(height));
    let stores: [(u8, u32, u64); 12] = [
        (0x08, 1920, w),
        (0x18, 1920, w),
        (0x24, 1920, w),
        (0x38, 1920, w),
        (0x0c, 1080, h),
        (0x1c, 1080, h),
        (0x28, 1080, h),
        (0x3c, 1080, h),
        (0x40, 240, w / 8),
        (0x44, 270, h / 4),
        (0x30, 2560, 2560 * w / 1920),
        (0x34, 1216, 1216 * h / 1080),
    ];
    // movl $imm32, disp8(%reg): CUSA02320 uses %rdi (1.00) and %rax (1.02); other dumps other
    // registers.
    for base in BASES {
        for (disp, old, new) in stores {
            let new =
                u32::try_from(new).map_err(|_| hard("Resolution patch: size out of range"))?;
            let mut old_bytes = vec![0xc7, base, disp];
            old_bytes.extend_from_slice(&old.to_le_bytes());
            let mut new_bytes = vec![0xc7, base, disp];
            new_bytes.extend_from_slice(&new.to_le_bytes());
            replace_all(image, range, &old_bytes, &new_bytes, log)?;
        }
    }
    let packed = |a: u32, b: u32| {
        let mut bytes = vec![0x48, 0xb8];
        bytes.extend_from_slice(&a.to_le_bytes());
        bytes.extend_from_slice(&b.to_le_bytes());
        bytes
    };
    replace_all(
        image,
        range,
        &packed(1920, 1080),
        &packed(width, height),
        log,
    )?;

    // The render targets come from a fixed "Render Target Heap" (497 MiB at 1080p) inside the
    // 2988 MiB GPU (garlic) block of the engine's memory table; both grow with the pixel count
    // (+10%), and the direct memory reported to the game by the same amount (BB_DMEM_MB).
    let factor = (f64::from(width) * f64::from(height) / (1920.0 * 1080.0)).max(1.0);
    // Entry 2 = CPU ("onion", context 5) and entry 3 = GPU ("garlic", context 6). An area holds
    // at most 4096 MiB, so above ~1620p the Render Target Heap moves to the CPU area; Drake's
    // Fortune cannot address its heaps there, so it stops where the GPU area is full.
    let (onion_at, onion_old) = table_entry(image, 2, 2, 0x100000, 0x4000_0000)?;
    let (garlic_at, garlic_old) = table_entry(image, 3, 2, 0x100000, 0x8000_0000)?;
    let (heap_at, heap_old) = table_entry(image, 0x85, 6, 0x80, 0x1000_0000)?;
    let (rt_at, rt_old) = table_entry(image, 0x84, 6, 0x80, 0x400_0000)?;
    let (onion_old, garlic_old) = (i128::from(onion_old), i128::from(garlic_old));
    let (heap_old, rt_old) = (i128::from(heap_old), i128::from(rt_old));
    let step = 64 * MIB;
    let (area_limit, headroom) = (4096 * MIB, 128 * MIB);

    let layout = |margin: f64| -> std::result::Result<Layout, Fail> {
        let mut heap_new = grown(heap_old, factor, margin, step);
        let mut rt_new = grown(rt_old, factor, margin, step);
        let mut onion_new = onion_old;
        let mut garlic_new = garlic_old + (heap_new - heap_old) + (rt_new - rt_old);
        let mut heap_context = 6;
        if garlic_new >= area_limit {
            if !move_heap {
                return Err(Fail::TooLarge);
            }
            // Area sizes must be whole MiB: the moved heap is rounded up, and the GPU area's
            // rounding goes to the other render target heap.
            heap_new = ceil_to(heap_new, step);
            onion_new = onion_old + heap_new;
            let rest = garlic_old - heap_old - rt_old;
            garlic_new = ceil_to(rest + rt_new, step);
            rt_new = garlic_new - rest;
            heap_context = 5;
        }
        // Uncharted 3 1.02 maps its CPU area 100 MiB larger than the table says: keep 128 MiB
        // below the limit.
        if onion_new + headroom >= area_limit || garlic_new >= area_limit {
            return Err(Fail::TooLarge);
        }
        Ok((heap_new, rt_new, onion_new, garlic_new, heap_context))
    };
    // The render target heaps grow with the pixel count plus 10%, less when that doesn't fit.
    let mut chosen = layout(1.1);
    for margin in [1.05, 1.0] {
        if matches!(chosen, Err(Fail::TooLarge)) {
            chosen = layout(margin);
        }
    }
    let (heap_new, rt_new, onion_new, garlic_new, heap_context) = chosen?;
    let extra = (onion_new - onion_old) + (garlic_new - garlic_old);

    let mut put = |at: usize, bytes: Vec<u8>, image: &mut [u8]| -> Result<()> {
        let write = CheckedWrite {
            offset: at as u64,
            expect: image
                .get(at..at + bytes.len())
                .ok_or_else(|| refuse("Resolution patch: memory table out of range".into()))?
                .to_vec(),
            bytes,
        };
        commit(image, &write)?;
        log.push(write);
        Ok(())
    };
    let mut heap_entry = Vec::with_capacity(16);
    heap_entry.extend_from_slice(&0x85u32.to_le_bytes());
    heap_entry.extend_from_slice(&heap_context.to_le_bytes());
    heap_entry.extend_from_slice(&put_u64(heap_new)?);
    put(heap_at, heap_entry, image)?;
    put(rt_at + 8, put_u64(rt_new)?.to_vec(), image)?;
    put(onion_at + 8, put_u64(onion_new)?.to_vec(), image)?;
    put(garlic_at + 8, put_u64(garlic_new)?.to_vec(), image)?;

    // The memory table check (ValidateContext: lea rsi,[rbp-0x20 or -0x30]; movabs rdx,<total>)
    // expects the direct memory blocks (context 2) to add up to 4608 MiB.
    let mut checks = Vec::new();
    let mut at = 0;
    while at + 14 <= image.len() {
        let w = &image[at..at + 14];
        if w[..3] == [0x48, 0x8d, 0x75]
            && w[4..6] == [0x48, 0xba]
            && w[6..14] == [0, 0, 0, 0x20, 0x01, 0, 0, 0]
        {
            checks.push(at);
            at += 14;
        } else {
            at += 1;
        }
    }
    if checks.len() != 1 {
        return Err(Fail::Hard(refuse(format!(
            "Resolution patch: memory total check found {} times",
            checks.len()
        ))));
    }
    let total = put_u64(0x1_2000_0000 + extra)?;
    put(checks[0] + 6, total.to_vec(), image)?;
    u32::try_from(extra.div_euclid(MIB))
        .map_err(|_| hard("Resolution patch: extra memory out of range"))
}

/// Patch the boot image for `game` at `size`, trying smaller sizes in turn until one fits
/// (`patch_image` in `render_res.py`). No size that fits is `Error::RenderSize(..)`.
pub fn patch_image(image: &[u8], game: Game, size: (u32, u32)) -> Result<Patched> {
    patch_image_with_writes(image, game, size).map(|(patched, _)| patched)
}

/// [`patch_image`] and the checked writes that turn `image` into the result.
pub fn patch_image_with_writes(
    image: &[u8],
    game: Game,
    size: (u32, u32),
) -> Result<(Patched, Vec<CheckedWrite>)> {
    let (width, height) = size;
    if width == 0 || height == 0 {
        return Err(refuse("Render resolution: size must not be zero".into()));
    }
    let move_heap = game != Game::U1;
    let pixels = u64::from(width) * u64::from(height);
    let mut candidates = vec![size];
    if (f64::from(width) / f64::from(height) - 16.0 / 9.0).abs() > 0.01 {
        // Other shapes: smaller sizes of the same shape (width/16 and height/4 whole: Uncharted
        // 3 asserts widths in steps of 16 in its post processing).
        for scale in [0.9, 0.8, 0.75, 2.0 / 3.0, 0.6, 0.5] {
            candidates.push((
                (f64::from(width) * scale) as u32 / 16 * 16,
                (f64::from(height) * scale) as u32 / 4 * 4,
            ));
        }
    } else {
        candidates.extend(
            FALLBACK_SIZES
                .iter()
                .copied()
                .filter(|s| u64::from(s.0) * u64::from(s.1) < pixels),
        );
    }
    for candidate in candidates {
        let mut patched = image.to_vec();
        let mut log = Vec::new();
        match resolution_patch(&mut patched, candidate.0, candidate.1, move_heap, &mut log) {
            Ok(extra_mib) => {
                let result = Patched {
                    image: patched,
                    render: candidate,
                    extra_mib,
                };
                return Ok((result, log));
            }
            Err(Fail::TooLarge) => continue,
            Err(Fail::Hard(error)) => return Err(error),
        }
    }
    Err(refuse(format!(
        "Render resolution: no size up to {width}x{height} fits {}'s memory",
        game.key().to_uppercase()
    )))
}

const NEW_SIZE: (u32, u32) = (1280, 720);
/// The size fields of one setup follow within this many bytes.
const WINDOW: usize = 0x90;
/// CUSA02320 has two default setups and the 1080p mode; CUSA02344 has been reported with one.
const MAX_SITES: usize = 3;

fn is_modrm(byte: u8) -> bool {
    (0x40..0x48).contains(&byte) && byte != 0x44
}

/// `movl $imm32, disp8(%reg)` prefix at `at`: an optional REX.B prefix, C7, ModRM mod=01 reg=0
/// with any base register except rsp. Returns its length (2 or 3), the displacement byte excluded.
fn movl_len(elf: &[u8], at: usize) -> Option<usize> {
    let plain = |p: usize| {
        (elf.get(p) == Some(&0xc7) && elf.get(p + 1).is_some_and(|b| is_modrm(*b))).then_some(2)
    };
    if elf.get(at) == Some(&0x41) {
        plain(at + 1).map(|n| n + 1)
    } else {
        plain(at)
    }
}

/// Starts of `movl $1920, 8(%reg); movl $1080, 0xc(%reg)`, non-overlapping, left to right.
fn find_sites(elf: &[u8]) -> Vec<usize> {
    let mut sites = Vec::new();
    let mut at = 0;
    while at < elf.len() {
        let hit = movl_len(elf, at).and_then(|a| {
            let p = at + a;
            (elf.get(p..p + 5) == Some(&[0x08, 0x80, 0x07, 0x00, 0x00][..])).then_some(())?;
            let q = p + 5;
            let r = q + movl_len(elf, q)?;
            (elf.get(r..r + 5) == Some(&[0x0c, 0x38, 0x04, 0x00, 0x00][..])).then_some(r + 5)
        });
        match hit {
            Some(end) => {
                sites.push(at);
                at = end;
            }
            None => at += 1,
        }
    }
    sites
}

/// `movl $1920, d(%rsp); movl $1088, d+4(%rsp)`: the movie decoder's maximum frame size.
fn find_decoders(elf: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + 16 <= elf.len() {
        let w = &elf[at..at + 16];
        if w[..3] == [0xc7, 0x44, 0x24]
            && w[4..8] == [0x80, 0x07, 0x00, 0x00]
            && w[8..11] == [0xc7, 0x44, 0x24]
            && w[12..16] == [0x40, 0x04, 0x00, 0x00]
        {
            if u16::from(w[11]) == u16::from(w[3]) + 4 {
                found.push(at);
            }
            at += 16;
        } else {
            at += 1;
        }
    }
    found
}

/// File offset minus image offset of the first program header (the image maps vaddr 0 at 0).
fn image_base(elf: &[u8]) -> Result<i64> {
    let bad = || refuse("expected a 64-bit ELF with a program header".to_owned());
    let phoff = elf
        .get(0x20..0x28)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(bad)?;
    let header = phoff
        .checked_add(24)
        .and_then(|end| elf.get(phoff + 8..end))
        .ok_or_else(bad)?;
    let offset = i64::try_from(u64::from_le_bytes(header[..8].try_into().unwrap()));
    let vaddr = i64::try_from(u64::from_le_bytes(header[8..16].try_into().unwrap()));
    match (offset, vaddr) {
        (Ok(offset), Ok(vaddr)) => Ok(offset - vaddr),
        _ => Err(bad()),
    }
}

/// The checked writes that make the executable start in its own 720p mode (Deck mode), with
/// the bytes each expects, offsets into the memory image. Same sites as [`deck_720p_writes`].
pub fn deck_720p_patches(elf: &[u8]) -> Result<Vec<CheckedWrite>> {
    let base = image_base(elf)?;
    let sites = find_sites(elf);
    if !(1..=MAX_SITES).contains(&sites.len()) {
        return Err(refuse(format!(
            "expected 1 to {MAX_SITES} video setting sites, found {}",
            sites.len()
        )));
    }
    let image_offset = |file_at: usize| -> Result<u64> {
        i64::try_from(file_at)
            .ok()
            .and_then(|at| at.checked_sub(base))
            .and_then(|at| u64::try_from(at).ok())
            .ok_or_else(|| refuse("a patch site lies below the image base".to_owned()))
    };
    let u32_at = |at: usize| u32::from_le_bytes(elf[at..at + 4].try_into().unwrap());
    let mut out = Vec::new();
    for (index, &site) in sites.iter().enumerate() {
        // A setup's fields end before the next one (and its cmp/jne) begins.
        let end = (site + WINDOW).min(
            sites
                .get(index + 1)
                .map_or(elf.len(), |next| next.saturating_sub(9)),
        );
        // cmp $1, %esi; jne <next mode>: the mode switch's 1080p case. It jumps to the game's
        // own 720p case instead.
        if site >= 9
            && elf[site - 9] == 0x83
            && elf[site - 8] >= 0xf8
            && elf[site - 7] == 0x01
            && elf[site - 6..site - 4] == [0x0f, 0x85]
        {
            let rel = i32::from_le_bytes(elf[site - 4..site].try_into().unwrap()).wrapping_add(1);
            let mut bytes = vec![0xe9];
            bytes.extend_from_slice(&rel.to_le_bytes());
            bytes.push(0x90);
            out.push(CheckedWrite {
                offset: image_offset(site - 6)?,
                expect: elf[site - 6..site].to_vec(),
                bytes,
            });
            continue;
        }
        for at in site..end.min(elf.len().saturating_sub(3)) {
            let value = u32_at(at);
            let new = match value {
                1920 => NEW_SIZE.0,
                1080 => NEW_SIZE.1,
                240 => NEW_SIZE.0 / 8,
                270 => NEW_SIZE.1 / 4,
                _ => continue,
            };
            // movl $imm32, disp8(%reg), or the two halves of movabs $imm64, %rax
            let movl = at >= 3 && elf[at - 3] == 0xc7 && is_modrm(elf[at - 2]);
            let movabs = (at >= 2 && elf[at - 2..at] == [0x48, 0xb8])
                || (at >= 6 && elf[at - 6..at - 4] == [0x48, 0xb8]);
            // 1920/8 and 1080/4 only in fields 0x40/0x44 (the 1080p mode's 240x270)
            if matches!(value, 240 | 270) && !(movl && matches!(elf[at - 1], 0x40 | 0x44)) {
                continue;
            }
            if movl || movabs {
                out.push(CheckedWrite {
                    offset: image_offset(at)?,
                    expect: elf[at..at + 4].to_vec(),
                    bytes: new.to_le_bytes().to_vec(),
                });
            }
        }
    }
    // The movie shader maps its 720p picture to the decoder's maximum frame size, so it becomes
    // 1280x720 and the 1080p movies arrive scaled to it.
    let decoders = find_decoders(elf);
    if decoders.len() != 1 {
        return Err(refuse(format!(
            "expected one movie decoder setup, found {}",
            decoders.len()
        )));
    }
    let at = decoders[0];
    for (field, value) in [(4, NEW_SIZE.0), (12, NEW_SIZE.1)] {
        out.push(CheckedWrite {
            offset: image_offset(at + field)?,
            expect: elf[at + field..at + field + 4].to_vec(),
            bytes: value.to_le_bytes().to_vec(),
        });
    }
    Ok(out)
}

/// The writes that make the executable start in its own 720p mode (Deck mode): the 1280x720
/// setup sites found by pattern. Zero sites, or more than three, is
/// `Error::RenderSize("expected 1 to 3 video setting sites, found <n>")`.
pub fn deck_720p_writes(elf: &[u8]) -> Result<Vec<ByteWrite>> {
    Ok(deck_720p_patches(elf)?
        .iter()
        .map(CheckedWrite::to_byte_write)
        .collect())
}

/// Extra render-target memory a game needs per extra 1080p worth of pixels, in MiB (`RT_MIB_PER_1080P`,
/// `RT2_MIB_PER_1080P` in `run.py`; docs/resolution.md).
pub const RT_MIB_PER_1080P: u32 = 300;
pub const RT2_MIB_PER_1080P: u32 = 150;

/// The variables for the game's own render size (`BB_GAME_RES`, `game_res` in `run.py`, #139).
/// Reads `BB_GAME_RES` ("WxH", case-insensitive, `x` or the multiplication sign) from
/// `player_env`. Returns nothing when it is missing, not a size, outside `MIN_SIZE` ..
/// `MAX_SIZE`, or 1920x1080. Otherwise returns `BB_GAME_RES` as "<w>x<h>" and the heap
/// variables `BB_GAME_RT_MB`, `BB_GAME_RT2_MB`, `BB_GAME_FREE_MB`, `BB_GAME_MOVIE_MB` and, when
/// the pool needs it, `BB_DMEM_MB`. A heap variable the player set to digits keeps its value.
pub fn game_res_env(player_env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    // Get BB_GAME_RES from player_env, lowercase it and replace multiplication sign with 'x'
    let res_text = match player_env.get("BB_GAME_RES") {
        Some(text) => text.to_lowercase().replace('\u{d7}', "x"),
        None => return BTreeMap::new(),
    };

    // Split into width and height
    let parts: Vec<&str> = res_text.split('x').collect();
    if parts.len() != 2 {
        return BTreeMap::new();
    }

    // Parse width and height as integers
    let width = match parts[0].parse::<u32>() {
        Ok(w) => w,
        Err(_) => return BTreeMap::new(),
    };
    let height = match parts[1].parse::<u32>() {
        Ok(h) => h,
        Err(_) => return BTreeMap::new(),
    };

    // Check if the size is within bounds
    if width < MIN_SIZE.0 || width > MAX_SIZE.0 || height < MIN_SIZE.1 || height > MAX_SIZE.1 {
        return BTreeMap::new();
    }

    // Return empty map if it's exactly 1920x1080
    if width == 1920 && height == 1080 {
        return BTreeMap::new();
    }

    // Calculate scale factor
    let scale = (width as f64 * height as f64) / (1920.0 * 1080.0) - 1.0;

    // Helper function to check if a string is all digits (optionally with leading '-')
    let is_all_digits = |s: &str| -> bool {
        let digits = s.strip_prefix('-').unwrap_or(s);
        !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
    };

    // Helper function to compute heap sizes with rounding to nearest multiple of 64
    let compute_heap_size = |per_1080p: u32| -> u32 {
        if scale <= 0.0 {
            0
        } else {
            let value = (scale * per_1080p as f64).trunc() / 64.0;
            (value.ceil() * 64.0) as u32
        }
    };

    // Helper function to compute movie MB
    let compute_movie_mb = |width: u32, height: u32| -> u32 {
        // frame_width = ceil(width / 16) * 16
        let frame_width = width.div_ceil(16) * 16;
        // frame_height = ceil(height / 64) * 64
        let frame_height = height.div_ceil(64) * 64;
        // frame_bytes = frame_width * frame_height * 3 / 2 + 65536
        let frame_bytes = (frame_width as u64 * frame_height as u64 * 3 / 2) + 65536;
        // ceil(frame_bytes / 2097152) * 2, where 2097152 = 2^21
        let mb = frame_bytes.div_ceil(2097152); // Use div_ceil for ceiling division
        (mb * 2).max(4) as u32
    };

    // Build the result map
    let mut result = BTreeMap::new();

    // Always set BB_GAME_RES
    result.insert("BB_GAME_RES".to_string(), format!("{}x{}", width, height));

    // Handle BB_GAME_RT_MB
    if let Some(rt_mb) = player_env.get("BB_GAME_RT_MB") {
        if is_all_digits(rt_mb) {
            result.insert("BB_GAME_RT_MB".to_string(), rt_mb.clone());
        } else {
            result.insert(
                "BB_GAME_RT_MB".to_string(),
                compute_heap_size(RT_MIB_PER_1080P).to_string(),
            );
        }
    } else {
        result.insert(
            "BB_GAME_RT_MB".to_string(),
            compute_heap_size(RT_MIB_PER_1080P).to_string(),
        );
    }

    // Handle BB_GAME_RT2_MB
    if let Some(rt2_mb) = player_env.get("BB_GAME_RT2_MB") {
        if is_all_digits(rt2_mb) {
            result.insert("BB_GAME_RT2_MB".to_string(), rt2_mb.clone());
        } else {
            result.insert(
                "BB_GAME_RT2_MB".to_string(),
                compute_heap_size(RT2_MIB_PER_1080P).to_string(),
            );
        }
    } else {
        result.insert(
            "BB_GAME_RT2_MB".to_string(),
            compute_heap_size(RT2_MIB_PER_1080P).to_string(),
        );
    }

    // Handle BB_GAME_FREE_MB
    if let Some(free_mb) = player_env.get("BB_GAME_FREE_MB") {
        if is_all_digits(free_mb) {
            result.insert("BB_GAME_FREE_MB".to_string(), free_mb.clone());
        } else {
            result.insert("BB_GAME_FREE_MB".to_string(), "0".to_string());
        }
    } else {
        result.insert("BB_GAME_FREE_MB".to_string(), "0".to_string());
    }

    // Handle BB_GAME_MOVIE_MB
    if let Some(movie_mb) = player_env.get("BB_GAME_MOVIE_MB") {
        if is_all_digits(movie_mb) {
            result.insert("BB_GAME_MOVIE_MB".to_string(), movie_mb.clone());
        } else {
            result.insert(
                "BB_GAME_MOVIE_MB".to_string(),
                compute_movie_mb(width, height).to_string(),
            );
        }
    } else {
        result.insert(
            "BB_GAME_MOVIE_MB".to_string(),
            compute_movie_mb(width, height).to_string(),
        );
    }

    // Calculate pool size and BB_DMEM_MB
    let rt_mb = result
        .get("BB_GAME_RT_MB")
        .unwrap_or(&"0".to_string())
        .parse::<i64>()
        .unwrap_or(0);
    let rt2_mb = result
        .get("BB_GAME_RT2_MB")
        .unwrap_or(&"0".to_string())
        .parse::<i64>()
        .unwrap_or(0);
    let free_mb = result
        .get("BB_GAME_FREE_MB")
        .unwrap_or(&"0".to_string())
        .parse::<i64>()
        .unwrap_or(0);

    // Player-set heap values can be any digits that parse as i64, so use saturating maths:
    // an oversized value must clamp to the 16384 cap, not panic on overflow.
    let pool = rt_mb.saturating_add(rt2_mb).saturating_sub(free_mb);

    if let Some(dmem_mb) = player_env.get("BB_DMEM_MB") {
        if !dmem_mb.is_empty() {
            result.insert("BB_DMEM_MB".to_string(), dmem_mb.clone());
        } else if pool > 0 {
            let dmem_val = 5056i64.saturating_add(pool);
            result.insert("BB_DMEM_MB".to_string(), dmem_val.min(16384).to_string());
        }
    } else if pool > 0 {
        let dmem_val = 5056i64.saturating_add(pool);
        result.insert("BB_DMEM_MB".to_string(), dmem_val.min(16384).to_string());
    }

    result
}
