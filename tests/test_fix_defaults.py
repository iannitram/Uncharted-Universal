"""run.py picks the per-game fix defaults in one function, fix_defaults(player_env, exe)."""
from paths import ROOT
import copy
import importlib.util
from types import MappingProxyType
import unittest

spec = importlib.util.spec_from_file_location('bbrun', ROOT / 'run.py')
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)

U1, U2, U3 = run.EXECUTABLES['u1'], run.EXECUTABLES['u2'], run.EXECUTABLES['u3']


def frozen(**env):
    return MappingProxyType(dict(env))  # any write to it raises TypeError


class FixDefaults(unittest.TestCase):
    def test_uncharted_1_gets_the_mesh_raycaster_copies(self):
        got = run.fix_defaults(frozen(), U1)
        self.assertEqual(got['BB_SCALAR_COPY64_SYNC'], '2')
        self.assertEqual(got['BB_SCALAR_COPY64_DEFER'], '1')

    def test_uncharted_1_keeps_its_other_defaults(self):
        got = run.fix_defaults(frozen(), U1)
        self.assertEqual(got['BB_TINY_READBACK'], '0')
        self.assertNotIn('BB_GPU_MERGE', got)

    def test_uncharted_2_gets_all_four_of_its_fixes(self):
        self.assertEqual(run.fix_defaults(frozen(), U2),
                         {'BB_GPU_MERGE': '0', 'BB_TINY_READBACK': '1',
                          'BB_SCALAR_COPY64_SYNC': '2', 'BB_SCALAR_COPY64_DEFER': '1',
                          'BB_READBACK_SMALL_BUFFERS': '1'})

    def test_uncharted_3_keeps_the_copies_off_because_they_cost_94_to_54_fps(self):
        self.assertEqual(run.fix_defaults(frozen(), U3),
                         {'BB_GPU_MERGE': '0', 'BB_TINY_READBACK': '0', 'BB_SCALAR_COPY64_SYNC': '0',
                          'BB_SCALAR_COPY64_DEFER': '0', 'BB_TOGGLE2': '12'})

    def test_a_copy_setting_the_player_chose_removes_both_copy_defaults(self):
        got = run.fix_defaults(frozen(BB_SCALAR_COPY64_SYNC='0'), U1)
        self.assertNotIn('BB_SCALAR_COPY64_SYNC', got)
        self.assertNotIn('BB_SCALAR_COPY64_DEFER', got)
        self.assertEqual(got['BB_TINY_READBACK'], '0')

    def test_a_setting_the_player_chose_is_left_out_and_the_rest_stay(self):
        got = run.fix_defaults(frozen(BB_TINY_READBACK='0'), U2)
        self.assertEqual(got, {'BB_GPU_MERGE': '0', 'BB_SCALAR_COPY64_SYNC': '2',
                               'BB_SCALAR_COPY64_DEFER': '1', 'BB_READBACK_SMALL_BUFFERS': '1'})

    def test_u2_label_shortcuts_are_restricted_to_u2(self):
        for exe in (U1, U3):
            self.assertEqual(run.fix_defaults(frozen(), exe)['BB_TOGGLE2'], '12')
            self.assertNotIn('BB_TOGGLE2', run.fix_defaults(frozen(BB_TOGGLE2='0'), exe))
        self.assertNotIn('BB_TOGGLE2', run.fix_defaults(frozen(), U2))

    def test_u3_keeps_a_player_memory_override(self):
        self.assertNotIn('BB_GPU_MERGE', run.fix_defaults(frozen(BB_GPU_MERGE='1'), U3))

    def test_an_unknown_executable_gets_no_defaults(self):
        self.assertEqual(run.fix_defaults(frozen(), 'other.elf'), {})

    def test_the_player_environment_is_not_changed(self):
        env = {'BB_TINY_READBACK': '0', 'PATH': '/bin'}
        before = copy.deepcopy(env)
        run.fix_defaults(env, U2)
        self.assertEqual(env, before)


if __name__ == '__main__':
    unittest.main()
