//! System services for an offline, single-user console (piece 3.11; the deleted
//! `src/runtime_services.c`):
//!
//! - one local user (id 1) who is logged in but not signed in to PSN;
//! - the network cable unplugged: NetCtl disconnected, sockets unavailable, HTTP fails;
//! - common dialogs complete at once (no dialog UI is drawn), except the text input dialog,
//!   which the GPU library shows in the game window, and save dialogs that only wait;
//! - the whole package installed (PlayGo reports every chunk local), no add-ons, no disc map;
//! - no mouse, microphone or voice chat audio.
//!
//! Every function here is an explicit contract; unknown functions still stop. The plain parts
//! ([`language`], [`utf16_to_utf8`], [`utf8_to_utf16`], [`online_symbol`]) build for every
//! target; the guest functions (x86-64 only) are found by symbol name through the NID table, as
//! `RUNTIME_LOOKUP` found them. With `BB_ONLINE=1` (co-op, loader build) network and PSN imports
//! go to the GPU library's online versions first.

/// `sceSystemServiceParamGetInt(1)`: the system language from `BB_LANGUAGE` read as C `atoi`
/// does, or 1 (English, United States) without it. 8 is Russian.
pub fn language(value: Option<&str>) -> i32 {
    value.map_or(1, atoi)
}

/// C `atoi`: leading white space, an optional sign, then decimal digits; anything else ends
/// the number, and no digits is 0. Overflow wraps.
pub fn atoi(text: &str) -> i32 {
    let bytes = text.as_bytes();
    let mut i = bytes
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
        .unwrap_or(bytes.len());
    let negative = match bytes.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let mut value = 0i32;
    while let Some(digit @ b'0'..=b'9') = bytes.get(i) {
        value = value.wrapping_mul(10).wrapping_add(i32::from(digit - b'0'));
        i += 1;
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

/// UTF-16 `input` (ending at its first NUL) as UTF-8 of at most `size - 5` bytes, whole
/// characters only, as the C converter wrote into a buffer of `size` bytes. A surrogate pair is
/// one character; a lone surrogate is encoded as itself.
pub fn utf16_to_utf8(input: &[u16], size: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < input.len() && input[i] != 0 && out.len() + 4 < size {
        let mut c = u32::from(input[i]);
        if (0xD800..0xDC00).contains(&c)
            && i + 1 < input.len()
            && (0xDC00..0xE000).contains(&input[i + 1])
        {
            c = 0x10000 + ((c - 0xD800) << 10) + (u32::from(input[i + 1]) - 0xDC00);
            i += 1;
        }
        if c < 0x80 {
            out.push(c as u8);
        } else if c < 0x800 {
            out.extend([0xC0 | (c >> 6) as u8, 0x80 | (c & 63) as u8]);
        } else if c < 0x10000 {
            out.extend([
                0xE0 | (c >> 12) as u8,
                0x80 | ((c >> 6) & 63) as u8,
                0x80 | (c & 63) as u8,
            ]);
        } else {
            out.extend([
                0xF0 | (c >> 18) as u8,
                0x80 | ((c >> 12) & 63) as u8,
                0x80 | ((c >> 6) & 63) as u8,
                0x80 | (c & 63) as u8,
            ]);
        }
        i += 1;
    }
    out
}

/// UTF-8 `input` (ending at its first NUL) as at most `max` UTF-16 units; a character that does
/// not fit whole ends the text. Malformed bytes are decoded leniently, as the C did: a lead byte
/// takes the continuation bytes that follow it, and a stray continuation byte reads as a 4-byte
/// lead. The caller writes the terminator after the units (at index `max` when they fill it).
pub fn utf8_to_utf16(input: &[u8], max: usize) -> Vec<u16> {
    let mut out = Vec::new();
    let mut p = 0;
    let byte = |i: usize| input.get(i).copied().unwrap_or(0);
    while byte(p) != 0 && out.len() < max {
        let lead = byte(p);
        let (mut c, extra) = if lead < 0x80 {
            (u32::from(lead), 0)
        } else if lead & 0xE0 == 0xC0 {
            (u32::from(lead & 31), 1)
        } else if lead & 0xF0 == 0xE0 {
            (u32::from(lead & 15), 2)
        } else {
            (u32::from(lead & 7), 3)
        };
        p += 1;
        for _ in 0..extra {
            if byte(p) & 0xC0 != 0x80 {
                break;
            }
            c = (c << 6) | u32::from(byte(p) & 63);
            p += 1;
        }
        if c >= 0x10000 {
            if out.len() + 2 > max {
                break;
            }
            c -= 0x10000;
            out.push((0xD800 + (c >> 10)) as u16);
            out.push((0xDC00 + (c & 1023)) as u16);
        } else {
            out.push(c as u16);
        }
    }
    out
}

/// Whether co-op (`BB_ONLINE=1`) sends the import `symbol` to the GPU library's online
/// version: network and PSN calls do; trophies, the store and profile dialogs stay here.
pub fn online_symbol(symbol: &str) -> bool {
    const ONLINE: [&str; 4] = ["sceNet", "sceNp", "sceHttp", "sceSsl"];
    const LOCAL: [&str; 3] = ["sceNpTrophy", "sceNpCommerce", "sceNpProfileDialog"];
    ONLINE.iter().any(|prefix| symbol.starts_with(prefix))
        && !LOCAL.iter().any(|prefix| symbol.starts_with(prefix))
}

/// Whether the environment value is on: it starts with `1`.
fn flag(value: Option<std::ffi::OsString>) -> bool {
    value.is_some_and(|v| v.as_encoded_bytes().first() == Some(&b'1'))
}

#[cfg(target_arch = "x86_64")]
mod guest {
    //! The guest's service functions (System V ABI on every host).

    use super::*;
    use crate::abi::host as c;
    use std::ffi::{c_char, c_void, CStr};
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    const USER_ID: i32 = 1;
    const fn code(value: u32) -> i32 {
        value as i32
    }
    const USER_INVALID_ARGUMENT: i32 = code(0x8096_0005);
    const USER_NO_EVENT: i32 = code(0x8096_0007);
    const USER_BUFFER_TOO_SHORT: i32 = code(0x8096_000a);
    const SYSTEM_PARAMETER: i32 = code(0x80A1_0003);
    const SYSTEM_NO_EVENT: i32 = code(0x80A1_0004);
    pub(super) const NET_CTL_INVALID_ADDR: i32 = code(0x8041_2107);
    pub(super) const NET_CTL_NOT_CONNECTED: i32 = code(0x8041_2108);
    pub(super) const NP_INVALID_ARGUMENT: i32 = code(0x8055_0003);
    pub(super) const NP_SIGNED_OUT: i32 = code(0x8055_0006);
    const NP_UTIL_NOT_MATCH: i32 = code(0x8055_0609);
    pub(super) const NET_ENETUNREACH: i32 = code(0x8041_0133);
    pub(super) const NET_EINVAL: i32 = code(0x8041_0116);
    pub(super) const HTTP_NETWORK: i32 = code(0x8043_1063);
    const HTTP_INVALID_VALUE: i32 = code(0x8043_1077);
    pub(super) const DIALOG_NOT_INITIALIZED: i32 = code(0x80B8_0003);
    pub(super) const DIALOG_ALREADY_INITIALIZED: i32 = code(0x80B8_0004);
    pub(super) const DIALOG_NOT_FINISHED: i32 = code(0x80B8_0005);
    pub(super) const DIALOG_ARG_NULL: i32 = code(0x80B8_0006);
    pub(super) const PLAYGO_BAD_HANDLE: i32 = code(0x80B2_0009);
    pub(super) const PLAYGO_BAD_POINTER: i32 = code(0x80B2_000A);
    pub(super) const PLAYGO_BAD_SIZE: i32 = code(0x80B2_000B);
    pub(super) const PLAYGO_BAD_CHUNK: i32 = code(0x80B2_000C);
    pub(super) const AUDIO_IN_NOT_OPENED: i32 = code(0x8026_0109);
    const VOICE_INVALID_ARGUMENT: i32 = code(0x8029_000b);
    pub(super) const APP_CONTENT_NOT_FOUND: i32 = code(0x80D9_0005);
    const APP_CONTENT_PARAMETER: i32 = code(0x80D9_0002);
    pub(super) const DISC_MAP_NO_BITMAP: i32 = code(0x8110_0004);
    const MOUSE_INVALID_ARGUMENT: i32 = code(0x80DF_0001);
    pub(super) const IME_INVALID_ADDRESS: i32 = code(0x80BC_0004);
    pub(super) const IME_BUSY: i32 = code(0x80BC_0003);
    pub(super) const IME_NOT_FINISHED: i32 = code(0x80BC_0101);
    /// The process exit code that asks `run.py` to start the executable in `BB_LOADEXEC_FILE`.
    const LOADEXEC_EXIT: i32 = 101;

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    static NEXT_ID: AtomicI32 = AtomicI32::new(1);

    /// A fresh id for an object that does nothing (pools, contexts, requests, ports).
    fn new_id() -> i32 {
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    }

    fn note(what: &str) {
        c::out_line(&format!("Runtime: {what}"));
    }

    fn env(name: &str) -> Option<String> {
        std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
    }

    /// The bytes of a guest string; null is empty.
    ///
    /// # Safety
    /// `text` is null or NUL-terminated.
    unsafe fn guest_str<'a>(text: *const c_char) -> &'a [u8] {
        if text.is_null() {
            return &[];
        }
        // SAFETY: NUL-terminated (caller).
        unsafe { CStr::from_ptr(text) }.to_bytes()
    }

    /// Reads a `T` at `offset` bytes into a guest structure, unaligned.
    ///
    /// # Safety
    /// `base + offset` is readable for a `T`.
    unsafe fn field<T: Copy>(base: *const u8, offset: usize) -> T {
        // SAFETY: readable (caller).
        unsafe { base.add(offset).cast::<T>().read_unaligned() }
    }

    /// Writes `value` at `offset` bytes into a guest structure, unaligned.
    ///
    /// # Safety
    /// `base + offset` is writable for a `T`.
    unsafe fn set_field<T: Copy>(base: *mut u8, offset: usize, value: T) {
        // SAFETY: writable (caller).
        unsafe { base.add(offset).cast::<T>().write_unaligned(value) }
    }

    extern "sysv64" fn ok_void() -> i32 {
        0
    }

    // ---- UserService ----

    static LOGIN_EVENT_PENDING: AtomicBool = AtomicBool::new(true);

    extern "sysv64" fn user_initialize(_params: *const c_void) -> i32 {
        note("UserService initialized (user 1 logged in)");
        0
    }

    extern "sysv64" fn user_initial(id: *mut i32) -> i32 {
        // SAFETY: the guest passes null or its user id.
        let Some(id) = (unsafe { id.as_mut() }) else {
            return USER_INVALID_ARGUMENT;
        };
        *id = USER_ID;
        0
    }

    extern "sysv64" fn user_list(ids: *mut [i32; 4]) -> i32 {
        // SAFETY: the guest passes null or its list of four user ids.
        let Some(ids) = (unsafe { ids.as_mut() }) else {
            return USER_INVALID_ARGUMENT;
        };
        *ids = [USER_ID, -1, -1, -1];
        0
    }

    extern "sysv64" fn user_name(id: i32, name: *mut u8, size: u64) -> i32 {
        if id != USER_ID || name.is_null() {
            return USER_INVALID_ARGUMENT;
        }
        let value = env("BB_USER_NAME").unwrap_or_else(|| "Hunter".into());
        if value.len() as u64 + 1 > size {
            return USER_BUFFER_TOO_SHORT;
        }
        // SAFETY: the guest's buffer holds `size` bytes, at least the name and its NUL.
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), name, value.len());
            *name.add(value.len()) = 0;
        }
        0
    }

    extern "sysv64" fn user_event(event: *mut [i32; 2]) -> i32 {
        // SAFETY: the guest passes null or its event (type, user).
        let Some(event) = (unsafe { event.as_mut() }) else {
            return USER_INVALID_ARGUMENT;
        };
        if !LOGIN_EVENT_PENDING.swap(false, Ordering::AcqRel) {
            return USER_NO_EVENT;
        }
        *event = [0, USER_ID]; // LOGIN
        0
    }

    // ---- SystemService ----

    pub(super) extern "sysv64" fn system_param(id: i32, value: *mut i32) -> i32 {
        // SAFETY: the guest passes null or its int.
        let Some(value) = (unsafe { value.as_mut() }) else {
            return SYSTEM_PARAMETER;
        };
        *value = match id {
            1 => language(env("BB_LANGUAGE").as_deref()),
            2 => 1, // date format DD/MM/YYYY
            3 => 1, // 24-hour clock
            4 => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_secs() as i64);
                (crate::time::utc_offset(now) / 60) as i32
            }
            5 => 0,    // summer time
            7 => 0,    // parental level off
            1000 => 1, // enter button = cross
            _ => crate::abi::libc::stop(&format!("STOP: unsupported system parameter {id}\n")),
        };
        0
    }

    extern "sysv64" fn system_status(status: *mut [u8; 12]) -> i32 {
        // SAFETY: the guest passes null or its 12-byte status.
        let Some(status) = (unsafe { status.as_mut() }) else {
            return SYSTEM_PARAMETER;
        };
        *status = [0; 12]; // no events, no overlay, foreground, normal CPU mode
        0
    }

    extern "sysv64" fn system_event(_event: *mut c_void) -> i32 {
        SYSTEM_NO_EVENT
    }

    extern "sysv64" fn hide_splash() -> i32 {
        note("SystemService: splash screen hidden");
        0
    }

    extern "sysv64" fn launch_browser() -> i32 {
        note("SystemService: web browser request ignored (offline)");
        0
    }

    /// The game replaces itself with another executable of its package (the collection menu
    /// starting another game): the guest path, then one argument per line, go to
    /// `BB_LOADEXEC_FILE`, and the process ends with exit code 101; `run.py` starts it.
    extern "sysv64" fn load_exec(path: *const c_char, argv: *const *const c_char) -> i32 {
        if path.is_null() {
            return SYSTEM_PARAMETER;
        }
        // SAFETY: the guest passes a NUL-terminated path.
        let path = unsafe { guest_str(path) };
        let mut arguments: Vec<&[u8]> = Vec::new();
        if !argv.is_null() {
            for i in 0..32 {
                // SAFETY: the guest's argument list ends with a null entry.
                let argument = unsafe { *argv.add(i) };
                if argument.is_null() {
                    break;
                }
                // SAFETY: each argument is a NUL-terminated string.
                arguments.push(unsafe { guest_str(argument) });
            }
        }
        let mut line = format!(
            "Runtime: SystemService: load executable {}",
            String::from_utf8_lossy(path)
        );
        for argument in &arguments {
            line.push_str(&format!(" \"{}\"", String::from_utf8_lossy(argument)));
        }
        c::out_line(&line);
        let mut request = path.to_vec();
        request.push(b'\n');
        for argument in &arguments {
            request.extend_from_slice(argument);
            request.push(b'\n');
        }
        let written = std::env::var_os("BB_LOADEXEC_FILE")
            .filter(|file| !file.is_empty())
            .is_some_and(|file| std::fs::write(file, &request).is_ok());
        if !written {
            crate::abi::libc::stop(
                "STOP: sceSystemServiceLoadExec needs BB_LOADEXEC_FILE (run.py sets it)\n",
            );
        }
        c::flush();
        #[cfg(windows)]
        {
            #[link(name = "user32")]
            extern "system" {
                fn AllowSetForegroundWindow(process: u32) -> i32;
            }
            // The next game's window may take the focus from this one (else Windows opens it
            // behind and it gets no input until clicked). SAFETY: ASFW_ANY, no pointers.
            unsafe { AllowSetForegroundWindow(u32::MAX) };
        }
        c::quit(LOADEXEC_EXIT)
    }

    // ---- Net / NetCtl / Ssl / Http: no network ----

    static NET_ERRNO: AtomicI32 = AtomicI32::new(0);

    pub(super) fn net_errno() -> i32 {
        NET_ERRNO.load(Ordering::Relaxed)
    }

    extern "sysv64" fn net_errno_loc() -> *mut i32 {
        NET_ERRNO.as_ptr()
    }

    extern "sysv64" fn new_object() -> i32 {
        new_id()
    }

    extern "sysv64" fn net_unreachable() -> i32 {
        NET_ERRNO.store(51, Ordering::Relaxed); // ENETUNREACH
        NET_ENETUNREACH
    }

    extern "sysv64" fn net_htons(value: u16) -> u16 {
        value.to_be()
    }

    extern "sysv64" fn net_htonl(value: u32) -> u32 {
        value.to_be()
    }

    /// `sceNetInetPton`: IPv4 only (`AF_INET` is 2); 1 for an address, 0 for anything else.
    extern "sysv64" fn net_pton(family: i32, source: *const c_char, out: *mut [u8; 4]) -> i32 {
        if family != 2 {
            NET_ERRNO.store(47, Ordering::Relaxed); // EAFNOSUPPORT
            return NET_EINVAL;
        }
        // SAFETY: the guest passes a NUL-terminated string.
        let text = unsafe { guest_str(source) };
        let Some(address) = std::str::from_utf8(text)
            .ok()
            .and_then(|text| text.parse::<std::net::Ipv4Addr>().ok())
        else {
            return 0;
        };
        // SAFETY: the guest passes its 4-byte `in_addr`.
        if let Some(out) = unsafe { out.as_mut() } {
            *out = address.octets();
        }
        1
    }

    /// `sceNetInetNtop`: IPv4 only; the dotted address in `out`, or null when it does not fit.
    extern "sysv64" fn net_ntop(
        family: i32,
        source: *const [u8; 4],
        out: *mut u8,
        size: u32,
    ) -> *const u8 {
        if family != 2 {
            NET_ERRNO.store(47, Ordering::Relaxed); // EAFNOSUPPORT
            return std::ptr::null();
        }
        // SAFETY: the guest passes its 4-byte `in_addr`.
        let Some(&octets) = (unsafe { source.as_ref() }) else {
            return std::ptr::null();
        };
        let text = std::net::Ipv4Addr::from(octets).to_string();
        if out.is_null() || text.len() + 1 > size as usize {
            NET_ERRNO.store(28, Ordering::Relaxed); // ENOSPC
            return std::ptr::null();
        }
        // SAFETY: the guest's buffer holds `size` bytes, enough for the text and its NUL.
        unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), out, text.len());
            *out.add(text.len()) = 0;
        }
        out
    }

    extern "sysv64" fn netctl_state(state: *mut i32) -> i32 {
        // SAFETY: the guest passes null or its int.
        let Some(state) = (unsafe { state.as_mut() }) else {
            return NET_CTL_INVALID_ADDR;
        };
        *state = 0; // disconnected
        0
    }

    extern "sysv64" fn netctl_info(_code: i32, _info: *mut c_void) -> i32 {
        NET_CTL_NOT_CONNECTED
    }

    extern "sysv64" fn netctl_register(
        _callback: *const c_void,
        _argument: *mut c_void,
        id: *mut i32,
    ) -> i32 {
        // SAFETY: the guest passes null or its int.
        let Some(id) = (unsafe { id.as_mut() }) else {
            return NET_CTL_INVALID_ADDR;
        };
        *id = new_id();
        0
    }

    extern "sysv64" fn netctl_nat(info: *mut [u32; 4]) -> i32 {
        // SAFETY: the guest passes null or its NAT info (size, then three words).
        let Some(info) = (unsafe { info.as_mut() }) else {
            return NET_CTL_INVALID_ADDR;
        };
        info[1] = 0; // STUN failed
        info[2] = 3; // NAT type 3
        info[3] = 0; // no mapped address
        0
    }

    extern "sysv64" fn http_fail() -> i32 {
        HTTP_NETWORK
    }

    extern "sysv64" fn http_epoll(_context: i32, handle: *mut usize) -> i32 {
        // SAFETY: the guest passes null or its handle.
        let Some(handle) = (unsafe { handle.as_mut() }) else {
            return HTTP_INVALID_VALUE;
        };
        *handle = 0x100 + new_id() as usize;
        0
    }

    /// No request is ever in flight: the wait sleeps its timeout (microseconds) and reports
    /// no events.
    extern "sysv64" fn http_wait(
        _handle: *mut c_void,
        _events: *mut c_void,
        _max: i32,
        timeout: i64,
    ) -> i32 {
        if timeout > 0 {
            crate::kernel::sleep_ns((timeout as u64).saturating_mul(1000));
        }
        0
    }

    // ---- NP (PSN): signed out ----

    extern "sysv64" fn np_state(_user: i32, state: *mut i32) -> i32 {
        // SAFETY: the guest passes null or its int.
        let Some(state) = (unsafe { state.as_mut() }) else {
            return NP_INVALID_ARGUMENT;
        };
        *state = 1; // SIGNED_OUT
        0
    }

    extern "sysv64" fn np_signed_out() -> i32 {
        NP_SIGNED_OUT
    }

    /// `BB_OFFLINE_NPID=1`: the player's NP ID and online ID are a local one ("Player") instead
    /// of "signed out". The games ask for the NP ID once after Continue or New Game and, without
    /// one, stop on a "friends' stats are not available" notice. Sign-in state stays signed out:
    /// nothing goes online.
    fn offline_npid() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| flag(std::env::var_os("BB_OFFLINE_NPID")))
    }

    /// Writes the local ID "Player" into `size` bytes at `out` (an `SceNpOnlineId` is 16
    /// characters, a terminator and 3 pad bytes; an `SceNpId` is that and 16 opaque bytes).
    fn local_id(out: *mut u8, size: usize) -> i32 {
        if !offline_npid() {
            return NP_SIGNED_OUT;
        }
        if out.is_null() {
            return NP_INVALID_ARGUMENT;
        }
        // SAFETY: the guest's ID holds `size` bytes.
        unsafe {
            std::ptr::write_bytes(out, 0, size);
            std::ptr::copy_nonoverlapping(b"Player".as_ptr(), out, 6);
        }
        0
    }

    extern "sysv64" fn np_get_npid(_user: i32, npid: *mut u8) -> i32 {
        local_id(npid, 36)
    }

    extern "sysv64" fn np_get_online_id(_user: i32, online_id: *mut u8) -> i32 {
        local_id(online_id, 20)
    }

    extern "sysv64" fn np_register_void() {}

    extern "sysv64" fn np_poll(_request: i32, result: *mut i32) -> i32 {
        // SAFETY: the guest passes null or its int.
        if let Some(result) = unsafe { result.as_mut() } {
            *result = NP_SIGNED_OUT;
        }
        0
    }

    extern "sysv64" fn np_compare(a: *const [u8; 16], b: *const [u8; 16]) -> i32 {
        // SAFETY: the guest passes null or its IDs (16 bytes compared).
        match unsafe { (a.as_ref(), b.as_ref()) } {
            (Some(a), Some(b)) if a == b => 0,
            (Some(_), Some(_)) => NP_UTIL_NOT_MATCH,
            _ => NP_INVALID_ARGUMENT,
        }
    }

    // ---- Voice chat: ports exist and carry no audio ----

    extern "sysv64" fn voice_port(_param: *const c_void, port: *mut u32) -> i32 {
        // SAFETY: the guest passes null or its port id.
        let Some(port) = (unsafe { port.as_mut() }) else {
            return VOICE_INVALID_ARGUMENT;
        };
        *port = new_id() as u32;
        0
    }

    extern "sysv64" fn voice_read(_port: u32, _data: *mut c_void, size: *mut u32) -> i32 {
        // SAFETY: the guest passes null or its size.
        if let Some(size) = unsafe { size.as_mut() } {
            *size = 0;
        }
        0
    }

    extern "sysv64" fn voice_info(_port: u32, info: *mut [u32; 10]) -> i32 {
        // SAFETY: the guest passes null or its 40-byte port info.
        let Some(info) = (unsafe { info.as_mut() }) else {
            return VOICE_INVALID_ARGUMENT;
        };
        *info = [0; 10]; // type, state unconnected, no bytes available
        0
    }

    // ---- Common dialogs: nothing is displayed; an opened dialog finishes ----

    const DIALOG_NAMES: [&str; 8] = [
        "CommonDialog",
        "MsgDialog",
        "SaveDataDialog",
        "NpProfileDialog",
        "NpCommerceDialog",
        "ImeDialog",
        "ErrorDialog",
        "InvitationDialog",
    ];
    const SAVE: usize = 2;
    const ERROR: usize = 6;
    const INVITATION: usize = 7;

    #[derive(Clone, Copy)]
    struct Dialog {
        initialized: bool,
        /// None 0, initialized 1, running 2, finished 3.
        status: i32,
    }

    static DIALOGS: Mutex<[Dialog; 8]> = Mutex::new(
        [Dialog {
            initialized: false,
            status: 0,
        }; 8],
    );

    fn dialog_init(i: usize) -> i32 {
        let mut dialogs = lock(&DIALOGS);
        if dialogs[i].initialized {
            return DIALOG_ALREADY_INITIALIZED;
        }
        dialogs[i] = Dialog {
            initialized: true,
            status: 1,
        };
        0
    }

    fn dialog_open(i: usize) -> i32 {
        let mut dialogs = lock(&DIALOGS);
        if !dialogs[i].initialized {
            return DIALOG_NOT_INITIALIZED;
        }
        c::out_line(&format!(
            "Runtime: {} opened; completed immediately (no dialog UI yet)",
            DIALOG_NAMES[i]
        ));
        dialogs[i].status = 3;
        0
    }

    fn dialog_status(i: usize) -> i32 {
        lock(&DIALOGS)[i].status
    }

    fn dialog_term(i: usize) -> i32 {
        let mut dialogs = lock(&DIALOGS);
        if !dialogs[i].initialized {
            return DIALOG_NOT_INITIALIZED;
        }
        dialogs[i] = Dialog {
            initialized: false,
            status: 0,
        };
        0
    }

    /// The message (1), profile (3) and commerce (4) dialogs.
    extern "sysv64" fn plain_init<const I: usize>() -> i32 {
        dialog_init(I)
    }

    extern "sysv64" fn plain_open<const I: usize>(_param: *const c_void) -> i32 {
        dialog_open(I)
    }

    extern "sysv64" fn plain_status<const I: usize>() -> i32 {
        dialog_status(I)
    }

    extern "sysv64" fn plain_term<const I: usize>() -> i32 {
        dialog_term(I)
    }

    extern "sysv64" fn profile_result(result: *mut [u8; 4]) -> i32 {
        // SAFETY: the guest passes null or its result.
        if let Some(result) = unsafe { result.as_mut() } {
            *result = [0; 4];
        }
        0
    }

    /// ErrorDialog param {size, errorCode, userId, reserved}: the code is what the player
    /// would see.
    extern "sysv64" fn error_open(param: *const [i32; 4]) -> i32 {
        // SAFETY: the guest passes null or its param.
        let Some(param) = (unsafe { param.as_ref() }) else {
            return DIALOG_ARG_NULL;
        };
        c::out_line(&format!(
            "Runtime: ErrorDialog error code 0x{:08x}",
            param[1] as u32
        ));
        dialog_open(ERROR)
    }

    extern "sysv64" fn invitation_term() -> i32 {
        lock(&DIALOGS)[INVITATION] = Dialog {
            initialized: false,
            status: 0,
        };
        0
    }

    // ---- SaveDataDialog ----
    //
    // Dialogs that only wait (a progress bar, a message without buttons, the "progress" system
    // message) stay running until the game closes them, as on the console; the others are
    // answered OK/Yes when the game next asks for the status. OrbisSaveDataDialogParam:
    // BaseParam (48), size, mode (52), dispType (56), pad, animParam, items (72), userMsgParam
    // (80), sysMsgParam (88), errorCodeParam, progressBarParam, userData (112), optionParam.

    const SAVE_LIST: u32 = 1;
    const SAVE_USER_MSG: u32 = 2;
    const SAVE_SYSTEM_MSG: u32 = 3;
    const SAVE_PROGRESS_BAR: u32 = 5;
    /// `OrbisSaveDataParam`.
    const SAVE_PARAM_SIZE: usize = 1328;

    struct SaveDialog {
        mode: u32,
        waits: bool,
        /// 1 an existing slot, 0 the new save entry (empty name: the game names it), -1
        /// cancelled.
        pick: i32,
        user_data: usize,
        dir: [u8; 32],
        param: [u8; SAVE_PARAM_SIZE],
    }

    static SAVE_DIALOG: Mutex<SaveDialog> = Mutex::new(SaveDialog {
        mode: 0,
        waits: false,
        pick: 0,
        user_data: 0,
        dir: [0; 32],
        param: [0; SAVE_PARAM_SIZE],
    });

    /// The slot picker of `runtime_savedata.c` (loader build): `which` 0 first slot, 1 last,
    /// 2 first, 3 last slot, 4 latest, 5 oldest... (see `save_list`).
    #[cfg(uncharted_loader)]
    fn savedata_pick(
        user: i32,
        title: *const c_char,
        names: *const [u8; 32],
        count: u32,
        which: i32,
        focus: *const c_char,
        out: &mut [u8; 32],
        param_out: &mut [u8; SAVE_PARAM_SIZE],
    ) -> i32 {
        extern "C" {
            fn runtime_savedata_pick(
                user: i32,
                title: *const c_char,
                names: *const [u8; 32],
                count: u32,
                which: i32,
                focus: *const c_char,
                out: *mut u8,
                param_out: *mut c_void,
            ) -> i32;
        }
        // SAFETY: the guest's pointers passed through; `out` and `param_out` are live buffers of
        // the sizes the C function writes.
        unsafe {
            runtime_savedata_pick(
                user,
                title,
                names,
                count,
                which,
                focus,
                out.as_mut_ptr(),
                param_out.as_mut_ptr().cast(),
            )
        }
    }

    /// Without the loader there is no save store to pick from: nothing to choose.
    #[cfg(not(uncharted_loader))]
    #[allow(clippy::too_many_arguments)]
    fn savedata_pick(
        _user: i32,
        _title: *const c_char,
        _names: *const [u8; 32],
        _count: u32,
        _which: i32,
        _focus: *const c_char,
        _out: &mut [u8; 32],
        _param_out: &mut [u8; SAVE_PARAM_SIZE],
    ) -> i32 {
        -1
    }

    /// List mode. SaveDialogItems {userId, pad, titleId* (8), dirName* (16), dirNameNum (24),
    /// pad, newItem* (32), focusPos (40), pad, focusPosDirName* (48), ...}; dispType 1 save,
    /// 2 load, 3 delete. The entry the dialog focuses is taken, as when the player confirms at
    /// once: focusPos 0 list head (the new save entry comes first), 1 list tail, 2 first slot,
    /// 3 last slot, 4 latest, 5 oldest, 6 named.
    ///
    /// # Safety
    /// `param` is the guest's dialog param.
    unsafe fn save_list(save: &mut SaveDialog, param: *const u8) {
        // SAFETY: the dialog param is at least 120 bytes (caller).
        let (kind, items): (u32, *const u8) = unsafe { (field(param, 56), field(param, 72)) };
        save.pick = -1;
        save.dir = [0; 32];
        if items.is_null() || kind == 3 {
            return;
        }
        // SAFETY: `items` is the guest's SaveDialogItems.
        let (user, title, names, count, new_item, focus_pos, focus): (
            i32,
            *const c_char,
            *const [u8; 32],
            u32,
            *const c_void,
            u32,
            *const c_char,
        ) = unsafe {
            (
                field(items, 0),
                field(items, 8),
                field(items, 16),
                field(items, 24),
                field(items, 32),
                field(items, 40),
                field(items, 48),
            )
        };
        let offer_new = kind == 1 && !new_item.is_null();
        const WHICH: [i32; 7] = [0, 1, 0, 1, 2, 3, 4];
        save.pick = if focus_pos == 0 && offer_new {
            0
        } else {
            let which = WHICH.get(focus_pos as usize).copied().unwrap_or(2);
            savedata_pick(
                user,
                title,
                names,
                count,
                which,
                focus,
                &mut save.dir,
                &mut save.param,
            )
        };
        if save.pick < 0 && offer_new {
            save.pick = 0;
        }
        let dir_len = save.dir.iter().position(|&b| b == 0).unwrap_or(32);
        c::out_line(&format!(
            "Runtime: SaveDataDialog list ({}, {count} slots, focus {focus_pos}): {}{}",
            if kind == 1 { "save" } else { "load" },
            match save.pick {
                ..=-1 => "nothing to choose, cancelled",
                0 => "new save data",
                _ => "chose ",
            },
            if save.pick > 0 {
                String::from_utf8_lossy(&save.dir[..dir_len]).into_owned()
            } else {
                String::new()
            }
        ));
    }

    /// `BB_SAVE_LOG=1`: the dialog's calls on stdout, for diagnosis (as in `runtime_savedata.c`).
    fn save_log() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| flag(std::env::var_os("BB_SAVE_LOG")))
    }

    extern "sysv64" fn save_init() -> i32 {
        dialog_init(SAVE)
    }

    extern "sysv64" fn save_term() -> i32 {
        dialog_term(SAVE)
    }

    pub(super) extern "sysv64" fn save_open(param: *const u8) -> i32 {
        let mut dialogs = lock(&DIALOGS);
        if !dialogs[SAVE].initialized {
            return DIALOG_NOT_INITIALIZED;
        }
        if param.is_null() {
            return DIALOG_ARG_NULL;
        }
        let mut save = lock(&SAVE_DIALOG);
        if save_log() && dialogs[SAVE].status == 2 {
            c::out_line(&format!(
                "Runtime: SaveLog Open while RUNNING (previous mode {}, waits {}); accepted",
                save.mode,
                i32::from(save.waits)
            ));
        }
        // SAFETY: the guest's dialog param (at least 120 bytes).
        let (mode, user_msg, sys_msg, user_data): (u32, *const u32, *const u32, usize) = unsafe {
            (
                field(param, 52),
                field(param, 80),
                field(param, 88),
                field(param, 112),
            )
        };
        let mut kind = 0u32;
        let mut buttons = 0u32;
        if mode == SAVE_USER_MSG && !user_msg.is_null() {
            // SAFETY: the user message param starts with its button type.
            buttons = unsafe { user_msg.read_unaligned() }; // OK, YESNO, NONE, OKCANCEL
        }
        if mode == SAVE_SYSTEM_MSG && !sys_msg.is_null() {
            // SAFETY: the system message param starts with its type.
            kind = unsafe { sys_msg.read_unaligned() }; // NODATA, CONFIRM, ... PROGRESS=5
        }
        save.mode = mode;
        save.user_data = user_data;
        save.waits = mode == SAVE_PROGRESS_BAR
            || (mode == SAVE_USER_MSG && buttons == 2)
            || (mode == SAVE_SYSTEM_MSG && kind == 5);
        if mode == SAVE_LIST {
            // SAFETY: the guest's dialog param.
            unsafe { save_list(&mut save, param) };
        } else {
            c::out_line(&format!(
                "Runtime: SaveDataDialog mode {mode} (message type {}){}",
                if mode == SAVE_SYSTEM_MSG {
                    kind
                } else {
                    buttons
                },
                if save.waits {
                    ": shown until the game closes it"
                } else {
                    ": answered OK/Yes"
                }
            ));
        }
        dialogs[SAVE].status = 2; // RUNNING
        if save_log() {
            c::out_line(&format!(
                "Runtime: SaveLog Open mode {mode} -> status RUNNING"
            ));
        }
        0
    }

    pub(super) extern "sysv64" fn save_status() -> i32 {
        let mut dialogs = lock(&DIALOGS);
        if dialogs[SAVE].status == 2 && !lock(&SAVE_DIALOG).waits {
            dialogs[SAVE].status = 3;
        }
        dialogs[SAVE].status
    }

    /// Result {mode, result, buttonId, pad, dirName* (16), param* (24), userData* (32)}: OK /
    /// Yes, or USER_CANCELED for a list with nothing to choose. The output buffers the game
    /// passes in (dirName, param) are filled for a pick; the pad is left alone.
    pub(super) extern "sysv64" fn save_result(result: *mut u8) -> i32 {
        if result.is_null() {
            return DIALOG_ARG_NULL;
        }
        let status = dialog_status(SAVE);
        if status != 3 {
            if save_log() {
                c::out_line(&format!(
                    "Runtime: SaveLog GetResult -> 0x80B80005 NOT_FINISHED (status {status})"
                ));
            }
            return DIALOG_NOT_FINISHED;
        }
        let save = lock(&SAVE_DIALOG);
        let cancelled = save.mode == SAVE_LIST && save.pick < 0;
        let (outcome, button) = if cancelled { (1u32, 0u32) } else { (0, 1) };
        // SAFETY: the guest's result structure (40 bytes).
        unsafe {
            set_field(result, 0, save.mode);
            set_field(result, 4, outcome);
            set_field(result, 8, button);
            set_field(result, 32, save.user_data);
        }
        if save.mode == SAVE_LIST && !cancelled {
            // SAFETY: as above.
            let (dir, out_param): (*mut [u8; 32], *mut [u8; SAVE_PARAM_SIZE]) =
                unsafe { (field(result, 16), field(result, 24)) };
            // SAFETY: the game's dirName buffer (32 bytes) and save param, or null.
            unsafe {
                if let Some(dir) = dir.as_mut() {
                    *dir = save.dir;
                }
                if save.pick > 0 {
                    if let Some(out_param) = out_param.as_mut() {
                        *out_param = save.param;
                    }
                }
            }
            if save_log() {
                let dir_len = save.dir.iter().position(|&b| b == 0).unwrap_or(32);
                c::out_line(&format!(
                    "Runtime: SaveLog GetResult list: dirName ptr {} (wrote '{}'), param ptr {} ({})",
                    if dir.is_null() { "NULL" } else { "set" },
                    if dir.is_null() {
                        String::new()
                    } else {
                        String::from_utf8_lossy(&save.dir[..dir_len]).into_owned()
                    },
                    if out_param.is_null() { "NULL" } else { "set" },
                    if !out_param.is_null() && save.pick > 0 {
                        "filled"
                    } else {
                        "left alone"
                    }
                ));
            }
        }
        if save_log() {
            c::out_line(&format!(
                "Runtime: SaveLog GetResult -> 0: wrote mode {} result {outcome} button {button} userData {:#x}; pad (offset 12) left alone",
                save.mode, save.user_data
            ));
        }
        0
    }

    extern "sysv64" fn save_ready() -> i32 {
        1
    }

    extern "sysv64" fn save_progress(target: i32, rate: u32) -> i32 {
        if save_log() {
            c::out_line(&format!(
                "Runtime: SaveLog ProgressBarSetValue target {target} value {rate} (status {}, mode {})",
                dialog_status(SAVE),
                lock(&SAVE_DIALOG).mode
            ));
        }
        0
    }

    pub(super) extern "sysv64" fn save_close(_param: *const c_void) -> i32 {
        let mut dialogs = lock(&DIALOGS);
        if save_log() {
            c::out_line(&format!(
                "Runtime: SaveLog Close (status before {}, mode {}) -> 0",
                dialogs[SAVE].status,
                lock(&SAVE_DIALOG).mode
            ));
        }
        if dialogs[SAVE].status == 2 {
            dialogs[SAVE].status = 3;
        }
        0
    }

    // ---- ImeDialog: text typed on the keyboard into the game window ----

    /// `OrbisImeDialogParam`: user, type, languages, enter label, method, filter, option, max
    /// length, the char16 buffer, position, alignment, placeholder, title.
    #[repr(C)]
    pub(super) struct ImeParam {
        pub(super) user: i32,
        pub(super) kind: u32,
        pub(super) languages: u64,
        pub(super) enter_label: u32,
        pub(super) input_method: u32,
        pub(super) filter: *const c_void,
        pub(super) option: u32,
        pub(super) max_length: u32,
        pub(super) buffer: *mut u16,
        pub(super) x: f32,
        pub(super) y: f32,
        pub(super) horizontal_alignment: u32,
        pub(super) vertical_alignment: u32,
        pub(super) placeholder: *const u16,
        pub(super) title: *const u16,
        pub(super) reserved: [i8; 16],
    }

    const _: () = assert!(std::mem::size_of::<ImeParam>() == 96);

    struct Ime {
        running: bool,
        finished: bool,
        end_status: u32,
        buffer: usize,
        max_length: u32,
    }

    static IME: Mutex<Ime> = Mutex::new(Ime {
        running: false,
        finished: false,
        end_status: 0,
        buffer: 0,
        max_length: 0,
    });

    /// A guest UTF-16 string, up to its NUL or `limit` units.
    ///
    /// # Safety
    /// `text` is null or readable up to its NUL or `limit` units.
    unsafe fn guest_utf16(text: *const u16, limit: usize) -> Vec<u16> {
        let mut units = Vec::new();
        if text.is_null() {
            return units;
        }
        for i in 0..limit {
            // SAFETY: inside the string (caller).
            let unit = unsafe { *text.add(i) };
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        units
    }

    /// The text input box of the GPU library (loader build): 0 when it cannot be shown.
    #[cfg(uncharted_loader)]
    fn text_input_begin(initial: &[u8], prompt: &[u8]) -> i32 {
        extern "C" {
            fn bbgpu_text_input_begin(initial: *const c_char, prompt: *const c_char) -> i32;
        }
        let initial = std::ffi::CString::new(initial).unwrap_or_default();
        let prompt = std::ffi::CString::new(prompt).unwrap_or_default();
        // SAFETY: two NUL-terminated strings, copied by the callee.
        unsafe { bbgpu_text_input_begin(initial.as_ptr(), prompt.as_ptr()) }
    }

    /// The text input box (loader build): 0 still open, 1 accepted, 2 cancelled; the text in
    /// `out`.
    #[cfg(uncharted_loader)]
    fn text_input_poll(out: &mut Vec<u8>) -> i32 {
        extern "C" {
            fn bbgpu_text_input_poll(out: *mut c_char, size: u64) -> i32;
        }
        let mut text = [0u8; 512];
        // SAFETY: a live buffer of the size passed.
        let state = unsafe { bbgpu_text_input_poll(text.as_mut_ptr().cast(), text.len() as u64) };
        let len = text.iter().position(|&b| b == 0).unwrap_or(text.len());
        *out = text[..len].to_vec();
        state
    }

    /// Without the GPU library no box can be shown: the start text is accepted at once.
    #[cfg(not(uncharted_loader))]
    fn text_input_begin(_initial: &[u8], _prompt: &[u8]) -> i32 {
        0
    }

    #[cfg(not(uncharted_loader))]
    fn text_input_poll(_out: &mut Vec<u8>) -> i32 {
        0
    }

    fn ime_complete(ime: &mut Ime, end_status: u32, text: &[u8]) {
        if end_status == 0 && ime.buffer != 0 && ime.max_length != 0 {
            let max = ime.max_length as usize;
            let units = utf8_to_utf16(text, max);
            let buffer = ime.buffer as *mut u16;
            // SAFETY: the guest's text buffer from `sceImeDialogInit`, `max_length` units and
            // its terminator.
            unsafe {
                std::ptr::copy_nonoverlapping(units.as_ptr(), buffer, units.len());
                *buffer.add(units.len()) = 0;
            }
        }
        ime.end_status = end_status;
        ime.finished = true;
        ime.running = false;
        if end_status != 0 {
            note("ImeDialog cancelled");
        } else {
            note(&format!("ImeDialog text: {}", String::from_utf8_lossy(text)));
        }
    }

    pub(super) extern "sysv64" fn ime_init(param: *const ImeParam, _extended: *const c_void) -> i32 {
        // SAFETY: the guest passes null or its param.
        let Some(param) = (unsafe { param.as_ref() }) else {
            return IME_INVALID_ADDRESS;
        };
        if param.buffer.is_null() || param.max_length == 0 {
            return IME_INVALID_ADDRESS;
        }
        let mut ime = lock(&IME);
        if ime.running {
            return IME_BUSY;
        }
        *ime = Ime {
            running: true,
            finished: false,
            end_status: 0,
            buffer: param.buffer as usize,
            max_length: param.max_length,
        };
        // SAFETY: the guest's buffer holds `max_length` units; the title is null or a string.
        let (initial, prompt) = unsafe {
            (
                utf16_to_utf8(&guest_utf16(param.buffer, param.max_length as usize), 512),
                utf16_to_utf8(&guest_utf16(param.title, 128), 256),
            )
        };
        if let Some(preset) = std::env::var_os("BB_IME_TEXT") {
            ime_complete(&mut ime, 0, preset.to_string_lossy().as_bytes());
            return 0;
        }
        // An empty field starts with the player name (launcher) or "Hunter": a controller alone
        // can accept it (the port has no on-screen keyboard).
        let name = env("BB_USER_NAME").filter(|name| !name.is_empty());
        let start: Vec<u8> = if !initial.is_empty() {
            initial
        } else {
            name.unwrap_or_else(|| "Hunter".into()).into_bytes()
        };
        let prompt: &[u8] = if prompt.is_empty() { b"Text" } else { &prompt };
        if text_input_begin(&start, prompt) == 0 {
            ime_complete(&mut ime, 0, &start);
        } else {
            note("ImeDialog opened: shown on screen; keyboard Enter or Cross accepts, Esc or Circle cancels");
        }
        0
    }

    pub(super) extern "sysv64" fn ime_status() -> i32 {
        let mut ime = lock(&IME);
        if ime.running {
            let mut text = Vec::new();
            let state = text_input_poll(&mut text);
            if state != 0 {
                ime_complete(&mut ime, u32::from(state != 1), &text);
            }
        }
        if ime.running {
            1 // Running
        } else if ime.finished {
            2 // Finished
        } else {
            0 // None
        }
    }

    pub(super) extern "sysv64" fn ime_result(result: *mut u32) -> i32 {
        let ime = lock(&IME);
        if !ime.finished {
            return IME_NOT_FINISHED;
        }
        // SAFETY: the guest passes null or its result (the end status first).
        if let Some(result) = unsafe { result.as_mut() } {
            *result = ime.end_status;
        }
        0
    }

    pub(super) extern "sysv64" fn ime_term() -> i32 {
        *lock(&IME) = Ime {
            running: false,
            finished: false,
            end_status: 0,
            buffer: 0,
            max_length: 0,
        };
        0
    }

    // ---- PlayGo: a fully installed package ----

    static PLAYGO_HANDLE: AtomicI32 = AtomicI32::new(0);
    static PLAYGO_CHUNKS: AtomicI32 = AtomicI32::new(-1);
    static PLAYGO_LANGUAGES: AtomicU64 = AtomicU64::new(u64::MAX);

    /// The chunk count from the header of the package's `playgo-chunk.dat` (bytes 10 and 11),
    /// read through the file module (loader build); `None` without one.
    #[cfg(uncharted_loader)]
    fn playgo_chunk_count() -> Option<i32> {
        extern "C" {
            fn runtime_file_open(path: *const c_char, flags: i32, mode: i32) -> i64;
            fn runtime_file_read(fd: i32, buffer: *mut c_void, size: u64) -> i64;
            fn runtime_file_close(fd: i32) -> i64;
        }
        // SAFETY: a NUL-terminated guest path, read-only.
        let fd = unsafe { runtime_file_open(c"/app0/sce_sys/playgo-chunk.dat".as_ptr(), 0, 0) } as i32;
        if fd < 0 {
            return None;
        }
        let mut header = [0u8; 12];
        // SAFETY: a live buffer of the size passed; `fd` was just opened.
        let read = unsafe { runtime_file_read(fd, header.as_mut_ptr().cast(), 12) };
        // SAFETY: closes the file opened above.
        unsafe { runtime_file_close(fd) };
        (read == 12).then(|| i32::from(header[10]) | i32::from(header[11]) << 8)
    }

    /// Without the file module there is no package to read.
    #[cfg(not(uncharted_loader))]
    fn playgo_chunk_count() -> Option<i32> {
        None
    }

    pub(super) extern "sysv64" fn playgo_init(_params: *const c_void) -> i32 {
        let chunks = playgo_chunk_count().unwrap_or(1);
        PLAYGO_CHUNKS.store(chunks, Ordering::Release);
        note(&format!(
            "PlayGo initialized; {chunks} chunks, all installed locally"
        ));
        0
    }

    extern "sysv64" fn playgo_open(handle: *mut i32, _param: *const c_void) -> i32 {
        // SAFETY: the guest passes null or its handle.
        let Some(handle) = (unsafe { handle.as_mut() }) else {
            return PLAYGO_BAD_POINTER;
        };
        PLAYGO_HANDLE.store(1, Ordering::Release);
        *handle = 1;
        0
    }

    fn playgo_check(handle: i32) -> bool {
        handle != 0 && handle == PLAYGO_HANDLE.load(Ordering::Acquire)
    }

    pub(super) extern "sysv64" fn playgo_chunk_ids(
        handle: i32,
        ids: *mut u16,
        count: u32,
        out: *mut u32,
    ) -> i32 {
        if !playgo_check(handle) {
            return PLAYGO_BAD_HANDLE;
        }
        if out.is_null() {
            return PLAYGO_BAD_POINTER;
        }
        if !ids.is_null() && count == 0 {
            return PLAYGO_BAD_SIZE;
        }
        let chunks = PLAYGO_CHUNKS.load(Ordering::Acquire) as u32;
        let n = if ids.is_null() {
            chunks
        } else {
            let n = count.min(chunks);
            for i in 0..n {
                // SAFETY: the guest's list holds `count` ids, `n <= count`.
                unsafe { *ids.add(i as usize) = i as u16 };
            }
            n
        };
        // SAFETY: the guest's count (checked non-null).
        unsafe { *out = n };
        0
    }

    pub(super) extern "sysv64" fn playgo_locus(
        handle: i32,
        ids: *const u16,
        count: u32,
        loci: *mut i8,
    ) -> i32 {
        if !playgo_check(handle) {
            return PLAYGO_BAD_HANDLE;
        }
        if ids.is_null() || loci.is_null() {
            return PLAYGO_BAD_POINTER;
        }
        if count == 0 {
            return PLAYGO_BAD_SIZE;
        }
        let chunks = PLAYGO_CHUNKS.load(Ordering::Acquire);
        for i in 0..count as usize {
            // SAFETY: the guest's lists hold `count` entries each.
            unsafe {
                if i32::from(*ids.add(i)) >= chunks {
                    return PLAYGO_BAD_CHUNK;
                }
                *loci.add(i) = 3; // LocalFast
            }
        }
        0
    }

    extern "sysv64" fn playgo_speed(handle: i32, _speed: i32) -> i32 {
        if playgo_check(handle) {
            0
        } else {
            PLAYGO_BAD_HANDLE
        }
    }

    extern "sysv64" fn playgo_get_speed(handle: i32, speed: *mut i32) -> i32 {
        if !playgo_check(handle) {
            return PLAYGO_BAD_HANDLE;
        }
        // SAFETY: the guest passes null or its int.
        let Some(speed) = (unsafe { speed.as_mut() }) else {
            return PLAYGO_BAD_POINTER;
        };
        *speed = 2; // Full
        0
    }

    extern "sysv64" fn playgo_eta(handle: i32, ids: *const u16, count: u32, eta: *mut i64) -> i32 {
        if !playgo_check(handle) {
            return PLAYGO_BAD_HANDLE;
        }
        if ids.is_null() || eta.is_null() {
            return PLAYGO_BAD_POINTER;
        }
        if count == 0 {
            return PLAYGO_BAD_SIZE;
        }
        // SAFETY: the guest's ETA (checked non-null).
        unsafe { *eta = 0 };
        0
    }

    /// Progress of installed chunks: {progress, total}, both the chunk count asked about
    /// (sizes are not tracked).
    extern "sysv64" fn playgo_progress(
        handle: i32,
        ids: *const u16,
        count: u32,
        progress: *mut [u64; 2],
    ) -> i32 {
        if !playgo_check(handle) {
            return PLAYGO_BAD_HANDLE;
        }
        if ids.is_null() || progress.is_null() {
            return PLAYGO_BAD_POINTER;
        }
        if count == 0 {
            return PLAYGO_BAD_SIZE;
        }
        // SAFETY: the guest's progress (checked non-null).
        unsafe { *progress = [u64::from(count); 2] };
        0
    }

    extern "sysv64" fn playgo_todo(
        handle: i32,
        list: *mut c_void,
        count: u32,
        entries: *mut u32,
    ) -> i32 {
        if !playgo_check(handle) {
            return PLAYGO_BAD_HANDLE;
        }
        if list.is_null() || entries.is_null() {
            return PLAYGO_BAD_POINTER;
        }
        if count == 0 {
            return PLAYGO_BAD_SIZE;
        }
        // SAFETY: the guest's count (checked non-null).
        unsafe { *entries = 0 }; // nothing left to install
        0
    }

    extern "sysv64" fn playgo_prefetch(handle: i32, ids: *const u16, count: u32) -> i32 {
        if !playgo_check(handle) {
            return PLAYGO_BAD_HANDLE;
        }
        if ids.is_null() {
            return PLAYGO_BAD_POINTER;
        }
        if count == 0 {
            PLAYGO_BAD_SIZE
        } else {
            0
        }
    }

    extern "sysv64" fn playgo_get_languages(handle: i32, mask: *mut u64) -> i32 {
        if !playgo_check(handle) {
            return PLAYGO_BAD_HANDLE;
        }
        // SAFETY: the guest passes null or its mask.
        let Some(mask) = (unsafe { mask.as_mut() }) else {
            return PLAYGO_BAD_POINTER;
        };
        *mask = PLAYGO_LANGUAGES.load(Ordering::Acquire);
        0
    }

    extern "sysv64" fn playgo_set_languages(handle: i32, mask: u64) -> i32 {
        if !playgo_check(handle) {
            return PLAYGO_BAD_HANDLE;
        }
        PLAYGO_LANGUAGES.store(mask, Ordering::Release);
        0
    }

    // ---- AppContent: no add-ons installed; download data has room ----

    extern "sysv64" fn app_content_not_found() -> i32 {
        APP_CONTENT_NOT_FOUND
    }

    extern "sysv64" fn download_space(_mount: *const c_char, kb: *mut u64) -> i32 {
        // SAFETY: the guest passes null or its size.
        let Some(kb) = (unsafe { kb.as_mut() }) else {
            return APP_CONTENT_PARAMETER;
        };
        *kb = 1_048_576;
        0
    }

    // ---- DiscMap: the game is fully installed, no disc bitmap exists ----

    extern "sysv64" fn disc_map_no_bitmap() -> i32 {
        DISC_MAP_NO_BITMAP
    }

    // ---- Absent devices: a mouse port opens but never reports a device; no microphone ----

    extern "sysv64" fn mouse_read(_handle: i32, data: *mut [u8; 40], count: i32) -> i32 {
        if count < 1 {
            return MOUSE_INVALID_ARGUMENT;
        }
        // SAFETY: the guest passes null or at least one 40-byte mouse record.
        let Some(data) = (unsafe { data.as_mut() }) else {
            return MOUSE_INVALID_ARGUMENT;
        };
        *data = [0; 40]; // timestamp 0, not connected
        1
    }

    extern "sysv64" fn audio_in_not_opened() -> i32 {
        AUDIO_IN_NOT_OPENED
    }

    /// The function behind the symbol name `symbol`, or `None`.
    pub(super) fn by_symbol(symbol: &str) -> Option<usize> {
        let address = match symbol {
            "sceUserServiceInitialize" => user_initialize as *const () as usize,
            "sceUserServiceGetInitialUser" => user_initial as *const () as usize,
            "sceUserServiceGetLoginUserIdList" => user_list as *const () as usize,
            "sceUserServiceGetUserName" => user_name as *const () as usize,
            "sceUserServiceGetEvent" => user_event as *const () as usize,
            "sceSystemServiceParamGetInt" => system_param as *const () as usize,
            "sceSystemServiceGetStatus" => system_status as *const () as usize,
            "sceSystemServiceReceiveEvent" => system_event as *const () as usize,
            "sceSystemServiceHideSplashScreen" => hide_splash as *const () as usize,
            "sceSystemServiceLaunchWebBrowser" => launch_browser as *const () as usize,
            "sceSystemServiceLoadExec" => load_exec as *const () as usize,
            "sceNetErrnoLoc" => net_errno_loc as *const () as usize,
            "sceNetPoolCreate"
            | "sceNetEpollCreate"
            | "sceNetResolverCreate"
            | "sceSslInit"
            | "sceHttpInit"
            | "sceHttpCreateTemplate"
            | "sceHttpCreateConnectionWithURL"
            | "sceHttpCreateRequestWithURL"
            | "sceNpRegisterStateCallback"
            | "sceNpRegisterPlusEventCallback"
            | "sceNpCreateAsyncRequest"
            | "sceNpAuthCreateAsyncRequest"
            | "sceNpLookupCreateTitleCtx"
            | "sceNpLookupCreateAsyncRequest"
            | "sceNpScoreCreateNpTitleCtx"
            | "sceNpScoreCreateRequest"
            | "sceNpWebApiInitialize"
            | "sceMouseOpen" => new_object as *const () as usize,
            "sceNetHtons" | "sceNetNtohs" => net_htons as *const () as usize,
            "sceNetHtonl" | "sceNetNtohl" => net_htonl as *const () as usize,
            "sceNetInetPton" => net_pton as *const () as usize,
            "sceNetInetNtop" => net_ntop as *const () as usize,
            "sceNetSocket" | "sceNetConnect" | "sceNetBind" | "sceNetListen" | "sceNetAccept"
            | "sceNetSend" | "sceNetSendto" | "sceNetRecv" | "sceNetRecvfrom"
            | "sceNetSetsockopt" | "sceNetGetsockopt" | "sceNetGetsockname"
            | "sceNetShutdown" | "sceNetSocketClose" | "sceNetSocketAbort"
            | "sceNetEpollControl" | "sceNetEpollWait" | "sceNetEpollAbort"
            | "sceNetResolverStartNtoa" | "sceNetResolverStartAton" => {
                net_unreachable as *const () as usize
            }
            "sceNetCtlGetState" => netctl_state as *const () as usize,
            "sceNetCtlGetInfo" => netctl_info as *const () as usize,
            "sceNetCtlRegisterCallback" => netctl_register as *const () as usize,
            "sceNetCtlGetNatInfo" => netctl_nat as *const () as usize,
            "sceHttpSendRequest"
            | "sceHttpGetStatusCode"
            | "sceHttpGetResponseContentLength"
            | "sceHttpReadData" => http_fail as *const () as usize,
            "sceHttpCreateEpoll" => http_epoll as *const () as usize,
            "sceHttpWaitRequest" => http_wait as *const () as usize,
            "sceNpGetState" => np_state as *const () as usize,
            "sceNpGetOnlineId" => np_get_online_id as *const () as usize,
            "sceNpGetNpId" => np_get_npid as *const () as usize,
            "sceNpRegisterGamePresenceCallback" => np_register_void as *const () as usize,
            "sceNpPollAsync" | "sceNpAuthPollAsync" | "sceNpLookupPollAsync" => {
                np_poll as *const () as usize
            }
            "sceNpCmpNpId" | "sceNpCmpOnlineId" => np_compare as *const () as usize,
            "sceNpCheckNpAvailability"
            | "sceNpGetParentalControlInfo"
            | "sceNpCheckPlus"
            | "sceNpGetGamePresenceStatus"
            | "sceNpAuthGetAuthorizationCode"
            | "sceNpLookupNpId"
            | "sceNpWebApiCreateContext"
            | "sceNpWebApiCreateRequest"
            | "sceNpWebApiSendRequest"
            | "sceNpWebApiReadData"
            | "sceNpWebApiGetHttpStatusCode"
            | "sceNpWebApiGetHttpResponseHeaderValue"
            | "sceNpWebApiGetHttpResponseHeaderValueLength"
            | "sceNpWebApiCreatePushEventFilter"
            | "sceNpWebApiRegisterPushEventCallback"
            | "sceNpWebApiUtilityParseNpId"
            | "sceNpMatching2ContextStart"
            | "sceNpMatching2CreateJoinRoom"
            | "sceNpMatching2JoinRoom"
            | "sceNpMatching2LeaveRoom"
            | "sceNpMatching2SearchRoom"
            | "sceNpMatching2GetServerId"
            | "sceNpMatching2GetWorldInfoList"
            | "sceNpMatching2GetLobbyInfoList"
            | "sceNpMatching2JoinLobby"
            | "sceNpMatching2LeaveLobby"
            | "sceNpMatching2GrantRoomOwner"
            | "sceNpMatching2KickoutRoomMember"
            | "sceNpMatching2SetRoomDataExternal"
            | "sceNpMatching2SetRoomDataInternal"
            | "sceNpMatching2SetRoomMemberDataInternal"
            | "sceNpMatching2SignalingGetConnectionStatus"
            | "sceNpMatching2SignalingGetPingInfo"
            | "sceNpSignalingActivateConnection"
            | "sceNpSignalingGetConnectionStatus"
            | "sceNpScoreCensorComment"
            | "sceNpScoreSanitizeComment"
            | "sceNpScoreGetBoardInfo"
            | "sceNpScoreGetGameData"
            | "sceNpScoreGetRankingByNpIdPcId"
            | "sceNpScoreGetRankingByRange"
            | "sceNpScoreRecordGameData"
            | "sceNpScoreRecordScore"
            | "sceNpMatching2CreateContext"
            | "sceNpSignalingCreateContext" => np_signed_out as *const () as usize,
            "sceVoiceCreatePort" => voice_port as *const () as usize,
            "sceVoiceGetPortInfo" => voice_info as *const () as usize,
            "sceVoiceReadFromOPort" => voice_read as *const () as usize,
            "sceAudioInInput" | "sceAudioInClose" | "sceAudioInOpen" => {
                audio_in_not_opened as *const () as usize
            }
            "sceMsgDialogInitialize" => plain_init::<1> as *const () as usize,
            "sceMsgDialogOpen" => plain_open::<1> as *const () as usize,
            "sceMsgDialogUpdateStatus" => plain_status::<1> as *const () as usize,
            "sceMsgDialogTerminate" => plain_term::<1> as *const () as usize,
            "sceNpProfileDialogInitialize" => plain_init::<3> as *const () as usize,
            "sceNpProfileDialogOpen" => plain_open::<3> as *const () as usize,
            "sceNpProfileDialogUpdateStatus" => plain_status::<3> as *const () as usize,
            "sceNpProfileDialogTerminate" => plain_term::<3> as *const () as usize,
            "sceNpProfileDialogGetResult" => profile_result as *const () as usize,
            "sceNpCommerceDialogInitialize" => plain_init::<4> as *const () as usize,
            "sceNpCommerceDialogOpen" => plain_open::<4> as *const () as usize,
            "sceNpCommerceDialogUpdateStatus" => plain_status::<4> as *const () as usize,
            "sceNpCommerceDialogTerminate" => plain_term::<4> as *const () as usize,
            "sceErrorDialogInitialize" => plain_init::<6> as *const () as usize,
            "sceErrorDialogOpen" => error_open as *const () as usize,
            "sceErrorDialogUpdateStatus" => plain_status::<6> as *const () as usize,
            "sceErrorDialogTerminate" => plain_term::<6> as *const () as usize,
            "sceInvitationDialogTerminate" => invitation_term as *const () as usize,
            "sceSaveDataDialogInitialize" => save_init as *const () as usize,
            "sceSaveDataDialogOpen" => save_open as *const () as usize,
            "sceSaveDataDialogUpdateStatus" | "sceSaveDataDialogGetStatus" => {
                save_status as *const () as usize
            }
            "sceSaveDataDialogTerminate" => save_term as *const () as usize,
            "sceSaveDataDialogGetResult" => save_result as *const () as usize,
            "sceSaveDataDialogIsReadyToDisplay" => save_ready as *const () as usize,
            "sceSaveDataDialogProgressBarSetValue" => save_progress as *const () as usize,
            "sceSaveDataDialogClose" => save_close as *const () as usize,
            "sceImeDialogInit" => ime_init as *const () as usize,
            "sceImeDialogGetStatus" => ime_status as *const () as usize,
            "sceImeDialogGetResult" => ime_result as *const () as usize,
            "sceImeDialogTerm" | "sceImeDialogAbort" => ime_term as *const () as usize,
            "scePlayGoInitialize" => playgo_init as *const () as usize,
            "scePlayGoOpen" => playgo_open as *const () as usize,
            "scePlayGoGetChunkId" => playgo_chunk_ids as *const () as usize,
            "scePlayGoGetLocus" => playgo_locus as *const () as usize,
            "scePlayGoSetInstallSpeed" => playgo_speed as *const () as usize,
            "scePlayGoGetInstallSpeed" => playgo_get_speed as *const () as usize,
            "scePlayGoGetEta" => playgo_eta as *const () as usize,
            "scePlayGoGetProgress" => playgo_progress as *const () as usize,
            "scePlayGoGetToDoList" => playgo_todo as *const () as usize,
            "scePlayGoPrefetch" => playgo_prefetch as *const () as usize,
            "scePlayGoGetLanguageMask" => playgo_get_languages as *const () as usize,
            "scePlayGoSetLanguageMask" => playgo_set_languages as *const () as usize,
            "sceAppContentAddcontMount" | "sceAppContentAddcontEnqueueDownload" => {
                app_content_not_found as *const () as usize
            }
            "sceAppContentDownloadDataGetAvailableSpaceKb" => download_space as *const () as usize,
            "sceDiscMapIsRequestOnHDD" | "sceDiscMap_8A828CAEE7EDD5E9" => {
                disc_map_no_bitmap as *const () as usize
            }
            "sceMouseRead" => mouse_read as *const () as usize,
            // Calls that succeed and do nothing.
            "sceUserServiceTerminate"
            | "sceNetInit"
            | "sceNetTerm"
            | "sceNetPoolDestroy"
            | "sceNetEpollDestroy"
            | "sceNetResolverDestroy"
            | "sceNetCtlCheckCallback"
            | "sceNetCtlUnregisterCallback"
            | "sceSslTerm"
            | "sceHttpTerm"
            | "sceHttpDeleteTemplate"
            | "sceHttpSetNonblock"
            | "sceHttpSetConnectTimeOut"
            | "sceHttpsEnableOption"
            | "sceHttpsDisableOption"
            | "sceHttpAddRequestHeader"
            | "sceHttpSetRequestContentLength"
            | "sceHttpDeleteConnection"
            | "sceHttpDeleteRequest"
            | "sceHttpAbortWaitRequest"
            | "sceHttpDestroyEpoll"
            | "sceHttpSetEpoll"
            | "sceHttpUnsetEpoll"
            | "sceNpUnregisterStateCallback"
            | "sceNpUnregisterPlusEventCallback"
            | "sceNpCheckCallback"
            | "sceNpSetNpTitleId"
            | "sceNpNotifyPlusFeature"
            | "sceNpSetContentRestriction"
            | "sceNpDeleteRequest"
            | "sceNpAbortRequest"
            | "sceNpAuthDeleteRequest"
            | "sceNpLookupDeleteTitleCtx"
            | "sceNpLookupDeleteRequest"
            | "sceNpLookupAbortRequest"
            | "sceNpScoreDeleteNpTitleCtx"
            | "sceNpScoreDeleteRequest"
            | "sceNpScoreAbortRequest"
            | "sceNpWebApiTerminate"
            | "sceNpWebApiDeleteRequest"
            | "sceNpWebApiAbortRequest"
            | "sceNpWebApiDeleteContext"
            | "sceNpWebApiDeletePushEventFilter"
            | "sceNpWebApiUnregisterPushEventCallback"
            | "sceNpMatching2ContextStop"
            | "sceNpMatching2DestroyContext"
            | "sceNpMatching2RegisterContextCallback"
            | "sceNpMatching2RegisterLobbyEventCallback"
            | "sceNpMatching2RegisterRoomEventCallback"
            | "sceNpMatching2RegisterSignalingCallback"
            | "sceNpMatching2SetDefaultRequestOptParam"
            | "sceNpSignalingDeleteContext"
            | "sceNpSignalingDeactivateConnection"
            | "sceNpScoreSetPlayerCharacterId"
            | "sceVoiceDeletePort"
            | "sceVoiceConnectIPortToOPort"
            | "sceVoiceDisconnectIPortFromOPort"
            | "sceVoiceStart"
            | "sceVoiceStop"
            | "sceVoiceWriteToIPort"
            | "sceNpMatching2Initialize"
            | "sceNpMatching2Terminate"
            | "sceNpSignalingInitialize"
            | "sceNpSignalingTerminate"
            | "sceCommonDialogInitialize"
            | "sceRemoteplayInitialize"
            | "sceMouseInit"
            | "sceMouseClose"
            | "sceVoiceInit"
            | "sceVoiceEnd" => ok_void as *const () as usize,
            _ => return None,
        };
        Some(address)
    }

    /// Co-op (`BB_ONLINE=1`): the GPU library's online version of `name` when it has one.
    #[cfg(uncharted_loader)]
    fn online(name: &CStr, symbol: &str) -> Option<usize> {
        extern "C" {
            fn bbgpu_resolve(scoped_nid: *const c_char) -> usize;
        }
        static ON: OnceLock<bool> = OnceLock::new();
        if !*ON.get_or_init(|| flag(std::env::var_os("BB_ONLINE"))) || !online_symbol(symbol) {
            return None;
        }
        // SAFETY: a NUL-terminated scoped NID.
        let address = unsafe { bbgpu_resolve(name.as_ptr()) };
        if address != 0 {
            return Some(address);
        }
        c::out_line(&format!(
            "Co-op: {symbol} has no online version; using the offline one"
        ));
        None
    }

    /// Without the loader there is no GPU library and no co-op.
    #[cfg(not(uncharted_loader))]
    fn online(_name: &CStr, _symbol: &str) -> Option<usize> {
        None
    }

    /// The function behind the scoped import `name`, found by its symbol in the NID table.
    pub(super) fn resolve(name: &CStr) -> Option<usize> {
        let symbol = crate::abi::nid_name(name.to_str().ok()?)?;
        online(name, symbol).or_else(|| by_symbol(symbol))
    }
}

/// The guest function behind the scoped import `name`, or `None`. Only x86-64 hosts run guest
/// code.
#[cfg(target_arch = "x86_64")]
pub fn resolve(name: &std::ffi::CStr) -> Option<usize> {
    guest::resolve(name)
}

/// The exported C functions of this module, declared in `include/uncharted_runtime.h`.
#[cfg(target_arch = "x86_64")]
pub mod ffi {
    use std::ffi::{c_char, CStr};

    /// The address of the guest service function (user, system, network, PSN, dialogs, PlayGo
    /// and absent devices) behind the scoped import `name`, or 0 (the C
    /// `runtime_services_resolve`, now in `crates/runtime/src/services.rs`).
    ///
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C" fn runtime_services_resolve(name: *const c_char) -> usize {
        if name.is_null() {
            return 0;
        }
        // SAFETY: the caller passes a NUL-terminated string.
        super::resolve(unsafe { CStr::from_ptr(name) }).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_reads_like_atoi() {
        assert_eq!(language(None), 1);
        assert_eq!(language(Some("8")), 8);
        assert_eq!(language(Some(" 12abc")), 12);
        assert_eq!(language(Some("+3")), 3);
        assert_eq!(language(Some("-2")), -2);
        assert_eq!(language(Some("russian")), 0);
        assert_eq!(language(Some("")), 0);
        assert_eq!(atoi("2147483648"), i32::MIN);
    }

    #[test]
    fn utf16_to_utf8_matches_the_c_converter() {
        let text: Vec<u16> = "Ab\u{e9}\u{4e2d}\u{1f600}".encode_utf16().collect();
        assert_eq!(
            utf16_to_utf8(&text, 512),
            "Ab\u{e9}\u{4e2d}\u{1f600}".as_bytes()
        );
        // Stops at the NUL, and keeps 4 bytes and the terminator free.
        assert_eq!(utf16_to_utf8(&[0x41, 0, 0x42], 512), b"A");
        assert_eq!(utf16_to_utf8(&[0x41, 0x42, 0x43], 6), b"AB");
        assert_eq!(utf16_to_utf8(&[0x41], 0), b"");
        // A lone high surrogate is encoded as itself.
        assert_eq!(utf16_to_utf8(&[0xD800], 512), [0xED, 0xA0, 0x80]);
    }

    #[test]
    fn utf8_to_utf16_matches_the_c_converter() {
        let units = |text: &str| -> Vec<u16> { text.encode_utf16().collect() };
        assert_eq!(utf8_to_utf16("Hunter".as_bytes(), 16), units("Hunter"));
        assert_eq!(
            utf8_to_utf16("\u{e9}\u{4e2d}\u{1f600}".as_bytes(), 16),
            units("\u{e9}\u{4e2d}\u{1f600}")
        );
        assert_eq!(utf8_to_utf16(b"Hunter", 3), units("Hun"));
        // A surrogate pair that does not fit whole ends the text.
        assert_eq!(utf8_to_utf16("a\u{1f600}".as_bytes(), 2), units("a"));
        assert_eq!(utf8_to_utf16(b"ab\0cd", 16), units("ab"));
        // A truncated sequence keeps what it has.
        assert_eq!(utf8_to_utf16(&[0xE4, 0xB8], 16), [0x138]);
    }

    #[test]
    fn online_symbols() {
        assert!(online_symbol("sceNetSocket"));
        assert!(online_symbol("sceNpGetNpId"));
        assert!(online_symbol("sceHttpInit"));
        assert!(online_symbol("sceSslInit"));
        assert!(!online_symbol("sceNpTrophyCreateContext"));
        assert!(!online_symbol("sceNpCommerceDialogOpen"));
        assert!(!online_symbol("sceNpProfileDialogOpen"));
        assert!(!online_symbol("sceUserServiceGetUserName"));
    }

    #[test]
    fn flags_start_with_one() {
        assert!(flag(Some("1".into())));
        assert!(flag(Some("1yes".into())));
        assert!(!flag(Some("0".into())));
        assert!(!flag(Some("".into())));
        assert!(!flag(None));
    }

    #[cfg(target_arch = "x86_64")]
    mod guest_abi {
        use super::super::guest::*;
        use super::super::*;
        use std::ffi::{c_char, c_void};

        /// The guest function behind `symbol` as the function-pointer type `F`.
        ///
        /// # Safety
        /// `F` is the `extern "sysv64"` type of that function.
        unsafe fn function<F: Copy>(symbol: &str) -> F {
            let address =
                by_symbol(symbol).unwrap_or_else(|| panic!("{symbol} does not resolve"));
            assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<usize>());
            // SAFETY: an address of a function of type `F` (caller), the size of a pointer.
            unsafe { std::mem::transmute_copy(&address) }
        }

        type Status = extern "sysv64" fn() -> i32;

        #[test]
        fn exports_resolve_by_nid() {
            for (nid, symbol) in [
                (c"fZo48un7LK4#V#W", "sceSystemServiceParamGetInt"),
                (c"JoBqSQt1yyA#V#W", "sceSystemServiceLoadExec"),
                (c"1xxcMiGu2fo#D#E", "sceUserServiceGetUserName"),
                (c"iWQWrwiSt8A#E#F", "sceNetHtons"),
                (c"8Kcp5d-q1Uo#E#F", "sceNetInetPton"),
                (c"Q4qBuN-c0ZM#E#F", "sceNetSocket"),
                (c"uBPlr0lbuiI#J#K", "sceNetCtlGetState"),
                (c"obuxdTiwkF8#J#K", "sceNetCtlGetInfo"),
                (c"HQOwnfMGipQ#E#F", "sceNetErrnoLoc"),
            ] {
                let address = by_symbol(symbol).unwrap();
                assert_eq!(crate::services::resolve(nid), Some(address), "{nid:?}");
                // SAFETY: a NUL-terminated literal.
                assert_eq!(unsafe { ffi::runtime_services_resolve(nid.as_ptr()) }, address);
            }
            assert_eq!(crate::services::resolve(c"unknown"), None);
            assert_eq!(by_symbol("sceNpTrophyCreateContext"), None);
            assert_eq!(by_symbol("sceKernelUsleep"), None);
            // SAFETY: a null name is accepted and resolves to nothing.
            assert_eq!(unsafe { ffi::runtime_services_resolve(std::ptr::null()) }, 0);
        }

        #[test]
        fn every_c_export_resolves() {
            // The names of the deleted `exports[]` table, spot-checked across each group.
            for symbol in [
                "sceUserServiceInitialize",
                "sceUserServiceTerminate",
                "sceSystemServiceHideSplashScreen",
                "sceNetResolverStartAton",
                "sceNetCtlGetNatInfo",
                "sceHttpWaitRequest",
                "sceHttpsDisableOption",
                "sceNpWebApiUtilityParseNpId",
                "sceNpMatching2SignalingGetPingInfo",
                "sceNpScoreSetPlayerCharacterId",
                "sceVoiceWriteToIPort",
                "sceAudioInClose",
                "sceCommonDialogInitialize",
                "sceNpProfileDialogGetResult",
                "sceImeDialogAbort",
                "scePlayGoSetLanguageMask",
                "sceSaveDataDialogClose",
                "sceInvitationDialogTerminate",
                "sceAppContentDownloadDataGetAvailableSpaceKb",
                "sceRemoteplayInitialize",
                "sceMouseClose",
                "sceAudioInOpen",
                "sceDiscMap_8A828CAEE7EDD5E9",
                "sceVoiceEnd",
            ] {
                assert!(by_symbol(symbol).is_some(), "{symbol}");
            }
        }

        #[test]
        fn users_and_the_environment() {
            type Param = extern "sysv64" fn(i32, *mut i32) -> i32;
            type Name = extern "sysv64" fn(i32, *mut u8, u64) -> i32;
            type Ids = extern "sysv64" fn(*mut [i32; 4]) -> i32;
            type Event = extern "sysv64" fn(*mut [i32; 2]) -> i32;
            // SAFETY: the types of these functions.
            let (param, name, list, event): (Param, Name, Ids, Event) = unsafe {
                (
                    function("sceSystemServiceParamGetInt"),
                    function("sceUserServiceGetUserName"),
                    function("sceUserServiceGetLoginUserIdList"),
                    function("sceUserServiceGetEvent"),
                )
            };
            let mut value = -9;
            std::env::remove_var("BB_LANGUAGE");
            assert_eq!(param(1, &mut value), 0);
            assert_eq!(value, 1);
            std::env::set_var("BB_LANGUAGE", "8");
            assert_eq!(param(1, &mut value), 0);
            assert_eq!(value, 8);
            std::env::remove_var("BB_LANGUAGE");
            for (id, expected) in [(2, 1), (3, 1), (5, 0), (7, 0), (1000, 1)] {
                assert_eq!(param(id, &mut value), 0);
                assert_eq!(value, expected, "{id}");
            }
            assert_eq!(param(4, &mut value), 0);
            assert!(value.abs() <= 14 * 60);
            assert_eq!(param(1, std::ptr::null_mut()) as u32, 0x80A1_0003);

            let mut buffer = [0xffu8; 16];
            std::env::remove_var("BB_USER_NAME");
            assert_eq!(name(1, buffer.as_mut_ptr(), 16), 0);
            assert_eq!(&buffer[..7], b"Hunter\0");
            assert_eq!(name(1, buffer.as_mut_ptr(), 6) as u32, 0x8096_000a);
            assert_eq!(name(2, buffer.as_mut_ptr(), 16) as u32, 0x8096_0005);
            std::env::set_var("BB_USER_NAME", "Drake");
            assert_eq!(name(1, buffer.as_mut_ptr(), 6), 0);
            assert_eq!(&buffer[..6], b"Drake\0");

            // The IME dialog starts with the user name when its field is empty, and without
            // the GPU library it is accepted at once.
            let mut text = [0u16; 9];
            let title: Vec<u16> = "Name\0".encode_utf16().collect();
            let ime = ImeParam {
                user: 1,
                kind: 0,
                languages: 0,
                enter_label: 0,
                input_method: 0,
                filter: std::ptr::null(),
                option: 0,
                max_length: 8,
                buffer: text.as_mut_ptr(),
                x: 0.0,
                y: 0.0,
                horizontal_alignment: 0,
                vertical_alignment: 0,
                placeholder: std::ptr::null(),
                title: title.as_ptr(),
                reserved: [0; 16],
            };
            assert_eq!(ime_result(std::ptr::null_mut()) as u32, 0x80BC_0101);
            assert_eq!(ime_init(&ime, std::ptr::null()), 0);
            assert_eq!(ime_status(), 2);
            let mut result = 9u32;
            assert_eq!(ime_result(&mut result), 0);
            assert_eq!(result, 0);
            assert_eq!(&text[..6], &"Drake\0".encode_utf16().collect::<Vec<_>>()[..]);
            assert_eq!(ime_term(), 0);
            assert_eq!(ime_status(), 0);
            std::env::remove_var("BB_USER_NAME");
            // Text already in the field stays; a long text is cut at max_length.
            let initial: Vec<u16> = "Elena\0".encode_utf16().collect();
            text[..6].copy_from_slice(&initial);
            assert_eq!(ime_init(&ime, std::ptr::null()), 0);
            assert_eq!(&text[..6], &initial[..]);
            assert_eq!(ime_term(), 0);
            std::env::set_var("BB_IME_TEXT", "Nathan Drake");
            assert_eq!(ime_init(&ime, std::ptr::null()), 0);
            std::env::remove_var("BB_IME_TEXT");
            assert_eq!(&text, &"Nathan D\0".encode_utf16().collect::<Vec<_>>()[..]);
            assert_eq!(ime_term(), 0);
            let empty = ImeParam {
                buffer: std::ptr::null_mut(),
                ..ime
            };
            assert_eq!(ime_init(&empty, std::ptr::null()) as u32, 0x80BC_0004);
            assert_eq!(ime_init(std::ptr::null(), std::ptr::null()) as u32, 0x80BC_0004);

            let mut ids = [0; 4];
            assert_eq!(list(&mut ids), 0);
            assert_eq!(ids, [1, -1, -1, -1]);
            let mut login = [9; 2];
            assert_eq!(event(&mut login), 0);
            assert_eq!(login, [0, 1]);
            assert_eq!(event(&mut login) as u32, 0x8096_0007);
            assert_eq!(event(std::ptr::null_mut()) as u32, 0x8096_0005);
        }

        #[test]
        fn the_network_is_unplugged() {
            type Unreachable = extern "sysv64" fn() -> i32;
            type ErrnoLoc = extern "sysv64" fn() -> *mut i32;
            type Short = extern "sysv64" fn(u16) -> u16;
            type Long = extern "sysv64" fn(u32) -> u32;
            type Pton = extern "sysv64" fn(i32, *const c_char, *mut [u8; 4]) -> i32;
            type Ntop = extern "sysv64" fn(i32, *const [u8; 4], *mut u8, u32) -> *const u8;
            type State = extern "sysv64" fn(*mut i32) -> i32;
            type Info = extern "sysv64" fn(i32, *mut c_void) -> i32;
            type Nat = extern "sysv64" fn(*mut [u32; 4]) -> i32;
            type Object = extern "sysv64" fn() -> i32;
            // SAFETY: the types of these functions.
            unsafe {
                let errno_loc: ErrnoLoc = function("sceNetErrnoLoc");
                for symbol in [
                    "sceNetSocket",
                    "sceNetConnect",
                    "sceNetRecvfrom",
                    "sceNetEpollWait",
                    "sceNetResolverStartNtoa",
                ] {
                    *errno_loc() = 0;
                    assert_eq!(function::<Unreachable>(symbol)() as u32, 0x8041_0133);
                    assert_eq!(*errno_loc(), 51);
                }
                assert_eq!(function::<Unreachable>("sceNetInit")(), 0);
                let a = function::<Object>("sceNetPoolCreate")();
                let b = function::<Object>("sceNetResolverCreate")();
                assert!(a >= 1 && b > a);
                assert_eq!(function::<Short>("sceNetHtons")(0x1234), 0x3412);
                assert_eq!(function::<Short>("sceNetNtohs")(0x3412), 0x1234);
                assert_eq!(function::<Long>("sceNetHtonl")(0x1234_5678), 0x7856_3412);
                assert_eq!(function::<Long>("sceNetNtohl")(0x7856_3412), 0x1234_5678);

                let pton: Pton = function("sceNetInetPton");
                let mut address = [0u8; 4];
                assert_eq!(pton(2, c"192.168.1.20".as_ptr(), &mut address), 1);
                assert_eq!(address, [192, 168, 1, 20]);
                assert_eq!(pton(2, c"300.1.1.1".as_ptr(), &mut address), 0);
                assert_eq!(pton(28, c"::1".as_ptr(), &mut address) as u32, 0x8041_0116);
                assert_eq!(super::super::guest::net_errno(), 47);
                let ntop: Ntop = function("sceNetInetNtop");
                let mut text = [0xffu8; 16];
                let out = ntop(2, &[10, 0, 0, 1], text.as_mut_ptr(), 16);
                assert_eq!(out, text.as_ptr());
                assert_eq!(&text[..9], b"10.0.0.1\0");
                assert!(ntop(2, &[10, 0, 0, 1], text.as_mut_ptr(), 8).is_null());
                assert!(ntop(28, &[10, 0, 0, 1], text.as_mut_ptr(), 16).is_null());

                let mut state = 9;
                assert_eq!(function::<State>("sceNetCtlGetState")(&mut state), 0);
                assert_eq!(state, 0);
                assert_eq!(
                    function::<State>("sceNetCtlGetState")(std::ptr::null_mut()) as u32,
                    0x8041_2107
                );
                assert_eq!(
                    function::<Info>("sceNetCtlGetInfo")(1, std::ptr::null_mut()) as u32,
                    0x8041_2108
                );
                let mut nat = [7u32; 4];
                assert_eq!(function::<Nat>("sceNetCtlGetNatInfo")(&mut nat), 0);
                assert_eq!(nat, [7, 0, 3, 0]);
                assert_eq!(function::<Object>("sceHttpSendRequest")() as u32, 0x8043_1063);
                assert_eq!(function::<Object>("sceHttpReadData")() as u32, 0x8043_1063);
                assert!(function::<Object>("sceHttpInit")() >= 1);
                assert_eq!(function::<Object>("sceHttpTerm")(), 0);

                type NpState = extern "sysv64" fn(i32, *mut i32) -> i32;
                type NpId = extern "sysv64" fn(i32, *mut u8) -> i32;
                type Compare = extern "sysv64" fn(*const [u8; 16], *const [u8; 16]) -> i32;
                let mut np = 0;
                assert_eq!(function::<NpState>("sceNpGetState")(1, &mut np), 0);
                assert_eq!(np, 1);
                let mut id = [0u8; 36];
                if std::env::var_os("BB_OFFLINE_NPID").is_none() {
                    assert_eq!(
                        function::<NpId>("sceNpGetNpId")(1, id.as_mut_ptr()) as u32,
                        0x8055_0006
                    );
                }
                assert_eq!(function::<Object>("sceNpCheckPlus")() as u32, 0x8055_0006);
                let compare: Compare = function("sceNpCmpNpId");
                assert_eq!(compare(&[1; 16], &[1; 16]), 0);
                assert_eq!(compare(&[1; 16], &[2; 16]) as u32, 0x8055_0609);
                assert_eq!(compare(std::ptr::null(), &[2; 16]) as u32, 0x8055_0003);
            }
        }

        #[test]
        fn plain_dialogs_finish_at_once() {
            type Open = extern "sysv64" fn(*const c_void) -> i32;
            type ErrorOpen = extern "sysv64" fn(*const [i32; 4]) -> i32;
            // SAFETY: the types of these functions.
            unsafe {
                let init: Status = function("sceMsgDialogInitialize");
                let open: Open = function("sceMsgDialogOpen");
                let status: Status = function("sceMsgDialogUpdateStatus");
                let term: Status = function("sceMsgDialogTerminate");
                assert_eq!(open(std::ptr::null()) as u32, 0x80B8_0003);
                assert_eq!(init(), 0);
                assert_eq!(init() as u32, 0x80B8_0004);
                assert_eq!(status(), 1);
                assert_eq!(open(std::ptr::null()), 0);
                assert_eq!(status(), 3);
                assert_eq!(term(), 0);
                assert_eq!(status(), 0);
                assert_eq!(term() as u32, 0x80B8_0003);

                let init: Status = function("sceErrorDialogInitialize");
                let open: ErrorOpen = function("sceErrorDialogOpen");
                let status: Status = function("sceErrorDialogUpdateStatus");
                assert_eq!(init(), 0);
                assert_eq!(open(std::ptr::null()) as u32, 0x80B8_0006);
                assert_eq!(open(&[16, 0x8001_0001_u32 as i32, 1, 0]), 0);
                assert_eq!(status(), 3);
                assert_eq!(function::<Status>("sceErrorDialogTerminate")(), 0);
            }
        }

        #[test]
        fn save_dialog_answers_or_waits() {
            assert_eq!(save_open([0u8; 128].as_ptr()) as u32, 0x80B8_0003);
            // SAFETY: the types of these functions.
            let (init, term, ready): (Status, Status, Status) = unsafe {
                (
                    function("sceSaveDataDialogInitialize"),
                    function("sceSaveDataDialogTerminate"),
                    function("sceSaveDataDialogIsReadyToDisplay"),
                )
            };
            assert_eq!(init(), 0);
            assert_eq!(ready(), 1);
            assert_eq!(save_open(std::ptr::null()) as u32, 0x80B8_0006);
            let mut result = [0u8; 40];
            assert_eq!(save_result(result.as_mut_ptr()) as u32, 0x80B8_0005);

            // A user message with YES/NO is answered at the next status.
            let buttons = 1u32;
            let mut param = [0u8; 128];
            param[52..56].copy_from_slice(&2u32.to_ne_bytes());
            param[80..88].copy_from_slice(&(&buttons as *const u32 as usize).to_ne_bytes());
            param[112..120].copy_from_slice(&0x5678usize.to_ne_bytes());
            assert_eq!(save_open(param.as_ptr()), 0);
            assert_eq!(save_status(), 3);
            assert_eq!(save_result(result.as_mut_ptr()), 0);
            assert_eq!(result[0..4], 2u32.to_ne_bytes());
            assert_eq!(result[4..8], 0u32.to_ne_bytes());
            assert_eq!(result[8..12], 1u32.to_ne_bytes());
            assert_eq!(result[32..40], 0x5678usize.to_ne_bytes());

            // A progress bar runs until the game closes it.
            param[52..56].copy_from_slice(&5u32.to_ne_bytes());
            assert_eq!(save_open(param.as_ptr()), 0);
            assert_eq!(save_status(), 2);
            assert_eq!(save_status(), 2);
            assert_eq!(save_close(std::ptr::null()), 0);
            assert_eq!(save_status(), 3);

            // A save list offering a new entry takes it.
            let new_item = 1u8;
            let mut items = [0u8; 56];
            items[32..40].copy_from_slice(&(&new_item as *const u8 as usize).to_ne_bytes());
            let mut list = [0u8; 128];
            list[52..56].copy_from_slice(&1u32.to_ne_bytes());
            list[56..60].copy_from_slice(&1u32.to_ne_bytes());
            list[72..80].copy_from_slice(&(items.as_ptr() as usize).to_ne_bytes());
            assert_eq!(save_open(list.as_ptr()), 0);
            assert_eq!(save_status(), 3);
            let mut dir = [0xffu8; 32];
            let mut result = [0u8; 40];
            result[16..24].copy_from_slice(&(dir.as_mut_ptr() as usize).to_ne_bytes());
            assert_eq!(save_result(result.as_mut_ptr()), 0);
            assert_eq!(result[4..8], 0u32.to_ne_bytes()); // OK
            assert_eq!(dir, [0; 32]); // the new entry: the game names it

            // A load list with nothing to choose is cancelled.
            list[56..60].copy_from_slice(&2u32.to_ne_bytes());
            items[40..44].copy_from_slice(&1u32.to_ne_bytes());
            assert_eq!(save_open(list.as_ptr()), 0);
            assert_eq!(save_status(), 3);
            assert_eq!(save_result(result.as_mut_ptr()), 0);
            assert_eq!(result[4..8], 1u32.to_ne_bytes()); // USER_CANCELED
            assert_eq!(result[8..12], 0u32.to_ne_bytes());
            assert_eq!(term(), 0);
        }

        #[test]
        fn playgo_reports_everything_installed() {
            type Open = extern "sysv64" fn(*mut i32, *const c_void) -> i32;
            type Speed = extern "sysv64" fn(i32, *mut i32) -> i32;
            type Progress = extern "sysv64" fn(i32, *const u16, u32, *mut [u64; 2]) -> i32;
            type Languages = extern "sysv64" fn(i32, *mut u64) -> i32;
            type SetLanguages = extern "sysv64" fn(i32, u64) -> i32;
            // SAFETY: the types of these functions.
            unsafe {
                let mut handle = 0;
                assert_eq!(playgo_init(std::ptr::null()), 0);
                assert_eq!(
                    function::<Open>("scePlayGoOpen")(std::ptr::null_mut(), std::ptr::null())
                        as u32,
                    0x80B2_000A
                );
                assert_eq!(function::<Open>("scePlayGoOpen")(&mut handle, std::ptr::null()), 0);
                assert_eq!(handle, 1);
                // Without the file module the package has one chunk.
                let mut count = 0u32;
                assert_eq!(playgo_chunk_ids(1, std::ptr::null_mut(), 0, &mut count), 0);
                assert_eq!(count, 1);
                let mut ids = [9u16; 4];
                assert_eq!(playgo_chunk_ids(1, ids.as_mut_ptr(), 4, &mut count), 0);
                assert_eq!((count, ids[0], ids[1]), (1, 0, 9));
                assert_eq!(
                    playgo_chunk_ids(1, ids.as_mut_ptr(), 0, &mut count) as u32,
                    0x80B2_000B
                );
                assert_eq!(
                    playgo_chunk_ids(2, ids.as_mut_ptr(), 4, &mut count) as u32,
                    0x80B2_0009
                );
                let mut loci = [0i8; 2];
                assert_eq!(playgo_locus(1, [0u16].as_ptr(), 1, loci.as_mut_ptr()), 0);
                assert_eq!(loci[0], 3);
                assert_eq!(
                    playgo_locus(1, [0u16, 1].as_ptr(), 2, loci.as_mut_ptr()) as u32,
                    0x80B2_000C
                );
                let mut speed = 0;
                assert_eq!(function::<Speed>("scePlayGoGetInstallSpeed")(1, &mut speed), 0);
                assert_eq!(speed, 2);
                let mut progress = [0u64; 2];
                assert_eq!(
                    function::<Progress>("scePlayGoGetProgress")(1, [0u16].as_ptr(), 1, &mut progress),
                    0
                );
                assert_eq!(progress, [1, 1]);
                let mut mask = 0u64;
                assert_eq!(function::<Languages>("scePlayGoGetLanguageMask")(1, &mut mask), 0);
                assert_eq!(mask, u64::MAX);
                assert_eq!(function::<SetLanguages>("scePlayGoSetLanguageMask")(1, 5), 0);
                assert_eq!(function::<Languages>("scePlayGoGetLanguageMask")(1, &mut mask), 0);
                assert_eq!(mask, 5);
            }
        }

        #[test]
        fn absent_content_and_devices() {
            type Space = extern "sysv64" fn(*const c_char, *mut u64) -> i32;
            type Mouse = extern "sysv64" fn(i32, *mut [u8; 40], i32) -> i32;
            // SAFETY: the types of these functions.
            unsafe {
                assert_eq!(function::<Status>("sceAppContentAddcontMount")() as u32, 0x80D9_0005);
                let mut kb = 0u64;
                assert_eq!(
                    function::<Space>("sceAppContentDownloadDataGetAvailableSpaceKb")(
                        std::ptr::null(),
                        &mut kb
                    ),
                    0
                );
                assert_eq!(kb, 1_048_576);
                assert_eq!(function::<Status>("sceDiscMapIsRequestOnHDD")() as u32, 0x8110_0004);
                assert_eq!(function::<Status>("sceAudioInOpen")() as u32, 0x8026_0109);
                let mut record = [0xffu8; 40];
                assert_eq!(function::<Mouse>("sceMouseRead")(1, &mut record, 1), 1);
                assert_eq!(record, [0; 40]);
                assert_eq!(
                    function::<Mouse>("sceMouseRead")(1, &mut record, 0) as u32,
                    0x80DF_0001
                );
            }
        }
    }
}
