//! The audio device behind [`super`]: SDL3 audio streams (`sdl3-sys`) in the loader build, no
//! device anywhere else.
//!
//! `sdl3-sys` is built with `no-link`: the loader already links SDL3 (`build.sh` through
//! pkg-config, `gpu/CMakeLists.txt` through `bbgpu`), and `cargo test` and the mobile builds link
//! no C (`build.rs`, `cfg(uncharted_loader)`). Without the loader every port runs on the timer
//! sink, as the C module did with `BB_AUDIO=none` or without an audio device.

use std::ffi::CStr;

/// The format of one side of a stream: 16-bit or float samples, channels, rate in Hz.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Spec {
    pub float: bool,
    pub channels: usize,
    pub freq: i32,
}

#[cfg(uncharted_loader)]
pub(super) use sdl::*;

#[cfg(not(uncharted_loader))]
pub(super) use none::*;
// The bindings are a dependency of every build (one manifest); only the loader build calls them.
#[cfg(not(uncharted_loader))]
use sdl3_sys as _;

#[cfg(uncharted_loader)]
mod sdl {
    use super::Spec;
    use sdl3_sys::audio::{
        SDL_AudioSpec, SDL_AudioStream, SDL_DestroyAudioStream, SDL_GetAudioDeviceFormat,
        SDL_GetAudioStreamQueued, SDL_GetCurrentAudioDriver, SDL_OpenAudioDeviceStream,
        SDL_PutAudioStreamData, SDL_ResumeAudioStreamDevice, SDL_SetAudioStreamFormat,
        SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, SDL_AUDIO_F32, SDL_AUDIO_S16,
    };
    use sdl3_sys::error::SDL_GetError;
    use sdl3_sys::hints::{SDL_GetHint, SDL_ResetHint, SDL_SetHint, SDL_HINT_AUDIO_DRIVER};
    use sdl3_sys::init::{SDL_InitSubSystem, SDL_QuitSubSystem, SDL_INIT_AUDIO};
    use std::ffi::{c_char, c_int, CStr};
    use std::ptr::{null, null_mut, NonNull};

    /// A string SDL owns, copied; `None` for null.
    fn text(pointer: *const c_char) -> Option<String> {
        // SAFETY: SDL returns null or a NUL-terminated string that lives until the next call.
        (!pointer.is_null()).then(|| unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into())
    }

    fn sdl_spec(spec: Spec) -> SDL_AudioSpec {
        SDL_AudioSpec {
            format: if spec.float {
                SDL_AUDIO_F32
            } else {
                SDL_AUDIO_S16
            },
            channels: spec.channels as c_int,
            freq: spec.freq,
        }
    }

    /// One SDL audio stream bound to the default playback device; destroyed on drop.
    pub(in crate::audio) struct Stream(NonNull<SDL_AudioStream>);

    // SAFETY: SDL audio streams lock themselves; each one is used under its port's mutex.
    unsafe impl Send for Stream {}

    impl Stream {
        /// Bytes queued and not yet played.
        pub fn queued(&self) -> i32 {
            // SAFETY: a live stream.
            unsafe { SDL_GetAudioStreamQueued(self.0.as_ptr()) }
        }

        /// Queues `data` (a whole number of frames of the stream's input format).
        pub fn put(&self, data: &[u8]) {
            // SAFETY: a live stream; SDL copies `data.len()` bytes from `data`.
            unsafe {
                SDL_PutAudioStreamData(self.0.as_ptr(), data.as_ptr().cast(), data.len() as c_int)
            };
        }

        /// Starts the device: SDL opens it paused.
        pub fn resume(&self) {
            // SAFETY: a live stream.
            unsafe { SDL_ResumeAudioStreamDevice(self.0.as_ptr()) };
        }
    }

    impl Drop for Stream {
        fn drop(&mut self) {
            // SAFETY: the stream is live and dropped once.
            unsafe { SDL_DestroyAudioStream(self.0.as_ptr()) };
        }
    }

    /// `SDL_AUDIO_DRIVER` (hint or environment) names a backend.
    pub fn driver_hint() -> bool {
        // SAFETY: a hint name; SDL returns null or a string it owns.
        text(unsafe { SDL_GetHint(SDL_HINT_AUDIO_DRIVER) }).is_some_and(|hint| !hint.is_empty())
    }

    /// Starts SDL's audio subsystem.
    pub fn init() -> bool {
        // SAFETY: SDL counts subsystem starts; any thread may start audio.
        unsafe { SDL_InitSubSystem(SDL_INIT_AUDIO) }
    }

    /// Stops SDL's audio subsystem, dropping every stream and device.
    pub fn quit() {
        // SAFETY: as `init`; no port holds a stream when this is called.
        unsafe { SDL_QuitSubSystem(SDL_INIT_AUDIO) };
    }

    /// Sets the audio driver hint to `driver`, or back to its default for `None`.
    pub fn set_driver(driver: Option<&CStr>) {
        // SAFETY: NUL-terminated hint name and value.
        unsafe {
            match driver {
                Some(driver) => SDL_SetHint(SDL_HINT_AUDIO_DRIVER, driver.as_ptr()),
                None => SDL_ResetHint(SDL_HINT_AUDIO_DRIVER),
            }
        };
    }

    /// The running audio driver ("pipewire", "wasapi", ...).
    pub fn current_driver() -> Option<String> {
        // SAFETY: SDL returns null or a static driver name.
        text(unsafe { SDL_GetCurrentAudioDriver() })
    }

    /// Channels of the default playback device, or `None` when SDL cannot tell.
    pub fn default_channels() -> Option<i32> {
        let mut spec = SDL_AudioSpec {
            format: SDL_AUDIO_S16,
            channels: 0,
            freq: 0,
        };
        // SAFETY: `spec` is writable; the frame count is optional (null).
        let known = unsafe {
            SDL_GetAudioDeviceFormat(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, &mut spec, null_mut())
        };
        known.then_some(spec.channels)
    }

    /// SDL's last error message on this thread ("" when there is none).
    pub fn error() -> String {
        text(SDL_GetError()).unwrap_or_default()
    }

    /// Opens a stream on the default playback device as `device`; with `input`, the game's
    /// samples keep that format and SDL converts them to what the device took.
    pub fn open(device: Spec, input: Option<Spec>) -> Option<Stream> {
        let spec = sdl_spec(device);
        // SAFETY: `spec` is a valid spec; no callback, so no user data.
        let stream = unsafe {
            SDL_OpenAudioDeviceStream(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, &spec, None, null_mut())
        };
        let stream = Stream(NonNull::new(stream)?);
        if let Some(input) = input {
            let input = sdl_spec(input);
            // SAFETY: a live stream and a valid source spec; the device side stays as it is.
            if !unsafe { SDL_SetAudioStreamFormat(stream.0.as_ptr(), &input, null()) } {
                return None;
            }
        }
        Some(stream)
    }
}

#[cfg(not(uncharted_loader))]
mod none {
    use super::Spec;
    use std::ffi::CStr;

    /// No stream can be opened without the loader.
    pub(in crate::audio) enum Stream {}

    impl Stream {
        pub fn queued(&self) -> i32 {
            match *self {}
        }

        pub fn put(&self, _data: &[u8]) {
            match *self {}
        }

        pub fn resume(&self) {
            match *self {}
        }
    }

    pub fn driver_hint() -> bool {
        false
    }

    pub fn init() -> bool {
        false
    }

    pub fn quit() {}

    pub fn set_driver(_driver: Option<&CStr>) {}

    pub fn current_driver() -> Option<String> {
        None
    }

    pub fn default_channels() -> Option<i32> {
        None
    }

    pub fn error() -> String {
        String::new()
    }

    pub fn open(_device: Spec, _input: Option<Spec>) -> Option<Stream> {
        None
    }
}

/// The driver SDL falls back to on Windows when WASAPI cannot open a stream (SDL3 has no winmm).
pub(super) const DIRECTSOUND: &CStr = c"directsound";
