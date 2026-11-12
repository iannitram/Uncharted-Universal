"""Boot smoke test of the loader with crates/runtime linked in (piece 3.01).

A synthetic x86-64 image (no game bytes) boots the way the eboot starts: it calls the libc
contracts that are Rust now (`_init_env`, `atexit` with a handler of its own) and ends through
libc `exit(0)`. Passing means the loader started, its ABI version check agreed with the static
library, imports resolved through the Rust `runtime_resolve`, the guest's exit ran the handlers
and the report, and the process ended through the loader's `runtime_quit` with the guest's status.
CI runs it on Linux after `bash build.sh --test` (.github/workflows/build.yml, "Boot smoke test").
"""
from paths import ROOT
from pathlib import Path
import os
import struct
import subprocess
import tempfile
import unittest

EXE = ROOT / ('out/bb-probe' + ('.exe' if os.name == 'nt' else ''))

INIT_ENV = 'bzQExy189ZI#q#q'
ATEXIT = '8G2LB+A3rzg#q#q'
EXIT = 'uMei1W9uyNo#q#q'


def package(code, relocs, names, capabilities):
    """The BBPROBE2 container of tests/test_probe.py: one r-x segment at 0, entry at 0."""
    image = code.ljust(4096, b'\0')
    header = struct.pack('<8sQQQQQ', b'BBPROBE2', len(image), 0, 1, len(relocs), len(names))
    header += struct.pack('<Q', capabilities)
    segment = struct.pack('<QQQ', 0, len(image), 5)
    return (header + segment + b''.join(n.encode().ljust(128, b'\0') for n in names)
            + b''.join(struct.pack('<QQqq', *r) for r in relocs) + image)


def boot_image(capabilities=1):
    # Import slots at 40 (_init_env), 48 (atexit), 56 (exit); the exit handler (ret) at 64.
    code = (b'\x48\x83\xec\x08'              # sub rsp, 8          (call alignment)
            b'\xff\x15\x1e\0\0\0'            # call [rip+30]       _init_env
            b'\x48\x8d\x3d\x2f\0\0\0'        # lea rdi, [rip+47]   the handler at 64
            b'\xff\x15\x19\0\0\0'            # call [rip+25]       atexit(handler)
            b'\x31\xff'                      # xor edi, edi
            b'\xff\x15\x19\0\0\0'            # call [rip+25]       exit(0)
            b'\x0f\x0b')                     # ud2                 (exit does not return)
    assert len(code) <= 40
    code = code.ljust(64, b'\0') + b'\xc3'
    relocs = [(40, 1, 0, 0), (48, 1, 1, 0), (56, 1, 2, 0)]
    return package(code, relocs, [INIT_ENV, ATEXIT, EXIT], capabilities)


@unittest.skipUnless(EXE.exists(), 'build native probe first')
class BootSmokeTests(unittest.TestCase):
    def run_image(self, data, *options):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'boot.bin'
            path.write_bytes(data)
            return subprocess.run([str(EXE.resolve()), str(path), '--cpu-only', *options],
                                  capture_output=True, text=True, timeout=10)

    def test_guest_boots_through_the_rust_runtime_and_exits_cleanly(self):
        r = self.run_image(boot_image())
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertNotIn('ABI version', r.stderr)
        self.assertIn('Runtime: _init_env returned', r.stdout)
        self.assertIn('Runtime: guest requested exit(0)', r.stdout)
        self.assertIn('Runtime: _init_env=1, atexit=1', r.stdout)
        self.assertNotIn('STOP:', r.stdout)

    def test_without_the_runtime_capability_the_first_import_stops(self):
        r = self.run_image(boot_image(capabilities=0))
        self.assertEqual(r.returncode, 20, r.stdout + r.stderr)
        self.assertIn('first unsupported PS4 import: ' + INIT_ENV, r.stdout)


if __name__ == '__main__':
    unittest.main()
