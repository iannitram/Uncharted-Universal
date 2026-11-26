"""packaging/windows/dll_imports.py (#625): fake import tables, no llvm-objdump or Windows needed."""
from paths import ROOT
import importlib.util
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('dll_imports', ROOT / 'packaging' / 'windows' / 'dll_imports.py')
dll_imports = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dll_imports)

OBJDUMP = """
The Import Tables:
  lookup 0001 time 00000000 fwd 00000000 name 0002 addr 0003

    DLL Name: avcodec-63.dll
    vma:  Hint/Ord Member-Name Bound-To
    0004       12  avcodec_open2

    DLL Name: KERNEL32.dll
    DLL Name: avcodec-63.dll
"""


class ParseTest(unittest.TestCase):
    def test_names_once_in_order(self):
        self.assertEqual(dll_imports.parse_imports(OBJDUMP), ['avcodec-63.dll', 'KERNEL32.dll'])


class SystemTest(unittest.TestCase):
    def test_api_sets_system32_and_redistributables(self):
        with tempfile.TemporaryDirectory() as sys32:
            for name in ('kernel32.dll', 'VCRUNTIME140.dll'):
                (Path(sys32) / name).write_text('')
            self.assertTrue(dll_imports.is_system('api-ms-win-crt-runtime-l1-1-0.dll', sys32))
            self.assertTrue(dll_imports.is_system('kernel32.dll', sys32))
            # Present on this PC from the VC++ Redistributable, absent on a clean Windows.
            self.assertFalse(dll_imports.is_system('VCRUNTIME140.dll', sys32))
            self.assertFalse(dll_imports.is_system('avcodec-63.dll', sys32))


def fake(table):
    return lambda path: table.get(Path(path).name, [])


def no_system(name):
    return name.lower().startswith('kernel32')


class ClosureTest(unittest.TestCase):
    def test_first_folder_wins_and_dependencies_follow(self):
        table = {'bb-probe.exe': ['avcodec-63.dll', 'SDL3.dll', 'KERNEL32.dll'],
                 'avcodec-63.dll': ['avutil-61.dll'], 'avutil-61.dll': ['KERNEL32.dll']}
        with tempfile.TemporaryDirectory() as lgpl, tempfile.TemporaryDirectory() as clang64:
            for folder, names in ((lgpl, ('avcodec-63.dll', 'avutil-61.dll')),
                                  (clang64, ('avcodec-63.dll', 'avutil-61.dll', 'SDL3.dll', 'libdav1d-7.dll'))):
                for name in names:
                    (Path(folder) / name).write_text('')
            found, missing = dll_imports.closure(['bb-probe.exe'], [lgpl, clang64], fake(table), no_system)
            self.assertEqual(missing, [])
            self.assertEqual(found, [Path(lgpl) / 'avcodec-63.dll', Path(clang64) / 'SDL3.dll', Path(lgpl) / 'avutil-61.dll'])

    def test_missing_is_named(self):
        found, missing = dll_imports.closure(['x.exe'], [], fake({'x.exe': ['swresample-7.dll']}), no_system)
        self.assertEqual((found, missing), ([], ['swresample-7.dll']))


class CheckTest(unittest.TestCase):
    def test_missing_import_reported_per_folder_and_crt_filled(self):
        table = {'bb-probe.exe': ['avcodec-63.dll', 'kernel32.dll'], 'save-relay.exe': ['VCRUNTIME140.dll'],
                 'avcodec-63.dll': []}
        with tempfile.TemporaryDirectory() as root, tempfile.TemporaryDirectory() as crt:
            (Path(root) / 'bin').mkdir()
            for name in ('bin/bb-probe.exe', 'save-relay.exe', 'avcodec-63.dll'):
                (Path(root) / name).write_text('')
            (Path(crt) / 'vcruntime140.dll').write_text('crt')
            problems = dll_imports.missing_imports(root, fake(table), no_system)
            # avcodec-63.dll sits in the top folder, not next to bin/bb-probe.exe.
            self.assertEqual(problems, {'bin/bb-probe.exe': ['avcodec-63.dll'], 'save-relay.exe': ['VCRUNTIME140.dll']})
            self.assertEqual(dll_imports.fill_crt(root, crt, problems), ['vcruntime140.dll'])
            self.assertEqual(dll_imports.missing_imports(root, fake(table), no_system),
                             {'bin/bb-probe.exe': ['avcodec-63.dll']})


if __name__ == '__main__':
    unittest.main()
