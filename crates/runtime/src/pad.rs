//! Deterministic synthetic pad inputs for runtime tests and benchmark replay (#173).
//!
//! This is a pure schedule: the runtime integration must tick it at 60 Hz and pass each
//! sample through the game's ordinary pad callback. It does not load a save or drive a game.

/// The synthetic camera input for one fixed 60 Hz tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub buttons: u32,
    pub left_x: u8,
    pub left_y: u8,
    pub right_x: u8,
    pub right_y: u8,
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
            buttons: 0,
            left_x: 128,
            left_y: 128,
            right_x: if turn == 0 { 60 } else { 195 },
            right_y: 128,
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
}
