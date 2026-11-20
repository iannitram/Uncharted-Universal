//! Synthetic child for the Windows package Play test. Built in a temporary directory.
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::exit;

fn main() {
    let dir = PathBuf::from(env::var_os("RELAY_FIXTURE").unwrap());
    let args: Vec<_> = env::args().skip(1).collect();
    let log_path = dir.join("calls");
    let step = fs::read_to_string(&log_path)
        .unwrap_or_default()
        .lines()
        .count();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .unwrap();
    if matches!(args.first().map(String::as_str), Some("pull" | "push")) {
        writeln!(log, "{}", args.join("\t")).unwrap();
        if env::var_os("RELAY_SLOT_FIXTURE").is_some() {
            let option = |name: &str| args.iter().position(|arg| arg == name)
                .and_then(|at| args.get(at + 1));
            let id = option("--slot").map_or_else(|| args[1].clone(),
                |slot| format!("{}-slot-{slot}", args[1]));
            let folder = PathBuf::from(option("--saves-dir").unwrap());
            let server = dir.join("server").join(&id);
            let local = folder.join("progress");
            if args[0] == "push" && local.is_file() {
                fs::create_dir_all(server.parent().unwrap()).unwrap();
                fs::copy(&local, &server).unwrap();
            } else if args[0] == "pull" && server.is_file() && !local.exists() {
                fs::create_dir_all(&folder).unwrap();
                fs::copy(&server, &local).unwrap();
            }
            println!("{{\"ok\":true,\"command\":\"{}\",\"data\":{{\"game\":\"{}\",\"action\":\"noop\"}}}}", args[0], id);
            return;
        }
        print!(
            "{}",
            fs::read_to_string(dir.join(format!("{step}.json"))).unwrap()
        );
        let code: i32 = fs::read_to_string(dir.join(format!("{step}.code")))
            .unwrap()
            .parse()
            .unwrap();
        exit(code);
    }
    writeln!(log, "run\t{}", env::var("BB_EXE").unwrap()).unwrap();
    println!("synthetic runtime stdout");
    let request = dir.join(format!("{step}.switch"));
    if request.exists() {
        let target = PathBuf::from(env::var_os("BB_LOADEXEC_FILE").unwrap());
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(request, target).unwrap();
        exit(101);
    }
}
