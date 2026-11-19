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
