//! ATRAC9 through LibAtrac9 (MIT, `third_party/LibAtrac9`), which `build.rs` compiles into this
//! library on x86-64 hosts. The decoder keeps global tables that it fills on every
//! initialization, so every call here happens under the module's lock ([`super::STATE`]).

use std::ffi::{c_int, c_short, c_uchar, c_void};
use std::ptr::NonNull;

/// LibAtrac9's `Atrac9CodecInfo`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) struct CodecInfo {
    pub channels: c_int,
    pub channel_config_index: c_int,
    pub sampling_rate: c_int,
    pub superframe_size: c_int,
    pub frames_in_superframe: c_int,
    pub frame_samples: c_int,
    pub wlength: c_int,
    pub config_data: [c_uchar; 4],
}

extern "C" {
    fn Atrac9GetHandle() -> *mut c_void;
    fn Atrac9ReleaseHandle(handle: *mut c_void);
    fn Atrac9InitDecoder(handle: *mut c_void, config: *mut c_uchar) -> c_int;
    fn Atrac9Decode(
        handle: *mut c_void,
        audio: *const c_uchar,
        pcm: *mut c_short,
        used: *mut c_int,
    ) -> c_int;
    fn Atrac9GetCodecInfo(handle: *mut c_void, info: *mut CodecInfo) -> c_int;
}

/// The most interleaved samples one frame can hold: 256 samples, 8 channels.
pub(super) const MAX_FRAME_PCM: usize = 256 * 8;

/// One LibAtrac9 decoder (`Atrac9GetHandle`), released on drop.
pub(super) struct Handle(NonNull<c_void>);

// SAFETY: the handle is plain heap memory owned by this value; LibAtrac9 keeps no thread state.
unsafe impl Send for Handle {}

impl Handle {
    pub fn new() -> Self {
        // SAFETY: allocates a zeroed decoder; no arguments.
        let raw = unsafe { Atrac9GetHandle() };
        Handle(NonNull::new(raw).expect("LibAtrac9: out of memory for a decoder"))
    }

    /// `Atrac9InitDecoder`: true when the four configuration bytes describe a stream.
    pub fn init(&mut self, config: [u8; 4]) -> bool {
        let mut config = config;
        // SAFETY: a live handle and four readable configuration bytes.
        unsafe { Atrac9InitDecoder(self.0.as_ptr(), config.as_mut_ptr()) == 0 }
    }

    pub fn info(&self) -> CodecInfo {
        let mut info = CodecInfo::default();
        // SAFETY: a live handle and a writable `Atrac9CodecInfo`.
        unsafe { Atrac9GetCodecInfo(self.0.as_ptr(), &mut info) };
        info
    }

    /// `Atrac9Decode`: one frame from `input` into `pcm` (interleaved). Returns LibAtrac9's
    /// status (0 on success) and the bytes the frame used.
    ///
    /// # Safety
    /// `input` points to readable memory holding at least one frame, as the C runtime passed it:
    /// the decoder reads the bits the frame needs, without a length.
    pub unsafe fn decode(
        &mut self,
        input: *const u8,
        pcm: &mut [i16; MAX_FRAME_PCM],
    ) -> (i32, i32) {
        let mut used: c_int = 0;
        // SAFETY: a live handle, the caller's input, and a PCM buffer large enough for any frame
        // (the caller checked frame samples times channels against MAX_FRAME_PCM).
        let status = unsafe { Atrac9Decode(self.0.as_ptr(), input, pcm.as_mut_ptr(), &mut used) };
        (status, used)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from `Atrac9GetHandle` and is released once.
        unsafe { Atrac9ReleaseHandle(self.0.as_ptr()) };
    }
}
