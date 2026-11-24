// SPDX-License-Identifier: GPL-2.0-or-later
#include "bbport_movie.h"
#include "core/libraries/videodec/video_utils.h"
#include "core/libraries/videodec/videodec2_impl.h"
#include "video_core/texture_cache/tile.h"

#include <cassert>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <tuple>
#include <vector>

namespace {

void SetEnv(const char* key, const char* value) {
#ifdef _WIN32
    if (_putenv_s(key, value) != 0) std::abort();
#else
    if (setenv(key, value, 1) != 0) std::abort();
#endif
}

constexpr std::uint32_t Align64(std::uint32_t v) {
    return (v + 63) & ~63u;
}

void TestDecoder1620pNativePath() {
    // Verify CopyNV12Data layout matches Remap native layout.
    AVFrame* src = av_frame_alloc();
    assert(src != nullptr);
    src->format = AV_PIX_FMT_NV12;
    src->width = 1920;
    src->height = 1080;
    assert(av_frame_get_buffer(src, 0) >= 0);
    for (int y = 0; y < src->height; ++y) {
        std::memset(src->data[0] + static_cast<size_t>(y) * src->linesize[0], 110, 1920);
        if (y < src->height / 2) {
            u8* uv = src->data[1] + static_cast<size_t>(y) * src->linesize[1];
            for (int x = 0; x < 960; ++x) {
                uv[x * 2] = 64;
                uv[x * 2 + 1] = 192;
            }
        }
    }
    std::vector<u8> direct_buf(4ull << 20, 0);
    Libraries::Videodec::CopyNV12Data(direct_buf.data(), direct_buf.size(), *src, 16);
    assert(direct_buf[0] == 110);
    assert(direct_buf[1920ull * 1088ull] == 64);
    assert(direct_buf[1920ull * 1088ull + 1] == 192);
    av_frame_free(&src);

    // If FFmpeg has an H.264 encoder available, encode a 1920x1080 YUV420P frame and run it
    // through VdecDecoder at BB_MOVIE_SIZE=2880x1620. This exercises multi-threaded H.264 decode,
    // YUV420P->NV12 conversion, and confirms 16:9 1620p skips CPU scaling / ScratchFrame
    // (runtime_map_guest_memory in test_gpu_runtime_stubs.cpp aborts if called).
    const AVCodec* enc_codec = avcodec_find_encoder(AV_CODEC_ID_H264);
    if (!enc_codec) {
        return;
    }
    AVCodecContext* enc = avcodec_alloc_context3(enc_codec);
    assert(enc != nullptr);
    enc->width = 1920;
    enc->height = 1080;
    enc->time_base = {1, 60};
    enc->framerate = {60, 1};
    enc->pix_fmt = AV_PIX_FMT_YUV420P;
    enc->gop_size = 1;
    enc->max_b_frames = 0;
    if (avcodec_open2(enc, enc_codec, nullptr) < 0) {
        avcodec_free_context(&enc);
        return;
    }

    AVFrame* yuv = av_frame_alloc();
    assert(yuv != nullptr);
    yuv->format = AV_PIX_FMT_YUV420P;
    yuv->width = 1920;
    yuv->height = 1080;
    yuv->pts = 0;
    assert(av_frame_get_buffer(yuv, 0) >= 0);
    for (int y = 0; y < 1080; ++y) {
        std::memset(yuv->data[0] + static_cast<size_t>(y) * yuv->linesize[0], 100, 1920);
        if (y < 540) {
            std::memset(yuv->data[1] + static_cast<size_t>(y) * yuv->linesize[1], 64, 960);
            std::memset(yuv->data[2] + static_cast<size_t>(y) * yuv->linesize[2], 192, 960);
        }
    }

    assert(avcodec_send_frame(enc, yuv) >= 0);
    assert(avcodec_send_frame(enc, nullptr) >= 0);
    AVPacket* pkt = av_packet_alloc();
    assert(pkt != nullptr);
    assert(avcodec_receive_packet(enc, pkt) >= 0);
    assert(pkt->size > 0);

    Libraries::Videodec2::OrbisVideodec2DecoderConfigInfo cfg{};
    cfg.this_size = sizeof(cfg);
    cfg.codec_type = Libraries::Videodec2::OrbisVideodec2CodecType::Avc;
    cfg.max_frame_width = 1920;
    cfg.max_frame_height = 1080;
    Libraries::Videodec2::OrbisVideodec2DecoderMemoryInfo mem{};
    mem.this_size = sizeof(mem);
    Libraries::Videodec2::VdecDecoder decoder(cfg, mem);

    std::vector<u8> guest_fb(4ull << 20, 0);
    Libraries::Videodec2::OrbisVideodec2InputData in{};
    in.this_size = sizeof(in);
    in.au_data = pkt->data;
    in.au_size = static_cast<u64>(pkt->size);
    Libraries::Videodec2::OrbisVideodec2FrameBuffer fb{};
    fb.this_size = sizeof(fb);
    fb.frame_buffer = guest_fb.data();
    fb.frame_buffer_size = guest_fb.size();
    Libraries::Videodec2::OrbisVideodec2OutputInfo out{};
    out.this_size = sizeof(out);

    assert(decoder.Decode(in, fb, out) == 0);
    if (!out.is_valid) {
        assert(decoder.Flush(fb, out) == 0);
    }
    assert(out.is_valid);
    assert(fb.is_accepted);
    assert(out.frame_width == 1920);
    assert(out.frame_pitch == 1920);
    assert(out.frame_height == 1088);

    // Check decoded Y and interleaved NV12 UV values (tolerant of lossy H.264 quantization).
    const u8 y_sample = guest_fb[1920 * 540 + 960];
    const u8 u_sample = guest_fb[1920ull * 1088ull + 1920 * 270 + 960];
    const u8 v_sample = guest_fb[1920ull * 1088ull + 1920 * 270 + 961];
    assert(y_sample >= 90 && y_sample <= 110);
    assert(u_sample >= 54 && u_sample <= 74);
    assert(v_sample >= 182 && v_sample <= 202);

    // Verify BbMovie::Remap maps 2880x1620 / 1440x810 T# descriptors to this native buffer.
    const std::uint64_t fb_addr = reinterpret_cast<std::uint64_t>(guest_fb.data());
    std::uint64_t luma_addr = fb_addr;
    std::uint32_t lw = 2880, lh = 1620, lp = 2880;
    assert(BbMovie::Remap(luma_addr, lw, lh, lp, 8));
    assert(luma_addr == fb_addr && lw == 1920 && lh == 1080 && lp == 1920);

    const std::uint64_t player_chroma_offset = std::uint32_t(2880u * Align64(1620u)) & ~63u;
    std::uint64_t chroma_addr = fb_addr + player_chroma_offset;
    std::uint32_t cw = 1440, ch = 810, cp = 1440;
    assert(BbMovie::Remap(chroma_addr, cw, ch, cp, 16));
    assert(chroma_addr == fb_addr + 1920ull * 1088ull && cw == 960 && ch == 540 && cp == 960);

    av_packet_free(&pkt);
    av_frame_free(&yuv);
    avcodec_free_context(&enc);
}

} // namespace

int main() {
    // Set BB_MOVIE_SIZE=2880x1620 before any BbMovie call caches Display().
    SetEnv("BB_MOVIE_SIZE", "2880x1620");

    std::uint32_t disp_w = 0, disp_h = 0;
    assert(BbMovie::DisplaySize(disp_w, disp_h));
    assert(disp_w == 2880 && disp_h == 1620);

    // 1. LinearAlignedPitch must match VideoCore::ImageSizeLinearAligned across standard 16:9,
    // ultrawide, 16:10, handheld, and non-multiple-of-4-height resolutions.
    struct ResCase {
        std::uint32_t w, h;
    };
    const ResCase cases[] = {
        {1920, 1080}, {2560, 1440}, {2880, 1620}, {3200, 1800}, {3840, 2160},
        {3440, 1440}, {2560, 1600}, {2266, 1488}, {2208, 1840}, {2092, 1080},
        {1920, 1082},
    };
    for (const auto& c : cases) {
        const auto expected_y = std::get<0>(VideoCore::ImageSizeLinearAligned(c.w, c.h, 8, 1));
        const auto got_y = BbMovie::LinearAlignedPitch(c.w, c.h, 8);
        assert(got_y == expected_y);

        const auto expected_uv =
            std::get<0>(VideoCore::ImageSizeLinearAligned(c.w / 2, c.h / 2, 16, 1));
        const auto got_uv = BbMovie::LinearAlignedPitch(c.w / 2, c.h / 2, 16);
        assert(got_uv == expected_uv);
    }

    // 1620p specifically: 1440 * 810 % 128 == 64 != 0, so chroma texel pitch must pad to 1472
    // (2944 bytes), matching ScaledPitch()/ScaledChromaPitch().
    assert(BbMovie::ScaledPitch() == 2880);
    assert(BbMovie::ScaledRows() == 1664);
    assert(BbMovie::ScaledChromaPitch() == 1472 * 2);

    // 2. NeedsCpuScale skips CPU scaling when 16:9 movie fills the display canvas natively on GPU,
    // and enables it only for mismatched aspect ratios needing letterbox/pillarbox bars.
    assert(!BbMovie::NeedsCpuScale(1920, 1080, 1920, 1080));
    assert(!BbMovie::NeedsCpuScale(2560, 1440, 1920, 1080));
    assert(!BbMovie::NeedsCpuScale(2880, 1620, 1920, 1080));
    assert(!BbMovie::NeedsCpuScale(3200, 1800, 1920, 1080));
    assert(!BbMovie::NeedsCpuScale(3840, 2160, 1920, 1080));
    assert(!BbMovie::NeedsCpuScale(1280, 720, 1920, 1080));
    assert(BbMovie::NeedsCpuScale(3440, 1440, 1920, 1080));
    assert(BbMovie::NeedsCpuScale(2560, 1600, 1920, 1080));
    assert(BbMovie::NeedsCpuScale(2266, 1488, 1920, 1080));
    assert(BbMovie::NeedsCpuScale(2208, 1840, 1920, 1080));
    assert(BbMovie::NeedsCpuScale(2092, 1080, 1920, 1080));

    // 3. Remap at 1620p without ScratchFrame points directly to native 1920x1080 / 960x540 planes.
    const std::uint64_t base = 0x100000000ull;
    BbMovie::RecordFrame(base, 1920, 1080, 1920, 1088);

    std::uint64_t luma_addr = base;
    std::uint32_t luma_w = 2880, luma_h = 1620, luma_pitch = 2880;
    assert(BbMovie::Remap(luma_addr, luma_w, luma_h, luma_pitch, 8));
    assert(luma_addr == base);
    assert(luma_w == 1920 && luma_h == 1080 && luma_pitch == 1920);
    assert(std::get<0>(VideoCore::ImageSizeLinearAligned(luma_w, luma_h, 8, 1)) == luma_pitch);

    const std::uint64_t player_chroma_offset = std::uint32_t(2880u * Align64(1620u)) & ~63u;
    std::uint64_t chroma_addr = base + player_chroma_offset;
    std::uint32_t chroma_w = 1440, chroma_h = 810, chroma_pitch = 1440;
    assert(BbMovie::Remap(chroma_addr, chroma_w, chroma_h, chroma_pitch, 16));
    assert(chroma_addr == base + 1920ull * 1088ull);
    assert(chroma_w == 960 && chroma_h == 540 && chroma_pitch == 960);
    assert(std::get<0>(VideoCore::ImageSizeLinearAligned(chroma_w, chroma_h, 16, 1)) ==
           chroma_pitch);

    // 4. Movie shader detection and 1620p UV scale constants.
    const std::uint32_t shader_words[4] = {0x3a088889u, 0x39888889u, 0x3a70f0f1u, 0x39f0f0f1u};
    assert(BbMovie::IsMovieShader(shader_words, 4));
    assert(BbMovie::ShaderKey(0x123456789abcdef0ull, shader_words, 4) != 0x123456789abcdef0ull);

    float patched = 0.0f;
    assert(BbMovie::MovieShaderConstant(0x3a088889u, patched));
    assert(std::fabs(patched - 1.0f / 2880.0f) < 1e-8f);
    assert(BbMovie::MovieShaderConstant(0x39888889u, patched));
    assert(std::fabs(patched - 0.5f / 2880.0f) < 1e-8f);
    assert(BbMovie::MovieShaderConstant(0x3a70f0f1u, patched));
    assert(std::fabs(patched - 1.0f / (1620.0f * 1088.0f / 1080.0f)) < 1e-8f);
    assert(BbMovie::MovieShaderConstant(0x39f0f0f1u, patched));
    assert(std::fabs(patched - 0.5f / (1620.0f * 1088.0f / 1080.0f)) < 1e-8f);

    // 5. End-to-end VdecDecoder + YUV420P->NV12 + CopyNV12Data + Remap at 1620p.
    TestDecoder1620pNativePath();

    return 0;
}
