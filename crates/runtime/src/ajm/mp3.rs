//! MP3 for libSceAjm: the frame header parser of `sceAjmDecMp3ParseFrame` (tables as in the AJM
//! library, plus Sony's "FGH" gapless block) and the decoder behind MP3 instances (U3's London
//! streets create one).
//!
//! The C runtime decoded with FFmpeg's libavcodec. Here it is Symphonia's pure-Rust MP3 decoder
//! (MPL-2.0, which allows combining with GPL-2.0 code), so AJM no longer needs FFmpeg (#299).
//! Both hand back one frame of float PCM per MP3 frame, with no decoder delay of their own.

use symphonia_bundle_mp3::MpaDecoder;
use symphonia_core::audio::{AudioBuffer, AudioBufferRef, Signal};
use symphonia_core::codecs::{CodecParameters, Decoder as _, DecoderOptions, CODEC_TYPE_MP3};
use symphonia_core::formats::Packet;

use super::ERR_INVALID_PARAMETER;

/// `SceAjmDecMp3ParseFrame` (40 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) struct Mp3Frame {
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

const _: () = assert!(std::mem::size_of::<Mp3Frame>() == 40);

/// Sample rates by version (2.5, reserved, 2, 1) and rate index.
const RATES: [[u32; 4]; 4] = [
    [11025, 12000, 8000, 0],
    [0, 0, 0, 0],
    [22050, 24000, 16000, 0],
    [44100, 48000, 32000, 0],
];
/// Layer III bitrates in kbit/s by version and bitrate index.
const KBPS: [[u32; 16]; 4] = [
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 0, 0, 0, 0, 0, 0, 0],
    [0; 16],
    [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ],
    [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ],
];

/// `count` bits of `data` from bit `*offset` on, most significant first.
fn read_bits(data: &[u8], offset: &mut usize, count: u32) -> u32 {
    let mut value = 0;
    for _ in 0..count {
        value = value << 1 | u32::from((data[*offset / 8] >> (7 - *offset % 8)) & 1);
        *offset += 1;
    }
    value
}

/// The FGH block's check byte over its first nine bytes, as the AJM library computes it.
fn fgh_crc(block: &[u8]) -> u8 {
    let mut crc: u8 = 0xff;
    for &byte in &block[..9] {
        for bit in (0..8).rev() {
            let shifted = crc.wrapping_mul(2);
            crc = if (crc >> 7) & 1 != (byte >> bit) & 1 {
                shifted
            } else {
                shifted ^ 0x45
            };
        }
    }
    crc
}

/// The header of the Layer III frame at the start of `data` into `frame` (0, or
/// `ERR_INVALID_PARAMETER` when `data` does not start with one). With `parse_ofl`, also Sony's
/// FGH block (encoder delay, total samples) that follows the side information of the first,
/// empty frame. As in the C runtime, the sample rate and bitrate are written even when one of
/// them is invalid.
pub(super) fn parse_frame(data: &[u8], parse_ofl: bool, frame: &mut Mp3Frame) -> i32 {
    if data.len() < 4 {
        return ERR_INVALID_PARAMETER;
    }
    let h = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    if h >> 21 != 0x7ff || (h >> 17) & 3 != 1 {
        return ERR_INVALID_PARAMETER; // sync word, Layer III
    }
    let version = ((h >> 19) & 3) as usize;
    let protection = (h >> 16) & 1;
    let bitrate_index = ((h >> 12) & 15) as usize;
    let rate_index = ((h >> 10) & 3) as usize;
    let padding = u64::from((h >> 9) & 1);
    let mono = (h >> 6) & 3 == 3;
    frame.sample_rate = RATES[version][rate_index];
    frame.bitrate = KBPS[version][bitrate_index] * 1000;
    if frame.sample_rate == 0 || frame.bitrate == 0 {
        return ERR_INVALID_PARAMETER;
    }
    frame.num_channels = if mono { 1 } else { 2 };
    frame.samples_per_channel = if version == 3 { 1152 } else { 576 };
    let per_kbit: u64 = if version == 3 { 144 } else { 72 };
    frame.frame_size = per_kbit * u64::from(frame.bitrate) / u64::from(frame.sample_rate) + padding;
    frame.encoder_delay = 0;
    frame.num_frames = 0;
    frame.total_samples = 0;
    frame.ofl_type = 0;
    if !parse_ofl || frame.frame_size > data.len() as u64 {
        return 0;
    }
    // Offsets from the end of the header; `end` is the frame's length after it.
    let body = &data[4..];
    let end = frame.frame_size as usize - 4;
    let mut bit: usize = if protection != 0 { 0 } else { 16 };
    bit += match (version == 3, mono) {
        (true, true) => 18,
        (true, false) => 20,
        (false, true) => 9,
        (false, false) => 10,
    };
    let granules = if version == 3 { 2 } else { 1 };
    let mut part23: usize = 0;
    for _ in 0..granules {
        for _ in 0..frame.num_channels {
            if (bit + 12) / 8 >= end {
                return 0;
            }
            part23 += read_bits(body, &mut bit, 12) as usize;
            bit += if version == 3 { 47 } else { 51 };
        }
    }
    bit += part23;
    let mut p = bit.div_ceil(8);
    if p + 10 >= end {
        return 0;
    }
    if [&b"Xing"[..], b"Info", b"VBRI"].contains(&&body[p..p + 4]) {
        return 0;
    }
    while p + 9 < end && body[p] != 0xB4 {
        p += 1;
    }
    if p + 9 >= end {
        return 0;
    }
    let block = &body[p..p + 10];
    if block[9] != fgh_crc(block) {
        return 0;
    }
    frame.encoder_delay = u32::from(block[1]) << 8 | u32::from(block[2]);
    frame.total_samples = u32::from_be_bytes([block[3], block[4], block[5], block[6]]);
    frame.ofl_type = 3; // Fgh
    0
}

/// The decoder of one MP3 instance.
pub(super) struct Decoder {
    inner: MpaDecoder,
    /// Sample rate and channels of the last frame decoded: Symphonia keeps one output layout per
    /// decoder, so a stream that changes either starts a new decoder (FFmpeg followed it).
    layout: Option<(u32, u32)>,
}

impl Decoder {
    pub fn new() -> Self {
        let mut params = CodecParameters::new();
        params.for_codec(CODEC_TYPE_MP3);
        let inner = MpaDecoder::try_new(&params, &DecoderOptions::default())
            .expect("Symphonia decodes MP3");
        Decoder {
            inner,
            layout: None,
        }
    }

    /// Forgets the bit reservoir and any partial state (`avcodec_flush_buffers`).
    pub fn reset(&mut self) {
        *self = Decoder::new();
    }

    /// One whole frame (`packet` is exactly its `frame_size` bytes, header included) whose header
    /// [`parse_frame`] read as `header`. `None` for a damaged frame.
    pub fn decode(&mut self, packet: &[u8], header: &Mp3Frame) -> Option<&AudioBuffer<f32>> {
        let layout = (header.sample_rate, header.num_channels);
        if self.layout.is_some_and(|last| last != layout) {
            self.reset();
        }
        self.layout = Some(layout);
        match self.inner.decode(&Packet::new_from_slice(0, 0, 0, packet)) {
            Ok(AudioBufferRef::F32(buffer)) => Some(buffer.into_owned_ref()),
            _ => None,
        }
    }
}

/// `Cow::Borrowed` only: Symphonia's MP3 decoder lends its own buffer.
trait IntoOwnedRef<'a> {
    fn into_owned_ref(self) -> &'a AudioBuffer<f32>;
}

impl<'a> IntoOwnedRef<'a> for std::borrow::Cow<'a, AudioBuffer<f32>> {
    fn into_owned_ref(self) -> &'a AudioBuffer<f32> {
        match self {
            std::borrow::Cow::Borrowed(buffer) => buffer,
            std::borrow::Cow::Owned(_) => unreachable!("Symphonia's MP3 decoder lends its buffer"),
        }
    }
}

/// Channels and frames of a decoded buffer, and one sample.
pub(super) fn channels(buffer: &AudioBuffer<f32>) -> usize {
    buffer.spec().channels.count()
}

pub(super) fn frames(buffer: &AudioBuffer<f32>) -> usize {
    buffer.frames()
}

pub(super) fn sample(buffer: &AudioBuffer<f32>, channel: usize, index: usize) -> f32 {
    buffer.chan(channel)[index]
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// MPEG-1 Layer III, no CRC, 128 kbit/s, 44.1 kHz, joint stereo: 417 bytes.
    pub const HEADER_128K: [u8; 4] = [0xFF, 0xFB, 0x90, 0x64];

    /// A whole frame with this header whose side information and main data are all zero: a valid
    /// frame of silence.
    pub fn silent_frame(header: [u8; 4]) -> Vec<u8> {
        let mut frame = Mp3Frame::default();
        assert_eq!(parse_frame(&header, false, &mut frame), 0);
        let mut bytes = vec![0u8; frame.frame_size as usize];
        bytes[..4].copy_from_slice(&header);
        bytes
    }

    #[test]
    fn headers_parse_as_the_ajm_library_reads_them() {
        let mut frame = Mp3Frame::default();
        assert_eq!(parse_frame(&HEADER_128K, false, &mut frame), 0);
        assert_eq!(
            frame,
            Mp3Frame {
                frame_size: 417,
                num_channels: 2,
                samples_per_channel: 1152,
                bitrate: 128_000,
                sample_rate: 44100,
                ..Mp3Frame::default()
            }
        );
        // Padding adds a byte; mono is channel mode 3.
        assert_eq!(parse_frame(&[0xFF, 0xFB, 0x92, 0xC4], false, &mut frame), 0);
        assert_eq!((frame.frame_size, frame.num_channels), (418, 1));
        // MPEG-2, 64 kbit/s, 22.05 kHz: 576 samples, 72 * 64000 / 22050 bytes.
        assert_eq!(parse_frame(&[0xFF, 0xF3, 0x80, 0x00], false, &mut frame), 0);
        assert_eq!(
            (
                frame.samples_per_channel,
                frame.sample_rate,
                frame.frame_size
            ),
            (576, 22050, 208)
        );
        // MPEG-2.5, 8 kHz, 8 kbit/s.
        assert_eq!(parse_frame(&[0xFF, 0xE3, 0x18, 0x00], false, &mut frame), 0);
        assert_eq!((frame.sample_rate, frame.bitrate), (8000, 8000));
    }

    #[test]
    fn invalid_headers_are_rejected() {
        let mut frame = Mp3Frame::default();
        let headers: [&[u8]; 7] = [
            &[0xFF, 0xFB, 0x90],       // too short
            &[0xFF, 0x7B, 0x90, 0x64], // no sync word
            &[0xFF, 0xFD, 0x90, 0x64], // Layer II
            &[0xFF, 0xFB, 0x08, 0x64], // free format (bitrate 0)
            &[0xFF, 0xFB, 0xF0, 0x64], // bitrate index 15
            &[0xFF, 0xFB, 0x9C, 0x64], // rate index 3
            &[0xFF, 0xEB, 0x90, 0x64], // reserved version
        ];
        for header in headers {
            assert_eq!(
                parse_frame(header, false, &mut frame),
                ERR_INVALID_PARAMETER
            );
        }
        // The sample rate and bitrate are written before the check, as in C.
        frame = Mp3Frame::default();
        parse_frame(&[0xFF, 0xFB, 0x08, 0x64], false, &mut frame);
        assert_eq!((frame.sample_rate, frame.bitrate), (32000, 0));
    }

    /// A silent first frame carrying an FGH block right after its (empty) side information.
    fn fgh_frame(delay: u16, total: u32, good_crc: bool) -> Vec<u8> {
        let mut bytes = silent_frame(HEADER_128K);
        // No CRC, stereo MPEG-1: 32 bytes of side information, all part2_3 lengths zero.
        let at = 4 + 32;
        let mut block = [0u8; 10];
        block[0] = 0xB4;
        block[1..3].copy_from_slice(&delay.to_be_bytes());
        block[3..7].copy_from_slice(&total.to_be_bytes());
        block[9] = fgh_crc(&block) ^ u8::from(!good_crc);
        bytes[at..at + 10].copy_from_slice(&block);
        bytes
    }

    #[test]
    fn the_fgh_block_carries_delay_and_total_samples() {
        let mut frame = Mp3Frame::default();
        let bytes = fgh_frame(1105, 441_000, true);
        assert_eq!(parse_frame(&bytes, true, &mut frame), 0);
        assert_eq!(
            (frame.encoder_delay, frame.total_samples, frame.ofl_type),
            (1105, 441_000, 3)
        );
        // Without parse_ofl, or with a bad check byte, or a frame cut short: header only.
        assert_eq!(parse_frame(&bytes, false, &mut frame), 0);
        assert_eq!(frame.ofl_type, 0);
        assert_eq!(
            parse_frame(&fgh_frame(1105, 441_000, false), true, &mut frame),
            0
        );
        assert_eq!((frame.ofl_type, frame.encoder_delay), (0, 0));
        assert_eq!(parse_frame(&bytes[..300], true, &mut frame), 0);
        assert_eq!((frame.frame_size, frame.ofl_type), (417, 0));
        // A Xing (or Info, VBRI) frame is not an FGH frame.
        let mut xing = silent_frame(HEADER_128K);
        xing[36..40].copy_from_slice(b"Xing");
        xing[50] = 0xB4;
        assert_eq!(parse_frame(&xing, true, &mut frame), 0);
        assert_eq!(frame.ofl_type, 0);
    }

    #[test]
    fn a_silent_frame_decodes_to_one_frame_of_silence() {
        let mut decoder = Decoder::new();
        let mut header = Mp3Frame::default();
        let bytes = silent_frame(HEADER_128K);
        parse_frame(&bytes, false, &mut header);
        let buffer = decoder.decode(&bytes, &header).expect("a valid frame");
        assert_eq!((channels(buffer), buffer.frames()), (2, 1152));
        assert!((0..1152).all(|i| sample(buffer, 0, i) == 0.0 && sample(buffer, 1, i) == 0.0));
        // A frame cut short is damaged; the decoder goes on with the next one.
        assert!(decoder.decode(&bytes[..200], &header).is_none());
        // A mono frame after stereo ones starts a new decoder instead of failing.
        let mono = silent_frame([0xFF, 0xFB, 0x90, 0xC4]);
        parse_frame(&mono, false, &mut header);
        let buffer = decoder.decode(&mono, &header).expect("a valid mono frame");
        assert_eq!((channels(buffer), buffer.frames()), (1, 1152));
    }
}
