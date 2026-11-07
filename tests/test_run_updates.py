"""Title update overlay cache invalidation in run.py."""
import importlib.util
import tempfile
import unittest
from pathlib import Path

from paths import ROOT

spec = importlib.util.spec_from_file_location('bbrun_updates', ROOT / 'run.py')
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)


class MergeUpdateTests(unittest.TestCase):
    def test_base_file_changes_rebuild_the_cached_overlay(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            base, update, view = root / 'game', root / 'update', root / 'merged'
            (base / 'sce_sys').mkdir(parents=True)
            (update / 'sce_sys').mkdir(parents=True)
            (base / 'sce_sys' / 'param.sfo').write_text('base')
            (update / 'sce_sys' / 'param.sfo').write_text('update')
            (base / 'eboot.bin').write_text('original')

            run.merge_update(base, update, view)
            self.assertEqual((view / 'eboot.bin').read_text(), 'original')
            (base / 'eboot.bin').write_text('replaced')

            run.merge_update(base, update, view)
            self.assertEqual((view / 'eboot.bin').read_text(), 'replaced')
            self.assertEqual((view / 'sce_sys' / 'param.sfo').read_text(), 'update')


if __name__ == '__main__':
    unittest.main()
