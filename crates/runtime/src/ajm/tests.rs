//! `tests/test_ajm.c` (unsupported codecs return an error, #56/#132) and the cases the C module
//! had no test for: the batch builders' layout, malformed batches, ATRAC9 and MP3 jobs with their
//! sidebands, gapless trimming, RIFF headers. The module's state is process-wide, so every test
//! holds [`LOCK`] and starts from no contexts.

use super::*;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

/// The lock, with every context finalized.
fn fresh() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut state = state();
    for context in state.contexts.iter_mut() {
        *context = None;
    }
    for batch in state.batches.iter_mut() {
        *batch = Batch::default();
    }
    guard
}

fn initialize() -> u32 {
    let mut context = 0;
    // SAFETY: a valid output word.
    assert_eq!(unsafe { ajm_initialize(0, &mut context) }, 0);
    context
}

fn create(context: u32, codec: u32, flags: u64) -> Result<u32, i32> {
    let mut instance = 0;
    // SAFETY: a valid output word.
    match unsafe { ajm_instance_create(context, codec, flags, &mut instance) } {
        0 => Ok(instance),
        error => Err(error),
    }
}

/// An instance of `codec` (registered if it was not) with output `format` and codec flags
/// `codec_flags`.
fn instance(context: u32, codec: u32, format: u64, codec_flags: u32) -> u32 {
    let registered = ajm_module_register(context, codec, 0);
    assert!(registered == 0 || registered == ERR_ALREADY_REGISTERED);
    create(
        context,
        codec,
        1 | format << 7 | u64::from(codec_flags) << 32,
    )
    .unwrap()
}

/// Runs one batch of `buffer`'s first `end` bytes; returns the batch ID.
fn start(context: u32, buffer: &mut [u8], end: *mut c_void) -> Result<u32, (i32, BatchError)> {
    let size = (end as usize - buffer.as_ptr() as usize) as u32;
    let mut error = BatchError {
        error_code: 0,
        job_address: ptr::null(),
        command_offset: 0,
        job_return_address: ptr::null(),
    };
    let mut batch = 0;
    // SAFETY: the batch is `size` bytes of `buffer`; the error and output are valid.
    let result = unsafe {
        ajm_batch_start(
            context,
            buffer.as_mut_ptr(),
            size,
            0,
            &mut error,
            &mut batch,
        )
    };
    if result == 0 {
        assert_eq!(ajm_batch_wait(context, batch, 0, ptr::null_mut()), 0);
        Ok(batch)
    } else {
        Err((result, error))
    }
}

/// One run job on `instance`; returns the sideband.
fn run(
    context: u32,
    instance: u32,
    flags: u64,
    input: &[u8],
    output: &mut [u8],
    sideband_size: usize,
) -> Vec<u8> {
    let mut buffer = [0u8; 256];
    let mut sideband = vec![0xAAu8; sideband_size];
    // SAFETY: a batch buffer large enough for one run job; the buffers outlive the batch.
    let end = unsafe {
        job_run(
            buffer.as_mut_ptr().cast(),
            instance,
            flags,
            input.as_ptr() as *mut c_void,
            input.len() as u64,
            output.as_mut_ptr().cast(),
            output.len() as u64,
            sideband.as_mut_ptr().cast(),
            sideband_size as u64,
            ptr::null_mut(),
        )
    };
    start(context, &mut buffer, end).unwrap();
    sideband
}

/// One control job on `instance` with `control` as its input.
fn control(
    context: u32,
    instance: u32,
    flags: u64,
    control: &[u8],
    sideband_size: usize,
) -> Vec<u8> {
    let mut buffer = [0u8; 128];
    let mut sideband = vec![0xAAu8; sideband_size];
    // SAFETY: a batch buffer large enough for one control job; the buffers outlive the batch.
    let end = unsafe {
        job_control(
            buffer.as_mut_ptr().cast(),
            instance,
            flags,
            control.as_ptr() as *mut c_void,
            control.len() as u64,
            sideband.as_mut_ptr().cast(),
            sideband_size as u64,
            ptr::null_mut(),
        )
    };
    start(context, &mut buffer, end).unwrap();
    sideband
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn i32_at(bytes: &[u8], at: usize) -> i32 {
    u32_at(bytes, at) as i32
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_ne_bytes(bytes[at..at + 8].try_into().unwrap())
}

const STREAM: u64 = 1 << SIDEBAND_STREAM;
const FORMAT: u64 = 1 << SIDEBAND_FORMAT;
const GAPLESS: u64 = 1 << SIDEBAND_GAPLESS;
const MULTIPLE: u64 = 1 << RUN_MULTIPLE_FRAMES;
const CODEC_INFO: u64 = 1 << RUN_CODEC_INFO;
const INITIALIZE: u64 = 1 << CONTROL_INITIALIZE;
const RESET: u64 = 1 << CONTROL_RESET;

/// ATRAC9, 48 kHz mono, 256-byte frames of 256 samples, four frames per superframe.
const AT9_CONFIG: [u8; 4] = [0xFE, 0x70, 0x1F, 0xF0];
/// One decoded [`AT9_CONFIG`] frame in S16.
const AT9_FRAME_S16: usize = 256 * 2;
/// The bytes of one [`at9_silence`] frame: LibAtrac9 reports the bits a frame used, rounded
/// up to bytes, and the superframe's padding is skipped after its last frame.
const AT9_FRAME_BYTES: usize = 13;

/// An ATRAC9 instance initialized with [`AT9_CONFIG`].
fn at9_instance(context: u32, format: u64, codec_flags: u32) -> u32 {
    let id = instance(context, 1, format, codec_flags);
    let mut init = [0u8; 8];
    init[..4].copy_from_slice(&AT9_CONFIG);
    let side = control(context, id, INITIALIZE, &init, 8);
    assert_eq!((i32_at(&side, 0), i32_at(&side, 4)), (0, 0));
    id
}

#[test]
fn unsupported_codecs_return_an_error() {
    let _lock = fresh();
    // test_ajm.c, through the functions the guest's imports resolve to.
    let get = |nid: &CStr| resolve(nid).expect("resolves");
    // SAFETY: the resolved addresses are this module's functions with these signatures.
    let (init, register, create) = unsafe {
        (
            std::mem::transmute::<usize, unsafe extern "sysv64" fn(i64, *mut u32) -> i32>(get(
                c"dl+4eHSzUu4#N#O",
            )),
            std::mem::transmute::<usize, extern "sysv64" fn(u32, u32, i64) -> i32>(get(
                c"Q3dyFuwGn64#N#O",
            )),
            std::mem::transmute::<usize, unsafe extern "sysv64" fn(u32, u32, u64, *mut u32) -> i32>(
                get(c"AxoDrINp4J8#N#O"),
            ),
        )
    };
    let mut context = 0;
    // SAFETY: valid output words throughout.
    unsafe {
        assert_eq!(init(0, &mut context), 0);
        assert_eq!(context, 1);
        assert_eq!((register(context, 0, 0), register(context, 1, 0)), (0, 0));
        assert_eq!(register(context, 2, 0), 0);
        // AAC (codec 2) is not implemented: an error comes back and the output is untouched.
        let mut out = 0xdead_beef;
        assert_eq!(create(context, 2, 1, &mut out) as u32, 0x8093_0005);
        assert_eq!(out, 0xdead_beef);
        // MP3 (codec 0) and ATRAC9 (codec 1) still create instances.
        assert_eq!(create(context, 0, 1, &mut out), 0);
        assert_eq!(out, 0x1);
        assert_eq!(create(context, 1, 1, &mut out), 0);
        assert_eq!(out, 0x4002);
    }
}

#[test]
fn every_export_resolves_by_its_nid() {
    let _lock = fresh();
    for (nid, function) in [
        (c"dl+4eHSzUu4#N#O", ajm_initialize as *const () as usize),
        (c"MHur6qCsUus#N#O", ajm_finalize as *const () as usize),
        (
            c"Q3dyFuwGn64#N#O",
            ajm_module_register as *const () as usize,
        ),
        (
            c"Wi7DtlLV+KI#N#O",
            ajm_module_unregister as *const () as usize,
        ),
        (
            c"AxoDrINp4J8#N#O",
            ajm_instance_create as *const () as usize,
        ),
        (
            c"RbLbuKv8zho#N#O",
            ajm_instance_destroy as *const () as usize,
        ),
        (c"dmDybN--Fn8#N#O", job_control as *const () as usize),
        (c"ElslOCpOIns#N#O", job_run as *const () as usize),
        (c"7jdAXK+2fMo#N#O", job_run_split as *const () as usize),
        (c"fFFkk0xfGWs#N#O", ajm_batch_start as *const () as usize),
        (c"-qLsfDAywIY#N#O", ajm_batch_wait as *const () as usize),
        (c"NVDXiUesSbA#N#O", ajm_batch_cancel as *const () as usize),
        (c"WfAiBW8Wcek#N#O", ajm_error_dump as *const () as usize),
        (c"eDFeTyi+G3Y#N#O", mp3_parse_frame as *const () as usize),
    ] {
        assert_eq!(resolve(nid), Some(function), "{nid:?}");
        // SAFETY: a NUL-terminated literal.
        assert_eq!(unsafe { runtime_ajm_resolve(nid.as_ptr()) }, function);
    }
    assert_eq!(resolve(c"dl+4eHSzUu4#wrong#module"), None);
    assert_eq!(resolve(c"8G2LB+A3rzg#q#q"), None); // atexit: not this module's
                                                   // SAFETY: null is accepted.
    assert_eq!(unsafe { runtime_ajm_resolve(ptr::null()) }, 0);
}

#[test]
fn contexts_modules_and_instances() {
    let _lock = fresh();
    // SAFETY: null output and a valid one.
    unsafe {
        assert_eq!(ajm_initialize(0, ptr::null_mut()), ERR_INVALID_PARAMETER);
        let mut out = 0;
        assert_eq!(ajm_initialize(1, &mut out), ERR_INVALID_PARAMETER);
    }
    let contexts: Vec<u32> = (0..MAX_CONTEXTS).map(|_| initialize()).collect();
    assert_eq!(contexts, (1..=8).collect::<Vec<_>>());
    let mut out = 0;
    // SAFETY: a valid output word.
    assert_eq!(unsafe { ajm_initialize(0, &mut out) }, ERR_OUT_OF_RESOURCES);
    assert_eq!(ajm_finalize(3), 0);
    assert_eq!(ajm_finalize(3), ERR_INVALID_CONTEXT);
    assert_eq!(
        (ajm_finalize(0), ajm_finalize(9)),
        (ERR_INVALID_CONTEXT, ERR_INVALID_CONTEXT)
    );
    assert_eq!(initialize(), 3);

    assert_eq!(ajm_module_register(1, 24, 0), ERR_INVALID_PARAMETER);
    assert_eq!(ajm_module_register(1, 1, 1), ERR_INVALID_PARAMETER);
    assert_eq!(ajm_module_register(9, 1, 0), ERR_INVALID_CONTEXT);
    assert_eq!(ajm_module_register(1, 1, 0), 0);
    assert_eq!(ajm_module_register(1, 1, 0), ERR_ALREADY_REGISTERED);
    assert_eq!(ajm_module_unregister(1, 0), ERR_NOT_REGISTERED);
    assert_eq!(ajm_module_unregister(1, 24), ERR_INVALID_PARAMETER);
    assert_eq!(ajm_module_unregister(9, 0), ERR_INVALID_CONTEXT);
    assert_eq!(ajm_module_unregister(1, 1), 0);

    // Not registered, wrong revision, bad format, null output.
    assert_eq!(create(1, 1, 1), Err(ERR_NOT_REGISTERED));
    assert_eq!(ajm_module_register(1, 1, 0), 0);
    assert_eq!(create(1, 1, 0), Err(ERR_WRONG_REVISION));
    assert_eq!(create(1, 1, 1 | 3 << 7), Err(ERR_INVALID_PARAMETER));
    assert_eq!(create(1, 24, 1), Err(ERR_INVALID_PARAMETER));
    assert_eq!(create(9, 1, 1), Err(ERR_INVALID_CONTEXT));
    // SAFETY: a null output is rejected before anything is written.
    assert_eq!(
        unsafe { ajm_instance_create(1, 1, 1, ptr::null_mut()) },
        ERR_INVALID_PARAMETER
    );
    // Instances take the lowest free ID, the codec in bits 14 and up.
    assert_eq!(create(1, 1, 1), Ok(0x4001));
    assert_eq!(create(1, 1, 1), Ok(0x4002));
    assert_eq!(ajm_instance_codec(0x4002), 1);
    assert_eq!(ajm_instance_destroy(1, 0x4001), 0);
    assert_eq!(ajm_instance_destroy(1, 0x4001), ERR_INVALID_INSTANCE);
    assert_eq!(ajm_instance_destroy(1, 0x4000), ERR_INVALID_INSTANCE);
    assert_eq!(ajm_instance_destroy(9, 0x4002), ERR_INVALID_CONTEXT);
    assert_eq!(create(1, 1, 1), Ok(0x4001));
    // Finalizing frees the instances.
    assert_eq!(ajm_finalize(1), 0);
    assert_eq!(initialize(), 1);
    assert_eq!(ajm_module_register(1, 1, 0), 0);
    assert_eq!(create(1, 1, 1), Ok(0x4001));
    assert_eq!(ajm_memory_register(1, ptr::null_mut(), 1), 0);
    assert_eq!(
        ajm_memory_register(9, ptr::null_mut(), 1),
        ERR_INVALID_CONTEXT
    );
    assert_eq!(ajm_error_dump(), 0);
}

#[test]
fn the_builders_write_the_sdk_chunk_layout() {
    let _lock = fresh();
    let mut buffer = [0u8; 256];
    let base = buffer.as_mut_ptr();
    let (input, output, side, ret) = (0x1000usize, 0x2000usize, 0x3000usize, 0x4000usize);
    // SAFETY: the buffer has room for each job; the addresses are only stored.
    let end = unsafe {
        job_run(
            base.cast(),
            0x4001,
            u64::MAX,
            input as *mut c_void,
            100,
            output as *mut c_void,
            200,
            side as *mut c_void,
            16,
            ret as *mut c_void,
        )
    };
    // Job header, return address, input, flags, output, sideband: 8 + 16 * 4 + 8 bytes.
    assert_eq!(end as usize - base as usize, 80);
    assert_eq!(u32_at(&buffer, 0), header(IDENT_JOB, 0x4001));
    assert_eq!(u32_at(&buffer, 4), 72);
    assert_eq!(u32_at(&buffer, 8), IDENT_RETURN_ADDRESS);
    assert_eq!(u64_at(&buffer, 16), ret as u64);
    assert_eq!(
        (u32_at(&buffer, 24), u32_at(&buffer, 28)),
        (IDENT_INPUT_RUN, 100)
    );
    assert_eq!(u64_at(&buffer, 32), input as u64);
    // Run flags are masked to 0xE00000001FFF: payload holds the high word, size the low one.
    assert_eq!(u32_at(&buffer, 40), header(IDENT_RUN_FLAGS, 0xE000));
    assert_eq!(u32_at(&buffer, 44), 0x1FFF);
    assert_eq!(
        (u32_at(&buffer, 48), u32_at(&buffer, 52)),
        (IDENT_OUTPUT_RUN, 200)
    );
    assert_eq!(u64_at(&buffer, 56), output as u64);
    assert_eq!(
        (u32_at(&buffer, 64), u32_at(&buffer, 68)),
        (IDENT_OUTPUT_CONTROL, 16)
    );
    assert_eq!(u64_at(&buffer, 72), side as u64);

    // A control job without a return address; statistics jobs keep other flags.
    buffer = [0; 256];
    // SAFETY: as above.
    let end = unsafe {
        job_control(
            base.cast(),
            0x4001,
            u64::MAX,
            input as *mut c_void,
            8,
            side as *mut c_void,
            8,
            ptr::null_mut(),
        )
    };
    assert_eq!(end as usize - base as usize, 48);
    assert_eq!(
        (u32_at(&buffer, 0), u32_at(&buffer, 4)),
        (header(IDENT_JOB, 0x4001), 40)
    );
    assert_eq!(u32_at(&buffer, 8), IDENT_INPUT_CONTROL);
    assert_eq!(u32_at(&buffer, 24), header(IDENT_CONTROL_FLAGS, 0x6000));
    assert_eq!(u32_at(&buffer, 28), 0xE7FF);
    assert_eq!(u32_at(&buffer, 32), IDENT_OUTPUT_CONTROL);
    // SAFETY: as above.
    unsafe {
        job_control(
            base.cast(),
            STATISTICS_INSTANCE,
            u64::MAX,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
        )
    };
    assert_eq!(u32_at(&buffer, 0), header(IDENT_JOB, STATISTICS_INSTANCE));
    assert_eq!(u32_at(&buffer, 24), header(IDENT_CONTROL_FLAGS, 0));
    assert_eq!(u32_at(&buffer, 28), 0xC001_8007);

    // A split job lists every input, then the flags, then every output.
    buffer = [0; 256];
    let inputs = [
        AjmBuffer {
            address: 0x10 as *mut c_void,
            size: 1,
        },
        AjmBuffer {
            address: 0x20 as *mut c_void,
            size: 2,
        },
    ];
    let outputs = [AjmBuffer {
        address: 0x30 as *mut c_void,
        size: 3,
    }];
    // SAFETY: as above.
    let end = unsafe {
        job_run_split(
            base.cast(),
            0x4002,
            1,
            inputs.as_ptr(),
            2,
            outputs.as_ptr(),
            1,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
        )
    };
    assert_eq!(end as usize - base as usize, 8 + 16 + 16 + 8 + 16 + 16);
    assert_eq!(
        (u32_at(&buffer, 8), u32_at(&buffer, 12)),
        (IDENT_INPUT_RUN, 1)
    );
    assert_eq!(
        (u32_at(&buffer, 24), u32_at(&buffer, 28)),
        (IDENT_INPUT_RUN, 2)
    );
    assert_eq!(u32_at(&buffer, 40), header(IDENT_RUN_FLAGS, 0));
    assert_eq!(
        (u32_at(&buffer, 48), u32_at(&buffer, 52)),
        (IDENT_OUTPUT_RUN, 3)
    );

    // Inline data is padded to eight bytes; `address` points at the copy.
    buffer = [0; 256];
    let mut landed: *const c_void = ptr::null();
    // SAFETY: as above, and five readable bytes.
    let end = unsafe { job_inline(base.cast(), b"hello".as_ptr().cast(), 5, &mut landed) };
    assert_eq!(end as usize - base as usize, 16);
    assert_eq!((u32_at(&buffer, 0), u32_at(&buffer, 4)), (IDENT_INLINE, 8));
    assert_eq!(&buffer[8..13], b"hello");
    assert_eq!(landed as usize, base as usize + 8);
}

#[test]
fn malformed_batches_and_batch_slots() {
    let _lock = fresh();
    let context = initialize();
    let id = instance(context, 1, 0, 0);
    let mut buffer = [0u8; 256];
    let mut batch = 0;
    // SAFETY: valid buffers; sizes not a multiple of 8 are rejected first.
    unsafe {
        assert_eq!(
            ajm_batch_start(
                context,
                buffer.as_mut_ptr(),
                12,
                0,
                ptr::null_mut(),
                &mut batch
            ),
            ERR_MALFORMED_BATCH
        );
        assert_eq!(
            ajm_batch_start(9, buffer.as_mut_ptr(), 0, 0, ptr::null_mut(), &mut batch),
            ERR_INVALID_CONTEXT
        );
    }
    // An inline chunk, then a chunk that is not a job.
    // SAFETY: the buffer has room.
    let end = unsafe {
        job_inline(
            buffer.as_mut_ptr().cast(),
            [1u8; 8].as_ptr().cast(),
            8,
            ptr::null_mut(),
        )
    };
    let offset = end as usize - buffer.as_ptr() as usize;
    buffer[offset..offset + 8].copy_from_slice(&[5, 0, 0, 0, 0, 0, 0, 0]);
    let (code, error) = start(context, &mut buffer, end.wrapping_byte_add(8)).unwrap_err();
    assert_eq!(
        (code, error.error_code),
        (ERR_MALFORMED_BATCH, ERR_MALFORMED_BATCH)
    );
    assert_eq!(error.command_offset, 16);
    assert_eq!(error.job_address as usize, buffer.as_ptr() as usize + 16);
    assert!(error.job_return_address.is_null());
    // A job without flags; a job that runs past the batch.
    buffer = [0; 256];
    buffer[..8].copy_from_slice(&[0, 0, 0, 0, 16, 0, 0, 0]);
    buffer[8] = IDENT_INPUT_RUN as u8;
    let end = buffer.as_mut_ptr().wrapping_add(24).cast();
    assert_eq!(
        start(context, &mut buffer, end).unwrap_err().0,
        ERR_MALFORMED_BATCH
    );
    buffer[4] = 64;
    assert_eq!(
        start(context, &mut buffer, end).unwrap_err().0,
        ERR_MALFORMED_BATCH
    );

    // An empty run job on a destroyed instance reports a fatal invalid parameter.
    assert_eq!(ajm_instance_destroy(context, id), 0);
    let side = run(context, id, 0, &[], &mut [], 8);
    assert_eq!(
        i32_at(&side, 0) as u32,
        RESULT_FATAL | RESULT_INVALID_PARAMETER
    );
    // A statistics job zeroes its sideband.
    let side = control(context, STATISTICS_INSTANCE, 0, &[], 24);
    assert!(side.iter().all(|&byte| byte == 0));

    // Batch IDs come from 256 slots, freed by waiting.
    buffer = [0; 256];
    let mut batches = Vec::new();
    for _ in 0..MAX_BATCHES {
        // SAFETY: an empty batch.
        assert_eq!(
            unsafe {
                ajm_batch_start(
                    context,
                    buffer.as_mut_ptr(),
                    0,
                    0,
                    ptr::null_mut(),
                    &mut batch,
                )
            },
            0
        );
        batches.push(batch);
    }
    assert_eq!(batches, (1..=256).collect::<Vec<_>>());
    // SAFETY: as above.
    assert_eq!(
        unsafe {
            ajm_batch_start(
                context,
                buffer.as_mut_ptr(),
                0,
                0,
                ptr::null_mut(),
                &mut batch,
            )
        },
        ERR_OUT_OF_RESOURCES
    );
    assert_eq!(ajm_batch_cancel(context, 7), 0);
    assert_eq!(ajm_batch_wait(context, 7, 0, ptr::null_mut()), 0);
    assert_eq!(
        ajm_batch_wait(context, 7, 0, ptr::null_mut()),
        ERR_INVALID_BATCH
    );
    assert_eq!(ajm_batch_cancel(context, 7), ERR_INVALID_BATCH);
    assert_eq!(
        ajm_batch_wait(context, 0, 0, ptr::null_mut()),
        ERR_INVALID_BATCH
    );
    assert_eq!(
        ajm_batch_wait(context, 257, 0, ptr::null_mut()),
        ERR_INVALID_BATCH
    );
    assert_eq!(
        ajm_batch_wait(9, 1, 0, ptr::null_mut()),
        ERR_INVALID_CONTEXT
    );
    assert_eq!(ajm_batch_cancel(9, 1), ERR_INVALID_CONTEXT);
    // SAFETY: as above.
    assert_eq!(
        unsafe {
            ajm_batch_start(
                context,
                buffer.as_mut_ptr(),
                0,
                0,
                ptr::null_mut(),
                &mut batch,
            )
        },
        0
    );
    assert_eq!(batch, 7);
}

/// `superframes` superframes of [`AT9_CONFIG`] silence: four frames back to back, then zero
/// padding. Each frame is one mono block: the "not first in superframe" bit (all frames but the
/// first), zero band, gradient and extension parameters (36 bits), scale factor mode 1
/// (fixed-length, all zero) and zero spectra, so every sample decodes to zero.
fn at9_silence(superframes: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; 1024 * superframes];
    for superframe in bytes.as_chunks_mut::<1024>().0 {
        for index in 0..4 {
            let frame = &mut superframe[index * AT9_FRAME_BYTES..];
            if index != 0 {
                frame[0] = 0x80;
            }
            frame[4] = 0x04; // bits 36-37: scale factor mode 01
        }
    }
    bytes
}

#[test]
fn atrac9_jobs_decode_frames_and_report_sidebands() {
    let _lock = fresh();
    let context = initialize();
    let id = at9_instance(context, u64::from(FORMAT_S16), 0);
    // One frame: 256 mono S16 samples.
    let input = at9_silence(2);
    let mut output = vec![0xAAu8; 4096];
    let side = run(
        context,
        id,
        STREAM | FORMAT | GAPLESS | CODEC_INFO,
        &input,
        &mut output,
        8 + 16 + 24 + 8 + 16,
    );
    assert_eq!((i32_at(&side, 0), i32_at(&side, 4)), (0, 0));
    // Stream: one frame in, 256 * 2 bytes out, 256 samples so far.
    assert_eq!(
        (i32_at(&side, 8) as usize, i32_at(&side, 12) as usize),
        (AT9_FRAME_BYTES, AT9_FRAME_S16)
    );
    assert_eq!(u64_at(&side, 16), 256);
    // Format: mono, 48 kHz, S16, 48000 * 1024 * 8 / (4 * 256) bits per second.
    assert_eq!(
        [
            u32_at(&side, 24),
            u32_at(&side, 28),
            u32_at(&side, 32),
            u32_at(&side, 36)
        ],
        [1, 4, 48000, FORMAT_S16]
    );
    assert_eq!((u32_at(&side, 40), u32_at(&side, 44)), (384_000, 0));
    // Gapless: nothing set.
    assert_eq!(u64_at(&side, 48), 0);
    // Codec info: superframe 1024 bytes, 4 frames, the bytes left of it, 256 samples.
    assert_eq!(
        [
            u32_at(&side, 56),
            u32_at(&side, 60),
            u32_at(&side, 64),
            u32_at(&side, 68)
        ],
        [1024, 4, 1024 - AT9_FRAME_BYTES as u32, 256]
    );
    assert!(output[..AT9_FRAME_S16].iter().all(|&byte| byte == 0));
    assert!(output[AT9_FRAME_S16..].iter().all(|&byte| byte == 0xAA));

    // Several frames: the rest of the superframe (3 frames), then the next one, until the output
    // is full. Ending on a full output after a frame is not an error.
    let mut output = vec![0xAAu8; AT9_FRAME_S16 * 5];
    let side = run(
        context,
        id,
        STREAM | MULTIPLE,
        &input[AT9_FRAME_BYTES..],
        &mut output,
        8 + 16 + 8,
    );
    assert_eq!((i32_at(&side, 0), i32_at(&side, 4)), (0, 0));
    // Consumed: three frames, the first superframe's padding, two frames of the second.
    assert_eq!(
        (i32_at(&side, 8) as usize, i32_at(&side, 12) as usize),
        (
            1024 - AT9_FRAME_BYTES + 2 * AT9_FRAME_BYTES,
            AT9_FRAME_S16 * 5
        )
    );
    assert_eq!(u64_at(&side, 16), 256 * 6);
    assert_eq!((u32_at(&side, 24), u32_at(&side, 28)), (5, 0));

    // An output too small for one frame; an input shorter than the superframe's rest.
    let side = run(context, id, STREAM, &input[..1024], &mut [0u8; 100], 8 + 16);
    assert_eq!(i32_at(&side, 0) as u32, RESULT_NOT_ENOUGH_ROOM);
    assert_eq!((i32_at(&side, 8), i32_at(&side, 12)), (0, 0));
    let side = run(context, id, 0, &input[..100], &mut [0u8; 1024], 8);
    assert_eq!(i32_at(&side, 0) as u32, RESULT_PARTIAL_INPUT);

    // An instance that was never initialized.
    let raw = create(context, 1, 1).unwrap();
    let side = run(context, raw, 0, &input, &mut [0u8; 1024], 8);
    assert_eq!(i32_at(&side, 0) as u32, RESULT_NOT_INITIALIZED);
    // A configuration LibAtrac9 rejects leaves it uninitialized.
    let side = control(context, raw, INITIALIZE, &[0u8; 8], 8);
    assert_eq!(i32_at(&side, 0), 0);
    let side = run(context, raw, 0, &input, &mut [0u8; 1024], 8);
    assert_eq!(i32_at(&side, 0) as u32, RESULT_NOT_INITIALIZED);
}

#[test]
fn atrac9_output_formats_and_split_inputs() {
    let _lock = fresh();
    let context = initialize();
    for (format, size) in [(FORMAT_S16, 2), (FORMAT_S32, 4), (FORMAT_FLOAT, 4)] {
        // Non-interleaved (codec flag bit 8) changes only the order; silence is zero either way.
        for codec_flags in [0, 1 << 8] {
            let id = at9_instance(context, u64::from(format), codec_flags);
            let mut output = vec![0xAAu8; 256 * size + 8];
            let side = run(context, id, STREAM, &at9_silence(1), &mut output, 24);
            assert_eq!(i32_at(&side, 12) as usize, 256 * size);
            assert!(output[..256 * size].iter().all(|&byte| byte == 0));
            assert_eq!(output[256 * size], 0xAA);
        }
    }
    // A split job joins its inputs into one stream and fills its outputs in order.
    let id = at9_instance(context, u64::from(FORMAT_S16), 0);
    let input = at9_silence(1);
    let mut first = vec![0xAAu8; 600];
    let mut second = vec![0xAAu8; 424];
    let inputs = [
        AjmBuffer {
            address: input.as_ptr() as *mut c_void,
            size: 20,
        },
        AjmBuffer {
            address: input[20..].as_ptr() as *mut c_void,
            size: 1004,
        },
    ];
    let outputs = [
        AjmBuffer {
            address: first.as_mut_ptr().cast(),
            size: 600,
        },
        AjmBuffer {
            address: second.as_mut_ptr().cast(),
            size: 424,
        },
    ];
    let mut sideband = [0xAAu8; 24];
    let mut buffer = [0u8; 256];
    // SAFETY: the buffers outlive the batch.
    let end = unsafe {
        job_run_split(
            buffer.as_mut_ptr().cast(),
            id,
            STREAM | MULTIPLE,
            inputs.as_ptr(),
            2,
            outputs.as_ptr(),
            2,
            sideband.as_mut_ptr().cast(),
            24,
            ptr::null_mut(),
        )
    };
    start(context, &mut buffer, end).unwrap();
    // Two frames (the second across the inputs' seam) fill 1024 bytes across both outputs.
    assert_eq!((i32_at(&sideband, 0), i32_at(&sideband, 4)), (0, 0));
    assert_eq!(
        (i32_at(&sideband, 8), i32_at(&sideband, 12)),
        (2 * AT9_FRAME_BYTES as i32, 1024)
    );
    assert!(first.iter().chain(&second).all(|&byte| byte == 0));
}

#[test]
fn atrac9_gapless_and_riff_headers() {
    let _lock = fresh();
    let context = initialize();
    // RIFF parsing is codec flag bit 0. fmt carries the configuration at byte 44, fact the total
    // samples and the encoder delay.
    let id = instance(context, 1, u64::from(FORMAT_S16), 1);
    let mut riff = Vec::new();
    riff.extend_from_slice(b"RIFF\0\0\0\0WAVE");
    riff.extend_from_slice(b"fmt ");
    riff.extend_from_slice(&52u32.to_le_bytes());
    let mut fmt = [0u8; 52];
    fmt[44..48].copy_from_slice(&AT9_CONFIG);
    riff.extend_from_slice(&fmt);
    riff.extend_from_slice(b"fact");
    riff.extend_from_slice(&12u32.to_le_bytes());
    riff.extend_from_slice(&300u32.to_le_bytes()); // total samples
    riff.extend_from_slice(&100u32.to_le_bytes()); // encoder delay
    riff.extend_from_slice(&0u32.to_le_bytes());
    riff.extend_from_slice(b"data");
    riff.extend_from_slice(&2048u32.to_le_bytes());
    let header = riff.len();
    riff.extend_from_slice(&at9_silence(2));
    let mut output = vec![0xAAu8; 4096];
    let side = run(
        context,
        id,
        STREAM | GAPLESS | MULTIPLE,
        &riff,
        &mut output,
        8 + 16 + 8 + 8,
    );
    assert_eq!((i32_at(&side, 0), i32_at(&side, 4)), (0, 0));
    // 300 samples after skipping 100: frames of 156 and 144 samples. The stream ended in the
    // first superframe, so the rest of it is drained and skipped.
    assert_eq!(i32_at(&side, 8) as usize, header + 1024);
    assert_eq!(i32_at(&side, 12), 300 * 2);
    assert_eq!(u64_at(&side, 16), 300);
    assert_eq!(
        gapless_at(&side, 24),
        SidebandGapless {
            total_samples: 0,
            skip_samples: 0,
            skipped_samples: 100 + 112,
        }
    );
    assert_eq!((u32_at(&side, 32), u32_at(&side, 36)), (2, 0));
    assert_eq!(output[300 * 2], 0xAA);

    // After the end, a job writes nothing and is not an error (it drains the superframe it
    // started); a reset starts the stream over. An empty control input changes nothing.
    let side = run(context, id, STREAM, &at9_silence(1), &mut output, 24);
    assert_eq!(
        (i32_at(&side, 0), i32_at(&side, 8), i32_at(&side, 12)),
        (0, 1024, 0)
    );
    let side = control(context, id, RESET | GAPLESS, &[], 16);
    assert_eq!(i32_at(&side, 0), 0);
    let gapless = gapless_at(&side, 8);
    assert_eq!((gapless.total_samples, gapless.skip_samples), (300, 100));

    // A gapless control block moves the total and skip; one below what was used is invalid.
    let mut block = [0u8; 8];
    block[..4].copy_from_slice(&1000u32.to_ne_bytes());
    block[4..6].copy_from_slice(&50u16.to_ne_bytes());
    let side = control(context, id, GAPLESS, &block, 16);
    assert_eq!(i32_at(&side, 0), 0);
    let gapless = gapless_at(&side, 8);
    assert_eq!((gapless.total_samples, gapless.skip_samples), (1000, 50));
    let side = run(context, id, STREAM, &at9_silence(1), &mut output, 24);
    assert_eq!(i32_at(&side, 12), (256 - 50) * 2);
    block[..4].copy_from_slice(&100u32.to_ne_bytes());
    block[4..6].copy_from_slice(&10u16.to_ne_bytes());
    let side = control(context, id, GAPLESS, &block, 8);
    assert_eq!(i32_at(&side, 0) as u32, RESULT_INVALID_PARAMETER);

    // Gapless looping (instance flag bit 10) restarts the stream at its end: here every
    // superframe's first frame is a whole stream, and the job stops when the output is full.
    let looping = create(context, 1, 1 | 1 << 10).unwrap();
    let mut init = [0u8; 8];
    init[..4].copy_from_slice(&AT9_CONFIG);
    control(context, looping, INITIALIZE, &init, 8);
    block[..4].copy_from_slice(&256u32.to_ne_bytes());
    block[4..6].copy_from_slice(&0u16.to_ne_bytes());
    control(context, looping, GAPLESS, &block, 8);
    let mut two_frames = vec![0xAAu8; AT9_FRAME_S16 * 2];
    let side = run(
        context,
        looping,
        STREAM | MULTIPLE,
        &at9_silence(3),
        &mut two_frames,
        32,
    );
    assert_eq!((i32_at(&side, 0), u32_at(&side, 24)), (0, 2));
    assert_eq!((i32_at(&side, 8), i32_at(&side, 12)), (2048, 1024));
}

fn gapless_at(bytes: &[u8], at: usize) -> SidebandGapless {
    SidebandGapless {
        total_samples: u32_at(bytes, at),
        skip_samples: u16::from_ne_bytes(bytes[at + 4..at + 6].try_into().unwrap()),
        skipped_samples: u16::from_ne_bytes(bytes[at + 6..at + 8].try_into().unwrap()),
    }
}

/// True when every 4-byte float in `bytes` is zero (of either sign).
fn silent_f32(bytes: &[u8]) -> bool {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .all(|&v| f32::from_ne_bytes(v) == 0.0)
}

#[test]
fn mp3_jobs_decode_frames_and_report_sidebands() {
    let _lock = fresh();
    let context = initialize();
    let id = instance(context, 0, u64::from(FORMAT_FLOAT), 0);
    let frame = mp3::tests::silent_frame(mp3::tests::HEADER_128K);
    // A tag before the first frame is skipped.
    let mut input = b"TAG?".to_vec();
    input.extend_from_slice(&frame);
    input.extend_from_slice(&frame);
    let mut output = vec![0xAAu8; 1152 * 2 * 4 * 2];
    let side = run(
        context,
        id,
        STREAM | FORMAT | GAPLESS | MULTIPLE | CODEC_INFO,
        &input,
        &mut output,
        8 + 16 + 24 + 8 + 8 + 16,
    );
    assert_eq!((i32_at(&side, 0), i32_at(&side, 4)), (0, 0));
    assert_eq!(
        (i32_at(&side, 8), i32_at(&side, 12)),
        (4 + 417 * 2, 1152 * 2 * 4 * 2)
    );
    assert_eq!(u64_at(&side, 16), 2304);
    assert_eq!(
        [
            u32_at(&side, 24),
            u32_at(&side, 28),
            u32_at(&side, 32),
            u32_at(&side, 36)
        ],
        [2, 3, 44100, FORMAT_FLOAT]
    );
    assert_eq!((u32_at(&side, 40), u32_at(&side, 44)), (128_000, 0));
    assert_eq!(u64_at(&side, 48), 0);
    assert_eq!((u32_at(&side, 56), u32_at(&side, 60)), (2, 0));
    // Codec info: the header word, no CRC, joint stereo, mode extension 2, original.
    assert_eq!(u32_at(&side, 64), 0xFFFB_9064);
    assert_eq!(&side[68..80], &[0, 1, 2, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
    assert!(silent_f32(&output));

    // Room for less than a frame; a frame cut short; skip and total samples.
    let side = run(context, id, 0, &frame, &mut [0u8; 100], 8);
    assert_eq!(i32_at(&side, 0) as u32, RESULT_NOT_ENOUGH_ROOM);
    let side = run(context, id, 0, &frame[..300], &mut output, 8);
    assert_eq!(i32_at(&side, 0) as u32, RESULT_PARTIAL_INPUT);
    let side = run(context, id, 0, &[0u8; 64], &mut output, 8);
    assert_eq!(i32_at(&side, 0) as u32, RESULT_PARTIAL_INPUT);
    let s16 = instance(context, 0, u64::from(FORMAT_S16), 0);
    let mut block = [0u8; 8];
    block[..4].copy_from_slice(&1500u32.to_ne_bytes());
    block[4..6].copy_from_slice(&576u16.to_ne_bytes());
    control(context, s16, GAPLESS, &block, 8);
    let mut twice = frame.clone();
    twice.extend_from_slice(&frame);
    let side = run(context, s16, STREAM | MULTIPLE, &twice, &mut output, 32);
    assert_eq!(i32_at(&side, 0), 0);
    // 576 skipped, then 576 + 924 samples: the total ends the stream in the second frame.
    assert_eq!(i32_at(&side, 12), 1500 * 2 * 2);
    assert_eq!(u32_at(&side, 24), 2);
    // A damaged frame is skipped as silence with the decoder's error as internal result.
    let mut damaged = frame.clone();
    damaged[4..40].fill(0xFF);
    let side = run(context, id, 0, &damaged, &mut output, 8);
    assert_eq!(i32_at(&side, 0), 0);
    let internal = i32_at(&side, 4);
    assert!(internal == 0 || internal == MP3_INVALID_DATA, "{internal}");
}

#[test]
fn mp3_parse_frame_is_the_guest_function() {
    let _lock = fresh();
    let mut frame = mp3::Mp3Frame {
        frame_size: 99,
        ..mp3::Mp3Frame::default()
    };
    let data = mp3::tests::silent_frame(mp3::tests::HEADER_128K);
    // SAFETY: valid data and record; null pointers are rejected.
    unsafe {
        assert_eq!(mp3_parse_frame(data.as_ptr(), 417, 1, &mut frame), 0);
        assert_eq!((frame.frame_size, frame.sample_rate), (417, 44100));
        assert_eq!(
            mp3_parse_frame(data.as_ptr(), 3, 0, &mut frame),
            ERR_INVALID_PARAMETER
        );
        assert_eq!(
            mp3_parse_frame(ptr::null(), 417, 0, &mut frame),
            ERR_INVALID_PARAMETER
        );
        assert_eq!(
            mp3_parse_frame(data.as_ptr(), 417, 0, ptr::null_mut()),
            ERR_INVALID_PARAMETER
        );
    }
}

#[test]
fn the_report_counts_batches_jobs_and_frames() {
    let _lock = fresh();
    let before = {
        let state = state();
        (state.batches_run, state.jobs_run, state.frames_decoded)
    };
    let context = initialize();
    let id = at9_instance(context, u64::from(FORMAT_S16), 0);
    run(context, id, 0, &at9_silence(1), &mut [0u8; 1024], 8);
    let state = state();
    assert_eq!(state.batches_run - before.0, 2);
    assert_eq!(state.jobs_run - before.1, 2);
    assert_eq!(state.frames_decoded - before.2, 1);
    drop(state);
    runtime_ajm_report();
}
