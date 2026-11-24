//! `runtime_display_patch` with the process environment, as the loader calls it (one test:
//! it sets the variables of this process).
#![cfg(target_arch = "x86_64")]

use uncharted_runtime::display::{
    ffi::runtime_display_patch, uncharted_runtime_display_fov, uncharted_runtime_movie_fit_rect,
    uncharted_runtime_safe_area,
};

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

    // End-to-end check across 6:5, 3:2, 16:9, 16:10, 21:9/5K2K, and cover screen.
    for (w, h, expected_mode) in [
        (2208u32, 1840u32, 2u32), // 6:5 (Pixel Fold unfolded)
        (2266, 1488, 2),          // ~3:2 (iPad mini)
        (1920, 1080, 0),          // 16:9
        (2560, 1600, 2),          // 16:10 (Z13)
        (3440, 1440, 1),          // 21:9 ultrawide
        (5120, 2160, 1),          // 21:9 5K2K
        (2092, 1080, 1),          // 17.4:9 (Pixel Fold cover)
    ] {
        let mut target = original.clone();
        std::env::set_var("BB_GAME_RES", format!("{w}x{h}"));
        // SAFETY: `target` is `target.len()` writable bytes.
        unsafe { runtime_display_patch(target.as_mut_ptr(), target.len() as u64) };
        assert_eq!(target[0x13..0x17], w.to_le_bytes());
        assert_eq!(target[0x1a..0x1e], h.to_le_bytes());

        let fov = uncharted_runtime_display_fov(w, h);
        assert_eq!(fov.mode, expected_mode, "{w}x{h}");
        let safe = uncharted_runtime_safe_area(w, h);
        let movie = uncharted_runtime_movie_fit_rect(w, h, 1920, 1080);
        assert_eq!(safe, movie);
        assert!(safe.x + safe.width <= w && safe.y + safe.height <= h);
    }
    std::env::remove_var("BB_GAME_RES");
}
