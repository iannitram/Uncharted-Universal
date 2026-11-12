//! The skeleton links, its stubs say "not built yet" with a stable code, and the filled-in
//! constant data reads back. Delete a case here when its module is built (its own tests
//! replace it).

use uncharted_core::error::Error;
use uncharted_core::games::{self, Game};
use uncharted_core::platform::AppDirs;
use uncharted_core::{artwork, cheats};

#[test]
fn a_stub_returns_not_built_with_a_stable_code() {
    let err = cheats::parse("").unwrap_err();
    assert_eq!(err, Error::NotBuilt("cheats::parse"));
    assert_eq!(err.code(), "not_built");
    assert_eq!(err.to_string(), "not built yet: cheats::parse");
}

#[test]
fn error_codes_are_stable() {
    assert_eq!(Error::Io(String::new()).code(), "io");
    assert_eq!(
        Error::WrongRevision {
            game: "u1".into(),
            found: "01.99".into()
        }
        .code(),
        "wrong_revision"
    );
}

#[test]
fn app_dirs_sit_under_uncharted_universal() {
    let dirs = AppDirs::new(&uncharted_core::platform::Vars {
        home: "/h".into(),
        data: "/h/data".into(),
        config: "/h/config".into(),
    });
    assert!(dirs.config.ends_with("uncharted-universal"));
    assert!(dirs.install("u2").ends_with("installs/u2"));
}

#[test]
fn games_and_their_executables() {
    assert_eq!(games::TITLE_IDS.len(), 4);
    assert!(games::is_collection_title_id("CUSA02320"));
    assert_eq!(Game::from_key("U3"), Some(Game::U3));
    assert_eq!(Game::U2.executable(), "big2-ps4_Shipping.elf");
    assert_eq!(Game::from_executable("eboot.bin"), Some(Game::U1));
    assert!(games::supported(Game::U1).is_empty());
    assert_eq!(games::SUPPORTED.len(), 3);
    assert_eq!(
        games::title_region("CUSA02320").unwrap_err().code(),
        "not_built"
    );
    assert_eq!(
        games::check_revision(Game::U1, "01.00").unwrap_err().code(),
        "wrong_revision"
    );
}

#[test]
fn artwork_file_names() {
    assert_eq!(artwork::file_name(artwork::Art::Hero, 7), "7_hero.png");
    assert_eq!(artwork::size(artwork::Art::Grid), (600, 900));
}

#[cfg(feature = "hub")]
#[test]
fn the_hub_module_exists_with_the_feature() {
    assert_eq!(
        uncharted_core::hub::SERVICE,
        "_uncharted-universal._tcp.local."
    );
}
