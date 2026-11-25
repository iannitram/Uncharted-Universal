//! Guest entry preparation, `bbgpu_init`, `BB_RESTART_COMMAND` handling, `enter_on_stack`, and
//! `runtime_quit` / `runtime_restart` helpers for `uncharted-run` (roadmap 3.16 / 3.17c, #457).

#[cfg(all(
    target_arch = "x86_64",
    any(target_os = "linux", target_os = "windows")
))]
use super::{fault, hooks, image, patches, ProbeFormat};
use super::{BootImage, BootOptions, ContentProfile};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static GPU_ENABLED: AtomicBool = AtomicBool::new(false);
static QUITTING: AtomicBool = AtomicBool::new(false);

static BBGPU_REGISTER_KERNEL_FN: AtomicUsize = AtomicUsize::new(0);
static BBNET_CONFIGURE_FN: AtomicUsize = AtomicUsize::new(0);
static BBGPU_INIT_FN: AtomicUsize = AtomicUsize::new(0);
static BBGPU_PREPARE_EXIT_FN: AtomicUsize = AtomicUsize::new(0);
static BBGPU_SYMBOL_COUNT_FN: AtomicUsize = AtomicUsize::new(0);
static BBGPU_TROPHY_NOTIFY_FN: AtomicUsize = AtomicUsize::new(0);

/// Render dimensions returned by [`BbGpuConfig::prepare_render`], matching `BbRenderSize`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BbRenderSize {
    pub width: u32,
    pub height: u32,
}

/// GPU/window startup configuration matching `BbGpuConfig` in `gpu/bbgpu.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BbGpuConfig {
    pub title: *const c_char,
    pub serial: *const c_char,
    pub user_dir: *const c_char,
    pub sdk_version: u32,
    pub psf_attributes: u32,
    pub width: i32,
    pub height: i32,
    pub prepare_render: Option<unsafe extern "C" fn(u32, u32, *mut c_void) -> BbRenderSize>,
    pub render_context: *mut c_void,
}

/// Optional C/GPU entrypoints invoked by the loader when `libbbgpu` is linked.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuHooks {
    pub bbgpu_register_kernel: Option<unsafe extern "C" fn()>,
    pub bbnet_configure:
        Option<unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, *const c_char)>,
    pub bbgpu_init: Option<unsafe extern "C" fn(*const BbGpuConfig) -> c_int>,
    pub bbgpu_prepare_exit: Option<unsafe extern "C" fn()>,
    pub bbgpu_symbol_count: Option<unsafe extern "C" fn() -> u32>,
    pub bbgpu_trophy_notify:
        Option<unsafe extern "C" fn(c_int, *const c_char, *const c_char, *const c_void, u64)>,
}

/// Whether automatic startup render sizing should be wired into `BbGpuConfig::prepare_render`.
/// External boot patches and explicit/legacy resolution overrides keep their existing launch paths.
#[cfg(any(
    test,
    all(
        target_arch = "x86_64",
        any(target_os = "linux", target_os = "windows")
    )
))]
fn should_auto_render_size(has_patch_files: bool, var: &dyn Fn(&str) -> Option<String>) -> bool {
    if has_patch_files {
        return false;
    }
    let non_empty = |name: &str| var(name).is_some_and(|v| !v.is_empty());
    if non_empty("BB_GAME_RES") || non_empty("BB_RENDER_RES") {
        return false;
    }
    var("BB_RENDER_720P")
        .as_deref()
        .is_none_or(|v| !v.starts_with('1'))
}

/// Registers optional GPU library hooks used during guest startup and shutdown.
pub fn configure_gpu_hooks(hooks: GpuHooks) {
    BBGPU_REGISTER_KERNEL_FN.store(
        hooks.bbgpu_register_kernel.map_or(0, |f| f as usize),
        Ordering::Release,
    );
    BBNET_CONFIGURE_FN.store(
        hooks.bbnet_configure.map_or(0, |f| f as usize),
        Ordering::Release,
    );
    BBGPU_INIT_FN.store(
        hooks.bbgpu_init.map_or(0, |f| f as usize),
        Ordering::Release,
    );
    BBGPU_PREPARE_EXIT_FN.store(
        hooks.bbgpu_prepare_exit.map_or(0, |f| f as usize),
        Ordering::Release,
    );
    BBGPU_SYMBOL_COUNT_FN.store(
        hooks.bbgpu_symbol_count.map_or(0, |f| f as usize),
        Ordering::Release,
    );
    BBGPU_TROPHY_NOTIFY_FN.store(
        hooks.bbgpu_trophy_notify.map_or(0, |f| f as usize),
        Ordering::Release,
    );
}

/// Returns whether GPU initialization has been activated for this process.
pub fn gpu_enabled() -> bool {
    GPU_ENABLED.load(Ordering::Acquire)
}

/// Registers GPU kernel event queues when a GPU backend is linked.
pub fn bbgpu_register_kernel() {
    let fn_ptr = BBGPU_REGISTER_KERNEL_FN.load(Ordering::Acquire);
    if fn_ptr != 0 {
        // SAFETY: registered via `configure_gpu_hooks`.
        let f: unsafe extern "C" fn() = unsafe { std::mem::transmute(fn_ptr) };
        unsafe { f() };
    }
}

/// Passes `app0` and `param.sfo` identity to the GPU/network library when linked.
pub fn bbnet_configure(app0: &CStr, serial: &CStr, title: &CStr, app_ver: &CStr) {
    let fn_ptr = BBNET_CONFIGURE_FN.load(Ordering::Acquire);
    if fn_ptr != 0 {
        // SAFETY: registered via `configure_gpu_hooks` with NUL-terminated C strings.
        let f: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, *const c_char) =
            unsafe { std::mem::transmute(fn_ptr) };
        unsafe {
            f(
                app0.as_ptr(),
                serial.as_ptr(),
                title.as_ptr(),
                app_ver.as_ptr(),
            )
        };
    }
}

/// Initializes the GPU window, Vulkan presenter, and command processor via the registered hook.
pub fn bbgpu_init(config: &BbGpuConfig) -> Result<(), String> {
    let fn_ptr = BBGPU_INIT_FN.load(Ordering::Acquire);
    if fn_ptr == 0 {
        return Err("GPU initialization failed (GPU library not linked)".into());
    }
    GPU_ENABLED.store(true, Ordering::Release);
    // SAFETY: `fn_ptr` was registered via `configure_gpu_hooks` and `config` is valid.
    let f: unsafe extern "C" fn(*const BbGpuConfig) -> c_int =
        unsafe { std::mem::transmute(fn_ptr) };
    let rc = unsafe { f(config) };
    if rc != 0 {
        GPU_ENABLED.store(false, Ordering::Release);
        return Err("GPU initialization failed".into());
    }
    Ok(())
}

/// Returns the number of GPU-provided HLE symbols (or `0` when no GPU backend is linked).
pub fn bbgpu_symbol_count() -> u32 {
    let fn_ptr = BBGPU_SYMBOL_COUNT_FN.load(Ordering::Acquire);
    if fn_ptr == 0 {
        return 0;
    }
    // SAFETY: registered via `configure_gpu_hooks`.
    let f: unsafe extern "C" fn() -> u32 = unsafe { std::mem::transmute(fn_ptr) };
    unsafe { f() }
}

/// Reads and validates a `BBCONT01` content profile file if configured in `opts`.
pub fn load_content_profile(opts: &BootOptions) -> Result<Option<ContentProfile>, String> {
    let Some(path) = opts.content_profile.as_deref() else {
        return Ok(None);
    };
    let bytes = std::fs::read(path).map_err(|_| "cannot open content profile".to_string())?;
    ContentProfile::parse(&bytes).map(Some)
}

/// Resolves the restart command from `BB_RESTART_COMMAND`, returning `None` when unset or empty.
pub fn restart_command_from_env(var: Option<&str>) -> Option<String> {
    var.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

/// Decision made by [`runtime_restart`] from `BB_RESTART_COMMAND` and the host OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartPlan {
    Command(String),
    RunSh,
    MissingCommand,
}

/// Computes the restart plan for `BB_RESTART_COMMAND` without mutating process state.
pub fn restart_plan(var: Option<&str>, windows: bool) -> RestartPlan {
    match restart_command_from_env(var) {
        Some(cmd) => RestartPlan::Command(cmd),
        None if windows => RestartPlan::MissingCommand,
        None => RestartPlan::RunSh,
    }
}

fn flush_all_streams() {
    extern "C" {
        fn fflush(stream: *mut c_void) -> c_int;
    }
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: `fflush(NULL)` flushes all open C stdio output streams.
    unsafe {
        fflush(std::ptr::null_mut());
    }
}

fn prepare_quit_once(quitting: &AtomicBool, gpu_on: bool, prepare_exit: impl FnOnce()) -> bool {
    if !quitting.swap(true, Ordering::AcqRel) && gpu_on {
        prepare_exit();
        return true;
    }
    false
}

/// Flushes streams, saves the shader cache once if GPU mode was active, and exits immediately
/// without running host destructors (`probe.c`'s `runtime_quit`).
pub fn runtime_quit(code: i32) -> ! {
    crate::benchmark_run::uncharted_benchmark_crash();
    flush_all_streams();
    let prepare_fn = BBGPU_PREPARE_EXIT_FN.load(Ordering::Acquire);
    prepare_quit_once(&QUITTING, gpu_enabled(), || {
        if prepare_fn != 0 {
            // SAFETY: registered via `configure_gpu_hooks`.
            let f: unsafe extern "C" fn() = unsafe { std::mem::transmute(prepare_fn) };
            unsafe { f() };
        }
    });
    flush_all_streams();
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        // SAFETY: terminates the current process cleanly without DLL detach hangs.
        unsafe { TerminateProcess(GetCurrentProcess(), code as u32) };
        std::process::exit(code);
    }
    #[cfg(unix)]
    {
        extern "C" {
            fn _exit(status: c_int) -> !;
        }
        // SAFETY: async-signal-safe immediate process termination without destructors.
        unsafe { _exit(code) };
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::process::exit(code);
    }
}

/// Compatibility alias for [`runtime_quit`].
pub fn quit_process(code: i32) -> ! {
    runtime_quit(code)
}

/// Restarts the game via `BB_RESTART_COMMAND` (or `bash run.sh` on Unix), matching
/// `src/probe.c`'s `runtime_restart`.
pub fn runtime_restart() -> ! {
    flush_all_streams();
    let env_val = std::env::var("BB_RESTART_COMMAND").ok();
    match restart_plan(env_val.as_deref(), cfg!(windows)) {
        #[cfg(windows)]
        RestartPlan::MissingCommand | RestartPlan::RunSh => {
            println!("Runtime: restarting through run.py");
            eprintln!("runtime_restart: BB_RESTART_COMMAND is not set");
            flush_all_streams();
            runtime_quit(1);
        }
        #[cfg(windows)]
        RestartPlan::Command(cmd) => {
            use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
            use windows_sys::Win32::System::Threading::{
                CreateProcessA, GetCurrentProcess, TerminateProcess, PROCESS_INFORMATION,
                STARTF_USESTDHANDLES, STARTUPINFOA,
            };
            extern "system" {
                fn GetStdHandle(n_std_handle: u32) -> *mut c_void;
            }
            println!("Runtime: restarting through run.py");
            flush_all_streams();
            let Ok(mut line) = CString::new(cmd).map(|c| c.into_bytes_with_nul()) else {
                eprintln!("runtime_restart: invalid BB_RESTART_COMMAND");
                runtime_quit(1);
            };
            // SAFETY: inherits standard handles so the launcher keeps reading the log.
            unsafe {
                let mut startup: STARTUPINFOA = std::mem::zeroed();
                startup.cb = std::mem::size_of::<STARTUPINFOA>() as u32;
                startup.dwFlags = STARTF_USESTDHANDLES;
                startup.hStdInput = GetStdHandle((-10i32) as u32);
                startup.hStdOutput = GetStdHandle((-11i32) as u32);
                startup.hStdError = GetStdHandle((-12i32) as u32);
                let mut process: PROCESS_INFORMATION = std::mem::zeroed();
                if CreateProcessA(
                    std::ptr::null(),
                    line.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    &startup,
                    &mut process,
                ) == 0
                {
                    eprintln!("runtime_restart: CreateProcess failed ({})", GetLastError());
                    runtime_quit(1);
                }
                CloseHandle(process.hThread);
                CloseHandle(process.hProcess);
                TerminateProcess(GetCurrentProcess(), 0);
            }
            runtime_quit(0);
        }
        #[cfg(unix)]
        plan => {
            extern "C" {
                fn _exit(status: c_int) -> !;
                fn execl(path: *const c_char, arg0: *const c_char, ...) -> c_int;
                fn execlp(file: *const c_char, arg0: *const c_char, ...) -> c_int;
            }
            #[cfg(target_os = "linux")]
            // SAFETY: close inherited fds >= 3 before re-exec so GPU/device fds are released.
            unsafe {
                libc::syscall(libc::SYS_close_range, 3u32, !0u32, 0u32);
            }
            match plan {
                RestartPlan::Command(cmd) => {
                    println!("Runtime: restarting through run.py");
                    flush_all_streams();
                    if let Ok(c_cmd) = CString::new(cmd) {
                        // SAFETY: null-terminated static/CString arguments to `execl`.
                        unsafe {
                            execl(
                                c"/bin/sh".as_ptr(),
                                c"sh".as_ptr(),
                                c"-c".as_ptr(),
                                c_cmd.as_ptr(),
                                std::ptr::null::<c_char>(),
                            );
                        }
                    }
                }
                RestartPlan::RunSh | RestartPlan::MissingCommand => {
                    println!("Runtime: restarting through run.sh");
                    flush_all_streams();
                    // SAFETY: null-terminated static arguments to `execlp`.
                    unsafe {
                        execlp(
                            c"bash".as_ptr(),
                            c"bash".as_ptr(),
                            c"run.sh".as_ptr(),
                            std::ptr::null::<c_char>(),
                        );
                    }
                }
            }
            eprintln!("runtime_restart: exec failed");
            // SAFETY: exec failed; terminate immediately.
            unsafe { _exit(1) };
        }
        #[cfg(not(any(unix, windows)))]
        _ => runtime_quit(1),
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
core::arch::global_asm!(
    ".globl uncharted_enter_on_stack",
    "uncharted_enter_on_stack:",
    "mov rsp, rcx",
    "mov rax, rdi",
    "mov rdi, rsi",
    "mov rsi, rdx",
    "jmp rax",
    ".globl uncharted_call_on_stack",
    "uncharted_call_on_stack:",
    "push rbp",
    "mov rbp, rsp",
    "and rcx, -16",
    "mov rsp, rcx",
    "mov rax, rdi",
    "mov rdi, rsi",
    "mov rsi, rdx",
    "call rax",
    "mov rsp, rbp",
    "pop rbp",
    "ret",
);

#[cfg(all(target_arch = "x86_64", windows))]
core::arch::global_asm!(
    ".globl uncharted_enter_on_stack",
    "uncharted_enter_on_stack:",
    "mov rsp, r9",
    "mov rax, rcx",
    "mov rdi, rdx",
    "mov rsi, r8",
    "jmp rax",
    ".globl uncharted_call_on_stack",
    "uncharted_call_on_stack:",
    "push rbp",
    "mov rbp, rsp",
    "push rdi",
    "push rsi",
    "and r9, -16",
    "mov rsp, r9",
    "mov rax, rcx",
    "mov rdi, rdx",
    "mov rsi, r8",
    "call rax",
    "lea rsp, [rbp - 16]",
    "pop rsi",
    "pop rdi",
    "pop rbp",
    "ret",
);

#[cfg(all(
    target_arch = "x86_64",
    any(target_os = "linux", target_os = "windows")
))]
extern "C" {
    /// Transfers control to a SysV x86-64 guest `entry(arg0, arg1)` on a freshly prepared stack
    /// (`top`, 16-byte aligned minus 8 or 64 as expected by the guest).
    pub fn uncharted_enter_on_stack(
        entry: *const c_void,
        arg0: *mut c_void,
        arg1: *mut c_void,
        top: *mut c_void,
    ) -> !;

    fn uncharted_call_on_stack(
        entry: *const c_void,
        arg0: *mut c_void,
        arg1: *mut c_void,
        top: *mut c_void,
    );
}

/// Calls SysV x86-64 `entry(arg0, arg1)` on a new 16-byte-aligned stack at `top`, returning if
/// `entry` returns (`probe.c`'s `enter_on_stack`).
///
/// # Safety
/// `entry` must point to executable SysV64 code and `top` must have usable writable stack space
/// below it.
#[cfg(all(
    target_arch = "x86_64",
    any(target_os = "linux", target_os = "windows")
))]
pub unsafe fn enter_on_stack(
    entry: *const c_void,
    arg0: *mut c_void,
    arg1: *mut c_void,
    top: *mut c_void,
) {
    // SAFETY: forwarded from caller.
    unsafe { uncharted_call_on_stack(entry, arg0, arg1, top) };
}

/// Value looked up from a PS4 `param.sfo` (`\0PSF`) file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SfoValue {
    Text(String),
    Number(u32),
}

/// Looks up `key` in raw `param.sfo` bytes using bounded 64-bit offsets matching `src/probe.c`.
pub fn sfo_lookup_bytes(data: &[u8], key: &str) -> Option<SfoValue> {
    if data.len() < 20 || &data[..4] != b"\0PSF" {
        return None;
    }
    let kbase = u64::from(u32::from_le_bytes(data[8..12].try_into().ok()?));
    let vbase = u64::from(u32::from_le_bytes(data[12..16].try_into().ok()?));
    let mut count = u64::from(u32::from_le_bytes(data[16..20].try_into().ok()?));
    let max_entries = ((data.len() - 20) / 16) as u64;
    if count > max_entries {
        count = max_entries;
    }
    for i in 0..count {
        let e = 20 + (i as usize) * 16;
        let key_off = u64::from(u16::from_le_bytes(data[e..e + 2].try_into().ok()?));
        let format = u16::from_le_bytes(data[e + 2..e + 4].try_into().ok()?);
        let length = u64::from(u32::from_le_bytes(data[e + 4..e + 8].try_into().ok()?));
        let val_off = u64::from(u32::from_le_bytes(data[e + 12..e + 16].try_into().ok()?));

        let key_at = usize::try_from(kbase.checked_add(key_off)?).ok()?;
        let val_at = usize::try_from(vbase.checked_add(val_off)?).ok()?;
        let val_end = val_at.checked_add(usize::try_from(length).ok()?)?;
        if key_at >= data.len() || val_end > data.len() {
            continue;
        }
        let key_tail = &data[key_at..];
        let nul = key_tail
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(key_tail.len());
        if &key_tail[..nul] != key.as_bytes() {
            continue;
        }
        if format == 0x0404 && length >= 4 {
            let num = u32::from_le_bytes(data[val_at..val_at + 4].try_into().ok()?);
            return Some(SfoValue::Number(num));
        }
        let raw = &data[val_at..val_end];
        let text_end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let text = String::from_utf8_lossy(&raw[..text_end]).into_owned();
        return Some(SfoValue::Text(text));
    }
    None
}

#[cfg(all(
    target_arch = "x86_64",
    any(target_os = "linux", target_os = "windows")
))]
fn sfo_lookup_file(path: &Path, key: &str) -> Option<SfoValue> {
    let data = std::fs::read(path).ok()?;
    let limit = data.len().min(65536);
    sfo_lookup_bytes(&data[..limit], key)
}

/// Guest entry parameter block `{ argc, argv[34] }` passed in `rdi`.
#[repr(C)]
pub struct GuestEntryParams {
    pub argc: u64,
    pub argv: [*const c_char; 34],
}

/// Backing storage owning the NUL-terminated guest `argv` strings.
pub struct GuestArgv {
    strings: Vec<CString>,
}

impl GuestArgv {
    /// Parses `--guest-argv` text lines with `probe.c`'s semantics:
    /// line 0 is the guest executable path and is skipped when additional arguments follow.
    pub fn from_text(text: Option<&str>) -> Self {
        let mut lines: Vec<String> = Vec::new();
        if let Some(content) = text {
            for line in content.lines().take(33) {
                lines.push(line.trim_end_matches('\r').to_owned());
            }
        }
        let selected: Vec<String> = if lines.len() > 1 {
            lines.into_iter().skip(1).collect()
        } else if lines.len() == 1 {
            lines
        } else {
            vec!["/app0/eboot.bin".to_owned()]
        };
        let strings = selected
            .into_iter()
            .map(|s| CString::new(s).unwrap_or_else(|_| CString::from(c"/app0/eboot.bin")))
            .collect();
        Self { strings }
    }

    /// Reads `--guest-argv` from `path` (falling back to `/app0/eboot.bin` if unset/unreadable).
    pub fn from_file(path: Option<&Path>) -> Self {
        let content = path.and_then(|p| std::fs::read_to_string(p).ok());
        Self::from_text(content.as_deref())
    }

    /// Builds the `GuestEntryParams` referencing `self`'s owned strings.
    pub fn as_params(&self) -> GuestEntryParams {
        let mut argv = [std::ptr::null::<c_char>(); 34];
        let count = self.strings.len().min(33);
        for (i, s) in self.strings.iter().take(count).enumerate() {
            argv[i] = s.as_ptr();
        }
        GuestEntryParams {
            argc: count as u64,
            argv,
        }
    }

    /// Returns the argv strings as Rust `&str` slices for inspection/testing.
    pub fn args(&self) -> Vec<&str> {
        self.strings
            .iter()
            .filter_map(|s| s.to_str().ok())
            .collect()
    }
}

#[cfg(all(
    target_arch = "x86_64",
    any(target_os = "linux", target_os = "windows")
))]
mod runtime_boot {
    use super::*;
    use std::sync::Mutex;

    struct UnresolvedContext {
        image_base: u64,
        import_names: Vec<String>,
        modules: Vec<super::super::LinkedModule>,
    }

    static UNRESOLVED_CTX: Mutex<Option<UnresolvedContext>> = Mutex::new(None);
    static ENTERED_GAME: AtomicBool = AtomicBool::new(false);

    extern "sysv64" fn guest_exit() {
        println!("Runtime: process finalizer callback reached");
    }

    extern "sysv64" fn unresolved_import(id: u32, argument: usize) -> ! {
        let ctx_guard = UNRESOLVED_CTX.lock().ok();
        if let Some(ctx) = ctx_guard.as_ref().and_then(|g| g.as_ref()) {
            if let Some(nid) = ctx.import_names.get(id as usize) {
                println!("STOP: first unsupported PS4 import: {nid} (index {id})");
                if let Ok(c_nid) = CString::new(nid.as_str()) {
                    // SAFETY: `c_nid` is a valid NUL-terminated string.
                    let api = unsafe { crate::abi::ffi::runtime_import_name(c_nid.as_ptr()) };
                    if !api.is_null() {
                        // SAFETY: static string returned by `runtime_import_name`.
                        let api_str = unsafe { CStr::from_ptr(api) }.to_string_lossy();
                        println!("API: {api_str}");
                    }
                }
                println!(
                    "First argument: {argument:#x}; image base: {:#x}",
                    ctx.image_base
                );
                if !ctx.modules.is_empty() {
                    println!("Linked modules loaded: {}", ctx.modules.len());
                }
            } else {
                eprintln!("ERROR: bad import trap index {id}");
                runtime_quit(1);
            }
        } else {
            eprintln!("ERROR: bad import trap index {id}");
            runtime_quit(1);
        }
        crate::abi::libc::runtime_report();
        if ENTERED_GAME.load(Ordering::Acquire) {
            println!(
                "Original guest entry instructions executed; game initialization is incomplete."
            );
        } else {
            println!("Native libc initialization is incomplete; game entry has not run.");
        }
        runtime_quit(20);
    }

    extern "C" fn host_trophy_notify(
        grade: c_int,
        game: *const c_char,
        name: *const c_char,
        png: *const c_void,
        png_size: u64,
    ) {
        let fn_ptr = BBGPU_TROPHY_NOTIFY_FN.load(Ordering::Acquire);
        if fn_ptr != 0 {
            // SAFETY: registered via `configure_gpu_hooks`.
            let f: unsafe extern "C" fn(c_int, *const c_char, *const c_char, *const c_void, u64) =
                unsafe { std::mem::transmute(fn_ptr) };
            unsafe { f(grade, game, name, png, png_size) };
        }
    }

    struct AutoRenderContext {
        image: *mut u8,
        image_size: u64,
        error: Option<String>,
    }

    unsafe extern "C" fn prepare_window_render(
        width: u32,
        height: u32,
        context: *mut c_void,
    ) -> BbRenderSize {
        if context.is_null() {
            return BbRenderSize::default();
        }
        // SAFETY: `context` points to a live `AutoRenderContext` for the duration of `bbgpu_init`.
        let ctx = unsafe { &mut *(context as *mut AutoRenderContext) };
        // SAFETY: `ctx.image` points to `ctx.image_size` writable bytes of the mapped guest image.
        let render = unsafe {
            crate::display::ffi::runtime_display_auto(ctx.image, ctx.image_size, width, height)
        };
        if render.direct_memory_mib != 0
            && crate::memory::abi::runtime_memory_set_dmem(render.direct_memory_mib) != 0
        {
            ctx.error = Some("cannot configure automatic render direct memory".into());
            return BbRenderSize::default();
        }
        if render.width != 0 {
            println!(
                "Display: window {width}x{height}, startup render {}x{}",
                render.width, render.height
            );
        }
        BbRenderSize {
            width: render.width,
            height: render.height,
        }
    }

    fn host_page_size() -> usize {
        #[cfg(target_os = "linux")]
        {
            // SAFETY: standard POSIX sysconf query.
            let ps = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            usize::try_from(ps).ok().filter(|&n| n > 0).unwrap_or(4096)
        }
        #[cfg(windows)]
        {
            4096
        }
    }

    fn host_exe_base() -> u64 {
        #[cfg(target_os = "linux")]
        {
            let mut dl: libc::Dl_info = unsafe { std::mem::zeroed() };
            // SAFETY: query module base containing `host_exe_base`.
            if unsafe { libc::dladdr(host_exe_base as *const () as *const c_void, &mut dl) } != 0 {
                dl.dli_fbase as usize as u64
            } else {
                0
            }
        }
        #[cfg(windows)]
        {
            0
        }
    }

    pub fn boot(opts: &BootOptions, boot: &BootImage, payload: &[u8]) -> Result<(), (i32, String)> {
        if std::env::var_os("BB_SKIP_CPU_CHECK").is_none() && !crate::probe::cpu().supported {
            return Err((
                24,
                "unsupported CPU: missing required instructions (AVX/POPCNT/SSE4.2 or patchable BMI1/LZCNT/MOVBE)"
                    .into(),
            ));
        }

        #[cfg(windows)]
        {
            crate::memory::abi::runtime_memory_reserve();
            crate::thread::runtime_thread_tcb_offset();
        }

        if let Some(profile) = load_content_profile(opts).map_err(|e| (1, e))? {
            // SAFETY: `profile.0` has 5 `u32` values.
            unsafe { crate::content::ffi::runtime_content_configure(profile.0.as_ptr()) };
        }

        let user_dir = opts.user_dir.as_deref().unwrap_or("user");
        let user_c = CString::new(user_dir).map_err(|_| (1, "invalid --user path".to_string()))?;

        if let Some(app0) = opts.app0.as_deref() {
            let app0_c = CString::new(app0).map_err(|_| (1, "invalid --app0 path".to_string()))?;
            // SAFETY: both pointers are valid NUL-terminated strings.
            unsafe { crate::file::ffi::runtime_file_configure(app0_c.as_ptr(), user_c.as_ptr()) };

            let sfo_path = Path::new(app0).join("sce_sys/param.sfo");
            let savedata_id = match sfo_lookup_file(&sfo_path, "INSTALL_DIR_SAVEDATA")
                .or_else(|| sfo_lookup_file(&sfo_path, "TITLE_ID"))
            {
                Some(SfoValue::Text(t)) if !t.is_empty() => Some(t),
                _ => None,
            };
            if let Some(id) = savedata_id {
                if let Ok(id_c) = CString::new(id) {
                    // SAFETY: valid NUL-terminated string.
                    unsafe { crate::savedata::runtime_savedata_configure(id_c.as_ptr()) };
                }
            }
            let serial = match sfo_lookup_file(&sfo_path, "TITLE_ID") {
                Some(SfoValue::Text(t)) => t,
                _ => String::new(),
            };
            let title = match sfo_lookup_file(&sfo_path, "TITLE") {
                Some(SfoValue::Text(t)) => t,
                _ => String::new(),
            };
            let version = match sfo_lookup_file(&sfo_path, "APP_VER") {
                Some(SfoValue::Text(t)) => t,
                _ => String::new(),
            };
            if let (Ok(sc), Ok(tc), Ok(vc)) = (
                CString::new(serial),
                CString::new(title),
                CString::new(version),
            ) {
                bbnet_configure(&app0_c, &sc, &tc, &vc);
            }
        }

        let notify_fn = if BBGPU_TROPHY_NOTIFY_FN.load(Ordering::Acquire) != 0 {
            Some(
                host_trophy_notify
                    as unsafe extern "C" fn(
                        c_int,
                        *const c_char,
                        *const c_char,
                        *const c_void,
                        u64,
                    ),
            )
        } else {
            None
        };
        crate::trophy::runtime_trophy_bind(
            Some(crate::file::ffi::runtime_file_user_dir),
            notify_fn,
        );
        if let Some(tdir) = opts.trophy_dir.as_deref() {
            let tdir_c =
                CString::new(tdir).map_err(|_| (1, "invalid --trophies path".to_string()))?;
            // SAFETY: valid NUL-terminated string.
            unsafe { crate::trophy::runtime_trophy_configure(tdir_c.as_ptr()) };
        }

        // SAFETY: `user_c` is a valid NUL-terminated path.
        if unsafe { crate::benchmark_run::uncharted_benchmark_start(user_c.as_ptr()) } < 0 {
            return Err((2, "BB_BENCHMARK is set but cannot run".into()));
        }
        fault::configure_benchmark_crash(Some(crate::benchmark_run::uncharted_benchmark_crash));

        bbgpu_register_kernel();

        #[cfg(target_os = "linux")]
        // SAFETY: called on the main thread before guest code runs.
        unsafe {
            fault::linux::install_handlers();
            libc::alarm(u32::try_from(opts.timeout_seconds).unwrap_or(u32::MAX));
        }
        #[cfg(windows)]
        // SAFETY: called on the main thread before guest code runs.
        unsafe {
            fault::windows::install_handlers();
        }

        let start_caps = if opts.strict_imports {
            0
        } else {
            boot.capabilities
        };
        crate::abi::libc::runtime_start(start_caps);

        let page_size = host_page_size();
        let mapped = image::MappedImage::map_without_relocs(
            boot,
            payload,
            page_size,
            unresolved_import as *const () as usize,
        )
        .map_err(|e| (1, e))?;

        let image_base = mapped.base();
        fault::configure_fault_context(image_base, host_exe_base(), !opts.cpu_only);

        if let Ok(mut guard) = UNRESOLVED_CTX.lock() {
            *guard = Some(UnresolvedContext {
                image_base,
                import_names: boot.import_names.clone(),
                modules: boot.modules.clone(),
            });
        }

        #[cfg(windows)]
        // SAFETY: image covers `mapped.mapped_size()` bytes.
        unsafe {
            crate::red_zone::red_zone_reserve(
                image_base as usize as *mut u8,
                mapped.mapped_size() as u64,
            );
        }

        let image_end = boot
            .segments
            .iter()
            .map(|s| s.address + s.size)
            .max()
            .unwrap_or(0);

        if !opts.cpu_only {
            let sfo_path = Path::new(opts.app0.as_deref().unwrap_or(".")).join("sce_sys/param.sfo");
            let title = match sfo_lookup_file(&sfo_path, "TITLE") {
                Some(SfoValue::Text(t)) if !t.is_empty() => t,
                _ => "Bloodborne".to_owned(),
            };
            let serial = match sfo_lookup_file(&sfo_path, "TITLE_ID") {
                Some(SfoValue::Text(t)) if !t.is_empty() => t,
                _ => "UNKNOWN".to_owned(),
            };
            let attributes = match sfo_lookup_file(&sfo_path, "ATTRIBUTE") {
                Some(SfoValue::Number(n)) => n,
                _ => 0,
            };
            let sdk_version = if boot.procparam != 0 {
                // SAFETY: `BootImage::parse` validated `procparam..procparam+64` is mapped.
                let sdk = unsafe {
                    std::ptr::read_unaligned((image_base + boot.procparam + 16) as *const u64)
                };
                sdk as u32
            } else {
                0
            };
            std::env::set_var("BB_GUEST_IMAGE_BASE", format!("{image_base:#x}"));
            let title_c = CString::new(title).unwrap_or_else(|_| CString::from(c"Bloodborne"));
            let serial_c = CString::new(serial).unwrap_or_else(|_| CString::from(c"UNKNOWN"));
            let mut auto_ctx = AutoRenderContext {
                image: image_base as usize as *mut u8,
                image_size: image_end,
                error: None,
            };
            let auto_render = should_auto_render_size(!opts.patch_files.is_empty(), &|name| {
                std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
            });
            let gpu = BbGpuConfig {
                title: title_c.as_ptr(),
                serial: serial_c.as_ptr(),
                user_dir: user_c.as_ptr(),
                sdk_version,
                psf_attributes: attributes,
                width: 1920,
                height: 1080,
                prepare_render: auto_render.then_some(prepare_window_render as _),
                render_context: if auto_render {
                    (&raw mut auto_ctx).cast()
                } else {
                    std::ptr::null_mut()
                },
            };
            bbgpu_init(&gpu).map_err(|e| (1, e))?;
            if let Some(err) = auto_ctx.error {
                return Err((1, err));
            }
            println!(
                "GPU: window and Vulkan presenter ready; SDK {sdk_version:#010x}, {} HLE symbols",
                bbgpu_symbol_count()
            );
        }

        mapped
            .apply_relocs(boot, opts.strict_imports)
            .map_err(|e| (1, e))?;

        for patch_path in &opts.patch_files {
            // SAFETY: image is mapped and writable before `protect_segments`.
            let (bytes_written, rebased) = unsafe {
                patches::apply_file(
                    Path::new(patch_path),
                    image_base,
                    &boot.segments,
                    &boot.relocs,
                )
            }
            .map_err(|e| (1, e))?;
            println!(
                "Patches: {bytes_written} bytes applied, {rebased} pointers rebased ({patch_path})"
            );
        }

        // SAFETY: image segments are mapped and writable.
        unsafe {
            hooks::install_game_callbacks(image_base, image_end, &boot.segments);
            hooks::patch_u2_convoy_sink_segments(image_base, &boot.segments);
            crate::display::ffi::runtime_display_patch(image_base as usize as *mut u8, image_end);
        }

        #[cfg(windows)]
        {
            let disp = crate::thread::runtime_thread_tcb_offset();
            // SAFETY: image segments are mapped and writable.
            let patched = unsafe { hooks::patch_tcb_loads(image_base, &boot.segments, disp) };
            println!("Guest TCB loads redirected to the TEB TLS slot: {patched}");
            let rz_segs: Vec<crate::red_zone::RedZoneSegment> = boot
                .segments
                .iter()
                .map(|s| crate::red_zone::RedZoneSegment {
                    address: s.address,
                    size: s.size,
                    flags: s.flags,
                })
                .collect();
            // SAFETY: image is mapped and writable.
            unsafe {
                crate::red_zone::red_zone_patch(
                    image_base as usize as *mut u8,
                    boot.size,
                    rz_segs.as_ptr(),
                    rz_segs.len(),
                );
            }
        }

        let code_ranges: Vec<[u64; 2]> = boot
            .segments
            .iter()
            .filter(|s| (s.flags & 1) != 0)
            .map(|s| [s.address, s.size])
            .collect();
        // SAFETY: image is mapped and writable for `boot.size` bytes.
        unsafe {
            crate::cpu_patch::cpu_patch_image(
                image_base as usize as *mut u8,
                boot.size,
                code_ranges.as_ptr(),
                code_ranges.len(),
            );
        }

        mapped
            .protect_segments(&boot.segments)
            .map_err(|e| (1, e))?;

        let entry_exec = boot.segments.iter().any(|s| {
            (s.flags & 1) != 0 && boot.entry >= s.address && boot.entry - s.address < s.size
        });
        if !entry_exec {
            return Err((1, "entry is not executable".into()));
        }

        println!(
            "Mapped {} bytes, {} segments; applied {} relocations",
            boot.size,
            boot.segments.len(),
            boot.relocs.len()
        );
        println!("Guest image at {image_base:#x}");

        let native_libc = matches!(
            boot.format,
            ProbeFormat::V3 | ProbeFormat::V4 | ProbeFormat::V5
        ) && !opts.strict_imports
            && (boot.capabilities & 1) != 0;
        if native_libc {
            for m in &boot.modules {
                let init_exec = boot.segments.iter().any(|s| {
                    (s.flags & 1) != 0 && m.init >= s.address && m.init - s.address < s.size
                });
                if !init_exec {
                    return Err((1, "module init is not executable".into()));
                }
                if m.tls_module != 0 {
                    crate::abi::libc::runtime_set_module_tls(
                        m.tls_module,
                        (image_base + m.tls_address) as usize as *const c_void,
                        m.tls_filesz,
                        m.tls_memsz,
                    );
                }
            }
            crate::thread::runtime_set_main_tls(
                (image_base + boot.main_tls.address) as usize as *const c_void,
                boot.main_tls.filesz,
                boot.main_tls.memsz,
                boot.main_tls.align,
            );
            crate::thread::runtime_thread_attach_main();
            crate::abi::libc::runtime_set_procparam(
                (image_base + boot.procparam) as usize as *mut c_void,
            );
            crate::thread::runtime_set_guest_fpu();
            for (idx, m) in boot.modules.iter().enumerate() {
                println!("Starting linked module {idx} at image offset {:#x}", m.init);
                // SAFETY: `m.init` was verified inside an executable segment of the image.
                let init_fn: extern "sysv64" fn(u64, *mut c_void, *mut c_void) -> c_int =
                    unsafe { std::mem::transmute((image_base + m.init) as usize) };
                let result = init_fn(0, std::ptr::null_mut(), std::ptr::null_mut());
                println!("Module {idx} initializer returned {result}");
                if result != 0 {
                    return Err((1, "module initializer failed".into()));
                }
            }
        }

        println!(
            "Entering original x86-64 code at guest offset {:#x}",
            boot.entry
        );
        ENTERED_GAME.store(true, Ordering::Release);

        let guest_argv = GuestArgv::from_file(opts.guest_argv_file.as_deref().map(Path::new));
        let mut params = guest_argv.as_params();

        const MAIN_STACK: usize = 8 * 1024 * 1024;
        let stack = crate::memory::abi::runtime_low_map(MAIN_STACK, 3);
        if stack.is_null() {
            return Err((1, "cannot allocate guest main stack".into()));
        }

        // Keep the guest image mapped for the lifetime of the process.
        let _ = mapped.into_raw();

        // SAFETY: `image_base + boot.entry` is in a validated executable segment, `params` and
        // `guest_exit` remain valid, and `stack + MAIN_STACK - 64` is 16-byte aligned.
        unsafe {
            enter_on_stack(
                (image_base + boot.entry) as usize as *const c_void,
                (&raw mut params).cast(),
                guest_exit as *const () as *mut c_void,
                stack.cast::<u8>().add(MAIN_STACK - 64).cast(),
            );
        }
        Err((1, "entry unexpectedly returned".into()))
    }
}

/// Validates/inspects or boots a `BBPROBE*` image according to `opts`, returning the process exit code.
pub fn run_boot(opts: &BootOptions) -> i32 {
    let bytes = match std::fs::read(&opts.boot_file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("uncharted-run: cannot open {}: {e}", opts.boot_file);
            return 1;
        }
    };
    let page_size = {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            // SAFETY: standard POSIX sysconf query.
            let ps = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            u64::try_from(ps).ok().filter(|&n| n > 0).unwrap_or(4096)
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            4096u64
        }
    };
    let boot = match BootImage::parse(&bytes, page_size) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("uncharted-run: {e}");
            return 1;
        }
    };
    if opts.list_missing {
        let missing = boot.missing_imports(opts.strict_imports);
        for m in &missing {
            let sym = m.symbol.unwrap_or("?");
            let suffix = if m.is_data { " (data)" } else { "" };
            println!("missing {} {sym}{suffix}", m.nid);
        }
        println!(
            "{} of {} imports missing",
            missing.len(),
            boot.import_names.len()
        );
        return if missing.is_empty() { 0 } else { 20 };
    }

    #[cfg(all(
        target_arch = "x86_64",
        any(target_os = "linux", target_os = "windows")
    ))]
    {
        match runtime_boot::boot(opts, &boot, &bytes[boot.image_offset..]) {
            Ok(()) => 0,
            Err((code, msg)) => {
                eprintln!("uncharted-run: {msg}");
                code
            }
        }
    }

    #[cfg(not(all(
        target_arch = "x86_64",
        any(target_os = "linux", target_os = "windows")
    )))]
    {
        eprintln!("uncharted-run: guest execution requires x86-64 Linux or Windows");
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn restart_command_rejects_empty_or_whitespace() {
        assert_eq!(restart_command_from_env(None), None);
        assert_eq!(restart_command_from_env(Some("")), None);
        assert_eq!(restart_command_from_env(Some("   ")), None);
        assert_eq!(
            restart_command_from_env(Some("uncharted-universal play u2")),
            Some("uncharted-universal play u2".to_string())
        );

        assert_eq!(restart_plan(None, false), RestartPlan::RunSh);
        assert_eq!(restart_plan(None, true), RestartPlan::MissingCommand);
        assert_eq!(
            restart_plan(Some("  foo --bar  "), true),
            RestartPlan::Command("foo --bar".to_string())
        );
    }

    #[test]
    fn load_content_profile_parses_valid_bbcont01_file() {
        let path =
            std::env::temp_dir().join(format!("uncharted-entry-test-{}.bin", std::process::id()));
        let mut raw = Vec::new();
        raw.extend_from_slice(b"BBCONT01");
        for v in [1u32, 2, 3, 4, 5] {
            raw.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&path, raw).unwrap();

        let opts = BootOptions {
            boot_file: "boot.bin".into(),
            cpu_only: true,
            strict_imports: false,
            list_missing: false,
            content_profile: Some(path.to_string_lossy().into_owned()),
            app0: None,
            user_dir: None,
            patch_files: Vec::new(),
            guest_argv_file: None,
            trophy_dir: None,
            timeout_seconds: 10,
        };
        let parsed = load_content_profile(&opts).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed, Some(ContentProfile([1, 2, 3, 4, 5])));
    }

    #[test]
    fn bbgpu_init_and_quit_once_guard_track_gpu_hooks() {
        static INIT_CALLS: AtomicUsize = AtomicUsize::new(0);
        static PREPARE_EXIT_CALLS: AtomicUsize = AtomicUsize::new(0);

        extern "C" fn ok_init(_cfg: *const BbGpuConfig) -> c_int {
            INIT_CALLS.fetch_add(1, Ordering::SeqCst);
            0
        }
        extern "C" fn fail_init(_cfg: *const BbGpuConfig) -> c_int {
            -1
        }
        extern "C" fn sym_count() -> u32 {
            17
        }

        configure_gpu_hooks(GpuHooks::default());
        let cfg = BbGpuConfig {
            title: c"Test".as_ptr(),
            serial: c"CUSA00900".as_ptr(),
            user_dir: c"user".as_ptr(),
            sdk_version: 0x0450_0001,
            psf_attributes: 0,
            width: 1920,
            height: 1080,
            prepare_render: None,
            render_context: std::ptr::null_mut(),
        };
        assert!(bbgpu_init(&cfg).is_err());
        assert!(!gpu_enabled());

        configure_gpu_hooks(GpuHooks {
            bbgpu_init: Some(fail_init),
            ..Default::default()
        });
        assert!(bbgpu_init(&cfg).is_err());
        assert!(!gpu_enabled());

        configure_gpu_hooks(GpuHooks {
            bbgpu_init: Some(ok_init),
            bbgpu_symbol_count: Some(sym_count),
            ..Default::default()
        });
        assert!(bbgpu_init(&cfg).is_ok());
        assert!(gpu_enabled());
        assert_eq!(INIT_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(bbgpu_symbol_count(), 17);

        let guard = AtomicBool::new(false);
        assert!(prepare_quit_once(&guard, true, || {
            PREPARE_EXIT_CALLS.fetch_add(1, Ordering::SeqCst);
        }));
        assert!(!prepare_quit_once(&guard, true, || {
            PREPARE_EXIT_CALLS.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(PREPARE_EXIT_CALLS.load(Ordering::SeqCst), 1);

        configure_gpu_hooks(GpuHooks::default());
        GPU_ENABLED.store(false, Ordering::Release);
    }

    #[test]
    fn guest_argv_follows_probe_load_exec_rules() {
        let default_argv = GuestArgv::from_text(None);
        assert_eq!(default_argv.args(), vec!["/app0/eboot.bin"]);
        assert_eq!(default_argv.as_params().argc, 1);

        let path_only = GuestArgv::from_text(Some("/app0/eboot.bin\n"));
        assert_eq!(path_only.args(), vec!["/app0/eboot.bin"]);

        let load_exec = GuestArgv::from_text(Some(
            "/app0/eboot2.bin\r\neboot.bin\r\nispartofuccollection\r\n",
        ));
        assert_eq!(load_exec.args(), vec!["eboot.bin", "ispartofuccollection"]);
        let params = load_exec.as_params();
        assert_eq!(params.argc, 2);
        assert!(!params.argv[0].is_null());
        assert!(!params.argv[1].is_null());
        assert!(params.argv[2].is_null());
    }

    #[test]
    fn sfo_lookup_parses_text_and_integer_entries() {
        // Minimal \0PSF with TITLE="Uncharted" (0x0204) and ATTRIBUTE=0x1234 (0x0404).
        let keys = b"TITLE\0ATTRIBUTE\0";
        let mut vals = Vec::new();
        vals.extend_from_slice(b"Uncharted\0\0\0"); // 12 bytes
        vals.extend_from_slice(&0x1234u32.to_le_bytes()); // 4 bytes
        let kbase = 20u32 + 2 * 16;
        let vbase = kbase + keys.len() as u32;

        let mut sfo = Vec::new();
        sfo.extend_from_slice(b"\0PSF");
        sfo.extend_from_slice(&0x0101u32.to_le_bytes());
        sfo.extend_from_slice(&kbase.to_le_bytes());
        sfo.extend_from_slice(&vbase.to_le_bytes());
        sfo.extend_from_slice(&2u32.to_le_bytes());

        // Entry 0: TITLE
        sfo.extend_from_slice(&0u16.to_le_bytes());
        sfo.extend_from_slice(&0x0204u16.to_le_bytes());
        sfo.extend_from_slice(&10u32.to_le_bytes());
        sfo.extend_from_slice(&12u32.to_le_bytes());
        sfo.extend_from_slice(&0u32.to_le_bytes());
        // Entry 1: ATTRIBUTE
        sfo.extend_from_slice(&6u16.to_le_bytes());
        sfo.extend_from_slice(&0x0404u16.to_le_bytes());
        sfo.extend_from_slice(&4u32.to_le_bytes());
        sfo.extend_from_slice(&4u32.to_le_bytes());
        sfo.extend_from_slice(&12u32.to_le_bytes());

        sfo.extend_from_slice(keys);
        sfo.extend_from_slice(&vals);

        assert_eq!(
            sfo_lookup_bytes(&sfo, "TITLE"),
            Some(SfoValue::Text("Uncharted".into()))
        );
        assert_eq!(
            sfo_lookup_bytes(&sfo, "ATTRIBUTE"),
            Some(SfoValue::Number(0x1234))
        );
        assert_eq!(sfo_lookup_bytes(&sfo, "MISSING"), None);
    }

    #[cfg(all(
        target_arch = "x86_64",
        any(target_os = "linux", target_os = "windows")
    ))]
    #[test]
    fn enter_on_stack_runs_sysv_entry_on_new_stack_and_returns() {
        use std::sync::atomic::AtomicU64;

        static SEEN_ARG0: AtomicU64 = AtomicU64::new(0);
        static SEEN_ARG1: AtomicU64 = AtomicU64::new(0);
        static SEEN_RSP: AtomicU64 = AtomicU64::new(0);

        extern "sysv64" fn fake_guest_entry(arg0: *mut c_void, arg1: *mut c_void) {
            let mut local = 0u64;
            let rsp_approx = (&raw mut local) as usize as u64;
            SEEN_ARG0.store(arg0 as usize as u64, Ordering::SeqCst);
            SEEN_ARG1.store(arg1 as usize as u64, Ordering::SeqCst);
            SEEN_RSP.store(rsp_approx, Ordering::SeqCst);
        }

        let stack = vec![0u8; 64 * 1024];
        let lo = stack.as_ptr() as usize as u64;
        let hi = lo + stack.len() as u64;
        let top = (hi - 64) as usize as *mut c_void;

        // SAFETY: `fake_guest_entry` is SysV64 and `stack` has 64 KiB writable below `top`.
        unsafe {
            enter_on_stack(
                fake_guest_entry as *const () as *const c_void,
                0x1111_2222usize as *mut c_void,
                0x3333_4444usize as *mut c_void,
                top,
            );
        }

        assert_eq!(SEEN_ARG0.load(Ordering::SeqCst), 0x1111_2222);
        assert_eq!(SEEN_ARG1.load(Ordering::SeqCst), 0x3333_4444);
        let got_rsp = SEEN_RSP.load(Ordering::SeqCst);
        assert!(
            (lo..hi).contains(&got_rsp),
            "guest rsp {got_rsp:#x} was not inside new stack [{lo:#x}, {hi:#x})"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn runtime_quit_invokes_prepare_exit_once_and_exits_with_code() {
        static MARKER_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
        extern "C" fn ok_init(_cfg: *const BbGpuConfig) -> c_int {
            0
        }
        extern "C" fn on_prepare_exit() {
            let fd = MARKER_FD.load(Ordering::SeqCst);
            if fd >= 0 {
                unsafe {
                    libc::write(fd, b"GPU_PREPARED_EXIT\n".as_ptr().cast(), 18);
                }
            }
        }

        let mut pipefd = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(pipefd.as_mut_ptr()) }, 0);
        MARKER_FD.store(pipefd[1], Ordering::SeqCst);
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            unsafe {
                libc::close(pipefd[0]);
            }
            configure_gpu_hooks(GpuHooks {
                bbgpu_init: Some(ok_init),
                bbgpu_prepare_exit: Some(on_prepare_exit),
                ..Default::default()
            });
            let cfg = BbGpuConfig {
                title: c"Test".as_ptr(),
                serial: c"CUSA00900".as_ptr(),
                user_dir: c"user".as_ptr(),
                sdk_version: 0,
                psf_attributes: 0,
                width: 1920,
                height: 1080,
                prepare_render: None,
                render_context: std::ptr::null_mut(),
            };
            bbgpu_init(&cfg).unwrap();
            runtime_quit(20);
        }
        MARKER_FD.store(-1, Ordering::SeqCst);
        unsafe { libc::close(pipefd[1]) };
        let mut out = Vec::new();
        let mut buf = [0u8; 64];
        loop {
            let n = unsafe { libc::read(pipefd[0], buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            out.extend_from_slice(&buf[..n as usize]);
        }
        unsafe { libc::close(pipefd[0]) };
        let mut status: libc::c_int = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 20);
        assert_eq!(String::from_utf8_lossy(&out), "GPU_PREPARED_EXIT\n");
    }

    #[test]
    fn should_auto_render_size_respects_overrides_and_patches() {
        let none = |_: &str| None;
        assert!(should_auto_render_size(false, &none));
        assert!(!should_auto_render_size(true, &none));

        let empty_overrides = |name: &str| match name {
            "BB_GAME_RES" | "BB_RENDER_RES" | "BB_RENDER_720P" => Some(String::new()),
            _ => None,
        };
        assert!(should_auto_render_size(false, &empty_overrides));

        for (key, val) in [
            ("BB_GAME_RES", "2560x1440"),
            ("BB_GAME_RES", "1920x1080"),
            ("BB_RENDER_RES", "1600x900"),
            ("BB_RENDER_720P", "1"),
        ] {
            let env = |name: &str| (name == key).then(|| val.to_owned());
            assert!(!should_auto_render_size(false, &env), "{key}={val}");
        }

        let deck_off = |name: &str| (name == "BB_RENDER_720P").then(|| "0".to_owned());
        assert!(should_auto_render_size(false, &deck_off));
    }
}
