//! Guest entry preparation, `BB_RESTART_COMMAND` handling, and `runtime_quit` / `runtime_restart`
//! helpers for `uncharted-run` (roadmap 3.17 phase 2c, #457).

use super::{BootOptions, ContentProfile};

/// Reads and validates a `BBCONT01` content profile file if configured in `opts`.
pub fn load_content_profile(opts: &BootOptions) -> Result<Option<ContentProfile>, String> {
    let Some(path) = opts.content_profile.as_deref() else {
        return Ok(None);
    };
    let bytes = std::fs::read(path).map_err(|_| "cannot open content profile".to_string())?;
    ContentProfile::parse(&bytes).map(Some)
}

/// Resolves the restart command from `BB_RESTART_COMMAND`, returning `None` when unset or empty.
pub fn restart_command_from_env(var: Option<&str>) -> Option<String> {
    var.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

/// Flushes standard streams and terminates the process with `code`.
pub fn quit_process(code: i32) -> ! {
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_command_rejects_empty_or_whitespace() {
        assert_eq!(restart_command_from_env(None), None);
        assert_eq!(restart_command_from_env(Some("")), None);
        assert_eq!(restart_command_from_env(Some("   ")), None);
        assert_eq!(
            restart_command_from_env(Some("uncharted-universal play u2")),
            Some("uncharted-universal play u2".to_string())
        );
    }

    #[test]
    fn load_content_profile_parses_valid_bbcont01_file() {
        let path =
            std::env::temp_dir().join(format!("uncharted-entry-test-{}.bin", std::process::id()));
        let mut raw = Vec::new();
        raw.extend_from_slice(b"BBCONT01");
        for v in [1u32, 2, 3, 4, 5] {
            raw.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&path, raw).unwrap();

        let opts = BootOptions {
            boot_file: "boot.bin".into(),
            cpu_only: true,
            strict_imports: false,
            list_missing: false,
            content_profile: Some(path.to_string_lossy().into_owned()),
            app0: None,
            user_dir: None,
            patch_files: Vec::new(),
            guest_argv_file: None,
            trophy_dir: None,
            timeout_seconds: 10,
        };
        let parsed = load_content_profile(&opts).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed, Some(ContentProfile([1, 2, 3, 4, 5])));
    }
}
