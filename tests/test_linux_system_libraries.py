"""The Linux package's guard on the Rust faces (packaging/linux/system_libraries.sh): every
library a face names itself must be a system library. Checked against the real `readelf -d` and
`ldd` output of uncharted-app, built in a Debian 13 container as CI's chroot builds it
(fontconfig and udev linked). `ldd` also lists what those system libraries pull in (libpng16,
libbz2 and libbrotli through fontconfig's freetype), which the package must not fail on."""
from paths import ROOT, bash_executable, requires_bash
import subprocess
import unittest

SCRIPT = 'packaging/linux/system_libraries.sh'

APP_READELF = '''
Dynamic section at offset 0x1658f20 contains 32 entries:
  Tag        Type                         Name/Value
 0x0000000000000001 (NEEDED)             Shared library: [libfontconfig.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libudev.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libm.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [ld-linux-x86-64.so.2]
 0x000000000000001e (FLAGS)              BIND_NOW
 0x000000006ffffffb (FLAGS_1)            Flags: NOW PIE
 0x0000000000000015 (DEBUG)              0x0
 0x0000000000000007 (RELA)               0x28b8
 0x0000000000000008 (RELASZ)             807600 (bytes)
 0x0000000000000009 (RELAENT)            24 (bytes)
 0x000000006ffffff9 (RELACOUNT)          33435
 0x0000000000000017 (JMPREL)             0xc7b68
 0x0000000000000002 (PLTRELSZ)           48 (bytes)
 0x0000000000000003 (PLTGOT)             0x1665d30
 0x0000000000000014 (PLTREL)             RELA
 0x0000000000000006 (SYMTAB)             0x340
 0x000000000000000b (SYMENT)             24 (bytes)
 0x0000000000000005 (STRTAB)             0x1ba4
 0x000000000000000a (STRSZ)              3344 (bytes)
 0x000000006ffffef5 (GNU_HASH)           0x1b88
 0x0000000000000019 (INIT_ARRAY)         0x15d4980
 0x000000000000001b (INIT_ARRAYSZ)       24 (bytes)
 0x000000000000001a (FINI_ARRAY)         0x15d4978
 0x000000000000001c (FINI_ARRAYSZ)       8 (bytes)
 0x000000000000000c (INIT)               0x15d3870
 0x000000000000000d (FINI)               0x15d3888
 0x000000006ffffff0 (VERSYM)             0x17b0
 0x000000006ffffffe (VERNEED)            0x1964
 0x000000006fffffff (VERNEEDNUM)         5
 0x0000000000000000 (NULL)               0x0
'''

APP_LDD = '''\
\tlinux-vdso.so.1 (0x00007ffdb72fe000)
\tlibfontconfig.so.1 => /lib/x86_64-linux-gnu/libfontconfig.so.1 (0x00007f34156b3000)
\tlibudev.so.1 => /lib/x86_64-linux-gnu/libudev.so.1 (0x00007f341566e000)
\tlibgcc_s.so.1 => /lib/x86_64-linux-gnu/libgcc_s.so.1 (0x00007f3415641000)
\tlibm.so.6 => /lib/x86_64-linux-gnu/libm.so.6 (0x00007f3415551000)
\tlibc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x00007f341535b000)
\t/lib64/ld-linux-x86-64.so.2 (0x00007f3416d78000)
\tlibfreetype.so.6 => /lib/x86_64-linux-gnu/libfreetype.so.6 (0x00007f341528b000)
\tlibexpat.so.1 => /lib/x86_64-linux-gnu/libexpat.so.1 (0x00007f341525d000)
\tlibcap.so.2 => /lib/x86_64-linux-gnu/libcap.so.2 (0x00007f3415251000)
\tlibz.so.1 => /lib/x86_64-linux-gnu/libz.so.1 (0x00007f3415231000)
\tlibbz2.so.1.0 => /lib/x86_64-linux-gnu/libbz2.so.1.0 (0x00007f341521c000)
\tlibpng16.so.16 => /lib/x86_64-linux-gnu/libpng16.so.16 (0x00007f34151e3000)
\tlibbrotlidec.so.1 => /lib/x86_64-linux-gnu/libbrotlidec.so.1 (0x00007f34151d5000)
\tlibbrotlicommon.so.1 => /lib/x86_64-linux-gnu/libbrotlicommon.so.1 (0x00007f34151b2000)
'''

# uncharted-universal and uncharted-run (same build): the NEEDED entries of `readelf -d`.
CLI_NEEDED = '''\
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [ld-linux-x86-64.so.2]
'''

TRANSITIVE = ['libbz2.so.1.0', 'libpng16.so.16', 'libbrotlidec.so.1', 'libbrotlicommon.so.1']


def bash(script, stdin):
    """Runs `script` after sourcing system_libraries.sh; its output lines."""
    result = subprocess.run([bash_executable(), '-c', f'source {SCRIPT}\n{script}'], cwd=ROOT,
                            input=stdin.encode(), capture_output=True, check=True)
    return result.stdout.decode().split()


def foreign_needed(readelf):
    return bash('foreign_needed', readelf)


def not_system(names):
    """The names the `system` pattern leaves out (what package.sh bundles for bb-probe)."""
    return bash('while read -r name; do [[ $name =~ $system ]] || echo "$name"; done',
                '\n'.join(names) + '\n')


def ldd_names(ldd):
    """The library file names `ldd` resolved, as the guard before this one read them."""
    return [line.split()[0].rsplit('/', 1)[-1] for line in ldd.splitlines() if '=> /' in line]


@requires_bash
class SystemLibrariesTests(unittest.TestCase):
    def test_the_app_needs_only_system_libraries(self):
        self.assertEqual(foreign_needed(APP_READELF), [])

    def test_the_command_line_faces_need_only_system_libraries(self):
        self.assertEqual(foreign_needed(CLI_NEEDED), [])

    def test_libraries_pulled_in_by_system_libraries_are_not_checked(self):
        """ldd lists fontconfig's own dependencies, which the system pattern does not cover:
        a guard over ldd's list failed the package on them."""
        self.assertEqual(not_system(ldd_names(APP_LDD)), TRANSITIVE)
        self.assertEqual(foreign_needed(APP_READELF), [])

    def test_a_library_the_face_names_outside_the_system_list_is_reported(self):
        readelf = APP_READELF.replace(
            ' 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]\n',
            ' 0x0000000000000001 (NEEDED)             Shared library: [libssl.so.3]\n'
            ' 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]\n'
            ' 0x0000000000000001 (NEEDED)             Shared library: [libpng16.so.16]\n')
        self.assertEqual(foreign_needed(readelf), ['libssl.so.3', 'libpng16.so.16'])

    def test_windows_line_ends_are_read_too(self):
        readelf = CLI_NEEDED + ' 0x0000000000000001 (NEEDED)             Shared library: [libssl.so.3]\n'
        self.assertEqual(foreign_needed(readelf.replace('\n', '\r\n')), ['libssl.so.3'])


class PackageScriptTests(unittest.TestCase):
    def test_package_checks_each_face_with_the_guard(self):
        text = (ROOT / 'packaging' / 'linux' / 'package.sh').read_text(encoding='utf-8')
        self.assertIn(f'source {SCRIPT}\n', text)
        self.assertIn('foreign=$(readelf -d "$dest/$face" | foreign_needed)', text)
        self.assertNotIn('\nsystem=', text)


if __name__ == '__main__':
    unittest.main()
