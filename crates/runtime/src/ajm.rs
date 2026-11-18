//! libSceAjm: audio decoder batches (piece 3.08; order 8 in docs/code-map.md), ported from the
//! deleted `src/runtime_ajm.c`. `tests/test_ajm.c` is this module's first test.
//!
//! Batches use the SDK chunk format (the builder functions are imports, but games may also build
//! chunks inline) and run synchronously at submission: decoding a batch takes well under a
//! millisecond, so waiting always finds it complete. ATRAC9 is decoded by LibAtrac9 (MIT,
//! `third_party/LibAtrac9`, linked into the loader; [`At9Decoder`] is the seam, so tests use a
//! stand-in). MP3 is decoded by `symphonia-bundle-mp3` (pure Rust, MPL-2.0), which replaces the
//! FFmpeg decoder of the C file. Licence (#299): FFmpeg's libavcodec is built here as GPL-3.0,
//! which this GPL-2.0-only project could not combine with; MPL-2.0 can (its section 3.3 names
//! GPL-2.0 as a Secondary License), so AJM no longer depends on FFmpeg. (The GPU library still
//! links FFmpeg for video, which #299 settles on its own.) U3's London streets create an MP3
//! instance. AAC instances stop explicitly: an unsupported codec returns an error (#56, #132).
//!
//! `BB_AUDIO_TRACE=1` prints instance configurations and jobs that return a non-zero result;
//! `BB_AUDIO_TRACE=2` prints every job.

// Only x86-64 hosts run guest code: elsewhere the guest entry points are not built.
#![cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]

use std::collections::{BTreeMap, HashMap};
#[cfg(target_arch = "x86_64")]
use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use symphonia_bundle_mp3::MpaDecoder;
use symphonia_core::audio::SampleBuffer;
use symphonia_core::codecs::{CodecParameters, Decoder, DecoderOptions, CODEC_TYPE_MP3};
use symphonia_core::formats::Packet;

pub const ERR_INVALID_CONTEXT: i32 = 0x8093_0002_u32 as i32;
pub const ERR_INVALID_INSTANCE: i32 = 0x8093_0003_u32 as i32;
pub const ERR_INVALID_BATCH: i32 = 0x8093_0004_u32 as i32;
pub const ERR_INVALID_PARAMETER: i32 = 0x8093_0005_u32 as i32;
pub const ERR_OUT_OF_RESOURCES: i32 = 0x8093_0007_u32 as i32;
pub const ERR_ALREADY_REGISTERED: i32 = 0x8093_0009_u32 as i32;
pub const ERR_NOT_REGISTERED: i32 = 0x8093_000A_u32 as i32;
pub const ERR_WRONG_REVISION: i32 = 0x8093_000B_u32 as i32;
pub const ERR_MALFORMED_BATCH: i32 = 0x8093_0011_u32 as i32;
pub const ERR_CANCELLED: i32 = 0x8093_0017_u32 as i32;
const RESULT_NOT_INITIALIZED: u32 = 0x1;
const RESULT_INVALID_PARAMETER: u32 = 0x4;
const RESULT_PARTIAL_INPUT: u32 = 0x8;
const RESULT_NOT_ENOUGH_ROOM: u32 = 0x10;
const RESULT_CODEC_ERROR: u32 = 0x4000_0000;
const RESULT_FATAL: u32 = 0x8000_0000;
const STATISTICS_INSTANCE: u32 = 0x80000;
const MAX_INSTANCES: u32 = 0x3fff;
const MAX_CONTEXTS: u32 = 8;
const MAX_BATCHES: usize = 256;
const CODECS: usize = 24;

const IDENT_JOB: u32 = 0;
const IDENT_INPUT_RUN: u32 = 1;
const IDENT_INPUT_CONTROL: u32 = 2;
const IDENT_CONTROL_FLAGS: u32 = 3;
const IDENT_RUN_FLAGS: u32 = 4;
const IDENT_RETURN_ADDRESS: u32 = 6;
const IDENT_INLINE: u32 = 7;
const IDENT_OUTPUT_RUN: u32 = 17;
const IDENT_OUTPUT_CONTROL: u32 = 18;

const FORMAT_S16: u32 = 0;
const FORMAT_S32: u32 = 1;
const FORMAT_FLOAT: u32 = 2;

// Job flags: version:3 codec:8 run:2 control:3 reserved:29 sideband:3.
fn bit(flags: u64, n: u32) -> bool {
    (flags >> n) & 1 != 0
}
fn run_codec_info(f: u64) -> bool {
    bit(f, 11)
}
fn run_multiple_frames(f: u64) -> bool {
    bit(f, 12)
}
fn control_reset(f: u64) -> bool {
    bit(f, 13)
}
fn control_initialize(f: u64) -> bool {
    bit(f, 14)
}
fn control_resample(f: u64) -> bool {
    bit(f, 15)
}
fn sideband_gapless(f: u64) -> bool {
    bit(f, 45)
}
fn sideband_format(f: u64) -> bool {
    bit(f, 46)
}
fn sideband_stream(f: u64) -> bool {
    bit(f, 47)
}

/// Size of a chunk header (`ident:6 payload:20 reserved:6`, then a 32-bit size).
const CHUNK: usize = 8;
/// Size of a buffer chunk: the header and the guest address.
const BUFFER_CHUNK: usize = 16;

fn ident(word: u32) -> u32 {
    word & 0x3f
}
fn payload(word: u32) -> u32 {
    (word >> 6) & 0xf_ffff
}
fn header(id: u32, value: u32) -> u32 {
    (id & 0x3f) | ((value & 0xf_ffff) << 6)
}
fn channel_mask(channels: i32) -> u32 {
    match channels {
        1 => 4,
        2 => 3,
        4 => 0x33,
        6 => 0x60f,
        8 => 0x63f,
        _ => 0,
    }
}
fn pcm_size(format: u32) -> u32 {
    if format == FORMAT_S16 {
        2
    } else {
        4
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

// ---- tracing and counters ----

static JOBS_RUN: AtomicUsize = AtomicUsize::new(0);
static FRAMES_DECODED: AtomicUsize = AtomicUsize::new(0);
static BATCHES_RUN: AtomicUsize = AtomicUsize::new(0);

static TRACE_LEFT: AtomicI64 = AtomicI64::new(-1);
static TRACE_ALL: AtomicBool = AtomicBool::new(false);

/// Whether the next trace line is printed (`BB_AUDIO_TRACE`, a budget of 400 lines, or 200000
/// with `=2`).
fn trace() -> bool {
    if TRACE_LEFT.load(Ordering::Relaxed) < 0 {
        let setting = std::env::var("BB_AUDIO_TRACE").unwrap_or_default();
        let all = setting.starts_with('2');
        TRACE_ALL.store(all, Ordering::Relaxed);
        let budget = if all {
            200_000
        } else if setting.starts_with('1') {
            400
        } else {
            0
        };
        TRACE_LEFT.store(budget, Ordering::Relaxed);
    }
    TRACE_LEFT.load(Ordering::Relaxed) > 0 && TRACE_LEFT.fetch_sub(1, Ordering::Relaxed) > 0
}

fn note(line: &str) {
    #[cfg(target_arch = "x86_64")]
    crate::abi::host::out_line(line);
    #[cfg(not(target_arch = "x86_64"))]
    println!("{line}");
}

fn warn(line: &str) {
    #[cfg(target_arch = "x86_64")]
    crate::abi::host::err(&format!("{line}\n"));
    #[cfg(not(target_arch = "x86_64"))]
    eprintln!("{line}");
}

// ---- guest buffers ----

/// A buffer chunk of a batch: the guest address and size of an input or output.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ChunkBuffer {
    pub word: u32,
    pub size: u32,
    pub address: usize,
}

impl ChunkBuffer {
    fn parse(bytes: &[u8]) -> Option<Self> {
        let address = u64::from_le_bytes(bytes.get(8..16)?.try_into().ok()?);
        Some(ChunkBuffer {
            word: u32_at(bytes, 0)?,
            size: u32_at(bytes, 4)?,
            address: address as usize,
        })
    }

    fn encode(self) -> [u8; BUFFER_CHUNK] {
        let mut out = [0u8; BUFFER_CHUNK];
        out[0..4].copy_from_slice(&self.word.to_le_bytes());
        out[4..8].copy_from_slice(&self.size.to_le_bytes());
        out[8..16].copy_from_slice(&(self.address as u64).to_le_bytes());
        out
    }

    /// The guest memory behind the chunk, read-only; `None` when empty.
    fn bytes<'a>(&self) -> Option<&'a [u8]> {
        if self.size == 0 || self.address == 0 {
            return None;
        }
        // SAFETY: the guest passed this buffer in a batch and keeps it valid and unaliased by
        // writes for the job's duration (the batch runs synchronously inside the guest's call).
        Some(unsafe { std::slice::from_raw_parts(self.address as *const u8, self.size as usize) })
    }

    /// The guest memory behind the chunk, writable; `None` when empty.
    fn bytes_mut<'a>(&self) -> Option<&'a mut [u8]> {
        if self.size == 0 || self.address == 0 {
            return None;
        }
        // SAFETY: as `bytes`, for a buffer the guest gave to the job to write.
        Some(unsafe { std::slice::from_raw_parts_mut(self.address as *mut u8, self.size as usize) })
    }
}

/// An input or output buffer of a split job, as the guest passes it (`SceAjmBuffer`).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct AjmBuffer {
    pub address: usize,
    pub size: u64,
}

/// `SceAjmBatchError`, written when a batch is malformed.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct BatchError {
    pub error_code: i32,
    pub job_address: usize,
    pub command_offset: u32,
    pub job_return_address: usize,
}

// ---- batch builders ----

fn put_buffer(out: &mut Vec<u8>, id: u32, address: usize, size: u64) {
    let chunk = ChunkBuffer {
        word: header(id, 0),
        size: size as u32,
        address,
    };
    out.extend_from_slice(&chunk.encode());
}

fn put_flags(out: &mut Vec<u8>, id: u32, flags: u64) {
    out.extend_from_slice(&header(id, (flags >> 32) as u32).to_le_bytes());
    out.extend_from_slice(&(flags as u32).to_le_bytes());
}

fn finish_job(mut body: Vec<u8>, instance: u32) -> Vec<u8> {
    let size = (body.len() - CHUNK) as u32;
    body[0..4].copy_from_slice(&header(IDENT_JOB, instance).to_le_bytes());
    body[4..8].copy_from_slice(&size.to_le_bytes());
    body
}

fn start_job(return_address: usize) -> Vec<u8> {
    let mut out = vec![0u8; CHUNK];
    if return_address != 0 {
        put_buffer(&mut out, IDENT_RETURN_ADDRESS, return_address, 0);
    }
    out
}

/// The bytes of a control job (`sceAjmBatchJobControlBufferRa`).
pub fn build_control(
    instance: u32,
    flags: u64,
    input: ChunkBuffer,
    output: ChunkBuffer,
    return_address: usize,
) -> Vec<u8> {
    let mut out = start_job(return_address);
    put_buffer(
        &mut out,
        IDENT_INPUT_CONTROL,
        input.address,
        u64::from(input.size),
    );
    let mask = if instance == STATISTICS_INSTANCE {
        0xC001_8007
    } else {
        0x6000_0000_E7FF
    };
    put_flags(&mut out, IDENT_CONTROL_FLAGS, flags & mask);
    put_buffer(
        &mut out,
        IDENT_OUTPUT_CONTROL,
        output.address,
        u64::from(output.size),
    );
    finish_job(out, instance)
}

/// The bytes of a run job (`sceAjmBatchJobRunBufferRa`).
pub fn build_run(
    instance: u32,
    flags: u64,
    input: ChunkBuffer,
    output: ChunkBuffer,
    sideband: ChunkBuffer,
    return_address: usize,
) -> Vec<u8> {
    build_run_split(
        instance,
        flags,
        &[input],
        &[output],
        sideband,
        return_address,
    )
}

/// The bytes of a run job with several inputs and outputs (`sceAjmBatchJobRunSplitBufferRa`).
pub fn build_run_split(
    instance: u32,
    flags: u64,
    inputs: &[ChunkBuffer],
    outputs: &[ChunkBuffer],
    sideband: ChunkBuffer,
    return_address: usize,
) -> Vec<u8> {
    let mut out = start_job(return_address);
    for input in inputs {
        put_buffer(
            &mut out,
            IDENT_INPUT_RUN,
            input.address,
            u64::from(input.size),
        );
    }
    put_flags(&mut out, IDENT_RUN_FLAGS, flags & 0xE000_0000_1FFF);
    for output in outputs {
        put_buffer(
            &mut out,
            IDENT_OUTPUT_RUN,
            output.address,
            u64::from(output.size),
        );
    }
    put_buffer(
        &mut out,
        IDENT_OUTPUT_CONTROL,
        sideband.address,
        u64::from(sideband.size),
    );
    finish_job(out, instance)
}

/// An inline chunk (`sceAjmBatchJobInlineBuffer`): the header, then `data` padded to 8 bytes.
/// The data's offset from the start is [`CHUNK`].
pub fn build_inline(data: &[u8]) -> Vec<u8> {
    let size = (data.len() + 7) & !7;
    let mut out = Vec::with_capacity(CHUNK + size);
    out.extend_from_slice(&header(IDENT_INLINE, 0).to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(data);
    out.resize(CHUNK + size, 0);
    out
}

// ---- ATRAC9 ----

/// What LibAtrac9 reports about a stream (`Atrac9CodecInfo`, the fields used).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct CodecInfo {
    pub channels: i32,
    pub sampling_rate: i32,
    pub superframe_size: i32,
    pub frames_in_superframe: i32,
    pub frame_samples: i32,
}

/// An ATRAC9 frame decoder: LibAtrac9 in the loader build, a stand-in in tests.
pub trait At9Decoder: Send {
    /// Starts the stream described by the 4-byte config; `None` if it is not valid.
    fn init(&mut self, config: [u8; 4]) -> Option<CodecInfo>;
    /// Decodes the frame at the start of `input` into `pcm` (interleaved) and returns the
    /// bytes it used, or LibAtrac9's error code.
    fn decode(&mut self, input: &[u8], pcm: &mut [i16]) -> Result<usize, i32>;
}

/// No decoder: every ATRAC9 stream reports "not initialized". Builds without LibAtrac9.
struct NoAt9;

impl At9Decoder for NoAt9 {
    fn init(&mut self, _config: [u8; 4]) -> Option<CodecInfo> {
        None
    }
    fn decode(&mut self, _input: &[u8], _pcm: &mut [i16]) -> Result<usize, i32> {
        Err(-1)
    }
}

#[cfg(uncharted_loader)]
mod libatrac9 {
    use super::{At9Decoder, CodecInfo};
    use std::ffi::{c_int, c_void};

    /// `Atrac9CodecInfo` of `third_party/LibAtrac9/C/src/libatrac9.h`, with room to spare.
    #[repr(C)]
    #[derive(Default)]
    struct RawInfo {
        channels: c_int,
        channel_config_index: c_int,
        sampling_rate: c_int,
        superframe_size: c_int,
        frames_in_superframe: c_int,
        frame_samples: c_int,
        wlength: c_int,
        config_data: [u8; 4],
        spare: [u8; 16],
    }

    extern "C" {
        fn Atrac9GetHandle() -> *mut c_void;
        fn Atrac9ReleaseHandle(handle: *mut c_void);
        fn Atrac9InitDecoder(handle: *mut c_void, config: *mut u8) -> c_int;
        fn Atrac9Decode(
            handle: *mut c_void,
            input: *const u8,
            pcm: *mut i16,
            used: *mut c_int,
        ) -> c_int;
        fn Atrac9GetCodecInfo(handle: *mut c_void, info: *mut RawInfo) -> c_int;
    }

    /// A LibAtrac9 decoder handle, released on drop.
    pub(super) struct Handle(*mut c_void);

    // SAFETY: the handle is owned by one instance and used under the module's lock.
    unsafe impl Send for Handle {}

    impl Handle {
        pub(super) fn new() -> Self {
            // SAFETY: returns a new handle (or null, which every call below accepts as an error).
            Handle(unsafe { Atrac9GetHandle() })
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the handle came from `Atrac9GetHandle` and is released once.
                unsafe { Atrac9ReleaseHandle(self.0) };
            }
        }
    }

    impl At9Decoder for Handle {
        fn init(&mut self, mut config: [u8; 4]) -> Option<CodecInfo> {
            if self.0.is_null() {
                return None;
            }
            // SAFETY: a live handle and a 4-byte config; the info struct is as large as C's.
            unsafe {
                if Atrac9InitDecoder(self.0, config.as_mut_ptr()) != 0 {
                    return None;
                }
                let mut raw = RawInfo::default();
                Atrac9GetCodecInfo(self.0, &mut raw);
                Some(CodecInfo {
                    channels: raw.channels,
                    sampling_rate: raw.sampling_rate,
                    superframe_size: raw.superframe_size,
                    frames_in_superframe: raw.frames_in_superframe,
                    frame_samples: raw.frame_samples,
                })
            }
        }

        fn decode(&mut self, input: &[u8], pcm: &mut [i16]) -> Result<usize, i32> {
            if self.0.is_null() {
                return Err(-1);
            }
            let mut used: c_int = 0;
            // SAFETY: a live handle; `pcm` holds the frame the caller sized it for (it checked
            // frame samples times channels), and LibAtrac9 reads one frame of `input`, as the C
            // runtime handed it the guest's pointer.
            let error =
                unsafe { Atrac9Decode(self.0, input.as_ptr(), pcm.as_mut_ptr(), &mut used) };
            if error != 0 {
                Err(error)
            } else {
                Ok(usize::try_from(used).unwrap_or(0))
            }
        }
    }
}

fn default_at9() -> Box<dyn At9Decoder> {
    #[cfg(uncharted_loader)]
    {
        Box::new(libatrac9::Handle::new())
    }
    #[cfg(not(uncharted_loader))]
    {
        Box::new(NoAt9)
    }
}

// ---- sidebands ----

/// `SceAjmSidebandGaplessDecode`: total samples, samples to skip, samples skipped.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Gapless {
    pub total_samples: u32,
    pub skip_samples: u16,
    pub skipped_samples: u16,
}

impl Gapless {
    fn parse(bytes: &[u8]) -> Option<Self> {
        Some(Gapless {
            total_samples: u32_at(bytes, 0)?,
            skip_samples: u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?),
            skipped_samples: u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?),
        })
    }

    fn encode(self) -> [u8; 8] {
        let mut out = [0u8; 8];
        out[0..4].copy_from_slice(&self.total_samples.to_le_bytes());
        out[4..6].copy_from_slice(&self.skip_samples.to_le_bytes());
        out[6..8].copy_from_slice(&self.skipped_samples.to_le_bytes());
        out
    }
}

/// Writes into the output control (sideband) buffer in order, only where it still fits.
struct Side<'a> {
    bytes: Option<&'a mut [u8]>,
    at: usize,
}

impl Side<'_> {
    fn has(&self, len: usize) -> bool {
        self.bytes
            .as_ref()
            .is_some_and(|b| self.at + len <= b.len())
    }

    /// Appends `data` if the buffer exists and has the room.
    fn put(&mut self, data: &[u8]) -> bool {
        let at = self.at;
        match self.bytes.as_deref_mut() {
            Some(bytes) if at + data.len() <= bytes.len() => {
                bytes[at..at + data.len()].copy_from_slice(data);
                self.at += data.len();
                true
            }
            _ => false,
        }
    }

    fn zero_rest(&mut self) {
        let at = self.at;
        if let Some(bytes) = self.bytes.as_deref_mut() {
            bytes[at..].fill(0);
        }
    }
}

// ---- output ----

/// The output chunks of a job, filled in order.
struct Output<'a> {
    chunks: &'a [ChunkBuffer],
    current: usize,
    offset: u32,
}

impl Output<'_> {
    fn room(&self) -> u64 {
        self.chunks
            .iter()
            .enumerate()
            .skip(self.current)
            .map(|(i, c)| u64::from(c.size - if i == self.current { self.offset } else { 0 }))
            .sum()
    }

    fn write(&mut self, mut data: &[u8]) {
        while !data.is_empty() && self.current < self.chunks.len() {
            let chunk = self.chunks[self.current];
            let n = ((chunk.size - self.offset) as usize).min(data.len());
            if chunk.address != 0 {
                // SAFETY: the guest gave this output buffer to the job, `offset + n` is within
                // its size.
                let target = unsafe {
                    std::slice::from_raw_parts_mut(
                        (chunk.address as *mut u8).add(self.offset as usize),
                        n,
                    )
                };
                target.copy_from_slice(&data[..n]);
            }
            data = &data[n..];
            self.offset += n as u32;
            if self.offset == chunk.size {
                self.current += 1;
                self.offset = 0;
            }
        }
    }
}

// ---- instances ----

struct Instance {
    codec: u32,
    format: u32,
    gapless_loop: bool,
    codec_flags: u32,
    factory: fn() -> Box<dyn At9Decoder>,
    at9: Option<Box<dyn At9Decoder>>,
    config: [u8; 4],
    initialized: bool,
    info: CodecInfo,
    superframe_remain: u32,
    frames: u32,
    gapless_init: Gapless,
    gapless: Gapless,
    total_samples: u64,
    /// MP3: the decoder, and the last frame header (format and codec-info sidebands).
    mp3: Option<MpaDecoder>,
    mp3_header: u32,
    mp3_channels: u32,
    mp3_rate: u32,
    mp3_bitrate: u32,
}

impl Instance {
    fn new(codec: u32, flags: u64, factory: fn() -> Box<dyn At9Decoder>) -> Self {
        Instance {
            codec,
            format: ((flags >> 7) & 7) as u32,
            gapless_loop: (flags >> 10) & 1 != 0,
            codec_flags: (flags >> 32) as u32,
            factory,
            at9: None,
            config: [0; 4],
            initialized: false,
            info: CodecInfo::default(),
            superframe_remain: 0,
            frames: 0,
            gapless_init: Gapless::default(),
            gapless: Gapless::default(),
            total_samples: 0,
            mp3: None,
            mp3_header: 0,
            mp3_channels: 0,
            mp3_rate: 0,
            mp3_bitrate: 0,
        }
    }

    fn is_mp3(&self) -> bool {
        self.codec == 0
    }

    fn non_interleaved(&self) -> bool {
        (self.codec_flags >> 8) & 1 != 0
    }

    fn at9_reset(&mut self) {
        let mut decoder = (self.factory)();
        let info = decoder.init(self.config);
        self.initialized = info.is_some();
        if let Some(info) = info {
            self.info = info;
        }
        self.at9 = Some(decoder);
        self.frames = 0;
        self.superframe_remain = self.info.superframe_size as u32;
        if trace() {
            note(&format!(
                "Audio trace: ATRAC9 init {}: {} ch, {} Hz, frame {} samples, {} frames/superframe, superframe {} bytes, format {}, flags {:#x}",
                if self.initialized { "ok" } else { "FAILED" },
                self.info.channels,
                self.info.sampling_rate,
                self.info.frame_samples,
                self.info.frames_in_superframe,
                self.info.superframe_size,
                self.format,
                self.codec_flags
            ));
        }
    }

    fn gapless_reset(&mut self) {
        self.gapless.total_samples = self.gapless_init.total_samples;
        self.gapless.skip_samples = self.gapless_init.skip_samples;
        self.gapless.skipped_samples = 0;
    }

    fn gapless_end(&self) -> bool {
        self.gapless_init.total_samples != 0 && self.gapless.total_samples == 0
    }

    /// Bytes of PCM the next frame of `frame` samples per channel writes after gapless trimming.
    fn next_bytes(&self, frame: u32, channels: u32) -> u32 {
        let skip = u32::from(self.gapless.skip_samples).min(frame);
        let mut samples = frame - skip;
        if self.gapless_init.total_samples != 0 && self.gapless.total_samples < samples {
            samples = self.gapless.total_samples;
        }
        samples
            .wrapping_mul(channels)
            .wrapping_mul(pcm_size(self.format))
    }

    fn next_frame_bytes(&self) -> u32 {
        self.next_bytes(self.info.frame_samples as u32, self.info.channels as u32)
    }

    /// Skips the gapless head of a decoded frame of `frame` samples and caps the rest at the
    /// stream's end and the room left; returns the samples to skip and to write.
    fn trim(&mut self, frame: u32, channels: u32, out: &Output) -> (u32, u32) {
        let mut skip = 0;
        if self.gapless.skip_samples != 0 {
            skip = u32::from(self.gapless.skip_samples).min(frame);
            self.gapless.skip_samples -= skip as u16;
        }
        let mut samples = frame - skip;
        if self.gapless_init.total_samples != 0 && samples > self.gapless.total_samples {
            samples = self.gapless.total_samples;
        }
        let room = out.room() / (u64::from(channels) * u64::from(pcm_size(self.format)));
        if u64::from(samples) > room {
            samples = room as u32;
        }
        (skip, samples)
    }

    /// Accounts `samples` written out of a frame of `frame` samples.
    fn trimmed(&mut self, frame: u32, samples: u32) {
        self.gapless.skipped_samples = self
            .gapless
            .skipped_samples
            .wrapping_add(frame.wrapping_sub(samples) as u16);
        if self.gapless_init.total_samples != 0 {
            self.gapless.total_samples = self.gapless.total_samples.wrapping_sub(samples);
        }
    }

    /// Skips what is left of the superframe in the input.
    fn skip_superframe_rest(&mut self, input: &mut &[u8]) {
        let skip = (self.superframe_remain as usize).min(input.len());
        *input = &input[skip..];
        self.superframe_remain = self.info.superframe_size as u32;
        self.frames = 0;
    }

    /// One ATRAC9 frame; returns result flags and the samples written.
    fn at9_frame(&mut self, input: &mut &[u8], out: &mut Output, internal: &mut i32) -> (u32, u32) {
        let channels = self.info.channels;
        let frame = self.info.frame_samples;
        let fatal = RESULT_CODEC_ERROR | RESULT_FATAL;
        let mut pcm = [0i16; 256 * 8];
        if channels <= 0 || frame < 0 || frame as i64 * channels as i64 > pcm.len() as i64 {
            return (fatal, 0);
        }
        let Some(decoder) = self.at9.as_mut() else {
            return (fatal, 0);
        };
        let used = match decoder.decode(input, &mut pcm) {
            Ok(used) => used.min(input.len()),
            Err(error) => {
                *internal = error;
                return (fatal, 0);
            }
        };
        *input = &input[used..];
        self.superframe_remain = self.superframe_remain.wrapping_sub(used as u32);
        FRAMES_DECODED.fetch_add(1, Ordering::Relaxed);
        let (channels, frame) = (channels as u32, frame as u32);
        let (skip, samples) = self.trim(frame, channels, out);
        let mut converted = Vec::with_capacity((samples * channels * 4) as usize);
        let planes = if self.non_interleaved() { samples } else { 0 };
        converted.resize((samples * channels * pcm_size(self.format)) as usize, 0);
        let width = pcm_size(self.format) as usize;
        for s in 0..samples {
            for c in 0..channels {
                let v = pcm[((skip + s) * channels + c) as usize];
                let index = if planes != 0 {
                    (c * samples + s) as usize
                } else {
                    (s * channels + c) as usize
                };
                let target = &mut converted[index * width..(index + 1) * width];
                match self.format {
                    FORMAT_S16 => target.copy_from_slice(&v.to_le_bytes()),
                    FORMAT_S32 => target.copy_from_slice(&(i32::from(v) << 16).to_le_bytes()),
                    _ => target.copy_from_slice(&(f32::from(v) / 32768.0).to_le_bytes()),
                }
            }
        }
        out.write(&converted);
        self.trimmed(frame, samples);
        self.frames += 1;
        let per_superframe = self.info.frames_in_superframe as u32;
        if per_superframe != 0 && self.frames.is_multiple_of(per_superframe) {
            self.skip_superframe_rest(input);
        } else if self.gapless_end() {
            // Drain the rest of the superframe so the next stream starts aligned.
            while per_superframe != 0
                && !self.frames.is_multiple_of(per_superframe)
                && !input.is_empty()
            {
                let Some(decoder) = self.at9.as_mut() else {
                    break;
                };
                let Ok(used) = decoder.decode(input, &mut pcm) else {
                    break;
                };
                let used = used.min(input.len());
                *input = &input[used..];
                self.superframe_remain = self.superframe_remain.wrapping_sub(used as u32);
                self.frames += 1;
            }
            self.skip_superframe_rest(input);
        }
        (0, samples)
    }

    /// A RIFF header at the start of the input: the `fmt ` chunk configures the decoder, `fact`
    /// sets the gapless values; ends at the `data` chunk, which is where the input resumes.
    fn parse_riff(&mut self, input: &mut &[u8]) {
        let data = *input;
        let end = data.len();
        let mut p = 12.min(end);
        while p + 8 <= end {
            let (Some(tag), Some(length)) = (u32_at(data, p), u32_at(data, p + 4)) else {
                break;
            };
            p += 8;
            if tag == 0x6174_6164 {
                break; // "data"
            }
            if tag == 0x2074_6d66 && length >= 52 && p + 52 <= end {
                // "fmt "
                self.config.copy_from_slice(&data[p + 44..p + 48]);
                self.at9_reset();
            } else if tag == 0x7463_6166 && length >= 12 && p + 12 <= end {
                // "fact"
                let total = u32_at(data, p).unwrap_or(0);
                let delay = u32_at(data, p + 4).unwrap_or(0);
                self.gapless_init.total_samples = total;
                self.gapless_init.skip_samples = delay as u16;
                self.gapless_reset();
            }
            p = p.saturating_add(length as usize).min(end);
        }
        *input = &data[p..];
    }

    // ---- MP3 ----

    fn mp3_open(&mut self) -> bool {
        if self.mp3.is_some() {
            return true;
        }
        let mut params = CodecParameters::new();
        params.for_codec(CODEC_TYPE_MP3);
        match MpaDecoder::try_new(&params, &DecoderOptions::default()) {
            Ok(decoder) => {
                self.mp3 = Some(decoder);
                true
            }
            Err(_) => false,
        }
    }

    fn mp3_reset(&mut self) {
        if let Some(decoder) = self.mp3.as_mut() {
            decoder.reset();
        }
    }

    /// One MP3 frame of `fr.frame_size` bytes at the start of the input; returns result flags
    /// and the samples written. A damaged frame is skipped, as silence.
    fn mp3_frame(
        &mut self,
        fr: &Mp3Frame,
        input: &mut &[u8],
        out: &mut Output,
        internal: &mut i32,
    ) -> (u32, u32) {
        let size = fr.frame_size as usize;
        let head = &input[..size];
        self.mp3_header = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
        self.mp3_channels = fr.num_channels;
        self.mp3_rate = fr.sample_rate;
        self.mp3_bitrate = fr.bitrate;
        FRAMES_DECODED.fetch_add(1, Ordering::Relaxed);
        let decoded = {
            let Some(decoder) = self.mp3.as_mut() else {
                return (0, 0);
            };
            let packet = Packet::new_from_slice(0, 0, 0, head);
            match decoder.decode(&packet) {
                Ok(buffer) => {
                    let frames = buffer.frames();
                    let spec = *buffer.spec();
                    let mut planar = SampleBuffer::<f32>::new(buffer.capacity() as u64, spec);
                    planar.copy_planar_ref(buffer);
                    Some((planar.samples().to_vec(), frames, spec.channels.count()))
                }
                Err(_) => {
                    *internal = -1;
                    None
                }
            }
        };
        *input = &input[size..];
        let Some((planar, frames, decoded_channels)) = decoded else {
            return (0, 0);
        };
        let channels = if decoded_channels > 0 {
            decoded_channels
        } else {
            fr.num_channels as usize
        };
        if channels == 0 || frames == 0 {
            return (0, 0);
        }
        let frame = frames as u32;
        let (skip, mut samples) = self.trim(frame, channels as u32, out);
        let cap = (1152 * 2 * 4) / (channels * 4);
        if samples as usize > cap {
            samples = cap as u32;
        }
        let width = pcm_size(self.format) as usize;
        let mut converted = vec![0u8; samples as usize * channels * width];
        for s in 0..samples as usize {
            for c in 0..channels {
                let v = planar[c * frames + skip as usize + s].clamp(-1.0, 1.0);
                let index = if self.non_interleaved() {
                    c * samples as usize + s
                } else {
                    s * channels + c
                };
                let target = &mut converted[index * width..(index + 1) * width];
                match self.format {
                    FORMAT_S16 => target.copy_from_slice(&((v * 32767.0) as i16).to_le_bytes()),
                    FORMAT_S32 => {
                        target.copy_from_slice(&((v * 2_147_483_647.0) as i32).to_le_bytes());
                    }
                    _ => target.copy_from_slice(&v.to_le_bytes()),
                }
            }
        }
        out.write(&converted);
        self.trimmed(frame, samples);
        (0, samples)
    }

    // ---- job execution ----

    fn run_job(&mut self, id: u32, job: &Job, side: &mut Side) {
        let f = job.flags;
        let mut flags_result = 0u32;
        let control_bytes = job.input_control.bytes();
        let mut control = control_bytes.unwrap_or(&[]);
        let has_control = control_bytes.is_some();
        let mp3 = self.is_mp3();
        if control_reset(f) {
            self.total_samples = 0;
            self.gapless_reset();
            if mp3 {
                self.mp3_reset();
            } else if self.initialized {
                self.at9_reset();
            }
        }
        if has_control && sideband_format(f) && control.len() >= 24 {
            control = &control[24..]; // output format fixed per instance
        }
        if has_control && sideband_gapless(f) && control.len() >= 8 {
            if let Some(g) = Gapless::parse(control) {
                control = &control[8..];
                flags_result |= self.apply_gapless(g);
            }
        }
        if has_control && control_resample(f) && control.len() >= 8 {
            control = &control[8..];
        }
        if has_control && control_initialize(f) && control.len() >= 8 {
            if mp3 {
                self.mp3_reset();
            } else {
                self.config.copy_from_slice(&control[..4]);
                self.at9_reset();
            }
        }
        let in_size: u64 = job.inputs.iter().map(|c| u64::from(c.size)).sum();
        let joined: Vec<u8>;
        let mut input: &[u8] = &[];
        if let [only] = job.inputs.as_slice() {
            input = only.bytes().unwrap_or(&[]);
        } else if job.inputs.len() > 1 {
            joined = job
                .inputs
                .iter()
                .flat_map(|c| c.bytes().unwrap_or(&[]).iter().copied())
                .collect();
            input = &joined;
        }
        let in_start = input.len() as u64;
        let mut out = Output {
            chunks: &job.outputs,
            current: 0,
            offset: 0,
        };
        let out_start = out.room();
        let mut frames = 0u32;
        let mut internal = 0i32;
        if in_size != 0 && mp3 {
            self.run_mp3(
                f,
                &mut input,
                &mut out,
                &mut frames,
                &mut internal,
                &mut flags_result,
            );
        } else if in_size != 0 {
            self.run_at9(
                f,
                &mut input,
                &mut out,
                &mut frames,
                &mut internal,
                &mut flags_result,
            );
        }
        let consumed = in_start - input.len() as u64;
        let written = out_start - out.room();
        if (flags_result != 0 || TRACE_ALL.load(Ordering::Relaxed)) && trace() {
            note(&format!(
                "Audio trace: Ajm job instance {id} flags {f:#x} -> result {flags_result:#x} internal {internal}; input {consumed}/{in_start} used, output {written}/{out_start} written, frames {frames}"
            ));
        }
        // The result goes at the start of the sideband, which `Side` has already reserved.
        if let Some(bytes) = side.bytes.as_deref_mut() {
            if bytes.len() >= 8 {
                bytes[0..4].copy_from_slice(&flags_result.to_le_bytes());
                bytes[4..8].copy_from_slice(&internal.to_le_bytes());
            }
        }
        // Output sideband order: stream, format, gapless, multi-frame, codec info.
        if sideband_stream(f) && side.has(16) {
            let mut s = [0u8; 16];
            s[0..4].copy_from_slice(&(consumed as i32).to_le_bytes());
            s[4..8].copy_from_slice(&(written as i32).to_le_bytes());
            s[8..16].copy_from_slice(&self.total_samples.to_le_bytes());
            side.put(&s);
        }
        if sideband_format(f) && side.has(24) {
            let (channels, mask, rate, bitrate) = if mp3 {
                (
                    self.mp3_channels,
                    channel_mask(self.mp3_channels as i32),
                    self.mp3_rate,
                    self.mp3_bitrate,
                )
            } else {
                let i = &self.info;
                let mut bitrate = 0u32;
                if i.frames_in_superframe != 0 && i.frame_samples != 0 {
                    bitrate = (i.sampling_rate as u32 as u64 * i.superframe_size as u32 as u64 * 8
                        / (i.frames_in_superframe as u32 as u64 * i.frame_samples as u32 as u64))
                        as u32;
                }
                (
                    i.channels as u32,
                    channel_mask(i.channels),
                    i.sampling_rate as u32,
                    bitrate,
                )
            };
            let mut s = [0u8; 24];
            for (n, value) in [channels, mask, rate, self.format, bitrate, 0]
                .into_iter()
                .enumerate()
            {
                s[n * 4..n * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            side.put(&s);
        }
        if sideband_gapless(f) && side.has(8) {
            side.put(&self.gapless.encode());
        }
        if run_multiple_frames(f) && side.has(8) {
            let mut m = [0u8; 8];
            m[0..4].copy_from_slice(&frames.to_le_bytes());
            side.put(&m);
        }
        if run_codec_info(f) && side.has(16) {
            let mut info = [0u8; 16];
            if mp3 {
                // header, has_crc, channel mode, mode extension, copyright, original, emphasis
                let h = self.mp3_header;
                info[0..4].copy_from_slice(&h.to_le_bytes());
                info[4] = u8::from((h >> 16) & 1 == 0);
                info[5] = ((h >> 6) & 3) as u8;
                info[6] = ((h >> 4) & 3) as u8;
                info[7] = ((h >> 3) & 1) as u8;
                info[8] = ((h >> 2) & 1) as u8;
                info[9] = (h & 3) as u8;
            } else {
                for (n, value) in [
                    self.info.superframe_size as u32,
                    self.info.frames_in_superframe as u32,
                    self.superframe_remain,
                    self.info.frame_samples as u32,
                ]
                .into_iter()
                .enumerate()
                {
                    info[n * 4..n * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
            }
            side.put(&info);
        }
    }

    /// The gapless control sideband: moves the stream's end and head by what the game asks for.
    fn apply_gapless(&mut self, g: Gapless) -> u32 {
        let mut flags = 0;
        let processed = self
            .gapless_init
            .total_samples
            .wrapping_sub(self.gapless.total_samples);
        if g.total_samples != 0 || g.skip_samples == 0 {
            if g.total_samples >= processed {
                let diff = i64::from(self.gapless_init.total_samples) - i64::from(g.total_samples);
                self.gapless_init.total_samples = g.total_samples;
                self.gapless.total_samples = (i64::from(self.gapless.total_samples) - diff) as u32;
            } else {
                flags |= RESULT_INVALID_PARAMETER;
            }
        }
        let skipped = u32::from(self.gapless_init.skip_samples)
            .wrapping_sub(u32::from(self.gapless.skip_samples));
        if g.skip_samples != 0 || g.total_samples == 0 {
            if u32::from(g.skip_samples) >= skipped {
                let diff = i32::from(self.gapless_init.skip_samples) - i32::from(g.skip_samples);
                self.gapless_init.skip_samples = g.skip_samples;
                self.gapless.skip_samples = (i32::from(self.gapless.skip_samples) - diff) as u16;
            } else {
                flags |= RESULT_INVALID_PARAMETER;
            }
        }
        flags
    }

    fn run_mp3(
        &mut self,
        f: u64,
        input: &mut &[u8],
        out: &mut Output,
        frames: &mut u32,
        internal: &mut i32,
        flags_result: &mut u32,
    ) {
        loop {
            // As the ATRAC9 loop: the stream's end, then room for the next whole frame.
            if self.gapless_loop && self.gapless_end() {
                self.gapless_reset();
                self.total_samples = 0;
            }
            if self.gapless_end() && *frames != 0 {
                break;
            }
            if !self.mp3_open() {
                *flags_result |= RESULT_NOT_INITIALIZED;
                break;
            }
            let Ok(fr) = mp3_parse_frame(input, false) else {
                // Not at a frame header: padding or a tag. Skip to the next sync word.
                let mut skip = 1;
                while skip + 1 < input.len()
                    && !(input[skip] == 0xff && (input[skip + 1] & 0xe0) == 0xe0)
                {
                    skip += 1;
                }
                if skip + 1 >= input.len() {
                    *input = &[];
                    if *frames == 0 {
                        *flags_result |= RESULT_PARTIAL_INPUT;
                    }
                    break;
                }
                *input = &input[skip..];
                continue;
            };
            if (input.len() as u64) < fr.frame_size {
                *flags_result |= RESULT_PARTIAL_INPUT;
                break;
            }
            if !self.gapless_end()
                && out.room() < u64::from(self.next_bytes(fr.samples_per_channel, fr.num_channels))
            {
                if *frames == 0 {
                    *flags_result |= RESULT_NOT_ENOUGH_ROOM;
                }
                break;
            }
            let (result, written) = self.mp3_frame(&fr, input, out, internal);
            self.total_samples += u64::from(written);
            *frames += 1;
            if result != 0 {
                *flags_result |= result;
                break;
            }
            if !run_multiple_frames(f) || input.is_empty() {
                break;
            }
        }
    }

    fn run_at9(
        &mut self,
        f: u64,
        input: &mut &[u8],
        out: &mut Output,
        frames: &mut u32,
        internal: &mut i32,
        flags_result: &mut u32,
    ) {
        loop {
            if self.gapless_loop && self.gapless_end() {
                self.gapless_reset();
                self.total_samples = 0;
            }
            // The stream's last sample is out: the job is done, without asking for more input.
            // Uncharted decodes each sound effect to its end in one go and dropped every effect
            // whose last job reported partial input (engine noise, gunfire, menu clicks).
            if self.gapless_end() && *frames != 0 {
                break;
            }
            if self.codec_flags & 1 != 0 && input.len() >= 4 && &input[..4] == b"RIFF" {
                self.parse_riff(input);
                self.total_samples = 0;
            }
            if !self.initialized {
                *flags_result |= RESULT_NOT_INITIALIZED;
                break;
            }
            // A full output after decoded frames ends the job normally (as on the PS4, and in
            // shadPS4's Ajm): the error is only for an output too small for the first frame.
            // Uncharted's sound engine drops voices on it: engine noise and gunfire went silent.
            if !self.gapless_end() && out.room() < u64::from(self.next_frame_bytes()) {
                if *frames != 0 {
                    break;
                }
                *flags_result |= RESULT_NOT_ENOUGH_ROOM;
            }
            if (input.len() as u64) < u64::from(self.superframe_remain) {
                *flags_result |= RESULT_PARTIAL_INPUT;
            }
            if *flags_result != 0 {
                break;
            }
            let (result, written) = self.at9_frame(input, out, internal);
            self.total_samples += u64::from(written);
            *frames += 1;
            if result != 0 {
                *flags_result |= result;
                break;
            }
            if !run_multiple_frames(f) {
                break;
            }
        }
    }
}

// ---- MP3 frame header parsing (tables as in the AJM library) ----

/// `SceAjmDecMp3FrameInfo`-like result of [`mp3_parse_frame`].
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Mp3Frame {
    pub frame_size: u64,
    pub num_channels: u32,
    pub samples_per_channel: u32,
    pub bitrate: u32,
    pub sample_rate: u32,
    pub encoder_delay: u32,
    pub num_frames: u32,
    pub total_samples: u32,
    pub ofl_type: u32,
}

const MP3_RATES: [[u32; 4]; 4] = [
    [11025, 12000, 8000, 0],
    [0, 0, 0, 0],
    [22050, 24000, 16000, 0],
    [44100, 48000, 32000, 0],
];
const MP3_KBPS: [[u32; 16]; 4] = [
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 0, 0, 0, 0, 0, 0, 0],
    [0; 16],
    [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ],
    [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ],
];

fn read_bits(data: &[u8], offset: &mut usize, count: u32) -> u32 {
    let mut value = 0;
    for _ in 0..count {
        value = value << 1 | u32::from((data[*offset / 8] >> (7 - *offset % 8)) & 1);
        *offset += 1;
    }
    value
}

/// CRC-8 (polynomial 0x45, start 0xff) over the first 9 bytes of Sony's "FGH" block.
fn fgh_crc(block: &[u8]) -> u8 {
    let mut crc = 0xffu8;
    for byte in &block[..9] {
        for j in (0..8).rev() {
            crc = if (crc >> 7) & 1 != (byte >> j) & 1 {
                crc.wrapping_mul(2)
            } else {
                crc.wrapping_mul(2) ^ 0x45
            };
        }
    }
    crc
}

/// Parses the MP3 frame header at the start of `data` (`sceAjmDecMp3ParseFrame`). With
/// `parse_ofl` also reads Sony's "FGH" gapless block (encoder delay, total samples) that
/// follows the side information of the first (empty) frame.
pub fn mp3_parse_frame(data: &[u8], parse_ofl: bool) -> Result<Mp3Frame, i32> {
    if data.len() < 4 {
        return Err(ERR_INVALID_PARAMETER);
    }
    let h = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    if (h >> 21) != 0x7ff || ((h >> 17) & 3) != 1 {
        return Err(ERR_INVALID_PARAMETER); // sync word, Layer III
    }
    let version = ((h >> 19) & 3) as usize;
    let protection = (h >> 16) & 1 != 0;
    let bitrate_index = ((h >> 12) & 15) as usize;
    let rate_index = ((h >> 10) & 3) as usize;
    let padding = u64::from((h >> 9) & 1);
    let mono = ((h >> 6) & 3) == 3;
    let sample_rate = MP3_RATES[version][rate_index];
    let bitrate = MP3_KBPS[version][bitrate_index] * 1000;
    if sample_rate == 0 || bitrate == 0 {
        return Err(ERR_INVALID_PARAMETER);
    }
    let mpeg1 = version == 3;
    let mut frame = Mp3Frame {
        frame_size: (if mpeg1 { 144 } else { 72 }) * u64::from(bitrate) / u64::from(sample_rate)
            + padding,
        num_channels: if mono { 1 } else { 2 },
        samples_per_channel: if mpeg1 { 1152 } else { 576 },
        bitrate,
        sample_rate,
        ..Mp3Frame::default()
    };
    if !parse_ofl || frame.frame_size > data.len() as u64 {
        return Ok(frame);
    }
    let end = frame.frame_size as usize;
    let body = &data[4..end.max(4)];
    let mut bit = if protection { 0 } else { 16 };
    bit += if mpeg1 {
        if mono {
            18
        } else {
            20
        }
    } else if mono {
        9
    } else {
        10
    };
    let mut part23 = 0usize;
    for _ in 0..(if mpeg1 { 2 } else { 1 }) {
        for _ in 0..frame.num_channels {
            if (bit + 12) / 8 >= body.len() {
                return Ok(frame);
            }
            part23 += read_bits(body, &mut bit, 12) as usize;
            bit += if mpeg1 { 47 } else { 51 };
        }
    }
    bit += part23;
    let mut p = 4 + bit.div_ceil(8);
    if p + 10 >= end {
        return Ok(frame);
    }
    let tag = &data[p..p + 4];
    if tag == b"Xing" || tag == b"Info" || tag == b"VBRI" {
        return Ok(frame);
    }
    while p + 9 < end && data[p] != 0xB4 {
        p += 1;
    }
    if p + 9 >= end {
        return Ok(frame);
    }
    let block = &data[p..p + 10];
    if block[9] != fgh_crc(block) {
        return Ok(frame);
    }
    frame.encoder_delay = u32::from(block[1]) << 8 | u32::from(block[2]);
    frame.total_samples = u32::from_be_bytes([block[3], block[4], block[5], block[6]]);
    frame.ofl_type = 3; // Fgh
    Ok(frame)
}

// ---- batches ----

#[derive(Default)]
struct Job {
    flags: u64,
    have_flags: bool,
    inputs: Vec<ChunkBuffer>,
    outputs: Vec<ChunkBuffer>,
    input_control: ChunkBuffer,
    output_control: ChunkBuffer,
}

/// The chunks of one job body; `None` if it is malformed.
fn parse_job(body: &[u8]) -> Option<Job> {
    let mut job = Job::default();
    let mut p = 0;
    while p < body.len() {
        let word = u32_at(body, p)?;
        match ident(word) {
            IDENT_CONTROL_FLAGS | IDENT_RUN_FLAGS => {
                let size = u32_at(body, p + 4)?;
                job.flags = (u64::from(payload(word)) << 32) | u64::from(size);
                job.have_flags = true;
                p += 8;
            }
            IDENT_INPUT_RUN => {
                if job.inputs.len() == 16 {
                    return None;
                }
                job.inputs.push(ChunkBuffer::parse(body.get(p..)?)?);
                p += BUFFER_CHUNK;
            }
            IDENT_OUTPUT_RUN => {
                if job.outputs.len() == 16 {
                    return None;
                }
                job.outputs.push(ChunkBuffer::parse(body.get(p..)?)?);
                p += BUFFER_CHUNK;
            }
            IDENT_INPUT_CONTROL => {
                job.input_control = ChunkBuffer::parse(body.get(p..)?)?;
                p += BUFFER_CHUNK;
            }
            IDENT_OUTPUT_CONTROL => {
                job.output_control = ChunkBuffer::parse(body.get(p..)?)?;
                p += BUFFER_CHUNK;
            }
            IDENT_RETURN_ADDRESS => p += BUFFER_CHUNK,
            _ => return None,
        }
    }
    job.have_flags.then_some(job)
}

struct Context {
    registered: [bool; CODECS],
    instances: HashMap<u32, Instance>,
}

#[derive(Clone, Copy)]
struct Batch {
    canceled: bool,
}

/// What a malformed batch reports: the error code and the byte offset of the bad job.
#[derive(Debug, PartialEq, Eq)]
pub struct BatchFailure {
    pub code: i32,
    pub offset: usize,
}

/// All AJM contexts and batches (the C file's statics).
pub struct Ajm {
    contexts: BTreeMap<u32, Box<Context>>,
    batches: [Option<Batch>; MAX_BATCHES],
    factory: fn() -> Box<dyn At9Decoder>,
}

impl Ajm {
    pub const fn new(factory: fn() -> Box<dyn At9Decoder>) -> Self {
        Ajm {
            contexts: BTreeMap::new(),
            batches: [None; MAX_BATCHES],
            factory,
        }
    }

    pub fn initialize(&mut self, reserved: i64) -> Result<u32, i32> {
        if reserved != 0 {
            return Err(ERR_INVALID_PARAMETER);
        }
        for id in 1..=MAX_CONTEXTS {
            if let std::collections::btree_map::Entry::Vacant(slot) = self.contexts.entry(id) {
                slot.insert(Box::new(Context {
                    registered: [false; CODECS],
                    instances: HashMap::new(),
                }));
                note("Runtime: Ajm context initialized (ATRAC9 via LibAtrac9)");
                return Ok(id);
            }
        }
        Err(ERR_OUT_OF_RESOURCES)
    }

    pub fn finalize(&mut self, id: u32) -> Result<(), i32> {
        self.contexts
            .remove(&id)
            .map(|_| ())
            .ok_or(ERR_INVALID_CONTEXT)
    }

    fn context(&mut self, id: u32) -> Result<&mut Context, i32> {
        if id == 0 || id > MAX_CONTEXTS {
            return Err(ERR_INVALID_CONTEXT);
        }
        self.contexts
            .get_mut(&id)
            .map(|c| &mut **c)
            .ok_or(ERR_INVALID_CONTEXT)
    }

    pub fn module_register(&mut self, id: u32, codec: u32, reserved: i64) -> Result<(), i32> {
        if reserved != 0 || codec as usize >= CODECS {
            return Err(ERR_INVALID_PARAMETER);
        }
        let c = self.context(id)?;
        if c.registered[codec as usize] {
            return Err(ERR_ALREADY_REGISTERED);
        }
        c.registered[codec as usize] = true;
        Ok(())
    }

    pub fn module_unregister(&mut self, id: u32, codec: u32) -> Result<(), i32> {
        let c = self.context(id)?;
        if codec as usize >= CODECS {
            return Err(ERR_INVALID_PARAMETER);
        }
        if !c.registered[codec as usize] {
            return Err(ERR_NOT_REGISTERED);
        }
        c.registered[codec as usize] = false;
        Ok(())
    }

    pub fn instance_create(&mut self, id: u32, codec: u32, flags: u64) -> Result<u32, i32> {
        if codec as usize >= CODECS {
            return Err(ERR_INVALID_PARAMETER);
        }
        if flags & 7 == 0 {
            return Err(ERR_WRONG_REVISION);
        }
        if codec > 1 {
            warn(&format!(
                "Ajm codec {codec} is not implemented; returning an error"
            ));
            return Err(ERR_INVALID_PARAMETER);
        }
        if trace() {
            note(&format!(
                "Audio trace: Ajm instance create codec {codec} flags {flags:#x}"
            ));
        }
        let factory = self.factory;
        let c = self.context(id)?;
        if !c.registered[codec as usize] {
            return Err(ERR_NOT_REGISTERED);
        }
        let instance = Instance::new(codec, flags, factory);
        if instance.format > FORMAT_FLOAT {
            return Err(ERR_INVALID_PARAMETER);
        }
        let slot = (1..=MAX_INSTANCES)
            .find(|i| !c.instances.contains_key(i))
            .ok_or(ERR_OUT_OF_RESOURCES)?;
        c.instances.insert(slot, instance);
        Ok(slot | (codec << 14))
    }

    pub fn instance_destroy(&mut self, id: u32, instance: u32) -> Result<(), i32> {
        let c = self.context(id)?;
        c.instances
            .remove(&(instance & MAX_INSTANCES))
            .map(|_| ())
            .ok_or(ERR_INVALID_INSTANCE)
    }

    /// Runs every job of the batch in `buffer` at once; returns the batch id.
    pub fn batch_start(&mut self, id: u32, buffer: &[u8]) -> Result<u32, BatchFailure> {
        let malformed = |offset| BatchFailure {
            code: ERR_MALFORMED_BATCH,
            offset,
        };
        if buffer.len() & 7 != 0 {
            return Err(malformed(0));
        }
        let c = self
            .contexts
            .get_mut(&id)
            .filter(|_| id != 0 && id <= MAX_CONTEXTS);
        let Some(c) = c else {
            return Err(BatchFailure {
                code: ERR_INVALID_CONTEXT,
                offset: 0,
            });
        };
        let Some(slot) = self.batches.iter().position(Option::is_none) else {
            return Err(BatchFailure {
                code: ERR_OUT_OF_RESOURCES,
                offset: 0,
            });
        };
        let mut p = 0;
        while p < buffer.len() {
            let (Some(word), Some(size)) = (u32_at(buffer, p), u32_at(buffer, p + 4)) else {
                return Err(malformed(p));
            };
            let body_start = p + CHUNK;
            let next = body_start + size as usize;
            if ident(word) == IDENT_INLINE {
                p = next;
                continue;
            }
            let job = if ident(word) == IDENT_JOB && next <= buffer.len() {
                parse_job(&buffer[body_start..next])
            } else {
                None
            };
            let Some(job) = job else {
                return Err(malformed(p));
            };
            run_job(c, payload(word), &job);
            p = next;
        }
        self.batches[slot] = Some(Batch { canceled: false });
        BATCHES_RUN.fetch_add(1, Ordering::Relaxed);
        Ok(slot as u32 + 1)
    }

    fn batch(&mut self, id: u32, batch: u32) -> Result<&mut Batch, i32> {
        self.context(id)?;
        if batch == 0 || batch as usize > MAX_BATCHES {
            return Err(ERR_INVALID_BATCH);
        }
        self.batches[batch as usize - 1]
            .as_mut()
            .ok_or(ERR_INVALID_BATCH)
    }

    pub fn batch_wait(&mut self, id: u32, batch: u32) -> Result<(), i32> {
        let canceled = self.batch(id, batch)?.canceled;
        self.batches[batch as usize - 1] = None;
        if canceled {
            Err(ERR_CANCELLED)
        } else {
            Ok(())
        }
    }

    /// Already processed at submission: nothing to cancel.
    pub fn batch_cancel(&mut self, id: u32, batch: u32) -> Result<(), i32> {
        self.batch(id, batch).map(|_| ())
    }

    pub fn memory_register(&mut self, id: u32) -> Result<(), i32> {
        self.context(id).map(|_| ())
    }
}

fn run_job(ctx: &mut Context, id: u32, job: &Job) {
    JOBS_RUN.fetch_add(1, Ordering::Relaxed);
    let mut side = Side {
        bytes: if job.output_control.size != 0 {
            job.output_control.bytes_mut()
        } else {
            None
        },
        at: 0,
    };
    let has_result = side.put(&[0u8; 8]);
    if id == STATISTICS_INSTANCE {
        side.zero_rest();
        return;
    }
    let instance = ctx.instances.get_mut(&(id & MAX_INSTANCES));
    let Some(instance) = instance.filter(|_| id & MAX_INSTANCES != 0) else {
        if has_result {
            let result = RESULT_FATAL | RESULT_INVALID_PARAMETER;
            if let Some(bytes) = side.bytes.as_deref_mut() {
                bytes[0..4].copy_from_slice(&result.to_le_bytes());
            }
        }
        return;
    };
    instance.run_job(id, job, &mut side);
}

// ---- the process-wide state and the guest entry points ----

static STATE: Mutex<Ajm> = Mutex::new(Ajm::new(default_at9));

fn state() -> MutexGuard<'static, Ajm> {
    // Every critical section leaves its state whole, so a poisoned lock is still usable.
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn code(result: Result<(), i32>) -> i32 {
    result.err().unwrap_or(0)
}

/// The guest's names of the exports of this module, resolved through the NID table.
pub const EXPORTS: [&str; 17] = [
    "sceAjmInitialize",
    "sceAjmFinalize",
    "sceAjmModuleRegister",
    "sceAjmModuleUnregister",
    "sceAjmInstanceCreate",
    "sceAjmInstanceDestroy",
    "sceAjmInstanceCodecType",
    "sceAjmBatchJobControlBufferRa",
    "sceAjmBatchJobRunBufferRa",
    "sceAjmBatchJobRunSplitBufferRa",
    "sceAjmBatchJobInlineBuffer",
    "sceAjmBatchStartBuffer",
    "sceAjmBatchWait",
    "sceAjmBatchCancel",
    "sceAjmBatchErrorDump",
    "sceAjmMemoryRegister",
    "sceAjmDecMp3ParseFrame",
];

/// Guest memory writes of the entry points: `value` at `target`, if not null.
fn store<T: Copy>(target: *mut T, value: T) {
    if !target.is_null() {
        // SAFETY: a guest out-parameter, valid for a write of `T` (the guest's contract).
        unsafe { target.write_unaligned(value) };
    }
}

/// Appends a built job at `buffer` and returns the end.
fn emit(buffer: *mut u8, bytes: &[u8]) -> *mut u8 {
    // SAFETY: the guest's batch buffer has room for the job it is building (the SDK contract).
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len());
        buffer.add(bytes.len())
    }
}

fn chunk(address: *const u8, size: u64) -> ChunkBuffer {
    ChunkBuffer {
        word: 0,
        size: size as u32,
        address: address as usize,
    }
}

#[cfg(target_arch = "x86_64")]
mod guest {
    use super::*;

    pub(super) extern "sysv64" fn ajm_initialize(reserved: i64, out: *mut u32) -> i32 {
        if out.is_null() || reserved != 0 {
            return ERR_INVALID_PARAMETER;
        }
        match state().initialize(reserved) {
            Ok(id) => {
                store(out, id);
                0
            }
            Err(e) => e,
        }
    }

    pub(super) extern "sysv64" fn ajm_finalize(id: u32) -> i32 {
        code(state().finalize(id))
    }

    pub(super) extern "sysv64" fn ajm_module_register(id: u32, codec: u32, reserved: i64) -> i32 {
        code(state().module_register(id, codec, reserved))
    }

    pub(super) extern "sysv64" fn ajm_module_unregister(id: u32, codec: u32) -> i32 {
        code(state().module_unregister(id, codec))
    }

    pub(super) extern "sysv64" fn ajm_instance_create(
        id: u32,
        codec: u32,
        flags: u64,
        out: *mut u32,
    ) -> i32 {
        if out.is_null() {
            return ERR_INVALID_PARAMETER;
        }
        match state().instance_create(id, codec, flags) {
            Ok(instance) => {
                store(out, instance);
                0
            }
            Err(e) => e,
        }
    }

    pub(super) extern "sysv64" fn ajm_instance_destroy(id: u32, instance: u32) -> i32 {
        code(state().instance_destroy(id, instance))
    }

    pub(super) extern "sysv64" fn ajm_instance_codec(instance: u32) -> u32 {
        (instance >> 14) & 0x1f
    }

    pub(super) extern "sysv64" fn job_control(
        buffer: *mut u8,
        instance: u32,
        flags: u64,
        input: *const u8,
        input_size: u64,
        output: *const u8,
        output_size: u64,
        return_address: usize,
    ) -> *mut u8 {
        let bytes = build_control(
            instance,
            flags,
            chunk(input, input_size),
            chunk(output, output_size),
            return_address,
        );
        emit(buffer, &bytes)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) extern "sysv64" fn job_run(
        buffer: *mut u8,
        instance: u32,
        flags: u64,
        input: *const u8,
        input_size: u64,
        output: *const u8,
        output_size: u64,
        sideband: *const u8,
        sideband_size: u64,
        return_address: usize,
    ) -> *mut u8 {
        let bytes = build_run(
            instance,
            flags,
            chunk(input, input_size),
            chunk(output, output_size),
            chunk(sideband, sideband_size),
            return_address,
        );
        emit(buffer, &bytes)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) extern "sysv64" fn job_run_split(
        buffer: *mut u8,
        instance: u32,
        flags: u64,
        inputs: *const AjmBuffer,
        input_count: u64,
        outputs: *const AjmBuffer,
        output_count: u64,
        sideband: *const u8,
        sideband_size: u64,
        return_address: usize,
    ) -> *mut u8 {
        let list = |buffers: *const AjmBuffer, count: u64| -> Vec<ChunkBuffer> {
            (0..count as usize)
                .map(|i| {
                    // SAFETY: the guest's array of `count` buffers.
                    let b = unsafe { buffers.add(i).read_unaligned() };
                    ChunkBuffer {
                        word: 0,
                        size: b.size as u32,
                        address: b.address,
                    }
                })
                .collect()
        };
        let bytes = build_run_split(
            instance,
            flags,
            &list(inputs, input_count),
            &list(outputs, output_count),
            chunk(sideband, sideband_size),
            return_address,
        );
        emit(buffer, &bytes)
    }

    pub(super) extern "sysv64" fn job_inline(
        buffer: *mut u8,
        data: *const u8,
        size: u64,
        address: *mut usize,
    ) -> *mut u8 {
        // SAFETY: the guest's `size` bytes of data.
        let data = unsafe { std::slice::from_raw_parts(data, size as usize) };
        let bytes = build_inline(data);
        // SAFETY: as `emit`: the data's offset is inside the chunk just written.
        store(address, unsafe { buffer.add(CHUNK) } as usize);
        emit(buffer, &bytes)
    }

    pub(super) extern "sysv64" fn ajm_batch_start(
        id: u32,
        buffer: *mut u8,
        size: u32,
        priority: i32,
        error: *mut BatchError,
        out: *mut u32,
    ) -> i32 {
        if size & 7 != 0 {
            return ERR_MALFORMED_BATCH;
        }
        let bytes: &[u8] = if buffer.is_null() {
            &[]
        } else {
            // SAFETY: the guest's batch of `size` bytes, built by the job functions.
            unsafe { std::slice::from_raw_parts(buffer, size as usize) }
        };
        match state().batch_start(id, bytes) {
            Ok(batch) => {
                store(out, batch);
                0
            }
            Err(failure) => {
                if failure.code == ERR_MALFORMED_BATCH {
                    store(
                        error,
                        BatchError {
                            error_code: ERR_MALFORMED_BATCH,
                            job_address: buffer as usize + failure.offset,
                            command_offset: failure.offset as u32,
                            job_return_address: 0,
                        },
                    );
                }
                failure.code
            }
        }
    }

    pub(super) extern "sysv64" fn ajm_batch_wait(
        id: u32,
        batch: u32,
        timeout: u32,
        error: *mut BatchError,
    ) -> i32 {
        code(state().batch_wait(id, batch))
    }

    pub(super) extern "sysv64" fn ajm_batch_cancel(id: u32, batch: u32) -> i32 {
        code(state().batch_cancel(id, batch))
    }

    pub(super) extern "sysv64" fn ajm_error_dump() -> i32 {
        0
    }

    pub(super) extern "sysv64" fn ajm_memory_register(id: u32, memory: *mut u8, pages: u64) -> i32 {
        code(state().memory_register(id))
    }

    pub(super) extern "sysv64" fn ajm_mp3_parse_frame(
        data: *const u8,
        size: u32,
        parse_ofl: i32,
        frame: *mut Mp3Frame,
    ) -> i32 {
        if data.is_null() || size < 4 || frame.is_null() {
            return ERR_INVALID_PARAMETER;
        }
        // SAFETY: the guest's `size` bytes at `data`.
        let bytes = unsafe { std::slice::from_raw_parts(data, size as usize) };
        match mp3_parse_frame(bytes, parse_ofl != 0) {
            Ok(parsed) => {
                store(frame, parsed);
                0
            }
            Err(e) => e,
        }
    }
}

/// The guest function behind the libSceAjm symbol `symbol`.
#[cfg(target_arch = "x86_64")]
fn export(symbol: &str) -> Option<usize> {
    use guest::*;
    Some(match symbol {
        "sceAjmInitialize" => ajm_initialize as *const () as usize,
        "sceAjmFinalize" => ajm_finalize as *const () as usize,
        "sceAjmModuleRegister" => ajm_module_register as *const () as usize,
        "sceAjmModuleUnregister" => ajm_module_unregister as *const () as usize,
        "sceAjmInstanceCreate" => ajm_instance_create as *const () as usize,
        "sceAjmInstanceDestroy" => ajm_instance_destroy as *const () as usize,
        "sceAjmInstanceCodecType" => ajm_instance_codec as *const () as usize,
        "sceAjmBatchJobControlBufferRa" => job_control as *const () as usize,
        "sceAjmBatchJobRunBufferRa" => job_run as *const () as usize,
        "sceAjmBatchJobRunSplitBufferRa" => job_run_split as *const () as usize,
        "sceAjmBatchJobInlineBuffer" => job_inline as *const () as usize,
        "sceAjmBatchStartBuffer" => ajm_batch_start as *const () as usize,
        "sceAjmBatchWait" => ajm_batch_wait as *const () as usize,
        "sceAjmBatchCancel" => ajm_batch_cancel as *const () as usize,
        "sceAjmBatchErrorDump" => ajm_error_dump as *const () as usize,
        "sceAjmMemoryRegister" => ajm_memory_register as *const () as usize,
        "sceAjmDecMp3ParseFrame" => ajm_mp3_parse_frame as *const () as usize,
        _ => return None,
    })
}

/// Where the guest import `name` (a scoped NID) resolves to in this module, if anywhere.
#[cfg(target_arch = "x86_64")]
pub fn resolve(name: &CStr) -> Option<usize> {
    name.to_str()
        .ok()
        .and_then(crate::abi::nid_name)
        .and_then(export)
}

/// The address of the libSceAjm function a guest import resolves to, or 0
/// (`uintptr_t runtime_ajm_resolve(const char *name)`; `runtime_resolve` asks it in the C
/// order, after the services).
///
/// # Safety
/// `name` is null or a NUL-terminated string.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn runtime_ajm_resolve(name: *const c_char) -> usize {
    if name.is_null() {
        return 0;
    }
    // SAFETY: a NUL-terminated string (caller).
    resolve(unsafe { CStr::from_ptr(name) }).unwrap_or(0)
}

/// The AJM counters on stdout (`runtime_report`).
#[no_mangle]
pub extern "C" fn runtime_ajm_report() {
    note(&format!(
        "Runtime: Ajm batches={}, jobs={}, ATRAC9 frames decoded={}",
        BATCHES_RUN.load(Ordering::Relaxed),
        JOBS_RUN.load(Ordering::Relaxed),
        FRAMES_DECODED.load(Ordering::Relaxed)
    ));
}

#[cfg(test)]
mod tests;
