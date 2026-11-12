//! libSceAjm: audio decoder batches (piece 3.08; the deleted `src/runtime_ajm.c`). Batches use
//! the SDK chunk format (the builder functions are imports, but games may also build chunks
//! inline) and run synchronously at submission: decoding a batch takes well under a millisecond,
//! so waiting always finds it complete. ATRAC9 is decoded by LibAtrac9 ([`at9`]), MP3 by
//! Symphonia ([`mp3`]; U3's London streets create an MP3 instance). Other codecs (AAC) return an
//! error at instance creation (#56, #132).
//!
//! Guest code calls the `sceAjm*` functions with the System V ABI (`extern "sysv64"`); they reach
//! the guest through [`runtime_ajm_resolve`], which `runtime_resolve` asks after the libc
//! contracts. Every pointer they take is guest memory the game handed over, read and written as
//! the C runtime did, unaligned.

mod at9;
mod mp3;

use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;
use std::sync::{Mutex, MutexGuard};

use crate::abi::host;

const ERR_INVALID_CONTEXT: i32 = 0x8093_0002_u32 as i32;
const ERR_INVALID_INSTANCE: i32 = 0x8093_0003_u32 as i32;
const ERR_INVALID_BATCH: i32 = 0x8093_0004_u32 as i32;
const ERR_INVALID_PARAMETER: i32 = 0x8093_0005_u32 as i32;
const ERR_OUT_OF_RESOURCES: i32 = 0x8093_0007_u32 as i32;
const ERR_ALREADY_REGISTERED: i32 = 0x8093_0009_u32 as i32;
const ERR_NOT_REGISTERED: i32 = 0x8093_000A_u32 as i32;
const ERR_WRONG_REVISION: i32 = 0x8093_000B_u32 as i32;
const ERR_MALFORMED_BATCH: i32 = 0x8093_0011_u32 as i32;
const ERR_CANCELLED: i32 = 0x8093_0017_u32 as i32;
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
const CODECS: u32 = 24;
/// FFmpeg's `AVERROR_INVALIDDATA`, the internal result of a damaged MP3 frame as the C runtime
/// reported it.
const MP3_INVALID_DATA: i32 = -1_094_995_529;

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

/// Job flags: version:3 codec:8 run:2 control:3 reserved:29 sideband:3.
fn flag(flags: u64, bit: u32) -> bool {
    (flags >> bit) & 1 != 0
}
const RUN_CODEC_INFO: u32 = 11;
const RUN_MULTIPLE_FRAMES: u32 = 12;
const CONTROL_RESET: u32 = 13;
const CONTROL_INITIALIZE: u32 = 14;
const CONTROL_RESAMPLE: u32 = 15;
const SIDEBAND_GAPLESS: u32 = 45;
const SIDEBAND_FORMAT: u32 = 46;
const SIDEBAND_STREAM: u32 = 47;

/// A chunk header: `word` is ident:6 payload:20 reserved:6.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Chunk {
    word: u32,
    size: u32,
}

/// A chunk that names a buffer.
#[repr(C)]
#[derive(Clone, Copy)]
struct ChunkBuffer {
    word: u32,
    size: u32,
    address: *mut u8,
}

impl Default for ChunkBuffer {
    fn default() -> Self {
        ChunkBuffer {
            word: 0,
            size: 0,
            address: ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SidebandResult {
    result: i32,
    internal_result: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SidebandStream {
    input_consumed: i32,
    output_written: i32,
    total_decoded_samples: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SidebandFormat {
    channels: u32,
    channel_mask: u32,
    sample_rate: u32,
    encoding: u32,
    bitrate: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct SidebandGapless {
    total_samples: u32,
    skip_samples: u16,
    skipped_samples: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct At9Info {
    super_frame_size: u32,
    frames_in_super_frame: u32,
    next_frame_size: u32,
    frame_samples: u32,
}

/// `SceAjmBatchError`.
#[repr(C)]
#[derive(Debug)]
struct BatchError {
    error_code: c_int,
    job_address: *const c_void,
    command_offset: u32,
    job_return_address: *const c_void,
}

/// `SceAjmBuffer`, an element of a split job's input or output list.
#[repr(C)]
struct AjmBuffer {
    address: *mut c_void,
    size: u64,
}

const _: () = {
    assert!(std::mem::size_of::<Chunk>() == 8);
    assert!(std::mem::size_of::<ChunkBuffer>() == 16);
    assert!(std::mem::size_of::<SidebandResult>() == 8);
    assert!(std::mem::size_of::<SidebandStream>() == 16);
    assert!(std::mem::size_of::<SidebandFormat>() == 24);
    assert!(std::mem::size_of::<SidebandGapless>() == 8);
    assert!(std::mem::size_of::<At9Info>() == 16);
    assert!(std::mem::size_of::<BatchError>() == 32);
    assert!(std::mem::size_of::<AjmBuffer>() == 16);
};

fn ident(word: u32) -> u32 {
    word & 0x3f
}
fn payload(word: u32) -> u32 {
    (word >> 6) & 0xfffff
}
fn header(id: u32, value: u32) -> u32 {
    (id & 0x3f) | ((value & 0xfffff) << 6)
}
fn channel_mask(channels: u32) -> u32 {
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

/// `value` as C's `%#x` prints it: `0x` before anything but zero.
fn hex(value: u64) -> String {
    if value == 0 {
        "0".to_owned()
    } else {
        format!("{value:#x}")
    }
}

/// `*address`, unaligned.
///
/// # Safety
/// `address` points to `size_of::<T>()` readable bytes.
unsafe fn read<T: Copy>(address: *const u8) -> T {
    // SAFETY: the caller's contract.
    unsafe { ptr::read_unaligned(address.cast::<T>()) }
}

/// `*address = value`, unaligned.
///
/// # Safety
/// `address` points to `size_of::<T>()` writable bytes.
unsafe fn write<T>(address: *mut u8, value: T) {
    // SAFETY: the caller's contract.
    unsafe { ptr::write_unaligned(address.cast::<T>(), value) }
}

// ---- trace ----

/// `BB_AUDIO_TRACE=1`: instance configurations and jobs that return a non-zero result (400
/// lines); `BB_AUDIO_TRACE=2`: every job (200000 lines).
struct Trace {
    left: u32,
    all: bool,
}

static TRACE: Mutex<Option<Trace>> = Mutex::new(None);

fn trace_state() -> MutexGuard<'static, Option<Trace>> {
    let mut trace = TRACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if trace.is_none() {
        let value = std::env::var("BB_AUDIO_TRACE").unwrap_or_default();
        let all = value.starts_with('2');
        let left = if all {
            200_000
        } else if value.starts_with('1') {
            400
        } else {
            0
        };
        *trace = Some(Trace { left, all });
    }
    trace
}

/// True while trace lines are left (and uses one up).
fn trace() -> bool {
    let mut guard = trace_state();
    let trace = guard.as_mut().expect("initialized");
    if trace.left == 0 {
        return false;
    }
    trace.left -= 1;
    true
}

fn trace_all() -> bool {
    trace_state().as_ref().is_some_and(|trace| trace.all)
}

// ---- state ----

struct Instance {
    codec: u32,
    format: u32,
    gapless_loop: bool,
    codec_flags: u32,
    /// ATRAC9: the decoder (made at the first initialization), its configuration bytes, and the
    /// stream description it reported.
    handle: Option<at9::Handle>,
    config: [u8; 4],
    initialized: bool,
    info: at9::CodecInfo,
    superframe_remain: u32,
    frames: u32,
    gapless_init: SidebandGapless,
    gapless: SidebandGapless,
    total_samples: u64,
    /// MP3: the decoder (made at the first frame), and the last frame header (format and
    /// codec-info sidebands).
    mp3: Option<mp3::Decoder>,
    mp3_header: u32,
    mp3_channels: u32,
    mp3_rate: u32,
    mp3_bitrate: u32,
}

impl Instance {
    fn new(codec: u32, flags: u64) -> Self {
        Instance {
            codec,
            format: ((flags >> 7) & 7) as u32,
            gapless_loop: (flags >> 10) & 1 != 0,
            codec_flags: (flags >> 32) as u32,
            handle: None,
            config: [0; 4],
            initialized: false,
            info: at9::CodecInfo::default(),
            superframe_remain: 0,
            frames: 0,
            gapless_init: SidebandGapless::default(),
            gapless: SidebandGapless::default(),
            total_samples: 0,
            mp3: None,
            mp3_header: 0,
            mp3_channels: 0,
            mp3_rate: 0,
            mp3_bitrate: 0,
        }
    }
}

struct Context {
    registered: [bool; CODECS as usize],
    /// Instance IDs 1 to [`MAX_INSTANCES`]; slot 0 is never used.
    instances: Vec<Option<Box<Instance>>>,
}

impl Context {
    fn new() -> Self {
        Context {
            registered: [false; CODECS as usize],
            instances: (0..=MAX_INSTANCES).map(|_| None).collect(),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Batch {
    used: bool,
    canceled: bool,
}

struct State {
    /// Context IDs 1 to [`MAX_CONTEXTS`]; slot 0 is never used.
    contexts: [Option<Box<Context>>; MAX_CONTEXTS as usize + 1],
    batches: [Batch; MAX_BATCHES],
    jobs_run: usize,
    frames_decoded: usize,
    batches_run: usize,
}

static STATE: Mutex<State> = Mutex::new(State {
    contexts: [const { None }; MAX_CONTEXTS as usize + 1],
    batches: [Batch {
        used: false,
        canceled: false,
    }; MAX_BATCHES],
    jobs_run: 0,
    frames_decoded: 0,
    batches_run: 0,
});

/// The module's state. A guest thread that panicked here left whole values behind (every
/// update is a plain store), so a poisoned lock is still usable.
fn state() -> MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl State {
    fn context(&mut self, id: u32) -> Option<&mut Context> {
        if id == 0 || id > MAX_CONTEXTS {
            return None;
        }
        self.contexts[id as usize].as_deref_mut()
    }
}

// ---- batch builders ----

/// # Safety
/// `p` points to 16 writable bytes.
unsafe fn put_buffer(p: *mut u8, id: u32, address: *const c_void, size: u64) -> *mut u8 {
    let chunk = ChunkBuffer {
        word: header(id, 0),
        size: size as u32,
        address: address as *mut u8,
    };
    // SAFETY: the caller's contract.
    unsafe { write(p, chunk) };
    p.wrapping_add(16)
}

/// # Safety
/// `p` points to 8 writable bytes.
unsafe fn put_flags(p: *mut u8, id: u32, flags: u64) -> *mut u8 {
    let chunk = Chunk {
        word: header(id, (flags >> 32) as u32),
        size: flags as u32,
    };
    // SAFETY: the caller's contract.
    unsafe { write(p, chunk) };
    p.wrapping_add(8)
}

/// # Safety
/// `start` points to 8 writable bytes; `end` is in the same buffer.
unsafe fn finish_job(start: *mut u8, end: *mut u8, instance: u32) -> *mut c_void {
    let job = Chunk {
        word: header(IDENT_JOB, instance),
        size: (end as usize).wrapping_sub(start as usize).wrapping_sub(8) as u32,
    };
    // SAFETY: the caller's contract.
    unsafe { write(start, job) };
    end.cast()
}

/// `sceAjmBatchJobControlBufferRa`.
#[allow(clippy::too_many_arguments)]
unsafe extern "sysv64" fn job_control(
    buffer: *mut c_void,
    instance: u32,
    flags: u64,
    input: *mut c_void,
    input_size: u64,
    output: *mut c_void,
    output_size: u64,
    return_address: *mut c_void,
) -> *mut c_void {
    let start = buffer.cast::<u8>();
    let flags = flags
        & if instance == STATISTICS_INSTANCE {
            0xC001_8007
        } else {
            0x6000_0000_E7FF
        };
    // SAFETY: the game's batch buffer has room for the job (the SDK contract).
    unsafe {
        let mut p = start.wrapping_add(8);
        if !return_address.is_null() {
            p = put_buffer(p, IDENT_RETURN_ADDRESS, return_address, 0);
        }
        p = put_buffer(p, IDENT_INPUT_CONTROL, input, input_size);
        p = put_flags(p, IDENT_CONTROL_FLAGS, flags);
        p = put_buffer(p, IDENT_OUTPUT_CONTROL, output, output_size);
        finish_job(start, p, instance)
    }
}

/// `sceAjmBatchJobRunBufferRa`.
#[allow(clippy::too_many_arguments)]
unsafe extern "sysv64" fn job_run(
    buffer: *mut c_void,
    instance: u32,
    flags: u64,
    input: *mut c_void,
    input_size: u64,
    output: *mut c_void,
    output_size: u64,
    sideband: *mut c_void,
    sideband_size: u64,
    return_address: *mut c_void,
) -> *mut c_void {
    let start = buffer.cast::<u8>();
    // SAFETY: the game's batch buffer has room for the job (the SDK contract).
    unsafe {
        let mut p = start.wrapping_add(8);
        if !return_address.is_null() {
            p = put_buffer(p, IDENT_RETURN_ADDRESS, return_address, 0);
        }
        p = put_buffer(p, IDENT_INPUT_RUN, input, input_size);
        p = put_flags(p, IDENT_RUN_FLAGS, flags & 0xE000_0000_1FFF);
        p = put_buffer(p, IDENT_OUTPUT_RUN, output, output_size);
        p = put_buffer(p, IDENT_OUTPUT_CONTROL, sideband, sideband_size);
        finish_job(start, p, instance)
    }
}

/// `sceAjmBatchJobRunSplitBufferRa`.
#[allow(clippy::too_many_arguments)]
unsafe extern "sysv64" fn job_run_split(
    buffer: *mut c_void,
    instance: u32,
    flags: u64,
    inputs: *const AjmBuffer,
    input_count: u64,
    outputs: *const AjmBuffer,
    output_count: u64,
    sideband: *mut c_void,
    sideband_size: u64,
    return_address: *mut c_void,
) -> *mut c_void {
    let start = buffer.cast::<u8>();
    // SAFETY: the game's batch buffer has room for the job, and its lists hold the counts given
    // (the SDK contract).
    unsafe {
        let mut p = start.wrapping_add(8);
        if !return_address.is_null() {
            p = put_buffer(p, IDENT_RETURN_ADDRESS, return_address, 0);
        }
        for i in 0..input_count as usize {
            let input = &*inputs.wrapping_add(i);
            p = put_buffer(p, IDENT_INPUT_RUN, input.address, input.size);
        }
        p = put_flags(p, IDENT_RUN_FLAGS, flags & 0xE000_0000_1FFF);
        for i in 0..output_count as usize {
            let output = &*outputs.wrapping_add(i);
            p = put_buffer(p, IDENT_OUTPUT_RUN, output.address, output.size);
        }
        p = put_buffer(p, IDENT_OUTPUT_CONTROL, sideband, sideband_size);
        finish_job(start, p, instance)
    }
}

/// `sceAjmBatchJobInlineBuffer`: `size` bytes of `data` in the batch, padded to 8 bytes;
/// `*address` (if given) is where they landed.
unsafe extern "sysv64" fn job_inline(
    buffer: *mut c_void,
    data: *const c_void,
    size: u64,
    address: *mut *const c_void,
) -> *mut c_void {
    let start = buffer.cast::<u8>();
    let padded = ((size + 7) & !7) as u32;
    let chunk = Chunk {
        word: header(IDENT_INLINE, 0),
        size: padded,
    };
    // SAFETY: the game's batch buffer has room for the chunk and its data (the SDK contract).
    unsafe {
        write(start, chunk);
        let p = start.wrapping_add(8);
        ptr::copy(data.cast::<u8>(), p, size as usize);
        if !address.is_null() {
            *address = p.cast();
        }
        p.wrapping_add(padded as usize).cast()
    }
}

// ---- output ----

/// The output buffers of a run job, filled in order.
struct Output {
    chunks: Vec<ChunkBuffer>,
    current: usize,
    offset: u32,
}

impl Output {
    fn room(&self) -> u64 {
        self.chunks
            .iter()
            .enumerate()
            .skip(self.current)
            .map(|(i, chunk)| {
                u64::from(chunk.size) - u64::from(if i == self.current { self.offset } else { 0 })
            })
            .sum()
    }

    fn write(&mut self, mut data: &[u8]) {
        while !data.is_empty() && self.current < self.chunks.len() {
            let chunk = self.chunks[self.current];
            let n = ((chunk.size - self.offset) as usize).min(data.len());
            if !chunk.address.is_null() {
                // SAFETY: the guest's output buffer holds `size` bytes; `offset + n <= size`.
                unsafe {
                    ptr::copy_nonoverlapping(
                        data.as_ptr(),
                        chunk.address.wrapping_add(self.offset as usize),
                        n,
                    )
                };
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

// ---- ATRAC9 ----

fn at9_reset(instance: &mut Instance) {
    let mut handle = at9::Handle::new();
    instance.initialized = handle.init(instance.config);
    if instance.initialized {
        instance.info = handle.info();
    }
    instance.handle = Some(handle);
    instance.frames = 0;
    instance.superframe_remain = instance.info.superframe_size as u32;
    if trace() {
        let info = &instance.info;
        host::out_line(&format!(
            "Audio trace: ATRAC9 init {}: {} ch, {} Hz, frame {} samples, {} frames/superframe, superframe {} bytes, format {}, flags {}",
            if instance.initialized { "ok" } else { "FAILED" },
            info.channels,
            info.sampling_rate,
            info.frame_samples,
            info.frames_in_superframe,
            info.superframe_size,
            instance.format,
            hex(u64::from(instance.codec_flags))
        ));
    }
}

fn gapless_reset(instance: &mut Instance) {
    instance.gapless = SidebandGapless {
        skipped_samples: 0,
        ..instance.gapless_init
    };
}

fn gapless_end(instance: &Instance) -> bool {
    instance.gapless_init.total_samples != 0 && instance.gapless.total_samples == 0
}

/// Bytes of PCM the next frame of `frame` samples and `channels` channels writes after gapless
/// trimming.
fn next_frame_bytes(instance: &Instance, frame: u32, channels: u32) -> u32 {
    let skip = u32::from(instance.gapless.skip_samples).min(frame);
    let mut samples = frame - skip;
    if instance.gapless_init.total_samples != 0 && instance.gapless.total_samples < samples {
        samples = instance.gapless.total_samples;
    }
    samples
        .wrapping_mul(channels)
        .wrapping_mul(pcm_size(instance.format))
}

/// Trims a decoded frame of `frame` samples by the gapless skip and total and by the room left;
/// returns the samples skipped at its start and the samples to write.
fn trim(instance: &mut Instance, frame: u32, channels: u32, out: &Output) -> (u32, u32) {
    let mut skip = 0;
    if instance.gapless.skip_samples != 0 {
        skip = u32::from(instance.gapless.skip_samples).min(frame);
        instance.gapless.skip_samples -= skip as u16;
    }
    let mut samples = frame - skip;
    if instance.gapless_init.total_samples != 0 && samples > instance.gapless.total_samples {
        samples = instance.gapless.total_samples;
    }
    let room = out.room() / u64::from(channels * pcm_size(instance.format));
    if u64::from(samples) > room {
        samples = room as u32;
    }
    (skip, samples)
}

/// Counts written samples against the gapless state.
fn account(instance: &mut Instance, frame: u32, samples: u32) {
    instance.gapless.skipped_samples = instance
        .gapless
        .skipped_samples
        .wrapping_add(frame.wrapping_sub(samples) as u16);
    if instance.gapless_init.total_samples != 0 {
        instance.gapless.total_samples -= samples;
    }
}

/// Converts `samples` samples of `channels` channels, starting at sample `skip`, into the
/// instance's encoding (and de-interleaves them for a non-interleaved instance).
fn convert(
    instance: &Instance,
    channels: u32,
    skip: u32,
    samples: u32,
    sample: impl Fn(usize, usize) -> Sample,
) -> Vec<u8> {
    let size = pcm_size(instance.format) as usize;
    let mut converted = vec![0u8; samples as usize * channels as usize * size];
    let non_interleaved = (instance.codec_flags >> 8) & 1 != 0;
    for s in 0..samples as usize {
        for c in 0..channels as usize {
            let index = if non_interleaved {
                c * samples as usize + s
            } else {
                s * channels as usize + c
            };
            let bytes = &mut converted[index * size..(index + 1) * size];
            match (sample(skip as usize + s, c), instance.format) {
                (Sample::I16(v), FORMAT_S16) => bytes.copy_from_slice(&v.to_ne_bytes()),
                (Sample::I16(v), FORMAT_S32) => {
                    bytes.copy_from_slice(&(i32::from(v) << 16).to_ne_bytes())
                }
                (Sample::I16(v), _) => {
                    bytes.copy_from_slice(&(f32::from(v) / 32768.0).to_ne_bytes())
                }
                (Sample::F32(v), format) => {
                    let v = v.clamp(-1.0, 1.0);
                    match format {
                        FORMAT_S16 => bytes.copy_from_slice(&((v * 32767.0) as i16).to_ne_bytes()),
                        FORMAT_S32 => {
                            bytes.copy_from_slice(&((v * 2_147_483_647.0) as i32).to_ne_bytes())
                        }
                        _ => bytes.copy_from_slice(&v.to_ne_bytes()),
                    }
                }
            }
        }
    }
    converted
}

enum Sample {
    I16(i16),
    F32(f32),
}

/// The undecoded input of a run job: guest memory, or the joined copy of a split job's inputs.
struct Input {
    at: *const u8,
    size: u64,
}

impl Input {
    fn advance(&mut self, bytes: u64) {
        self.at = self.at.wrapping_add(bytes as usize);
        self.size = self.size.wrapping_sub(bytes);
    }

    /// The input as a slice of at most `limit` bytes.
    fn bytes(&self, limit: u64) -> &[u8] {
        let len = self.size.min(limit) as usize;
        if self.at.is_null() || len == 0 {
            return &[];
        }
        // SAFETY: the job's input holds `size` readable bytes (the guest's buffer, or the joined
        // copy that outlives this `Input`).
        unsafe { std::slice::from_raw_parts(self.at, len) }
    }
}

/// One ATRAC9 frame; returns result flags.
fn at9_frame(
    frames_decoded: &mut usize,
    instance: &mut Instance,
    input: &mut Input,
    out: &mut Output,
    samples_written: &mut u32,
    internal: &mut i32,
) -> u32 {
    let channels = instance.info.channels as u32;
    let frame = instance.info.frame_samples as u32;
    if frame as usize * channels as usize > at9::MAX_FRAME_PCM {
        return RESULT_CODEC_ERROR | RESULT_FATAL;
    }
    let Some(mut handle) = instance.handle.take() else {
        return RESULT_CODEC_ERROR | RESULT_FATAL;
    };
    let result = at9_decode(
        frames_decoded,
        &mut handle,
        instance,
        input,
        out,
        samples_written,
        internal,
    );
    instance.handle = Some(handle);
    result
}

fn at9_decode(
    frames_decoded: &mut usize,
    handle: &mut at9::Handle,
    instance: &mut Instance,
    input: &mut Input,
    out: &mut Output,
    samples_written: &mut u32,
    internal: &mut i32,
) -> u32 {
    let channels = instance.info.channels as u32;
    let frame = instance.info.frame_samples as u32;
    let mut pcm = [0i16; at9::MAX_FRAME_PCM];
    // SAFETY: the input holds the rest of the superframe (the caller checked `superframe_remain`).
    let (status, used) = unsafe { handle.decode(input.at, &mut pcm) };
    if status != 0 {
        *internal = status;
        return RESULT_CODEC_ERROR | RESULT_FATAL;
    }
    input.advance(used as u64);
    instance.superframe_remain = instance.superframe_remain.wrapping_sub(used as u32);
    *frames_decoded += 1;
    let (skip, samples) = trim(instance, frame, channels, out);
    let converted = convert(instance, channels, skip, samples, |s, c| {
        Sample::I16(pcm[s * channels as usize + c])
    });
    out.write(&converted);
    *samples_written = samples;
    account(instance, frame, samples);
    // LibAtrac9 reports at least one frame per superframe for any stream it accepts.
    let per_superframe = (instance.info.frames_in_superframe as u32).max(1);
    instance.frames += 1;
    if instance.frames.is_multiple_of(per_superframe) {
        end_superframe(instance, input);
    } else if gapless_end(instance) {
        // Drain the rest of the superframe so the next stream starts aligned.
        while !instance.frames.is_multiple_of(per_superframe) && input.size != 0 {
            // SAFETY: as above; LibAtrac9 reads one frame of the superframe's rest.
            let (status, used) = unsafe { handle.decode(input.at, &mut pcm) };
            if status != 0 {
                break;
            }
            input.advance(used as u64);
            instance.superframe_remain = instance.superframe_remain.wrapping_sub(used as u32);
            instance.frames += 1;
        }
        end_superframe(instance, input);
    }
    0
}

/// Skips the superframe's padding and starts the next one.
fn end_superframe(instance: &mut Instance, input: &mut Input) {
    let skip = u64::from(instance.superframe_remain).min(input.size);
    input.advance(skip);
    instance.superframe_remain = instance.info.superframe_size as u32;
    instance.frames = 0;
}

/// A RIFF (`.at9`) header at the input: its `fmt ` chunk configures the decoder, its `fact`
/// chunk sets the gapless total and skip; the input moves to the `data` chunk's contents.
fn parse_riff(instance: &mut Instance, input: &mut Input) {
    let bytes = input.bytes(u64::MAX);
    let end = bytes.len();
    // The C runtime wrapped its remaining size when a chunk ran past the input; this stops at
    // the input's end instead.
    let mut p = 12;
    while p + 8 <= end {
        let tag = u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap());
        let length = u32::from_le_bytes(bytes[p + 4..p + 8].try_into().unwrap()) as usize;
        p += 8;
        if tag == 0x6174_6164 {
            break; // "data"
        }
        if tag == 0x2074_6d66 && length >= 52 && p + 52 <= end {
            // "fmt "
            instance.config.copy_from_slice(&bytes[p + 44..p + 48]);
            at9_reset(instance);
        } else if tag == 0x7463_6166 && length >= 12 && p + 12 <= end {
            // "fact"
            let total = u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap());
            let delay = u32::from_le_bytes(bytes[p + 4..p + 8].try_into().unwrap());
            instance.gapless_init.total_samples = total;
            instance.gapless_init.skip_samples = delay as u16;
            gapless_reset(instance);
        }
        p = p.saturating_add(length);
    }
    input.advance(p.min(end) as u64);
}

// ---- MP3 ----

/// One MP3 frame of `header.frame_size` bytes at the input; returns result flags (always 0: a
/// damaged frame is skipped, as silence, with the decoder's error as the internal result).
fn mp3_frame(
    frames_decoded: &mut usize,
    instance: &mut Instance,
    header: &mp3::Mp3Frame,
    input: &mut Input,
    out: &mut Output,
    samples_written: &mut u32,
    internal: &mut i32,
) -> u32 {
    *samples_written = 0;
    let packet = input.bytes(header.frame_size).to_vec();
    instance.mp3_header = u32::from_be_bytes([packet[0], packet[1], packet[2], packet[3]]);
    input.advance(header.frame_size);
    instance.mp3_channels = header.num_channels;
    instance.mp3_rate = header.sample_rate;
    instance.mp3_bitrate = header.bitrate;
    *frames_decoded += 1;
    let mut decoder = instance.mp3.take().unwrap_or_else(mp3::Decoder::new);
    match decoder.decode(&packet, header) {
        None => *internal = MP3_INVALID_DATA,
        Some(buffer) => {
            let channels = mp3::channels(buffer) as u32;
            let frame = mp3::frames(buffer) as u32;
            let (skip, mut samples) = trim(instance, frame, channels, out);
            // The C runtime converted into a buffer of one stereo frame of floats.
            let cap = (1152 * 2 * 4) / (channels as usize * 4);
            if samples as usize > cap {
                samples = cap as u32;
            }
            let converted = convert(instance, channels, skip, samples, |s, c| {
                Sample::F32(mp3::sample(buffer, c, s))
            });
            out.write(&converted);
            *samples_written += samples;
            account(instance, frame, samples);
        }
    }
    instance.mp3 = Some(decoder);
    0
}

// ---- job execution ----

#[derive(Default)]
struct Job {
    flags: u64,
    have_flags: bool,
    inputs: Vec<ChunkBuffer>,
    outputs: Vec<ChunkBuffer>,
    input_control: ChunkBuffer,
    output_control: ChunkBuffer,
}

/// The chunks of one job, `size` bytes at `p`; `None` for a malformed job.
///
/// # Safety
/// `p` points to the job's chunks (guest memory, as the C runtime read it).
unsafe fn parse_job(mut p: *const u8, size: u32) -> Option<Job> {
    let mut job = Job::default();
    let end = p.wrapping_add(size as usize);
    while p < end {
        // SAFETY: the job's chunks (the caller's contract).
        let word: u32 = unsafe { read(p) };
        match ident(word) {
            IDENT_CONTROL_FLAGS | IDENT_RUN_FLAGS => {
                // SAFETY: as above.
                let chunk: Chunk = unsafe { read(p) };
                job.flags = (u64::from(payload(chunk.word)) << 32) | u64::from(chunk.size);
                job.have_flags = true;
                p = p.wrapping_add(8);
            }
            id @ (IDENT_INPUT_RUN | IDENT_OUTPUT_RUN | IDENT_INPUT_CONTROL
            | IDENT_OUTPUT_CONTROL | IDENT_RETURN_ADDRESS) => {
                // SAFETY: as above.
                let chunk: ChunkBuffer = unsafe { read(p) };
                match id {
                    IDENT_INPUT_RUN if job.inputs.len() == 16 => return None,
                    IDENT_INPUT_RUN => job.inputs.push(chunk),
                    IDENT_OUTPUT_RUN if job.outputs.len() == 16 => return None,
                    IDENT_OUTPUT_RUN => job.outputs.push(chunk),
                    IDENT_INPUT_CONTROL => job.input_control = chunk,
                    IDENT_OUTPUT_CONTROL => job.output_control = chunk,
                    _ => {}
                }
                p = p.wrapping_add(16);
            }
            _ => return None,
        }
    }
    job.have_flags.then_some(job)
}

/// The sideband output of a job: a cursor over the guest's output-control buffer.
struct Sideband {
    at: *mut u8,
    end: *mut u8,
}

impl Sideband {
    /// Writes `value` if `size_of::<T>()` bytes are left.
    fn put<T>(&mut self, value: T) -> bool {
        let size = std::mem::size_of::<T>();
        if self.at.is_null() || (self.end as usize).wrapping_sub(self.at as usize) < size {
            return false;
        }
        // SAFETY: the guest's output-control buffer has `size` bytes left at `at`.
        unsafe { write(self.at, value) };
        self.at = self.at.wrapping_add(size);
        true
    }
}

fn run_job(state: &mut State, context: u32, id: u32, job: &Job) {
    state.jobs_run += 1;
    let (mut side, result_at) = {
        let at = if job.output_control.size != 0 {
            job.output_control.address
        } else {
            ptr::null_mut()
        };
        let mut side = Sideband {
            at,
            end: at.wrapping_add(job.output_control.size as usize),
        };
        let result_at = side.at;
        let has_result = side.put(SidebandResult::default());
        (side, has_result.then_some(result_at))
    };
    let set_result = |result: u32, internal: i32| {
        if let Some(at) = result_at {
            let value = SidebandResult {
                result: result as i32,
                internal_result: internal,
            };
            // SAFETY: the sideband had room for the result (checked by `put`).
            unsafe { write(at, value) };
        }
    };
    if id == STATISTICS_INSTANCE {
        if !side.at.is_null() {
            let rest = (side.end as usize).wrapping_sub(side.at as usize);
            // SAFETY: the rest of the guest's output-control buffer.
            unsafe { ptr::write_bytes(side.at, 0, rest) };
        }
        return;
    }
    let State {
        contexts,
        frames_decoded,
        ..
    } = state;
    let ctx = contexts[context as usize]
        .as_deref_mut()
        .expect("a valid context");
    let slot = (id & MAX_INSTANCES) as usize;
    let Some(instance) = ctx.instances[slot].as_deref_mut().filter(|_| slot != 0) else {
        set_result(RESULT_FATAL | RESULT_INVALID_PARAMETER, 0);
        return;
    };
    let f = job.flags;
    let mut flags_result = 0u32;
    let mp3 = instance.codec == 0;

    if flag(f, CONTROL_RESET) {
        instance.total_samples = 0;
        gapless_reset(instance);
        if mp3 {
            if let Some(decoder) = instance.mp3.as_mut() {
                decoder.reset();
            }
        } else if instance.initialized {
            at9_reset(instance);
        }
    }
    let mut control = if job.input_control.size != 0 {
        job.input_control.address as *const u8
    } else {
        ptr::null()
    };
    let control_end = control.wrapping_add(job.input_control.size as usize);
    let has = |control: *const u8, bytes: usize| {
        !control.is_null() && (control_end as usize).wrapping_sub(control as usize) >= bytes
    };
    if flag(f, SIDEBAND_FORMAT) && has(control, 24) {
        control = control.wrapping_add(24); // the output format is fixed per instance
    }
    if flag(f, SIDEBAND_GAPLESS) && has(control, 8) {
        // SAFETY: eight bytes of the guest's control input.
        let g: SidebandGapless = unsafe { read(control) };
        control = control.wrapping_add(8);
        let processed = instance
            .gapless_init
            .total_samples
            .wrapping_sub(instance.gapless.total_samples);
        if g.total_samples != 0 || g.skip_samples == 0 {
            if g.total_samples >= processed {
                let diff =
                    i64::from(instance.gapless_init.total_samples) - i64::from(g.total_samples);
                instance.gapless_init.total_samples = g.total_samples;
                instance.gapless.total_samples =
                    (i64::from(instance.gapless.total_samples) - diff) as u32;
            } else {
                flags_result |= RESULT_INVALID_PARAMETER;
            }
        }
        let skipped = u32::from(instance.gapless_init.skip_samples)
            .wrapping_sub(u32::from(instance.gapless.skip_samples));
        if g.skip_samples != 0 || g.total_samples == 0 {
            if u32::from(g.skip_samples) >= skipped {
                let diff =
                    i32::from(instance.gapless_init.skip_samples) - i32::from(g.skip_samples);
                instance.gapless_init.skip_samples = g.skip_samples;
                instance.gapless.skip_samples =
                    (i32::from(instance.gapless.skip_samples) - diff) as u16;
            } else {
                flags_result |= RESULT_INVALID_PARAMETER;
            }
        }
    }
    if flag(f, CONTROL_RESAMPLE) && has(control, 8) {
        control = control.wrapping_add(8);
    }
    if flag(f, CONTROL_INITIALIZE) && has(control, 8) {
        if mp3 {
            if let Some(decoder) = instance.mp3.as_mut() {
                decoder.reset();
            }
        } else {
            // SAFETY: four bytes of the guest's control input.
            instance.config = unsafe { read(control) };
            at9_reset(instance);
        }
    }

    let in_size: u64 = job.inputs.iter().map(|chunk| u64::from(chunk.size)).sum();
    // A split job's inputs are decoded as one stream. The padding keeps LibAtrac9's reads of a
    // frame cut short inside the copy, as they stayed inside the guest's memory before.
    let mut joined = Vec::new();
    let mut input = Input {
        at: ptr::null(),
        size: in_size,
    };
    match job.inputs.as_slice() {
        [] => {}
        [only] => input.at = only.address,
        inputs => {
            joined.reserve_exact(in_size as usize + 4096);
            for chunk in inputs {
                if !chunk.address.is_null() {
                    // SAFETY: the guest's input buffer holds `size` bytes.
                    let bytes =
                        unsafe { std::slice::from_raw_parts(chunk.address, chunk.size as usize) };
                    joined.extend_from_slice(bytes);
                } else {
                    joined.resize(joined.len() + chunk.size as usize, 0);
                }
            }
            joined.resize(in_size as usize + 4096, 0);
            input.at = joined.as_ptr();
        }
    }
    if input.at.is_null() {
        input.size = 0;
    }
    let in_start = input.size;
    let mut out = Output {
        chunks: job.outputs.clone(),
        current: 0,
        offset: 0,
    };
    let out_start = out.room();
    let mut frames = 0u32;
    let mut internal = 0i32;

    if input.size != 0 && mp3 {
        loop {
            // As the ATRAC9 loop below: the stream's end, then room for the next whole frame.
            if instance.gapless_loop && gapless_end(instance) {
                gapless_reset(instance);
                instance.total_samples = 0;
            }
            if gapless_end(instance) && frames != 0 {
                break;
            }
            let mut header = mp3::Mp3Frame::default();
            if mp3::parse_frame(input.bytes(u64::from(u32::MAX)), false, &mut header) != 0 {
                // Not at a frame header: padding or a tag. Skip to the next sync word.
                let bytes = input.bytes(u64::MAX);
                let mut skip = 1usize;
                while skip + 1 < bytes.len()
                    && !(bytes[skip] == 0xff && bytes[skip + 1] & 0xe0 == 0xe0)
                {
                    skip += 1;
                }
                if skip + 1 >= bytes.len() {
                    input.advance(input.size);
                    if frames == 0 {
                        flags_result |= RESULT_PARTIAL_INPUT;
                    }
                    break;
                }
                input.advance(skip as u64);
                continue;
            }
            if input.size < header.frame_size {
                flags_result |= RESULT_PARTIAL_INPUT;
                break;
            }
            let next = next_frame_bytes(instance, header.samples_per_channel, header.num_channels);
            if !gapless_end(instance) && out.room() < u64::from(next) {
                if frames == 0 {
                    flags_result |= RESULT_NOT_ENOUGH_ROOM;
                }
                break;
            }
            let mut written = 0;
            let r = mp3_frame(
                frames_decoded,
                instance,
                &header,
                &mut input,
                &mut out,
                &mut written,
                &mut internal,
            );
            instance.total_samples += u64::from(written);
            frames += 1;
            if r != 0 {
                flags_result |= r;
                break;
            }
            if !flag(f, RUN_MULTIPLE_FRAMES) || input.size == 0 {
                break;
            }
        }
    } else if input.size != 0 {
        loop {
            if instance.gapless_loop && gapless_end(instance) {
                gapless_reset(instance);
                instance.total_samples = 0;
            }
            // The stream's last sample is out: the job is done, without asking for more input.
            // Uncharted decodes each sound effect to its end in one go and dropped every effect
            // whose last job reported partial input (engine noise, gunfire, menu clicks).
            if gapless_end(instance) && frames != 0 {
                break;
            }
            if instance.codec_flags & 1 != 0 && input.bytes(4) == b"RIFF" {
                parse_riff(instance, &mut input);
                instance.total_samples = 0;
            }
            if !instance.initialized {
                flags_result |= RESULT_NOT_INITIALIZED;
                break;
            }
            // A full output after decoded frames ends the job normally (as on the PS4, and in
            // shadPS4's Ajm): the error is only for an output too small for the first frame.
            // Uncharted's sound engine drops voices on it: engine noise and gunfire went silent.
            let next = next_frame_bytes(
                instance,
                instance.info.frame_samples as u32,
                instance.info.channels as u32,
            );
            if !gapless_end(instance) && out.room() < u64::from(next) {
                if frames != 0 {
                    break;
                }
                flags_result |= RESULT_NOT_ENOUGH_ROOM;
            }
            if input.size < u64::from(instance.superframe_remain) {
                flags_result |= RESULT_PARTIAL_INPUT;
            }
            if flags_result != 0 {
                break;
            }
            let mut written = 0;
            let r = at9_frame(
                frames_decoded,
                instance,
                &mut input,
                &mut out,
                &mut written,
                &mut internal,
            );
            instance.total_samples += u64::from(written);
            frames += 1;
            if r != 0 {
                flags_result |= r;
                break;
            }
            if !flag(f, RUN_MULTIPLE_FRAMES) {
                break;
            }
        }
    }
    let consumed = in_start.wrapping_sub(input.size);
    let written = out_start - out.room();
    drop(joined);
    if (flags_result != 0 || trace_all()) && trace() {
        host::out_line(&format!(
            "Audio trace: Ajm job instance {id} flags {} -> result {} internal {internal}; input {consumed}/{in_start} used, output {written}/{out_start} written, frames {frames}",
            hex(f),
            hex(u64::from(flags_result))
        ));
    }
    set_result(flags_result, internal);
    // Output sideband order: stream, format, gapless, multi-frame, codec info.
    if flag(f, SIDEBAND_STREAM) {
        side.put(SidebandStream {
            input_consumed: consumed as i32,
            output_written: written as i32,
            total_decoded_samples: instance.total_samples,
        });
    }
    if flag(f, SIDEBAND_FORMAT) {
        let format = if mp3 {
            SidebandFormat {
                channels: instance.mp3_channels,
                channel_mask: channel_mask(instance.mp3_channels),
                sample_rate: instance.mp3_rate,
                encoding: instance.format,
                bitrate: instance.mp3_bitrate,
                reserved: 0,
            }
        } else {
            let info = &instance.info;
            let mut format = SidebandFormat {
                channels: info.channels as u32,
                channel_mask: channel_mask(info.channels as u32),
                sample_rate: info.sampling_rate as u32,
                encoding: instance.format,
                bitrate: 0,
                reserved: 0,
            };
            if info.frames_in_superframe != 0 && info.frame_samples != 0 {
                format.bitrate = (info.sampling_rate as u64 * info.superframe_size as u64 * 8
                    / (info.frames_in_superframe as u64 * info.frame_samples as u64))
                    as u32;
            }
            format
        };
        side.put(format);
    }
    if flag(f, SIDEBAND_GAPLESS) {
        side.put(instance.gapless);
    }
    if flag(f, RUN_MULTIPLE_FRAMES) {
        side.put([frames, 0u32]);
    }
    if flag(f, RUN_CODEC_INFO) {
        if mp3 {
            // header, has_crc, channel mode, mode extension, copyright, original, emphasis
            let h = instance.mp3_header;
            let mut info = [0u8; 16];
            info[..4].copy_from_slice(&h.to_ne_bytes());
            info[4] = u8::from((h >> 16) & 1 == 0);
            info[5] = ((h >> 6) & 3) as u8;
            info[6] = ((h >> 4) & 3) as u8;
            info[7] = ((h >> 3) & 1) as u8;
            info[8] = ((h >> 2) & 1) as u8;
            info[9] = (h & 3) as u8;
            side.put(info);
        } else {
            side.put(At9Info {
                super_frame_size: instance.info.superframe_size as u32,
                frames_in_super_frame: instance.info.frames_in_superframe as u32,
                next_frame_size: instance.superframe_remain,
                frame_samples: instance.info.frame_samples as u32,
            });
        }
    }
}

// ---- API ----

/// `sceAjmInitialize`: a new context in `*out`.
unsafe extern "sysv64" fn ajm_initialize(reserved: i64, out: *mut u32) -> i32 {
    if out.is_null() || reserved != 0 {
        return ERR_INVALID_PARAMETER;
    }
    let mut state = state();
    let Some(id) = (1..=MAX_CONTEXTS).find(|&id| state.contexts[id as usize].is_none()) else {
        return ERR_OUT_OF_RESOURCES;
    };
    state.contexts[id as usize] = Some(Box::new(Context::new()));
    drop(state);
    // SAFETY: the guest's output word.
    unsafe { out.write_unaligned(id) };
    host::out_line("Runtime: Ajm context initialized (ATRAC9 via LibAtrac9)");
    0
}

/// `sceAjmFinalize`: the context and its instances go away.
extern "sysv64" fn ajm_finalize(id: u32) -> i32 {
    let mut state = state();
    if state.context(id).is_none() {
        return ERR_INVALID_CONTEXT;
    }
    state.contexts[id as usize] = None;
    0
}

/// `sceAjmModuleRegister`.
extern "sysv64" fn ajm_module_register(id: u32, codec: u32, reserved: i64) -> i32 {
    if reserved != 0 || codec >= CODECS {
        return ERR_INVALID_PARAMETER;
    }
    let mut state = state();
    let Some(context) = state.context(id) else {
        return ERR_INVALID_CONTEXT;
    };
    if context.registered[codec as usize] {
        return ERR_ALREADY_REGISTERED;
    }
    context.registered[codec as usize] = true;
    0
}

/// `sceAjmModuleUnregister`.
extern "sysv64" fn ajm_module_unregister(id: u32, codec: u32) -> i32 {
    let mut state = state();
    let Some(context) = state.context(id) else {
        return ERR_INVALID_CONTEXT;
    };
    if codec >= CODECS {
        return ERR_INVALID_PARAMETER;
    }
    if !context.registered[codec as usize] {
        return ERR_NOT_REGISTERED;
    }
    context.registered[codec as usize] = false;
    0
}

/// `sceAjmInstanceCreate`: an MP3 (codec 0) or ATRAC9 (codec 1) instance; its ID, with the codec
/// in bits 14 and up, in `*out`. Other codecs are not implemented and return an error.
unsafe extern "sysv64" fn ajm_instance_create(
    id: u32,
    codec: u32,
    flags: u64,
    out: *mut u32,
) -> i32 {
    if out.is_null() || codec >= CODECS {
        return ERR_INVALID_PARAMETER;
    }
    if flags & 7 == 0 {
        return ERR_WRONG_REVISION;
    }
    if codec > 1 {
        host::err(&format!(
            "Ajm codec {codec} is not implemented; returning an error\n"
        ));
        return ERR_INVALID_PARAMETER;
    }
    if trace() {
        host::out_line(&format!(
            "Audio trace: Ajm instance create codec {codec} flags {}",
            hex(flags)
        ));
    }
    let mut state = state();
    let Some(context) = state.context(id) else {
        return ERR_INVALID_CONTEXT;
    };
    if !context.registered[codec as usize] {
        return ERR_NOT_REGISTERED;
    }
    let Some(slot) = (1..=MAX_INSTANCES as usize).find(|&i| context.instances[i].is_none()) else {
        return ERR_OUT_OF_RESOURCES;
    };
    let instance = Instance::new(codec, flags);
    if instance.format > FORMAT_FLOAT {
        return ERR_INVALID_PARAMETER;
    }
    context.instances[slot] = Some(Box::new(instance));
    // SAFETY: the guest's output word.
    unsafe { out.write_unaligned(slot as u32 | (codec << 14)) };
    0
}

/// `sceAjmInstanceDestroy`.
extern "sysv64" fn ajm_instance_destroy(id: u32, instance: u32) -> i32 {
    let mut state = state();
    let Some(context) = state.context(id) else {
        return ERR_INVALID_CONTEXT;
    };
    let slot = (instance & MAX_INSTANCES) as usize;
    if slot == 0 || context.instances[slot].is_none() {
        return ERR_INVALID_INSTANCE;
    }
    context.instances[slot] = None;
    0
}

/// `sceAjmInstanceCodecType`.
extern "sysv64" fn ajm_instance_codec(instance: u32) -> u32 {
    (instance >> 14) & 0x1f
}

/// `sceAjmBatchStartBuffer`: runs every job of the batch now; the batch's ID in `*out`.
unsafe extern "sysv64" fn ajm_batch_start(
    id: u32,
    buffer: *mut u8,
    size: u32,
    priority: c_int,
    error: *mut BatchError,
    out: *mut u32,
) -> i32 {
    let _ = priority;
    if size & 7 != 0 {
        return ERR_MALFORMED_BATCH;
    }
    let mut state = state();
    if state.context(id).is_none() {
        return ERR_INVALID_CONTEXT;
    }
    let Some(slot) = state.batches.iter().position(|batch| !batch.used) else {
        return ERR_OUT_OF_RESOURCES;
    };
    let end = buffer.wrapping_add(size as usize);
    let mut p = buffer;
    while p < end {
        // SAFETY: the game's batch buffer, read as the C runtime read it.
        let chunk: Chunk = unsafe { read(p) };
        let body = p.wrapping_add(8);
        p = body.wrapping_add(chunk.size as usize);
        if ident(chunk.word) == IDENT_INLINE {
            continue;
        }
        let job = if ident(chunk.word) != IDENT_JOB || p > end {
            None
        } else {
            // SAFETY: the job's chunks are inside the batch buffer.
            unsafe { parse_job(body, chunk.size) }
        };
        let Some(job) = job else {
            if !error.is_null() {
                let start = body.wrapping_sub(8);
                // SAFETY: the guest's error record.
                unsafe {
                    error.write_unaligned(BatchError {
                        error_code: ERR_MALFORMED_BATCH,
                        job_address: start.cast(),
                        command_offset: (start as usize).wrapping_sub(buffer as usize) as u32,
                        job_return_address: ptr::null(),
                    })
                };
            }
            return ERR_MALFORMED_BATCH;
        };
        run_job(&mut state, id, payload(chunk.word), &job);
    }
    state.batches[slot] = Batch {
        used: true,
        canceled: false,
    };
    state.batches_run += 1;
    if !out.is_null() {
        // SAFETY: the guest's output word.
        unsafe { out.write_unaligned(slot as u32 + 1) };
    }
    0
}

fn batch_slot(state: &mut State, id: u32, batch: u32) -> Result<usize, i32> {
    if state.context(id).is_none() {
        return Err(ERR_INVALID_CONTEXT);
    }
    let slot = (batch as usize).wrapping_sub(1);
    if batch == 0 || slot >= MAX_BATCHES || !state.batches[slot].used {
        return Err(ERR_INVALID_BATCH);
    }
    Ok(slot)
}

/// `sceAjmBatchWait`: the batch ran at submission; its slot is freed.
extern "sysv64" fn ajm_batch_wait(
    id: u32,
    batch: u32,
    timeout: u32,
    error: *mut BatchError,
) -> i32 {
    let _ = (timeout, error);
    let mut state = state();
    match batch_slot(&mut state, id, batch) {
        Err(code) => code,
        Ok(slot) => {
            let canceled = state.batches[slot].canceled;
            state.batches[slot].used = false;
            if canceled {
                ERR_CANCELLED
            } else {
                0
            }
        }
    }
}

/// `sceAjmBatchCancel`: already processed at submission, so nothing to cancel.
extern "sysv64" fn ajm_batch_cancel(id: u32, batch: u32) -> i32 {
    let mut state = state();
    batch_slot(&mut state, id, batch).map_or_else(|code| code, |_| 0)
}

/// `sceAjmBatchErrorDump`.
extern "sysv64" fn ajm_error_dump() -> i32 {
    0
}

/// `sceAjmMemoryRegister`.
extern "sysv64" fn ajm_memory_register(id: u32, memory: *mut c_void, pages: u64) -> i32 {
    let _ = (memory, pages);
    if state().context(id).is_some() {
        0
    } else {
        ERR_INVALID_CONTEXT
    }
}

/// `sceAjmDecMp3ParseFrame`: the Layer III frame header at `data` (and with `parse_ofl`, the
/// FGH gapless block of a first frame).
unsafe extern "sysv64" fn mp3_parse_frame(
    data: *const u8,
    size: u32,
    parse_ofl: c_int,
    frame: *mut mp3::Mp3Frame,
) -> i32 {
    if data.is_null() || size < 4 || frame.is_null() {
        return ERR_INVALID_PARAMETER;
    }
    // SAFETY: the guest's frame data holds `size` bytes, and its record is writable.
    unsafe {
        let data = std::slice::from_raw_parts(data, size as usize);
        let mut parsed = frame.read_unaligned();
        let result = mp3::parse_frame(data, parse_ofl != 0, &mut parsed);
        frame.write_unaligned(parsed);
        result
    }
}

/// The address of this module's function for the scoped NID `name`, by its symbol name (as
/// `RUNTIME_LOOKUP` matched the C table).
pub(crate) fn resolve(name: &CStr) -> Option<usize> {
    let symbol = name.to_str().ok().and_then(crate::abi::nid_name)?;
    let address = match symbol {
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
        "sceAjmDecMp3ParseFrame" => mp3_parse_frame as *const () as usize,
        _ => return None,
    };
    Some(address)
}

/// The address of this module's function for the scoped NID `name`, or 0
/// (`uintptr_t runtime_ajm_resolve(const char *name)`).
///
/// # Safety
/// `name` is null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn runtime_ajm_resolve(name: *const c_char) -> usize {
    if name.is_null() {
        return 0;
    }
    // SAFETY: the caller passes a NUL-terminated string.
    resolve(unsafe { CStr::from_ptr(name) }).unwrap_or(0)
}

/// The module's counters on stdout (`runtime_report`).
#[no_mangle]
pub extern "C" fn runtime_ajm_report() {
    let state = state();
    host::out_line(&format!(
        "Runtime: Ajm batches={}, jobs={}, ATRAC9 frames decoded={}",
        state.batches_run, state.jobs_run, state.frames_decoded
    ));
}

#[cfg(test)]
mod tests;
