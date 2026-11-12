//! One-line runtime crash records consumed by `crates/core::logs::crash_line` (#168).
//!
//! This module owns the output format, but the legacy C probe/assertion handlers are not yet
//! wired to the Rust runtime. That integration follows the loader port (roadmap 3.16).

use std::io::{self, Write};

/// The failure category written to the `kind` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashKind {
    Assertion,
    Fault,
    Exception,
}

impl CrashKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Assertion => "assert",
            Self::Fault => "fault",
            Self::Exception => "exception",
        }
    }
}

/// Write one bounded `Crash:` record to stderr.
///
/// Only the caller-provided guest offset or source location and short diagnostic are emitted;
/// no paths to game data, dump contents, or guest buffers are read or serialized here.
pub fn report(kind: CrashKind, location: &str, text: &str) -> io::Result<()> {
    write_record(&mut io::stderr().lock(), kind, location, text)
}

fn write_record(
    writer: &mut impl Write,
    kind: CrashKind,
    location: &str,
    text: &str,
) -> io::Result<()> {
    let location = one_line(location, 128);
    let text = one_line(text, 200);
    writeln!(
        writer,
        "Crash: kind={} where={} text={}",
        kind.as_str(),
        if location.is_empty() {
            "unknown"
        } else {
            &location
        },
        if text.is_empty() { "unknown" } else { &text },
    )
}

fn one_line(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .take(max_chars)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{write_record, CrashKind};

    #[test]
    fn forced_fault_emits_one_bounded_parseable_crash_line() {
        let mut output = Vec::new();
        write_record(
            &mut output,
            CrashKind::Fault,
            "0x1234\nforged prefix",
            &format!("read access to 0x5678{}", "é".repeat(250)),
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("Crash: kind=fault where=0x1234 forged prefix text="));
        let text = lines[0].split_once(" text=").unwrap().1;
        assert_eq!(text.chars().count(), 200);
        assert!(!text.chars().any(char::is_control));
    }
}
