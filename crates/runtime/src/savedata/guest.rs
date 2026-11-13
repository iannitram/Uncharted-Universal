//! The libSceSaveData functions the guest calls (`extern "sysv64"`, x86-64 hosts only): each
//! reads the guest's structures and hands plain values to [`super::STORE`]. Layouts as the
//! deleted `src/runtime_savedata.c` declared them.

use super::store::{
    c_text, MountResult, Param, ParamValue, DIR_NAME, ERR_NOT_INITIALIZED, ERR_PARAMETER,
};
use super::STORE;
use crate::abi::nid_name;
use std::ffi::{c_void, CStr};
use std::io::{Read, Write};

/// `OrbisSaveDataTitleId`: ten characters and padding, read as a C string.
type TitleId = [u8; 16];
/// `OrbisSaveDataDirName`.
type DirName = [u8; DIR_NAME];
/// `OrbisSaveDataMountPoint`: `/savedata0` ..
type MountPoint = [u8; 16];

/// `OrbisSaveDataMount` (`sceSaveDataMount`).
#[repr(C)]
#[allow(dead_code)] // the guest's layout, read or not
struct Mount1 {
    user: i32,
    pad: i32,
    title: *const TitleId,
    dir: *const DirName,
    fingerprint: *const c_void,
    blocks: u64,
    mode: u32,
    reserved: [u8; 32],
}

/// `OrbisSaveDataMount2` (`sceSaveDataMount2`).
#[repr(C)]
#[allow(dead_code)] // the guest's layout, read or not
struct Mount2 {
    user: i32,
    pad: i32,
    dir: *const DirName,
    blocks: u64,
    mode: u32,
    reserved: [u8; 32],
    pad2: i32,
}

/// `OrbisSaveDataDelete`.
#[repr(C)]
#[allow(dead_code)] // the guest's layout, read or not
struct Delete {
    user: i32,
    pad: i32,
    title: *const TitleId,
    dir: *const DirName,
    unused: u32,
    reserved: [u8; 32],
    pad2: i32,
}

/// `OrbisSaveDataDirNameSearchCond`.
#[repr(C)]
#[allow(dead_code)] // the guest's layout, read or not
struct SearchCond {
    user: i32,
    pad: i32,
    title: *const TitleId,
    dir: *const DirName,
    key: u32,
    order: u32,
    reserved: [u8; 32],
}

/// `OrbisSaveDataSearchInfo`.
#[repr(C)]
#[allow(dead_code)] // the guest's layout, read or not
struct SearchInfo {
    blocks: u64,
    free_blocks: u64,
    reserved: [u8; 32],
}

/// `OrbisSaveDataDirNameSearchResult`.
#[repr(C)]
#[allow(dead_code)] // the guest's layout, read or not
struct SearchResult {
    hits: u32,
    pad: i32,
    names: *mut DirName,
    names_capacity: u32,
    set_count: u32,
    params: *mut Param,
    infos: *mut SearchInfo,
    reserved: [u8; 12],
    pad2: i32,
}

/// `OrbisSaveDataIcon`.
#[repr(C)]
#[allow(dead_code)] // the guest's layout, read or not
struct Icon {
    buffer: *const c_void,
    buffer_size: u64,
    data_size: u64,
    reserved: [u8; 32],
}

const _: () = {
    use std::mem::size_of;
    assert!(size_of::<Mount1>() == 80 && size_of::<Mount2>() == 64);
    assert!(size_of::<MountResult>() == 64 && size_of::<Delete>() == 64);
    assert!(size_of::<SearchCond>() == 64 && size_of::<SearchResult>() == 56);
    assert!(size_of::<SearchInfo>() == 48 && size_of::<Icon>() == 56);
};

/// A title as the store takes it (None for a null pointer).
fn title(title: Option<&TitleId>) -> Option<String> {
    title.map(|t| String::from_utf8_lossy(c_text(t)).into_owned())
}

/// A guest C string at `text`: its bytes up to the NUL, at most `max`, read one at a time (never
/// past the NUL). None for a null pointer.
///
/// # Safety
/// `text` is null or a string readable up to its NUL or `max` bytes.
pub(super) unsafe fn c_bytes(text: *const u8, max: usize) -> Option<Vec<u8>> {
    if text.is_null() {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..max {
        // SAFETY: within the string (caller), not past its NUL.
        let byte = unsafe { text.add(i).read() };
        if byte == 0 {
            break;
        }
        out.push(byte);
    }
    Some(out)
}

extern "sysv64" fn initialize(_param: *const c_void) -> i32 {
    STORE.initialize()
}

extern "sysv64" fn terminate() -> i32 {
    STORE.terminate()
}

extern "sysv64" fn mount(m: *const Mount1, result: *mut MountResult) -> i32 {
    // SAFETY: the guest passes null or its mount request, whose title and dir are null or point
    // to a title id and a directory name; `result` is null or its result.
    unsafe {
        let Some(m) = m.as_ref() else {
            return ERR_PARAMETER;
        };
        let title = title(m.title.as_ref());
        let dir = m.dir.as_ref().map(|d| &d[..]);
        STORE.mount(m.user, title.as_deref(), dir, m.mode, result.as_mut())
    }
}

extern "sysv64" fn mount2(m: *const Mount2, result: *mut MountResult) -> i32 {
    // SAFETY: as in `mount`.
    unsafe {
        let Some(m) = m.as_ref() else {
            return ERR_PARAMETER;
        };
        let dir = m.dir.as_ref().map(|d| &d[..]);
        STORE.mount(m.user, None, dir, m.mode, result.as_mut())
    }
}

/// The guest's mount point, or None for a null pointer.
///
/// # Safety
/// `point` is null or a mount point.
unsafe fn point<'a>(point: *const MountPoint) -> Option<&'a [u8]> {
    // SAFETY: forwarded from the caller.
    unsafe { point.as_ref() }.map(|p| &p[..])
}

extern "sysv64" fn umount(at: *const MountPoint) -> i32 {
    // SAFETY: the guest passes null or a mount point.
    STORE.umount(unsafe { point(at) })
}

/// What `SetParam` writes: type 0 the whole param (at least its size), 1 to 3 the title,
/// subtitle or detail (`%.*s` of `size`: up to the NUL or `size` bytes, as many as fit), 4 the
/// user param (at least 4 bytes).
///
/// # Safety
/// `buffer` holds `size` bytes, or for types 1 to 3 a string ending within them.
unsafe fn param_value(kind: u32, buffer: *const u8, size: u64) -> ParamValue {
    match kind {
        // SAFETY: at least a whole param (caller).
        0 if size >= std::mem::size_of::<Param>() as u64 => {
            ParamValue::All(Box::new(unsafe { buffer.cast::<Param>().read_unaligned() }))
        }
        1..=3 => {
            let fits = if kind == 3 { 1023 } else { 127 };
            // `%.*s` takes the size as an int; a negative precision is no limit.
            let limit = usize::try_from(size as i32).map_or(fits, |n| n.min(fits));
            // SAFETY: the string ends within `size` bytes (caller); at most `limit` are read.
            ParamValue::Text(kind, unsafe { c_bytes(buffer, limit) }.unwrap_or_default())
        }
        // SAFETY: at least 4 bytes (caller).
        4 if size >= 4 => ParamValue::UserParam(unsafe { buffer.cast::<u32>().read_unaligned() }),
        _ => ParamValue::Bad,
    }
}

extern "sysv64" fn set_param(
    at: *const MountPoint,
    kind: u32,
    buffer: *const c_void,
    size: u64,
) -> i32 {
    // SAFETY: the guest passes null or a mount point, and a buffer of `size` bytes.
    let value =
        (!buffer.is_null()).then_some(move || unsafe { param_value(kind, buffer.cast(), size) });
    // SAFETY: as above.
    STORE.set_param(unsafe { point(at) }, value)
}

extern "sysv64" fn save_icon(at: *const MountPoint, icon: *const Icon) -> i32 {
    // SAFETY: the guest passes null or its icon, whose buffer is null or holds `data_size`
    // bytes.
    let icon = unsafe { icon.as_ref() }.filter(|i| !i.buffer.is_null());
    let bytes = icon.map(|i| {
        // SAFETY: as above.
        move || unsafe { std::slice::from_raw_parts(i.buffer.cast::<u8>(), i.data_size as usize) }
    });
    // SAFETY: as above.
    STORE.save_icon(unsafe { point(at) }, bytes)
}

extern "sysv64" fn delete(d: *const Delete) -> i32 {
    // SAFETY: the guest passes null or its request, whose title and dir are null or point to a
    // title id and a directory name.
    unsafe {
        let d = d.as_ref();
        let title = title(d.and_then(|d| d.title.as_ref()));
        let dir = d.and_then(|d| d.dir.as_ref()).map(|n| &n[..]);
        STORE.delete(d.map_or(0, |d| d.user), title.as_deref(), dir)
    }
}

extern "sysv64" fn search(cond: *const SearchCond, result: *mut SearchResult) -> i32 {
    if !STORE.is_initialized() {
        return ERR_NOT_INITIALIZED;
    }
    // SAFETY: the guest passes null or its condition (title and dir null or valid) and null or
    // its result, whose arrays are null or hold `names_capacity` entries.
    unsafe {
        let (Some(cond), Some(result)) = (cond.as_ref(), result.as_mut()) else {
            return ERR_PARAMETER;
        };
        let title = title(cond.title.as_ref());
        let pattern = cond.dir.as_ref().map(|d| &d[..]);
        let entries = match STORE.search(cond.user, title.as_deref(), pattern, cond.key, cond.order)
        {
            Ok(entries) => entries,
            Err(r) => return r,
        };
        result.hits = entries.len() as u32;
        let set = entries.len().min(result.names_capacity as usize);
        for (i, entry) in entries.iter().take(set).enumerate() {
            if !result.names.is_null() {
                let mut name = [0u8; DIR_NAME];
                let n = entry.name.len().min(DIR_NAME - 1);
                name[..n].copy_from_slice(&entry.name.as_bytes()[..n]);
                result.names.add(i).write_unaligned(name);
            }
            if !result.params.is_null() {
                result.params.add(i).write_unaligned(entry.param);
            }
            if !result.infos.is_null() {
                result.infos.add(i).write_unaligned(SearchInfo {
                    blocks: 32768,
                    free_blocks: 16384,
                    reserved: [0; 32],
                });
            }
        }
        result.set_count = set as u32;
    }
    0
}

extern "sysv64" fn memory_setup(user: i32, size: u64, param: *const Param) -> i32 {
    // SAFETY: the guest passes null or a whole param.
    let param = (!param.is_null()).then(|| unsafe { param.read_unaligned() });
    STORE.memory_setup(user, size, param.as_ref())
}

extern "sysv64" fn memory_get(_user: i32, buffer: *mut c_void, size: u64, offset: i64) -> i32 {
    STORE.memory_io(!buffer.is_null(), size, offset, false, |file| {
        // SAFETY: the guest's buffer holds `size` bytes (it is not null here).
        let out = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), size as usize) };
        file.read_exact(out)
    })
}

extern "sysv64" fn memory_set(_user: i32, buffer: *const c_void, size: u64, offset: i64) -> i32 {
    STORE.memory_io(!buffer.is_null(), size, offset, true, |file| {
        // SAFETY: the guest's buffer holds `size` bytes (it is not null here).
        let data = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), size as usize) };
        file.write_all(data)
    })
}

/// The libSceSaveData and libSceSaveDataDialog function for the scoped NID `name`, by its
/// symbol name as `RUNTIME_LOOKUP` matched it. `sceSaveDataInitialize2`, `..3` and
/// `sceSaveDataMount2` are not in the NID table; they are listed as the C table listed them.
pub(super) fn resolve(name: &CStr) -> Option<usize> {
    let symbol = name.to_str().ok().and_then(nid_name)?;
    let address = match symbol {
        "sceSaveDataInitialize" | "sceSaveDataInitialize2" | "sceSaveDataInitialize3" => {
            initialize as *const () as usize
        }
        "sceSaveDataTerminate" => terminate as *const () as usize,
        "sceSaveDataMount" => mount as *const () as usize,
        "sceSaveDataMount2" => mount2 as *const () as usize,
        "sceSaveDataUmount" => umount as *const () as usize,
        "sceSaveDataSetParam" => set_param as *const () as usize,
        "sceSaveDataSaveIcon" => save_icon as *const () as usize,
        "sceSaveDataDelete" => delete as *const () as usize,
        "sceSaveDataDirNameSearch" => search as *const () as usize,
        "sceSaveDataSetupSaveDataMemory" => memory_setup as *const () as usize,
        "sceSaveDataGetSaveDataMemory" => memory_get as *const () as usize,
        "sceSaveDataSetSaveDataMemory" => memory_set as *const () as usize,
        _ => return super::dialog::resolve(symbol),
    };
    Some(address)
}

#[cfg(test)]
mod tests;
