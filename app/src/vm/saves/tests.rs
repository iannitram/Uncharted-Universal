use super::*;
use crate::focus::Nav;

fn clock() -> Clock<'static> {
    Clock {
        now: 1_794_519_000,
        offset: &|_| 0,
    }
}

fn slot(slot: u8, used: bool) -> SlotInfo {
    SlotInfo {
        slot,
        name: String::new(),
        used,
        detail: if used {
            "Chapter 4".into()
        } else {
            String::new()
        },
        saved_unix: if used { 1_794_518_040 } else { 0 },
    }
}

/// Uncharted 2 with saves in slots 1 and 3 (3 active), slot 2 free, two backups; the others
/// empty.
fn games() -> Vec<GameSaves> {
    let mut all: Vec<GameSaves> = Game::ALL.into_iter().map(GameSaves::empty).collect();
    all[1] = GameSaves {
        game: Game::U2,
        slots: vec![slot(1, true), slot(3, true)],
        selected: 3,
        free: Some(2),
        backups: vec![stamp(1_794_000_000, 1), stamp(1_794_500_000, 3)],
    };
    all
}

#[test]
fn opens_on_the_game_with_focus_on_the_active_slot() {
    let vm = SavesVm::new(games(), Game::U2, "relay off");
    assert_eq!(vm.current_game(), Game::U2);
    assert_eq!(vm.focus(), Grid::at(1, 0));
    let view = vm.view(&clock());
    assert_eq!(view.slots.len(), 2);
    assert_eq!(view.slots[1].title, "Slot 3");
    assert_eq!(view.slots[1].mark, "Active");
    assert_eq!(
        view.slots[1].buttons,
        ["Active", "Copy to slot 2", "Back up", "Delete"]
    );
    assert_eq!(view.slots[0].buttons[0], "Use this slot");
    assert_eq!(view.slots[0].detail, "Chapter 4 · Saved today 21:14");
    // Newest backup first.
    assert_eq!(view.backups[0].title, "Backup of slot 3");
    assert_eq!(view.backups[1].title, "Backup of slot 1");
    assert_eq!(view.backups[0].buttons, ["Restore to slot 2"]);
    assert_eq!(
        view.relay,
        "save-relay syncs the active slot of Uncharted 2: Slot 3. relay off"
    );
    assert_eq!(view.confirm, None);
}

#[test]
fn every_button_gives_its_command() {
    let mut vm = SavesVm::new(games(), Game::U2, "");
    // Slot 1: Use it.
    vm.handle(Input::Move(Nav::Up));
    assert_eq!(
        vm.handle(Input::Activate),
        SavesCommand::Select {
            game: Game::U2,
            slot: 1
        }
    );
    vm.handle(Input::Move(Nav::Right));
    assert_eq!(
        vm.handle(Input::Activate),
        SavesCommand::Copy {
            game: Game::U2,
            from: 1,
            to: 2
        }
    );
    vm.handle(Input::Move(Nav::Right));
    assert_eq!(
        vm.handle(Input::Activate),
        SavesCommand::BackUp {
            game: Game::U2,
            slot: 1
        }
    );
    // Down to the first backup row: one button, the column cut to it.
    vm.handle(Input::Move(Nav::Down));
    vm.handle(Input::Move(Nav::Down));
    assert_eq!(vm.focus(), Grid::at(2, 0));
    assert_eq!(
        vm.handle(Input::Activate),
        SavesCommand::Restore {
            game: Game::U2,
            stamp: stamp(1_794_500_000, 3),
            to: 2
        }
    );
    assert_eq!(vm.handle(Input::Back), SavesCommand::Back);
}

#[test]
fn the_active_slot_says_so_instead_of_selecting_again() {
    let mut vm = SavesVm::new(games(), Game::U2, "");
    assert_eq!(vm.handle(Input::Activate), SavesCommand::None);
    assert_eq!(
        vm.view(&clock()).status,
        "Slot 3 is already the active one."
    );
}

#[test]
fn delete_asks_first() {
    let mut vm = SavesVm::new(games(), Game::U2, "");
    vm.handle(Input::Move(Nav::Right));
    vm.handle(Input::Move(Nav::Right));
    vm.handle(Input::Move(Nav::Right));
    assert_eq!(vm.handle(Input::Activate), SavesCommand::None);
    assert_eq!(
        vm.view(&clock()).confirm.as_deref(),
        Some("Delete the saves in slot 3 of Uncharted 2? A backup is made first.")
    );
    // Moving does nothing while it asks; B cancels.
    vm.handle(Input::Move(Nav::Left));
    assert_eq!(vm.handle(Input::Back), SavesCommand::None);
    assert_eq!(vm.view(&clock()).confirm, None);
    assert_eq!(vm.focus(), Grid::at(1, 3));
    vm.handle(Input::Activate);
    assert_eq!(
        vm.handle(Input::Activate),
        SavesCommand::Delete {
            game: Game::U2,
            slot: 3
        }
    );
    // A click on the prompt's confirm button.
    vm.handle(Input::Activate);
    assert_eq!(
        vm.click(-2, 0),
        SavesCommand::Delete {
            game: Game::U2,
            slot: 3
        }
    );
}

#[test]
fn lb_and_rb_change_game_and_an_empty_slot_has_only_use() {
    let mut vm = SavesVm::new(games(), Game::U2, "");
    vm.handle(Input::NextGame);
    assert_eq!(vm.current_game(), Game::U3);
    let view = vm.view(&clock());
    assert_eq!(view.slots.len(), 1);
    assert_eq!(view.slots[0].detail, "Empty");
    assert_eq!(view.slots[0].buttons, ["Active"]);
    assert!(view.backups.is_empty());
    vm.handle(Input::NextGame);
    assert_eq!(vm.current_game(), Game::U1);
    vm.handle(Input::PrevGame);
    vm.handle(Input::PrevGame);
    assert_eq!(vm.current_game(), Game::U2);
    // A click on the game strip.
    vm.click(-1, 0);
    assert_eq!(vm.current_game(), Game::U1);
}

#[test]
fn no_free_slot_hides_copy_and_restore() {
    let mut all = games();
    all[1].free = None;
    let vm = SavesVm::new(all, Game::U2, "");
    let view = vm.view(&clock());
    assert_eq!(view.slots[1].buttons, ["Active", "Back up", "Delete"]);
    assert!(view.backups[0].buttons.is_empty());
    assert!(view.backups[0]
        .detail
        .ends_with("No free slot to restore to"));
}

#[test]
fn a_click_focuses_and_acts() {
    let mut vm = SavesVm::new(games(), Game::U2, "");
    assert_eq!(
        vm.click(0, 2),
        SavesCommand::BackUp {
            game: Game::U2,
            slot: 1
        }
    );
    assert_eq!(vm.focus(), Grid::at(0, 2));
    // Past the row's buttons: nothing.
    assert_eq!(vm.click(0, 9), SavesCommand::None);
    assert_eq!(vm.click(9, 0), SavesCommand::None);
}

#[test]
fn reload_keeps_the_game_and_the_nearest_button() {
    let mut vm = SavesVm::new(games(), Game::U2, "");
    vm.handle(Input::Move(Nav::Right));
    vm.handle(Input::Move(Nav::Right));
    vm.handle(Input::Move(Nav::Right));
    // Slot 3 deleted: slot 1 is active, slot 3 gone.
    let mut after = games();
    after[1].slots.truncate(1);
    after[1].selected = 1;
    vm.reload(after);
    assert_eq!(vm.current_game(), Game::U2);
    assert_eq!(vm.focus(), Grid::at(1, 0));
    assert_eq!(vm.view(&clock()).backups[0].buttons, ["Restore to slot 2"]);
}

#[test]
fn stamps_round_trip_and_sort_by_time() {
    let s = stamp(1_794_519_000, 2);
    assert_eq!(s, "20261112-213000-slot2");
    assert_eq!(stamp_parts(&s), Some((1_794_519_000, 2)));
    assert!(stamp(1_794_519_000, 9) < stamp(1_794_519_001, 1));
    assert_eq!(stamp_parts(&stamp(0, 10)), Some((0, 10)));
    for bad in [
        "",
        "backup",
        "20261312-215000-slot2",
        "2026111-215000-slot2",
        "20261112-215000",
    ] {
        assert_eq!(stamp_parts(bad), None, "{bad}");
    }
    // A folder named another way shows by its name.
    let mut all = games();
    all[1].backups = vec!["by hand".into()];
    let vm = SavesVm::new(all, Game::U2, "");
    assert_eq!(vm.view(&clock()).backups[0].title, "by hand");
}
