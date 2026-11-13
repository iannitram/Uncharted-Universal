//! View-model tests with fake `SaveCard`s (docs/continue-screen.md, "How 2.04 tests it"):
//! the four states for each game, each field `None` in turn, the default focus, open then A,
//! LB/RB, and a forced conflict. Synthetic data only: no save, picture or game file.

use super::*;

/// 2026-11-12 21:30:00 UTC.
const NOW: u64 = 1_794_519_000;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nsynthetic save picture";
const ICON: &[u8] = b"\x89PNG\r\n\x1a\nsynthetic collection icon";

fn clock() -> Clock<'static> {
    Clock {
        now: NOW,
        offset: &|_| 0,
    }
}

fn full_card(game: Game, saved_unix: u64) -> SaveCard {
    SaveCard {
        game,
        slot: format!("{}-A", uncharted_core::saves::save_prefix(game)),
        picture: Some(PNG.to_vec()),
        title: Some(format!("{} save", game.name())),
        detail: Some("Chapter 4".into()),
        saved_unix,
        play_time_secs: Some(6 * 3_600 + 12 * 60),
        device: Some("Strix Halo tablet".into()),
        build_id: Some("a1b2c3".into()),
    }
}

fn entry(slot: u8, card: SaveCard) -> SaveEntry {
    SaveEntry { slot, card }
}

/// A game with `n` saves, newest first, one minute apart, ending 16 minutes before `NOW`.
fn game_with(game: Game, n: usize) -> GameData {
    let saves = (0..n)
        .map(|i| entry(i as u8 + 1, full_card(game, NOW - 16 * 60 - 60 * i as u64)))
        .collect();
    GameData {
        saves,
        fallback_picture: Some(ICON.to_vec()),
        ..GameData::empty(game)
    }
}

fn three(n: [usize; 3]) -> Vec<GameData> {
    Game::ALL
        .into_iter()
        .zip(n)
        .map(|(game, n)| game_with(game, n))
        .collect()
}

fn screen_on(game: Game, games: Vec<GameData>) -> Screen {
    let mut screen = Screen::new(games, None, "");
    while screen.current_game() != game {
        screen.handle(Input::NextGame);
    }
    screen
}

fn pair(game: Game, server_newer: bool) -> SideBySide {
    let (server, local) = if server_newer {
        (NOW - 600, NOW - 3_600)
    } else {
        (NOW - 3_600, NOW - 600)
    };
    uncharted_core::savecard::side_by_side(full_card(game, server), full_card(game, local))
}

#[test]
fn input_codes_match_the_slint_file() {
    assert_eq!(Input::from_code(0), Some(Input::Move(Nav::Up)));
    assert_eq!(Input::from_code(3), Some(Input::Move(Nav::Right)));
    assert_eq!(Input::from_code(5), Some(Input::Tab { back: true }));
    assert_eq!(Input::from_code(6), Some(Input::Activate));
    assert_eq!(Input::from_code(7), Some(Input::Back));
    assert_eq!(Input::from_code(9), Some(Input::NextGame));
    assert_eq!(Input::from_code(10), Some(Input::OtherSaves));
    assert_eq!(Input::from_code(11), None);
    assert_eq!(Input::from_code(-1), None);
}

#[test]
fn state_1_no_save_for_each_game() {
    for game in Game::ALL {
        let screen = screen_on(game, three([0, 0, 0]));
        let view = screen.view(&clock());
        assert_eq!(view.games, ["Uncharted 1", "Uncharted 2", "Uncharted 3"]);
        assert_eq!(view.games[view.current], game.name());
        assert_eq!(view.actions, ["Play"]);
        assert_eq!(view.action_hints, ["A"]);
        assert_eq!(view.focus, Focus::actions(0));
        assert_eq!(
            view.card,
            CardView {
                title: game.name().into(),
                lines: vec!["No save yet".into()],
                picture: Some(ICON.to_vec()),
                picture_label: game.name().into(),
            }
        );
        assert_eq!(view.pages, ["Saves", "Settings", "Fleet", "About"]);
        assert_eq!(view.overlay, OverlayView::None);
    }
}

#[test]
fn state_1_without_a_readable_icon_is_a_plain_panel_with_the_name() {
    let mut games = three([0, 0, 0]);
    games[1].fallback_picture = None;
    let view = screen_on(Game::U2, games).view(&clock());
    assert_eq!(view.card.picture, None);
    assert_eq!(view.card.picture_label, "Uncharted 2");
}

#[test]
fn state_2_one_save_for_each_game() {
    for game in Game::ALL {
        let screen = screen_on(game, three([1, 1, 1]));
        let view = screen.view(&clock());
        assert_eq!(view.actions, ["Continue", "Play"]);
        assert_eq!(view.action_hints, ["A", ""]);
        assert_eq!(view.focus, Focus::actions(0));
        assert_eq!(
            view.card,
            CardView {
                title: format!("{} save", game.name()),
                lines: vec![
                    "Chapter 4".into(),
                    "Saved today 21:14".into(),
                    "on the Strix Halo tablet".into(),
                    "Play time 6 h 12 min".into(),
                ],
                picture: Some(PNG.to_vec()),
                picture_label: String::new(),
            }
        );
    }
}

#[test]
fn state_3_several_saves_for_each_game() {
    for game in Game::ALL {
        let screen = screen_on(game, three([3, 3, 3]));
        let view = screen.view(&clock());
        assert_eq!(view.actions, ["Continue", "Play", "Other saves (3)"]);
        assert_eq!(view.action_hints, ["A", "", "Y"]);
        assert_eq!(view.card.lines[1], "Saved today 21:14");
    }
}

#[test]
fn each_field_none_in_turn_closes_the_card_up() {
    let base = || {
        let mut games = three([0, 1, 0]);
        games[1].saves[0].card.saved_unix = NOW - 16 * 60;
        games
    };
    let card_with = |change: &dyn Fn(&mut SaveCard)| {
        let mut games = base();
        change(&mut games[1].saves[0].card);
        screen_on(Game::U2, games).view(&clock()).card
    };

    let card = card_with(&|c| c.picture = None);
    assert_eq!(card.picture, Some(ICON.to_vec()));
    assert_eq!(card.picture_label, "Uncharted 2");

    let card = card_with(&|c| c.title = None);
    assert_eq!(card.title, "Uncharted 2");

    let card = card_with(&|c| c.detail = None);
    assert_eq!(
        card.lines,
        [
            "Saved today 21:14",
            "on the Strix Halo tablet",
            "Play time 6 h 12 min"
        ]
    );

    let card = card_with(&|c| c.saved_unix = 0);
    assert_eq!(card.lines[1], "Save time not known");

    let card = card_with(&|c| c.device = None);
    assert_eq!(
        card.lines,
        ["Chapter 4", "Saved today 21:14", "Play time 6 h 12 min"]
    );

    let card = card_with(&|c| c.play_time_secs = None);
    assert_eq!(
        card.lines,
        ["Chapter 4", "Saved today 21:14", "on the Strix Halo tablet"]
    );

    // The build is not on the card at all.
    let card = card_with(&|c| c.build_id = None);
    assert_eq!(card, screen_on(Game::U2, base()).view(&clock()).card);

    // Everything optional missing: the name and the time only, on the fallback picture.
    let card = card_with(&|c| {
        c.picture = None;
        c.title = None;
        c.detail = None;
        c.device = None;
        c.play_time_secs = None;
        c.build_id = None;
    });
    assert_eq!(card.title, "Uncharted 2");
    assert_eq!(card.lines, ["Saved today 21:14"]);
}

#[test]
fn opens_on_the_game_with_the_newest_save() {
    let mut games = three([1, 1, 1]);
    games[2].saves[0].card.saved_unix = NOW - 10;
    assert_eq!(Screen::new(games, None, "").current_game(), Game::U3);

    let mut games = three([0, 1, 1]);
    games[1].saves[0].card.saved_unix = NOW - 5;
    assert_eq!(
        Screen::new(games, Some(Game::U3), "").current_game(),
        Game::U2
    );

    // A tie goes to the earlier game.
    assert_eq!(
        Screen::new(three([0, 1, 1]), None, "").current_game(),
        Game::U2
    );
}

#[test]
fn with_no_save_opens_on_the_game_last_started_else_uncharted_1() {
    let games = three([0, 0, 0]);
    assert_eq!(
        Screen::new(games.clone(), Some(Game::U3), "").current_game(),
        Game::U3
    );
    assert_eq!(Screen::new(games, None, "").current_game(), Game::U1);
}

#[test]
fn open_then_a_starts_in_one_press() {
    // With a save: Continue, in the newest save's slot.
    let mut games = three([0, 2, 0]);
    games[1].saves[0].slot = 4;
    let mut screen = Screen::new(games, None, "");
    assert_eq!(
        screen.handle(Input::Activate),
        Command::Start(Start {
            game: Game::U2,
            slot: Some(4),
            keep: None,
        })
    );
    // With no save: Play, in the active slot.
    let mut screen = Screen::new(three([0, 0, 0]), Some(Game::U3), "");
    assert_eq!(
        screen.handle(Input::Activate),
        Command::Start(Start {
            game: Game::U3,
            slot: None,
            keep: None,
        })
    );
}

#[test]
fn play_starts_with_the_active_slot() {
    let mut screen = screen_on(Game::U1, three([1, 0, 0]));
    screen.handle(Input::Move(Nav::Right));
    assert_eq!(
        screen.handle(Input::Activate),
        Command::Start(Start {
            game: Game::U1,
            slot: None,
            keep: None,
        })
    );
}

#[test]
fn lb_and_rb_change_game_and_reset_focus() {
    let mut screen = Screen::new(three([1, 0, 3]), None, "");
    assert_eq!(screen.current_game(), Game::U1);
    screen.handle(Input::Move(Nav::Right));
    assert_eq!(screen.focus(), Focus::actions(1));
    screen.handle(Input::NextGame);
    assert_eq!(screen.current_game(), Game::U2);
    assert_eq!(screen.focus(), Focus::actions(0));
    assert_eq!(screen.view(&clock()).actions, ["Play"]);
    screen.handle(Input::Move(Nav::Down));
    screen.handle(Input::NextGame);
    assert_eq!(screen.current_game(), Game::U3);
    assert_eq!(screen.focus(), Focus::actions(0));
    screen.handle(Input::NextGame);
    assert_eq!(screen.current_game(), Game::U1);
    screen.handle(Input::PrevGame);
    assert_eq!(screen.current_game(), Game::U3);
}

#[test]
fn d_pad_reaches_the_pages_and_a_opens_one() {
    let mut screen = screen_on(Game::U1, three([1, 0, 0]));
    screen.handle(Input::Move(Nav::Down));
    assert_eq!(screen.focus(), Focus::pages(0));
    screen.handle(Input::Move(Nav::Right));
    screen.handle(Input::Move(Nav::Right));
    assert_eq!(screen.handle(Input::Activate), Command::Open(Page::Fleet));
    screen.handle(Input::Move(Nav::Up));
    assert_eq!(screen.focus(), Focus::actions(1));
    screen.handle(Input::Tab { back: false });
    assert_eq!(screen.focus(), Focus::pages(0));
}

#[test]
fn other_saves_list_continues_the_chosen_save() {
    let mut games = three([0, 0, 3]);
    games[2].saves[1].slot = 5;
    games[2].saves[2].slot = 5;
    let mut screen = screen_on(Game::U3, games);
    // Y opens the list.
    screen.handle(Input::OtherSaves);
    let OverlayView::OtherSaves { title, rows, index } = screen.view(&clock()).overlay else {
        panic!("the list is not open");
    };
    assert_eq!(title, "Other saves: Uncharted 3");
    assert_eq!(index, 0);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0],
        RowView {
            picture: Some(PNG.to_vec()),
            title: "Uncharted 3 save".into(),
            detail: "Chapter 4".into(),
            when: "today 21:14".into(),
            device: "Strix Halo tablet".into(),
            slot: "Slot 1".into(),
            note: String::new(),
        }
    );
    assert_eq!(rows[1].note, "In the game: Load");
    assert_eq!(rows[2].slot, "Slot 5");
    screen.handle(Input::Move(Nav::Down));
    screen.handle(Input::Move(Nav::Down));
    screen.handle(Input::Move(Nav::Down));
    assert!(matches!(
        screen.view(&clock()).overlay,
        OverlayView::OtherSaves { index: 2, .. }
    ));
    assert_eq!(
        screen.handle(Input::Activate),
        Command::Start(Start {
            game: Game::U3,
            slot: Some(5),
            keep: None,
        })
    );
    assert_eq!(screen.view(&clock()).overlay, OverlayView::None);
}

#[test]
fn other_saves_b_goes_back_to_its_button() {
    let mut screen = screen_on(Game::U2, three([0, 2, 0]));
    screen.handle(Input::Move(Nav::Right));
    screen.handle(Input::Move(Nav::Right));
    assert_eq!(screen.handle(Input::Activate), Command::None);
    assert_eq!(screen.view(&clock()).overlay.code(), 2);
    screen.handle(Input::Back);
    assert_eq!(screen.view(&clock()).overlay, OverlayView::None);
    assert_eq!(screen.focus(), Focus::actions(2));
}

#[test]
fn y_does_nothing_with_one_save() {
    let mut screen = screen_on(Game::U2, three([0, 1, 0]));
    screen.handle(Input::OtherSaves);
    assert_eq!(screen.view(&clock()).overlay, OverlayView::None);
}

#[test]
fn state_4_conflict_prompt_for_each_game_only_over_that_game() {
    for game in Game::ALL {
        let mut screen = screen_on(game, three([1, 1, 1]));
        screen.set_conflict(game, pair(game, true));
        let OverlayView::Conflict {
            title,
            sides,
            server,
        } = screen.view(&clock()).overlay
        else {
            panic!("no prompt");
        };
        assert_eq!(
            title,
            format!("Two saves of {}. Keep which one?", game.name())
        );
        assert!(server, "focus starts on the newer side");
        assert_eq!(sides[0].heading, "This device");
        assert!(!sides[0].newer);
        assert_eq!(
            sides[0].lines,
            [
                "Chapter 4",
                "Saved today 20:30, on the Strix Halo tablet",
                "Play time 6 h 12 min",
                "Slot 1, build a1b2c3",
            ]
        );
        assert_eq!(sides[1].heading, "Synced");
        assert!(sides[1].newer);
        assert_eq!(
            sides[1].lines[1],
            "Saved today 21:20, on the Strix Halo tablet"
        );

        // The other two games play as normal.
        screen.handle(Input::Back);
        let other = Game::ALL.into_iter().find(|g| *g != game).unwrap();
        while screen.current_game() != other {
            screen.handle(Input::NextGame);
        }
        assert_eq!(screen.view(&clock()).card.lines[0], "Chapter 4");
        assert!(matches!(screen.handle(Input::Activate), Command::Start(_)));
    }
}

#[test]
fn conflict_focus_starts_on_the_local_side_when_it_is_newer() {
    let mut screen = screen_on(Game::U1, three([1, 0, 0]));
    screen.set_conflict(Game::U1, pair(Game::U1, false));
    let OverlayView::Conflict { sides, server, .. } = screen.view(&clock()).overlay else {
        panic!("no prompt");
    };
    assert!(!server);
    assert!(sides[0].newer);
    assert!(!sides[1].newer);
}

#[test]
fn conflict_without_a_build_says_so() {
    let mut screen = screen_on(Game::U1, three([1, 0, 0]));
    let mut both = pair(Game::U1, true);
    both.local.build_id = None;
    screen.set_conflict(Game::U1, both);
    let OverlayView::Conflict { sides, .. } = screen.view(&clock()).overlay else {
        panic!("no prompt");
    };
    assert_eq!(sides[0].lines.last().unwrap(), "Slot 1, Build not recorded");
}

#[test]
fn a_forced_conflict_keeps_the_side_picked() {
    // Opened by the pull: A keeps the focused (newer) side and starts nothing.
    let mut screen = screen_on(Game::U2, three([0, 1, 0]));
    screen.set_conflict(Game::U2, pair(Game::U2, true));
    assert_eq!(
        screen.handle(Input::Activate),
        Command::Keep {
            game: Game::U2,
            keep: Keep::Server,
        }
    );
    assert_eq!(screen.view(&clock()).overlay, OverlayView::None);
    assert_eq!(
        screen.handle(Input::Activate),
        Command::Start(Start {
            game: Game::U2,
            slot: Some(1),
            keep: None,
        })
    );

    // Left picks this device's save.
    let mut screen = screen_on(Game::U2, three([0, 1, 0]));
    screen.set_conflict(Game::U2, pair(Game::U2, true));
    screen.handle(Input::Move(Nav::Left));
    assert_eq!(
        screen.handle(Input::Activate),
        Command::Keep {
            game: Game::U2,
            keep: Keep::Local,
        }
    );
}

#[test]
fn decide_later_then_continue_asks_again_and_starts_with_the_answer() {
    let mut screen = screen_on(Game::U3, three([0, 0, 1]));
    screen.set_conflict(Game::U3, pair(Game::U3, false));
    screen.handle(Input::Back);
    let view = screen.view(&clock());
    assert_eq!(view.overlay, OverlayView::None);
    assert_eq!(view.card.lines[0], "Two saves: pick one to play");
    // Continue opens the prompt instead of starting.
    assert_eq!(screen.handle(Input::Activate), Command::None);
    assert_eq!(screen.view(&clock()).overlay.code(), 3);
    screen.handle(Input::Move(Nav::Right));
    assert_eq!(
        screen.handle(Input::Activate),
        Command::Start(Start {
            game: Game::U3,
            slot: Some(1),
            keep: Some(Keep::Server),
        })
    );
    // Settled: the card is back to normal.
    assert_eq!(screen.view(&clock()).card.lines[0], "Chapter 4");

    // Play asks too.
    let mut screen = screen_on(Game::U3, three([0, 0, 1]));
    screen.set_conflict(Game::U3, pair(Game::U3, false));
    screen.handle(Input::Back);
    screen.handle(Input::Move(Nav::Right));
    assert_eq!(screen.handle(Input::Activate), Command::None);
    assert_eq!(
        screen.handle(Input::Activate),
        Command::Start(Start {
            game: Game::U3,
            slot: None,
            keep: Some(Keep::Local),
        })
    );
}

#[test]
fn a_conflict_for_another_game_waits_until_that_game_is_shown() {
    let mut screen = screen_on(Game::U1, three([1, 1, 0]));
    screen.set_conflict(Game::U2, pair(Game::U2, true));
    assert_eq!(screen.view(&clock()).overlay, OverlayView::None);
    screen.handle(Input::NextGame);
    assert_eq!(
        screen.view(&clock()).card.lines[0],
        "Two saves: pick one to play"
    );
    assert_eq!(screen.handle(Input::Activate), Command::None);
    assert_eq!(screen.view(&clock()).overlay.code(), 3);
}

#[test]
fn build_warning_before_continue() {
    let mut games = three([0, 1, 0]);
    games[1].warning = Some("AMD handheld made this save with 3 of our fixes; this device has 4. Update this device first, or play on and keep both saves.".into());
    let mut screen = screen_on(Game::U2, games);
    assert_eq!(screen.handle(Input::Activate), Command::None);
    let OverlayView::Warning(text) = screen.view(&clock()).overlay else {
        panic!("no warning");
    };
    assert!(text.starts_with("AMD handheld made this save"));
    // B goes back; A plays anyway.
    screen.handle(Input::Back);
    assert_eq!(screen.view(&clock()).overlay, OverlayView::None);
    screen.handle(Input::Activate);
    assert_eq!(
        screen.handle(Input::Activate),
        Command::Start(Start {
            game: Game::U2,
            slot: Some(1),
            keep: None,
        })
    );
    // Play does not warn.
    screen.handle(Input::Move(Nav::Right));
    assert!(matches!(screen.handle(Input::Activate), Command::Start(_)));
}

#[test]
fn not_set_up_shows_set_up_only() {
    let mut games = three([0, 2, 0]);
    games[1].set_up = false;
    let mut screen = screen_on(Game::U2, games);
    assert_eq!(screen.view(&clock()).actions, ["Set up"]);
    assert_eq!(screen.handle(Input::Activate), Command::SetUp(Game::U2));
}

#[test]
fn b_asks_to_quit() {
    let mut screen = Screen::new(three([0, 0, 0]), None, "");
    screen.handle(Input::Back);
    assert_eq!(screen.view(&clock()).overlay, OverlayView::Quit);
    // B stays.
    screen.handle(Input::Back);
    assert_eq!(screen.view(&clock()).overlay, OverlayView::None);
    screen.handle(Input::Back);
    assert_eq!(screen.handle(Input::NextGame), Command::None);
    assert_eq!(screen.handle(Input::Activate), Command::Quit);
}

#[test]
fn while_running_input_does_nothing() {
    let mut screen = Screen::new(three([1, 0, 0]), None, "");
    screen.running("Starting Uncharted 1...");
    assert_eq!(
        screen.view(&clock()).overlay,
        OverlayView::Running("Starting Uncharted 1...".into())
    );
    for input in [Input::Activate, Input::Back, Input::NextGame] {
        assert_eq!(screen.handle(input), Command::None);
    }
    assert_eq!(screen.current_game(), Game::U1);
    screen.finished("Uncharted 1 closed.");
    let view = screen.view(&clock());
    assert_eq!(view.overlay, OverlayView::None);
    assert_eq!(view.status, "Uncharted 1 closed.");
}

#[test]
fn reload_keeps_the_game_and_the_focus() {
    let mut screen = screen_on(Game::U2, three([0, 1, 0]));
    screen.handle(Input::Move(Nav::Right));
    assert_eq!(screen.focus(), Focus::actions(1));
    // A pull brings a newer card: replaced in place, focus kept on Play.
    let mut games = three([0, 1, 0]);
    games[1].saves[0].card.title = Some("Newer".into());
    screen.reload(games);
    assert_eq!(screen.current_game(), Game::U2);
    assert_eq!(screen.focus(), Focus::actions(1));
    assert_eq!(screen.view(&clock()).card.title, "Newer");
    // The save went away: Play moved, so focus goes to the default button.
    screen.reload(three([0, 0, 0]));
    assert_eq!(screen.focus(), Focus::actions(0));
}

#[test]
fn clicks_focus_then_act() {
    let mut screen = Screen::new(three([2, 0, 0]), None, "");
    assert_eq!(screen.click(2, 2), Command::None);
    assert_eq!(screen.current_game(), Game::U3);
    assert_eq!(screen.click(1, 3), Command::Open(Page::About));
    assert_eq!(screen.focus(), Focus::pages(3));
    assert_eq!(screen.click(0, 9), Command::None);
    screen.click(2, 0);
    assert_eq!(screen.click(0, 2), Command::None);
    assert_eq!(screen.view(&clock()).overlay.code(), 2);
    // A click on the screen behind a prompt does nothing.
    assert_eq!(screen.click(0, 0), Command::None);
    assert_eq!(
        screen.click(3, 1),
        Command::Start(Start {
            game: Game::U1,
            slot: Some(2),
            keep: None,
        })
    );

    screen.set_conflict(Game::U1, pair(Game::U1, true));
    assert_eq!(
        screen.click(4, 0),
        Command::Keep {
            game: Game::U1,
            keep: Keep::Local,
        }
    );

    screen.handle(Input::Back);
    assert_eq!(screen.click(6, 0), Command::None);
    assert_eq!(screen.view(&clock()).overlay, OverlayView::None);
    screen.handle(Input::Back);
    assert_eq!(screen.click(5, 0), Command::Quit);
}
