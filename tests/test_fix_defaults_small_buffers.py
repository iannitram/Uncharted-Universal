"""Uncharted 2 runs with the brightness flicker fix unless the player chose otherwise."""
from paths import ROOT
import copy
import importlib.util
from types import MappingProxyType
import unittest

spec = importlib.util.spec_from_file_location('bbrun', ROOT / 'run.py')
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)

U1, U2, U3 = run.EXECUTABLES['u1'], run.EXECUTABLES['u2'], run.EXECUTABLES['u3']
KEY = 'BB_READBACK_SMALL_BUFFERS'


def frozen(**env):
    return MappingProxyType(dict(env))


class SmallBuffers(unittest.TestCase):
    def test_uncharted_2_gets_the_fix(self):
        self.assertEqual(run.fix_defaults(frozen(), U2)[KEY], '1')

    def test_uncharted_1_and_3_do_not(self):
        self.assertNotIn(KEY, run.fix_defaults(frozen(), U1))
        self.assertNotIn(KEY, run.fix_defaults(frozen(), U3))

    def test_unknown_executable_gets_nothing(self):
        self.assertEqual(run.fix_defaults(frozen(), 'other.elf'), {})

    def test_the_players_choice_is_left_out_for_any_value(self):
        for value in ('0', '1', ''):
            self.assertNotIn(KEY, run.fix_defaults(frozen(**{KEY: value}), U2), value)

    def test_the_other_uncharted_2_fixes_stay(self):
        got = run.fix_defaults(frozen(), U2)
        self.assertEqual({k: v for k, v in got.items() if k != KEY},
                         {'BB_GPU_MERGE': '0', 'BB_TINY_READBACK': '1',
                          'BB_SCALAR_COPY64_SYNC': '2', 'BB_SCALAR_COPY64_DEFER': '1'})

    def test_the_player_environment_is_not_changed(self):
        env = {'PATH': '/bin'}
        before = copy.deepcopy(env)
        run.fix_defaults(env, U2)
        self.assertEqual(env, before)


if __name__ == '__main__':
    unittest.main()
