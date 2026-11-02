"""Mouse button names for key capture: X11 reports the wheel as buttons 4 and 5."""
from paths import ROOT
import sys
import unittest

sys.path.insert(0, str(ROOT / 'launcher'))
import uncharted_launcher as launcher  # noqa: E402

name = launcher.mouse_button_name


class MouseNames(unittest.TestCase):
    def test_the_three_main_buttons_on_both_platforms(self):
        for windows in (False, True):
            self.assertEqual([name(n, windows) for n in (1, 2, 3)],
                             ['Mouse Left', 'Mouse Middle', 'Mouse Right'])

    def test_x11_wheel_is_4_and_5_and_the_side_buttons_are_8_and_9(self):
        self.assertEqual(name(4, False), 'Wheel Up')
        self.assertEqual(name(5, False), 'Wheel Down')
        self.assertEqual(name(8, False), 'Mouse 4')
        self.assertEqual(name(9, False), 'Mouse 5')

    def test_windows_keeps_its_mapping(self):
        self.assertEqual([name(n, True) for n in (4, 5, 8, 9)],
                         ['Mouse 4', 'Mouse 5', 'Mouse 4', 'Mouse 5'])

    def test_other_buttons_have_no_name(self):
        for n in (0, 6, 7, 10, 99):
            self.assertIsNone(name(n, False))
            self.assertIsNone(name(n, True))


if __name__ == '__main__':
    unittest.main()
