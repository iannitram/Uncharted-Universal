//! Owner-only files on Windows, without `unsafe` (`crates/core` forbids it).
//!
//! The Win32 security API needs raw pointers, which this crate cannot hold. The system's own
//! `whoami.exe` and `icacls.exe` (absolute paths under `%SystemRoot%\System32`, never searched
//! on `PATH`) do the same job: `icacls /inheritance:r /grant:r *<SID>:(F)` leaves a protected
//! DACL with one allow entry for the current user, and `icacls /save` prints the DACL as SDDL
//! so it can be checked again before a secret is read.
//!
//! The parsers are plain functions over text and compile on every OS, so Linux and macOS CI
//! test them; only the two process-running functions are `cfg(windows)`.

#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
use crate::error::{Error, Result};

/// The SID in `whoami /user /fo csv /nh` output (`"DOMAIN\user","S-1-5-..."`).
pub(super) fn parse_user_sid(whoami_csv: &str) -> Option<String> {
    let line = whoami_csv.lines().find(|l| !l.trim().is_empty())?;
    let sid = line.rsplit(',').next()?.trim().trim_matches('"');
    is_user_sid(sid).then(|| sid.to_string())
}

/// A plain account SID: `S-1-<authority>-<sub-authority>...`, digits and dashes only.
fn is_user_sid(sid: &str) -> bool {
    let Some(rest) = sid.strip_prefix("S-1-") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('-').collect();
    parts.len() >= 2
        && parts.len() <= 16
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 10 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Text of an `icacls /save` file: UTF-16 (with or without a byte-order mark) or UTF-8.
pub(super) fn decode_icacls_save(bytes: &[u8]) -> String {
    let utf16 = |data: &[u8], big: bool| -> String {
        let mut units = Vec::with_capacity(data.len() / 2);
        let mut rest = data;
        while let [first, second, tail @ ..] = rest {
            units.push(if big {
                u16::from_be_bytes([*first, *second])
            } else {
                u16::from_le_bytes([*first, *second])
            });
            rest = tail;
        }
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xff, 0xfe, rest @ ..] => utf16(rest, false),
        [0xfe, 0xff, rest @ ..] => utf16(rest, true),
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        // No mark: a NUL in the second byte of the first character means UTF-16LE.
        [_, 0, ..] => utf16(bytes, false),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Whether the SDDL line in `icacls /save` output describes a protected DACL (no inherited
/// entries) whose only entries are allow entries for exactly `sid`.
pub(super) fn sddl_is_owner_only(save_output: &str, sid: &str) -> bool {
    let Some(sddl) = save_output
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("D:"))
    else {
        return false;
    };
    let dacl = &sddl[2..];
    let dacl = dacl.split("S:").next().unwrap_or(dacl);
    let Some(open) = dacl.find('(') else {
        return false; // no entries at all: nobody (not even us) may read it
    };
    let (flags, aces) = dacl.split_at(open);
    if !flags.contains('P') {
        return false; // not protected: inheritance can add entries back
    }
    let mut count = 0;
    let mut rest = aces;
    while let Some(body) = rest.strip_prefix('(') {
        let Some(end) = body.find(')') else {
            return false;
        };
        let fields: Vec<&str> = body[..end].split(';').collect();
        // type;flags;rights;object guid;inherit guid;trustee
        if fields.len() != 6
            || fields[0] != "A"
            || fields[1].contains("ID")
            || !fields[3].is_empty()
            || !fields[4].is_empty()
            || fields[5] != sid
        {
            return false;
        }
        count += 1;
        rest = &body[end + 1..];
    }
    count > 0 && rest.is_empty()
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn system_tool(name: &str) -> PathBuf {
        let root = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("windir"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        root.join("System32").join(name)
    }

    fn run(name: &str, args: &[&std::ffi::OsStr]) -> Result<std::process::Output> {
        let output = Command::new(system_tool(name))
            .args(args)
            .stdin(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|_| Error::Hub(format!("could not run {name} to protect storage")))?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(Error::Hub(format!("{name} could not protect storage")))
        }
    }

    fn current_sid() -> Result<&'static str> {
        static SID: OnceLock<Option<String>> = OnceLock::new();
        SID.get_or_init(|| {
            let out = run(
                "whoami.exe",
                &[
                    "/user".as_ref(),
                    "/fo".as_ref(),
                    "csv".as_ref(),
                    "/nh".as_ref(),
                ],
            )
            .ok()?;
            parse_user_sid(&String::from_utf8_lossy(&out.stdout))
        })
        .as_deref()
        .ok_or_else(|| Error::Hub("could not identify the current user".into()))
    }

    /// A path icacls cannot mistake for a switch.
    fn absolute(path: &Path) -> Result<PathBuf> {
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            Ok(std::env::current_dir()?.join(path))
        }
    }

    /// Remove inherited entries from `path` and leave one allow entry: the current user.
    pub fn restrict_to_current_user(path: &Path) -> Result<()> {
        let sid = current_sid()?;
        let path = absolute(path)?;
        let grant = format!("*{sid}:(F)");
        run(
            "icacls.exe",
            &[
                path.as_os_str(),
                "/inheritance:r".as_ref(),
                "/grant:r".as_ref(),
                grant.as_ref(),
            ],
        )?;
        Ok(())
    }

    /// Whether `path` has a protected DACL naming only the current user.
    pub fn is_owner_only(path: &Path) -> Result<bool> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sid = current_sid()?;
        let path = absolute(path)?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        // Beside the checked file: its folder is writable (the file lives there) and a
        // stripped-down environment may have no usable temporary folder.
        let folder = path.parent().unwrap_or(Path::new("."));
        let out = folder.join(format!(
            ".acl-check-{}-{nanos}-{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let result = run(
            "icacls.exe",
            &[path.as_os_str(), "/save".as_ref(), out.as_os_str()],
        )
        .and_then(|_| Ok(std::fs::read(&out)?));
        let _ = std::fs::remove_file(&out);
        Ok(sddl_is_owner_only(&decode_icacls_save(&result?), sid))
    }
}

#[cfg(windows)]
pub(super) use imp::{is_owner_only, restrict_to_current_user};

#[cfg(test)]
mod tests {
    use super::*;

    const SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";

    #[test]
    fn parses_whoami_csv() {
        let line = format!("\"host\\user\",\"{SID}\"\r\n");
        assert_eq!(parse_user_sid(&line).as_deref(), Some(SID));
        assert_eq!(parse_user_sid(""), None);
        assert_eq!(parse_user_sid("\"a\",\"not-a-sid\""), None);
        assert_eq!(parse_user_sid("\"a\",\"S-1-5-21-x\""), None);
        assert_eq!(parse_user_sid("\"a\",\"S-1-\""), None);
    }

    #[test]
    fn owner_only_accepts_one_protected_allow_for_the_user() {
        let ok = format!("file.json\nD:PAI(A;;FA;;;{SID})\n");
        assert!(sddl_is_owner_only(&ok, SID));
        assert!(sddl_is_owner_only(&format!("D:P(A;;FA;;;{SID})"), SID));
        assert!(sddl_is_owner_only(
            &format!("D:PAI(A;;0x1f01ff;;;{SID})S:AI(ML;;NW;;;LW)"),
            SID
        ));
    }

    #[test]
    fn owner_only_refuses_everything_else() {
        let other = "S-1-5-21-1-2-3-1002";
        // inheritance not blocked, inherited and extra entries, other users, denies, empty
        assert!(!sddl_is_owner_only(&format!("D:AI(A;;FA;;;{SID})"), SID));
        assert!(!sddl_is_owner_only(&format!("D:PAI(A;ID;FA;;;{SID})"), SID));
        assert!(!sddl_is_owner_only(
            &format!("D:PAI(A;;FA;;;{SID})(A;;FA;;;SY)"),
            SID
        ));
        assert!(!sddl_is_owner_only(&format!("D:PAI(A;;FA;;;{other})"), SID));
        assert!(!sddl_is_owner_only(&format!("D:PAI(D;;FA;;;{SID})"), SID));
        assert!(!sddl_is_owner_only(&format!("D:PAI(A;;FA;;;{SID}"), SID));
        assert!(!sddl_is_owner_only(&format!("D:PAI(A;;FA;;;{SID})x"), SID));
        assert!(!sddl_is_owner_only("D:PAI", SID));
        assert!(!sddl_is_owner_only("D:P", SID));
        assert!(!sddl_is_owner_only("no sddl here", SID));
        assert!(!sddl_is_owner_only("", SID));
        assert!(!sddl_is_owner_only("D:PAI(A;;FA;;;S-1-1-0)", SID));
    }

    #[test]
    fn decodes_utf16_and_utf8_save_files() {
        let text = format!("f\r\nD:PAI(A;;FA;;;{SID})\r\n");
        let mut le = vec![0xff, 0xfe];
        le.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode_icacls_save(&le), text);
        let bare: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode_icacls_save(&bare), text);
        let mut be = vec![0xfe, 0xff];
        be.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decode_icacls_save(&be), text);
        assert_eq!(decode_icacls_save(text.as_bytes()), text);
        assert!(sddl_is_owner_only(&decode_icacls_save(&le), SID));
    }
}
