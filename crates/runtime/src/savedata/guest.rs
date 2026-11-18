//! The guest-facing side of savedata: the global [`Store`], the exported C functions of the
//! deleted `src/runtime_savedata.c` (`runtime_savedata_configure/resolve/report/wait_idle/pick`)
//! and, on x86-64, the `sceSaveData*` calls themselves. Guest pointers are read here and
//! nowhere else; the logic is in [`super::store`]. Guest code calls these with the System V ABI.
//! The `SaveDataDialog` family is [`crate::services`]'s.

use super::store::{bloodborne_sound_hack, Commit, Store, PARAM_SIZE};
use std::ffi::{c_char, c_int, c_uint, c_void, CStr};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

static STORE: Mutex<Store> = Mutex::new(Store::new());

/// The tests' user folder and mounts: they must not touch the file layer's global table.
#[cfg(test)]
static TEST_USER: Mutex<Option<String>> = Mutex::new(None);

/// The store with the user directory and `BB_SAVEDATA_DIRS` as they are now.
fn store() -> MutexGuard<'static, Store> {
    let mut store = STORE.lock().unwrap_or_else(PoisonError::into_inner);
    store.user = crate::file::user_dir().to_string_lossy().into_owned();
    #[cfg(test)]
    if let Some(user) = TEST_USER
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    {
        store.user = user;
    }
    store.spec = std::env::var("BB_SAVEDATA_DIRS").ok();
    store
}

/// A line on the loader's stdout (C stdio, in order with the C modules' lines).
fn out_line(text: &str) {
    #[cfg(target_arch = "x86_64")]
    crate::abi::host::out_line(text);
    #[cfg(not(target_arch = "x86_64"))]
    println!("{text}");
}

/// A line on stderr.
fn err_line(text: &str) {
    #[cfg(target_arch = "x86_64")]
    crate::abi::host::err(&format!("{text}\n"));
    #[cfg(not(target_arch = "x86_64"))]
    eprintln!("{text}");
}

/// `BB_SAVE_LOG=1` prints every mount and umount with its arguments and result code.
fn save_log() -> bool {
    std::env::var_os("BB_SAVE_LOG").is_some_and(|v| v.as_encoded_bytes().first() == Some(&b'1'))
}

/// Publishes `where.json` for a closed save. A failure is never ignored: it is on stderr (the
/// folder and the error, never the save's text), counted and in the report line (#38).
pub(super) fn publish(commit: &Commit) {
    if let Err(e) = commit.publish() {
        store().where_failures += 1;
        err_line(&format!(
            "Runtime: where.json not written in {}: {e}",
            commit.saves
        ));
    }
}

/// `runtime_savedata_configure`: the title id of the running game (its save folder name), then
/// the Bloodborne sound flag.
///
/// # Safety
/// `title` is null or a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn runtime_savedata_configure(title: *const c_char) {
    let mut store = store();
    if !title.is_null() {
        // SAFETY: non-null, and the caller guarantees a NUL-terminated string.
        let bytes = unsafe { CStr::from_ptr(title) }.to_bytes();
        if !bytes.is_empty() {
            let id = String::from_utf8_lossy(&bytes[..bytes.len().min(15)]).into_owned();
            store.title_id = id;
        }
    }
    let disabled = std::env::var_os("BB_SOUND_HACK")
        .is_some_and(|v| v.as_encoded_bytes().first() == Some(&b'0'));
    for line in bloodborne_sound_hack(&store, disabled) {
        out_line(&line);
    }
}

/// The module's counters, one line on stdout (and the `where.json` failures, when there were any).
#[no_mangle]
pub extern "C" fn runtime_savedata_report() {
    let store = store();
    out_line(&format!(
        "Runtime: save data mounts={}, memory writes={}",
        store.mounts_done, store.memory_writes
    ));
    if store.where_failures != 0 {
        out_line(&format!(
            "Runtime: where.json write failures={}",
            store.where_failures
        ));
    }
}

/// A save is in progress while a directory is mounted for writing (the game writes its files
/// between mount and unmount). Waits up to `timeout_ms` for none: 0 nothing was in progress,
/// 1 it finished, -1 still in progress at the timeout.
#[no_mangle]
pub extern "C" fn runtime_savedata_wait_idle(timeout_ms: c_uint) -> c_int {
    let mut waited_ms = 0;
    loop {
        if store().writing() == 0 {
            return c_int::from(waited_ms != 0);
        }
        if waited_ms >= timeout_ms {
            return -1;
        }
        std::thread::sleep(Duration::from_millis(20));
        waited_ms += 20;
    }
}

/// An existing slot of a save list: `which` 0 first, 1 last, 2 latest written, 3 oldest, 4 the
/// one named `focus`. 1 with the slot's name in `out` and its parameters in `param_out` (when
/// not null), or -1 when there is none.
///
/// # Safety
/// `names` is null or `count` names of 32 bytes; `title` and `focus` are null or NUL-terminated
/// strings; `out` is 32 writable bytes and `param_out` null or 1328 writable bytes.
#[no_mangle]
pub unsafe extern "C" fn runtime_savedata_pick(
    user: i32,
    title: *const c_char,
    names: *const [u8; 32],
    count: u32,
    which: c_int,
    focus: *const c_char,
    out: *mut u8,
    param_out: *mut c_void,
) -> c_int {
    if names.is_null() || out.is_null() {
        return -1;
    }
    // SAFETY: the caller passes `count` names at `names`.
    let names = unsafe { std::slice::from_raw_parts(names, count as usize) };
    // SAFETY: the caller passes null or NUL-terminated strings.
    let (title, focus) = unsafe { (c_string(title), c_bytes(focus)) };
    let store = store();
    let Some((name, param)) = store.pick(user, title.as_deref(), names, which, focus) else {
        return -1;
    };
    // SAFETY: `out` is 32 and `param_out` 1328 writable bytes (caller).
    unsafe {
        std::ptr::copy_nonoverlapping(name.as_ptr(), out, 32);
        if !param_out.is_null() {
            std::ptr::copy_nonoverlapping(param.0.as_ptr(), param_out.cast(), PARAM_SIZE);
        }
    }
    1
}

/// # Safety
/// `text` is null or a NUL-terminated string valid for the call.
unsafe fn c_bytes<'a>(text: *const c_char) -> Option<&'a [u8]> {
    // SAFETY: forwarded from the caller.
    (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_bytes())
}

/// A title id: null and empty are "the running game's".
///
/// # Safety
/// As [`c_bytes`].
unsafe fn c_string(text: *const c_char) -> Option<String> {
    // SAFETY: forwarded from the caller.
    unsafe { c_bytes(text) }
        .filter(|b| !b.is_empty())
        .map(|b| String::from_utf8_lossy(b).into_owned())
}

/// The address of the guest entry point for the import `name` (`NID#library#module`), 0 when it
/// is not one of this library's.
///
/// # Safety
/// `name` is null or a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn runtime_savedata_resolve(name: *const c_char) -> usize {
    // SAFETY: forwarded from the caller.
    let Some(name) = (unsafe { c_bytes(name) }) else {
        return 0;
    };
    std::str::from_utf8(name)
        .ok()
        .and_then(crate::abi::nid_name)
        .and_then(export)
        .unwrap_or(0)
}

/// The entry point for a library symbol name.
pub(super) fn export(symbol: &str) -> Option<usize> {
    #[cfg(target_arch = "x86_64")]
    {
        abi::lookup(symbol)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = symbol;
        None
    }
}

#[cfg(target_arch = "x86_64")]
pub(super) mod abi {
    use super::{out_line, publish, save_log, store};
    use crate::savedata::store::{
        Param, Search, ERR_NOT_INITIALIZED, ERR_NOT_MOUNTED, ERR_PARAMETER, PARAM_SIZE,
    };

    /// A field of a guest struct at byte `at`.
    ///
    /// # Safety
    /// `base + at` is readable for `size_of::<T>()` bytes.
    unsafe fn field<T>(base: *const u8, at: usize) -> T {
        // SAFETY: the caller's contract; guest structs are packed by the ABI, so unaligned.
        unsafe { base.add(at).cast::<T>().read_unaligned() }
    }

    /// `char name[32]` behind a guest pointer.
    ///
    /// # Safety
    /// Null or 32 readable bytes.
    unsafe fn dir_name(p: *const u8) -> Option<[u8; 32]> {
        // SAFETY: forwarded from the caller.
        (!p.is_null()).then(|| unsafe { p.cast::<[u8; 32]>().read_unaligned() })
    }

    /// The text of a title id (`char[16]`): null or empty is the running game's.
    ///
    /// # Safety
    /// Null or a readable NUL-terminated string of at most 16 bytes.
    unsafe fn title_of(p: *const u8) -> Option<String> {
        if p.is_null() {
            return None;
        }
        let mut bytes = Vec::new();
        for i in 0..16 {
            // SAFETY: the caller's contract; reading stops at the NUL.
            let b: u8 = unsafe { field(p, i) };
            if b == 0 {
                break;
            }
            bytes.push(b);
        }
        (!bytes.is_empty()).then(|| String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Up to `max` bytes at `p`, ending at a NUL (not included).
    ///
    /// # Safety
    /// Readable up to the NUL or `max` bytes.
    unsafe fn bytes_to_nul(p: *const u8, max: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for i in 0..max {
            // SAFETY: the caller's contract.
            let b: u8 = unsafe { field(p, i) };
            if b == 0 {
                break;
            }
            bytes.push(b);
        }
        bytes
    }

    /// The text of a `char[16]` mount point (`None` for null).
    ///
    /// # Safety
    /// Null or 16 readable bytes.
    unsafe fn mount_point(p: *const u8) -> Option<[u8; 16]> {
        // SAFETY: forwarded from the caller.
        (!p.is_null()).then(|| unsafe { p.cast::<[u8; 16]>().read_unaligned() })
    }

    unsafe extern "sysv64" fn save_initialize(_param: *const u8) -> i32 {
        store().initialize()
    }

    unsafe extern "sysv64" fn save_terminate() -> i32 {
        store().terminate()
    }

    /// `OrbisSaveDataMountResult`: point[16], required_blocks, unused, status (CREATED 1).
    ///
    /// # Safety
    /// `result` is null or 64 writable bytes; `dir` null or 32 readable.
    unsafe fn mount(
        user: i32,
        title: Option<String>,
        dir: *const u8,
        mode: u32,
        result: *mut u8,
    ) -> i32 {
        // SAFETY: forwarded from the caller.
        let name = unsafe { dir_name(dir) };
        let outcome = store().mount(
            user,
            title.as_deref(),
            name.as_ref(),
            mode,
            !result.is_null(),
        );
        let code = match &outcome {
            Ok((point, created)) => {
                // SAFETY: `result` is non-null (the store checked) and 64 writable bytes.
                unsafe {
                    std::ptr::write_bytes(result, 0, 64);
                    std::ptr::copy_nonoverlapping(point.as_ptr(), result, point.len());
                    result
                        .add(28)
                        .cast::<u32>()
                        .write_unaligned(u32::from(*created));
                }
                let shown = name.map(|n| text(&n)).unwrap_or_default();
                out_line(&format!(
                    "Runtime: save data '{shown}' mounted at {point} ({}{})",
                    if *created { "created" } else { "existing" },
                    if mode & 1 != 0 { ", read-only" } else { "" }
                ));
                0
            }
            Err(code) => *code,
        };
        if save_log() {
            let shown = name.map_or_else(|| "(null)".to_owned(), |n| text(&n));
            out_line(&format!(
                "Runtime: SaveLog mount dir '{shown}' mode 0x{mode:x} user {user} -> 0x{:08x}{}",
                code as u32,
                match &outcome {
                    Ok((point, _)) => format!(" point {point}"),
                    Err(_) => " FAILED".to_owned(),
                }
            ));
        }
        code
    }

    fn text(name: &[u8]) -> String {
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        String::from_utf8_lossy(&name[..end]).into_owned()
    }

    /// `OrbisSaveDataMount`: user@0, title*@8, dir*@16, fingerprint*@24, blocks@32, mode@40.
    unsafe extern "sysv64" fn save_mount(m: *const u8, result: *mut u8) -> i32 {
        if m.is_null() {
            return ERR_PARAMETER;
        }
        // SAFETY: the guest's 80-byte mount struct.
        unsafe {
            mount(
                field(m, 0),
                title_of(field(m, 8)),
                field(m, 16),
                field(m, 40),
                result,
            )
        }
    }

    /// `OrbisSaveDataMount2`: user@0, dir*@8, blocks@16, mode@24.
    unsafe extern "sysv64" fn save_mount2(m: *const u8, result: *mut u8) -> i32 {
        if m.is_null() {
            return ERR_PARAMETER;
        }
        // SAFETY: the guest's 64-byte mount struct.
        unsafe { mount(field(m, 0), None, field(m, 8), field(m, 24), result) }
    }

    unsafe extern "sysv64" fn save_umount(point: *const u8) -> i32 {
        // SAFETY: the guest's mount point, null or 16 bytes.
        let name = unsafe { mount_point(point) };
        let outcome = store().umount(name.as_ref().map(<[u8; 16]>::as_slice));
        if save_log() {
            let shown = name.map_or_else(|| "(null)".to_owned(), |n| text(&n));
            out_line(&format!(
                "Runtime: SaveLog umount '{shown}' slot {} -> 0x{:08x}",
                outcome.slot.map_or(-1, |s| s as i64),
                outcome.code as u32
            ));
        }
        if let Some(commit) = &outcome.commit {
            publish(commit);
        }
        outcome.code
    }

    /// `kind`: 0 all, 1 title, 2 subtitle, 3 detail, 4 user param.
    unsafe extern "sysv64" fn save_set_param(
        point: *const u8,
        kind: u32,
        buffer: *const u8,
        size: u64,
    ) -> i32 {
        let mut store = store();
        if !store.initialized {
            return ERR_NOT_INITIALIZED;
        }
        if buffer.is_null() {
            return ERR_PARAMETER;
        }
        // SAFETY: the guest's buffer holds `size` bytes (text: up to its NUL).
        let data: Vec<u8> = unsafe {
            match kind {
                0 if size >= PARAM_SIZE as u64 => {
                    std::slice::from_raw_parts(buffer, PARAM_SIZE).to_vec()
                }
                1..=3 => bytes_to_nul(buffer, size.min(1024) as usize),
                4 if size >= 4 => std::slice::from_raw_parts(buffer, 4).to_vec(),
                _ => Vec::new(),
            }
        };
        // SAFETY: the guest's mount point, null or 16 bytes.
        let point = unsafe { mount_point(point) }.unwrap_or_default();
        store.set_param(&point, kind, &data)
    }

    unsafe extern "sysv64" fn save_icon(point: *const u8, icon: *const u8) -> i32 {
        let mut store = store();
        if !store.initialized {
            return ERR_NOT_INITIALIZED;
        }
        // SAFETY: the guest's icon struct: buffer*@0, buffer_size@8, data_size@16.
        let buffer: *const u8 = if icon.is_null() {
            std::ptr::null()
        } else {
            unsafe { field(icon, 0) }
        };
        if buffer.is_null() {
            return ERR_PARAMETER;
        }
        // SAFETY: as above, and the mount point is null or 16 bytes.
        let (size, point) = unsafe { (field::<u64>(icon, 16), mount_point(point)) };
        let point = point.unwrap_or_default();
        if !store.mounted(&point) {
            return ERR_NOT_MOUNTED;
        }
        // SAFETY: the guest's icon buffer holds `data_size` bytes.
        let data = unsafe { std::slice::from_raw_parts(buffer, size as usize) };
        store.save_icon(&point, data)
    }

    /// `OrbisSaveDataDelete`: user@0, title*@8, dir*@16.
    unsafe extern "sysv64" fn save_delete(d: *const u8) -> i32 {
        let store = store();
        if !store.initialized {
            return ERR_NOT_INITIALIZED;
        }
        if d.is_null() {
            return ERR_PARAMETER;
        }
        // SAFETY: the guest's delete struct.
        let (user, title, dir) = unsafe {
            (
                field::<i32>(d, 0),
                title_of(field(d, 8)),
                dir_name(field(d, 16)),
            )
        };
        let code = store.delete(user, title.as_deref(), dir.as_ref());
        if code == 0 {
            if let Some(dir) = dir {
                out_line(&format!("Runtime: save data '{}' deleted", text(&dir)));
            }
        }
        code
    }

    /// `OrbisSaveDataDirNameSearchCond`: user@0, title*@8, dir*@16, key@24, order@28. Result:
    /// hits@0, names*@8, names_capacity@16, set_count@20, params*@24, infos*@32.
    unsafe extern "sysv64" fn save_search(cond: *const u8, result: *mut u8) -> i32 {
        let store = store();
        if !store.initialized {
            return ERR_NOT_INITIALIZED;
        }
        if cond.is_null() || result.is_null() {
            return ERR_PARAMETER;
        }
        // SAFETY: the guest's condition and result structs.
        let (user, title, dir, key, order) = unsafe {
            (
                field::<i32>(cond, 0),
                title_of(field(cond, 8)),
                field::<*const u8>(cond, 16),
                field::<u32>(cond, 24),
                field::<u32>(cond, 28),
            )
        };
        // SAFETY: the pattern is a `char[32]` or null.
        let pattern = if dir.is_null() {
            Vec::new()
        } else {
            unsafe { bytes_to_nul(dir, 32) }
        };
        let entries = store.search(&Search {
            user,
            title: title.as_deref(),
            pattern: &pattern,
            key,
            order,
        });
        // SAFETY: the result struct and the arrays it points to hold `names_capacity` entries.
        unsafe {
            let capacity: u32 = field(result, 16);
            let (names, params, infos): (*mut u8, *mut u8, *mut u8) =
                (field(result, 8), field(result, 24), field(result, 32));
            result.cast::<u32>().write_unaligned(entries.len() as u32);
            let set = entries.len().min(capacity as usize);
            for (i, entry) in entries.iter().take(set).enumerate() {
                if !names.is_null() {
                    std::ptr::copy_nonoverlapping(entry.name.as_ptr(), names.add(i * 32), 32);
                }
                if !params.is_null() {
                    std::ptr::copy_nonoverlapping(
                        entry.param.0.as_ptr(),
                        params.add(i * PARAM_SIZE),
                        PARAM_SIZE,
                    );
                }
                if !infos.is_null() {
                    let info = infos.add(i * 48);
                    std::ptr::write_bytes(info, 0, 48);
                    info.cast::<u64>().write_unaligned(32768);
                    info.add(8).cast::<u64>().write_unaligned(16384);
                }
            }
            result.add(20).cast::<u32>().write_unaligned(set as u32);
        }
        0
    }

    unsafe extern "sysv64" fn memory_setup(user: i32, size: u64, param: *const u8) -> i32 {
        // SAFETY: the guest's param is null or a whole `OrbisSaveDataParam`.
        let param = (!param.is_null()).then(|| unsafe {
            let mut p = Param::zero();
            std::ptr::copy_nonoverlapping(param, p.0.as_mut_ptr(), PARAM_SIZE);
            p
        });
        let code = store().memory_setup(user, size, param.as_ref());
        if code == 0 {
            out_line(&format!("Runtime: save data memory ready ({size} bytes)"));
        }
        code
    }

    unsafe extern "sysv64" fn memory_get(
        _user: i32,
        buffer: *mut u8,
        size: u64,
        offset: i64,
    ) -> i32 {
        let mut store = store();
        if !store.initialized {
            return ERR_NOT_INITIALIZED;
        }
        if buffer.is_null() || offset < 0 {
            return ERR_PARAMETER;
        }
        let code = store.memory_check(offset, size);
        if code != 0 {
            return code;
        }
        // SAFETY: the guest's buffer holds `size` bytes, and `size` is within the memory.
        let out = unsafe { std::slice::from_raw_parts_mut(buffer, size as usize) };
        store.memory_io(offset, Some(out), &[])
    }

    unsafe extern "sysv64" fn memory_set(
        _user: i32,
        buffer: *mut u8,
        size: u64,
        offset: i64,
    ) -> i32 {
        let mut store = store();
        if !store.initialized {
            return ERR_NOT_INITIALIZED;
        }
        if buffer.is_null() || offset < 0 {
            return ERR_PARAMETER;
        }
        let code = store.memory_check(offset, size);
        if code != 0 {
            return code;
        }
        // SAFETY: the guest's buffer holds `size` bytes, and `size` is within the memory.
        let data = unsafe { std::slice::from_raw_parts(buffer, size as usize) };
        store.memory_io(offset, None, data)
    }

    /// The function behind exported `symbol`: the table of `src/runtime_savedata.c`.
    pub(in crate::savedata) fn lookup(symbol: &str) -> Option<usize> {
        Some(match symbol {
            "sceSaveDataInitialize" | "sceSaveDataInitialize2" | "sceSaveDataInitialize3" => {
                save_initialize as *const () as usize
            }
            "sceSaveDataTerminate" => save_terminate as *const () as usize,
            "sceSaveDataMount" => save_mount as *const () as usize,
            "sceSaveDataMount2" => save_mount2 as *const () as usize,
            "sceSaveDataUmount" => save_umount as *const () as usize,
            "sceSaveDataSetParam" => save_set_param as *const () as usize,
            "sceSaveDataSaveIcon" => save_icon as *const () as usize,
            "sceSaveDataDelete" => save_delete as *const () as usize,
            "sceSaveDataDirNameSearch" => save_search as *const () as usize,
            "sceSaveDataSetupSaveDataMemory" => memory_setup as *const () as usize,
            "sceSaveDataGetSaveDataMemory" => memory_get as *const () as usize,
            "sceSaveDataSetSaveDataMemory" => memory_set as *const () as usize,
            _ => return None,
        })
    }
}

/// The guest ABI through the resolver, as the guest calls it: raw structs in, codes out.
#[cfg(all(test, target_arch = "x86_64"))]
#[allow(clippy::missing_transmute_annotations)] // each cast's type is on its binding
mod tests {
    use super::*;
    use crate::savedata::store::{ERR_NOT_FOUND, ERR_NOT_INITIALIZED, ERR_PARAMETER};
    use std::sync::Mutex;

    static SERIAL: Mutex<()> = Mutex::new(());

    fn symbol(name: &str) -> usize {
        export(name).unwrap_or_else(|| panic!("{name} is not exported"))
    }

    type Call1 = unsafe extern "sysv64" fn(*const u8) -> i32;
    type Call2 = unsafe extern "sysv64" fn(*const u8, *mut u8) -> i32;
    type Call4 = unsafe extern "sysv64" fn(*const u8, u32, *const u8, u64) -> i32;

    fn dir(text: &str) -> [u8; 32] {
        let mut raw = [0u8; 32];
        raw[..text.len()].copy_from_slice(text.as_bytes());
        raw
    }

    #[test]
    fn every_symbol_of_the_c_table_is_exported() {
        for name in [
            "sceSaveDataInitialize",
            "sceSaveDataInitialize2",
            "sceSaveDataInitialize3",
            "sceSaveDataTerminate",
            "sceSaveDataMount",
            "sceSaveDataMount2",
            "sceSaveDataUmount",
            "sceSaveDataSetParam",
            "sceSaveDataSaveIcon",
            "sceSaveDataDelete",
            "sceSaveDataDirNameSearch",
            "sceSaveDataSetupSaveDataMemory",
            "sceSaveDataGetSaveDataMemory",
            "sceSaveDataSetSaveDataMemory",
        ] {
            assert!(export(name).is_some(), "{name}");
        }
        assert!(export("sceSaveDataDialogOpen").is_none());
        // SAFETY: null is accepted.
        assert_eq!(unsafe { runtime_savedata_resolve(std::ptr::null()) }, 0);
    }

    #[test]
    fn a_game_saves_closes_and_finds_it_again() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let root = std::env::temp_dir().join(format!("uu-saveabi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        *TEST_USER.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(root.to_str().unwrap().to_owned());
        store().vfs = false;
        let (initialize, terminate): (Call1, unsafe extern "sysv64" fn() -> i32);
        let (mount2, umount, search, set_param, delete): (Call2, Call1, Call2, Call4, Call1);
        // SAFETY: each address is the `extern "sysv64"` function of the signature it is cast to.
        unsafe {
            initialize = std::mem::transmute(symbol("sceSaveDataInitialize"));
            terminate = std::mem::transmute(symbol("sceSaveDataTerminate"));
            mount2 = std::mem::transmute(symbol("sceSaveDataMount2"));
            umount = std::mem::transmute(symbol("sceSaveDataUmount"));
            search = std::mem::transmute(symbol("sceSaveDataDirNameSearch"));
            set_param = std::mem::transmute(symbol("sceSaveDataSetParam"));
            delete = std::mem::transmute(symbol("sceSaveDataDelete"));
        }
        let name = dir("SAVEDATA01P");
        // `OrbisSaveDataMount2`: user, dir*, blocks, mode (CREATE 4); 64 bytes.
        let mut mount = [0u8; 64];
        mount[..4].copy_from_slice(&1i32.to_le_bytes());
        mount[8..16].copy_from_slice(&(name.as_ptr() as u64).to_le_bytes());
        mount[24..28].copy_from_slice(&4u32.to_le_bytes());
        let mut result = [0xffu8; 64];
        // SAFETY: every pointer is to a live buffer of the size the call reads or writes.
        unsafe {
            assert_eq!(
                mount2(mount.as_ptr(), result.as_mut_ptr()),
                ERR_NOT_INITIALIZED
            );
            assert_eq!(initialize(std::ptr::null()), 0);
            assert_eq!(mount2(std::ptr::null(), result.as_mut_ptr()), ERR_PARAMETER);
            assert_eq!(mount2(mount.as_ptr(), std::ptr::null_mut()), ERR_PARAMETER);
            assert_eq!(mount2(mount.as_ptr(), result.as_mut_ptr()), 0);
            assert_eq!(&result[..10], b"/savedata0");
            assert_eq!(result[10], 0);
            assert_eq!(u32::from_le_bytes(result[28..32].try_into().unwrap()), 1);
            assert_eq!(
                set_param(result.as_ptr(), 1, c"Chapter 2".as_ptr().cast(), 9),
                0
            );
            assert_eq!(umount(result.as_ptr()), 0);
            assert_eq!(umount(result.as_ptr()), ERR_NOT_FOUND);
        }
        let user = root.join("savedata/1/UNKNOWN");
        let user = if user.exists() {
            user
        } else {
            root.join("savedata/1")
                .read_dir()
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path()
        };
        assert!(user.join("SAVEDATA01P").is_dir());
        let where_json = std::fs::read_to_string(user.join("where.json")).unwrap();
        assert!(where_json.contains("\"u1\"") && where_json.contains("Chapter 2"));
        // A search with room for one name returns it and counts the hits.
        let mut cond = [0u8; 64];
        cond[..4].copy_from_slice(&1i32.to_le_bytes());
        let mut names = [0u8; 64];
        let mut result = [0u8; 56];
        result[8..16].copy_from_slice(&(names.as_mut_ptr() as u64).to_le_bytes());
        result[16..20].copy_from_slice(&2u32.to_le_bytes());
        // SAFETY: live buffers of the struct sizes.
        unsafe {
            assert_eq!(search(cond.as_ptr(), result.as_mut_ptr()), 0);
        }
        assert_eq!(u32::from_le_bytes(result[..4].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(result[20..24].try_into().unwrap()), 1);
        assert_eq!(&names[..11], b"SAVEDATA01P");
        // `OrbisSaveDataDelete`: user, title*, dir*.
        let mut del = [0u8; 56];
        del[..4].copy_from_slice(&1i32.to_le_bytes());
        del[16..24].copy_from_slice(&(name.as_ptr() as u64).to_le_bytes());
        // SAFETY: live buffers.
        unsafe {
            assert_eq!(delete(del.as_ptr()), 0);
            assert_eq!(delete(del.as_ptr()), ERR_NOT_FOUND);
            assert_eq!(terminate(), 0);
        }
        assert!(!user.join("SAVEDATA01P").exists());
        *TEST_USER.lock().unwrap_or_else(PoisonError::into_inner) = None;
        store().vfs = true;
        let _ = std::fs::remove_dir_all(&root);
    }
}
