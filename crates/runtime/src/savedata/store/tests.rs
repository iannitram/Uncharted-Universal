//! The save store against the C behaviour of the deleted `src/runtime_savedata.c` (its results
//! are the expected values here), the #38 failure paths, `where.json` (#168) and the alignment
//! with `crates/core` `saves` (the mapping and the save-relay manifest, 1.17). Each test has its
//! own store, a fake host and a temporary user directory; no game data.

use super::*;
use std::collections::HashMap;
use std::path::PathBuf;
use uncharted_core::games::Game;
use uncharted_core::saves;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("uu-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Fake {
    user: String,
    vars: Mutex<HashMap<String, String>>,
    lines: Mutex<Vec<String>>,
    mounted: Mutex<Vec<(String, String)>>,
    refuse_mounts: AtomicBool,
}

impl Host for Fake {
    fn user_dir(&self) -> String {
        self.user.clone()
    }
    fn mount(&self, guest: &str, host: &str) -> bool {
        if self.refuse_mounts.load(Ordering::Relaxed) {
            return false;
        }
        self.mounted
            .lock()
            .unwrap()
            .push((guest.to_owned(), host.to_owned()));
        true
    }
    fn unmount(&self, guest: &str) {
        self.mounted.lock().unwrap().retain(|(g, _)| g != guest);
    }
    fn var(&self, name: &str) -> Option<String> {
        self.vars.lock().unwrap().get(name).cloned()
    }
    fn say(&self, line: &str) {
        self.lines.lock().unwrap().push(line.to_owned());
    }
}

/// A store over `dir`, initialized.
fn store(dir: &TempDir) -> Store<Fake> {
    let s = Store::new(Fake {
        user: dir.path(),
        ..Fake::default()
    });
    assert_eq!(s.initialize(), 0);
    s
}

fn set_var(s: &Store<Fake>, name: &str, value: &str) {
    s.host
        .vars
        .lock()
        .unwrap()
        .insert(name.to_owned(), value.to_owned());
}

fn lines(s: &Store<Fake>) -> Vec<String> {
    std::mem::take(&mut *s.host.lines.lock().unwrap())
}

/// A guest directory name field.
fn dir(name: &str) -> [u8; DIR_NAME] {
    let mut field = [0u8; DIR_NAME];
    field[..name.len()].copy_from_slice(name.as_bytes());
    field
}

fn mount(s: &Store<Fake>, name: &str, mode: u32) -> Result<String, i32> {
    let mut result = MountResult::ZERO;
    match s.mount(1, None, Some(&dir(name)), mode, Some(&mut result)) {
        0 => Ok(String::from_utf8_lossy(c_text(&result.point)).into_owned()),
        r => Err(r),
    }
}

fn point(text: &str) -> [u8; 16] {
    let mut field = [0u8; 16];
    field[..text.len()].copy_from_slice(text.as_bytes());
    field
}

fn title_of(s: &Store<Fake>, at: &str, text: &str) -> i32 {
    s.set_param(
        Some(&point(at)),
        Some(|| ParamValue::Text(1, text.as_bytes().to_vec())),
    )
}

fn root(dir: &TempDir) -> String {
    format!("{}/savedata/1/UNKNOWN", dir.path())
}

#[test]
fn store_mount_creates_then_mounts_the_existing_directory() {
    let d = TempDir::new("mount");
    let s = store(&d);
    let mut result = MountResult::ZERO;
    result.required_blocks = 7;
    assert_eq!(
        s.mount(
            1,
            None,
            Some(&dir("SAVEDATA00")),
            MODE_CREATE,
            Some(&mut result)
        ),
        0
    );
    assert_eq!(c_text(&result.point), b"/savedata0");
    assert_eq!((result.status, result.required_blocks), (1, 0)); // CREATED, the rest zeroed
    let host = format!("{}/SAVEDATA00", root(&d));
    assert!(is_dir(&host));
    let param = fs::read(format!("{host}.sce_sys/param.bin")).unwrap();
    assert_eq!(param.len(), PARAM_SIZE);
    assert!(param[..1288].iter().all(|&b| b == 0));
    assert_eq!(
        *s.host.mounted.lock().unwrap(),
        [("/savedata0".to_owned(), host.clone())]
    );
    assert_eq!(
        lines(&s),
        ["Runtime: save data 'SAVEDATA00' mounted at /savedata0 (created)"]
    );
    // Created again: EXISTS; opened while mounted: BAD_MOUNTED.
    assert_eq!(mount(&s, "SAVEDATA00", MODE_CREATE), Err(ERR_EXISTS));
    assert_eq!(mount(&s, "SAVEDATA00", 0), Err(ERR_BAD_MOUNTED));
    assert_eq!(mount(&s, "SAVEDATA00", MODE_CREATE2), Err(ERR_BAD_MOUNTED));
    assert_eq!(s.umount(Some(&point("/savedata0"))), 0);
    assert!(s.host.mounted.lock().unwrap().is_empty());
    let mut result = MountResult::ZERO;
    assert_eq!(
        s.mount(
            1,
            None,
            Some(&dir("SAVEDATA00")),
            MODE_RDONLY,
            Some(&mut result)
        ),
        0
    );
    assert_eq!(result.status, 0);
    // CREATE2 opens an existing directory or creates one.
    assert_eq!(
        mount(&s, "SAVEDATA01", MODE_CREATE2).as_deref(),
        Ok("/savedata1")
    );
    assert_eq!(
        lines(&s),
        [
            "Runtime: save data 'SAVEDATA00' mounted at /savedata0 (existing, read-only)",
            "Runtime: save data 'SAVEDATA01' mounted at /savedata1 (created)",
        ]
    );
    assert_eq!(s.umount(Some(&point("/savedata1"))), 0);
    assert_eq!(
        mount(&s, "SAVEDATA01", MODE_CREATE2).as_deref(),
        Ok("/savedata1")
    );
    // A title of the request replaces the configured one.
    let mut result = MountResult::ZERO;
    assert_eq!(
        s.mount(
            2,
            Some("CUSA02320"),
            Some(&dir("X")),
            MODE_CREATE,
            Some(&mut result)
        ),
        0
    );
    assert!(is_dir(&format!("{}/savedata/2/CUSA02320/X", d.path())));
    s.report();
    assert_eq!(
        lines(&s).last().unwrap(),
        "Runtime: save data mounts=5, memory writes=0"
    );
}

#[test]
fn store_mount_checks_its_arguments_in_the_c_order() {
    let d = TempDir::new("mount-args");
    let s = Store::new(Fake {
        user: d.path(),
        ..Fake::default()
    });
    let mut result = MountResult::ZERO;
    assert_eq!(mount(&s, "A", MODE_CREATE), Err(ERR_NOT_INITIALIZED));
    assert_eq!(s.terminate(), ERR_NOT_INITIALIZED);
    s.initialize();
    assert_eq!(s.mount(-1, None, None, MODE_CREATE, None), ERR_INVALID_USER);
    assert_eq!(
        s.mount(0, None, None, MODE_CREATE, Some(&mut result)),
        ERR_PARAMETER
    );
    assert_eq!(
        s.mount(0, None, Some(&dir("A")), MODE_CREATE, None),
        ERR_PARAMETER
    );
    for bad in ["", ".A", "A..B", "A/B", "A\\B", "..", "."] {
        assert_eq!(mount(&s, bad, MODE_CREATE), Err(ERR_PARAMETER), "{bad:?}");
    }
    // 32 characters leave no NUL in the field.
    assert_eq!(mount(&s, &"A".repeat(32), MODE_CREATE), Err(ERR_PARAMETER));
    assert_eq!(
        mount(&s, &"A".repeat(31), MODE_CREATE).as_deref(),
        Ok("/savedata0")
    );
    assert_eq!(mount(&s, "A.B", MODE_CREATE).as_deref(), Ok("/savedata1"));
    assert_eq!(mount(&s, "MISSING", 0), Err(ERR_NOT_FOUND));
    assert_eq!(mount(&s, "MISSING", MODE_RDONLY), Err(ERR_NOT_FOUND));
    assert_eq!(s.terminate(), 0);
    assert_eq!(s.terminate(), ERR_NOT_INITIALIZED);
    assert_eq!(s.umount(Some(&point("/savedata0"))), ERR_NOT_INITIALIZED);
}

#[test]
fn store_has_sixteen_mount_points_and_needs_the_file_layer() {
    let d = TempDir::new("full");
    let s = store(&d);
    for i in 0..SLOTS {
        assert_eq!(
            mount(&s, &format!("D{i}"), MODE_CREATE),
            Ok(format!("/savedata{i}"))
        );
    }
    assert_eq!(mount(&s, "D16", MODE_CREATE), Err(ERR_MOUNT_FULL));
    assert!(!Path::new(&format!("{}/D16", root(&d))).exists());
    assert_eq!(s.umount(Some(&point("/savedata7"))), 0);
    s.host.refuse_mounts.store(true, Ordering::Relaxed);
    assert_eq!(mount(&s, "D16", MODE_CREATE), Err(ERR_MOUNT_FULL));
    s.host.refuse_mounts.store(false, Ordering::Relaxed);
    assert_eq!(mount(&s, "D16", MODE_CREATE).as_deref(), Ok("/savedata7"));
}

#[test]
fn store_umount_parses_the_point_as_c_did() {
    let d = TempDir::new("umount");
    let s = store(&d);
    assert_eq!(s.umount(None), ERR_NOT_FOUND);
    assert_eq!(s.umount(Some(&point("/savedata0"))), ERR_NOT_FOUND);
    mount(&s, "A", MODE_CREATE).unwrap();
    mount(&s, "B", MODE_CREATE).unwrap();
    for bad in ["/savedata16", "/savedata-1", "/data0", "savedata1", ""] {
        assert_eq!(s.umount(Some(&point(bad))), ERR_NOT_FOUND, "{bad:?}");
    }
    // C atoi: blanks, a sign and trailing text are accepted, no digits is slot 0 (#49 is open
    // for this; the port keeps it).
    assert_eq!(s.umount(Some(&point("/savedata+1x"))), 0);
    assert_eq!(s.umount(Some(&point("/savedataX"))), 0);
    assert_eq!(s.umount(Some(&point("/savedata0"))), ERR_NOT_FOUND);
    // The file layer is asked to unmount the text as given (as C did), so these stay there.
    assert_eq!(s.host.mounted.lock().unwrap().len(), 2);
    assert_eq!(atoi(b"  -12ab"), -12);
    assert_eq!(atoi(b"\t+7"), 7);
    assert_eq!(atoi(b"x1"), 0);
    assert_eq!(atoi(b"99999999999999999999999"), i64::MAX);
}

#[test]
fn store_save_log_prints_mounts_and_unmounts() {
    let d = TempDir::new("log");
    let s = store(&d);
    set_var(&s, "BB_SAVE_LOG", "1");
    mount(&s, "SAVEDATA00", MODE_CREATE).unwrap();
    assert_eq!(mount(&s, "OTHER", 0), Err(ERR_NOT_FOUND));
    s.mount(1, None, None, 0, None);
    s.umount(Some(&point("/savedata0")));
    s.umount(None);
    assert_eq!(
        lines(&s),
        [
            "Runtime: save data 'SAVEDATA00' mounted at /savedata0 (created)",
            "Runtime: SaveLog mount dir 'SAVEDATA00' mode 0x4 user 1 -> 0x00000000 point /savedata0",
            "Runtime: SaveLog mount dir 'OTHER' mode 0x0 user 1 -> 0x809f0008 FAILED",
            "Runtime: SaveLog mount dir '(null)' mode 0x0 user 1 -> 0x809f0000 FAILED",
            "Runtime: SaveLog umount '/savedata0' slot 0 -> 0x00000000",
            "Runtime: SaveLog umount '(null)' slot -1 -> 0x809f0008",
        ]
    );
    set_var(&s, "BB_SAVE_LOG", "0");
    mount(&s, "SAVEDATA00", 0).unwrap();
    assert_eq!(lines(&s).len(), 1);
}

#[test]
fn store_set_param_writes_each_type_as_c_did() {
    let d = TempDir::new("param");
    let s = store(&d);
    let at = Some(point("/savedata0"));
    let at = at.as_ref().map(|p| &p[..]);
    assert_eq!(
        s.set_param(at, Some(|| ParamValue::UserParam(1))),
        ERR_NOT_MOUNTED
    );
    mount(&s, "S", MODE_CREATE).unwrap();
    let meta = format!("{}/S.sce_sys", root(&d));
    assert_eq!(s.set_param(at, None::<fn() -> ParamValue>), ERR_PARAMETER);
    assert_eq!(title_of(&s, "/savedata0", "Hello"), 0);
    // snprintf: the shorter text and its NUL; the rest of the field stays.
    assert_eq!(title_of(&s, "/savedata0", "Hi"), 0);
    let p = read_param(&meta);
    assert_eq!(&p.title[..6], b"Hi\0lo\0");
    assert_eq!(
        s.set_param(at, Some(|| ParamValue::Text(2, b"Sub\0ignored".to_vec()))),
        0
    );
    assert_eq!(
        s.set_param(at, Some(|| ParamValue::Text(3, vec![b'd'; 2000]))),
        0
    );
    assert_eq!(s.set_param(at, Some(|| ParamValue::UserParam(0xABCD))), 0);
    let p = read_param(&meta);
    assert_eq!(c_text(&p.subtitle), b"Sub");
    assert_eq!(c_text(&p.detail), &[b'd'; 1023][..]);
    assert_eq!(p.user_param, 0xABCD);
    let now = unix_seconds(SystemTime::now());
    assert!((now - p.mtime).abs() < 5, "mtime is the file's time");
    let mut all = Param::ZERO;
    all.title[0] = b'T';
    all.user_param = 3;
    all.mtime = 1;
    assert_eq!(s.set_param(at, Some(|| ParamValue::All(Box::new(all)))), 0);
    let p = read_param(&meta);
    assert_eq!(
        (c_text(&p.title), c_text(&p.subtitle), p.user_param),
        (&b"T"[..], &b""[..], 3)
    );
    // param.bin as written: mtime is the time of writing, not the guest's.
    let raw: [u8; PARAM_SIZE] = fs::read(format!("{meta}/param.bin"))
        .unwrap()
        .try_into()
        .unwrap();
    assert!(Param::from_bytes(&raw).mtime > 1);
    assert_eq!(s.set_param(at, Some(|| ParamValue::Bad)), ERR_PARAMETER);
    assert_eq!(
        s.set_param(at, Some(|| ParamValue::Text(5, b"x".to_vec()))),
        ERR_PARAMETER
    );
    assert_eq!(read_param(&meta).user_param, 3);
    let leftovers: Vec<_> = fs::read_dir(&meta)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["param.bin"]);
}

#[test]
fn store_metadata_write_failures_are_reported_and_keep_the_old_file() {
    // #38: C ignored these and answered success.
    let d = TempDir::new("failures");
    let s = store(&d);
    mount(&s, "S", MODE_CREATE).unwrap();
    let meta = format!("{}/S.sce_sys", root(&d));
    assert_eq!(title_of(&s, "/savedata0", "Kept"), 0);
    let before = fs::read(format!("{meta}/param.bin")).unwrap();
    // The folder of param.bin is gone: the temporary file cannot be made.
    fs::rename(&meta, format!("{meta}.away")).unwrap();
    assert_eq!(title_of(&s, "/savedata0", "Lost"), ERR_INTERNAL);
    fs::rename(format!("{meta}.away"), &meta).unwrap();
    // param.bin cannot be replaced (a non-empty folder in its place).
    fs::rename(format!("{meta}/param.bin"), format!("{meta}/saved")).unwrap();
    fs::create_dir(format!("{meta}/param.bin")).unwrap();
    fs::write(format!("{meta}/param.bin/keep"), "made-up").unwrap();
    assert_eq!(title_of(&s, "/savedata0", "Lost"), ERR_INTERNAL);
    let names: Vec<_> = fs::read_dir(&meta)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "no temporary file left: {names:?}");
    assert_eq!(fs::read(format!("{meta}/saved")).unwrap(), before);
    // Creating a directory whose metadata cannot be made fails and leaves nothing mounted.
    fs::write(format!("{}/N.sce_sys", root(&d)), "a file in the way").unwrap();
    assert_eq!(mount(&s, "N", MODE_CREATE), Err(ERR_INTERNAL));
    assert!(!Path::new(&format!("{}/N", root(&d))).exists());
    assert_eq!(s.host.mounted.lock().unwrap().len(), 1);
    // SaveIcon into a folder that is gone.
    fs::remove_dir_all(&meta).unwrap();
    assert_eq!(
        s.save_icon(Some(&point("/savedata0")), Some(|| &b"png"[..])),
        ERR_INTERNAL
    );
}

#[test]
fn store_save_icon_and_delete() {
    let d = TempDir::new("icon");
    let s = store(&d);
    let at = point("/savedata0");
    assert_eq!(
        s.save_icon(Some(&at), None::<fn() -> &'static [u8]>),
        ERR_PARAMETER
    );
    assert_eq!(s.save_icon(Some(&at), Some(|| &b"x"[..])), ERR_NOT_MOUNTED);
    mount(&s, "S", MODE_CREATE).unwrap();
    assert_eq!(s.save_icon(Some(&at), Some(|| &b"made-up png"[..])), 0);
    let meta = format!("{}/S.sce_sys", root(&d));
    assert_eq!(
        fs::read(format!("{meta}/icon0.png")).unwrap(),
        b"made-up png"
    );
    fs::write(format!("{}/S/data", root(&d)), "made-up save").unwrap();
    lines(&s);
    assert_eq!(s.delete(1, None, None), ERR_PARAMETER);
    assert_eq!(s.delete(1, None, Some(&dir("../x"))), ERR_PARAMETER);
    assert_eq!(s.delete(1, None, Some(&dir("NONE"))), ERR_NOT_FOUND);
    assert_eq!(s.delete(1, None, Some(&dir("S"))), 0);
    assert!(!Path::new(&format!("{}/S", root(&d))).exists());
    assert!(!Path::new(&meta).exists());
    assert_eq!(lines(&s), ["Runtime: save data 'S' deleted"]);
    // A regular file of that name answers 0, removed or not (C: stat, not a directory check;
    // #51 is open for the result).
    fs::write(format!("{}/F", root(&d)), "made-up").unwrap();
    assert_eq!(s.delete(1, None, Some(&dir("F"))), 0);
    s.terminate();
    assert_eq!(s.delete(1, None, Some(&dir("S"))), ERR_NOT_INITIALIZED);
    assert_eq!(
        s.save_icon(Some(&at), Some(|| &b"x"[..])),
        ERR_NOT_INITIALIZED
    );
}

#[test]
fn store_like_matches_as_the_recursive_c_did() {
    fn reference(s: &[u8], p: &[u8]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some((b'%', rest)) => (0..=s.len()).any(|i| reference(&s[i..], rest)),
            Some((&c, rest)) => {
                !s.is_empty() && (c == b'_' || c == s[0]) && reference(&s[1..], rest)
            }
        }
    }
    let texts = [
        "",
        "A",
        "AB",
        "ABC",
        "SAVEDATA01P",
        "SAVEDATA02",
        "AAB",
        "BA",
    ];
    let patterns = [
        "",
        "%",
        "%%",
        "_",
        "__",
        "A%",
        "%B",
        "A_C",
        "%A%",
        "SAVEDATA01%",
        "SAVE%0_",
        "A%%B",
        "_%_",
        "%_",
        "B",
        "ABC",
        "ABCD",
        "%C%",
    ];
    for t in texts {
        for p in patterns {
            assert_eq!(
                like(t.as_bytes(), p.as_bytes()),
                reference(t.as_bytes(), p.as_bytes()),
                "{t:?} LIKE {p:?}"
            );
        }
    }
    // Bounded: the recursive C took exponential time on this (#57).
    let text = "A".repeat(31);
    let pattern = "%A".repeat(15) + "B";
    let started = std::time::Instant::now();
    assert!(!like(text.as_bytes(), pattern.as_bytes()));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn store_valid_name_follows_c() {
    assert_eq!(
        valid_name(b"SAVEDATA00\0junk", 32).as_deref(),
        Some("SAVEDATA00")
    );
    assert_eq!(valid_name(b"a.b.c", 32).as_deref(), Some("a.b.c"));
    assert_eq!(valid_name(b"a.", 32).as_deref(), Some("a."));
    for bad in [&b""[..], b"\0x", b".x", b"x..y", b"x/y", b"x\\y"] {
        assert_eq!(valid_name(bad, 32), None, "{bad:?}");
    }
    assert_eq!(valid_name(&[b'a'; 32], 32), None);
    assert_eq!(valid_name(&[b'a'; 32], 33).map(|n| n.len()), Some(32));
}

fn touch(path: &str, seconds: u64) {
    let file = File::options().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
        .unwrap();
}

/// Directories `names` in the root, each with a param (user param `i`, written at `1000 * i`).
fn saves(s: &Store<Fake>, d: &TempDir, names: &[&str]) {
    for (i, name) in names.iter().enumerate() {
        mount(s, name, MODE_CREATE).unwrap();
        let at = point(&format!("/savedata{i}"));
        let value = i as u32 + 1;
        assert_eq!(
            s.set_param(Some(&at), Some(|| ParamValue::UserParam(value))),
            0
        );
    }
    for (i, name) in names.iter().enumerate() {
        s.umount(Some(&point(&format!("/savedata{i}"))));
        touch(
            &format!("{}/{name}.sce_sys/param.bin", root(d)),
            1000 * (i as u64 + 1),
        );
    }
    lines(s);
}

fn names(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
}

#[test]
fn store_search_filters_sorts_and_reads_params() {
    let d = TempDir::new("search");
    let s = store(&d);
    assert_eq!(s.search(1, None, None, 0, 0), Ok(vec![]));
    saves(&s, &d, &["B", "C", "A"]);
    fs::write(format!("{}/.hidden", root(&d)), "").unwrap();
    fs::create_dir(format!("{}/{}", root(&d), "L".repeat(32))).unwrap();
    let all = s.search(1, None, None, 0, 0).unwrap();
    assert_eq!(names(&all), ["A", "B", "C"]);
    assert_eq!(
        all.iter()
            .map(|e| (e.param.user_param, e.param.mtime))
            .collect::<Vec<_>>(),
        [(3, 3000), (1, 1000), (2, 2000)]
    );
    assert_eq!(
        names(&s.search(1, None, None, 0, 1).unwrap()),
        ["C", "B", "A"]
    );
    assert_eq!(
        names(&s.search(1, None, None, 1, 0).unwrap()),
        ["B", "C", "A"]
    );
    assert_eq!(
        names(&s.search(1, None, None, 3, 1).unwrap()),
        ["A", "C", "B"]
    );
    assert_eq!(
        names(&s.search(1, None, Some(b"_\0"), 0, 0).unwrap()),
        ["A", "B", "C"]
    );
    assert_eq!(names(&s.search(1, None, Some(b"B"), 0, 0).unwrap()), ["B"]);
    assert_eq!(
        names(&s.search(1, None, Some(b"\0B"), 0, 0).unwrap()),
        ["A", "B", "C"]
    );
    assert_eq!(
        names(&s.search(1, None, Some(b"%%"), 0, 0).unwrap()).len(),
        3
    );
    assert_eq!(s.search(1, None, Some(b"Z%"), 0, 0), Ok(vec![]));
    assert_eq!(s.search(2, None, None, 0, 0), Ok(vec![]));
    // A regular file is listed with an empty param (C behaviour; #50 is open for this).
    fs::write(format!("{}/FILE", root(&d)), "made-up").unwrap();
    let with_file = s.search(1, None, None, 0, 0).unwrap();
    assert_eq!(names(&with_file), ["A", "B", "C", "FILE"]);
    assert_eq!(with_file[3].param, Param::ZERO);
    s.terminate();
    assert_eq!(s.search(1, None, None, 0, 0), Err(ERR_NOT_INITIALIZED));
}

#[test]
fn store_pick_chooses_as_the_save_dialog_asks() {
    let d = TempDir::new("pick");
    let s = store(&d);
    saves(&s, &d, &["OLD", "NEW", "MID"]);
    touch(&format!("{}/NEW.sce_sys/param.bin", root(&d)), 9000);
    let list = [
        dir("GONE"),
        dir("OLD"),
        dir(".bad"),
        dir("NEW"),
        dir("MID"),
        dir("GONE2"),
    ];
    let pick = |which, focus: Option<&[u8]>| s.pick(1, None, &list, which, focus).map(|(i, _)| i);
    assert_eq!(pick(0, None), Some(1));
    assert_eq!(pick(1, None), Some(4));
    assert_eq!(pick(2, None), Some(3));
    assert_eq!(pick(3, None), Some(1));
    assert_eq!(pick(4, Some(b"MID")), Some(4));
    assert_eq!(pick(4, Some(b"GONE")), None);
    assert_eq!(pick(4, None), None);
    assert_eq!(pick(9, Some(b"OLD\0x")), Some(1));
    let (_, param) = s.pick(1, None, &list, 2, None).unwrap();
    assert_eq!((param.user_param, param.mtime), (2, 9000));
    assert_eq!(s.pick(1, None, &[], 0, None), None);
    assert_eq!(s.pick(1, None, &[dir("GONE")], 1, None), None);
    // A 32-character name without a NUL is valid here (the C copied it into 33 bytes).
    let long = "N".repeat(32);
    fs::create_dir_all(format!("{}/{long}", root(&d))).unwrap();
    let mut field = [0u8; DIR_NAME];
    field.copy_from_slice(long.as_bytes());
    assert_eq!(s.pick(1, None, &[field], 0, None).map(|(i, _)| i), Some(0));
}

#[test]
fn store_save_data_memory() {
    let d = TempDir::new("memory");
    let s = store(&d);
    let io = |offset, data: &[u8], write| {
        let mut buffer = data.to_vec();
        let size = buffer.len() as u64;
        let r = s.memory_io(true, size, offset, write, |file| {
            if write {
                file.write_all(&buffer)
            } else {
                file.read_exact(&mut buffer)
            }
        });
        (r, buffer)
    };
    assert_eq!(io(0, b"x", false).0, ERR_MEMORY_NOT_READY);
    assert_eq!(s.memory_setup(1, 0, None), ERR_PARAMETER);
    let mut param = Param::ZERO;
    param.title[..4].copy_from_slice(b"Blob");
    assert_eq!(s.memory_setup(1, 64, Some(&param)), 0);
    let dir = format!("{}.memory", root(&d));
    assert_eq!(fs::metadata(format!("{dir}/memory.dat")).unwrap().len(), 64);
    assert_eq!(c_text(&read_param(&dir).title), b"Blob");
    assert_eq!(lines(&s), ["Runtime: save data memory ready (64 bytes)"]);
    assert_eq!(io(8, b"made-up", true).0, 0);
    assert_eq!(io(8, &[0; 7], false), (0, b"made-up".to_vec()));
    assert_eq!(io(60, b"12345", true).0, ERR_PARAMETER);
    assert_eq!(io(-1, b"1", true).0, ERR_PARAMETER);
    assert_eq!(io(64, b"", true).0, 0);
    assert_eq!(s.memory_io(false, 1, 0, true, |_| Ok(())), ERR_PARAMETER);
    assert_eq!(
        s.memory_io(true, u64::MAX, 1, true, |_| Ok(())),
        ERR_PARAMETER
    );
    // A smaller setup keeps the file and its contents; the size limits the calls.
    assert_eq!(s.memory_setup(1, 16, None), 0);
    assert_eq!(fs::metadata(format!("{dir}/memory.dat")).unwrap().len(), 64);
    assert_eq!(io(8, &[0; 7], false), (0, b"made-up".to_vec()));
    assert_eq!(io(16, b"x", false).0, ERR_PARAMETER);
    s.report();
    assert_eq!(
        lines(&s).last().unwrap(),
        "Runtime: save data mounts=0, memory writes=2"
    );
    fs::remove_file(format!("{dir}/memory.dat")).unwrap();
    assert_eq!(io(0, b"x", false).0, ERR_INTERNAL);
    s.terminate();
    assert_eq!(s.memory_setup(1, 8, None), ERR_NOT_INITIALIZED);
    assert_eq!(io(0, b"x", false).0, ERR_NOT_INITIALIZED);
}

#[test]
fn store_wait_idle_waits_for_mounts_that_write() {
    let d = TempDir::new("idle");
    let s = store(&d);
    assert_eq!(s.wait_idle(0), 0);
    mount(&s, "R", MODE_CREATE).unwrap();
    s.umount(Some(&point("/savedata0")));
    mount(&s, "R", MODE_RDONLY).unwrap();
    assert_eq!(s.wait_idle(100), 0);
    mount(&s, "W", MODE_CREATE).unwrap();
    assert_eq!(s.wait_idle(0), -1);
    assert_eq!(s.wait_idle(40), -1);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(60));
            assert_eq!(s.umount(Some(&point("/savedata1"))), 0);
        });
        assert_eq!(s.wait_idle(5000), 1);
    });
}

#[test]
fn store_configure_sets_the_title_and_the_bloodborne_flag() {
    let d = TempDir::new("configure");
    let s = store(&d);
    s.configure(b"");
    mount(&s, "A", MODE_CREATE).unwrap();
    assert!(is_dir(&format!("{}/A", root(&d))));
    s.configure(b"ABCDEFGHIJKLMNOPQRS");
    assert_eq!(s.title_id(), "ABCDEFGHIJKLMNO");
    s.configure(b"CUSA02320");
    mount(&s, "B", MODE_CREATE).unwrap();
    assert!(is_dir(&format!("{}/savedata/1/CUSA02320/B", d.path())));

    let flagged = |user: &str, title: &str, bytes: usize| {
        let folder = format!("{}/savedata/{user}/{title}/SPRJ0005", d.path());
        fs::create_dir_all(&folder).unwrap();
        let path = format!("{folder}/userdata0010");
        fs::write(&path, vec![0u8; bytes]).unwrap();
        path
    };
    let read = |path: &str| fs::read(path).unwrap().get(0x204E).copied();
    let first = flagged("1", "CUSA00207", 0x2050);
    let short = flagged("2", "CUSA00207", 0x204E);
    let other = flagged("1", "CUSA02320", 0x2050);
    lines(&s);
    set_var(&s, "BB_SOUND_HACK", "0");
    s.configure(b"CUSA00207");
    assert_eq!(read(&first), Some(0));
    assert!(lines(&s).is_empty());
    set_var(&s, "BB_SOUND_HACK", "1");
    s.configure(b"CUSA00207");
    assert_eq!(read(&first), Some(1));
    assert_eq!(read(&short), None);
    assert_eq!(read(&other), Some(0));
    assert_eq!(
        lines(&s),
        [format!(
            "Runtime: Bloodborne sound flag set in {first} (was 0)"
        )]
    );
    s.configure(b"CUSA00207");
    assert!(lines(&s).is_empty(), "set once");
}

fn where_json(folder: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(folder.join("where.json")).unwrap()).unwrap()
}

#[test]
fn store_unmounting_a_written_save_publishes_where_json_in_the_relay_folder() {
    let d = TempDir::new("where");
    let user = d.0.clone();
    saves::select(&user, Game::U1, 2).unwrap();
    let s = store(&d);
    set_var(&s, "BB_SAVEDATA_DIRS", &saves::mapping(&user));
    // The game's save directory goes to the selected slot, which save-relay syncs.
    mount(&s, "SAVEDATA01P", MODE_CREATE).unwrap();
    let slot = saves::relay_saves_dir(&user, Game::U1);
    assert!(slot.join("SAVEDATA01P").is_dir());
    for (kind, text) in [
        (1, "Made-up chapter"),
        (2, "Made-up \"sub\""),
        (3, "Made-up detail"),
    ] {
        let value = move || ParamValue::Text(kind, text.as_bytes().to_vec());
        assert_eq!(s.set_param(Some(&point("/savedata0")), Some(value)), 0);
    }
    fs::write(slot.join("SAVEDATA01P/data"), "made-up save").unwrap();
    assert_eq!(s.umount(Some(&point("/savedata0"))), 0);
    let got = where_json(&slot);
    assert_eq!(
        (
            &got["game"],
            &got["title"],
            &got["subtitle"],
            &got["detail"]
        ),
        (
            &"u1".into(),
            &"Made-up chapter".into(),
            &"Made-up \"sub\"".into(),
            &"Made-up detail".into()
        )
    );
    // Everything the store wrote in the slot travels with save-relay except where.json.
    let mut written = Vec::new();
    let mut stack = vec![slot.clone()];
    while let Some(folder) = stack.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(&slot).unwrap().to_string_lossy();
                written.push(rel.replace('\\', "/"));
            }
        }
    }
    written.sort();
    assert_eq!(
        written,
        [
            "SAVEDATA01P.sce_sys/param.bin",
            "SAVEDATA01P/data",
            "where.json"
        ]
    );
    for rel in &written {
        assert_eq!(
            saves::is_synced(Game::U1, rel),
            rel != "where.json",
            "{rel}"
        );
    }
    assert!(!saves::is_synced(
        Game::U1,
        "SAVEDATA01P.sce_sys/.param.bin.1.0.tmp"
    ));
    assert!(saves::relay_manifest(Game::U1).contains("\"exclude\":[\"**/*.tmp\"]"));

    // A read-only mount, and a directory of no game, publish nothing.
    let before = fs::read(slot.join("where.json")).unwrap();
    mount(&s, "SAVEDATA01P", MODE_RDONLY).unwrap();
    assert_eq!(s.umount(Some(&point("/savedata0"))), 0);
    mount(&s, "OTHER", MODE_CREATE).unwrap();
    assert_eq!(s.umount(Some(&point("/savedata0"))), 0);
    assert_eq!(fs::read(slot.join("where.json")).unwrap(), before);
    assert!(!Path::new(&format!("{}/where.json", root(&d))).exists());

    // Uncharted 2 in slot 1, Uncharted 3 without a mapping: its slot-1 folder.
    mount(&s, "SAVEDATA02", MODE_CREATE).unwrap();
    assert_eq!(s.umount(Some(&point("/savedata0"))), 0);
    assert_eq!(where_json(&user.join("Uncharted 2"))["game"], "u2");
    set_var(&s, "BB_SAVEDATA_DIRS", "");
    mount(&s, "SAVEDATA03X", MODE_CREATE).unwrap();
    assert_eq!(s.umount(Some(&point("/savedata0"))), 0);
    assert!(Path::new(&format!("{}/SAVEDATA03X", root(&d))).is_dir());
    assert_eq!(where_json(&user.join("Uncharted 3"))["game"], "u3");
    assert!(lines(&s).iter().all(|l| !l.contains("where.json")));
}

#[test]
fn store_where_json_failure_is_printed_and_the_unmount_succeeds() {
    let d = TempDir::new("where-fail");
    let s = store(&d);
    set_var(&s, "BB_SAVEDATA_DIRS", "SAVEDATA01=Uncharted 1");
    mount(&s, "SAVEDATA01", MODE_CREATE).unwrap();
    let folder = d.0.join("Uncharted 1");
    fs::create_dir_all(folder.join("where.json")).unwrap();
    fs::write(folder.join("where.json/keep"), "made-up").unwrap();
    lines(&s);
    assert_eq!(s.umount(Some(&point("/savedata0"))), 0);
    let printed = lines(&s);
    assert_eq!(printed.len(), 1);
    assert!(
        printed[0].starts_with(&format!(
            "Runtime: where.json not written in {}/Uncharted 1: ",
            d.path()
        )),
        "{printed:?}"
    );
    assert!(folder.join("where.json/keep").exists());
}

#[test]
fn store_mapping_follows_bb_savedata_dirs_as_c_did() {
    let d = TempDir::new("mapping");
    let s = store(&d);
    let user = d.path();
    set_var(&s, "BB_SAVEDATA_DIRS", "A=One;bad;;=All;B=Two=x;");
    assert_eq!(s.mapped_folder(Some("A1"), 0), Some(format!("{user}/One")));
    // The first match wins; an empty prefix matches every name.
    assert_eq!(s.mapped_folder(Some("Z"), 0), Some(format!("{user}/All")));
    // Entry numbers count the entries without `=` too.
    assert_eq!(s.mapped_folder(None, 0), Some(format!("{user}/One")));
    assert_eq!(s.mapped_folder(None, 1), None);
    assert_eq!(s.mapped_folder(None, 3), Some(format!("{user}/All")));
    assert_eq!(s.mapped_folder(None, 4), Some(format!("{user}/Two=x")));
    assert_eq!(s.mapped_folder(None, 5), None);

    // The search lists each mapped folder, every directory once, where it belongs; it stops at
    // the first entry number without a folder (as C did).
    set_var(&s, "BB_SAVEDATA_DIRS", &saves::mapping(&d.0));
    for name in ["SAVEDATA01P", "SAVEDATA02", "SAVEDATA03Q", "MENU"] {
        mount(&s, name, MODE_CREATE).unwrap();
        s.umount(Some(&point("/savedata0")));
    }
    // Stray copies in the title's folder belong to a mapped folder and are not listed.
    fs::create_dir_all(format!("{}/SAVEDATA01P", root(&d))).unwrap();
    assert_eq!(
        names(&s.search(1, None, None, 0, 0).unwrap()),
        ["MENU", "SAVEDATA01P", "SAVEDATA02", "SAVEDATA03Q"]
    );
    assert_eq!(
        names(&s.search(1, None, Some(b"SAVEDATA0_"), 0, 0).unwrap()),
        ["SAVEDATA02"]
    );
    assert!(d.0.join("Uncharted 3/SAVEDATA03Q").is_dir());
    // A dialog's pick and a delete look in the same folders.
    assert_eq!(
        s.pick(1, None, &[dir("SAVEDATA03Q")], 0, None)
            .map(|(i, _)| i),
        Some(0)
    );
    assert_eq!(s.delete(1, None, Some(&dir("SAVEDATA03Q"))), 0);
    assert!(!d.0.join("Uncharted 3/SAVEDATA03Q").exists());
}
