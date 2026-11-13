//! `host_env::clean_process` on this test's own process. One test in its own binary: it
//! changes the process environment, which no other test may read at the same time.

use uncharted_core::host_env::{self, Change};

#[test]
fn clean_process_takes_steam_preload_out_of_this_process() {
    std::env::set_var("LD_PRELOAD", "/steam/ubuntu12_64/gameoverlayrenderer.so");
    std::env::set_var("LD_LIBRARY_PATH", "");
    std::env::set_var("UU_HOST_ENV_UNTOUCHED", "kept");
    let made = host_env::clean_process();
    assert_eq!(
        std::env::var("UU_HOST_ENV_UNTOUCHED").as_deref(),
        Ok("kept")
    );
    if cfg!(target_os = "linux") {
        assert!(std::env::var_os("LD_PRELOAD").is_none());
        assert!(std::env::var_os("LD_LIBRARY_PATH").is_none());
        assert_eq!(
            std::env::var("BB_HOST_LD_PRELOAD").as_deref(),
            Ok("/steam/ubuntu12_64/gameoverlayrenderer.so")
        );
        assert!(std::env::var_os("BB_HOST_LD_LIBRARY_PATH").is_none());
        assert_eq!(
            std::env::var("DISABLE_VK_LAYER_VALVE_steam_overlay_1").as_deref(),
            Ok("1")
        );
        assert!(made.contains(&Change::Remove("LD_PRELOAD".into())));
        // A second run finds nothing new to copy: the saved value stays the first one.
        host_env::clean_process();
        assert_eq!(
            std::env::var("BB_HOST_LD_PRELOAD").as_deref(),
            Ok("/steam/ubuntu12_64/gameoverlayrenderer.so")
        );
    } else {
        // Not Linux: nothing changes.
        assert!(made.is_empty());
        assert_eq!(
            std::env::var("LD_PRELOAD").as_deref(),
            Ok("/steam/ubuntu12_64/gameoverlayrenderer.so")
        );
    }
}
