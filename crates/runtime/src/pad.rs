//! Deterministic synthetic pad inputs for runtime tests and benchmark replay (#173).
//!
//! This is a pure schedule: the runtime integration must tick it at 60 Hz and pass each
//! sample through the game's ordinary pad callback. It does not load a save or drive a game.
//! [`EntryScript`] is the menu lead-in that gets from the title screen into the level before
//! [`BenchmarkPath`] starts turning the camera; `benchmark_run` joins the two to the clock.

/// PlayStation button bits as `src/runtime_pad.c` reports them (the `BTN_*` enum there).
pub mod button {
    pub const CROSS: u32 = 0x4000;
}

/// The synthetic camera input for one fixed 60 Hz tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub buttons: u32,
    pub left_x: u8,
    pub left_y: u8,
    pub right_x: u8,
    pub right_y: u8,
}

impl Sample {
    /// Nothing pressed, both sticks centred.
    pub const NEUTRAL: Sample = Sample {
        buttons: 0,
        left_x: 128,
        left_y: 128,
        right_x: 128,
        right_y: 128,
    };
}

/// The title-screen lead-in: confirm the first menu entry twice, then wait for the level to load.
///
/// The first entry of the title menu is Continue (the newest save) when a save exists and New
/// Game (the first chapter) when none does, so confirming it covers both starts the issue asks
/// for. The timings are `tools/restart.sh`'s (`press.sh` holds 0.3 s; 6 s between the presses)
/// with fixed waits where that script polls the frame statistics, because a fixed schedule
/// replays the same way on every machine. They are not measured against a running game yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct EntryScript;

impl EntryScript {
    /// From the first pad read to the first confirm (the title screen settling).
    pub const TITLE_WAIT_MS: u64 = 20_000;
    /// How long each confirm is held.
    pub const PRESS_MS: u64 = 300;
    /// From the end of the first confirm to the start of the second.
    pub const CONFIRM_GAP_MS: u64 = 6_000;
    /// From the end of the second confirm until the level has loaded and the scene has settled.
    pub const LOAD_WAIT_MS: u64 = 60_000;
    pub const SECOND_PRESS_MS: u64 = Self::TITLE_WAIT_MS + Self::PRESS_MS + Self::CONFIRM_GAP_MS;
    /// The scripted camera turn starts, and the measured interval with it, at this offset.
    pub const DONE_MS: u64 = Self::SECOND_PRESS_MS + Self::PRESS_MS + Self::LOAD_WAIT_MS;

    /// The input `elapsed_ms` after the first pad read; the menu is only ever confirmed.
    pub fn sample_at_ms(self, elapsed_ms: u64) -> Sample {
        let pressed = |start: u64| (start..start + Self::PRESS_MS).contains(&elapsed_ms);
        if pressed(Self::TITLE_WAIT_MS) || pressed(Self::SECOND_PRESS_MS) {
            Sample {
                buttons: button::CROSS,
                ..Sample::NEUTRAL
            }
        } else {
            Sample::NEUTRAL
        }
    }
}

/// Two alternating eight-second camera turns, matching the existing soak script's stick
/// positions. Every tick maps to one fixed sample, independent of wall-clock timing.
#[derive(Debug, Clone, Copy, Default)]
pub struct BenchmarkPath;

impl BenchmarkPath {
    pub const TICKS_PER_TURN: u64 = 8 * 60;
    pub const PERIOD_TICKS: u64 = Self::TICKS_PER_TURN * 2;

    pub fn sample_at_tick(self, tick: u64) -> Sample {
        let turn = (tick % Self::PERIOD_TICKS) / Self::TICKS_PER_TURN;
        Sample {
            right_x: if turn == 0 { 60 } else { 195 },
            ..Sample::NEUTRAL
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_path_replays_identical_inputs_and_returns_to_start() {
        let path = BenchmarkPath;
        let run_a = (0..BenchmarkPath::PERIOD_TICKS * 2)
            .map(|tick| path.sample_at_tick(tick))
            .collect::<Vec<_>>();
        let run_b = (0..BenchmarkPath::PERIOD_TICKS * 2)
            .map(|tick| path.sample_at_tick(tick))
            .collect::<Vec<_>>();
        assert_eq!(run_a, run_b);
        assert!(run_a[..BenchmarkPath::TICKS_PER_TURN as usize]
            .iter()
            .all(|sample| sample.right_x == 60));
        assert!(run_a
            [BenchmarkPath::TICKS_PER_TURN as usize..BenchmarkPath::PERIOD_TICKS as usize]
            .iter()
            .all(|sample| sample.right_x == 195));
        assert_eq!(
            path.sample_at_tick(0),
            path.sample_at_tick(BenchmarkPath::PERIOD_TICKS)
        );
    }

    #[test]
    fn entry_script_confirms_twice_and_is_neutral_otherwise() {
        let script = EntryScript;
        let pressed: Vec<u64> = (0..EntryScript::DONE_MS + 1_000)
            .filter(|&ms| script.sample_at_ms(ms).buttons == button::CROSS)
            .collect();
        assert_eq!(pressed.len() as u64, 2 * EntryScript::PRESS_MS);
        assert_eq!(pressed[0], EntryScript::TITLE_WAIT_MS);
        assert_eq!(
            pressed[EntryScript::PRESS_MS as usize],
            EntryScript::SECOND_PRESS_MS
        );
        for ms in (0..EntryScript::DONE_MS + 1_000).filter(|ms| !pressed.contains(ms)) {
            assert_eq!(script.sample_at_ms(ms), Sample::NEUTRAL, "{ms} ms");
        }
        let again: Vec<u64> = (0..EntryScript::DONE_MS)
            .filter(|&ms| script.sample_at_ms(ms).buttons == button::CROSS)
            .collect();
        assert_eq!(pressed, again);
    }
}
