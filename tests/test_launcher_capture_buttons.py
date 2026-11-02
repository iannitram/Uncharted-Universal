"""Mouse capture on the Controls page: the binding each click gets, per platform."""
from paths import ROOT
import sys
import unittest
from unittest import mock

sys.path.insert(0, str(ROOT / 'launcher'))
import uncharted_launcher as launcher  # noqa: E402


class CapturedButtonTests(unittest.TestCase):
    def capture(self, num, windows):
        instance = launcher.Launcher.__new__(launcher.Launcher)
        instance.capturing = ('left', 0)
        calls = []
        instance.set_key = lambda action, slot, name: calls.append((action, slot, name))
        with mock.patch.object(launcher, 'WINDOWS', windows):
            result = instance.captured_button(mock.Mock(num=num))
        return result, calls

    def test_x11_scroll_during_capture_binds_wheel_not_side_buttons(self):
        self.assertEqual(self.capture(4, False), ('break', [('left', 0, 'Wheel Up')]))
        self.assertEqual(self.capture(5, False), ('break', [('left', 0, 'Wheel Down')]))

    def test_x11_side_buttons_bind_mouse_4_and_5(self):
        self.assertEqual(self.capture(8, False), ('break', [('left', 0, 'Mouse 4')]))
        self.assertEqual(self.capture(9, False), ('break', [('left', 0, 'Mouse 5')]))

    def test_windows_side_buttons_keep_mouse_names(self):
        self.assertEqual(self.capture(4, True), ('break', [('left', 0, 'Mouse 4')]))
        self.assertEqual(self.capture(5, True), ('break', [('left', 0, 'Mouse 5')]))

    def test_unnamed_button_binds_nothing_and_still_breaks(self):
        self.assertEqual(self.capture(6, False), ('break', []))
        self.assertEqual(self.capture(7, True), ('break', []))


if __name__ == '__main__':
    unittest.main()
