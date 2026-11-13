//! `uncharted-universal hub serve|pair|pull` (roadmap 4.05): a build hub on this machine, and
//! a device pairing with and pulling from one, over `uncharted_core::hub`'s service (one port,
//! pairing and sealed bundle pulls). The network side needs the `hub` feature; a build without
//! it checks the arguments and ends with "not built yet" (status 3).
//!
//! Folders: the paired-device file is `<config>/peers.json`, a hub serves the builds published
//! under `<data>/builds/<build name>/bundle.txt`, and a pull stages into `<data>/staged/<target>`
//! for the update flow to install. A device's name is `--name`, else `BB_DEVICE_NAME`, else the
//! machine's host name.

// Without the `hub` feature the plans are checked but not run, so some of their fields and
// helpers are only read by the tests.
#![cfg_attr(not(feature = "hub"), allow(dead_code))]

use super::{core, parsed, Done};
use crate::args::Parsed;
use crate::{Ctx, Outcome};
use std::path::PathBuf;
use uncharted_core::platform::{AppDirs, Os};
use uncharted_core::recipe::{self, Hub};
#[cfg(not(feature = "hub"))]
use uncharted_core::Error;

/// The paired-device file under the launcher's config folder.
pub const PEERS_FILE: &str = "peers.json";
/// How long `pair` and `pull` look for a hub on the home network when no `--hub` is given.
pub const DISCOVER_MS: u64 = 3000;

/// The lines `hub --help` adds under the command's usage.
pub const DETAILS: &str = "\
  hub serve [--builds <folder>] [--role strix-halo|desktop|mac] [--name <device>]
            [--bind <address>] [--port <n>] [--stop-after <n>]
      run a hub here: show a pairing code, announce it on the home network and serve the
      newest build in the builds folder to paired devices (until stopped, or <n> connections)
  hub pair <code> [--hub <address[:port]>] [--name <device>]
      pair with the hub showing <code>; without --hub, look for one on the home network
  hub pull [--hub <address[:port]>] [--from <hub>] [--target <os>] [--into <folder>]
           [--name <device>]
      pull and check this device's newest build from a paired hub
";

pub fn hub(args: &[String], ctx: &Ctx) -> Done {
    let (sub, rest) = match args.split_first() {
        None => return Err(Outcome::usage("hub", "name serve, pair or pull")),
        Some((sub, rest)) => (sub.as_str(), rest),
    };
    match sub {
        "serve" => serve(rest, ctx),
        "pair" => pair(rest, ctx),
        "pull" => pull(rest, ctx),
        other => Err(Outcome::usage(
            "hub",
            &format!("unknown hub command '{other}' (use serve, pair or pull)"),
        )),
    }
}

/// The paired-device file.
fn peers_file(ctx: &Ctx) -> std::result::Result<PathBuf, Outcome> {
    Ok(AppDirs::new(&core("hub", super::vars(ctx))?)
        .config
        .join(PEERS_FILE))
}

fn data_dir(ctx: &Ctx) -> std::result::Result<PathBuf, Outcome> {
    Ok(AppDirs::new(&core("hub", super::vars(ctx))?).data)
}

/// This device's name: `--name`, else `BB_DEVICE_NAME`, else the host name (`COMPUTERNAME`,
/// `HOSTNAME`, `/etc/hostname` on Linux), else "uncharted-universal".
pub fn device_name(p: &Parsed, ctx: &Ctx) -> String {
    p.flag("name")
        .map(str::to_owned)
        .or_else(|| ctx.var("BB_DEVICE_NAME"))
        .or_else(|| ctx.var("COMPUTERNAME"))
        .or_else(|| ctx.var("HOSTNAME"))
        .or_else(|| {
            if ctx.os != Os::Linux || !cfg!(target_os = "linux") {
                return None;
            }
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|text| text.trim().to_owned())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| "uncharted-universal".into())
}

/// The hub role this OS plays by default (ADR 0004): Linux is the Strix Halo tablet, Windows
/// the desktop, macOS the Mac.
pub fn default_role(os: Os) -> Option<Hub> {
    match os {
        Os::Linux => Some(Hub::StrixHalo),
        Os::Windows => Some(Hub::Desktop),
        Os::Mac => Some(Hub::Mac),
        Os::Ios | Os::Android => None,
    }
}

fn role(p: &Parsed, ctx: &Ctx) -> std::result::Result<Hub, Outcome> {
    match p.flag("role") {
        Some(key) => Hub::from_key(key).map_err(|_| {
            Outcome::usage(
                "hub",
                &format!("unknown hub role '{key}' (use strix-halo, desktop or mac)"),
            )
        }),
        None => default_role(ctx.os)
            .ok_or_else(|| Outcome::usage("hub", "name the hub's role with --role")),
    }
}

/// A `--hub` value: `address:port`, `[v6 address]:port`, or an address alone (port `PORT`).
pub fn hub_address(text: &str, default_port: u16) -> Option<std::net::SocketAddr> {
    text.parse().ok().or_else(|| {
        text.parse::<std::net::IpAddr>()
            .ok()
            .map(|ip| std::net::SocketAddr::new(ip, default_port))
    })
}

/// A whole number flag, or the usage error naming it.
fn number<T: std::str::FromStr>(p: &Parsed, name: &str) -> std::result::Result<Option<T>, Outcome> {
    p.flag(name)
        .map(|text| {
            text.parse().map_err(|_| {
                Outcome::usage(
                    "hub",
                    &format!("--{name} takes a whole number, not '{text}'"),
                )
            })
        })
        .transpose()
}

fn target(p: &Parsed, ctx: &Ctx) -> std::result::Result<Os, Outcome> {
    match p.flag("target") {
        None => Ok(ctx.os),
        Some(key) => Os::from_key(key).ok_or_else(|| {
            Outcome::usage(
                "hub",
                &format!("unknown target '{key}' (use linux, windows, mac, ios or android)"),
            )
        }),
    }
}

// ---- serve ----

/// What `serve` was asked to do, checked before anything is opened.
#[derive(Debug, PartialEq, Eq)]
pub struct ServePlan {
    pub name: String,
    pub role: Hub,
    pub targets: Vec<Os>,
    pub builds: PathBuf,
    pub peers: PathBuf,
    pub bind: std::net::IpAddr,
    pub port: u16,
    pub stop_after: Option<u64>,
}

pub fn serve_plan(args: &[String], ctx: &Ctx) -> std::result::Result<ServePlan, Outcome> {
    let p = parsed(
        "hub",
        args,
        &["builds", "role", "name", "bind", "port", "stop-after"],
        0,
    )?;
    let role = role(&p, ctx)?;
    let bind = match p.flag("bind") {
        None => std::net::IpAddr::from([0, 0, 0, 0]),
        Some(text) => text.parse().map_err(|_| {
            Outcome::usage("hub", &format!("--bind takes an address, not '{text}'"))
        })?,
    };
    let builds = match p.flag("builds") {
        Some(folder) => PathBuf::from(folder),
        None => data_dir(ctx)?.join("builds"),
    };
    Ok(ServePlan {
        name: device_name(&p, ctx),
        role,
        targets: recipe::serves(role).to_vec(),
        builds,
        peers: peers_file(ctx)?,
        bind,
        port: number(&p, "port")?.unwrap_or(PORT),
        stop_after: number(&p, "stop-after")?,
    })
}

/// The default port, without the `hub` feature too.
#[cfg(feature = "hub")]
const PORT: u16 = uncharted_core::hub::PORT;
#[cfg(not(feature = "hub"))]
const PORT: u16 = 47_210;

#[cfg(not(feature = "hub"))]
fn serve(args: &[String], ctx: &Ctx) -> Done {
    serve_plan(args, ctx)?;
    Err(Outcome::error(
        "hub",
        &Error::NotBuilt("hub serve (this build has no hub feature)"),
    ))
}

/// Runs the hub until `stop_after` connections were handled (forever without it). It prints as
/// it goes, since it runs for long: where it listens, the pairing code, then one line per
/// connection. A failed connection is reported and the hub goes on.
#[cfg(feature = "hub")]
fn serve(args: &[String], ctx: &Ctx) -> Done {
    use std::io::Write;
    use uncharted_core::hub::{self, Announcement, HubService, MdnsDaemon, PairingServer, Served};

    let plan = serve_plan(args, ctx)?;
    // Fail before listening where paired devices cannot be stored (Windows, for now).
    core("hub", hub::read_peers(&plan.peers))?;
    if !plan.builds.is_dir() {
        return Err(Outcome::error(
            "hub",
            &uncharted_core::Error::Hub(format!(
                "no builds folder at {}; prepare a build first",
                plan.builds.display()
            )),
        ));
    }
    let listener = core(
        "hub",
        std::net::TcpListener::bind((plan.bind, plan.port)).map_err(uncharted_core::Error::from),
    )?;
    let local = core(
        "hub",
        listener.local_addr().map_err(uncharted_core::Error::from),
    )?;
    let pairing = core("hub", PairingServer::new(&plan.name, &plan.peers))?;
    let mut service = HubService::new(pairing, &plan.peers, &plan.builds, &plan.targets);

    let say = |text: String| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
    };
    let warn = |text: String| {
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "{text}");
        let _ = err.flush();
    };
    let targets: Vec<&str> = plan.targets.iter().map(|os| os.key()).collect();
    // A hub bound to loopback is a test hub: nothing on the network could reach it.
    let _mdns = if plan.bind.is_loopback() {
        None
    } else {
        let announcement = Announcement {
            hub: plan.role,
            targets: plan.targets.clone(),
            port: local.port(),
            addrs: if plan.bind.is_unspecified() {
                Vec::new()
            } else {
                vec![plan.bind]
            },
        };
        match MdnsDaemon::new().and_then(|net| hub::announce(&net, &announcement).map(|()| net)) {
            Ok(net) => Some(net),
            Err(error) => {
                warn(format!(
                    "uncharted-universal hub: not announced on the home network ({error}); \
                     devices can still pair with --hub <this address>:{}",
                    local.port()
                ));
                None
            }
        }
    };
    say(format!(
        "hub {} ({}) listening on {local}, serving {} from {}",
        plan.name,
        plan.role.key(),
        targets.join(", "),
        plan.builds.display()
    ));
    say(format!(
        "pairing code: {} (on the device: uncharted-universal hub pair {})",
        service.pairing_code(),
        service.pairing_code().replace(' ', "")
    ));

    let mut handled = 0u64;
    while plan.stop_after.is_none_or(|stop| handled < stop) {
        match service.accept(&listener) {
            Ok(Served::Paired(peer)) => say(format!(
                "paired with {}; the code is used up (restart the hub to pair another device)",
                peer.device
            )),
            Ok(Served::Sent { device, bundle }) => say(format!(
                "sent build {} for {} (patch level {}, {} files) to {device}",
                bundle.build_id,
                bundle.target.key(),
                bundle.patch_level,
                bundle.files.len()
            )),
            Err(error) => warn(format!(
                "uncharted-universal hub: a connection failed: {error}"
            )),
        }
        handled += 1;
    }
    Ok(Outcome::default())
}

// ---- pair ----

/// What `pair` was asked to do.
#[derive(Debug, PartialEq, Eq)]
pub struct PairPlan {
    /// The six digits.
    pub code: String,
    pub hub: Option<std::net::SocketAddr>,
    pub name: String,
    pub peers: PathBuf,
}

pub fn pair_plan(args: &[String], ctx: &Ctx) -> std::result::Result<PairPlan, Outcome> {
    let p = parsed("hub", args, &["hub", "name"], 1)?;
    let typed = p
        .positional
        .first()
        .ok_or_else(|| Outcome::usage("hub", "give the pairing code the hub shows"))?;
    let code: String = typed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Outcome::usage(
            "hub",
            &format!("the pairing code is six digits, not '{typed}'"),
        ));
    }
    Ok(PairPlan {
        code,
        hub: hub_flag(&p)?,
        name: device_name(&p, ctx),
        peers: peers_file(ctx)?,
    })
}

fn hub_flag(p: &Parsed) -> std::result::Result<Option<std::net::SocketAddr>, Outcome> {
    p.flag("hub")
        .map(|text| {
            hub_address(text, PORT).ok_or_else(|| {
                Outcome::usage(
                    "hub",
                    &format!(
                        "--hub takes an address such as 192.168.1.20 or 192.168.1.20:{PORT}, \
                         not '{text}'"
                    ),
                )
            })
        })
        .transpose()
}

#[cfg(not(feature = "hub"))]
fn pair(args: &[String], ctx: &Ctx) -> Done {
    pair_plan(args, ctx)?;
    Err(Outcome::error(
        "hub",
        &Error::NotBuilt("hub pair (this build has no hub feature)"),
    ))
}

#[cfg(feature = "hub")]
fn pair(args: &[String], ctx: &Ctx) -> Done {
    use uncharted_core::hub;

    let plan = pair_plan(args, ctx)?;
    core("hub", hub::read_peers(&plan.peers))?;
    let mut out = Outcome::default();
    let addr = match plan.hub {
        Some(addr) => addr,
        None => find_hub(&mut out, None)?,
    };
    let peer = core(
        "hub",
        hub::pair_with_service(addr, &plan.code, &plan.name, &plan.peers),
    )?;
    out.line(format!(
        "paired with {} at {addr} as {}; pull its build with: uncharted-universal hub pull",
        peer.device, plan.name
    ));
    Ok(out)
}

/// The one hub on the home network (serving `target`, when given), or why there is none to
/// pick. Says which hub it found.
#[cfg(feature = "hub")]
fn find_hub(
    out: &mut Outcome,
    target: Option<Os>,
) -> std::result::Result<std::net::SocketAddr, Outcome> {
    let found: Vec<_> = core("hub", uncharted_core::hub::discover(DISCOVER_MS))?
        .into_iter()
        .filter(|f| target.is_none_or(|os| f.targets.contains(&os)))
        .collect();
    let serving = target.map_or(String::new(), |os| format!(" serving {}", os.key()));
    match found.as_slice() {
        [] => Err(Outcome::error(
            "hub",
            &uncharted_core::Error::Hub(format!(
                "no hub{serving} found on the home network; start one with \
                 `uncharted-universal hub serve`, or name it with --hub <address>"
            )),
        )),
        [one] => {
            let addr = std::net::SocketAddr::new(one.addr, one.port);
            out.line(format!("found the {} hub at {addr}", one.hub));
            Ok(addr)
        }
        several => {
            let list: Vec<String> = several
                .iter()
                .map(|f| format!("{} at {}", f.hub, std::net::SocketAddr::new(f.addr, f.port)))
                .collect();
            Err(Outcome::error(
                "hub",
                &uncharted_core::Error::Hub(format!(
                    "several hubs{serving} found ({}); name one with --hub <address>",
                    list.join(", ")
                )),
            ))
        }
    }
}

// ---- pull ----

/// What `pull` was asked to do.
#[derive(Debug, PartialEq, Eq)]
pub struct PullPlan {
    pub hub: Option<std::net::SocketAddr>,
    /// The paired hub's name, when given.
    pub from: Option<String>,
    pub target: Os,
    pub into: PathBuf,
    pub name: String,
    pub peers: PathBuf,
}

pub fn pull_plan(args: &[String], ctx: &Ctx) -> std::result::Result<PullPlan, Outcome> {
    let p = parsed("hub", args, &["hub", "from", "target", "into", "name"], 0)?;
    let target = target(&p, ctx)?;
    let into = match p.flag("into") {
        Some(folder) => PathBuf::from(folder),
        None => data_dir(ctx)?.join("staged").join(target.key()),
    };
    Ok(PullPlan {
        hub: hub_flag(&p)?,
        from: p.flag("from").map(str::to_owned),
        target,
        into,
        name: device_name(&p, ctx),
        peers: peers_file(ctx)?,
    })
}

/// The paired hub to pull from: the one named, else the only one; `names` are the paired
/// devices' names. The problem in words otherwise.
pub fn choose_hub<'a>(names: &[&'a str], from: Option<&str>) -> Result<&'a str, String> {
    match (from, names) {
        (Some(wanted), _) => names
            .iter()
            .copied()
            .find(|name| *name == wanted)
            .ok_or_else(|| format!("this device is not paired with {wanted}")),
        (None, []) => Err(
            "this device is not paired with a hub yet; run `uncharted-universal hub pair <code>` \
             first"
                .into(),
        ),
        (None, [one]) => Ok(one),
        (None, several) => Err(format!(
            "this device is paired with several hubs ({}); name one with --from",
            several.join(", ")
        )),
    }
}

#[cfg(not(feature = "hub"))]
fn pull(args: &[String], ctx: &Ctx) -> Done {
    pull_plan(args, ctx)?;
    Err(Outcome::error(
        "hub",
        &Error::NotBuilt("hub pull (this build has no hub feature)"),
    ))
}

#[cfg(feature = "hub")]
fn pull(args: &[String], ctx: &Ctx) -> Done {
    use uncharted_core::bundle::Kind;
    use uncharted_core::hub;

    let plan = pull_plan(args, ctx)?;
    let peers = core("hub", hub::read_peers(&plan.peers))?;
    let names: Vec<&str> = peers.iter().map(|peer| peer.device.as_str()).collect();
    let chosen = choose_hub(&names, plan.from.as_deref())
        .map_err(|problem| Outcome::error("hub", &uncharted_core::Error::Hub(problem)))?;
    let peer = peers
        .iter()
        .find(|peer| peer.device == chosen)
        .expect("choose_hub picks a listed name");
    let mut out = Outcome::default();
    let addr = match plan.hub {
        Some(addr) => addr,
        None => find_hub(&mut out, Some(plan.target))?,
    };
    let bundle = core(
        "hub",
        hub::pull_from_service(addr, peer, &plan.name, plan.target, &plan.into),
    )?;
    let shaders = bundle
        .files
        .iter()
        .filter(|file| file.kind == Kind::ShaderTranslation)
        .count();
    out.line(format!(
        "pulled build {} for {} from {} (patch level {}, {} files, {shaders} shader \
         translations) into {}",
        bundle.build_id,
        bundle.target.key(),
        peer.device,
        bundle.patch_level,
        bundle.files.len(),
        plan.into.display()
    ));
    Ok(out)
}
