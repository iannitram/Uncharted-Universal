"""The Rust crate licence file of the release packages (packaging/rust_licences.py): synthetic
`cargo metadata` and crate folders, no cargo run."""
from paths import ROOT
import importlib.util
import shutil
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('rust_licences', ROOT / 'packaging' / 'rust_licences.py')
rust_licences = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rust_licences)

NORMAL = [{'kind': None, 'target': None}]
DEV = [{'kind': 'dev', 'target': None}]
BUILD = [{'kind': 'build', 'target': None}]


def package(pid, name, folder, license='MIT', kind='lib'):
    return {'id': pid, 'name': name, 'version': '1.0.0', 'license': license,
            'repository': f'https://example.invalid/{name}',
            'manifest_path': str(Path(folder) / name / 'Cargo.toml'),
            'targets': [{'kind': [kind]}]}


def metadata(folder):
    """app -> slint (-> femtovg), slint-macros (proc macro -> syn); app dev -> testing;
    app build -> cc; cli -> core; runtime alone."""
    packages = [
        package('app', 'uncharted-app', folder, 'GPL-2.0-only', 'bin'),
        package('cli', 'uncharted_cli', folder, 'GPL-2.0-only', 'bin'),
        package('rt', 'uncharted_runtime', folder, 'GPL-2.0-only'),
        package('core', 'uncharted_core', folder, 'GPL-2.0-only'),
        package('slint', 'slint', folder, 'GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0'),
        package('femtovg', 'femtovg', folder, 'MIT OR Apache-2.0'),
        package('macros', 'slint-macros', folder, kind='proc-macro'),
        package('syn', 'syn', folder),
        package('testing', 'testing', folder),
        package('cc', 'cc', folder),
    ]
    deps = {
        'app': [('slint', NORMAL), ('testing', DEV), ('cc', BUILD), ('core', NORMAL)],
        'slint': [('femtovg', NORMAL), ('macros', NORMAL)],
        'macros': [('syn', NORMAL)],
        'cli': [('core', NORMAL)],
    }
    nodes = [{'id': p['id'], 'deps': [{'pkg': d, 'dep_kinds': k} for d, k in deps.get(p['id'], [])]}
             for p in packages]
    return {'packages': packages, 'resolve': {'nodes': nodes},
            'workspace_members': ['app', 'cli', 'rt', 'core']}


class RustLicencesTest(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.folder, ignore_errors=True)

    def crate_file(self, crate, name, text):
        path = Path(self.folder) / crate / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding='utf-8')

    def test_only_linked_crates_are_listed(self):
        ids = rust_licences.linked_packages(metadata(self.folder))
        # Normal dependencies only; no workspace member, dev or build dependency, proc macro,
        # or what a proc macro links.
        self.assertEqual(ids, {'slint', 'femtovg'})

    def test_a_missing_shipped_package_is_an_error(self):
        data = metadata(self.folder)
        data['workspace_members'].remove('rt')
        with self.assertRaises(ValueError):
            rust_licences.linked_packages(data)

    def test_licence_files_are_found_by_name(self):
        self.crate_file('femtovg', 'LICENSE-MIT', 'MIT text')
        self.crate_file('femtovg', 'LICENSE-APACHE', 'Apache text')
        self.crate_file('femtovg', 'COPYRIGHT', 'copyright text')
        self.crate_file('femtovg', 'README.md', 'not a licence')
        names = [p.name for p in rust_licences.licence_files(Path(self.folder) / 'femtovg')]
        self.assertEqual(names, ['COPYRIGHT', 'LICENSE-APACHE', 'LICENSE-MIT'])
        self.assertEqual(rust_licences.licence_files(Path(self.folder) / 'absent'), [])

    def test_each_text_once_with_its_crates(self):
        data = metadata(self.folder)
        self.crate_file('femtovg', 'LICENSE-MIT', 'Same MIT text\n')
        self.crate_file('slint', 'LICENSE.md', 'Same MIT text\n')
        text = rust_licences.render(data, {'slint', 'femtovg'})
        self.assertIn('femtovg 1.0.0: MIT OR Apache-2.0 (https://example.invalid/femtovg)', text)
        self.assertIn('slint 1.0.0: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0', text)
        self.assertEqual(text.count('Same MIT text'), 1)
        self.assertIn('  femtovg 1.0.0 (LICENSE-MIT)', text)
        self.assertIn('  slint 1.0.0 (LICENSE.md)', text)
        self.assertNotIn('No licence file', text)
        self.assertNotIn('uncharted_core', text)

    def test_a_crate_without_a_file_is_listed_by_its_expression(self):
        text = rust_licences.render(metadata(self.folder), {'femtovg'})
        self.assertIn('No licence file in the published crate', text)
        self.assertIn('  femtovg 1.0.0', text)
        self.assertIn('femtovg 1.0.0: MIT OR Apache-2.0', text)


if __name__ == '__main__':
    unittest.main()
