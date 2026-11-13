//! The save dialog as `src/runtime_services.c` answered it: its status machine, the list's
//! choice (on the process's store, under `test_lock`, in a temporary user directory) and what
//! `GetResult` writes through the game's pointers. The guest structures are byte buffers at the
//! offsets the module doc lists.

use super::super::{ffi, set_user_dir, test_lock};
use super::*;
use std::fs;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::time::{Duration, UNIX_EPOCH};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("uu-dialog-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        set_user_dir(&path.to_string_lossy());
        // SAFETY: a NUL-terminated literal.
        unsafe { ffi::runtime_savedata_configure(c"DIALOGTEST".as_ptr()) };
        Self(path)
    }

    /// A save directory with a param (user param `user`) written at `seconds`.
    fn save(&self, name: &str, user: u32, seconds: u64) {
        let root = self.0.join("savedata/1/DIALOGTEST");
        fs::create_dir_all(root.join(name)).unwrap();
        let meta = root.join(format!("{name}.sce_sys"));
        fs::create_dir_all(&meta).unwrap();
        let mut param = Param::ZERO;
        param.user_param = user;
        fs::write(meta.join("param.bin"), param.to_bytes()).unwrap();
        let file = fs::File::options()
            .write(true)
            .open(meta.join("param.bin"))
            .unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        set_user_dir("");
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A fresh, initialized dialog.
fn fresh() {
    terminate();
    assert_eq!(terminate(), NOT_INITIALIZED);
    assert_eq!(initialize(), 0);
}

/// `OrbisSaveDataDialogParam` with `mode`, `dispType` and `userData`.
fn param(mode: u32, kind: u32, user_data: usize) -> [u8; 128] {
    let mut p = [0u8; 128];
    // SAFETY: within the buffer.
    unsafe {
        put(p.as_mut_ptr(), 52, mode);
        put(p.as_mut_ptr(), 56, kind);
        put(p.as_mut_ptr(), 112, user_data);
    }
    p
}

fn with_pointer(mut p: [u8; 128], offset: usize, to: *const u8) -> [u8; 128] {
    // SAFETY: within the buffer.
    unsafe { put(p.as_mut_ptr(), offset, to) };
    p
}

/// SaveDialogItems for user 1 and the configured title.
fn items(names: &[[u8; DIR_NAME]], new_item: bool, focus_pos: u32, focus: *const u8) -> [u8; 56] {
    let mut i = [0u8; 56];
    let p = i.as_mut_ptr();
    // SAFETY: within the buffer.
    unsafe {
        put(p, 0, 1i32);
        put(p, 16, names.as_ptr());
        put(p, 24, names.len() as u32);
        put(p, 32, if new_item { p.cast_const() } else { null() });
        put(p, 40, focus_pos);
        put(p, 48, focus);
    }
    i
}

fn dir(name: &str) -> [u8; DIR_NAME] {
    let mut field = [0u8; DIR_NAME];
    field[..name.len()].copy_from_slice(name.as_bytes());
    field
}

/// `GetResult` into a result whose dirName and param point at `dir` and `out` (when given):
/// its code, then mode, result, buttonId and userData.
fn result(dir: Option<&mut [u8; DIR_NAME]>, out: Option<&mut Param>) -> (i32, [u32; 3], usize) {
    let mut r = [0u8; 48];
    let p = r.as_mut_ptr();
    // SAFETY: within the buffer.
    unsafe {
        put(p, 16, dir.map_or(null_mut(), |d| d.as_mut_ptr()));
        put(
            p,
            24,
            out.map_or(null_mut(), |o| (o as *mut Param).cast::<u8>()),
        );
    }
    let code = get_result(p);
    // SAFETY: within the buffer.
    unsafe {
        (
            code,
            [at(p, 0), at(p, 4), at(p, 8)],
            at::<usize>(p.cast_const(), 32),
        )
    }
}

#[test]
fn dialog_messages_finish_at_once_and_waiting_dialogs_when_closed() {
    let _lock = test_lock();
    let _d = TempDir::new("messages");
    terminate();
    assert_eq!(open(null()), NOT_INITIALIZED);
    fresh();
    assert_eq!(initialize(), ALREADY_INITIALIZED);
    assert_eq!(open(null()), ARG_NULL);
    assert_eq!(ready(), 1);

    // A user message with an OK button is answered OK.
    let ok = 0u32;
    let message = with_pointer(param(USER_MSG, 0, 0x1234), 80, (&ok as *const u32).cast());
    assert_eq!(open(message.as_ptr()), 0);
    assert_eq!(get_result(null_mut()), ARG_NULL);
    assert_eq!(result(None, None).0, NOT_FINISHED);
    assert_eq!(status(), FINISHED);
    assert_eq!(result(None, None), (0, [USER_MSG, 0, 1], 0x1234));

    // A progress bar, a message without buttons and the "progress" system message wait.
    let none = 2u32;
    let progress_kind = 5u32;
    for p in [
        param(PROGRESS_BAR, 0, 7),
        with_pointer(param(USER_MSG, 0, 7), 80, (&none as *const u32).cast()),
        with_pointer(
            param(SYSTEM_MSG, 0, 7),
            88,
            (&progress_kind as *const u32).cast(),
        ),
    ] {
        assert_eq!(open(p.as_ptr()), 0);
        assert_eq!(status(), RUNNING);
        assert_eq!(status(), RUNNING);
        assert_eq!(result(None, None).0, NOT_FINISHED);
        assert_eq!(progress(0, 50), 0);
        assert_eq!(close(null()), 0);
        assert_eq!(status(), FINISHED);
        assert_eq!(result(None, None).0, 0);
    }
    // Other system messages, and messages without their param, are answered.
    let no_data = 1u32;
    let system = with_pointer(param(SYSTEM_MSG, 0, 0), 88, (&no_data as *const u32).cast());
    assert_eq!(open(system.as_ptr()), 0);
    assert_eq!(status(), FINISHED);
    assert_eq!(open(param(USER_MSG, 0, 0).as_ptr()), 0);
    assert_eq!(status(), FINISHED);
    // Close of a finished dialog keeps it finished.
    assert_eq!(close(null()), 0);
    assert_eq!(status(), FINISHED);
    assert_eq!(terminate(), 0);
    assert_eq!(status(), 0);
}

#[test]
fn dialog_list_takes_the_focused_entry() {
    let _lock = test_lock();
    let d = TempDir::new("list");
    d.save("SAVEDATA01P", 1, 1000);
    d.save("SAVEDATA01Q", 2, 3000);
    d.save("SAVEDATA01R", 3, 2000);
    fresh();
    let names = [
        dir("MISSING"),
        dir("SAVEDATA01P"),
        dir("SAVEDATA01Q"),
        dir("SAVEDATA01R"),
    ];
    let load = |focus_pos: u32, focus: *const u8, user_data: usize| {
        let i = items(&names, false, focus_pos, focus);
        let p = with_pointer(param(LIST, 2, user_data), 72, i.as_ptr());
        assert_eq!(open(p.as_ptr()), 0);
        assert_eq!(status(), FINISHED);
        let mut name = [0xAAu8; DIR_NAME];
        let mut out = Param::ZERO;
        let r = result(Some(&mut name), Some(&mut out));
        (r, name, out.user_param)
    };
    // focusPos: 0 and 2 the first directory, 1 and 3 the last, 4 the latest, 5 the oldest.
    for (focus_pos, chosen, user) in [
        (0, "SAVEDATA01P", 1),
        (1, "SAVEDATA01R", 3),
        (2, "SAVEDATA01P", 1),
        (3, "SAVEDATA01R", 3),
        (4, "SAVEDATA01Q", 2),
        (5, "SAVEDATA01P", 1),
        (9, "SAVEDATA01Q", 2),
    ] {
        let (r, name, got) = load(focus_pos, null(), 0xBEEF);
        assert_eq!(r, (0, [LIST, 0, 1], 0xBEEF), "focus {focus_pos}");
        assert_eq!(name, dir(chosen), "focus {focus_pos}");
        assert_eq!(got, user, "focus {focus_pos}");
    }
    // 6: the named directory.
    let focus = c"SAVEDATA01R";
    let (r, name, _) = load(6, focus.as_ptr().cast(), 0);
    assert_eq!((r.1, name), ([LIST, 0, 1], dir("SAVEDATA01R")));
    // Named but missing: nothing to choose, cancelled; the game's buffers stay.
    let (r, name, user) = load(6, c"MISSING".as_ptr().cast(), 0);
    assert_eq!(
        (r.0, r.1, name, user),
        (0, [LIST, 1, 0], [0xAA; DIR_NAME], 0)
    );
    // A save list offers the new entry first; empty, a save list chooses the new entry too.
    for (focus_pos, entries) in [(0, &names[..]), (4, &names[..1])] {
        let i = items(entries, true, focus_pos, null());
        let p = with_pointer(param(LIST, 1, 0), 72, i.as_ptr());
        assert_eq!(open(p.as_ptr()), 0);
        assert_eq!(status(), FINISHED);
        let mut name = [0xAAu8; DIR_NAME];
        let mut out = Param::ZERO;
        out.user_param = 77;
        let r = result(Some(&mut name), Some(&mut out));
        assert_eq!(r.1, [LIST, 0, 1]);
        assert_eq!(name, [0; DIR_NAME], "new save data: an empty name");
        assert_eq!(out.user_param, 77, "param left alone");
    }
    // A save list with an existing directory focused takes it.
    let i = items(&names, true, 4, null());
    let p = with_pointer(param(LIST, 1, 0), 72, i.as_ptr());
    assert_eq!(open(p.as_ptr()), 0);
    assert_eq!(status(), FINISHED);
    let mut name = [0u8; DIR_NAME];
    assert_eq!(result(Some(&mut name), None).1, [LIST, 0, 1]);
    assert_eq!(name, dir("SAVEDATA01Q"));
    // A delete list and a list without items: cancelled.
    let i = items(&names, true, 0, null());
    for p in [
        with_pointer(param(LIST, 3, 0), 72, i.as_ptr()),
        param(LIST, 2, 0),
    ] {
        assert_eq!(open(p.as_ptr()), 0);
        assert_eq!(status(), FINISHED);
        assert_eq!(result(None, None).1, [LIST, 1, 0]);
    }
    assert_eq!(terminate(), 0);
}
