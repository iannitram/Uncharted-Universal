"""The AppImage entry point (packaging/linux/AppRun): it cleans the host's library injection out
of the environment for every path, the classic launcher's included, as packaging/bbport-entry.c
did, with the names of crates/core host_env. Run with stand-in programs that print their
environment."""
from paths import ROOT
import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

APPRUN = ROOT / 'packaging' / 'linux' / 'AppRun'
HOST_ENV = ROOT / 'crates' / 'core' / 'src' / 'host_env.rs'
PROGRAMS = ('Uncharted', 'uncharted-app', 'uncharted-universal')

# What Steam sets for a non-Steam game, plus an empty one (removed, not kept).
STEAM = {
    'LD_PRELOAD': '/steam/ubuntu12_64/gameoverlayrenderer.so',
    'LD_LIBRARY_PATH': '/steam/runtime/lib',
    'GTK_MODULES': 'gail:atk-bridge',
    'PYTHONPATH': '',
}


def rust_list(const):
    """The names in a `pub const <const>: &[&str] = &[...]` of host_env.rs."""
    text = HOST_ENV.read_text(encoding='utf-8')
    body = re.search(r'pub const ' + const + r': &\[&str\] = &\[(.*?)\];', text, re.S).group(1)
    return re.findall(r'"([A-Za-z0-9_]+)"', body)


def apprun_names():
    text = APPRUN.read_text(encoding='utf-8')
    body = re.search(r'^for name in (.*?); do$', text, re.S | re.M).group(1)
    return body.replace('\\', ' ').split()


def apprun_layers():
    text = APPRUN.read_text(encoding='utf-8')
    return re.findall(r'\b(DISABLE_VK_LAYER_[A-Za-z0-9_]+)=1\b', text)


# Linux only: Git for Windows' sh honours LD_PRELOAD itself and stops at the stand-in value.
@unittest.skipUnless(os.name == 'posix' and shutil.which('sh'), 'a Linux sh')
class AppRunEnvironmentTests(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        shutil.copy(APPRUN, self.dir / 'AppRun')
        for program in PROGRAMS:
            stand_in = self.dir / program
            stand_in.write_bytes(f'#!/bin/sh\necho "program={program}"\necho "args=$*"\nenv\n'.encode())
            stand_in.chmod(0o755)
        (self.dir / 'AppRun').chmod(0o755)

    def run_apprun(self, *args, extra=None):
        environ = {key: value for key, value in os.environ.items()
                   if not key.startswith('BB_') and not key.startswith('DISABLE_VK_LAYER_')}
        environ.update(HOME=str(self.dir), XDG_DATA_HOME=str(self.dir / 'data'))
        environ.update(STEAM)
        environ.update(extra or {})
        result = subprocess.run(['sh', './AppRun', *args], cwd=self.dir, env=environ,
                                capture_output=True, text=True, check=True)
        lines = result.stdout.splitlines()
        program = lines[0].split('=', 1)[1]
        passed = lines[1].split('=', 1)[1]
        seen = dict(line.split('=', 1) for line in lines[2:] if '=' in line)
        return program, passed, seen

    def assert_cleaned(self, seen):
        for name in rust_list('HOST_ONLY'):
            self.assertNotIn(name, seen)
        self.assertEqual(seen.get('BB_HOST_LD_PRELOAD'), STEAM['LD_PRELOAD'])
        self.assertEqual(seen.get('BB_HOST_LD_LIBRARY_PATH'), STEAM['LD_LIBRARY_PATH'])
        self.assertEqual(seen.get('BB_HOST_GTK_MODULES'), STEAM['GTK_MODULES'])
        self.assertNotIn('BB_HOST_PYTHONPATH', seen)
        for layer in rust_list('DISABLED_LAYERS'):
            self.assertEqual(seen.get(layer), '1')

    def test_classic_launcher_play_shortcut_is_cleaned(self):
        """`<AppImage> --play u1`, the shortcut the classic launcher wrote before 2.10."""
        program, passed, seen = self.run_apprun('--play', 'u1')
        self.assertEqual((program, passed), ('Uncharted', '--play u1'))
        self.assert_cleaned(seen)

    def test_classic_report_is_cleaned(self):
        program, passed, seen = self.run_apprun('--report')
        self.assertEqual((program, passed), ('Uncharted', '--report'))
        self.assert_cleaned(seen)

    def test_classic_flag_is_cleaned_and_dropped(self):
        program, passed, seen = self.run_apprun('--classic', '--play', 'u2')
        self.assertEqual((program, passed), ('Uncharted', '--play u2'))
        self.assert_cleaned(seen)

    def test_rust_faces_are_cleaned_too(self):
        program, passed, seen = self.run_apprun('play', 'u1')
        self.assertEqual((program, passed), ('uncharted-universal', 'play u1'))
        self.assert_cleaned(seen)
        program, _, seen = self.run_apprun()
        self.assertEqual(program, 'uncharted-app')
        self.assert_cleaned(seen)

    def test_earlier_diagnostic_copy_is_kept_when_the_value_is_empty(self):
        """As bbport-entry.c: an empty value leaves an existing BB_HOST_ copy alone."""
        _, _, seen = self.run_apprun('--play', 'u1', extra={'LD_PRELOAD': '',
                                                             'BB_HOST_LD_PRELOAD': 'earlier.so'})
        self.assertNotIn('LD_PRELOAD', seen)
        self.assertEqual(seen.get('BB_HOST_LD_PRELOAD'), 'earlier.so')


class AppRunListsTests(unittest.TestCase):
    def test_names_match_host_env(self):
        self.assertEqual(apprun_names(), rust_list('HOST_ONLY'))
        self.assertEqual(len(apprun_names()), 10)

    def test_layers_match_host_env(self):
        self.assertEqual(apprun_layers(), rust_list('DISABLED_LAYERS'))
        self.assertEqual(len(apprun_layers()), 2)


if __name__ == '__main__':
    unittest.main()
