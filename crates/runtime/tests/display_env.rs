//! `runtime_display_patch` with the process environment, as the loader calls it (one test:
//! it sets the variables of this process).
#![cfg(target_arch = "x86_64")]

use uncharted_runtime::display::ffi::runtime_display_patch;

#[test]
fn the_c_symbol_reads_the_environment() {
    // mov dword [rax+8],1920 ; mov dword [rax+0xc],1080
    let mut image = vec![0u8; 0x100];
    image[0x10..0x13].copy_from_slice(&[0xc7, 0x40, 0x08]);
    image[0x13..0x17].copy_from_slice(&1920u32.to_le_bytes());
    image[0x17..0x1a].copy_from_slice(&[0xc7, 0x40, 0x0c]);
    image[0x1a..0x1e].copy_from_slice(&1080u32.to_le_bytes());
    let original = image.clone();

    // Unset: nothing changes (and a null image is ignored).
    std::env::remove_var("BB_GAME_RES");
    // SAFETY: `image` is `image.len()` writable bytes; null is ignored.
    unsafe {
        runtime_display_patch(image.as_mut_ptr(), image.len() as u64);
        runtime_display_patch(std::ptr::null_mut(), 16);
    }
    assert_eq!(image, original);

    std::env::set_var("BB_GAME_RES", "3840x2160");
    // SAFETY: as above.
    unsafe { runtime_display_patch(image.as_mut_ptr(), image.len() as u64) };
    assert_eq!(image[0x13..0x17], 3840u32.to_le_bytes());
    assert_eq!(image[0x1a..0x1e], 2160u32.to_le_bytes());
}
