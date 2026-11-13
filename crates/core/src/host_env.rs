//! The host's library injection, taken out of the environment before the launcher starts
//! anything (SPEC.md M20, roadmap 2.10). Replaces `packaging/bbport-entry.c`, the static entry
//! program of the Nix AppImage: the app (`uncharted-app`) and the command line
//! (`uncharted-universal`) now clean their own environment at start, and `play` cleans the
//! runtime child's as well.
//!
//! Steam starts every non-Steam game with `LD_PRELOAD=.../gameoverlayrenderer.so` (any
//! compatibility tool, Steam Deck included), and with Steam runtime paths in
//! `LD_LIBRARY_PATH`. Loaded into the launcher's children they name host libraries the package
//! was not built against (the overlay looked for its `libGL.so.1` and the first program
//! exited). `GTK_PATH` and the others below name host libraries just as foreign to the
//! package; Steam's Vulkan layers (overlay, fossilize) are host libraries too, so both are
//! turned off. A non-empty value is kept as `BB_HOST_<name>` for diagnostics (the log can show
//! what the host had set).
//!
//! Linux only: on the other systems these names mean nothing to the loader and the
//! environment is left as it is.

use crate::platform::Os;
use std::process::Command;

/// The variables that name host libraries, removed (`host_only` in `bbport-entry.c`, same order).
pub const HOST_ONLY: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "GTK_PATH",
    "GTK_MODULES",
    "GTK_EXE_PREFIX",
    "GIO_EXTRA_MODULES",
    "GDK_PIXBUF_MODULEDIR",
    "PYTHONHOME",
    "PYTHONPATH",
];

/// Steam's implicit Vulkan layers, turned off with `<name>=1`.
pub const DISABLED_LAYERS: &[&str] = &[
    "DISABLE_VK_LAYER_VALVE_steam_overlay_1",
    "DISABLE_VK_LAYER_VALVE_steam_fossilize_1",
];

/// The prefix of the diagnostic copy of a removed value.
pub const SAVED_PREFIX: &str = "BB_HOST_";

/// One change to an environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Set(String, String),
    Remove(String),
}

/// The changes that clean an environment read through `lookup`, in the order
/// `bbport-entry.c` makes them: for each `HOST_ONLY` name, a non-empty value is copied to
/// `BB_HOST_<name>`, then the name is removed (always, so a child never inherits it); then each
/// of `DISABLED_LAYERS` is set to `1`. Empty off Linux.
pub fn changes(os: Os, lookup: impl Fn(&str) -> Option<String>) -> Vec<Change> {
    if os != Os::Linux {
        return Vec::new();
    }
    let mut out = Vec::new();
    for name in HOST_ONLY {
        if let Some(value) = lookup(name).filter(|value| !value.is_empty()) {
            out.push(Change::Set(format!("{SAVED_PREFIX}{name}"), value));
        }
        out.push(Change::Remove((*name).to_owned()));
    }
    for layer in DISABLED_LAYERS {
        out.push(Change::Set((*layer).to_owned(), "1".to_owned()));
    }
    out
}

/// `changes` applied to a child process's environment (the parent's stays as it is).
pub fn apply_to_command(command: &mut Command, changes: &[Change]) {
    for change in changes {
        match change {
            Change::Set(name, value) => command.env(name, value),
            Change::Remove(name) => command.env_remove(name),
        };
    }
}

/// Cleans this process's own environment, so every program it starts from now on gets the
/// cleaned one. Call it first thing in `main`, before any thread exists (changing the
/// environment while another thread reads it is not safe). Returns the changes made.
pub fn clean_process() -> Vec<Change> {
    let made = changes(Os::current(), |name| {
        std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
    });
    for change in &made {
        match change {
            Change::Set(name, value) => std::env::set_var(name, value),
            Change::Remove(name) => std::env::remove_var(name),
        }
    }
    made
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::ffi::OsStr;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// Applies `changes` to a copy of `start`, as the C program's setenv/unsetenv would.
    fn cleaned(start: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut out = start.clone();
        for change in changes(Os::Linux, |name| start.get(name).cloned()) {
            match change {
                Change::Set(name, value) => {
                    out.insert(name, value);
                }
                Change::Remove(name) => {
                    out.remove(&name);
                }
            }
        }
        out
    }

    #[test]
    fn steam_overlay_preload_is_removed_and_kept_for_diagnostics() {
        let start = env(&[
            (
                "LD_PRELOAD",
                "/home/deck/.local/share/Steam/ubuntu12_64/gameoverlayrenderer.so",
            ),
            ("HOME", "/home/deck"),
            ("SteamAppId", "0"),
        ]);
        let out = cleaned(&start);
        assert_eq!(out.get("LD_PRELOAD"), None);
        assert_eq!(
            out.get("BB_HOST_LD_PRELOAD").map(String::as_str),
            Some("/home/deck/.local/share/Steam/ubuntu12_64/gameoverlayrenderer.so")
        );
        // Everything else is left alone.
        assert_eq!(out.get("HOME").map(String::as_str), Some("/home/deck"));
        assert_eq!(out.get("SteamAppId").map(String::as_str), Some("0"));
    }

    #[test]
    fn every_host_only_name_is_removed() {
        let start: BTreeMap<String, String> = HOST_ONLY
            .iter()
            .map(|name| ((*name).to_owned(), format!("/host/{name}")))
            .collect();
        let out = cleaned(&start);
        for name in HOST_ONLY {
            assert_eq!(out.get(*name), None, "{name}");
            assert_eq!(
                out.get(&format!("BB_HOST_{name}")),
                Some(&format!("/host/{name}")),
                "{name}"
            );
        }
    }

    #[test]
    fn an_empty_value_is_removed_without_a_copy() {
        let out = cleaned(&env(&[("LD_PRELOAD", ""), ("LD_LIBRARY_PATH", "")]));
        assert_eq!(out.get("LD_PRELOAD"), None);
        assert_eq!(out.get("LD_LIBRARY_PATH"), None);
        assert_eq!(out.get("BB_HOST_LD_PRELOAD"), None);
        assert_eq!(out.get("BB_HOST_LD_LIBRARY_PATH"), None);
    }

    #[test]
    fn steam_vulkan_layers_are_turned_off() {
        let out = cleaned(&BTreeMap::new());
        for layer in DISABLED_LAYERS {
            assert_eq!(out.get(*layer).map(String::as_str), Some("1"), "{layer}");
        }
        // Nothing set means nothing copied: only the two layer switches appear.
        assert_eq!(out.len(), DISABLED_LAYERS.len());
    }

    #[test]
    fn the_order_matches_bbport_entry() {
        let start = env(&[("LD_PRELOAD", "a.so"), ("PYTHONPATH", "/p")]);
        let list = changes(Os::Linux, |name| start.get(name).cloned());
        assert_eq!(
            list[0],
            Change::Set("BB_HOST_LD_PRELOAD".into(), "a.so".into())
        );
        assert_eq!(list[1], Change::Remove("LD_PRELOAD".into()));
        assert_eq!(list[2], Change::Remove("LD_LIBRARY_PATH".into()));
        let n = list.len();
        assert_eq!(
            list[n - 3..],
            [
                Change::Remove("PYTHONPATH".into()),
                Change::Set("DISABLE_VK_LAYER_VALVE_steam_overlay_1".into(), "1".into()),
                Change::Set(
                    "DISABLE_VK_LAYER_VALVE_steam_fossilize_1".into(),
                    "1".into()
                ),
            ]
        );
        assert!(list.contains(&Change::Set("BB_HOST_PYTHONPATH".into(), "/p".into())));
    }

    #[test]
    fn other_systems_are_left_alone() {
        for os in [Os::Windows, Os::Mac, Os::Ios, Os::Android] {
            assert!(changes(os, |_| Some("x".into())).is_empty(), "{os:?}");
        }
    }

    #[test]
    fn a_child_command_gets_the_changes() {
        let mut command = Command::new("uncharted-run");
        let list = changes(Os::Linux, |name| {
            (name == "LD_PRELOAD").then(|| "/steam/overlay.so".to_owned())
        });
        apply_to_command(&mut command, &list);
        let envs: Vec<_> = command.get_envs().collect();
        assert!(envs.contains(&(OsStr::new("LD_PRELOAD"), None)));
        assert!(envs.contains(&(OsStr::new("LD_LIBRARY_PATH"), None)));
        assert!(envs.contains(&(
            OsStr::new("BB_HOST_LD_PRELOAD"),
            Some(OsStr::new("/steam/overlay.so"))
        )));
        assert!(envs.contains(&(
            OsStr::new("DISABLE_VK_LAYER_VALVE_steam_overlay_1"),
            Some(OsStr::new("1"))
        )));
    }
}
