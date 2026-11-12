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


if __name__ == '__main__':
    unittest.main()
