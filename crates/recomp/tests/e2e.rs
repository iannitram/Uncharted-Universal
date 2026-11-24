//! End to end on authored synthetic code (tests/recomp/fixture.rs): scan, generate, compile,
//! then run the library under bbcpu and against the interpreter alone. No game file is read.
#![cfg(all(target_vendor = "apple", target_arch = "aarch64"))]

mod common;
#[path = "../../../tests/recomp/fixture.rs"]
mod fixture;

use common::{run_host, temp_dir, HostRun};
use fixture::Fixture;
use std::path::PathBuf;
use std::process::Command;
use uncharted_core::link::{ImportSlot, RecompInputs};
use uncharted_core::prepare::nid;
use uncharted_recomp::pipeline::{run, Request, Summary};

/// The relocation facts a real executable's dynamic data would give: the import stub's slot is
/// `setjmp`'s.
fn inputs(fixture: &Fixture) -> RecompInputs {
    RecompInputs {
        has_dynamic: true,
        import_slots: vec![ImportSlot {
            slot: fixture.import_slot,
            nid: nid("setjmp"),
        }],
        code_pointers: Vec::new(),
    }
}

struct Built {
    elf: PathBuf,
    library: PathBuf,
    summary: Summary,
    fixture: Fixture,
}

fn build(name: &str) -> Built {
    let fixture = fixture::build();
    let out = temp_dir(name);
    let summary = run(&Request {
        title: "u1",
        elf: &fixture.elf,
        inputs: &inputs(&fixture),
        out: &out,
        jobs: 2,
    })
    .unwrap_or_else(|e| panic!("the pipeline failed: {e}"));
    Built {
        elf: out.join("image.elf"),
        library: out.join("librecomp-u1.dylib"),
        summary,
        fixture,
    }
}

fn call(fixture: &Fixture, name: &str, args: &[&str]) -> String {
    format!("{:#x}:{}", fixture.offset(name), args.join(","))
}

/// The calls every test makes, with what the code means (computed here, not by bbcpu).
fn calls(f: &Fixture) -> (Vec<String>, Vec<u64>) {
    let mut calls = Vec::new();
    let mut expected = Vec::new();
    calls.push(call(f, "add3", &["1", "2", "3"]));
    expected.push(6);
    calls.push(call(f, "sum_to", &["100"]));
    expected.push(5050);
    calls.push(call(f, "sum_to", &["0"]));
    expected.push(0);
    calls.push(call(f, "call_chain", &["10", "20", "30"]));
    expected.push(61);
    for i in 0..=5u64 {
        calls.push(call(f, "switch4", &[&i.to_string()]));
        expected.push(if i < 4 { 10 * (i + 1) } else { 0xffff_ffff });
    }
    calls.push(call(f, "square_f", &["12345"]));
    expected.push(12345 * 12345);
    calls.push(call(f, "bump", &["@scratch", "5"]));
    expected.push(5);
    calls.push(call(f, "bump", &["@scratch", "7"]));
    expected.push(12);
    calls.push(call(f, "call_import", &["3", "4"]));
    expected.push(3 * 1000 + 4 + 7); // the host's import
    (calls, expected)
}

fn host_args(f: &Fixture, extra: &[String]) -> Vec<String> {
    let mut args = vec!["--import".to_string(), format!("{:#x}", f.import_slot)];
    args.extend_from_slice(extra);
    args
}

#[test]
fn the_generated_library_matches_the_interpreter_and_the_expected_results() {
    let built = build("match");
    let f = &built.fixture;
    let s = &built.summary;
    // Nine functions: eight have unwind entries, add3 is a leaf the scan reaches through the call.
    assert_eq!(s.functions_found, 9);
    assert_eq!(s.functions_unwound, 8);
    // The stub is named setjmp from its slot: it and its caller stay with the translator.
    assert_eq!(s.import_stubs, 1);
    assert_eq!(s.setjmp_callers_left, 1);
    assert_eq!(s.import_stubs_left, 1);
    assert_eq!(s.functions_generated, 7);
    assert!(
        s.instructions_in_c > 0 && s.instructions_in_c < s.instructions,
        "{s:?}"
    );
    assert!(s.library_bytes > 0 && s.generated_c_bytes > 0);
    let summary_json = std::fs::read_to_string(built.elf.with_file_name("summary.json")).unwrap();
    assert!(summary_json.contains("\"functions_generated\": 7"));

    let (calls, expected) = calls(f);
    let args = host_args(f, &[]);
    let interpreter = run_host(&built.elf, None, &args, &calls);
    let library = run_host(&built.elf, Some(&built.library), &args, &calls);
    assert_eq!(interpreter.code(), Some(0), "{}", interpreter.stderr());
    assert_eq!(library.code(), Some(0), "{}", library.stderr());
    assert_eq!(interpreter.recompiled(), 0);
    assert_eq!(
        library.recompiled(),
        7,
        "every generated function matches its code"
    );
    assert_eq!(interpreter.rax(), expected);
    assert_eq!(library.rax(), expected);
    assert_eq!(
        interpreter.results(),
        library.results(),
        "registers and memory agree"
    );
    assert!(library
        .results()
        .iter()
        .any(|l| l.starts_with("scratch 0c00000000000000")));
}

#[test]
fn a_patched_byte_makes_the_hash_check_fall_back() {
    let built = build("patched");
    let f = &built.fixture;
    // add3's `add rax,rdx` (48 01 d0) becomes `sub rax,rdx` (48 29 d0) after the library was made.
    let patch = format!("{:#x}:0x29", f.offset("add3") + 5);
    let args = host_args(f, &["--patch".to_string(), patch]);
    let calls = vec![
        call(f, "add3", &["1", "2", "3"]),
        call(f, "call_chain", &["10", "20", "30"]),
        call(f, "sum_to", &["10"]),
    ];
    let interpreter = run_host(&built.elf, None, &args, &calls);
    let library = run_host(&built.elf, Some(&built.library), &args, &calls);
    assert_eq!(library.code(), Some(0), "{}", library.stderr());
    // One fewer function is replaced: the patched one is left to the interpreter.
    assert_eq!(library.recompiled(), 6);
    assert!(
        library.stdout().contains("1 changed by patches"),
        "{}",
        library.stdout()
    );
    // The patched code's results, from the interpreter, also where recompiled code calls it.
    assert_eq!(interpreter.rax(), vec![0, 1, 55]);
    assert_eq!(library.rax(), vec![0, 1, 55]);
    assert_eq!(interpreter.results(), library.results());
}

fn assert_refused(run: &HostRun, what: &str) {
    assert_eq!(run.code(), Some(30), "{what}: {}", run.stderr());
    assert!(
        run.stderr().contains("unsupported instruction: pdep"),
        "{what}: {}",
        run.stderr()
    );
    assert!(
        !run.stdout()
            .lines()
            .any(|line| line.starts_with("call ") && line.contains("rax=")),
        "{what}: no result may be made up for it"
    );
}

#[test]
fn an_unsupported_instruction_is_refused_cleanly() {
    let built = build("unsupported");
    let f = &built.fixture;
    // The generator finishes: the instruction is left to the interpreter, not translated.
    assert!(built.summary.instructions_in_c < built.summary.instructions);
    let calls = vec![call(f, "pdep_user", &["5", "9"])];
    let args = host_args(f, &[]);
    assert_refused(&run_host(&built.elf, None, &args, &calls), "interpreter");
    assert_refused(
        &run_host(&built.elf, Some(&built.library), &args, &calls),
        "library",
    );
}

#[test]
fn the_driver_builds_a_library_from_a_game_folder_and_refuses_unsafe_outputs() {
    let fixture = fixture::build();
    let game = temp_dir("driver-game");
    std::fs::write(game.join("eboot.bin"), &fixture.elf).unwrap();
    let work = temp_dir("driver-work");
    let driver = env!("CARGO_BIN_EXE_uu-recomp");
    let run = |args: &[&str]| Command::new(driver).args(args).output().unwrap();
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();

    // A plain ELF has no dynamic data: refused unless asked for.
    let out = work.join("out/u1");
    let refused = run(&[
        "--title",
        "u1",
        "--game-dir",
        game.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        text(&refused.stderr).contains("no dynamic data"),
        "{}",
        text(&refused.stderr)
    );
    assert!(!out.exists(), "nothing is written before the checks");

    let done = run(&[
        "--title",
        "u1",
        "--game-dir",
        game.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--allow-no-dynamic",
        "--jobs",
        "2",
    ]);
    assert_eq!(done.status.code(), Some(0), "{}", text(&done.stderr));
    assert!(out.join("librecomp-u1.dylib").is_file());
    let summary: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["functions_found"], 9);
    // With no import names, nothing is left out: all nine are generated and load.
    assert_eq!(summary["functions_generated"], 9);
    let f = &fixture;
    let args = host_args(f, &[]);
    let calls = vec![
        call(f, "call_chain", &["1", "2", "3"]),
        call(f, "sum_to", &["4"]),
    ];
    let library = run_host(
        &out.join("image.elf"),
        Some(&out.join("librecomp-u1.dylib")),
        &args,
        &calls,
    );
    assert_eq!(library.rax(), vec![7, 10], "{}", library.stderr());
    assert_eq!(library.recompiled(), 9);

    // The default folders are not used by tests; unsafe ones are refused before anything is read.
    let checkout = temp_dir("driver-checkout");
    std::fs::create_dir_all(checkout.join(".git")).unwrap();
    let inside = checkout.join("recomp-out/u1");
    let refused = run(&[
        "--title",
        "u1",
        "--game-dir",
        game.to_str().unwrap(),
        "--out",
        inside.to_str().unwrap(),
        "--allow-no-dynamic",
    ]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        text(&refused.stderr).contains("git checkout"),
        "{}",
        text(&refused.stderr)
    );
    assert!(!inside.exists());
    let in_game = game.join("recomp");
    let refused = run(&[
        "--title",
        "u1",
        "--game-dir",
        game.to_str().unwrap(),
        "--out",
        in_game.to_str().unwrap(),
        "--allow-no-dynamic",
    ]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(!in_game.exists());

    // Bad arguments.
    assert_eq!(run(&["--title", "u9"]).status.code(), Some(2));
    assert_eq!(run(&[]).status.code(), Some(2));
}
