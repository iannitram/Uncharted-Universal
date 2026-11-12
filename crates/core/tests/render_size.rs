//! `render_size` patches on synthetic byte fixtures: ports of `tests/test_render_res.py` and
//! `tests/test_render_720p.py` (issue #277). No game bytes; the fixtures are built here.

use uncharted_core::error::Error;
use uncharted_core::games::Game;
use uncharted_core::render_size::{
    apply_checked, deck_720p_patches, deck_720p_writes, parse_size, patch_image,
    patch_image_with_writes, CheckedWrite,
};

const MIB: u64 = 1 << 20;

/// A stand-in for boot-linked.bin: the display settings stores, the memory table and its check.
fn image(base: u8) -> Vec<u8> {
    let mut data = vec![0x90u8; 0x200];
    for (disp, value) in [
        (0x08u8, 1920u32),
        (0x0c, 1080),
        (0x18, 1920),
        (0x1c, 1080),
        (0x24, 1920),
        (0x28, 1080),
        (0x30, 2560),
        (0x34, 1216),
        (0x38, 1920),
        (0x3c, 1080),
        (0x40, 240),
        (0x44, 270),
    ] {
        data.extend_from_slice(&[0xc7, base, disp]);
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(&[0x48, 0xb8]);
    data.extend_from_slice(&1920u32.to_le_bytes());
    data.extend_from_slice(&1080u32.to_le_bytes());
    data.extend(std::iter::repeat_n(0x90, 0x900));
    for (id, parent, size, align) in [
        (2u32, 2u32, 1536 * MIB, 0x100000u64),
        (3, 2, 2988 * MIB, 0x100000),
        (0x85, 6, 497 * MIB, 0x80),
        (0x84, 6, 128 * MIB, 0x80),
    ] {
        data.extend_from_slice(&id.to_le_bytes());
        data.extend_from_slice(&parent.to_le_bytes());
        data.extend_from_slice(&size.to_le_bytes());
        data.extend_from_slice(&align.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
    }
    data.extend_from_slice(&[0x48, 0x8d, 0x75, 0xe0, 0x48, 0xba]);
    data.extend_from_slice(&0x1_2000_0000u64.to_le_bytes());
    data.extend(std::iter::repeat_n(0, 64));
    data
}

fn store(base: u8, disp: u8, value: u32) -> Vec<u8> {
    let mut bytes = vec![0xc7, base, disp];
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn expect_render_size_error<T: std::fmt::Debug>(result: Result<T, Error>) -> String {
    match result {
        Err(Error::RenderSize(message)) => message,
        other => panic!("expected a RenderSize error, got {other:?}"),
    }
}

#[test]
fn stores_and_memory_at_1440p() {
    let patched = patch_image(&image(0x47), Game::U2, (2560, 1440)).unwrap();
    assert_eq!(patched.render, (2560, 1440));
    assert!(patched.extra_mib > 0);
    for (disp, value) in [(0x08, 2560), (0x0c, 1440), (0x40, 320), (0x44, 360)] {
        assert!(contains(&patched.image, &store(0x47, disp, value)));
    }
    assert!(!contains(&patched.image, &store(0x47, 0x08, 1920)));
}

#[test]
fn rax_form() {
    let patched = patch_image(&image(0x40), Game::U3, (2560, 1440)).unwrap();
    assert!(contains(&patched.image, &store(0x40, 0x08, 2560)));
}

#[test]
fn other_base_register() {
    let patched = patch_image(&image(0x46), Game::U2, (2560, 1440)).unwrap();
    assert!(contains(&patched.image, &store(0x46, 0x08, 2560)));
    assert!(contains(&patched.image, &store(0x46, 0x44, 360)));
}

#[test]
fn drakes_fortune_steps_down() {
    let patched = patch_image(&image(0x47), Game::U1, (3840, 2160)).unwrap();
    assert_eq!(patched.render, (2880, 1620));
}

#[test]
fn four_k_moves_heap_for_u2_u3() {
    let patched = patch_image(&image(0x47), Game::U3, (3840, 2160)).unwrap();
    assert_eq!(patched.render, (3840, 2160));
    assert!(patched.extra_mib > 2000);
}

#[test]
fn memory_table_and_total_check_follow_the_extra_memory() {
    let source = image(0x47);
    let patched = patch_image(&source, Game::U2, (2560, 1440)).unwrap();
    let total = 0x1_2000_0000u64 + u64::from(patched.extra_mib) * MIB;
    let check = patched
        .image
        .windows(14)
        .position(|w| w[..3] == [0x48, 0x8d, 0x75] && w[4..6] == [0x48, 0xba])
        .unwrap();
    assert_eq!(
        u64::from_le_bytes(patched.image[check + 6..check + 14].try_into().unwrap()),
        total
    );
    assert_eq!(patched.image.len(), source.len());
}

#[test]
fn native_size_changes_no_store() {
    let source = image(0x47);
    let patched = patch_image(&source, Game::U2, (1920, 1080)).unwrap();
    assert_eq!(patched.render, (1920, 1080));
    assert_eq!(patched.extra_mib, 0);
    assert_eq!(patched.image, source);
}

#[test]
fn applying_the_recorded_writes_reproduces_the_image() {
    let source = image(0x47);
    let (patched, writes) = patch_image_with_writes(&source, Game::U3, (3840, 2160)).unwrap();
    assert!(!writes.is_empty());
    assert_eq!(apply_checked(&source, &writes).unwrap(), patched.image);
}

#[test]
fn unlike_images_are_refused() {
    // No display settings code, then a missing memory table, then a doubled one.
    let message = expect_render_size_error(patch_image(&[0x90; 0x400], Game::U2, (2560, 1440)));
    assert!(message.contains("display settings code not found"));

    let mut no_table = image(0x47);
    no_table.truncate(0xb00);
    let message = expect_render_size_error(patch_image(&no_table, Game::U2, (2560, 1440)));
    assert!(message.contains("memory table entry"));

    let mut doubled = image(0x47);
    let table_start = doubled.len() - 4 * 32 - 14 - 64;
    let first_entry = doubled[table_start..table_start + 32].to_vec();
    doubled.extend_from_slice(&first_entry);
    let message = expect_render_size_error(patch_image(&doubled, Game::U2, (2560, 1440)));
    assert!(message.contains("found 2 times"));
}

#[test]
fn off_16_9_sizes_step_down_in_the_same_shape() {
    // 21:9 at the largest size cannot fit Drake's Fortune; a smaller same-shape size is chosen.
    let patched = patch_image(&image(0x47), Game::U1, (5120, 2160)).unwrap();
    assert!(patched.render.0 < 5120);
    assert_eq!(patched.render.0 % 16, 0);
    assert_eq!(patched.render.1 % 4, 0);
}

#[test]
fn parse_size_accepts_and_refuses() {
    assert_eq!(parse_size("2560x1440").unwrap(), (2560, 1440));
    assert_eq!(parse_size("1920X1080").unwrap(), (1920, 1080));
    assert_eq!(parse_size(" 960 x 540 ").unwrap(), (960, 540));
    assert_eq!(parse_size("640x360").unwrap(), (640, 360));
    assert_eq!(parse_size("7680x4320").unwrap(), (7680, 4320));
    let message = expect_render_size_error(parse_size("2561x1440"));
    assert!(message.contains("2561x1440 is not supported"));
    for bad in [
        "2560x1442",
        "632x360",
        "7688x4320",
        "640x356",
        "-640x360",
        "0x0",
        "1920",
        "1920x1080x3",
        "axb",
        "",
    ] {
        parse_size(bad).expect_err(bad);
    }
    assert!(expect_render_size_error(parse_size("big")).contains("is not WIDTHxHEIGHT"));
}

// ---- 720p Deck mode (tests/test_render_720p.py) ----

const BASE: usize = 0x4000; // file offset of image offset 0, as in the games

fn setup(reg: u8) -> Vec<u8> {
    let mut code = Vec::new();
    for (field, value) in [
        (0x08u8, 1920u32),
        (0x0c, 1080),
        (0x18, 1920),
        (0x1c, 1080),
        (0x24, 1920),
        (0x28, 1080),
        (0x40, 240),
        (0x44, 270),
        (0x38, 1920),
        (0x3c, 1080),
    ] {
        code.extend(store(reg, field, value));
    }
    code
}

/// cmp $1, %esi; jne +0x40 (the 720p case); then the 1080p case.
fn mode_switch() -> Vec<u8> {
    let mut code = vec![0x83, 0xfe, 0x01, 0x0f, 0x85];
    code.extend_from_slice(&0x40i32.to_le_bytes());
    code.extend(setup(0x47));
    code
}

fn decoder() -> Vec<u8> {
    vec![
        0xc7, 0x44, 0x24, 0x58, 0x80, 0x07, 0x00, 0x00, 0xc7, 0x44, 0x24, 0x5c, 0x40, 0x04, 0x00,
        0x00,
    ]
}

/// A tiny ELF: header with one program header (offset BASE, vaddr 0), then the pieces.
fn elf(pieces: &[Vec<u8>]) -> (Vec<u8>, Vec<usize>) {
    let mut body = vec![0u8; 0x40];
    body[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes());
    body[0x38..0x3a].copy_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&5u32.to_le_bytes());
    body.extend_from_slice(&(BASE as u64).to_le_bytes());
    for value in [0u64, 0, 0x1000, 0x1000, 0x1000] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    body.resize(BASE, 0);
    let mut offsets = Vec::new();
    for piece in pieces {
        body.extend(std::iter::repeat_n(0x90, 16));
        offsets.push(body.len() - BASE);
        body.extend_from_slice(piece);
    }
    (body, offsets)
}

fn values_near(writes: &[CheckedWrite], site: usize) -> Vec<u32> {
    let mut values: Vec<u32> = writes
        .iter()
        .filter(|w| (site as u64..site as u64 + 0x50).contains(&w.offset))
        .map(|w| u32::from_le_bytes(w.bytes.clone().try_into().unwrap()))
        .collect();
    values.sort_unstable();
    values
}

fn wanted_values() -> Vec<u32> {
    let mut values = vec![1280, 1280, 1280, 1280, 720, 720, 720, 720, 160, 180];
    values.sort_unstable();
    values
}

fn find(writes: &[CheckedWrite], offset: usize) -> &CheckedWrite {
    writes.iter().find(|w| w.offset == offset as u64).unwrap()
}

#[test]
fn deck_patches_defaults_mode_switch_and_decoder() {
    let (data, offsets) = elf(&[setup(0x47), setup(0x40), mode_switch(), decoder()]);
    let (first, second, switch, dec) = (offsets[0], offsets[1], offsets[2], offsets[3]);
    let writes = deck_720p_patches(&data).unwrap();
    // The 1080p case jumps (rel32 one byte longer, nop) to its 720p target.
    let mut jump = vec![0xe9];
    jump.extend_from_slice(&0x41i32.to_le_bytes());
    jump.push(0x90);
    assert_eq!(find(&writes, switch + 3).bytes, jump);
    for site in [first, second] {
        assert_eq!(values_near(&writes, site), wanted_values());
    }
    assert_eq!(find(&writes, dec + 4).bytes, 1280u32.to_le_bytes());
    assert_eq!(find(&writes, dec + 12).bytes, 720u32.to_le_bytes());
    assert_eq!(writes.len(), 2 * 10 + 1 + 2);
    assert_eq!(deck_720p_writes(&data).unwrap().len(), writes.len());
}

#[test]
fn deck_one_setup_block_is_patched() {
    // 1 to 3 blocks are accepted; the file ends right after the decoder, so the field scan
    // must also stay inside the file.
    let (data, offsets) = elf(&[setup(0x47), decoder()]);
    let writes = deck_720p_patches(&data).unwrap();
    assert_eq!(values_near(&writes, offsets[0]), wanted_values());
    assert_eq!(find(&writes, offsets[1] + 4).bytes, 1280u32.to_le_bytes());
    assert_eq!(find(&writes, offsets[1] + 12).bytes, 720u32.to_le_bytes());
    assert_eq!(writes.len(), 10 + 2);
}

#[test]
fn deck_other_executables_are_left_alone() {
    // No video setting site (a decoder alone), more than three, or no movie decoder.
    let cases: Vec<Vec<Vec<u8>>> = vec![
        vec![decoder()],
        vec![
            setup(0x47),
            setup(0x47),
            setup(0x47),
            setup(0x47),
            decoder(),
        ],
        vec![setup(0x47), setup(0x40)],
    ];
    for pieces in cases {
        let (data, _) = elf(&pieces);
        expect_render_size_error(deck_720p_writes(&data));
    }
    let (data, _) = elf(&[decoder()]);
    assert!(expect_render_size_error(deck_720p_writes(&data))
        .contains("expected 1 to 3 video setting sites, found 0"));
}

#[test]
fn deck_rex_prefixed_registers_and_short_files() {
    // r8-r15 bases carry a 0x41 prefix; the first byte of a site may be the prefix.
    let mut prefixed = Vec::new();
    for (field, value) in [(0x08u8, 1920u32), (0x0c, 1080)] {
        prefixed.push(0x41);
        prefixed.extend(store(0x40, field, value));
    }
    let (data, offsets) = elf(&[prefixed, decoder()]);
    let writes = deck_720p_patches(&data).unwrap();
    assert_eq!(find(&writes, offsets[0] + 4).bytes, 1280u32.to_le_bytes());
    // Garbage that is not an ELF never panics.
    for bad in [vec![], vec![0u8; 8], vec![0u8; 0x30]] {
        expect_render_size_error(deck_720p_writes(&bad));
    }
}

// ---- Refusing a mismatch ----

#[test]
fn a_mismatch_at_the_patch_site_is_refused_and_writes_nothing() {
    let (data, _) = elf(&[setup(0x47), decoder()]);
    let writes = deck_720p_patches(&data).unwrap();
    // The image is the file from BASE on; flip a byte at the first patch site.
    let image = data[BASE..].to_vec();
    assert_eq!(apply_checked(&image, &writes).unwrap().len(), image.len());
    let mut wrong = image.clone();
    wrong[writes[0].offset as usize] ^= 0xff;
    let message = expect_render_size_error(apply_checked(&wrong, &writes));
    assert!(message.contains("does not match"));
    assert!(message.contains("nothing was written"));
}

#[test]
fn a_patch_outside_the_image_is_refused() {
    let write = CheckedWrite {
        offset: 8,
        expect: vec![0; 4],
        bytes: vec![1; 4],
    };
    assert!(expect_render_size_error(apply_checked(&[0; 10], &[write])).contains("outside"));
    let huge = CheckedWrite {
        offset: u64::MAX - 1,
        expect: vec![0; 4],
        bytes: vec![1; 4],
    };
    expect_render_size_error(apply_checked(&[0; 10], &[huge]));
    let uneven = CheckedWrite {
        offset: 0,
        expect: vec![0; 2],
        bytes: vec![1; 4],
    };
    expect_render_size_error(apply_checked(&[0; 10], &[uneven]));
}
