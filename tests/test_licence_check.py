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


if __name__ == '__main__':
    unittest.main()
