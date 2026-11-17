//! Raw mouse settings in `<data>/input.ini`, the file `core::launch` passes to the runtime.
//! Only these two values belong to this page; bindings, comments and formatting are preserved.

use std::{fs, io, path::Path};

const FILE: &str = "input.ini";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub raw: bool,
    // Hundredths avoid floating point drift when stepping the setting with a controller.
    sensitivity: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self::new(false, 1.0)
    }
}

impl Settings {
    pub fn new(raw: bool, sensitivity: f64) -> Self {
        let sensitivity = if sensitivity.is_finite() {
            sensitivity.clamp(0.1, 3.0)
        } else {
            1.0
        };
        Self {
            raw,
            sensitivity: (sensitivity * 100.0).round() as u16,
        }
    }

    pub fn sensitivity(self) -> String {
        format!("{:.2}", f64::from(self.sensitivity) / 100.0)
    }
}

fn read(data: &Path) -> io::Result<String> {
    match fs::read_to_string(data.join(FILE)) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        result => result,
    }
}

fn assignment(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim().trim_start_matches('\u{feff}'), value))
}

fn value_end(value: &str) -> usize {
    value.find(['#', ';']).unwrap_or(value.len())
}

/// Missing keys keep the shipped defaults. Last assignment wins, as in the runtime parser.
pub fn load(data: &Path) -> io::Result<Settings> {
    let mut raw = false;
    let mut sensitivity = 1.0;
    for line in read(data)?.lines() {
        if let Some((key, value)) = assignment(line) {
            let value = value[..value_end(value)].trim();
            match key {
                "mouse_raw" => raw = value.parse::<i32>().unwrap_or(0) != 0,
                "mouse_aim_sensitivity" => sensitivity = value.parse().unwrap_or(1.0),
                _ => {}
            }
        }
    }
    Ok(Settings::new(raw, sensitivity))
}

/// Replace values in place, including duplicate assignments, and append only missing keys.
/// Reading errors stop the update rather than replacing a file we could not read.
pub fn save(data: &Path, settings: Settings) -> io::Result<()> {
    let text = read(data)?;
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let values = [
        ("mouse_raw", u8::from(settings.raw).to_string()),
        ("mouse_aim_sensitivity", settings.sensitivity()),
    ];
    let mut seen = [false; 2];
    let mut output = String::new();
    for line in text.split_inclusive('\n') {
        if let Some((key, value)) = assignment(line) {
            if let Some(i) = values.iter().position(|(name, _)| key == *name) {
                let start = value.len() - value.trim_start_matches([' ', '\t']).len();
                let end = value[..value_end(value)].trim_end().len().max(start);
                output.push_str(&line[..line.len() - value.len() + start]);
                output.push_str(&values[i].1);
                output.push_str(&value[end..]);
                seen[i] = true;
                continue;
            }
        }
        output.push_str(line);
    }
    for (i, (key, value)) in values.iter().enumerate() {
        if !seen[i] {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push_str(newline);
            }
            output.push_str(&format!("{key}={value}{newline}"));
        }
    }
    fs::create_dir_all(data)?;
    let temp = data.join("input.ini.tmp");
    fs::write(&temp, output)?;
    fs::rename(temp, data.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::tests::Temp;

    #[test]
    fn defaults_are_off_and_one() {
        let temp = Temp::new();
        assert_eq!(load(&temp.0).unwrap(), Settings::default());
        assert!(!Settings::default().raw);
        assert_eq!(Settings::default().sensitivity(), "1.00");
        fs::write(temp.0.join(FILE), "mouse_look=1\n").unwrap();
        assert_eq!(load(&temp.0).unwrap(), Settings::default());
    }

    #[test]
    fn sensitivity_is_clamped_and_rounded() {
        for (value, expected) in [
            (-1.0, "0.10"),
            (0.1, "0.10"),
            (3.0, "3.00"),
            (99.0, "3.00"),
            (1.234, "1.23"),
            (f64::NAN, "1.00"),
            (f64::INFINITY, "1.00"),
        ] {
            assert_eq!(Settings::new(false, value).sensitivity(), expected);
        }
        let temp = Temp::new();
        fs::write(temp.0.join(FILE), "mouse_aim_sensitivity=8\n").unwrap();
        assert_eq!(load(&temp.0).unwrap().sensitivity(), "3.00");
        save(&temp.0, Settings::new(true, -5.0)).unwrap();
        assert!(fs::read_to_string(temp.0.join(FILE))
            .unwrap()
            .contains("mouse_aim_sensitivity=0.10"));
    }

    #[test]
    fn round_trip_creates_the_file_and_reads_later_edits() {
        let temp = Temp::new();
        let data = temp.0.join("data");
        let settings = Settings::new(true, 1.37);
        save(&data, settings).unwrap();
        assert_eq!(load(&data).unwrap(), settings);
        assert_eq!(
            fs::read_to_string(data.join(FILE)).unwrap(),
            "mouse_raw=1\nmouse_aim_sensitivity=1.37\n"
        );
        save(&data, Settings::default()).unwrap();
        assert_eq!(load(&data).unwrap(), Settings::default());
        fs::write(data.join(FILE), "mouse_raw=1\nmouse_aim_sensitivity=2.50\n").unwrap();
        assert_eq!(load(&data).unwrap(), Settings::new(true, 2.5));
    }

    #[test]
    fn update_keeps_other_keys_lines_comments_and_endings() {
        let temp = Temp::new();
        let before = "# Controls\r\nmouse_look=1\r\n\r\n mouse_raw = 0  # opt in\r\nkey.cross=space\r\npad.cross=a\r\nmouse_aim_sensitivity=0.50 ; aim\r\nmouse_raw=0\r\n# end";
        fs::write(temp.0.join(FILE), before).unwrap();
        save(&temp.0, Settings::new(true, 2.0)).unwrap();
        let after = fs::read_to_string(temp.0.join(FILE)).unwrap();
        assert_eq!(
            after,
            before
                .replace("mouse_raw = 0", "mouse_raw = 1")
                .replace("mouse_raw=0", "mouse_raw=1")
                .replace("sensitivity=0.50", "sensitivity=2.00")
        );
        assert_eq!(load(&temp.0).unwrap(), Settings::new(true, 2.0));
        fs::write(temp.0.join(FILE), "# no trailing newline").unwrap();
        save(&temp.0, Settings::default()).unwrap();
        assert_eq!(
            fs::read_to_string(temp.0.join(FILE)).unwrap(),
            "# no trailing newline\nmouse_raw=0\nmouse_aim_sensitivity=1.00\n"
        );
    }
}
