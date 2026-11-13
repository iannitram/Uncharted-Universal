//! The guided first run (roadmap 2.05, SPEC.md M10, M11, M13): what the app's first-run pages
//! and `uncharted-universal setup` share. Whether this device is set up at all, the dump folder
//! it remembers, the folders worth offering when the player picks a dump, preparing every game a
//! checked dump holds, pairing with a hub and pulling its prepared games (feature `hub`), and
//! the Steam shortcut that opens the launcher, with the restart of Steam around it. Replaces the
//! set-up half of `launcher/uncharted_launcher.py` (`game_folder`, `add_to_steam`) and
//! `steam_running` in `launcher/bbport_steam.py`.
//!
//! Nothing here copies, uploads or logs a file of the dump (SPEC.md M9): the dump is only read by
//! `dump::check` and `prepare::prepare`, and the remembered folder is a path, not its content.

use crate::artwork::{self, Art};
use crate::dump::DumpReport;
use crate::error::{Error, Result};
use crate::games::Game;
use crate::platform::{AppDirs, Os, Vars};
use crate::prepare::{self, InstallManifest, PrepareOptions};
use crate::steam::{self, Shortcut};
use std::fs;
use std::path::{Path, PathBuf};

/// The note of the dump folder the player picked, in the launcher's config folder.
pub const DUMP_NOTE: &str = "dump_dir";
/// The paired devices, in the launcher's config folder (`hub::read_peers`).
pub const PEERS_FILE: &str = "peers.json";
/// The name of the Steam shortcut: the collection, as `shortcut_name` in the old launcher.
pub const SHORTCUT_NAME: &str = "Uncharted: The Nathan Drake Collection";
/// How deep under a search root `dump_candidates` looks for a dump folder.
const SEARCH_DEPTH: usize = 2;

/// Whether `game` is prepared on this device: its install folder holds `install.json`, which
/// `prepare` writes last (an install without it is unfinished).
pub fn installed(dirs: &AppDirs, game: Game) -> bool {
    dirs.install(game.key())
        .join(prepare::MANIFEST_FILE)
        .is_file()
}

/// The games prepared on this device, in `Game::ALL` order.
pub fn installed_games(dirs: &AppDirs) -> Vec<Game> {
    Game::ALL
        .into_iter()
        .filter(|game| installed(dirs, *game))
        .collect()
}

/// `<config>/peers.json`.
pub fn peers_file(dirs: &AppDirs) -> PathBuf {
    dirs.config.join(PEERS_FILE)
}

/// Whether this device has paired with another: a non-empty peers file. Only its size is read
/// here, so the check needs no key and works without the `hub` feature.
pub fn has_peers(dirs: &AppDirs) -> bool {
    fs::metadata(peers_file(dirs)).is_ok_and(|meta| meta.is_file() && meta.len() > 2)
}

/// First run: no game is prepared here and no device is paired. The app opens on the guided
/// first run instead of the Continue screen.
pub fn is_first_run(dirs: &AppDirs) -> bool {
    installed_games(dirs).is_empty() && !has_peers(dirs)
}

/// Note `folder` as the player's dump folder.
pub fn remember_dump(dirs: &AppDirs, folder: &Path) -> Result<()> {
    fs::create_dir_all(&dirs.config)?;
    let text = folder
        .to_str()
        .ok_or_else(|| Error::Env("the dump folder's name is not valid Unicode".into()))?;
    fs::write(dirs.config.join(DUMP_NOTE), format!("{text}\n"))?;
    Ok(())
}

/// The dump folder noted by [`remember_dump`]; `None` when there is no note or it is empty.
pub fn remembered_dump(dirs: &AppDirs) -> Option<PathBuf> {
    let text = fs::read_to_string(dirs.config.join(DUMP_NOTE)).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// The dump folder of `game`: `BB_GAME_DIR_U1` .. `_U3`, else `BB_GAME_DIR` (the order of
/// `prepare::locate`), read through `env`, else the folder the first run noted. An empty value
/// counts as unset.
pub fn dump_folder(
    game: Game,
    env: impl Fn(&str) -> Option<String>,
    dirs: Option<&AppDirs>,
) -> Option<PathBuf> {
    let own = format!("BB_GAME_DIR_{}", game.key().to_ascii_uppercase());
    env(&own)
        .filter(|v| !v.is_empty())
        .or_else(|| env("BB_GAME_DIR").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .or_else(|| dirs.and_then(remembered_dump))
}

/// Whether `folder` looks like a dump: it holds `sce_sys/param.sfo`. Only the name is looked
/// at; `dump::check` reads it.
pub fn looks_like_dump(folder: &Path) -> bool {
    folder.join("sce_sys").join("param.sfo").is_file()
}

/// The folders to look in for a dump, in order: the game folder variables, the noted folder,
/// then `<home>/Games`, `<home>/Downloads`, `<home>/Desktop` and `<home>`.
pub fn search_roots(
    vars: &Vars,
    env: impl Fn(&str) -> Option<String>,
    dirs: Option<&AppDirs>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for name in [
        "BB_GAME_DIR",
        "BB_GAME_DIR_U1",
        "BB_GAME_DIR_U2",
        "BB_GAME_DIR_U3",
    ] {
        if let Some(value) = env(name).filter(|v| !v.is_empty()) {
            roots.push(PathBuf::from(value));
        }
    }
    roots.extend(dirs.and_then(remembered_dump));
    for name in ["Games", "Downloads", "Desktop"] {
        roots.push(vars.home.join(name));
    }
    roots.push(vars.home.clone());
    let mut seen = Vec::new();
    roots.retain(|root| {
        let new = !seen.contains(root);
        seen.push(root.clone());
        new
    });
    roots
}

/// Every folder at or up to two levels under each of `roots` that [`looks_like_dump`], in the
/// order found (roots in order, names sorted within a folder), each once. Hidden folders (a
/// name starting with `.`) are not entered; a folder that cannot be read is skipped.
pub fn dump_candidates(roots: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(folder: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        if looks_like_dump(folder) {
            if !found.iter().any(|f| f == folder) {
                found.push(folder.to_path_buf());
            }
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .map(|entry| entry.path())
            .collect();
        children.sort();
        for child in children {
            walk(&child, depth - 1, found);
        }
    }
    let mut found = Vec::new();
    for root in roots {
        walk(root, SEARCH_DEPTH, &mut found);
    }
    found
}

/// Whether the hub path may go on to prepare after `report`: the dump is decrypted and holds at
/// least one game. A revision the port does not support is still let through: `prepare` refuses
/// it per game, with its own message.
pub fn can_prepare(report: &DumpReport) -> bool {
    report.decrypted && !report.games.is_empty()
}

/// The games to prepare from `report`, with the folder of each: all it found, or only `only`.
pub fn prepare_plan(report: &DumpReport, only: Option<Game>) -> Vec<(Game, PathBuf)> {
    report
        .games
        .iter()
        .filter(|found| only.is_none_or(|game| game == found.game))
        .map(|found| (found.game, found.folder.clone()))
        .collect()
}

/// What preparing one game of a plan came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub game: Game,
    /// The manifest, or the plain message of the error.
    pub result: std::result::Result<InstallManifest, String>,
}

/// Prepare every game of `plan` into its install folder (`AppDirs::install`), one after the
/// other: `progress(index, game)` is called before each. A game that fails does not stop the
/// others. When at least one game was prepared, the dump folder of the first one is noted
/// ([`remember_dump`]) so the Continue screen and later prepares find it.
pub fn prepare_all(
    plan: &[(Game, PathBuf)],
    dirs: &AppDirs,
    options: &PrepareOptions,
    mut progress: impl FnMut(usize, Game),
) -> Vec<Prepared> {
    prepare_all_with(plan, dirs, &mut progress, |dump, install, game| {
        prepare::prepare(dump, install, game, options)
    })
}

/// [`prepare_all`] with the prepare step passed in (tests count calls and fail one game).
pub fn prepare_all_with(
    plan: &[(Game, PathBuf)],
    dirs: &AppDirs,
    progress: &mut dyn FnMut(usize, Game),
    run: impl Fn(&Path, &Path, Game) -> Result<InstallManifest>,
) -> Vec<Prepared> {
    let mut done = Vec::new();
    for (index, (game, folder)) in plan.iter().enumerate() {
        progress(index, *game);
        let result = run(folder, &dirs.install(game.key()), *game).map_err(|e| e.to_string());
        done.push(Prepared {
            game: *game,
            result,
        });
    }
    if let Some((_, folder)) = plan
        .iter()
        .zip(&done)
        .find(|(_, prepared)| prepared.result.is_ok())
        .map(|(entry, _)| entry)
    {
        // Best effort: a config folder that cannot be written only loses the note.
        let _ = remember_dump(dirs, folder);
    }
    done
}

/// This device's name for pairing: `COMPUTERNAME` (Windows), else `HOSTNAME`, else
/// `/etc/hostname`, else "This device"; control characters dropped, at most 64 characters.
pub fn device_name(env: impl Fn(&str) -> Option<String>) -> String {
    let from_file = || {
        fs::read_to_string("/etc/hostname")
            .ok()
            .map(|s| s.trim().to_owned())
    };
    let name = env("COMPUTERNAME")
        .or_else(|| env("HOSTNAME"))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .or_else(from_file)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "This device".to_owned());
    name.chars().filter(|c| !c.is_control()).take(64).collect()
}

/// The six digits of a pairing code typed as `123456`, `123 456` or `123-456`; anything else is
/// `Error::Hub("the pairing code must contain six digits")`.
pub fn pairing_code(text: &str) -> Result<String> {
    let code: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) {
        Ok(code)
    } else {
        Err(Error::Hub(
            "the pairing code must contain six digits".into(),
        ))
    }
}

/// Put the pulled install `staged` in place of `install`: the device-owned folders of the old
/// install (`bundle::DEVICE_OWNED`: saves, logs, mods, user) move into the new one first, then
/// the old install is moved aside, the new one renamed into place and the old one removed. A
/// failed rename puts the old install back.
pub fn install_pulled(staged: &Path, install: &Path) -> Result<()> {
    if !staged.join(prepare::MANIFEST_FILE).is_file() {
        return Err(Error::Hub(format!(
            "the pulled install has no {}",
            prepare::MANIFEST_FILE
        )));
    }
    if let Some(parent) = install.parent() {
        fs::create_dir_all(parent)?;
    }
    let aside = install.with_extension("replaced");
    if aside.exists() {
        fs::remove_dir_all(&aside)?;
    }
    if install.exists() {
        for owned in crate::bundle::DEVICE_OWNED {
            let old = install.join(owned);
            let new = staged.join(owned);
            if old.exists() && !new.exists() {
                fs::rename(&old, &new)?;
            }
        }
        fs::rename(install, &aside)?;
    }
    if let Err(error) = fs::rename(staged, install) {
        if aside.exists() {
            let _ = fs::rename(&aside, install);
        }
        return Err(error.into());
    }
    if aside.exists() {
        fs::remove_dir_all(&aside)?;
    }
    Ok(())
}

/// Each game the pull brought (`<staged>/<game key>/install.json`), put into its install folder
/// with [`install_pulled`]. Returns the games installed, in `Game::ALL` order.
pub fn install_all_pulled(staged: &Path, dirs: &AppDirs) -> Result<Vec<Game>> {
    let mut games = Vec::new();
    for game in Game::ALL {
        let from = staged.join(game.key());
        if from.join(prepare::MANIFEST_FILE).is_file() {
            install_pulled(&from, &dirs.install(game.key()))?;
            games.push(game);
        }
    }
    Ok(games)
}

/// The total size of the regular files under `folder` (the pull's progress, measured from
/// outside it); 0 for a folder that cannot be read.
pub fn folder_bytes(folder: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(folder) else {
        return 0;
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => folder_bytes(&entry.path()),
            Ok(t) if t.is_file() => entry.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// `bytes` for a person: "0 B", "512 KB", "2.4 GB" (decimal units, one decimal from GB).
pub fn bytes_text(bytes: u64) -> String {
    const KB: u64 = 1_000;
    const MB: u64 = 1_000_000;
    const GB: u64 = 1_000_000_000;
    match bytes {
        b if b >= GB => format!("{}.{} GB", b / GB, (b % GB) / (GB / 10)),
        b if b >= MB => format!("{} MB", b / MB),
        b if b >= KB => format!("{} KB", b / KB),
        b => format!("{b} B"),
    }
}

/// The client half of the first run (feature `hub`): pair with a hub and pull its games.
#[cfg(feature = "hub")]
pub mod client {
    use super::{install_all_pulled, peers_file, AppDirs, Error, Game, Result};
    use crate::hub::{self, Clock, PairingLimiter, Peer, Transport};
    use crate::platform::Os;
    use std::net::SocketAddr;
    use std::path::{Path, PathBuf};

    /// A hub as the first-run page lists it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct HubChoice {
        /// "desktop at 192.168.1.20:47120".
        pub label: String,
        /// The name the peer record keeps (`recipe::Hub::key()`, or the address typed).
        pub name: String,
        pub addr: SocketAddr,
    }

    /// The hubs `hub::discover` found that can serve `target`, as choices.
    pub fn choices(found: &[hub::Found], target: Os) -> Vec<HubChoice> {
        found
            .iter()
            .filter(|f| f.targets.contains(&target))
            .map(|f| {
                let addr = SocketAddr::new(f.addr, f.port);
                HubChoice {
                    label: format!("{} at {addr}", f.hub),
                    name: f.hub.clone(),
                    addr,
                }
            })
            .collect()
    }

    /// A hub address typed by the player: an IP address with its port (`192.168.1.20:47120`,
    /// `[fe80::1]:47120`). One without a port, one that does not parse, or one outside the
    /// home network (`hub::is_home_network`) is `Error::Hub` with a sentence saying so.
    pub fn hub_address(text: &str) -> Result<HubChoice> {
        let text = text.trim();
        let addr: SocketAddr = text.parse().map_err(|_| {
            Error::Hub(if text.parse::<std::net::IpAddr>().is_ok() {
                "add the hub's port after its address, as 192.168.1.20:47120".into()
            } else {
                "type the hub's address and port, as 192.168.1.20:47120".into()
            })
        })?;
        if !hub::is_home_network(addr.ip()) {
            return Err(Error::Hub(format!(
                "{addr} is not on the home network; a hub is only reached at home"
            )));
        }
        Ok(HubChoice {
            label: format!("{addr}"),
            name: addr.to_string(),
            addr,
        })
    }

    /// What pairing and pulling came to.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Pulled {
        /// The games now installed, in `Game::ALL` order.
        pub games: Vec<Game>,
        /// Bytes received.
        pub bytes: u64,
        /// `None` when the hub was saved in `peers.json`; else why not (Windows has no protected
        /// peer storage yet), and the next pull pairs again.
        pub peer_note: Option<String>,
    }

    /// The staging folder of a pull: `<data>/pull`.
    pub fn staging(dirs: &AppDirs) -> PathBuf {
        dirs.data.join("pull")
    }

    /// Over one connection `t` to the hub: pair with the six-digit `code` (`hub::pair_limited`),
    /// save the hub in `peers.json`, pull the prepared games for `target` into `staged`
    /// (`hub::pull_game`) and install each (`install_all_pulled`). The staging folder is removed
    /// once everything is in place.
    pub fn pair_and_pull(
        t: &mut dyn Transport,
        hub_name: &str,
        code: &str,
        dirs: &AppDirs,
        target: Os,
        clock: &dyn Clock,
    ) -> Result<Pulled> {
        let code = super::pairing_code(code)?;
        let mut limiter = PairingLimiter::default();
        let peer = hub::pair_limited(t, &code, hub_name, &mut limiter, clock)?;
        let peer_note = save_peer(&peers_file(dirs), &peer)
            .err()
            .map(|e| e.to_string());
        let staged = staging(dirs);
        let bytes = hub::pull_game(t, &peer, target, &staged)?;
        let games = install_all_pulled(&staged, dirs)?;
        if games.is_empty() {
            return Err(Error::Hub(
                "the hub sent no prepared game; prepare the games on the hub first".into(),
            ));
        }
        let _ = std::fs::remove_dir_all(&staged);
        Ok(Pulled {
            games,
            bytes,
            peer_note,
        })
    }

    /// Add `peer` to the peers file, replacing a record of the same device.
    fn save_peer(path: &Path, peer: &Peer) -> Result<()> {
        let mut peers = hub::read_peers(path)?;
        peers.retain(|p| p.device != peer.device);
        peers.push(peer.clone());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        hub::write_peers(path, &peers)
    }
}

/// The launcher's Steam shortcut: [`SHORTCUT_NAME`], the app at `app_exe`, started in its
/// folder, with the collection icon `icon` when one was written. Steam's Play button opens the
/// launcher, whose Continue screen starts the game (M13).
pub fn launcher_shortcut(app_exe: &Path, icon: Option<&Path>) -> Shortcut {
    Shortcut {
        name: SHORTCUT_NAME.to_owned(),
        exe: app_exe.to_string_lossy().into_owned(),
        start_dir: app_exe
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        launch_options: String::new(),
        icon: icon
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// The library images for the shortcut. The icon is the collection emblem, a 256 x 256 PNG as
/// `artwork::size(Art::Icon)` asks; the other images are cut from the dump by `artwork::render`
/// when there is a dump and that is built (1.22), and are left out otherwise (Steam then draws
/// its own).
pub fn launcher_images(dump: Option<&Path>) -> Vec<(Art, Vec<u8>)> {
    let mut images = Vec::new();
    if let Some(dump) = dump {
        for art in [Art::Grid, Art::WideGrid, Art::Hero, Art::Logo] {
            if let Ok(Some(png)) = artwork::render(dump, Game::U1, art) {
                images.push((art, png));
            }
        }
    }
    images.push((Art::Icon, artwork::emblem_png().to_vec()));
    images
}

/// What adding the shortcut came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedToSteam {
    pub app_id: u32,
    /// The `shortcuts.vdf` files written, one per Steam user.
    pub shortcuts: Vec<PathBuf>,
    /// The library images written, across every user.
    pub images: Vec<PathBuf>,
}

/// The first of `steam::userdata_roots_for(os, vars)` that exists.
pub fn userdata(os: Os, vars: &Vars) -> Option<PathBuf> {
    steam::userdata_roots_for(os, vars)
        .into_iter()
        .find(|root| root.is_dir())
}

/// The Steam user folders under `userdata` (`<id>` folders whose name is all digits, except
/// `0`), sorted: the folders `steam::write_shortcut` writes to.
pub fn steam_users(userdata: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(userdata) else {
        return Vec::new();
    };
    let mut users: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.is_empty() && name != "0" && name.bytes().all(|b| b.is_ascii_digit())
        })
        .map(|entry| entry.path())
        .collect();
    users.sort();
    users
}

/// Add the launcher to Steam under `userdata`: the shortcut into every user's `shortcuts.vdf`
/// (`steam::write_shortcut`, which refuses while `steam_running` and writes nothing then), its
/// icon pointing at the first user's copy, then `images` into each user's grid folder. Returns
/// what was written.
pub fn add_to_steam(
    userdata: &Path,
    app_exe: &Path,
    images: &[(Art, Vec<u8>)],
    steam_running: &dyn Fn() -> bool,
) -> Result<AddedToSteam> {
    // The app id depends only on the program and the name, so the icon's path is known first.
    let mut shortcut = launcher_shortcut(app_exe, None);
    let app_id = shortcut.app_id();
    let users = steam_users(userdata);
    if images.iter().any(|(art, _)| *art == Art::Icon) {
        if let Some(user) = users.first() {
            let icon = artwork::grid_dir(user).join(artwork::file_name(Art::Icon, app_id));
            shortcut.icon = icon.to_string_lossy().into_owned();
        }
    }
    let shortcuts = steam::write_shortcut(userdata, &shortcut, steam_running)?;
    let mut written = Vec::new();
    for user in &users {
        written.extend(steam::write_artwork(user, app_id, images)?);
    }
    Ok(AddedToSteam {
        app_id,
        shortcuts,
        images: written,
    })
}

/// A command that asks Steam to quit or to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// The command that asks Steam to quit (`quit`) or to start again: Linux `steam -shutdown` and
/// `steam`; Windows `steam.exe -shutdown` and `steam.exe` in Steam's own folder; Mac
/// `open steam://exit` and `open -a Steam`. iOS and Android have no Steam: `None`.
pub fn steam_command(os: Os, quit: bool) -> Option<SteamCommand> {
    let (program, args): (&str, &[&str]) = match (os, quit) {
        (Os::Linux, true) => ("steam", &["-shutdown"]),
        (Os::Linux, false) => ("steam", &[]),
        (Os::Windows, true) => (r"C:\Program Files (x86)\Steam\steam.exe", &["-shutdown"]),
        (Os::Windows, false) => (r"C:\Program Files (x86)\Steam\steam.exe", &[]),
        (Os::Mac, true) => ("open", &["steam://exit"]),
        (Os::Mac, false) => ("open", &["-a", "Steam"]),
        (Os::Ios | Os::Android, _) => return None,
    };
    Some(SteamCommand {
        program: PathBuf::from(program),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
    })
}

/// Whether `tasklist /FI "IMAGENAME eq steam.exe" /NH` printed a Steam process (its output, any
/// case), as `steam_running` in `bbport_steam.py`.
pub fn tasklist_shows_steam(stdout: &[u8]) -> bool {
    String::from_utf8_lossy(stdout)
        .to_ascii_lowercase()
        .contains("steam.exe")
}

/// Whether a process under `proc` (`/proc` on Linux) is the Steam client: a numbered folder
/// whose `comm` is `steam`. Helper processes such as `steamwebhelper` do not write
/// `shortcuts.vdf`.
pub fn proc_shows_steam(proc: &Path) -> bool {
    let Ok(entries) = fs::read_dir(proc) else {
        return false;
    };
    entries.filter_map(|entry| entry.ok()).any(|entry| {
        let pid = entry.file_name();
        pid.to_string_lossy().bytes().all(|b| b.is_ascii_digit())
            && fs::read_to_string(entry.path().join("comm"))
                .is_ok_and(|comm| comm.trim() == "steam")
    })
}

/// Whether Steam is running on this device: `/proc` on Linux, `tasklist` on Windows,
/// `pgrep -x steam_osx` on Mac. When the check itself cannot run, Steam counts as not running
/// and `steam::write_shortcut` goes ahead.
pub fn steam_running() -> bool {
    match Os::current() {
        Os::Linux => proc_shows_steam(Path::new("/proc")),
        Os::Windows => std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq steam.exe", "/NH"])
            .output()
            .is_ok_and(|out| tasklist_shows_steam(&out.stdout)),
        Os::Mac => std::process::Command::new("pgrep")
            .args(["-x", "steam_osx"])
            .output()
            .is_ok_and(|out| out.status.success()),
        Os::Ios | Os::Android => false,
    }
}

#[cfg(test)]
mod tests;
