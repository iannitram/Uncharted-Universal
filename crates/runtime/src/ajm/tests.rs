//! `tests/test_ajm.c` and the pieces of `runtime_ajm.c` it left untested. ATRAC9 runs on a
//! stand-in decoder (LibAtrac9 is C and only linked into the loader); MP3 runs the real decoder
//! on a silent frame built here.

use super::*;

/// Four bytes and 4 samples of 2 channels per frame, 2 frames in an 8-byte superframe; frame
/// `n` is the samples `n*100 + 0..8`.
struct Fake {
    n: i16,
}

impl At9Decoder for Fake {
    fn init(&mut self, config: [u8; 4]) -> Option<CodecInfo> {
        (config != [0xff; 4]).then_some(CodecInfo {
            channels: 2,
            sampling_rate: 48000,
            superframe_size: 8,
            frames_in_superframe: 2,
            frame_samples: 4,
        })
    }

    fn decode(&mut self, input: &[u8], pcm: &mut [i16]) -> Result<usize, i32> {
        if input.len() < 4 {
            return Err(-5);
        }
        for (i, sample) in pcm.iter_mut().take(8).enumerate() {
            *sample = self.n * 100 + i as i16;
        }
        self.n += 1;
        Ok(4)
    }
}

fn fake() -> Box<dyn At9Decoder> {
    Box::new(Fake { n: 0 })
}

const RUN_MULTIPLE: u64 = 1 << 12;
const RUN_CODEC_INFO: u64 = 1 << 11;
const CONTROL_RESET: u64 = 1 << 13;
const CONTROL_INITIALIZE: u64 = 1 << 14;
const SIDEBAND_GAPLESS: u64 = 1 << 45;
const SIDEBAND_FORMAT: u64 = 1 << 46;
const SIDEBAND_STREAM: u64 = 1 << 47;
const REVISION: u64 = 1;
const NON_INTERLEAVED: u64 = 1 << 40;
const RIFF: u64 = 1 << 32;

fn buffer(bytes: &[u8]) -> ChunkBuffer {
    ChunkBuffer {
        word: 0,
        size: bytes.len() as u32,
        address: bytes.as_ptr() as usize,
    }
}

fn u32_of(bytes: &[u8], at: usize) -> u32 {
    u32_at(bytes, at).unwrap()
}

fn i16s(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

/// A context with ATRAC9 and MP3 registered, and one instance of `codec`.
fn rig(codec: u32, flags: u64) -> (Ajm, u32, u32) {
    let mut ajm = Ajm::new(fake);
    let context = ajm.initialize(0).unwrap();
    ajm.module_register(context, 0, 0).unwrap();
    ajm.module_register(context, 1, 0).unwrap();
    let instance = ajm.instance_create(context, codec, flags).unwrap();
    (ajm, context, instance)
}

fn initialize(ajm: &mut Ajm, context: u32, instance: u32) {
    let config = [0u8; 8];
    let sideband = [0u8; 8];
    let job = build_control(
        instance,
        CONTROL_INITIALIZE,
        buffer(&config),
        buffer(&sideband),
        0,
    );
    ajm.batch_start(context, &job).unwrap();
    assert_eq!(u32_of(&sideband, 0), 0);
}

/// Runs one run job and returns the sideband bytes.
fn run(
    ajm: &mut Ajm,
    context: u32,
    instance: u32,
    flags: u64,
    input: &[u8],
    output: &mut [u8],
) -> Vec<u8> {
    let sideband = [0xeeu8; 80];
    let job = build_run(
        instance,
        flags,
        buffer(input),
        buffer(output),
        buffer(&sideband),
        0,
    );
    ajm.batch_start(context, &job).unwrap();
    sideband.to_vec()
}

// tests/test_ajm.c
#[test]
fn unsupported_codec_returns_an_error() {
    let mut ajm = Ajm::new(fake);
    let context = ajm.initialize(0).unwrap();
    assert_eq!(context, 1);
    for codec in 0..3 {
        ajm.module_register(context, codec, 0).unwrap();
    }
    // AAC (codec 2) is not implemented: an error code comes back.
    assert_eq!(
        ajm.instance_create(context, 2, 1),
        Err(ERR_INVALID_PARAMETER)
    );
    // MP3 (codec 0) and ATRAC9 (codec 1) still create instances.
    assert_eq!(ajm.instance_create(context, 0, 1), Ok(0x1));
    assert_eq!(ajm.instance_create(context, 1, 1), Ok(0x4002));
}

#[test]
fn contexts_modules_and_instances_follow_the_c_errors() {
    let mut ajm = Ajm::new(fake);
    assert_eq!(ajm.initialize(1), Err(ERR_INVALID_PARAMETER));
    let ids: Vec<u32> = (0..8).map(|_| ajm.initialize(0).unwrap()).collect();
    assert_eq!(ids, (1..=8).collect::<Vec<_>>());
    assert_eq!(ajm.initialize(0), Err(ERR_OUT_OF_RESOURCES));
    assert_eq!(ajm.finalize(3), Ok(()));
    assert_eq!(ajm.finalize(3), Err(ERR_INVALID_CONTEXT));
    assert_eq!(ajm.initialize(0), Ok(3));
    assert_eq!(ajm.module_register(0, 1, 0), Err(ERR_INVALID_CONTEXT));
    assert_eq!(ajm.module_register(9, 1, 0), Err(ERR_INVALID_CONTEXT));
    assert_eq!(ajm.module_register(1, 24, 0), Err(ERR_INVALID_PARAMETER));
    assert_eq!(ajm.module_register(1, 1, 1), Err(ERR_INVALID_PARAMETER));
    assert_eq!(ajm.module_unregister(1, 1), Err(ERR_NOT_REGISTERED));
    assert_eq!(ajm.module_register(1, 1, 0), Ok(()));
    assert_eq!(ajm.module_register(1, 1, 0), Err(ERR_ALREADY_REGISTERED));
    assert_eq!(ajm.instance_create(1, 0, 1), Err(ERR_NOT_REGISTERED));
    assert_eq!(ajm.instance_create(1, 24, 1), Err(ERR_INVALID_PARAMETER));
    assert_eq!(ajm.instance_create(1, 1, 0), Err(ERR_WRONG_REVISION));
    assert_eq!(
        ajm.instance_create(1, 1, 1 | (3 << 7)),
        Err(ERR_INVALID_PARAMETER)
    );
    assert_eq!(ajm.instance_create(1, 1, 1 | (2 << 7)), Ok(0x4001));
    assert_eq!(ajm.instance_create(1, 1, 1), Ok(0x4002));
    assert_eq!(ajm.instance_destroy(1, 0x4001), Ok(()));
    assert_eq!(ajm.instance_destroy(1, 0x4001), Err(ERR_INVALID_INSTANCE));
    assert_eq!(ajm.instance_destroy(1, 0x4000), Err(ERR_INVALID_INSTANCE));
    assert_eq!(ajm.instance_destroy(9, 1), Err(ERR_INVALID_CONTEXT));
    // The freed slot is the first free one.
    assert_eq!(ajm.instance_create(1, 1, 1), Ok(0x4001));
    assert_eq!(ajm.memory_register(1), Ok(()));
    assert_eq!(ajm.memory_register(0), Err(ERR_INVALID_CONTEXT));
}

#[test]
fn batch_buffers_have_the_sdk_layout() {
    let input = [1u8; 3];
    let output = [0u8; 6];
    let side = [0u8; 4];
    let control = build_control(0x4001, u64::MAX, buffer(&input), buffer(&output), 0x1234);
    // Job header, then the return address, input control, flags and output control chunks.
    assert_eq!(control.len(), 8 + 16 + 16 + 8 + 16);
    assert_eq!(u32_of(&control, 0), 0x4001 << 6);
    assert_eq!(u32_of(&control, 4) as usize, control.len() - 8);
    assert_eq!(u32_of(&control, 8) & 0x3f, IDENT_RETURN_ADDRESS);
    assert_eq!(u32_of(&control, 12), 0);
    assert_eq!(&control[16..24], &0x1234u64.to_le_bytes());
    assert_eq!(u32_of(&control, 24) & 0x3f, IDENT_INPUT_CONTROL);
    assert_eq!(u32_of(&control, 28), 3);
    assert_eq!(&control[32..40], &(input.as_ptr() as u64).to_le_bytes());
    // The control flag mask keeps 0x6000_0000_E7FF.
    assert_eq!(u32_of(&control, 40) & 0x3f, IDENT_CONTROL_FLAGS);
    assert_eq!(u32_of(&control, 40) >> 6, 0x6000);
    assert_eq!(u32_of(&control, 44), 0xE7FF);
    assert_eq!(u32_of(&control, 48) & 0x3f, IDENT_OUTPUT_CONTROL);
    assert_eq!(u32_of(&control, 52), 6);

    // The statistics instance has its own mask.
    let stats = build_control(
        STATISTICS_INSTANCE,
        u64::MAX,
        buffer(&input),
        buffer(&output),
        0,
    );
    assert_eq!(u32_of(&stats, 24) >> 6, 0);
    assert_eq!(u32_of(&stats, 28), 0xC001_8007);

    let run = build_run(
        2,
        u64::MAX,
        buffer(&input),
        buffer(&output),
        buffer(&side),
        0,
    );
    assert_eq!(run.len(), 8 + 16 + 8 + 16 + 16);
    assert_eq!(u32_of(&run, 8) & 0x3f, IDENT_INPUT_RUN);
    assert_eq!(u32_of(&run, 24) & 0x3f, IDENT_RUN_FLAGS);
    assert_eq!(u32_of(&run, 24) >> 6, 0xE000);
    assert_eq!(u32_of(&run, 28), 0x1FFF);
    assert_eq!(u32_of(&run, 32) & 0x3f, IDENT_OUTPUT_RUN);
    assert_eq!(u32_of(&run, 48) & 0x3f, IDENT_OUTPUT_CONTROL);
    assert_eq!(u32_of(&run, 52), 4);

    let split = build_run_split(
        2,
        0,
        &[buffer(&input), buffer(&input)],
        &[buffer(&output), buffer(&output), buffer(&output)],
        buffer(&side),
        0,
    );
    assert_eq!(split.len(), 8 + 2 * 16 + 8 + 3 * 16 + 16);
    assert_eq!(u32_of(&split, 8) & 0x3f, IDENT_INPUT_RUN);
    assert_eq!(u32_of(&split, 24) & 0x3f, IDENT_INPUT_RUN);
    assert_eq!(u32_of(&split, 40) & 0x3f, IDENT_RUN_FLAGS);

    let inline = build_inline(&[7u8; 5]);
    assert_eq!(inline.len(), 16);
    assert_eq!(u32_of(&inline, 0) & 0x3f, IDENT_INLINE);
    assert_eq!(u32_of(&inline, 4), 8);
    assert_eq!(&inline[8..13], &[7u8; 5]);
    assert_eq!(&inline[13..], &[0u8; 3]);
}

#[test]
fn malformed_batches_are_refused() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    let input = [0u8; 8];
    let output = [0u8; 16];
    let side = [0u8; 8];
    let good = build_run(
        instance,
        0,
        buffer(&input),
        buffer(&output),
        buffer(&side),
        0,
    );
    // Not a multiple of 8, an unknown context, a chunk that is not a job, a job without flags,
    // and a job that runs off the end.
    assert_eq!(
        ajm.batch_start(context, &good[..good.len() - 1])
            .unwrap_err()
            .code,
        ERR_MALFORMED_BATCH
    );
    assert_eq!(
        ajm.batch_start(7, &good).unwrap_err().code,
        ERR_INVALID_CONTEXT
    );
    let mut bad = good.clone();
    bad[0] = IDENT_INPUT_RUN as u8;
    assert_eq!(
        ajm.batch_start(context, &bad).unwrap_err(),
        BatchFailure {
            code: ERR_MALFORMED_BATCH,
            offset: 0
        }
    );
    let mut no_flags = good.clone();
    no_flags[24] = 0x3f; // the flags chunk becomes an unknown chunk
    assert_eq!(
        ajm.batch_start(context, &no_flags).unwrap_err().code,
        ERR_MALFORMED_BATCH
    );
    let mut long = good.clone();
    long[4] += 8;
    assert_eq!(
        ajm.batch_start(context, &long).unwrap_err().code,
        ERR_MALFORMED_BATCH
    );
    // The second job is the bad one: its offset is reported, the first already ran.
    let second = [good.clone(), bad].concat();
    assert_eq!(
        ajm.batch_start(context, &second).unwrap_err(),
        BatchFailure {
            code: ERR_MALFORMED_BATCH,
            offset: good.len()
        }
    );
    // Inline chunks between jobs are skipped.
    let inline = build_inline(&[1, 2, 3]);
    let ok = [inline, good.clone()].concat();
    let batch = ajm.batch_start(context, &ok).unwrap();
    assert_eq!(ajm.batch_cancel(context, batch), Ok(()));
    assert_eq!(ajm.batch_wait(context, batch), Ok(()));
    assert_eq!(ajm.batch_wait(context, batch), Err(ERR_INVALID_BATCH));
    assert_eq!(ajm.batch_cancel(context, batch), Err(ERR_INVALID_BATCH));
    assert_eq!(ajm.batch_wait(context, 0), Err(ERR_INVALID_BATCH));
    assert_eq!(ajm.batch_wait(context, 257), Err(ERR_INVALID_BATCH));
    assert_eq!(ajm.batch_wait(5, 1), Err(ERR_INVALID_CONTEXT));
}

#[test]
fn batch_slots_run_out_after_256_unwaited_batches() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    let side = [0u8; 8];
    let job = build_run(
        instance,
        0,
        ChunkBuffer::default(),
        ChunkBuffer::default(),
        buffer(&side),
        0,
    );
    for expected in 1..=256 {
        assert_eq!(ajm.batch_start(context, &job), Ok(expected));
    }
    assert_eq!(
        ajm.batch_start(context, &job).unwrap_err().code,
        ERR_OUT_OF_RESOURCES
    );
    assert_eq!(ajm.batch_wait(context, 2), Ok(()));
    assert_eq!(ajm.batch_start(context, &job), Ok(2));
}

#[test]
fn an_unknown_instance_fails_its_job_and_statistics_clear_their_output() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    let side = [0xaau8; 16];
    let job = build_run(
        0x4005,
        0,
        ChunkBuffer::default(),
        ChunkBuffer::default(),
        buffer(&side),
        0,
    );
    ajm.batch_start(context, &job).unwrap();
    assert_eq!(u32_of(&side, 0), RESULT_FATAL | RESULT_INVALID_PARAMETER);
    assert_eq!(u32_of(&side, 4), 0);
    assert_eq!(side[8], 0xaa, "the rest is not touched");
    let stats = build_control(
        STATISTICS_INSTANCE,
        0,
        ChunkBuffer::default(),
        buffer(&side),
        0,
    );
    ajm.batch_start(context, &stats).unwrap();
    assert_eq!(side, [0u8; 16]);
    let _ = instance;
}

#[test]
fn at9_decodes_a_frame_and_steps_through_the_superframe() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    initialize(&mut ajm, context, instance);
    let input = [9u8; 8];
    let mut output = [0u8; 64];
    let side = run(
        &mut ajm,
        context,
        instance,
        SIDEBAND_STREAM,
        &input,
        &mut output,
    );
    assert_eq!(u32_of(&side, 0), 0, "result");
    assert_eq!(u32_of(&side, 4), 0, "internal result");
    assert_eq!(u32_of(&side, 8), 4, "input consumed");
    assert_eq!(u32_of(&side, 12), 16, "output written");
    assert_eq!(u32_of(&side, 16), 4, "total decoded samples");
    assert_eq!(i16s(&output[..16]), (0..8).collect::<Vec<i16>>());
    assert_eq!(output[16], 0, "one frame only");

    // The second frame ends the superframe: nothing is left to skip.
    let mut output = [0u8; 64];
    let side = run(
        &mut ajm,
        context,
        instance,
        SIDEBAND_STREAM,
        &input[4..],
        &mut output,
    );
    assert_eq!(u32_of(&side, 8), 4);
    assert_eq!(u32_of(&side, 16), 8);
    assert_eq!(i16s(&output[..16]), (100..108).collect::<Vec<i16>>());
}

#[test]
fn at9_multiple_frames_stop_for_input_like_the_c_loop() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    initialize(&mut ajm, context, instance);
    let input = [9u8; 8];
    let mut output = [0u8; 64];
    let side = run(
        &mut ajm,
        context,
        instance,
        RUN_MULTIPLE,
        &input,
        &mut output,
    );
    // Two frames, then the loop asks for more input of the next superframe.
    assert_eq!(u32_of(&side, 0), RESULT_PARTIAL_INPUT);
    assert_eq!(u32_of(&side, 8), 2, "frames, in the multi-frame sideband");
    assert_eq!(
        i16s(&output[..32]),
        (0..8).chain(100..108).collect::<Vec<i16>>()
    );
}

#[test]
fn at9_needs_room_for_its_first_frame_only() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    initialize(&mut ajm, context, instance);
    let input = [9u8; 8];
    let mut small = [0u8; 15];
    let side = run(&mut ajm, context, instance, 0, &input, &mut small);
    assert_eq!(u32_of(&side, 0), RESULT_NOT_ENOUGH_ROOM);
    assert_eq!(small, [0u8; 15]);
    // Room for one frame and a bit: the second frame ends the job without an error.
    let mut room = [0u8; 20];
    let side = run(&mut ajm, context, instance, RUN_MULTIPLE, &input, &mut room);
    assert_eq!(u32_of(&side, 0), 0);
    assert_eq!(u32_of(&side, 8), 1);
}

#[test]
fn at9_without_a_valid_config_is_not_initialized() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    let mut output = [0u8; 64];
    let side = run(&mut ajm, context, instance, 0, &[0u8; 8], &mut output);
    assert_eq!(u32_of(&side, 0), RESULT_NOT_INITIALIZED);
    // A config the decoder refuses leaves it that way.
    let config = [0xffu8; 8];
    let side = [0u8; 8];
    let job = build_control(
        instance,
        CONTROL_INITIALIZE,
        buffer(&config),
        buffer(&side),
        0,
    );
    ajm.batch_start(context, &job).unwrap();
    let side = run(&mut ajm, context, instance, 0, &[0u8; 8], &mut output);
    assert_eq!(u32_of(&side, 0), RESULT_NOT_INITIALIZED);
}

#[test]
fn at9_converts_to_the_requested_format() {
    let input = [9u8; 8];
    // S32: the 16-bit sample in the high half.
    let (mut ajm, context, instance) = rig(1, REVISION | (1 << 7));
    initialize(&mut ajm, context, instance);
    let mut output = [0u8; 32];
    run(&mut ajm, context, instance, 0, &input, &mut output);
    let words: Vec<i32> = output
        .chunks(4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(
        &words[..8],
        &(0..8).map(|v| v << 16).collect::<Vec<i32>>()[..]
    );
    // Float: divided by 32768.
    let (mut ajm, context, instance) = rig(1, REVISION | (2 << 7));
    initialize(&mut ajm, context, instance);
    let mut output = [0u8; 32];
    run(&mut ajm, context, instance, 0, &input, &mut output);
    let floats: Vec<f32> = output
        .chunks(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(floats[3], 3.0 / 32768.0);
    // Non-interleaved: a plane per channel.
    let (mut ajm, context, instance) = rig(1, REVISION | NON_INTERLEAVED);
    initialize(&mut ajm, context, instance);
    let mut output = [0u8; 16];
    run(&mut ajm, context, instance, 0, &input, &mut output);
    assert_eq!(i16s(&output), vec![0, 2, 4, 6, 1, 3, 5, 7]);
}

#[test]
fn at9_writes_across_split_outputs_and_reads_split_inputs() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    initialize(&mut ajm, context, instance);
    let (first, second) = ([9u8; 3], [9u8; 5]);
    let (a, b) = ([0u8; 10], [0u8; 6]);
    let side = [0u8; 8];
    let job = build_run_split(
        instance,
        0,
        &[buffer(&first), buffer(&second)],
        &[buffer(&a), buffer(&b)],
        buffer(&side),
        0,
    );
    ajm.batch_start(context, &job).unwrap();
    assert_eq!(u32_of(&side, 0), 0);
    let all: Vec<u8> = a.iter().chain(b.iter()).copied().collect();
    assert_eq!(i16s(&all), (0..8).collect::<Vec<i16>>());
}

#[test]
fn at9_riff_header_sets_the_decoder_and_gapless_trim() {
    let (mut ajm, context, instance) = rig(1, REVISION | RIFF);
    let mut stream = Vec::new();
    stream.extend_from_slice(b"RIFF\0\0\0\0WAVE");
    stream.extend_from_slice(b"fmt ");
    stream.extend_from_slice(&52u32.to_le_bytes());
    stream.extend_from_slice(&[0u8; 52]);
    stream.extend_from_slice(b"fact");
    stream.extend_from_slice(&12u32.to_le_bytes());
    stream.extend_from_slice(&6u32.to_le_bytes()); // total samples
    stream.extend_from_slice(&2u32.to_le_bytes()); // samples to skip
    stream.extend_from_slice(&[0u8; 4]);
    stream.extend_from_slice(b"data");
    stream.extend_from_slice(&8u32.to_le_bytes());
    stream.extend_from_slice(&[9u8; 8]);
    let mut output = [0u8; 64];
    let side = run(
        &mut ajm,
        context,
        instance,
        RUN_MULTIPLE | SIDEBAND_STREAM | SIDEBAND_GAPLESS,
        &stream,
        &mut output,
    );
    // Two frames end the stream (6 samples, 2 skipped); no partial-input result.
    assert_eq!(u32_of(&side, 0), 0);
    assert_eq!(
        u32_of(&side, 8),
        stream.len() as u32,
        "input consumed, header included"
    );
    assert_eq!(u32_of(&side, 12), 24, "output written");
    assert_eq!(u32_of(&side, 16), 6);
    // Stream sideband (16 bytes) from 8, then gapless: total 0, skip 0, skipped 2.
    assert_eq!(
        &side[24..32],
        &Gapless {
            total_samples: 0,
            skip_samples: 0,
            skipped_samples: 2
        }
        .encode()
    );
    assert_eq!(u32_of(&side, 32), 2, "frames");
    assert_eq!(
        i16s(&output[..24]),
        vec![4, 5, 6, 7, 100, 101, 102, 103, 104, 105, 106, 107]
    );
}

#[test]
fn the_gapless_control_sideband_moves_the_stream_end() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    initialize(&mut ajm, context, instance);
    let control = Gapless {
        total_samples: 5,
        skip_samples: 1,
        skipped_samples: 0,
    }
    .encode();
    let side = [0u8; 8];
    let job = build_control(
        instance,
        SIDEBAND_GAPLESS,
        buffer(&control),
        buffer(&side),
        0,
    );
    ajm.batch_start(context, &job).unwrap();
    assert_eq!(u32_of(&side, 0), 0);
    let input = [9u8; 8];
    let mut output = [0u8; 64];
    let side = run(
        &mut ajm,
        context,
        instance,
        RUN_MULTIPLE | SIDEBAND_GAPLESS,
        &input,
        &mut output,
    );
    // 1 sample skipped, 5 in all: 3 from the first frame, 2 from the second.
    assert_eq!(u32_of(&side, 0), 0);
    assert_eq!(
        &side[8..16],
        &Gapless {
            total_samples: 0,
            skip_samples: 0,
            skipped_samples: 3
        }
        .encode()
    );
    assert_eq!(u32_of(&side, 16), 2);
    assert_eq!(
        i16s(&output[..20]),
        vec![2, 3, 4, 5, 6, 7, 100, 101, 102, 103]
    );
}

#[test]
fn a_gapless_total_below_what_ran_is_an_invalid_parameter() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    initialize(&mut ajm, context, instance);
    let set = |ajm: &mut Ajm, total: u32| {
        let control = Gapless {
            total_samples: total,
            skip_samples: 0,
            skipped_samples: 0,
        }
        .encode();
        let side = [0u8; 8];
        let job = build_control(
            instance,
            SIDEBAND_GAPLESS,
            buffer(&control),
            buffer(&side),
            0,
        );
        ajm.batch_start(context, &job).unwrap();
        u32_of(&side, 0)
    };
    assert_eq!(set(&mut ajm, 8), 0);
    let input = [9u8; 8];
    let mut output = [0u8; 64];
    run(&mut ajm, context, instance, 0, &input, &mut output);
    assert_eq!(set(&mut ajm, 2), RESULT_INVALID_PARAMETER);
}

#[test]
fn at9_sidebands_come_in_order() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    initialize(&mut ajm, context, instance);
    let input = [9u8; 8];
    let mut output = [0u8; 64];
    let side = run(
        &mut ajm,
        context,
        instance,
        SIDEBAND_STREAM | SIDEBAND_FORMAT | SIDEBAND_GAPLESS | RUN_MULTIPLE | RUN_CODEC_INFO,
        &input[..4],
        &mut output,
    );
    // Result (8), stream (16), format (24), gapless (8), multi-frame (8), codec info (16). Four
    // bytes are less than the 8 of the superframe, so the first frame reports partial input.
    assert_eq!(u32_of(&side, 0), RESULT_PARTIAL_INPUT);
    assert_eq!(u32_of(&side, 8), 0);
    let format = &side[24..48];
    assert_eq!(u32_of(format, 0), 2, "channels");
    assert_eq!(u32_of(format, 4), 3, "channel mask");
    assert_eq!(u32_of(format, 8), 48000);
    assert_eq!(u32_of(format, 12), 0, "S16");
    assert_eq!(u32_of(format, 16), 48000 * 8 * 8 / (2 * 4), "bit rate");
    assert_eq!(&side[48..56], &[0u8; 8], "gapless");
    assert_eq!(u32_of(&side, 56), 0, "frames");
    assert_eq!(u32_of(&side, 64), 8, "super frame size");
    assert_eq!(u32_of(&side, 68), 2, "frames in super frame");
    assert_eq!(u32_of(&side, 72), 8, "next frame size");
    assert_eq!(u32_of(&side, 76), 4, "frame samples");
}

#[test]
fn reset_restarts_the_stream() {
    let (mut ajm, context, instance) = rig(1, REVISION);
    initialize(&mut ajm, context, instance);
    let input = [9u8; 8];
    let mut output = [0u8; 16];
    run(&mut ajm, context, instance, 0, &input, &mut output);
    let side = [0u8; 8];
    let job = build_control(
        instance,
        CONTROL_RESET,
        ChunkBuffer::default(),
        buffer(&side),
        0,
    );
    ajm.batch_start(context, &job).unwrap();
    let mut output = [0u8; 16];
    run(&mut ajm, context, instance, 0, &input, &mut output);
    assert_eq!(
        i16s(&output),
        (0..8).collect::<Vec<i16>>(),
        "the decoder starts again"
    );
}

// ---- MP3 ----

/// An MPEG-1 Layer III frame header: 128 kbit/s, 44100 Hz, stereo, no CRC.
const STEREO: [u8; 4] = [0xff, 0xfb, 0x90, 0x00];
const MONO: [u8; 4] = [0xff, 0xfb, 0x90, 0xc0];

fn silent_frame(header: [u8; 4], size: usize) -> Vec<u8> {
    let mut frame = vec![0u8; size];
    frame[..4].copy_from_slice(&header);
    frame
}

#[test]
fn mp3_frame_headers_parse() {
    let stereo = mp3_parse_frame(&STEREO, false).unwrap();
    assert_eq!(
        stereo,
        Mp3Frame {
            frame_size: 417,
            num_channels: 2,
            samples_per_channel: 1152,
            bitrate: 128_000,
            sample_rate: 44100,
            ..Mp3Frame::default()
        }
    );
    assert_eq!(mp3_parse_frame(&MONO, false).unwrap().num_channels, 1);
    // Padding adds a byte; MPEG-2 has 576 samples and a different table.
    assert_eq!(
        mp3_parse_frame(&[0xff, 0xfb, 0x92, 0x00], false)
            .unwrap()
            .frame_size,
        418
    );
    let low = mp3_parse_frame(&[0xff, 0xf3, 0x90, 0x00], false).unwrap();
    assert_eq!(
        (
            low.samples_per_channel,
            low.sample_rate,
            low.bitrate,
            low.frame_size
        ),
        (576, 22050, 80_000, 261)
    );
    for bad in [
        &[0xff, 0xfb, 0x90][..],
        &[0x00, 0xfb, 0x90, 0x00],
        &[0xff, 0xfd, 0x90, 0x00], // Layer II
        &[0xff, 0xfb, 0xf0, 0x00], // bit rate index 15
        &[0xff, 0xfb, 0x9c, 0x00], // sample rate index 3
        &[0xff, 0xeb, 0x90, 0x00], // version 1 (reserved)
    ] {
        assert_eq!(
            mp3_parse_frame(bad, false),
            Err(ERR_INVALID_PARAMETER),
            "{bad:02x?}"
        );
    }
}

#[test]
fn mp3_fgh_block_gives_the_gapless_values() {
    let mut frame = silent_frame(STEREO, 417);
    // Side information (32 bytes, all zero: no main data), then the block.
    let at = 4 + 32;
    frame[at] = 0xB4;
    frame[at + 1..at + 3].copy_from_slice(&1105u16.to_be_bytes());
    frame[at + 3..at + 7].copy_from_slice(&123_456u32.to_be_bytes());
    frame[at + 9] = fgh_crc(&frame[at..at + 10]);
    let parsed = mp3_parse_frame(&frame, true).unwrap();
    assert_eq!(
        (parsed.encoder_delay, parsed.total_samples, parsed.ofl_type),
        (1105, 123_456, 3)
    );
    // Without the request, with a bad checksum, or in a frame cut short there is nothing.
    assert_eq!(mp3_parse_frame(&frame, false).unwrap().ofl_type, 0);
    let mut bad = frame.clone();
    bad[at + 9] ^= 1;
    assert_eq!(mp3_parse_frame(&bad, true).unwrap().ofl_type, 0);
    assert_eq!(mp3_parse_frame(&frame[..300], true).unwrap().ofl_type, 0);
    // A Xing header is not a gapless block.
    let mut xing = frame.clone();
    xing[at..at + 4].copy_from_slice(b"Xing");
    assert_eq!(mp3_parse_frame(&xing, true).unwrap().ofl_type, 0);
}

#[test]
fn mp3_decodes_frames_and_skips_padding() {
    let (mut ajm, context, instance) = rig(0, REVISION);
    let frame = silent_frame(STEREO, 417);
    let mut output = vec![0xeeu8; 3 * 4608];
    let side = run(
        &mut ajm,
        context,
        instance,
        SIDEBAND_STREAM | SIDEBAND_FORMAT | RUN_MULTIPLE | RUN_CODEC_INFO,
        &frame,
        &mut output,
    );
    // One frame of 1152 samples of two channels, silent; the input is used up.
    assert_eq!(u32_of(&side, 0), 0);
    assert_eq!(u32_of(&side, 8), 417);
    assert_eq!(u32_of(&side, 12), 4608);
    assert_eq!(u32_of(&side, 16), 1152);
    assert!(output[..4608].iter().all(|&b| b == 0));
    assert_eq!(output[4608], 0xee);
    let format = &side[24..48];
    assert_eq!(
        (
            u32_of(format, 0),
            u32_of(format, 4),
            u32_of(format, 8),
            u32_of(format, 16)
        ),
        (2, 3, 44100, 128_000)
    );
    assert_eq!(u32_of(&side, 48), 1, "frames");
    // Codec info: the header word, has_crc, channel mode, extension, copyright, original, emphasis.
    assert_eq!(&side[56..60], &0xfffb_9000u32.to_le_bytes());
    assert_eq!(&side[60..66], &[0, 0, 0, 0, 0, 0]);

    // Three bytes of padding, two frames and a cut-off third.
    let mut stream = vec![0u8, 1, 2];
    stream.extend_from_slice(&frame);
    stream.extend_from_slice(&frame);
    stream.extend_from_slice(&frame[..100]);
    let mut output = vec![0u8; 3 * 4608];
    let side = run(
        &mut ajm,
        context,
        instance,
        RUN_MULTIPLE | SIDEBAND_STREAM,
        &stream,
        &mut output,
    );
    assert_eq!(u32_of(&side, 0), RESULT_PARTIAL_INPUT);
    assert_eq!(u32_of(&side, 8), 3 + 2 * 417);
    assert_eq!(u32_of(&side, 12), 2 * 4608);
    assert_eq!(u32_of(&side, 24), 2, "frames");

    // Without MULTIPLE_FRAMES one frame per job; output room for the first frame only.
    let mut small = vec![0u8; 4607];
    let side = run(&mut ajm, context, instance, 0, &frame, &mut small);
    assert_eq!(u32_of(&side, 0), RESULT_NOT_ENOUGH_ROOM);
    // Nothing but noise: partial input.
    let mut output = vec![0u8; 4608];
    let side = run(&mut ajm, context, instance, 0, &[1u8; 64], &mut output);
    assert_eq!(u32_of(&side, 0), RESULT_PARTIAL_INPUT);
}

#[test]
fn mp3_converts_and_gapless_trims_the_decoded_frame() {
    let frame = silent_frame(MONO, 417);
    // Float output, mono, first 100 samples skipped through the instance's gapless values.
    let (mut ajm, context, instance) = rig(0, REVISION | (2 << 7));
    let control = Gapless {
        total_samples: 1000,
        skip_samples: 100,
        skipped_samples: 0,
    }
    .encode();
    let side = [0u8; 8];
    let job = build_control(
        instance,
        SIDEBAND_GAPLESS,
        buffer(&control),
        buffer(&side),
        0,
    );
    ajm.batch_start(context, &job).unwrap();
    let mut output = vec![0u8; 4608];
    let side = run(
        &mut ajm,
        context,
        instance,
        SIDEBAND_STREAM | SIDEBAND_GAPLESS,
        &frame,
        &mut output,
    );
    assert_eq!(u32_of(&side, 0), 0);
    assert_eq!(
        u32_of(&side, 12),
        1000 * 4,
        "capped at the stream's total of 1000 samples"
    );
    assert_eq!(
        &side[24..32],
        &Gapless {
            total_samples: 0,
            skip_samples: 0,
            skipped_samples: 152
        }
        .encode()
    );
}

#[test]
fn a_corrupt_mp3_body_never_aborts_the_job() {
    let (mut ajm, context, instance) = rig(0, REVISION);
    // A valid header with nonsense side information: the frame is consumed and the job ends
    // normally, with silence or whatever the decoder makes of it.
    let mut frame = silent_frame(STEREO, 417);
    frame[4] = 0xff;
    frame[5] = 0xff;
    let mut output = vec![0u8; 4608];
    let side = run(
        &mut ajm,
        context,
        instance,
        SIDEBAND_STREAM,
        &frame,
        &mut output,
    );
    assert_eq!(u32_of(&side, 0), 0);
    assert_eq!(u32_of(&side, 8), 417, "the frame is consumed");
    eprintln!(
        "INTERNAL {} WRITTEN {}",
        u32_of(&side, 4) as i32,
        u32_of(&side, 12)
    );
}

// ---- exports ----

#[test]
fn the_seventeen_exports_are_listed_once() {
    let mut names = EXPORTS.to_vec();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 17);
    // Those with a NID in the table resolve by it, as the C file's table did.
    for (nid, symbol) in [
        ("dl+4eHSzUu4#N#O", "sceAjmInitialize"),
        ("Q3dyFuwGn64#N#O", "sceAjmModuleRegister"),
        ("-qLsfDAywIY#N#O", "sceAjmBatchWait"),
        ("Wi7DtlLV+KI#N#O", "sceAjmModuleUnregister"),
        ("MHur6qCsUus#N#O", "sceAjmFinalize"),
        ("AxoDrINp4J8#N#O", "sceAjmInstanceCreate"),
        ("NVDXiUesSbA#N#O", "sceAjmBatchCancel"),
        ("RbLbuKv8zho#N#O", "sceAjmInstanceDestroy"),
        ("dmDybN--Fn8#N#O", "sceAjmBatchJobControlBufferRa"),
        ("ElslOCpOIns#N#O", "sceAjmBatchJobRunBufferRa"),
        ("fFFkk0xfGWs#N#O", "sceAjmBatchStartBuffer"),
        ("WfAiBW8Wcek#N#O", "sceAjmBatchErrorDump"),
        ("7jdAXK+2fMo#N#O", "sceAjmBatchJobRunSplitBufferRa"),
        ("eDFeTyi+G3Y#N#O", "sceAjmDecMp3ParseFrame"),
    ] {
        assert_eq!(crate::abi::nid_name(nid), Some(symbol));
        assert!(EXPORTS.contains(&symbol));
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn every_export_resolves_to_a_distinct_function() {
    let mut addresses: Vec<usize> = EXPORTS.iter().map(|s| export(s).unwrap()).collect();
    addresses.sort_unstable();
    addresses.dedup();
    assert_eq!(addresses.len(), 17);
    assert_eq!(export("sceAjmDecAt9ParseConfigData"), None);
    assert_eq!(resolve(c"dl+4eHSzUu4#N#O"), export("sceAjmInitialize"));
    assert_eq!(resolve(c"unknown"), None);
    // SAFETY: a null name resolves to nothing.
    assert_eq!(unsafe { runtime_ajm_resolve(std::ptr::null()) }, 0);
}
