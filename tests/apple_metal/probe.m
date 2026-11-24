// SPDX-License-Identifier: GPL-2.0-or-later
// Synthetic Metal probe (feasibility plan step 4, #408, #211).
//
// Authored from the Metal documentation. It contains no game data and no code from any peer.
// It creates a device, compiles MSL at run time, renders a textured triangle to an offscreen
// RGBA8 target, uploads an RGBA8 and a BC1 sub-rectangle through a staging buffer (the shape of
// ButtonPrompts::Patch upload), reads each result back and compares a CRC-32 with a value that
// was computed by hand (Python zlib, recorded below), then runs the no-copy MTLBuffer probe over
// an mmap'd scratch file. It prints one JSON record and returns 0 on pass or skip, 1 on fail.
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <TargetConditionals.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
#if TARGET_OS_IPHONE
#include <os/proc.h>
#endif

#ifndef PROBE_TARGET
#define PROBE_TARGET "unknown"
#endif
#ifndef PROBE_CLANG
#define PROBE_CLANG "unknown"
#endif
#ifndef PROBE_XCODE
#define PROBE_XCODE "unknown"
#endif

// Expected CRC-32 values, computed outside this program with Python's zlib.crc32 over the byte
// layout described next to each constant (see RESULTS.md for the generator one-liner).
// 64x64 RGBA8: quadrants TL red, TR green, BL blue, BR yellow, all alpha 255.
static const uint32_t kCrcTriangle = 0x9c3590c2u;
// 8x8 RGBA8, zero except a 4x4 patch at (2,2) with pixel (sx,sy) = {sx*16+1, sy*16+2, 0x80+sx+sy, 255}.
static const uint32_t kCrcRgbaRaw = 0xa63d7215u;
// The same texture sampled into an 8x8 target (identical bytes: nearest, pixel centres).
// 16x16 BC1, 4x4 blocks of 8 bytes. Zero except a 2x2 block patch at (4,4): colour0 red, green,
// blue, white (RGB565 0xF800, 0x07E0, 0x001F, 0xFFFF), colour1 0, indices 0. Raw block bytes (128).
static const uint32_t kCrcBc1Raw = 0x11dddd28u;
// The BC1 texture decoded by sampling into a 16x16 RGBA8 target: black alpha 255, with the four
// 4x4 colour quadrants at (4,4).
static const uint32_t kCrcBc1Decoded = 0xe5dd0b95u;
// The no-copy buffer holds 4 MiB of uint words; the GPU writes word i = i*7+3.
static const uint32_t kNoCopyWords = 1u << 20;  // 4 MiB

static uint32_t crc32_of(const uint8_t *p, size_t n) {
  uint32_t c = 0xFFFFFFFFu;
  for (size_t i = 0; i < n; i++) {
    c ^= p[i];
    for (int k = 0; k < 8; k++) c = (c >> 1) ^ (0xEDB88320u & (0u - (c & 1u)));
  }
  return ~c;
}

static NSMutableArray<NSDictionary *> *gChecks;
static int gFailures = 0;

static void record(NSString *name, BOOL ok, NSString *detail, NSDictionary *extra) {
  NSMutableDictionary *d = [NSMutableDictionary dictionaryWithDictionary:@{
    @"name" : name, @"status" : ok ? @"pass" : @"fail", @"detail" : detail ?: @""}];
  if (extra) [d addEntriesFromDictionary:extra];
  [gChecks addObject:d];
  if (!ok) gFailures++;
  fprintf(stderr, "[%s] %s %s\n", ok ? "pass" : "FAIL", name.UTF8String, detail.UTF8String);
}

static void skip(NSString *name, NSString *why) {
  [gChecks addObject:@{@"name" : name, @"status" : @"skipped", @"detail" : why}];
  fprintf(stderr, "[skip] %s %s\n", name.UTF8String, why.UTF8String);
}

static NSString *hex(uint32_t v) { return [NSString stringWithFormat:@"0x%08x", v]; }

static NSString *const kShader =
    @"#include <metal_stdlib>\n"
     "using namespace metal;\n"
     "struct VOut { float4 pos [[position]]; float2 uv; };\n"
     // One oversized triangle covering the target; uv is 0..1 across it, v down.
     "vertex VOut vs_tri(uint vid [[vertex_id]]) {\n"
     "  float2 p[3] = { float2(-1,-1), float2(3,-1), float2(-1,3) };\n"
     "  float2 t[3] = { float2(0,1), float2(2,1), float2(0,-1) };\n"
     "  VOut o; o.pos = float4(p[vid], 0, 1); o.uv = t[vid]; return o; }\n"
     "fragment float4 fs_tex(VOut in [[stage_in]], texture2d<float> tex [[texture(0)]]) {\n"
     "  constexpr sampler s(filter::nearest, address::clamp_to_edge);\n"
     "  return tex.sample(s, in.uv); }\n"
     "kernel void fill_words(device uint *buf [[buffer(0)]], uint i [[thread_position_in_grid]]) {\n"
     "  buf[i] = i * 7u + 3u; }\n";

typedef struct { id<MTLDevice> dev; id<MTLCommandQueue> q; id<MTLRenderPipelineState> rps; } Ctx;

static id<MTLTexture> make_tex(id<MTLDevice> dev, MTLPixelFormat f, NSUInteger w, NSUInteger h, MTLTextureUsage u) {
  MTLTextureDescriptor *d = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:f width:w height:h mipmapped:NO];
  d.storageMode = MTLStorageModePrivate;
  d.usage = u;
  return [dev newTextureWithDescriptor:d];
}

// Upload `w` x `h` pixels (or blocks*4 for BC) from `bytes` into tex at (x,y) through a staging buffer.
static void upload(Ctx *c, id<MTLTexture> tex, const void *bytes, size_t len, NSUInteger bytesPerRow,
                   NSUInteger x, NSUInteger y, NSUInteger w, NSUInteger h) {
  id<MTLBuffer> stage = [c->dev newBufferWithBytes:bytes length:len options:MTLResourceStorageModeShared];
  id<MTLCommandBuffer> cb = [c->q commandBuffer];
  id<MTLBlitCommandEncoder> b = [cb blitCommandEncoder];
  [b copyFromBuffer:stage sourceOffset:0 sourceBytesPerRow:bytesPerRow sourceBytesPerImage:len
         sourceSize:MTLSizeMake(w, h, 1) toTexture:tex destinationSlice:0 destinationLevel:0
  destinationOrigin:MTLOriginMake(x, y, 0)];
  [b endEncoding];
  [cb commit];
  [cb waitUntilCompleted];
}

// Copy the whole texture into a shared buffer and return its bytes.
static NSData *readback_tex(Ctx *c, id<MTLTexture> tex, NSUInteger bytesPerRow, NSUInteger rows) {
  id<MTLBuffer> out = [c->dev newBufferWithLength:bytesPerRow * rows options:MTLResourceStorageModeShared];
  id<MTLCommandBuffer> cb = [c->q commandBuffer];
  id<MTLBlitCommandEncoder> b = [cb blitCommandEncoder];
  [b copyFromTexture:tex sourceSlice:0 sourceLevel:0 sourceOrigin:MTLOriginMake(0, 0, 0)
          sourceSize:MTLSizeMake(tex.width, tex.height, 1) toBuffer:out destinationOffset:0
   destinationBytesPerRow:bytesPerRow destinationBytesPerImage:bytesPerRow * rows];
  [b endEncoding];
  [cb commit];
  [cb waitUntilCompleted];
  return [NSData dataWithBytes:out.contents length:out.length];
}

// Draw the triangle sampling `src` into a fresh w x h RGBA8 target and return the pixels.
static NSData *render_sample(Ctx *c, id<MTLTexture> src, NSUInteger w, NSUInteger h) {
  id<MTLTexture> rt = make_tex(c->dev, MTLPixelFormatRGBA8Unorm, w, h,
                               MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead);
  MTLRenderPassDescriptor *pd = [MTLRenderPassDescriptor renderPassDescriptor];
  pd.colorAttachments[0].texture = rt;
  pd.colorAttachments[0].loadAction = MTLLoadActionClear;
  pd.colorAttachments[0].clearColor = MTLClearColorMake(0, 0, 0, 0);
  pd.colorAttachments[0].storeAction = MTLStoreActionStore;
  id<MTLCommandBuffer> cb = [c->q commandBuffer];
  id<MTLRenderCommandEncoder> e = [cb renderCommandEncoderWithDescriptor:pd];
  [e setRenderPipelineState:c->rps];
  [e setFragmentTexture:src atIndex:0];
  [e drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
  [e endEncoding];
  [cb commit];
  [cb waitUntilCompleted];
  if (cb.error) fprintf(stderr, "render error: %s\n", cb.error.localizedDescription.UTF8String);
  return readback_tex(c, rt, w * 4, h);
}

static void check_crc(NSString *name, NSData *got, uint32_t want, NSString *what) {
  uint32_t crc = crc32_of(got.bytes, got.length);
  record(name, crc == want, [NSString stringWithFormat:@"%@: got %@ want %@", what, hex(crc), hex(want)],
         @{@"crc32" : hex(crc), @"expected_crc32" : hex(want)});
}

static void test_triangle(Ctx *c) {
  uint8_t texels[16] = {255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255};
  id<MTLTexture> t = make_tex(c->dev, MTLPixelFormatRGBA8Unorm, 2, 2, MTLTextureUsageShaderRead);
  upload(c, t, texels, sizeof texels, 8, 0, 0, 2, 2);
  check_crc(@"triangle_texture_64x64", render_sample(c, t, 64, 64), kCrcTriangle, @"2x2 RGBA8 sampled to 64x64");
}

static void test_rgba_subrect(Ctx *c) {
  uint8_t zero[8 * 8 * 4] = {0};
  uint8_t patch[4 * 4 * 4];
  for (int sy = 0; sy < 4; sy++)
    for (int sx = 0; sx < 4; sx++) {
      uint8_t *p = patch + (sy * 4 + sx) * 4;
      p[0] = sx * 16 + 1; p[1] = sy * 16 + 2; p[2] = 0x80 + sx + sy; p[3] = 255;
    }
  id<MTLTexture> t = make_tex(c->dev, MTLPixelFormatRGBA8Unorm, 8, 8, MTLTextureUsageShaderRead);
  upload(c, t, zero, sizeof zero, 32, 0, 0, 8, 8);
  upload(c, t, patch, sizeof patch, 16, 2, 2, 4, 4);
  check_crc(@"rgba8_subrect_raw", readback_tex(c, t, 32, 8), kCrcRgbaRaw, @"4x4 patch at (2,2) in 8x8, blit readback");
  check_crc(@"rgba8_subrect_sampled", render_sample(c, t, 8, 8), kCrcRgbaRaw, @"same texture sampled to 8x8");
}

static void test_bc1_subrect(Ctx *c) {
  if (@available(macOS 11.0, iOS 16.4, *)) {
    if (!c->dev.supportsBCTextureCompression) {
      skip(@"bc1_subrect", @"device does not support BC texture compression (iOS fallback is RGBA8 patches)");
      return;
    }
  }
  uint8_t zero[128] = {0};
  uint8_t blocks[32] = {0};
  const uint16_t c0[4] = {0xF800, 0x07E0, 0x001F, 0xFFFF};
  for (int i = 0; i < 4; i++) { blocks[i * 8] = c0[i] & 0xFF; blocks[i * 8 + 1] = c0[i] >> 8; }
  id<MTLTexture> t = make_tex(c->dev, MTLPixelFormatBC1_RGBA, 16, 16, MTLTextureUsageShaderRead);
  upload(c, t, zero, sizeof zero, 32, 0, 0, 16, 16);
  // Source rows are rows of blocks: 2 blocks * 8 bytes = 16 bytes per row, 2 block rows.
  upload(c, t, blocks, sizeof blocks, 16, 4, 4, 8, 8);
  check_crc(@"bc1_subrect_raw", readback_tex(c, t, 32, 4), kCrcBc1Raw, @"2x2 block patch at (4,4) in 16x16 BC1, blit readback");
  check_crc(@"bc1_subrect_decoded", render_sample(c, t, 16, 16), kCrcBc1Decoded, @"BC1 decoded by sampling to 16x16");
}

static void test_nocopy(Ctx *c, id<MTLLibrary> lib) {
  size_t len = (size_t)kNoCopyWords * 4;
  size_t page = (size_t)getpagesize();
  if (len % page) { record(@"nocopy_buffer", NO, @"length not page aligned", nil); return; }
  NSString *path = [NSTemporaryDirectory() stringByAppendingPathComponent:
                    [NSString stringWithFormat:@"uu-metal-probe-%d.bin", getpid()]];
  int fd = open(path.fileSystemRepresentation, O_RDWR | O_CREAT | O_TRUNC, 0600);
  if (fd < 0 || ftruncate(fd, (off_t)len) != 0) { record(@"nocopy_buffer", NO, @"scratch file create failed", nil); if (fd >= 0) close(fd); return; }
  void *map = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  close(fd);
  unlink(path.fileSystemRepresentation);
  if (map == MAP_FAILED) { record(@"nocopy_buffer", NO, @"mmap failed", nil); return; }
  memset(map, 0xA5, len);  // authored sentinel; the GPU must overwrite all of it
  NSMutableDictionary *extra = [NSMutableDictionary dictionary];
#if TARGET_OS_IPHONE
  extra[@"os_proc_available_memory_before"] = @((unsigned long long)os_proc_available_memory());
#endif
  id<MTLBuffer> buf = [c->dev newBufferWithBytesNoCopy:map length:len options:MTLResourceStorageModeShared deallocator:nil];
  BOOL ok = NO;
  NSString *detail = @"newBufferWithBytesNoCopy returned nil";
  if (buf && buf.contents == map) {
    NSError *err = nil;
    id<MTLComputePipelineState> ps = [c->dev newComputePipelineStateWithFunction:[lib newFunctionWithName:@"fill_words"] error:&err];
    id<MTLCommandBuffer> cb = [c->q commandBuffer];
    id<MTLComputeCommandEncoder> e = [cb computeCommandEncoder];
    [e setComputePipelineState:ps];
    [e setBuffer:buf offset:0 atIndex:0];
    NSUInteger tg = MIN(ps.maxTotalThreadsPerThreadgroup, (NSUInteger)256);
    [e dispatchThreads:MTLSizeMake(kNoCopyWords, 1, 1) threadsPerThreadgroup:MTLSizeMake(tg, 1, 1)];
    [e endEncoding];
    [cb commit];
    [cb waitUntilCompleted];
    // Hand-written check: word i must equal i*7+3, read straight from the mapping.
    const uint32_t *w = (const uint32_t *)map;
    uint32_t bad = 0;
    for (uint32_t i = 0; i < kNoCopyWords; i++) if (w[i] != i * 7u + 3u) bad++;
    ok = (bad == 0) && !cb.error;
    detail = [NSString stringWithFormat:@"%u MiB file-backed no-copy buffer, GPU wrote %u words, %u mismatches, contents==mapping",
              (unsigned)(len >> 20), kNoCopyWords, bad];
  } else if (buf) {
    detail = @"buffer contents pointer differs from the mapping (a copy was made)";
  }
#if TARGET_OS_IPHONE
  extra[@"os_proc_available_memory_after"] = @((unsigned long long)os_proc_available_memory());
#endif
  record(@"nocopy_buffer", ok, detail, extra);
  buf = nil;
  munmap(map, len);
}

static NSString *sh_version(void) { return [NSString stringWithUTF8String:PROBE_CLANG]; }

int main(int argc, char **argv) {
  @autoreleasepool {
    gChecks = [NSMutableArray array];
    NSMutableDictionary *rec = [NSMutableDictionary dictionary];
    NSProcessInfo *pi = NSProcessInfo.processInfo;
    rec[@"schema"] = @"uu-metal-probe/1";
    rec[@"target"] = @PROBE_TARGET;
    rec[@"os"] = pi.operatingSystemVersionString;
    rec[@"xcode"] = @PROBE_XCODE;
    rec[@"clang"] = sh_version();
    rec[@"page_size"] = @(getpagesize());
    id<MTLDevice> dev = MTLCreateSystemDefaultDevice();
    NSString *result;
    if (!dev) {
      rec[@"device_name"] = [NSNull null];
      rec[@"result"] = result = @"skipped";
      rec[@"reason"] = @"MTLCreateSystemDefaultDevice returned nil: no Metal device on this host";
      fprintf(stderr, "[skip] no Metal device on this host\n");
    } else {
      rec[@"device_name"] = dev.name;
      rec[@"unified_memory"] = @(dev.hasUnifiedMemory);
      if (@available(macOS 11.0, iOS 16.4, *)) rec[@"supports_bc"] = @(dev.supportsBCTextureCompression);
      NSError *err = nil;
      id<MTLLibrary> lib = [dev newLibraryWithSource:kShader options:nil error:&err];
      Ctx c = {dev, [dev newCommandQueue], nil};
      if (!lib || !c.q) {
        record(@"setup", NO, [NSString stringWithFormat:@"shader compile or queue failed: %@", err.localizedDescription], nil);
      } else {
        MTLRenderPipelineDescriptor *rd = [MTLRenderPipelineDescriptor new];
        rd.vertexFunction = [lib newFunctionWithName:@"vs_tri"];
        rd.fragmentFunction = [lib newFunctionWithName:@"fs_tex"];
        rd.colorAttachments[0].pixelFormat = MTLPixelFormatRGBA8Unorm;
        c.rps = [dev newRenderPipelineStateWithDescriptor:rd error:&err];
        if (!c.rps) {
          record(@"setup", NO, [NSString stringWithFormat:@"pipeline failed: %@", err.localizedDescription], nil);
        } else {
          record(@"setup", YES, @"device, runtime MSL compile, pipeline", nil);
          test_triangle(&c);
          test_rgba_subrect(&c);
          test_bc1_subrect(&c);
          test_nocopy(&c, lib);
        }
      }
      rec[@"result"] = result = gFailures ? @"fail" : @"pass";
    }
    rec[@"checks"] = gChecks;
    NSData *json = [NSJSONSerialization dataWithJSONObject:rec options:NSJSONWritingPrettyPrinted error:nil];
    fwrite(json.bytes, 1, json.length, stdout);
    fputc('\n', stdout);
    if (argc > 1) [json writeToFile:[NSString stringWithUTF8String:argv[1]] atomically:YES];
    return [result isEqualToString:@"fail"] ? 1 : 0;
  }
}
