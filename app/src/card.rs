//! The words on a save card (docs/continue-screen.md, "Save card fields"): the saved-at line,
//! the device line and the play time line. A field that is `None` gives no line, never an error
//! or a placeholder word. English only until the seven languages (2.09).

/// What the card needs to know about time: now, and this device's offset from UTC (seconds,
/// east positive) at a given moment, so a save made before a daylight-saving change still shows
/// its own local time.
pub struct Clock<'a> {
    pub now: u64,
    pub offset: &'a dyn Fn(u64) -> i64,
}

impl Clock<'_> {
    /// The local day number (days since 1970-01-01) and the seconds into that day.
    fn local(&self, unix: u64) -> (i64, i64) {
        let local = unix as i64 + (self.offset)(unix);
        (local.div_euclid(86_400), local.rem_euclid(86_400))
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The calendar date (year, month 1-12, day 1-31) of a day number (days since 1970-01-01).
pub(crate) fn civil(days: i64) -> (i64, usize, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month as usize, day)
}

/// When a save was made, without the word "Saved": "today 21:14", "yesterday 18:02", else the
/// date ("3 Nov 2026"), in this device's time. A time ahead of this device's clock shows its
/// date, never "in 3 hours". `None` for 0 (not known).
pub fn when(saved_unix: u64, clock: &Clock) -> Option<String> {
    if saved_unix == 0 {
        return None;
    }
    let (day, secs) = clock.local(saved_unix);
    let (today, _) = clock.local(clock.now);
    let clock_time = format!("{:02}:{:02}", secs / 3_600, secs % 3_600 / 60);
    if saved_unix <= clock.now && day == today {
        return Some(format!("today {clock_time}"));
    }
    if saved_unix <= clock.now && day == today - 1 {
        return Some(format!("yesterday {clock_time}"));
    }
    let (year, month, date) = civil(day);
    Some(format!("{date} {} {year}", MONTHS[month - 1]))
}

/// The card's time line: "Saved today 21:14", or "Save time not known" for 0.
pub fn saved_line(saved_unix: u64, clock: &Clock) -> String {
    when(saved_unix, clock).map_or_else(
        || "Save time not known".to_owned(),
        |when| format!("Saved {when}"),
    )
}

/// "on the Strix Halo tablet".
pub fn device_line(device: &str) -> String {
    format!("on the {device}")
}

/// "Play time 6 h 12 min"; minutes only under an hour.
pub fn play_time_line(secs: u64) -> String {
    let minutes = secs / 60;
    if minutes < 60 {
        format!("Play time {minutes} min")
    } else {
        format!("Play time {} h {} min", minutes / 60, minutes % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-12 21:30:00 UTC, a Thursday.
    const NOW: u64 = 1_794_519_000;

    fn utc(unix: u64) -> Option<String> {
        when(
            unix,
            &Clock {
                now: NOW,
                offset: &|_| 0,
            },
        )
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(-1), (1969, 12, 31));
        assert_eq!(civil(11_016), (2000, 2, 29));
        assert_eq!(civil(20_769), (2026, 11, 12));
    }

    #[test]
    fn today_yesterday_and_dates() {
        assert_eq!(utc(NOW - 16 * 60).as_deref(), Some("today 21:14"));
        assert_eq!(
            utc(NOW - 21 * 3_600 - 30 * 60).as_deref(),
            Some("today 00:00")
        );
        assert_eq!(
            utc(NOW - 21 * 3_600 - 31 * 60).as_deref(),
            Some("yesterday 23:59")
        );
        assert_eq!(utc(NOW - 9 * 86_400).as_deref(), Some("3 Nov 2026"));
        assert_eq!(utc(1).as_deref(), Some("1 Jan 1970"));
    }

    #[test]
    fn a_save_from_the_future_shows_its_date() {
        assert_eq!(utc(NOW + 3 * 3_600).as_deref(), Some("13 Nov 2026"));
        // Later the same day is still a date, never "today" or "in 2 hours".
        assert_eq!(utc(NOW + 60).as_deref(), Some("12 Nov 2026"));
    }

    #[test]
    fn local_time_uses_the_offset_at_the_save() {
        // UTC+1 then: 21:14 UTC is 22:14 local; now (21:30 UTC) is 22:30 local, same day.
        let clock = Clock {
            now: NOW,
            offset: &|_| 3_600,
        };
        assert_eq!(saved_line(NOW - 16 * 60, &clock), "Saved today 22:14");
        // UTC-10: now is 11:30 on the 12th; 23:00 UTC on the 11th is 13:00 the 11th.
        let clock = Clock {
            now: NOW,
            offset: &|_| -36_000,
        };
        assert_eq!(
            saved_line(NOW - 22 * 3_600 - 30 * 60, &clock),
            "Saved yesterday 13:00"
        );
    }

    #[test]
    fn zero_is_not_known() {
        assert_eq!(utc(0), None);
        let clock = Clock {
            now: NOW,
            offset: &|_| 0,
        };
        assert_eq!(saved_line(0, &clock), "Save time not known");
    }

    #[test]
    fn device_and_play_time_lines() {
        assert_eq!(device_line("Strix Halo tablet"), "on the Strix Halo tablet");
        assert_eq!(
            play_time_line(6 * 3_600 + 12 * 60 + 59),
            "Play time 6 h 12 min"
        );
        assert_eq!(play_time_line(45 * 60), "Play time 45 min");
        assert_eq!(play_time_line(59), "Play time 0 min");
        assert_eq!(play_time_line(3_600), "Play time 1 h 0 min");
    }
}
