//! The offline AppContent provider for the configured base-game profile (piece 3.04, the deleted
//! `src/runtime_content.c`). No package or add-on mounting, downloads, entitlement or licence
//! emulation. `tests/test_content.c`'s cases are this module's tests now; the ones that end the
//! process are in `tests/content_stops.rs`.
//!
//! System modules: every library this eboot imports is provided by the host runtime, so loading
//! one only counts a reference. AppContent (module `0xb4`) also needs the content profile the
//! loader gives from `--content-profile` ([`ffi::runtime_content_configure`]).
//!
//! Guest code calls the functions [`resolve`] hands out with the System V ABI. Their state is
//! one [`Content`] behind a lock; a `STOP` (exit code 21) or the profile error (exit code 1)
//! happens after the lock is released and before anything changed.

use crate::abi::host;
use std::ffi::{c_void, CStr};
use std::sync::{Mutex, MutexGuard};

/// Module id 0.
const INVALID_ID: i32 = 0x805a_1000_u32 as i32;
/// The module has no reference.
const NOT_LOADED: i32 = 0x805a_1001_u32 as i32;
/// An AppContent argument is null or out of range.
const PARAMETER: i32 = 0x80d9_0002_u32 as i32;
/// AppContent is already initialized.
const BUSY: i32 = 0x80d9_0003_u32 as i32;

/// The AppContent system module.
const APP_CONTENT: u16 = 0xb4;
/// One reference count per possible module id.
const MODULES: usize = 0x1_0000;
/// The bytes of the init parameter (all reserved, all zero) and of the boot parameter written.
const INIT_BYTES: usize = 32;
const BOOT_BYTES: usize = 40;

/// A guest request this provider does not support: the `STOP:` line for stderr (with its new
/// line), after which the loader ends with exit code 21.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stop(pub &'static str);

/// A content profile refused by [`Content::configure`]: missing, with another first value, or
/// given after AppContent was loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidProfile;

/// The provider's state: the module reference table, the content profile and the counters
/// [`ffi::runtime_content_report`] prints. Every request that answers a guest call takes a
/// `log` for its `Runtime:` lines (stdout in the loader).
pub struct Content {
    module_refs: [u32; MODULES],
    configured: bool,
    initialized: bool,
    parameters: [u32; 5],
    queries: u32,
    lists: u32,
}

impl Default for Content {
    fn default() -> Self {
        Self::new()
    }
}

impl Content {
    /// No profile, no module loaded, nothing counted.
    pub const fn new() -> Self {
        Content {
            module_refs: [0; MODULES],
            configured: false,
            initialized: false,
            parameters: [0; 5],
            queries: 0,
            lists: 0,
        }
    }

    /// AppContent's references (the C `refs`).
    fn refs(&self) -> u32 {
        self.module_refs[usize::from(APP_CONTENT)]
    }

    /// The content profile: the five integer app parameters, the first of which is 1 or 3.
    /// Refused (`Err`) once AppContent is loaded or initialized, or for another first value.
    pub fn configure(&mut self, values: Option<&[u32; 5]>) -> Result<(), InvalidProfile> {
        match values {
            Some(values) if self.refs() == 0 && !self.initialized && matches!(values[0], 1 | 3) => {
                self.parameters = *values;
                self.configured = true;
                Ok(())
            }
            _ => Err(InvalidProfile),
        }
    }

    fn require_provider(&self) -> Result<(), Stop> {
        if self.configured {
            Ok(())
        } else {
            Err(Stop("STOP: AppContent needs --content-profile\n"))
        }
    }

    fn require_loaded(&self) -> Result<(), Stop> {
        self.require_provider()?;
        if self.refs() == 0 {
            return Err(Stop("STOP: AppContent called before module load\n"));
        }
        Ok(())
    }

    fn require_initialized(&self) -> Result<(), Stop> {
        self.require_loaded()?;
        if !self.initialized {
            return Err(Stop("STOP: AppContent query before initialization\n"));
        }
        Ok(())
    }

    /// `sceSysmoduleLoadModule`: one more reference to module `id`.
    pub fn load(&mut self, id: u16, log: &mut dyn FnMut(&str)) -> Result<i32, Stop> {
        if id == 0 {
            return Ok(INVALID_ID);
        }
        if id == APP_CONTENT {
            self.require_provider()?;
        }
        let refs = &mut self.module_refs[usize::from(id)];
        if *refs == u32::MAX {
            return Err(Stop("STOP: module reference overflow\n"));
        }
        *refs += 1;
        if id == APP_CONTENT {
            log(&format!(
                "Runtime: AppContent host provider loaded; references={refs}"
            ));
        } else {
            log(&format!(
                "Runtime: system module {id:#x} loaded (host implementation)"
            ));
        }
        Ok(0)
    }

    /// `sceSysmoduleIsLoaded`: 0 when module `id` has a reference.
    pub fn loaded(&self, id: u16) -> i32 {
        if id == 0 {
            INVALID_ID
        } else if self.module_refs[usize::from(id)] == 0 {
            NOT_LOADED
        } else {
            0
        }
    }

    /// `sceSysmoduleUnloadModule`: one reference less; AppContent's last one also ends its
    /// initialization.
    pub fn unload(&mut self, id: u16) -> i32 {
        let result = self.loaded(id);
        if result != 0 {
            return result;
        }
        self.module_refs[usize::from(id)] -= 1;
        if id == APP_CONTENT && self.refs() == 0 {
            self.initialized = false;
        }
        0
    }

    /// `sceAppContentInitialize`: only the reserved-zero init parameter is implemented; the
    /// boot parameter is zeroed (no add-on to boot from).
    pub fn init(
        &mut self,
        init: Option<&[u8; INIT_BYTES]>,
        boot: Option<&mut [u8; BOOT_BYTES]>,
        log: &mut dyn FnMut(&str),
    ) -> Result<i32, Stop> {
        self.require_loaded()?;
        let (Some(init), Some(boot)) = (init, boot) else {
            return Ok(PARAMETER);
        };
        if self.initialized {
            return Ok(BUSY);
        }
        if init.iter().any(|&byte| byte != 0) {
            return Ok(PARAMETER);
        }
        boot.fill(0);
        self.initialized = true;
        log("Runtime: AppContent initialized; offline base-game profile, no mounted add-ons");
        Ok(0)
    }

    /// `sceAppContentAppParamGetInt`: parameter `id` (0..=4) of the profile into `out`, the four
    /// bytes of a guest `int32_t` (any alignment).
    pub fn param_int(
        &mut self,
        id: u32,
        out: Option<&mut [u8; 4]>,
        log: &mut dyn FnMut(&str),
    ) -> Result<i32, Stop> {
        self.require_initialized()?;
        let (Some(&value), Some(out)) = (self.parameters.get(id as usize), out) else {
            return Ok(PARAMETER);
        };
        *out = value.to_ne_bytes();
        self.queries = self.queries.wrapping_add(1);
        log(&format!(
            "Runtime: AppContent parameter {id} = {}",
            value as i32
        ));
        Ok(0)
    }

    /// `sceAppContentGetAddcontInfoList`: no add-ons are mounted, so `hits` (the four bytes of a guest
    /// `uint32_t`) becomes 0 and the list itself is never touched. A nonzero service label is a
    /// `STOP`.
    pub fn addon_list(
        &mut self,
        service: u32,
        has_list: bool,
        capacity: u32,
        hits: Option<&mut [u8; 4]>,
    ) -> Result<i32, Stop> {
        self.require_initialized()?;
        if service != 0 {
            return Err(Stop("STOP: nonzero AppContent service label unsupported\n"));
        }
        if (capacity == 0 || !has_list) && hits.is_none() {
            return Ok(PARAMETER);
        }
        if let Some(hits) = hits {
            *hits = 0u32.to_ne_bytes();
        }
        self.lists = self.lists.wrapping_add(1);
        Ok(0)
    }

    /// The report line (the C `runtime_content_report`).
    pub fn report(&self) -> String {
        format!(
            "Runtime: AppContent references={}, initialized={}, parameter queries={}, list queries={}",
            self.refs(),
            u8::from(self.initialized),
            self.queries,
            self.lists
        )
    }
}

static STATE: Mutex<Content> = Mutex::new(Content::new());

fn state() -> MutexGuard<'static, Content> {
    // A stop ends the process outside the lock, so a poisoned lock still holds whole state.
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn out(line: &str) {
    host::out_line(line);
}

/// The guest's result, or the `STOP` and exit code 21 (the lock is already released).
fn answer(result: Result<i32, Stop>) -> i32 {
    match result {
        Ok(code) => code,
        Err(Stop(message)) => {
            host::err(message);
            host::quit(21)
        }
    }
}

/// The guest passes a `uint16_t` module id; only its low 16 bits count.
extern "sysv64" fn module_load(id: u32) -> i32 {
    let result = state().load(id as u16, &mut out);
    answer(result)
}

extern "sysv64" fn module_loaded(id: u32) -> i32 {
    state().loaded(id as u16)
}

extern "sysv64" fn module_unload(id: u32) -> i32 {
    state().unload(id as u16)
}

extern "sysv64" fn content_init(init: *const u8, boot: *mut u8) -> i32 {
    // SAFETY: the guest passes null or its 32-byte init parameter and 40-byte boot parameter
    // (byte arrays, so any alignment is fine).
    let (init, boot) = unsafe {
        (
            init.cast::<[u8; INIT_BYTES]>().as_ref(),
            boot.cast::<[u8; BOOT_BYTES]>().as_mut(),
        )
    };
    let result = state().init(init, boot, &mut out);
    answer(result)
}

extern "sysv64" fn param_int(id: u32, value: *mut i32) -> i32 {
    // SAFETY: the guest passes null or its `int32_t`, written as four bytes (any alignment).
    let value = unsafe { value.cast::<[u8; 4]>().as_mut() };
    let result = state().param_int(id, value, &mut out);
    answer(result)
}

extern "sysv64" fn addon_list(
    service: u32,
    list: *mut c_void,
    capacity: u32,
    hits: *mut u32,
) -> i32 {
    // SAFETY: the guest passes null or its `uint32_t`, written as four bytes (any alignment).
    let hits = unsafe { hits.cast::<[u8; 4]>().as_mut() };
    let result = state().addon_list(service, !list.is_null(), capacity, hits);
    answer(result)
}

/// The function behind a scoped NID of the module or AppContent libraries, or `None`.
pub fn resolve(name: &CStr) -> Option<usize> {
    let address = match name.to_bytes() {
        b"g8cM39EUZ6o#M#N" => module_load as *const () as usize,
        b"fMP5NHUOaMk#M#N" => module_loaded as *const () as usize,
        b"eR2bZFAAU0Q#M#N" => module_unload as *const () as usize,
        b"R9lA82OraNs#c#d" => content_init as *const () as usize,
        b"99b82IKXpH4#c#d" => param_int as *const () as usize,
        b"xnd8BJzAxmk#c#d" => addon_list as *const () as usize,
        _ => return None,
    };
    Some(address)
}

/// The exported C functions of this module, declared in `include/uncharted_runtime.h`.
pub mod ffi {
    use super::{host, state};
    use std::ffi::{c_char, CStr};

    /// Sets the content profile (`--content-profile`: five integer app parameters, the first 1
    /// or 3) before the game loads AppContent. Null, another first value, or a call after the
    /// load ends the process (exit code 1).
    ///
    /// # Safety
    /// `values` is null or points to five `uint32_t`.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_content_configure(values: *const u32) {
        // SAFETY: the caller passes null or five values.
        let values = unsafe { values.cast::<[u32; 5]>().as_ref() };
        let result = state().configure(values);
        if result.is_err() {
            host::err("ERROR: invalid or late content profile\n");
            // SAFETY: ends the process through the C runtime, as the C module did.
            unsafe { host::exit(1) }
        }
    }

    /// The host function for a guest import of the module or AppContent libraries, or 0.
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_content_resolve(name: *const c_char) -> usize {
        if name.is_null() {
            return 0;
        }
        // SAFETY: a non-null name is a NUL-terminated string (caller).
        super::resolve(unsafe { CStr::from_ptr(name) }).unwrap_or(0)
    }

    /// AppContent's references, initialization and query counters, on stdout.
    #[no_mangle]
    pub extern "C" fn runtime_content_report() {
        let line = state().report();
        host::out_line(&line);
    }
}

#[cfg(test)]
mod tests {
    //! `tests/test_content.c`'s cases on a fresh [`Content`].

    use super::*;

    const PROFILE: [u32; 5] = [1, 13, 0x8000_0000, 0, 7];

    fn configured() -> Box<Content> {
        let mut content = Box::new(Content::new());
        content.configure(Some(&PROFILE)).unwrap();
        content
    }

    fn quiet(_: &str) {}

    #[test]
    fn every_nid_resolves_and_only_those() {
        for nid in [
            c"g8cM39EUZ6o#M#N",
            c"fMP5NHUOaMk#M#N",
            c"eR2bZFAAU0Q#M#N",
            c"R9lA82OraNs#c#d",
            c"99b82IKXpH4#c#d",
            c"xnd8BJzAxmk#c#d",
        ] {
            assert!(resolve(nid).is_some(), "{nid:?}");
        }
        assert_eq!(resolve(c"g8cM39EUZ6o#I#J"), None);
        // SAFETY: null and NUL-terminated literals.
        unsafe {
            assert_eq!(ffi::runtime_content_resolve(std::ptr::null()), 0);
            assert_eq!(
                ffi::runtime_content_resolve(c"99b82IKXpH4#c#d".as_ptr()),
                resolve(c"99b82IKXpH4#c#d").unwrap()
            );
        }
    }

    #[test]
    fn app_content_without_a_profile_stops_and_is_not_loaded() {
        let mut content = Box::new(Content::new());
        assert_eq!(
            content.load(APP_CONTENT, &mut quiet),
            Err(Stop("STOP: AppContent needs --content-profile\n"))
        );
        assert_eq!(content.loaded(APP_CONTENT), NOT_LOADED);
    }

    #[test]
    fn profile_is_refused_when_invalid_or_late() {
        let mut content = Box::new(Content::new());
        assert!(content.configure(None).is_err());
        assert!(content.configure(Some(&[2, 0, 0, 0, 0])).is_err());
        assert!(content.configure(Some(&[3, 0, 0, 0, 0])).is_ok());
        assert!(content.configure(Some(&PROFILE)).is_ok());
        content.load(APP_CONTENT, &mut quiet).unwrap();
        assert!(content.configure(Some(&PROFILE)).is_err());
    }

    #[test]
    fn other_modules_are_counted_independently() {
        let mut content = configured();
        let mut lines = Vec::new();
        assert_eq!(content.loaded(0xb5), NOT_LOADED);
        assert_eq!(
            content.load(0xb5, &mut |line| lines.push(line.to_owned())),
            Ok(0)
        );
        assert_eq!(content.loaded(0xb5), 0);
        assert_eq!(content.loaded(APP_CONTENT), NOT_LOADED);
        assert_eq!(content.unload(0xb5), 0);
        assert_eq!(content.loaded(0xb5), NOT_LOADED);
        assert_eq!(
            lines,
            ["Runtime: system module 0xb5 loaded (host implementation)"]
        );
        // Another module needs no profile.
        let mut bare = Box::new(Content::new());
        assert_eq!(bare.load(0x80, &mut quiet), Ok(0));
    }

    #[test]
    fn reference_overflow_stops() {
        let mut content = configured();
        content.module_refs[0x22] = u32::MAX;
        assert_eq!(
            content.load(0x22, &mut quiet),
            Err(Stop("STOP: module reference overflow\n"))
        );
        assert_eq!(content.module_refs[0x22], u32::MAX);
    }

    #[test]
    fn requests_before_load_and_before_init_stop() {
        let mut content = configured();
        let init = [0u8; INIT_BYTES];
        let mut boot = [0u8; BOOT_BYTES];
        assert_eq!(
            content.init(Some(&init), Some(&mut boot), &mut quiet),
            Err(Stop("STOP: AppContent called before module load\n"))
        );
        content.load(APP_CONTENT, &mut quiet).unwrap();
        let mut value = [0u8; 4];
        let before_init = Err(Stop("STOP: AppContent query before initialization\n"));
        assert_eq!(
            content.param_int(0, Some(&mut value), &mut quiet),
            before_init
        );
        assert_eq!(
            content.addon_list(0, false, 0, Some(&mut value)),
            before_init
        );
        content
            .init(Some(&init), Some(&mut boot), &mut quiet)
            .unwrap();
        assert_eq!(
            content.addon_list(1, false, 0, Some(&mut value)),
            Err(Stop("STOP: nonzero AppContent service label unsupported\n"))
        );
    }

    /// `test_content.c` with no argument: the lifecycle, the profile and the output boundaries.
    #[test]
    fn lifecycle_profile_and_output_boundaries() {
        let mut content = configured();
        let mut lines = Vec::new();
        let mut log = |line: &str| lines.push(line.to_owned());
        assert_eq!(content.loaded(0), INVALID_ID);
        assert_eq!(content.load(0, &mut log), Ok(INVALID_ID));
        assert_eq!(content.loaded(APP_CONTENT), NOT_LOADED);
        assert_eq!(content.unload(APP_CONTENT), NOT_LOADED);
        assert_eq!(content.load(APP_CONTENT, &mut log), Ok(0));
        assert_eq!(content.load(APP_CONTENT, &mut log), Ok(0));
        assert_eq!(content.loaded(APP_CONTENT), 0);

        let mut initial = [0u8; INIT_BYTES];
        let mut boot = [0xa5u8; BOOT_BYTES];
        assert_eq!(content.init(None, Some(&mut boot), &mut log), Ok(PARAMETER));
        assert_eq!(content.init(Some(&initial), None, &mut log), Ok(PARAMETER));
        initial[31] = 1;
        assert_eq!(
            content.init(Some(&initial), Some(&mut boot), &mut log),
            Ok(PARAMETER)
        );
        initial[31] = 0;
        assert_eq!(boot[0], 0xa5);
        assert_eq!(
            content.init(Some(&initial), Some(&mut boot), &mut log),
            Ok(0)
        );
        assert_eq!(boot, [0; BOOT_BYTES]);
        boot[0] = 0xcc;
        assert_eq!(
            content.init(Some(&initial), Some(&mut boot), &mut log),
            Ok(BUSY)
        );
        assert_eq!(boot[0], 0xcc);

        for (id, &expected) in PROFILE.iter().enumerate() {
            let mut value = (-1i32).to_ne_bytes();
            assert_eq!(
                content.param_int(id as u32, Some(&mut value), &mut log),
                Ok(0)
            );
            assert_eq!(u32::from_ne_bytes(value), expected);
        }
        let mut value = 77i32.to_ne_bytes();
        assert_eq!(
            content.param_int(5, Some(&mut value), &mut log),
            Ok(PARAMETER)
        );
        assert_eq!(i32::from_ne_bytes(value), 77);
        assert_eq!(content.param_int(0, None, &mut log), Ok(PARAMETER));

        let mut hits = 99u32.to_ne_bytes();
        assert_eq!(content.addon_list(0, false, 0, Some(&mut hits)), Ok(0));
        assert_eq!(hits, [0; 4]);
        hits = 99u32.to_ne_bytes();
        assert_eq!(content.addon_list(0, true, 2, Some(&mut hits)), Ok(0));
        assert_eq!(hits, [0; 4]);
        assert_eq!(content.addon_list(0, false, 0, None), Ok(PARAMETER));
        assert_eq!(content.addon_list(0, true, 0, None), Ok(PARAMETER));
        assert_eq!(content.addon_list(0, true, 2, None), Ok(0));
        assert_eq!(
            content.report(),
            "Runtime: AppContent references=2, initialized=1, parameter queries=5, list queries=3"
        );

        assert_eq!(content.unload(APP_CONTENT), 0);
        assert_eq!(content.loaded(APP_CONTENT), 0);
        assert_eq!(
            content.init(Some(&initial), Some(&mut boot), &mut log),
            Ok(BUSY)
        );
        assert_eq!(content.unload(APP_CONTENT), 0);
        assert_eq!(content.loaded(APP_CONTENT), NOT_LOADED);
        assert_eq!(content.load(APP_CONTENT, &mut log), Ok(0));
        assert_eq!(
            content.init(Some(&initial), Some(&mut boot), &mut log),
            Ok(0)
        );
        assert_eq!(content.unload(APP_CONTENT), 0);

        assert_eq!(
            lines[..4],
            [
                "Runtime: AppContent host provider loaded; references=1",
                "Runtime: AppContent host provider loaded; references=2",
                "Runtime: AppContent initialized; offline base-game profile, no mounted add-ons",
                "Runtime: AppContent parameter 0 = 1",
            ]
        );
        assert!(lines.contains(&"Runtime: AppContent parameter 2 = -2147483648".to_owned()));
    }
}
