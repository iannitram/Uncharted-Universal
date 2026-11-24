//! executable -> normalized ELF -> scan -> bbrecomp -> clang -> library, in one output folder.
//!
//! ```text
//! <out>/image.elf                  the normalized ELF (prepare::load_image), what the tools read
//! <out>/scan/functions.tsv ...     uu-scan's tables, imports.tsv and pointers.txt from the executable
//! <out>/gen/recomp*.c              uu-bbrecomp's C, one file per 128 KiB window and the table
//! <out>/obj/*.o                    clang's objects
//! <out>/librecomp-<title>.dylib    what BB_RECOMP_LIB names
//! <out>/summary.json               counts and timings only: no offsets, hashes or names
//! ```
//!
//! Nothing here chooses the folder: the caller passes one `guard::check_output_dir` accepted.
//! `BB_RECOMP_ANNOTATIONS` is removed from the generator's environment, so no name table can
//! reach the generated code.

use crate::tools;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use uncharted_core::link::RecompInputs;
use uncharted_core::prepare::nid;

/// What to build.
pub struct Request<'a> {
    /// "u1", "u2" or "u3": names the library.
    pub title: &'a str,
    /// The normalized ELF (`prepare::Image::elf`, or an authored plain ELF).
    pub elf: &'a [u8],
    /// Import slots and code pointers read from the executable's dynamic data.
    pub inputs: &'a RecompInputs,
    /// An accepted output folder (created if needed).
    pub out: &'a Path,
    /// Compiler processes at a time.
    pub jobs: usize,
}

/// Aggregates of a run. Counts, sizes and seconds only.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub title: String,
    /// Functions the scan found: the unwind tables plus what control flow and pointers reach.
    pub functions_found: u64,
    pub functions_unwound: u64,
    /// Import stubs (a jump through an import slot) named in imports.tsv.
    pub import_stubs: u64,
    /// Functions the generator wrote C for.
    pub functions_generated: u64,
    pub instructions: u64,
    /// Instructions translated to C; the rest are interpreted when run.
    pub instructions_in_c: u64,
    /// Callers of setjmp-like imports, and import stubs, left to the translator.
    pub setjmp_callers_left: u64,
    pub import_stubs_left: u64,
    pub c_files: u64,
    pub generated_c_bytes: u64,
    pub library_bytes: u64,
    pub seconds_scan: f64,
    pub seconds_generate: f64,
    pub seconds_compile: f64,
    pub seconds_total: f64,
}

impl Summary {
    /// `summary.json`: the fields above, nothing else.
    pub fn to_json(&self) -> String {
        let value = serde_json::json!({
            "title": self.title,
            "functions_found": self.functions_found,
            "functions_unwound": self.functions_unwound,
            "import_stubs": self.import_stubs,
            "functions_generated": self.functions_generated,
            "instructions": self.instructions,
            "instructions_in_c": self.instructions_in_c,
            "setjmp_callers_left": self.setjmp_callers_left,
            "import_stubs_left": self.import_stubs_left,
            "c_files": self.c_files,
            "generated_c_bytes": self.generated_c_bytes,
            "library_bytes": self.library_bytes,
            "seconds": {
                "scan": self.seconds_scan,
                "generate": self.seconds_generate,
                "compile": self.seconds_compile,
                "total": self.seconds_total,
            },
        });
        let mut text = serde_json::to_string_pretty(&value).expect("a JSON value");
        text.push('\n');
        text
    }
}

/// The imports whose behaviour the generator depends on, by the names the guest library
/// exports them under: setjmp-like imports (their callers stay with the translator) and the
/// ones that never return (nothing after the call is its fall-through).
const NAMED_IMPORTS: [&str; 10] = [
    "setjmp",
    "_setjmp",
    "sigsetjmp",
    "longjmp",
    "_longjmp",
    "siglongjmp",
    "__stack_chk_fail",
    "abort",
    "exit",
    "_exit",
];

/// `imports.tsv`: the import stubs of the scan, by name (`stub offset<TAB>name`). A stub is a
/// function of at most two instructions that jumps through an import slot; the name is one of
/// `NAMED_IMPORTS` when the slot's NID is that name's, else the NID.
pub fn imports_tsv(functions_tsv: &str, slots_tsv: &str, inputs: &RecompInputs) -> (String, u64) {
    let names: BTreeMap<String, &str> = NAMED_IMPORTS.iter().map(|n| (nid(n), *n)).collect();
    let slot_nid: BTreeMap<u64, &str> = inputs
        .import_slots
        .iter()
        .map(|s| (s.slot, s.nid.as_str()))
        .collect();
    let mut small: BTreeMap<u64, bool> = BTreeMap::new();
    for line in functions_tsv.lines().skip(1) {
        let mut cells = line.split('\t');
        if let (Some(address), Some(_size), Some(instructions)) =
            (cells.next(), cells.next(), cells.next())
        {
            if let (Some(address), Ok(instructions)) = (hex(address), instructions.parse::<u64>()) {
                small.insert(address, instructions <= 2);
            }
        }
    }
    let mut stubs: BTreeMap<u64, String> = BTreeMap::new();
    for line in slots_tsv.lines().skip(1) {
        let mut cells = line.split('\t');
        if let (Some(caller), Some(slot)) = (cells.next().and_then(hex), cells.next().and_then(hex))
        {
            if small.get(&caller) == Some(&true) {
                if let Some(nid) = slot_nid.get(&slot) {
                    let name = names.get(*nid).copied().unwrap_or(nid);
                    stubs.insert(caller, name.to_string());
                }
            }
        }
    }
    let mut text = String::from("stub\tname\n");
    for (stub, name) in &stubs {
        text.push_str(&format!("{stub:#x}\t{name}\n"));
    }
    (text, stubs.len() as u64)
}

fn hex(text: &str) -> Option<u64> {
    u64::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
}

/// The unsigned integers in a line, in order ("12 functions (3 from ...)" gives 12, 3).
fn numbers(line: &str) -> Vec<u64> {
    line.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn run_tool(command: &mut Command, what: &str) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|e| format!("{what}: cannot run: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(8).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(format!(
            "{what} failed ({}): {}",
            output.status,
            tail.join(" | ")
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn reset(dir: &Path) -> Result<(), String> {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

/// Name of the file that marks a folder as made by `uu-recomp`.
const MARKER: &str = ".uu-recomp";

/// Make `out` ours, or refuse. An existing folder that has files and no marker is somebody
/// else's: the pipeline deletes `scan`, `gen` and `obj` inside its folder, so it never starts in
/// one it did not make.
fn claim_output(out: &Path) -> Result<(), String> {
    let marker = out.join(MARKER);
    if !marker.exists() {
        let occupied = match fs::read_dir(out) {
            Ok(mut entries) => entries.next().is_some(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(format!("{}: {e}", out.display())),
        };
        if occupied {
            return Err(format!(
                "{}: the folder already has files and was not made by uu-recomp (no {MARKER}); \
                 use a new or empty folder",
                out.display()
            ));
        }
    }
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    fs::write(
        &marker,
        "made by uu-recomp: scan, gen and obj here are rebuilt on every run\n",
    )
    .map_err(|e| format!("{}: {e}", marker.display()))
}

fn seconds(since: Instant) -> f64 {
    (since.elapsed().as_secs_f64() * 100.0).round() / 100.0
}

/// Run the whole pipeline.
pub fn run(request: &Request) -> Result<Summary, String> {
    let started = Instant::now();
    let out = request.out;
    let scan_dir = out.join("scan");
    let gen_dir = out.join("gen");
    let obj_dir = out.join("obj");
    claim_output(out)?;
    for dir in [&scan_dir, &gen_dir, &obj_dir] {
        reset(dir)?;
    }
    let write = |path: PathBuf, bytes: &[u8]| {
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    };
    let elf = out.join("image.elf");
    write(elf.clone(), request.elf)?;

    let mut pointers = String::new();
    for target in &request.inputs.code_pointers {
        pointers.push_str(&format!("{target:#x}\n"));
    }
    let pointers_file = scan_dir.join("pointers.txt");
    write(pointers_file.clone(), pointers.as_bytes())?;

    // Scan: functions.tsv, calls.tsv, slots.tsv (and strings.tsv, which nothing reads: removed).
    let scan_started = Instant::now();
    let scan_out = run_tool(
        Command::new(tools::tool("uu-scan")?)
            .arg(&elf)
            .arg(&scan_dir)
            .arg(&pointers_file),
        "uu-scan",
    )?;
    let _ = fs::remove_file(scan_dir.join("strings.tsv"));
    let functions_tsv = scan_dir.join("functions.tsv");
    let functions_text =
        fs::read_to_string(&functions_tsv).map_err(|e| format!("functions.tsv: {e}"))?;
    let slots_text =
        fs::read_to_string(scan_dir.join("slots.tsv")).map_err(|e| format!("slots.tsv: {e}"))?;
    let (imports, import_stubs) = imports_tsv(&functions_text, &slots_text, request.inputs);
    write(scan_dir.join("imports.tsv"), imports.as_bytes())?;
    let mut list = String::new();
    for line in functions_text.lines().skip(1) {
        if let Some(address) = line.split('\t').next() {
            list.push_str(address);
            list.push('\n');
        }
    }
    let list_file = gen_dir.join("functions.list");
    write(list_file.clone(), list.as_bytes())?;
    let seconds_scan = seconds(scan_started);
    let scan_numbers = scan_out
        .lines()
        .find(|line| line.contains("from the unwind tables"))
        .map(numbers)
        .unwrap_or_default();

    // Generate.
    let generate_started = Instant::now();
    let generated = run_tool(
        Command::new(tools::tool("uu-bbrecomp")?)
            .env("BB_JIT", "0")
            .env_remove("BB_RECOMP_ANNOTATIONS")
            .env_remove("BB_RECOMP_TRACE")
            .arg(&elf)
            .arg(&functions_tsv)
            .arg(gen_dir.join("recomp.c"))
            .arg(format!("@{}", list_file.display())),
        "uu-bbrecomp",
    )?;
    let seconds_generate = seconds(generate_started);
    let main_line = generated
        .lines()
        .find(|line| line.contains("instructions:") && line.contains("in C"))
        .map(numbers)
        .unwrap_or_default();
    let line_count = |needle: &str| {
        generated
            .lines()
            .find(|line| line.contains(needle))
            .and_then(|line| numbers(line).first().copied())
            .unwrap_or(0)
    };

    // Compile every C file, then link.
    let compile_started = Instant::now();
    let mut sources: Vec<PathBuf> = fs::read_dir(&gen_dir)
        .map_err(|e| format!("{}: {e}", gen_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|e| e == "c"))
        .collect();
    sources.sort();
    let generated_c_bytes: u64 = sources
        .iter()
        .filter_map(|path| fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum();
    let includes = tools::include_dirs();
    let next = AtomicUsize::new(0);
    let failure: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..request.jobs.clamp(1, 64) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(source) = sources.get(index) else {
                    return;
                };
                if failure.lock().unwrap().is_some() {
                    return;
                }
                let object = obj_dir.join(format!(
                    "{}.o",
                    source.file_stem().unwrap().to_string_lossy()
                ));
                let result = run_tool(
                    Command::new("clang")
                        .args([
                            "-std=gnu11",
                            "-O2",
                            "-ffp-contract=off",
                            "-fno-strict-aliasing",
                        ])
                        .args([
                            "-fvisibility=hidden",
                            "-Wno-unused-label",
                            "-Wno-unused-variable",
                        ])
                        .args(["-DZYDIS_STATIC_BUILD", "-DZYCORE_STATIC_BUILD"])
                        .args(includes.iter().map(|dir| format!("-I{}", dir.display())))
                        .arg("-c")
                        .arg(source)
                        .arg("-o")
                        .arg(&object),
                    &format!("clang {}", source.file_name().unwrap().to_string_lossy()),
                );
                if let Err(message) = result {
                    failure.lock().unwrap().get_or_insert(message);
                }
            });
        }
    });
    if let Some(message) = failure.into_inner().unwrap() {
        return Err(message);
    }
    let library = out.join(format!("librecomp-{}.dylib", request.title));
    let mut objects: Vec<PathBuf> = fs::read_dir(&obj_dir)
        .map_err(|e| format!("{}: {e}", obj_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    objects.sort();
    run_tool(
        Command::new("clang")
            .arg("-dynamiclib")
            .args(&objects)
            .arg("-o")
            .arg(&library),
        "clang -dynamiclib",
    )?;
    let seconds_compile = seconds(compile_started);

    let summary = Summary {
        title: request.title.to_string(),
        functions_found: scan_numbers.first().copied().unwrap_or(0),
        functions_unwound: scan_numbers.get(1).copied().unwrap_or(0),
        import_stubs,
        functions_generated: main_line.first().copied().unwrap_or(0),
        instructions: main_line.get(1).copied().unwrap_or(0),
        instructions_in_c: main_line.get(2).copied().unwrap_or(0),
        setjmp_callers_left: line_count("call setjmp"),
        import_stubs_left: line_count("import stubs"),
        c_files: sources.len() as u64,
        generated_c_bytes,
        library_bytes: fs::metadata(&library).map(|m| m.len()).unwrap_or(0),
        seconds_scan,
        seconds_generate,
        seconds_compile,
        seconds_total: seconds(started),
    };
    write(out.join("summary.json"), summary.to_json().as_bytes())?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uncharted_core::link::ImportSlot;

    #[test]
    fn numbers_reads_the_integers_of_a_tool_line() {
        assert_eq!(
            numbers("21640 functions (21000 from the unwind tables), 9 instructions"),
            vec![21640, 21000, 9]
        );
        assert!(numbers("no digits").is_empty());
    }

    #[test]
    fn imports_tsv_names_small_stubs_through_known_slots_only() {
        let inputs = RecompInputs {
            has_dynamic: true,
            import_slots: vec![
                ImportSlot {
                    slot: 0x3000,
                    nid: nid("setjmp"),
                },
                ImportSlot {
                    slot: 0x3008,
                    nid: "UNKNOWNNID1".into(),
                },
            ],
            code_pointers: Vec::new(),
        };
        let functions = "address\tsize\tinstructions\tundecodable\n\
            0x1000\t6\t1\t0\n0x1010\t6\t1\t0\n0x1020\t40\t12\t0\n0x1100\t6\t1\t0\n";
        let slots =
            "caller\tslot\n0x1000\t0x3000\n0x1010\t0x3008\n0x1020\t0x3000\n0x1100\t0x3999\n";
        let (text, count) = imports_tsv(functions, slots, &inputs);
        assert_eq!(count, 2);
        assert_eq!(text, "stub\tname\n0x1000\tsetjmp\n0x1010\tUNKNOWNNID1\n");
    }

    #[test]
    fn the_summary_json_has_counts_and_seconds_only() {
        let summary = Summary {
            title: "u1".into(),
            functions_found: 7,
            ..Summary::default()
        };
        let json: serde_json::Value = serde_json::from_str(&summary.to_json()).unwrap();
        assert_eq!(json["functions_found"], 7);
        assert!(json["seconds"]["total"].is_number());
        let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
        assert!(keys
            .iter()
            .all(|k| !k.contains("offset") && !k.contains("hash") && !k.contains("name")));
    }
}
