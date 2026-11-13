//! libSceSaveDataDialog, moved from `src/runtime_services.c` with the save store it asks (piece
//! 3.06). Nothing is displayed. Dialogs that only wait (progress bar, a message without
//! buttons, the "progress" system message) stay running until the game closes them, as on the
//! console; the others are answered OK/Yes when the game next asks for the status.
//!
//! `OrbisSaveDataDialogParam`: BaseParam (48), size, mode (52), dispType (56), pad, animParam,
//! items (72), userMsgParam (80), sysMsgParam (88), errorCodeParam, progressBarParam,
//! userData (112), optionParam.

use super::store::{c_text, Param, DIR_NAME};
use super::STORE;
use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard};

const NOT_INITIALIZED: i32 = 0x80B8_0003_u32 as i32;
const ALREADY_INITIALIZED: i32 = 0x80B8_0004_u32 as i32;
const NOT_FINISHED: i32 = 0x80B8_0005_u32 as i32;
const ARG_NULL: i32 = 0x80B8_0006_u32 as i32;

const LIST: u32 = 1;
const USER_MSG: u32 = 2;
const SYSTEM_MSG: u32 = 3;
const PROGRESS_BAR: u32 = 5;

/// Dialog status: 0 none, 1 initialized, 2 running, 3 finished.
const RUNNING: i32 = 2;
const FINISHED: i32 = 3;

struct Dialog {
    initialized: bool,
    status: i32,
    mode: u32,
    waits: bool,
    /// List mode: 1 an existing directory, 0 the new save entry (empty name: the game names
    /// it), -1 cancelled.
    pick: i32,
    user_data: usize,
    dir: [u8; DIR_NAME],
    param: Param,
}

static DIALOG: Mutex<Dialog> = Mutex::new(Dialog {
    initialized: false,
    status: 0,
    mode: 0,
    waits: false,
    pick: 0,
    user_data: 0,
    dir: [0; DIR_NAME],
    param: Param::ZERO,
});

fn dialog() -> MutexGuard<'static, Dialog> {
    // Nothing panics while holding it; a poisoned lock still holds a whole state.
    DIALOG
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn say(line: &str) {
    super::store::Host::say(&STORE.host, line);
}

/// A `T` at `offset` of a guest structure.
///
/// # Safety
/// `base + offset` holds a `T` (any alignment).
unsafe fn at<T: Copy>(base: *const u8, offset: usize) -> T {
    // SAFETY: forwarded from the caller.
    unsafe { base.add(offset).cast::<T>().read_unaligned() }
}

/// # Safety
/// `base + offset` is writable for a `T` (any alignment).
unsafe fn put<T: Copy>(base: *mut u8, offset: usize, value: T) {
    // SAFETY: forwarded from the caller.
    unsafe { base.add(offset).cast::<T>().write_unaligned(value) }
}

fn text(field: &[u8]) -> String {
    String::from_utf8_lossy(c_text(field)).into_owned()
}

extern "sysv64" fn initialize() -> i32 {
    let mut d = dialog();
    if d.initialized {
        return ALREADY_INITIALIZED;
    }
    d.initialized = true;
    d.status = 1;
    0
}

extern "sysv64" fn terminate() -> i32 {
    let mut d = dialog();
    if !d.initialized {
        return NOT_INITIALIZED;
    }
    d.initialized = false;
    d.status = 0;
    0
}

/// List mode: SaveDialogItems {userId, pad, titleId*, dirName*, dirNameNum, pad, newItem*,
/// focusPos, pad, focusPosDirName*, ..}; dispType 1 save, 2 load, 3 delete. The entry the
/// dialog focuses is taken, as when the player confirms at once: focusPos 0 list head (the new
/// save entry comes first), 1 list tail, 2 first directory, 3 last, 4 latest, 5 oldest, 6 named.
///
/// # Safety
/// `param` is the guest's dialog param; its items are null or valid.
unsafe fn list(d: &mut Dialog, param: *const u8) {
    // SAFETY: the dialog param (caller).
    let (kind, items): (u32, *const u8) = unsafe { (at(param, 56), at(param, 72)) };
    d.pick = -1;
    d.dir = [0; DIR_NAME];
    if items.is_null() || kind == 3 {
        return;
    }
    // SAFETY: SaveDialogItems (caller).
    let (user, title, names, count, new_item, focus_pos, focus) = unsafe {
        (
            at::<i32>(items, 0),
            at::<*const u8>(items, 8),
            at::<*const [u8; DIR_NAME]>(items, 16),
            at::<u32>(items, 24),
            at::<*const c_void>(items, 32),
            at::<u32>(items, 40),
            at::<*const u8>(items, 48),
        )
    };
    let offer_new = kind == 1 && !new_item.is_null();
    const WHICH: [i32; 7] = [0, 1, 0, 1, 2, 3, 4];
    if focus_pos == 0 && offer_new {
        d.pick = 0;
    } else {
        // SAFETY: the title and the focused name are null or C strings; `names` is null or
        // holds `count` directory names.
        let (title, focus, names) = unsafe {
            (
                super::guest::c_bytes(title, 16),
                super::guest::c_bytes(focus, DIR_NAME),
                if names.is_null() {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(names, count as usize)
                },
            )
        };
        let title = title.map(|t| String::from_utf8_lossy(&t).into_owned());
        let which = WHICH.get(focus_pos as usize).copied().unwrap_or(2);
        if let Some((i, param)) = STORE.pick(user, title.as_deref(), names, which, focus.as_deref())
        {
            d.pick = 1;
            d.dir = names[i];
            d.param = param;
        }
    }
    if d.pick < 0 && offer_new {
        d.pick = 0;
    }
    say(&format!(
        "Runtime: SaveDataDialog list ({}, {count} slots, focus {focus_pos}): {}{}",
        if kind == 1 { "save" } else { "load" },
        match d.pick {
            p if p < 0 => "nothing to choose, cancelled",
            0 => "new save data",
            _ => "chose ",
        },
        if d.pick > 0 {
            text(&d.dir)
        } else {
            String::new()
        },
    ));
}

extern "sysv64" fn open(param: *const u8) -> i32 {
    let mut d = dialog();
    if !d.initialized {
        return NOT_INITIALIZED;
    }
    if param.is_null() {
        return ARG_NULL;
    }
    let log = STORE.save_log();
    if log && d.status == RUNNING {
        say(&format!(
            "Runtime: SaveLog Open while RUNNING (previous mode {}, waits {}); accepted",
            d.mode,
            i32::from(d.waits)
        ));
    }
    // SAFETY: the guest's dialog param; its message params are null or start with their type.
    let (mode, buttons, kind, user_data) = unsafe {
        let mode: u32 = at(param, 52);
        let user_msg: *const u8 = at(param, 80);
        let sys_msg: *const u8 = at(param, 88);
        // OK, YESNO, NONE, OKCANCEL / NODATA, CONFIRM, OVERWRITE, .. PROGRESS = 5.
        let buttons = if mode == USER_MSG && !user_msg.is_null() {
            at::<u32>(user_msg, 0)
        } else {
            0
        };
        let kind = if mode == SYSTEM_MSG && !sys_msg.is_null() {
            at::<u32>(sys_msg, 0)
        } else {
            0
        };
        (mode, buttons, kind, at::<usize>(param, 112))
    };
    d.mode = mode;
    d.user_data = user_data;
    d.waits = mode == PROGRESS_BAR
        || (mode == USER_MSG && buttons == 2)
        || (mode == SYSTEM_MSG && kind == 5);
    if mode == LIST {
        // SAFETY: as above.
        unsafe { list(&mut d, param) };
    } else {
        say(&format!(
            "Runtime: SaveDataDialog mode {mode} (message type {}){}",
            if mode == SYSTEM_MSG { kind } else { buttons },
            if d.waits {
                ": shown until the game closes it"
            } else {
                ": answered OK/Yes"
            }
        ));
    }
    d.status = RUNNING;
    if log {
        say(&format!(
            "Runtime: SaveLog Open mode {mode} -> status RUNNING"
        ));
    }
    0
}

/// `UpdateStatus` and `GetStatus`.
extern "sysv64" fn status() -> i32 {
    let mut d = dialog();
    if d.status == RUNNING && !d.waits {
        d.status = FINISHED;
    }
    d.status
}

/// Result {mode, result, buttonId, pad, dirName*, param*, userData*}: OK / Yes, or cancelled
/// when a list had nothing to choose. A list writes the chosen name and, for an existing
/// directory, its param through the pointers the game passes.
extern "sysv64" fn get_result(result: *mut u8) -> i32 {
    let d = dialog();
    if result.is_null() {
        return ARG_NULL;
    }
    let log = STORE.save_log();
    if d.status != FINISHED {
        if log {
            say(&format!(
                "Runtime: SaveLog GetResult -> 0x80B80005 NOT_FINISHED (status {})",
                d.status
            ));
        }
        return NOT_FINISHED;
    }
    let cancelled = d.mode == LIST && d.pick < 0;
    // OK / USER_CANCELED; button YES.
    let (outcome, button) = (u32::from(cancelled), u32::from(!cancelled));
    // SAFETY: the guest's result structure; dirName and param are null or hold a name and a
    // param.
    unsafe {
        put(result, 0, d.mode);
        put(result, 4, outcome);
        put(result, 8, button);
        put(result, 32, d.user_data);
        if d.mode == LIST && !cancelled {
            let dir: *mut u8 = at(result, 16);
            let out_param: *mut u8 = at(result, 24);
            if !dir.is_null() {
                put(dir, 0, d.dir);
            }
            let filled = !out_param.is_null() && d.pick > 0;
            if filled {
                put(out_param, 0, d.param);
            }
            if log {
                say(&format!(
                    "Runtime: SaveLog GetResult list: dirName ptr {} (wrote '{}'), param ptr {} ({})",
                    if dir.is_null() { "NULL" } else { "set" },
                    if dir.is_null() { String::new() } else { text(&d.dir) },
                    if out_param.is_null() { "NULL" } else { "set" },
                    if filled { "filled" } else { "left alone" },
                ));
            }
        }
    }
    if log {
        say(&format!(
            "Runtime: SaveLog GetResult -> 0: wrote mode {} result {outcome} button {button} userData {:#x}; pad (offset 12) left alone",
            d.mode, d.user_data
        ));
    }
    0
}

extern "sysv64" fn ready() -> i32 {
    1
}

extern "sysv64" fn progress(target: i32, rate: u32) -> i32 {
    if STORE.save_log() {
        let d = dialog();
        say(&format!(
            "Runtime: SaveLog ProgressBarSetValue target {target} value {rate} (status {}, mode {})",
            d.status, d.mode
        ));
    }
    0
}

extern "sysv64" fn close(_param: *const c_void) -> i32 {
    let mut d = dialog();
    if STORE.save_log() {
        say(&format!(
            "Runtime: SaveLog Close (status before {}, mode {}) -> 0",
            d.status, d.mode
        ));
    }
    if d.status == RUNNING {
        d.status = FINISHED;
    }
    0
}

/// The libSceSaveDataDialog function named `symbol`.
pub(super) fn resolve(symbol: &str) -> Option<usize> {
    let address = match symbol {
        "sceSaveDataDialogInitialize" => initialize as *const () as usize,
        "sceSaveDataDialogTerminate" => terminate as *const () as usize,
        "sceSaveDataDialogOpen" => open as *const () as usize,
        "sceSaveDataDialogUpdateStatus" | "sceSaveDataDialogGetStatus" => {
            status as *const () as usize
        }
        "sceSaveDataDialogGetResult" => get_result as *const () as usize,
        "sceSaveDataDialogIsReadyToDisplay" => ready as *const () as usize,
        "sceSaveDataDialogProgressBarSetValue" => progress as *const () as usize,
        "sceSaveDataDialogClose" => close as *const () as usize,
        _ => return None,
    };
    Some(address)
}

#[cfg(test)]
mod tests;
