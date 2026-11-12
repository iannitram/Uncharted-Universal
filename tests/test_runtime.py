from paths import ROOT
from pathlib import Path
import os
import subprocess
import unittest

EXE = ROOT / ('out/runtime-test' + ('.exe' if os.name=='nt' else ''))


@unittest.skipUnless(EXE.exists(), 'run bash build.sh --test first')
class RuntimeTests(unittest.TestCase):
    def run_case(self, *args):
        return subprocess.run([str(EXE.resolve()), *args], capture_output=True, text=True, timeout=10)

    def test_runtime_contracts(self):
        r=self.run_case()
        self.assertEqual(r.returncode,0,r.stdout+r.stderr)
        self.assertIn('PASS: thread attributes',r.stdout)
