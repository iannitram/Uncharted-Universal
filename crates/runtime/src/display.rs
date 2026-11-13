//! The games' display configuration (piece 3.04, the deleted `src/runtime_display.c`): one init
//! function per executable stores the screen size (1920x1080), the aspect ratio (16:9 as a
//! float) and the sizes derived from it into a global settings block that the whole renderer
//! reads. Rewriting the immediates of that function makes the game allocate and render
//! everything at another size. `tests/test_display.c`'s cases are this module's tests now.
//!
//! `BB_GAME_RES=WxH` is the game's render and display size (default: untouched, 1920x1080).
//!
//! The init function is found by its code, not by an offset, so every version of the three
//! executables is covered: `mov [r+8],1920  mov [r+0xc],1080` ... stores of 1920/1080 at
//! +0x18/+0x1c, +0x24/+0x28, +0x38/+0x3c, the aspect `0x3fe38e39` at +0x20/+0x2c, and the two
//! values derived from the size (1920/8 and 1080/4) at +0x40/+0x44.
//!
//! Larger targets need a larger GPU memory budget ([`grow_budget`]): run.py computes the amounts
//! (`BB_GAME_RT_MB`, `BB_GAME_RT2_MB`, `BB_GAME_FREE_MB`, `BB_GAME_TEX_MB`) and the system direct
//! memory. `BB_GAME_MOVIE_MB` sizes the movie player's frame buffers ([`patch_movie_slots`]) and
//! `BB_GAME_SKIP=0x38,0x18` leaves block fields (displacements) at 1920x1080 (diagnostics).
//!
//! The patch itself, [`patch`], is plain byte work on any host; the loader calls it through
//! [`ffi::runtime_display_patch`] (x86-64 only) with the process environment.

/// The 16:9 aspect the games store, `1.7777778f32`.
const ASPECT_16_9: u32 = 0x3fe3_8e39;
/// How far past the first store a configuration block's stores reach.
const BLOCK_WINDOW: usize = 0x1e0;
/// The movie player's frame buffer size (4 MiB, enough for 1920x1088 NV12).
const MOVIE_FRAME: u32 = 0x40_0000;
/// `movabs rdx, 0x120000000` (4608 MiB): the memory validation function's system total.
const MEMORY_TOTAL: [u8; 10] = [0x48, 0xba, 0, 0, 0, 0x20, 1, 0, 0, 0];

/// The little-endian `u32` at `at`, or `None` past the end of `code`.
fn get32(code: &[u8], at: usize) -> Option<u32> {
    let bytes = code.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn get64(code: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(code[at..at + 8].try_into().unwrap())
}

fn put32(code: &mut [u8], at: usize, value: u32) {
    code[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(code: &mut [u8], at: usize, value: u64) {
    code[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// `printf("%#llx")`: `0` for zero, `0x...` otherwise.
fn offset(value: usize) -> String {
    if value == 0 {
        "0".to_owned()
    } else {
        format!("{value:#x}")
    }
}

/// A number as C's `strto*` functions read it at the start of `text`: leading white space, a
/// sign, for base 16 an optional `0x`, then at least one digit. Returns the magnitude (`None`
/// when it does not fit 64 bits), whether a minus sign came first and the bytes read, or `None`
/// without a digit.
fn c_number(text: &[u8], radix: u32) -> Option<(Option<u64>, bool, usize)> {
    let digit = |at: usize| {
        text.get(at)
            .and_then(|&byte| char::from(byte).to_digit(radix))
    };
    let mut at = text
        .iter()
        // C's `isspace`: space, tab, new line, vertical tab, form feed, carriage return.
        .position(|byte| !matches!(byte, b' ' | 0x09..=0x0d))
        .unwrap_or(text.len());
    let negative = text.get(at) == Some(&b'-');
    if matches!(text.get(at), Some(b'-' | b'+')) {
        at += 1;
    }
    if radix == 16
        && text.get(at) == Some(&b'0')
        && matches!(text.get(at + 1), Some(b'x' | b'X'))
        && digit(at + 2).is_some()
    {
        at += 2;
    }
    let start = at;
    let mut magnitude = Some(0u64);
    while let Some(d) = digit(at) {
        magnitude = magnitude
            .and_then(|value| value.checked_mul(u64::from(radix)))
            .and_then(|value| value.checked_add(u64::from(d)));
        at += 1;
    }
    (at > start).then_some((magnitude, negative, at))
}

/// `strtoul`: too large is `u64::MAX`, a minus sign negates modulo 2^64. Returns the value and
/// the bytes read, or `None` without a digit.
fn c_unsigned(text: &[u8], radix: u32) -> Option<(u64, usize)> {
    let (magnitude, negative, end) = c_number(text, radix)?;
    let value = match magnitude {
        None => u64::MAX,
        Some(value) if negative => value.wrapping_neg(),
        Some(value) => value,
    };
    Some((value, end))
}

/// `strtoll(text, NULL, 10)`: 0 without digits, clamped to the `i64` range.
fn c_signed(text: &[u8]) -> i64 {
    match c_number(text, 10) {
        None => 0,
        Some((Some(value), false, _)) => i64::try_from(value).unwrap_or(i64::MAX),
        Some((None, false, _)) => i64::MAX,
        // 2^63 itself is `i64::MIN` (`as` wraps it there, and negating it keeps it).
        Some((Some(value), true, _)) if value <= 1 << 63 => (value as i64).wrapping_neg(),
        Some((_, true, _)) => i64::MIN,
    }
}

/// `sscanf(res, "%ux%u")` (each number truncated to 32 bits, as C's `unsigned`).
fn parse_size(text: &[u8]) -> Option<(u32, u32)> {
    let (width, end) = c_unsigned(text, 10)?;
    let rest = text.get(end..)?.strip_prefix(b"x")?;
    let (height, _) = c_unsigned(rest, 10)?;
    Some((width as u32, height as u32))
}

/// A memory amount in MiB (`BB_GAME_*_MB`), 0 when unset or empty.
fn env_mb(var: &dyn Fn(&str) -> Option<String>, name: &str) -> i64 {
    var(name).map_or(0, |value| c_signed(value.as_bytes()))
}

/// `BB_GAME_SKIP`: hex displacements separated by one character each; bit `disp / 4` per field.
fn skip_fields(list: Option<String>) -> u64 {
    let mut skip = 0u64;
    let Some(list) = list else {
        return skip;
    };
    let mut text = list.as_bytes();
    while !text.is_empty() {
        let Some((disp, end)) = c_unsigned(text, 16) else {
            break;
        };
        if disp < 256 {
            skip |= 1 << (disp / 4);
        }
        text = text.get(end + 1..).unwrap_or(&[]);
    }
    skip
}

/// One size block: `start` is the first store (`mov [r+8],1920`). Patches the stores in the next
/// `window` bytes that match the known (displacement, immediate) pairs, except the fields in
/// `skip`, and the `movabs rax, (1080<<32)|1920` that fills +0x38; returns how many it changed.
pub fn patch_block(
    code: &mut [u8],
    start: usize,
    window: usize,
    size: (u32, u32),
    skip: u64,
) -> usize {
    let (w, h) = size;
    let reg = code[start + 1];
    let aspect = (w as f32 / h as f32).to_bits();
    let mut changed = 0;
    let mut i = start;
    while i + 7 <= start + window && i + 7 <= code.len() {
        if code[i] == 0xc7 && code[i + 1] == reg {
            // mov dword [r+disp8], imm32
            let disp = code[i + 2];
            let imm = get32(code, i + 3).unwrap_or_default();
            let value = match (imm, disp) {
                (1920, 0x08 | 0x18 | 0x24 | 0x38) => w,
                (1080, 0x0c | 0x1c | 0x28 | 0x3c) => h,
                (ASPECT_16_9, 0x20 | 0x2c) => aspect,
                (0xf0, 0x40) => w.div_ceil(8),
                (0x10e, 0x44) => h.div_ceil(4),
                _ => imm,
            };
            if value != imm && (skip >> (disp / 4)) & 1 == 0 {
                put32(code, i + 3, value);
                changed += 1;
                i += 6;
            }
        } else if i + 10 <= code.len()
            && code[i] == 0x48
            && code[i + 1] == 0xb8
            && get32(code, i + 2) == Some(1920)
            && get32(code, i + 6) == Some(1080)
        {
            put32(code, i + 2, w);
            put32(code, i + 6, h);
            changed += 1;
            i += 9;
        }
        i += 1;
    }
    changed
}

/// One entry change in the memory budget table of each executable (module data): entries
/// `{u32 id, u32 type, u64 size, u64 alignment, char *name}` of 32 bytes that end at id 0. The
/// game checks that every type's entries add up to a constant (type 2: the system direct
/// memory, 4608 MiB, whose entry 3 is the GPU pool; type 6: the GPU pool's heaps). The heaps
/// that hold the render targets are entry 133 (497 MiB; 561 in Uncharted 3) and entry 132
/// (140 MiB) of type 6; entry 14 is the pool's unreserved remainder. Their allocation failures
/// name them ("Render Target Heap"). The pool is one arena whose size the game keeps in 32
/// bits: it must stay below 4 GiB.
///
/// Every entry `id` of `kind` with a power-of-two alignment and a size of `min_mb..=max_mb` MiB
/// (that a negative `delta_mb` does not take below zero) changes by `delta_mb` MiB; returns how
/// many did.
#[allow(clippy::too_many_arguments)]
pub fn change_entry(
    image: &mut [u8],
    id: u32,
    kind: u32,
    min_mb: u64,
    max_mb: u64,
    delta_mb: i64,
    what: &str,
    log: &mut dyn FnMut(&str),
) -> usize {
    let mut changed = 0;
    let mut i = 0usize;
    while i + 32 <= image.len() {
        if get32(image, i) == Some(id) && get32(image, i + 4) == Some(kind) {
            let size = get64(image, i + 8);
            let alignment = get64(image, i + 16);
            let fits = alignment.is_power_of_two()
                && size >= min_mb << 20
                && size <= max_mb << 20
                && !(delta_mb < 0 && size < (delta_mb.wrapping_neg() as u64) << 20);
            if fits {
                let size = (size as i64).wrapping_add(delta_mb << 20) as u64;
                put64(image, i + 8, size);
                log(&format!(
                    "Display: {what} (image offset {}) {delta_mb:+} MiB, now {} MiB",
                    offset(i),
                    size >> 20
                ));
                changed += 1;
            }
        }
        i += 8;
    }
    changed
}

/// The GPU memory budget for a larger size: the render target heaps grow by `BB_GAME_RT_MB` and
/// `BB_GAME_RT2_MB`, the unreserved remainder and the texture heap give `BB_GAME_FREE_MB` and
/// `BB_GAME_TEX_MB`, the pool (and the memory validation's system total) grows by the sum.
pub fn grow_budget(
    image: &mut [u8],
    var: &dyn Fn(&str) -> Option<String>,
    log: &mut dyn FnMut(&str),
) {
    let rt = env_mb(var, "BB_GAME_RT_MB");
    let rt2 = env_mb(var, "BB_GAME_RT2_MB");
    let freed = env_mb(var, "BB_GAME_FREE_MB");
    let tex = env_mb(var, "BB_GAME_TEX_MB");
    let pool = rt.wrapping_add(rt2).wrapping_sub(freed).wrapping_sub(tex);
    if rt == 0 && rt2 == 0 && freed == 0 && tex == 0 {
        return;
    }
    if rt != 0 {
        change_entry(image, 133, 6, 300, 1500, rt, "render target heap", log);
    }
    if rt2 != 0 {
        change_entry(
            image,
            132,
            6,
            100,
            1000,
            rt2,
            "second render target heap",
            log,
        );
    }
    if freed != 0 {
        let min = (freed as u64).wrapping_add(4);
        change_entry(
            image,
            14,
            6,
            min,
            1000,
            freed.wrapping_neg(),
            "unreserved GPU memory",
            log,
        );
    }
    if tex != 0 {
        let min = (tex as u64).wrapping_add(256);
        change_entry(
            image,
            134,
            6,
            min,
            2000,
            tex.wrapping_neg(),
            "texture heap",
            log,
        );
    }
    if pool != 0 {
        change_entry(image, 3, 2, 2000, 4000, pool, "GPU memory pool", log);
    }
    // The memory validation function compares the system total with a constant
    // (movabs rdx, 0x120000000 = 4608 MiB): the constant moves with the pool.
    let mut i = 0usize;
    while i + MEMORY_TOTAL.len() <= image.len() {
        if image[i..i + MEMORY_TOTAL.len()] == MEMORY_TOTAL {
            let value = 0x1_2000_0000_i64.wrapping_add(pool << 20) as u64;
            put64(image, i + 2, value);
            log(&format!(
                "Display: memory total check (image offset {}) now {} MiB",
                offset(i),
                value >> 20
            ));
        }
        i += 1;
    }
}

/// The movie player gives the video decoder 4 frame buffers of 4 MiB (enough for 1920x1088
/// NV12) and the games read the decoded frame at their display size, so at larger sizes a frame
/// needs more (`BB_GAME_MOVIE_MB`; the HLE decoder fits the movie into the display size).
/// Registers differ per executable; the code is:
///
/// - `add r64, 0x400000 / mov qword [slot+0x225c0], 0x400000 / ... mov qword [..], 0x30 ...`
///   (per slot);
/// - `add r64, 0x400000 / sub r64, r64 / mov eax, 0xffffffff / and r64, rax` (the offset before
///   them);
/// - `cmp qword [r+0x10], 0x400000` (the decode path's check of the frame buffer size).
///
/// Each becomes `bytes`; returns how many immediates changed.
pub fn patch_movie_slots(image: &mut [u8], bytes: u32) -> usize {
    let mut patched = 0;
    let mut i = 0usize;
    while i + 64 < image.len() {
        let p = &image[i..];
        let at = |k: usize| get32(p, k);
        // the decode path's check of the frame buffer size: cmp qword [r+0x10], 0x400000
        if matches!(p[0], 0x48 | 0x49)
            && p[1] == 0x81
            && (0x78..=0x7f).contains(&p[2])
            && p[3] == 0x10
            && at(4) == Some(MOVIE_FRAME)
        {
            put32(image, i + 4, bytes);
            patched += 1;
            i += 1;
            continue;
        }
        let add = matches!(p[0], 0x48 | 0x49 | 0x4c | 0x4d)
            && p[1] == 0x81
            && p[2] & 0xf8 == 0xc0
            && at(3) == Some(MOVIE_FRAME);
        if !add {
            i += 1;
            continue;
        }
        // per-slot: followed by mov qword [mem], 0x400000 and a store of 0x30
        let slot = p[7] & 0xf8 == 0x48
            && p[8] == 0xc7
            && p[9] == 0x84
            && at(15) == Some(MOVIE_FRAME)
            && (18..56).any(|k| p[k] == 0xc7 && p[k + 1] == 0x84 && at(k + 7) == Some(0x30));
        // offset before the slots: add r, 0x400000 ; sub r, r ; mov eax, 0xffffffff ; and r, rax
        let offset_before =
            p[7] & 0xf8 == 0x48 && p[8] == 0x29 && p[10] == 0xb8 && at(11) == Some(0xffff_ffff);
        if slot {
            put32(image, i + 3, bytes);
            put32(image, i + 15, bytes);
            patched += 2;
        } else if offset_before {
            put32(image, i + 3, bytes);
            patched += 1;
        }
        i += 1;
    }
    patched
}

/// `BB_GAME_RES` (and the budget, skip and movie variables, read through `var`) applied to the
/// loaded executable `image`; each `Display:` line goes to `log`. Nothing changes without a
/// size of 640x360 to 7680x4320 other than 1920x1080; odd sizes are made even.
pub fn patch(image: &mut [u8], var: &dyn Fn(&str) -> Option<String>, log: &mut dyn FnMut(&str)) {
    let Some((w, h)) = var("BB_GAME_RES").and_then(|res| parse_size(res.as_bytes())) else {
        return;
    };
    if w < 640 || h < 360 || w > 7680 || h > 4320 || (w == 1920 && h == 1080) {
        return;
    }
    let (w, h) = (w & !1, h & !1);
    grow_budget(image, var, log);
    let skip = skip_fields(var("BB_GAME_SKIP"));
    let (mut blocks, mut stores) = (0usize, 0usize);
    // mov dword [r+8],1920 ; mov dword [r+0xc],1080  (same base register)
    let mut i = 0usize;
    while i + 14 <= image.len() {
        let block = image[i] == 0xc7
            && image[i + 1] & 0xf8 == 0x40
            && image[i + 2] == 0x08
            && get32(image, i + 3) == Some(1920)
            && image[i + 7] == 0xc7
            && image[i + 8] == image[i + 1]
            && image[i + 9] == 0x0c
            && get32(image, i + 10) == Some(1080);
        if block {
            stores += patch_block(image, i, BLOCK_WINDOW, (w, h), skip);
            blocks += 1;
        }
        i += 1;
    }
    let movie = env_mb(var, "BB_GAME_MOVIE_MB");
    if movie > 4 {
        let places = patch_movie_slots(image, (movie as u32) << 20);
        log(&format!(
            "Display: movie frame buffers {movie} MiB: {places} places"
        ));
    }
    log(&format!(
        "Display: BB_GAME_RES {w}x{h}: {blocks} configuration block(s), {stores} stores changed"
    ));
}

/// The exported C function of this module, declared in `include/uncharted_runtime.h`.
#[cfg(target_arch = "x86_64")]
pub mod ffi {
    use crate::abi::host;

    /// `BB_GAME_RES`: rewrites the display configuration the game initializes, the memory
    /// budget and the movie frame buffers in the loaded executable (`image_size` bytes at
    /// `image`), from the process environment; prints a `Display:` line per change.
    ///
    /// # Safety
    /// `image` is null or points to `image_size` writable bytes.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_display_patch(image: *mut u8, image_size: u64) {
        if image.is_null() {
            return;
        }
        // SAFETY: the loader passes its loaded image, `image_size` writable bytes.
        let image = unsafe { std::slice::from_raw_parts_mut(image, image_size as usize) };
        let var =
            |name: &str| std::env::var_os(name).map(|value| value.to_string_lossy().into_owned());
        super::patch(image, &var, &mut |line| host::out_line(line));
    }
}

#[cfg(test)]
mod tests {
    //! `tests/test_display.c` and the byte patterns of each patch on synthetic images.

    use super::*;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn run(image: &mut [u8], pairs: &[(&str, &str)]) -> Vec<String> {
        let mut lines = Vec::new();
        patch(image, &vars(pairs), &mut |line| lines.push(line.to_owned()));
        lines
    }

    fn get(image: &[u8], at: usize) -> u32 {
        get32(image, at).unwrap()
    }

    fn put_entry(image: &mut [u8], at: usize, id: u32, kind: u32, mb: u64) {
        put32(image, at, id);
        put32(image, at + 4, kind);
        put64(image, at + 8, mb << 20);
        put64(image, at + 16, 0x10_0000);
    }

    const SIZE: usize = 0x4000;
    const CODE: usize = 0x100;
    const TABLE: usize = 0x1000;
    const TOTAL: usize = 0x800;

    /// `test_display.c`'s image: the configuration init function (as in the games), the memory
    /// budget table and the validation constant.
    fn synthetic_image() -> Vec<u8> {
        let mut image = vec![0u8; SIZE];
        // mov dword [rax+disp8], imm32
        let stores: [(u8, u32); 16] = [
            (0x08, 1920),
            (0x0c, 1080),
            (0x14, 0),
            (0x18, 1920),
            (0x1c, 1080),
            (0x20, ASPECT_16_9),
            (0x24, 1920),
            (0x28, 1080),
            (0x2c, ASPECT_16_9),
            (0x10, 0x3f80_0000),
            (0x40, 0xf0),
            (0x44, 0x10e),
            (0x30, 0xa00),
            (0x34, 0x4c0),
            (0x38, 1920),
            (0x3c, 1080),
        ];
        for (n, (disp, imm)) in stores.into_iter().enumerate() {
            let at = CODE + n * 7;
            image[at..at + 3].copy_from_slice(&[0xc7, 0x40, disp]);
            put32(&mut image, at + 3, imm);
        }
        image[TOTAL..TOTAL + 10].copy_from_slice(&MEMORY_TOTAL);
        put_entry(&mut image, TABLE, 3, 2, 2988);
        put_entry(&mut image, TABLE + 32, 132, 6, 140);
        put_entry(&mut image, TABLE + 64, 133, 6, 497);
        put_entry(&mut image, TABLE + 96, 14, 6, 177);
        image
    }

    fn store(image: &[u8], n: usize) -> u32 {
        get(image, CODE + n * 7 + 3)
    }

    #[test]
    fn display_patch_on_a_synthetic_image() {
        let mut image = synthetic_image();
        let lines = run(
            &mut image,
            &[
                ("BB_GAME_RES", "2561x1441"), // odd numbers are made even
                ("BB_GAME_RT_MB", "256"),
                ("BB_GAME_RT2_MB", "64"),
                ("BB_GAME_FREE_MB", "100"),
            ],
        );
        assert_eq!((store(&image, 0), store(&image, 1)), (2560, 1440));
        assert_eq!((store(&image, 3), store(&image, 4)), (2560, 1440));
        assert_eq!((store(&image, 6), store(&image, 7)), (2560, 1440));
        assert_eq!((store(&image, 14), store(&image, 15)), (2560, 1440));
        let aspect = f32::from_bits(store(&image, 5));
        assert!(aspect > 1.77 && aspect < 1.78);
        // Still 16:9: the two aspect stores keep their value and are not counted as changed.
        assert_eq!(store(&image, 5), ASPECT_16_9);
        assert_eq!(store(&image, 8), store(&image, 5));
        assert_eq!((store(&image, 10), store(&image, 11)), (320, 360)); // 2560/8, 1440/4
        assert_eq!(store(&image, 12), 0xa00); // untouched
        assert_eq!(store(&image, 9), 0x3f80_0000);
        assert_eq!(get64(&image, TABLE + 8) >> 20, 2988 + 220); // pool: 256 + 64 - 100
        assert_eq!(get64(&image, TABLE + 32 + 8) >> 20, 140 + 64);
        assert_eq!(get64(&image, TABLE + 64 + 8) >> 20, 497 + 256);
        assert_eq!(get64(&image, TABLE + 96 + 8) >> 20, 77);
        assert_eq!(get64(&image, TOTAL + 2) >> 20, 4608 + 220);
        assert_eq!(
            lines,
            [
                "Display: render target heap (image offset 0x1040) +256 MiB, now 753 MiB",
                "Display: second render target heap (image offset 0x1020) +64 MiB, now 204 MiB",
                "Display: unreserved GPU memory (image offset 0x1060) -100 MiB, now 77 MiB",
                "Display: GPU memory pool (image offset 0x1000) +220 MiB, now 3208 MiB",
                "Display: memory total check (image offset 0x800) now 4828 MiB",
                "Display: BB_GAME_RES 2560x1440: 1 configuration block(s), 10 stores changed",
            ]
        );

        // The default size changes nothing, and prints nothing.
        let mut same = image.clone();
        let lines = run(
            &mut same,
            &[("BB_GAME_RES", "1920x1080"), ("BB_GAME_RT_MB", "256")],
        );
        assert_eq!(same, image);
        assert!(lines.is_empty());
    }

    #[test]
    fn sizes_outside_the_range_or_unreadable_change_nothing() {
        let original = synthetic_image();
        for res in [
            "",
            "garbage",
            "639x360",
            "640x359",
            "7681x4320",
            "7680x4321",
            "1920x1080",
            "1280 720",
            "1280X720",
            "-1280x720",
            "x720",
            "1280x",
        ] {
            let mut image = original.clone();
            let lines = run(&mut image, &[("BB_GAME_RES", res), ("BB_GAME_RT_MB", "64")]);
            assert_eq!(image, original, "{res:?}");
            assert!(lines.is_empty(), "{res:?}");
        }
        let mut image = original.clone();
        assert!(run(&mut image, &[]).is_empty());
        assert_eq!(image, original);
        // As `sscanf("%ux%u")`: leading blanks, a plus sign and trailing text are accepted.
        for res in [
            "1280x720",
            " 1280x 720",
            "+1280x720",
            "1280x720p",
            "1281x721",
        ] {
            let mut image = original.clone();
            let lines = run(&mut image, &[("BB_GAME_RES", res)]);
            assert_eq!((store(&image, 0), store(&image, 1)), (1280, 720), "{res:?}");
            assert_eq!(
                lines.last().unwrap(),
                "Display: BB_GAME_RES 1280x720: 1 configuration block(s), 10 stores changed"
            );
        }
        // The edges of the range.
        let mut image = original.clone();
        run(&mut image, &[("BB_GAME_RES", "640x360")]);
        assert_eq!((store(&image, 0), store(&image, 1)), (640, 360));
        assert_eq!((store(&image, 10), store(&image, 11)), (80, 90));
        let mut image = original.clone();
        run(&mut image, &[("BB_GAME_RES", "7680x4320")]);
        assert_eq!((store(&image, 0), store(&image, 1)), (7680, 4320));
    }

    #[test]
    fn derived_values_round_up() {
        let mut image = synthetic_image();
        let lines = run(&mut image, &[("BB_GAME_RES", "1366x770")]);
        assert_eq!((store(&image, 0), store(&image, 1)), (1366, 770));
        assert_eq!((store(&image, 10), store(&image, 11)), (171, 193)); // (1366+7)/8, (770+3)/4
                                                                        // Not 16:9: both aspect stores change too.
        assert_eq!(store(&image, 5), (1366f32 / 770f32).to_bits());
        assert_eq!(store(&image, 8), store(&image, 5));
        assert_eq!(
            lines,
            ["Display: BB_GAME_RES 1366x770: 1 configuration block(s), 12 stores changed"]
        );
    }

    #[test]
    fn skipped_fields_stay_at_1920x1080() {
        let mut image = synthetic_image();
        let lines = run(
            &mut image,
            &[("BB_GAME_RES", "2560x1440"), ("BB_GAME_SKIP", "0x38,0x18")],
        );
        // +0x18 and +0x38 keep 1920; their heights (+0x1c, +0x3c) are other fields.
        assert_eq!((store(&image, 3), store(&image, 4)), (1920, 1440));
        assert_eq!((store(&image, 14), store(&image, 15)), (1920, 1440));
        assert_eq!((store(&image, 0), store(&image, 6)), (2560, 2560));
        assert_eq!(
            lines,
            ["Display: BB_GAME_RES 2560x1440: 1 configuration block(s), 8 stores changed"]
        );
        // Bit `disp / 4`: 0x1c shares nothing with 0x18; junk ends the list; >= 256 is ignored.
        assert_eq!(skip_fields(Some("1c".into())), 1 << 7);
        assert_eq!(
            skip_fields(Some("0x8;100;0X44".into())),
            (1 << 2) | (1 << 17)
        );
        assert_eq!(skip_fields(Some("8,zz,c".into())), 1 << 2);
        assert_eq!(skip_fields(Some(",8".into())), 0);
        assert_eq!(skip_fields(None), 0);
    }

    #[test]
    fn the_wide_store_and_the_window_end() {
        let mut image = vec![0u8; 0x400];
        // the first two stores of a block on rbx (0x43), then movabs rax, (1080<<32)|1920
        image[0x10..0x13].copy_from_slice(&[0xc7, 0x43, 0x08]);
        put32(&mut image, 0x13, 1920);
        image[0x17..0x1a].copy_from_slice(&[0xc7, 0x43, 0x0c]);
        put32(&mut image, 0x1a, 1080);
        image[0x40..0x42].copy_from_slice(&[0x48, 0xb8]);
        put32(&mut image, 0x42, 1920);
        put32(&mut image, 0x46, 1080);
        // a store on another base register is not part of the block
        image[0x60..0x63].copy_from_slice(&[0xc7, 0x40, 0x18]);
        put32(&mut image, 0x63, 1920);
        // past the window: untouched
        let late = 0x10 + BLOCK_WINDOW;
        image[late..late + 3].copy_from_slice(&[0xc7, 0x43, 0x18]);
        put32(&mut image, late + 3, 1920);
        let lines = run(&mut image, &[("BB_GAME_RES", "3840x2160")]);
        assert_eq!((get(&image, 0x13), get(&image, 0x1a)), (3840, 2160));
        assert_eq!((get(&image, 0x42), get(&image, 0x46)), (3840, 2160));
        assert_eq!(get(&image, 0x63), 1920);
        assert_eq!(get(&image, late + 3), 1920);
        assert_eq!(
            lines,
            ["Display: BB_GAME_RES 3840x2160: 1 configuration block(s), 3 stores changed"]
        );
    }

    #[test]
    fn budget_entries_are_checked_before_they_change() {
        let mut image = vec![0u8; 0x200];
        put_entry(&mut image, 0x00, 133, 6, 497); // changes
        put_entry(&mut image, 0x20, 133, 6, 200); // below 300 MiB
        put_entry(&mut image, 0x40, 133, 6, 1600); // above 1500 MiB
        put_entry(&mut image, 0x60, 133, 6, 561); // alignment not a power of two
        put64(&mut image, 0x70, 0x30_0000);
        put_entry(&mut image, 0x80, 133, 6, 561); // alignment 0
        put64(&mut image, 0x90, 0);
        put_entry(&mut image, 0xa0, 133, 2, 561); // another type
        put_entry(&mut image, 0xc8, 14, 6, 10); // 8-byte aligned, too small to give 100
        put_entry(&mut image, 0xe8, 14, 6, 177); // gives 100
        put_entry(&mut image, 0x108, 134, 6, 1024); // texture heap gives 64
        put_entry(&mut image, 0x128, 3, 2, 2988); // pool: 256 - 100 - 64
        let mut lines = Vec::new();
        let var = vars(&[
            ("BB_GAME_RT_MB", "256"),
            ("BB_GAME_FREE_MB", "100"),
            ("BB_GAME_TEX_MB", "64"),
        ]);
        grow_budget(&mut image, &var, &mut |line| lines.push(line.to_owned()));
        let mb = |at: usize| get64(&image, at + 8) >> 20;
        assert_eq!(
            [mb(0), mb(0x20), mb(0x40), mb(0x60), mb(0x80), mb(0xa0)],
            [753, 200, 1600, 561, 561, 561]
        );
        assert_eq!(
            [mb(0xc8), mb(0xe8), mb(0x108), mb(0x128)],
            [10, 77, 960, 3080]
        );
        assert_eq!(
            lines,
            [
                "Display: render target heap (image offset 0) +256 MiB, now 753 MiB",
                "Display: unreserved GPU memory (image offset 0xe8) -100 MiB, now 77 MiB",
                "Display: texture heap (image offset 0x108) -64 MiB, now 960 MiB",
                "Display: GPU memory pool (image offset 0x128) +92 MiB, now 3080 MiB",
            ]
        );
        // Nothing set: nothing at all, not even the total check.
        let mut image = vec![0u8; 0x40];
        image[0x10..0x1a].copy_from_slice(&MEMORY_TOTAL);
        let before = image.clone();
        grow_budget(&mut image, &vars(&[("BB_GAME_RT_MB", "junk")]), &mut |_| {});
        assert_eq!(image, before);
        // A budget whose pool does not move still rewrites the total to the same value.
        let mut lines = Vec::new();
        let var = vars(&[("BB_GAME_RT_MB", "64"), ("BB_GAME_FREE_MB", "64")]);
        grow_budget(&mut image, &var, &mut |line| lines.push(line.to_owned()));
        assert_eq!(image, before);
        assert_eq!(
            lines,
            ["Display: memory total check (image offset 0x10) now 4608 MiB"]
        );
    }

    /// The three movie sites, as the games have them.
    fn movie_image() -> Vec<u8> {
        let mut image = vec![0x90u8; 0x200];
        // cmp qword [rbx+0x10], 0x400000
        image[0x10..0x14].copy_from_slice(&[0x48, 0x81, 0x7b, 0x10]);
        put32(&mut image, 0x14, MOVIE_FRAME);
        // add rcx, 0x400000 ; mov qword [rax+disp32], 0x400000 ; ... mov qword [..], 0x30
        image[0x40..0x43].copy_from_slice(&[0x48, 0x81, 0xc1]);
        put32(&mut image, 0x43, MOVIE_FRAME);
        image[0x47..0x4a].copy_from_slice(&[0x48, 0xc7, 0x84]);
        put32(&mut image, 0x4f, MOVIE_FRAME);
        image[0x60..0x62].copy_from_slice(&[0xc7, 0x84]);
        put32(&mut image, 0x67, 0x30);
        // add r8, 0x400000 ; sub r8, rcx ; mov eax, 0xffffffff ; and r8, rax
        image[0xa0..0xa3].copy_from_slice(&[0x49, 0x81, 0xc0]);
        put32(&mut image, 0xa3, MOVIE_FRAME);
        image[0xa7..0xa9].copy_from_slice(&[0x49, 0x29]);
        image[0xaa] = 0xb8;
        put32(&mut image, 0xab, 0xffff_ffff);
        // add rdx, 0x400000 with neither tail: untouched
        image[0x100..0x103].copy_from_slice(&[0x48, 0x81, 0xc2]);
        put32(&mut image, 0x103, MOVIE_FRAME);
        image
    }

    #[test]
    fn movie_frame_buffers_grow_at_each_site() {
        let mut image = movie_image();
        assert_eq!(patch_movie_slots(&mut image, 8 << 20), 4);
        assert_eq!(
            [
                get(&image, 0x14),
                get(&image, 0x43),
                get(&image, 0x4f),
                get(&image, 0xa3)
            ],
            [8 << 20; 4]
        );
        assert_eq!(get(&image, 0x103), MOVIE_FRAME);
        // Through `patch`: only above 4 MiB, with its line before the summary.
        let mut image = movie_image();
        let lines = run(
            &mut image,
            &[("BB_GAME_RES", "2560x1440"), ("BB_GAME_MOVIE_MB", "12")],
        );
        assert_eq!(get(&image, 0x43), 12 << 20);
        assert_eq!(
            lines,
            [
                "Display: movie frame buffers 12 MiB: 4 places",
                "Display: BB_GAME_RES 2560x1440: 0 configuration block(s), 0 stores changed",
            ]
        );
        let original = movie_image();
        let mut image = original.clone();
        run(
            &mut image,
            &[("BB_GAME_RES", "2560x1440"), ("BB_GAME_MOVIE_MB", "4")],
        );
        assert_eq!(image, original);
    }

    #[test]
    fn a_site_at_the_very_end_is_not_read_past_the_image() {
        // The slot search looks 66 bytes ahead; the last position checked is 65 bytes from the
        // end, so its final store would be one byte past the image: it never matches.
        let mut image = movie_image();
        let end = 0x200 - 65;
        image[end..end + 3].copy_from_slice(&[0x48, 0x81, 0xc1]);
        put32(&mut image, end + 3, MOVIE_FRAME);
        image[end + 7..end + 10].copy_from_slice(&[0x48, 0xc7, 0x84]);
        put32(&mut image, end + 15, MOVIE_FRAME);
        image[end + 55] = 0xc7;
        image[end + 56] = 0x84;
        assert_eq!(patch_movie_slots(&mut image, 8 << 20), 4);
        assert_eq!(get(&image, end + 3), MOVIE_FRAME);
    }

    #[test]
    fn numbers_read_as_c_reads_them() {
        assert_eq!(c_signed(b"256"), 256);
        assert_eq!(c_signed(b"  -100MiB"), -100);
        assert_eq!(c_signed(b"+7"), 7);
        assert_eq!(c_signed(b"-1"), -1);
        assert_eq!(c_signed(b"junk"), 0);
        assert_eq!(c_signed(b""), 0);
        assert_eq!(c_signed(b"99999999999999999999"), i64::MAX);
        assert_eq!(c_signed(b"-99999999999999999999"), i64::MIN);
        assert_eq!(c_signed(b"-9223372036854775808"), i64::MIN);
        assert_eq!(parse_size(b"2560x1440"), Some((2560, 1440)));
        assert_eq!(parse_size(b"4294968576x720"), Some((1280, 720)));
        assert_eq!(parse_size(b"1280*720"), None);
        assert_eq!(offset(0), "0");
        assert_eq!(offset(0x1040), "0x1040");
    }
}
