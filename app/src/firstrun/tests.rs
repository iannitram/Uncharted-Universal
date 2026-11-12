use super::*;
use crate::focus::Nav;
use std::net::SocketAddr;
use uncharted_core::dump::FoundGame;
use uncharted_core::prepare::InstallManifest;

const A: Key = Key::Input(Input::Activate);
const B: Key = Key::Input(Input::Back);
const DOWN: Key = Key::Input(Input::Move(Nav::Down));
const UP: Key = Key::Input(Input::Move(Nav::Up));
const RIGHT: Key = Key::Input(Input::Move(Nav::Right));

fn report(games: &[Game], decrypted: bool, problems: &[&str]) -> DumpReport {
    DumpReport {
        title_id: Some("CUSA02320".into()),
        games: games
            .iter()
            .map(|&game| FoundGame {
                game,
                folder: PathBuf::from("/dump"),
                app_ver: Some("01.02".into()),
            })
            .collect(),
        decrypted,
        problems: problems.iter().map(|p| (*p).to_owned()).collect(),
    }
}

fn ok(game: Game) -> Prepared {
    Prepared {
        game,
        result: Ok(InstallManifest {
            game,
            title_id: "CUSA02320".into(),
            app_ver: "01.02".into(),
            files: Vec::new(),
        }),
    }
}

fn failed(game: Game, why: &str) -> Prepared {
    Prepared {
        game,
        result: Err(why.into()),
    }
}

fn hub() -> HubChoice {
    let addr: SocketAddr = "192.168.1.20:47120".parse().unwrap();
    HubChoice {
        label: format!("desktop at {addr}"),
        name: "desktop".into(),
        addr,
    }
}

/// Activate the button labelled `label`, with the D-pad, as the controller would.
fn press(page: &mut FirstRun, label: &str) -> Job {
    let view = page.view();
    let at = view
        .options
        .iter()
        .position(|o| o == label)
        .unwrap_or_else(|| panic!("no {label:?} in {:?}", view.options));
    for _ in 0..view.options.len() {
        page.handle(UP);
    }
    for _ in 0..at {
        page.handle(DOWN);
    }
    assert_eq!(page.view().focus, at);
    page.handle(A)
}

#[test]
fn keys_from_the_window() {
    assert_eq!(Key::from_code(6), Some(A));
    assert_eq!(Key::from_code(20), Some(Key::Digit(0)));
    assert_eq!(Key::from_code(29), Some(Key::Digit(9)));
    assert_eq!(Key::from_code(30), Some(Key::Backspace));
    assert_eq!(Key::from_code(11), None);
    assert_eq!(Key::from_code(31), None);
}

#[test]
fn the_welcome_step_offers_both_paths_and_leaves() {
    let mut page = FirstRun::new(None, true);
    let view = page.view();
    assert_eq!(view.step, "Step 1 of 4");
    assert_eq!(view.options, [HUB, CLIENT, NOT_NOW]);
    assert!(!view.busy);
    assert_eq!(page.handle(B), Job::Finish { start_steam: false });
    assert_eq!(
        press(&mut page, NOT_NOW),
        Job::Finish { start_steam: false }
    );
    // Down stops at the last button; Tab wraps to the first.
    for _ in 0..5 {
        page.handle(DOWN);
    }
    assert_eq!(page.view().focus, 2);
    page.handle(Key::Input(Input::Tab { back: false }));
    assert_eq!(page.view().focus, 0);
}

#[test]
fn hub_path_from_dump_to_steam() {
    let mut page = FirstRun::new(None, true);
    assert_eq!(press(&mut page, HUB), Job::Scan);
    let view = page.view();
    assert!(view.busy);
    assert_eq!(view.step, "Step 2 of 4");
    assert!(!view.field.is_empty());

    page.event(Event::Candidates(vec![PathBuf::from(
        "/home/deck/Games/CUSA02320",
    )]));
    let view = page.view();
    assert_eq!(view.options, ["/home/deck/Games/CUSA02320", TYPE_FOLDER]);
    assert_eq!(press(&mut page, TYPE_FOLDER), Job::EditField);
    page.handle(UP);
    assert_eq!(
        page.handle(A),
        Job::Check(PathBuf::from("/home/deck/Games/CUSA02320"))
    );
    assert!(page.view().busy);

    // A late answer for another folder is dropped.
    page.event(Event::Checked(
        PathBuf::from("/other"),
        Ok(report(&[], true, &[])),
    ));
    assert!(page.view().busy);
    page.event(Event::Checked(
        PathBuf::from("/home/deck/Games/CUSA02320"),
        Ok(report(&Game::ALL, true, &[])),
    ));
    let view = page.view();
    assert_eq!(view.options, ["Prepare the 3 games", CHOOSE_ANOTHER]);
    assert!(view
        .lines
        .contains(&"Uncharted 2: found, version 01.02".to_owned()));

    let Job::Prepare(plan) = page.handle(A) else {
        panic!("no prepare");
    };
    assert_eq!(plan.len(), 3);
    assert_eq!(page.view().progress, Some(0.0));
    page.event(Event::Preparing {
        index: 1,
        game: Game::U2,
    });
    let view = page.view();
    assert_eq!(view.step, "Step 3 of 4");
    assert_eq!(view.lines[0], "Preparing Uncharted 2 (2 of 3)...");
    assert!((view.progress.unwrap() - 1.0 / 3.0).abs() < 1e-6);
    assert!(view.options.is_empty());
    // B does not leave while a game is being prepared.
    assert_eq!(page.handle(B), Job::None);

    page.event(Event::Prepared(vec![
        ok(Game::U1),
        failed(Game::U2, "the dump is missing eboot.bin."),
        ok(Game::U3),
    ]));
    let view = page.view();
    assert_eq!(view.options, [NEXT, TRY_AGAIN, CHOOSE_ANOTHER]);
    assert_eq!(
        view.lines,
        [
            "Uncharted 1: ready.",
            "Uncharted 2: The dump is missing eboot.bin.",
            "Uncharted 3: ready."
        ]
    );
    // Try again runs only the game that failed.
    assert_eq!(
        press(&mut page, TRY_AGAIN),
        Job::Prepare(vec![(Game::U2, PathBuf::from("/dump"))])
    );
    page.event(Event::Prepared(vec![ok(Game::U2)]));
    assert_eq!(page.view().options, [NEXT, CHOOSE_ANOTHER]);
    assert_eq!(press(&mut page, NEXT), Job::None);
    assert_eq!(page.view().title, "Add to Steam");
    assert_eq!(page.view().step, "Step 4 of 4");
}

#[test]
fn a_set_up_button_prepares_only_its_game() {
    let mut page = FirstRun::new(Some(Game::U3), true);
    press(&mut page, HUB);
    page.event(Event::Candidates(Vec::new()));
    let view = page.view();
    assert_eq!(view.options, [TYPE_FOLDER]);
    assert!(view.lines[0].starts_with("No dump found"));
    assert_eq!(page.field("  "), Job::None);
    assert!(!page.view().error.is_empty());
    assert_eq!(page.field(" /dump "), Job::Check(PathBuf::from("/dump")));
    page.event(Event::Checked(
        PathBuf::from("/dump"),
        Ok(report(&Game::ALL, true, &[])),
    ));
    assert_eq!(page.view().options[0], "Prepare Uncharted 3");
    assert_eq!(
        page.handle(A),
        Job::Prepare(vec![(Game::U3, PathBuf::from("/dump"))])
    );
}

#[test]
fn a_dump_that_cannot_be_prepared_says_why() {
    let mut page = FirstRun::new(None, true);
    press(&mut page, HUB);
    page.event(Event::Candidates(vec![PathBuf::from("/pkg")]));
    page.handle(A);
    page.event(Event::Checked(
        PathBuf::from("/pkg"),
        Ok(report(
            &[Game::U1],
            false,
            &["The executable is still encrypted."],
        )),
    ));
    let view = page.view();
    assert_eq!(view.options, [CHOOSE_ANOTHER]);
    assert!(view
        .lines
        .contains(&"The executable is still encrypted.".to_owned()));
    assert_eq!(
        view.lines.last().unwrap(),
        "Nothing here can be prepared; choose another folder."
    );
    assert_eq!(page.handle(A), Job::Scan);

    // A check that fails shows its error.
    page.event(Event::Candidates(vec![PathBuf::from("/gone")]));
    page.handle(A);
    page.event(Event::Checked(
        PathBuf::from("/gone"),
        Err("no param.sfo in /gone".into()),
    ));
    assert_eq!(page.view().error, "No param.sfo in /gone.");
    // B goes back to the search.
    assert_eq!(page.handle(B), Job::Scan);
}

#[test]
fn every_game_failing_offers_no_next() {
    let mut page = FirstRun::new(None, true);
    press(&mut page, HUB);
    page.event(Event::Candidates(vec![PathBuf::from("/dump")]));
    page.handle(A);
    page.event(Event::Checked(
        PathBuf::from("/dump"),
        Ok(report(&[Game::U1], true, &[])),
    ));
    page.handle(A);
    page.event(Event::Prepared(vec![failed(Game::U1, "no space left")]));
    assert_eq!(page.view().options, [TRY_AGAIN, CHOOSE_ANOTHER]);
}

#[test]
fn client_path_pairs_pulls_and_goes_to_steam() {
    let mut page = FirstRun::new(None, true);
    assert_eq!(press(&mut page, CLIENT), Job::Discover);
    assert!(page.view().busy);
    page.event(Event::Hubs(Err(
        "hub::discover is not built yet (roadmap)".into()
    )));
    let view = page.view();
    assert_eq!(view.options, [TYPE_HUB]);
    assert!(view.lines[0].starts_with("Could not look for hubs: Hub::discover"));
    assert!(view.field.contains("192.168.1.20:47120"));

    // A typed address without a port, then one outside the home network, then a good one.
    assert_eq!(page.field("192.168.1.20"), Job::None);
    assert_eq!(
        page.view().error,
        "Add the hub's port after its address, as 192.168.1.20:47120."
    );
    page.field("8.8.8.8:47120");
    assert!(page.view().error.contains("not on the home network"));
    page.field("192.168.1.20:47120");
    let view = page.view();
    assert_eq!(view.title, "Pair with 192.168.1.20:47120");
    assert_eq!(view.code, ["", "", "", "", "", ""]);
    assert!(view.error.is_empty());

    // Pairing needs all six digits.
    assert_eq!(page.handle(A), Job::None);
    assert!(page.view().error.starts_with("Type all six digits"));
    // The D-pad: Up turns the first box to 0, Right moves on; then the keyboard types.
    page.handle(UP);
    page.handle(RIGHT);
    for d in [4, 8, 1, 5, 2] {
        page.handle(Key::Digit(d));
    }
    assert_eq!(page.view().code, ["0", "4", "8", "1", "5", "2"]);
    page.handle(Key::Backspace);
    page.handle(Key::Digit(7));
    let mut typed = hub_from_text("192.168.1.20:47120");
    typed.label = "192.168.1.20:47120".into();
    assert_eq!(
        page.handle(A),
        Job::PairPull {
            hub: typed.clone(),
            code: "048157".into()
        }
    );
    page.event(Event::PullBytes(3_000_000));
    let view = page.view();
    assert_eq!(view.step, "Step 3 of 4");
    assert!(view.lines[0].contains("3 MB so far"), "{:?}", view.lines);
    assert!(view.busy);

    // A wrong code comes back to the boxes, emptied, with the hub's message.
    page.event(Event::Pulled(Err("the pairing code did not match".into())));
    let view = page.view();
    assert_eq!(view.error, "The pairing code did not match.");
    assert_eq!(view.code, ["", "", "", "", "", ""]);

    for d in [1, 2, 3, 4, 5, 6] {
        page.handle(Key::Digit(d));
    }
    page.handle(A);
    page.event(Event::Pulled(Ok(Pulled {
        games: vec![Game::U1, Game::U2],
        bytes: 5_000_000_000,
        peer_note: Some("protected paired-device storage is not implemented on Windows".into()),
    })));
    let view = page.view();
    assert_eq!(
        view.lines[0],
        "Installed Uncharted 1, Uncharted 2 (5.0 GB)."
    );
    assert!(view.lines[1].starts_with("The hub was not remembered"));
    press(&mut page, NEXT);
    assert_eq!(page.view().title, "Add to Steam");
}

fn hub_from_text(text: &str) -> HubChoice {
    client::hub_address(text).unwrap()
}

#[test]
fn a_found_hub_goes_straight_to_the_code() {
    let mut page = FirstRun::new(None, true);
    press(&mut page, CLIENT);
    page.event(Event::Hubs(Ok(vec![hub()])));
    assert_eq!(
        page.view().options,
        ["desktop at 192.168.1.20:47120", TYPE_HUB]
    );
    page.handle(A);
    assert_eq!(page.view().title, "Pair with desktop at 192.168.1.20:47120");
    // B from the code looks for hubs again.
    assert_eq!(page.handle(B), Job::Discover);
    page.event(Event::Hubs(Ok(Vec::new())));
    assert!(page.view().lines[0].starts_with("No hub answered"));
}

fn at_steam() -> FirstRun {
    let mut page = FirstRun::new(None, true);
    press(&mut page, CLIENT);
    page.event(Event::Hubs(Ok(vec![hub()])));
    page.handle(A);
    for d in [1, 2, 3, 4, 5, 6] {
        page.handle(Key::Digit(d));
    }
    page.handle(A);
    page.event(Event::Pulled(Ok(Pulled {
        games: vec![Game::U2],
        bytes: 1,
        peer_note: None,
    })));
    press(&mut page, NEXT);
    page
}

#[test]
fn add_to_steam_when_steam_is_closed() {
    let mut page = at_steam();
    assert_eq!(page.view().options, [ADD, NOT_NOW]);
    assert_eq!(page.handle(A), Job::AddToSteam);
    assert!(page.view().busy);
    page.event(Event::Added(Ok("Added to Steam for 1 user.".into())));
    let view = page.view();
    assert_eq!(view.title, "All set");
    assert_eq!(view.lines, ["Added to Steam for 1 user."]);
    assert_eq!(view.options, [OPEN]);
    assert_eq!(page.handle(A), Job::Finish { start_steam: false });
}

#[test]
fn steam_open_asks_to_close_it_and_start_it_again() {
    let mut page = at_steam();
    page.handle(A);
    page.event(Event::SteamOpen);
    let view = page.view();
    assert_eq!(view.title, "Steam is open");
    assert_eq!(view.options, [CLOSE_AND_ADD, BACK]);
    // Back returns to the Steam step; Add again finds it open again.
    assert_eq!(page.handle(B), Job::None);
    assert_eq!(page.view().title, "Add to Steam");
    page.handle(A);
    page.event(Event::SteamOpen);
    assert_eq!(page.handle(A), Job::CloseSteamAndAdd);
    assert_eq!(
        page.view().lines[0],
        "Closing Steam, then adding the launcher..."
    );
    page.event(Event::Added(Ok("Added to Steam for 2 users.".into())));
    let view = page.view();
    assert_eq!(view.options, [START_STEAM, OPEN]);
    assert_eq!(view.lines.len(), 2);
    assert_eq!(page.handle(A), Job::Finish { start_steam: true });
    // B from the last step starts Steam too: it was closed for the shortcut.
    assert_eq!(page.handle(B), Job::Finish { start_steam: true });
}

#[test]
fn a_failed_add_shows_why_and_not_now_finishes() {
    let mut page = at_steam();
    page.handle(A);
    page.event(Event::Added(Err("steam: no userdata folder".into())));
    let view = page.view();
    assert_eq!(view.title, "Add to Steam");
    assert_eq!(view.error, "Steam: no userdata folder.");
    assert_eq!(press(&mut page, NOT_NOW), Job::None);
    let view = page.view();
    assert_eq!(view.title, "All set");
    assert!(view.lines[0].contains("uncharted-universal setup steam"));
    assert_eq!(page.handle(A), Job::Finish { start_steam: false });
}

#[test]
fn without_steam_the_last_step_is_done() {
    let mut page = FirstRun::new(None, false);
    press(&mut page, HUB);
    page.event(Event::Candidates(vec![PathBuf::from("/dump")]));
    page.handle(A);
    page.event(Event::Checked(
        PathBuf::from("/dump"),
        Ok(report(&[Game::U1], true, &[])),
    ));
    page.handle(A);
    page.event(Event::Prepared(vec![ok(Game::U1)]));
    press(&mut page, NEXT);
    let view = page.view();
    assert_eq!(view.title, "All set");
    assert_eq!(view.lines, ["Uncharted 1: ready."]);
}

#[test]
fn clicks_choose_like_the_controller() {
    let mut page = FirstRun::new(None, true);
    assert_eq!(page.click(9), Job::None);
    assert_eq!(page.click(1), Job::Discover);
    assert_eq!(page.view().focus, 0);
}

#[test]
fn sentences() {
    assert_eq!(sentence("no space left"), "No space left.");
    assert_eq!(sentence("Done."), "Done.");
    assert_eq!(sentence(" "), "");
}
