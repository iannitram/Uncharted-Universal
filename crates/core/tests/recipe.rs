//! `recipe`: hubs, the targets they serve, and the prepare steps per target (ADR 0004-0006).
//! New module, no Python counterpart; the expected step order is the preparation in `run.py`.

use uncharted_core::error::Error;
use uncharted_core::platform::Os;
use uncharted_core::recipe::{hub_for, recipe_for, serves, step, Hub};

#[test]
fn every_target_has_a_hub_that_serves_it() {
    for os in Os::ALL {
        assert!(serves(hub_for(os)).contains(&os), "{os:?}");
    }
}

#[test]
fn every_target_a_hub_serves_maps_back_to_that_hub() {
    for hub in Hub::ALL {
        for &os in serves(hub) {
            assert_eq!(hub_for(os), hub, "{os:?}");
        }
    }
    let served: usize = Hub::ALL.into_iter().map(|h| serves(h).len()).sum();
    assert_eq!(served, Os::ALL.len());
}

#[test]
fn the_hub_table_is_adr_0004() {
    assert_eq!(hub_for(Os::Linux), Hub::StrixHalo);
    assert_eq!(hub_for(Os::Android), Hub::StrixHalo);
    assert_eq!(hub_for(Os::Windows), Hub::Desktop);
    assert_eq!(hub_for(Os::Mac), Hub::Mac);
    assert_eq!(hub_for(Os::Ios), Hub::Mac);
    assert_eq!(serves(Hub::StrixHalo), &[Os::Linux, Os::Android]);
    assert_eq!(serves(Hub::Desktop), &[Os::Windows]);
    assert_eq!(serves(Hub::Mac), &[Os::Mac, Os::Ios]);
}

#[test]
fn hub_keys_round_trip() {
    let keys: Vec<_> = Hub::ALL.into_iter().map(Hub::key).collect();
    assert_eq!(keys, ["strix-halo", "desktop", "mac"]);
    for hub in Hub::ALL {
        assert_eq!(Hub::from_key(hub.key()), Ok(hub));
    }
}

#[test]
fn an_unknown_hub_key_is_a_hub_error() {
    for key in ["", "laptop", "Mac", "strix_halo", " mac"] {
        let err = Hub::from_key(key).unwrap_err();
        assert_eq!(err, Error::Hub(format!("no hub called {key}")));
        assert_eq!(err.code(), "hub");
    }
}

const EXECUTABLE: [&str; 10] = [
    "check dump",
    "check revision",
    "boot image",
    "link modules",
    "link libc",
    "render size",
    "content profile",
    "trophies",
    "patches",
    "bundle",
];

#[test]
fn linux_windows_and_android_prepare_the_executable() {
    for os in [Os::Linux, Os::Windows, Os::Android] {
        let r = recipe_for(os);
        assert_eq!(r.target, os);
        assert_eq!(r.hub, hub_for(os));
        assert!(!r.recompiled_library, "{os:?}");
        assert_eq!(r.steps, EXECUTABLE, "{os:?}");
    }
}

#[test]
fn the_mac_recompiles_and_bundles() {
    let r = recipe_for(Os::Mac);
    assert_eq!(r.target, Os::Mac);
    assert_eq!(r.hub, Hub::Mac);
    assert!(r.recompiled_library);
    assert_eq!(
        r.steps,
        [
            "check dump",
            "check revision",
            "boot image",
            "link modules",
            "link libc",
            "render size",
            "patches",
            "recompile",
            "content profile",
            "trophies",
            "bundle",
        ]
    );
}

#[test]
fn ios_recompiles_then_builds_the_app_and_installs_by_cable() {
    let r = recipe_for(Os::Ios);
    assert_eq!(r.target, Os::Ios);
    assert_eq!(r.hub, Hub::Mac);
    assert!(r.recompiled_library);
    let mac = recipe_for(Os::Mac).steps;
    let shared = mac.len() - 1;
    assert_eq!(r.steps[..shared], mac[..shared]);
    assert_eq!(r.steps[shared..], ["build ios app", "install by cable"]);
    assert!(!r.steps.contains(&step::BUNDLE));
}

#[test]
fn every_recipe_starts_with_the_dump_and_revision_checks() {
    for os in Os::ALL {
        let r = recipe_for(os);
        assert_eq!(r.steps[..2], [step::CHECK_DUMP, step::CHECK_REVISION]);
        assert_eq!(r.recompiled_library, r.steps.contains(&step::RECOMPILE));
        let mut seen = r.steps.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), r.steps.len(), "{os:?} repeats a step");
    }
}

#[test]
fn patches_and_render_size_come_before_the_recompile() {
    for os in [Os::Mac, Os::Ios] {
        let steps = recipe_for(os).steps;
        let at = |s: &str| steps.iter().position(|x| *x == s).unwrap();
        assert!(at(step::PATCHES) < at(step::RECOMPILE));
        assert!(at(step::RENDER_SIZE) < at(step::RECOMPILE));
    }
}
