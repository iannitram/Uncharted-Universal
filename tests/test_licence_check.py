"""The packaging licence rule (packaging/licence_check.py): fake inputs, no MSYS2 or Debian."""
from paths import ROOT
import importlib.util
import unittest

spec = importlib.util.spec_from_file_location('licence_check', ROOT / 'packaging' / 'licence_check.py')
licence_check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(licence_check)


class CheckLicencesTest(unittest.TestCase):
    def test_all_covered_is_clean(self):
        entries = [('SDL3.dll', 'mingw-w64-clang-x86_64-SDL3', ['LICENSE.txt']),
                   ('avcodec-61.dll', 'mingw-w64-clang-x86_64-ffmpeg', ['COPYING.GPLv3', 'COPYING.LGPLv3'])]
        self.assertEqual(licence_check.check_licences(entries), [])

    def test_no_owner_names_the_library(self):
        problems = licence_check.check_licences([('mystery.dll', None, [])])
        self.assertEqual(len(problems), 1)
        self.assertIn('mystery.dll', problems[0])

    def test_no_licence_files_names_library_and_package(self):
        problems = licence_check.check_licences([('libx265.dll', 'x265', [])])
        self.assertEqual(len(problems), 1)
        self.assertIn('libx265.dll', problems[0])
        self.assertIn('x265', problems[0])

    def test_every_problem_is_reported(self):
        entries = [('a.dll', None, []), ('b.dll', 'b', ['LICENSE']), ('c.dll', 'c', [])]
        problems = licence_check.check_licences(entries)
        self.assertEqual(len(problems), 2)
        self.assertIn('a.dll', problems[0])
        self.assertIn('c.dll', problems[1])


PACMAN_LISTING = '''mingw-w64-clang-x86_64-sdl3 /clang64/
mingw-w64-clang-x86_64-sdl3 /clang64/bin/SDL3.dll
mingw-w64-clang-x86_64-sdl3 /clang64/share/licenses/
mingw-w64-clang-x86_64-sdl3 /clang64/share/licenses/SDL3/
mingw-w64-clang-x86_64-sdl3 /clang64/share/licenses/SDL3/LICENSE.txt
'''


class LookupTest(unittest.TestCase):
    def test_licence_paths_ignores_folders_and_other_files(self):
        self.assertEqual(licence_check.licence_paths(PACMAN_LISTING), ['SDL3/LICENSE.txt'])

    def test_licence_paths_folder_name_differs_from_package(self):
        listing = 'pkg /clang64/share/licenses/libc++/LICENSE\npkg /clang64/share/licenses/libc++/NOTICE\n'
        self.assertEqual(licence_check.licence_paths(listing), ['libc++/LICENSE', 'libc++/NOTICE'])

    def test_licence_paths_none_listed(self):
        self.assertEqual(licence_check.licence_paths('ffmpeg /clang64/bin/avcodec-63.dll\n'), [])
        self.assertEqual(licence_check.licence_paths(''), [])

    def test_blank_library_names_are_skipped(self):
        import subprocess
        from unittest import mock
        done = subprocess.CompletedProcess([], 1, stdout='')
        with mock.patch.object(licence_check.subprocess, 'run', return_value=done) as run:
            self.assertEqual(licence_check.run('windows', 'unused', ['', '']), 0)
            run.assert_not_called()

    def test_short_name(self):
        self.assertEqual(licence_check.short_name('mingw-w64-clang-x86_64-ffmpeg'), 'ffmpeg')
        self.assertEqual(licence_check.short_name('other'), 'other')

    def test_fallback_used_when_pacman_lists_nothing(self):
        import subprocess, tempfile
        from pathlib import Path
        from unittest import mock
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / 'ffmpeg-LICENSE.txt').write_text('text')
            done = subprocess.CompletedProcess([], 0, stdout='ffmpeg /clang64/bin/x.dll\n')
            with mock.patch.object(licence_check, 'FALLBACK_DIR', Path(tmp)),                     mock.patch.object(licence_check.subprocess, 'run', return_value=done):
                found = licence_check._windows_files('mingw-w64-clang-x86_64-ffmpeg')
                self.assertEqual([t for _, t in found], [Path('ffmpeg-LICENSE.txt')])
                self.assertEqual(licence_check._windows_files('mingw-w64-clang-x86_64-nothing'), [])

    def test_pacman_files_win_over_fallback(self):
        import subprocess, tempfile
        from pathlib import Path
        from unittest import mock
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / 'SDL3').mkdir()
            (Path(tmp) / 'SDL3' / 'LICENSE.txt').write_text('zlib')
            done = subprocess.CompletedProcess([], 0, stdout=PACMAN_LISTING)
            with mock.patch.object(licence_check, 'MSYS2_LICENSES', Path(tmp)),                     mock.patch.object(licence_check, 'FALLBACK_DIR', Path(tmp) / 'none'),                     mock.patch.object(licence_check.subprocess, 'run', return_value=done):
                found = licence_check._windows_files('mingw-w64-clang-x86_64-sdl3')
                self.assertEqual(found, [(Path(tmp) / 'SDL3' / 'LICENSE.txt', Path('LICENSE.txt'))])

    def test_ffmpeg_gpl3_is_rejected_and_gpl2_is_accepted(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as tmp:
            gpl3 = Path(tmp) / 'COPYING.txt'
            gpl3.write_text('GNU GENERAL PUBLIC LICENSE\nVersion 3, 29 June 2007\n')
            gpl2 = Path(tmp) / 'COPYING.GPLv2'
            gpl2.write_text('GNU GENERAL PUBLIC LICENSE\nVersion 2, June 1991\n')
            bad = [('avcodec-61.dll', 'ffmpeg', [gpl3]), ('SDL3.dll', 'sdl3', [gpl3])]
            problems = licence_check.check_ffmpeg_compatibility(bad)
            self.assertEqual(len(problems), 1)
            self.assertIn('avcodec-61.dll', problems[0])
            good = [('avcodec-61.dll', 'ffmpeg', [gpl2]), ('libavutil.so.59', 'ffmpeg', [gpl2])]
            self.assertEqual(licence_check.check_ffmpeg_compatibility(good), [])


class AppleTreeTest(unittest.TestCase):
    """check_apple_tree: the tree vendored for the Apple build (ADR 0006, #396)."""

    def tree(self, files):
        import tempfile
        from pathlib import Path
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name) / 'apple'
        for name, text in files.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        return root

    GOOD = {'vendor/bbcpu/VENDOR.txt': 'Vendored from x\n', 'vendor/bbcpu/LICENSE': 'GNU GENERAL PUBLIC LICENSE Version 2',
            'vendor/bbcpu/src/cpu.c': '/* SPDX-License-Identifier: GPL-2.0-or-later */\nint x;\n'}

    def test_the_real_apple_tree_is_allowed(self):
        self.assertEqual(licence_check.check_apple_tree(ROOT / 'apple'), [])

    def test_a_clean_tree_passes(self):
        self.assertEqual(licence_check.check_apple_tree(self.tree(self.GOOD)), [])

    def test_a_bbnet20_path_fails(self):
        for name in ('vendor/bbcpu/bbnet20/a.c', 'vendor/bbnet20/VENDOR.txt', 'vendor/bbcpu/BBNET20.h'):
            problems = licence_check.check_apple_tree(self.tree({**self.GOOD, name: 'int y;\n'}))
            self.assertTrue(any('bbnet20' in p.lower() for p in problems), (name, problems))

    def test_a_gpl3_file_name_fails(self):
        for name in ('vendor/bbcpu/LICENSE.GPL-3.0', 'vendor/bbcpu/COPYING.GPLv3', 'vendor/bbcpu/AGPL-3.0.txt'):
            problems = licence_check.check_apple_tree(self.tree({**self.GOOD, name: 'text'}))
            self.assertEqual(len(problems), 1, (name, problems))
            self.assertIn('GPL-3.0 or bbnet20 path', problems[0])

    def test_gpl3_and_apache_text_fails_even_in_an_innocent_name(self):
        for text in ('/* SPDX-License-Identifier: GPL-3.0-or-later */', '/* SPDX-License-Identifier: Apache-2.0 */',
                     'GNU GENERAL PUBLIC LICENSE\n                       Version 3, 29 June 2007',
                     'uses bbnet20 code'):
            problems = licence_check.check_apple_tree(self.tree({**self.GOOD, 'vendor/bbcpu/src/net.c': text}))
            self.assertEqual(len(problems), 1, (text, problems))
            self.assertIn('src/net.c', problems[0])

    def test_a_vendor_file_may_name_what_it_leaves_out(self):
        files = {**self.GOOD, 'vendor/bbcpu/VENDOR.txt': 'Not vendored: gpu/bbnet20 (GPL-3.0).\n'}
        self.assertEqual(licence_check.check_apple_tree(self.tree(files)), [])

    def test_a_component_needs_vendor_txt_and_a_licence(self):
        problems = licence_check.check_apple_tree(self.tree({'vendor/zydis/src/a.c': 'int a;\n'}))
        self.assertEqual(len(problems), 2)
        self.assertTrue(any('no VENDOR.txt' in p for p in problems))
        self.assertTrue(any('no LICENSE' in p for p in problems))

    def test_a_missing_folder_is_a_problem(self):
        self.assertEqual(len(licence_check.check_apple_tree('/nonexistent-apple-dir')), 1)

    def test_main_exit_status(self):
        import subprocess, sys
        script = str(ROOT / 'packaging' / 'licence_check.py')
        ok = subprocess.run([sys.executable, script, 'apple', str(self.tree(self.GOOD))], capture_output=True)
        self.assertEqual(ok.returncode, 0)
        root = self.tree({**self.GOOD, 'vendor/bbcpu/LICENSE.GPL-3.0': 'x'})
        bad = subprocess.run([sys.executable, script, 'apple', str(root)], capture_output=True, text=True)
        self.assertEqual(bad.returncode, 1)
        self.assertIn('GPL-3.0 or bbnet20 path', bad.stderr)


if __name__ == '__main__':
    unittest.main()
