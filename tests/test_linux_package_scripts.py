import re
import unittest

from paths import ROOT


def packaged_scripts():
    """Script file names packaging/linux/package.sh copies into the package's scripts/."""
    text = (ROOT / 'packaging' / 'linux' / 'package.sh').read_text(encoding='utf-8')
    names = set()
    for line in text.splitlines():
        if line.startswith('cp scripts/'):
            names.update(re.findall(r'scripts/([A-Za-z0-9_]+\.py)', line))
    return names


def scripts_run_py_needs():
    """Script file names run.py starts (run_script) or runs by path (PORT / 'scripts' / ...)."""
    text = (ROOT / 'run.py').read_text(encoding='utf-8')
    return (set(re.findall(r"run_script\('([A-Za-z0-9_]+\.py)'", text))
            | set(re.findall(r"'scripts' / '([A-Za-z0-9_]+\.py)'", text)))


class LinuxPackageScriptsTests(unittest.TestCase):
    def test_run_py_scripts_are_found(self):
        self.assertEqual(scripts_run_py_needs(),
                         {'patches.py', 'prepare.py', 'link_modules.py', 'content_profile.py',
                          'trophies.py', 'render_720p.py', 'render_res.py'})

    def test_every_script_run_py_starts_is_packaged(self):
        self.assertEqual(scripts_run_py_needs() - packaged_scripts(), set())

    def test_every_script_run_py_imports_is_packaged(self):
        """run.py imports scripts/ modules too (cheats, prepare, save_slots for #246)."""
        text = (ROOT / 'run.py').read_text(encoding='utf-8')
        imported = {name + '.py' for name in re.findall(r'^\s*(?:import|from) ([A-Za-z0-9_]+)', text, re.M)}
        needed = {name for name in imported if (ROOT / 'scripts' / name).is_file()}
        self.assertIn('save_slots.py', needed)
        self.assertEqual(needed - packaged_scripts(), set())


if __name__ == '__main__':
    unittest.main()
