#!/usr/bin/env python3
"""Regenerates the expected CRC-32 constants in probe.m from the byte layouts described there.

Run: python3 expected_crc.py  (prints the four constants; they must match probe.m).
"""
import struct
import zlib


def triangle():
    # 64x64 RGBA8, quadrants TL red, TR green, BL blue, BR yellow, alpha 255.
    out = bytearray()
    for y in range(64):
        for x in range(64):
            top, left = y < 32, x < 32
            rgb = (255, 0, 0) if top and left else (0, 255, 0) if top else (0, 0, 255) if left else (255, 255, 0)
            out += bytes(rgb + (255,))
    return bytes(out)


def rgba_raw():
    # 8x8 RGBA8, zero except a 4x4 patch at (2,2): pixel (sx,sy) = {sx*16+1, sy*16+2, 0x80+sx+sy, 255}.
    out = bytearray(8 * 8 * 4)
    for sy in range(4):
        for sx in range(4):
            o = ((2 + sy) * 8 + 2 + sx) * 4
            out[o:o + 4] = bytes((sx * 16 + 1, sy * 16 + 2, 0x80 + sx + sy, 255))
    return bytes(out)


COLORS = (0xF800, 0x07E0, 0x001F, 0xFFFF)  # red, green, blue, white (RGB565)


def bc1_raw():
    # 16x16 BC1 = 4x4 blocks of 8 bytes (colour0 u16 LE, colour1 u16 LE, indices u32 LE).
    # Zero except a 2x2 block patch at pixel (4,4) = block (1,1), row-major.
    out = bytearray(16 * 8)
    for i, c in enumerate(COLORS):
        bx, by = 1 + i % 2, 1 + i // 2
        o = (by * 4 + bx) * 8
        out[o:o + 8] = struct.pack("<HHI", c, 0, 0)
    return bytes(out)


def bc1_decoded():
    # 16x16 RGBA8: black alpha 255, with the four 4x4 colour quadrants at (4,4).
    rgb = ((255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 255))
    out = bytearray()
    for y in range(16):
        for x in range(16):
            px = (0, 0, 0)
            if 4 <= x < 12 and 4 <= y < 12:
                px = rgb[((y - 4) // 4) * 2 + (x - 4) // 4]
            out += bytes(px + (255,))
    return bytes(out)


for name, fn in (("kCrcTriangle", triangle), ("kCrcRgbaRaw", rgba_raw),
                 ("kCrcBc1Raw", bc1_raw), ("kCrcBc1Decoded", bc1_decoded)):
    print(f"{name} = 0x{zlib.crc32(fn()):08x}u")
