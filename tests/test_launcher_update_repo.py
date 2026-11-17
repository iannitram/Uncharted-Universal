from paths import ROOT
import importlib.util
import sys
import unittest


spec = importlib.util.spec_from_file_location(
    'uncharted_launcher', ROOT / 'launcher/uncharted_launcher.py')
launcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launcher)  # also puts launcher/ on sys.path, so uncharted_linux imports below

RELEASES_API = 'https://api.github.com/repos/iannitram/Uncharted-Universal/releases/latest'
RELEASES_PAGE = 'https://github.com/iannitram/Uncharted-Universal/releases/latest'


class UpdateRepoTests(unittest.TestCase):
    def test_update_check_asks_this_repo(self):
        self.assertEqual(RELEASES_API, launcher.RELEASES_API)

    def test_releases_page_is_this_repo(self):
        self.assertEqual(RELEASES_PAGE, launcher.RELEASES_PAGE)


class LinuxTagOrderTests(unittest.TestCase):
    """The Linux launcher compares update tags with uncharted_linux.version_key (it replaces the
    launcher's version_tuple on Linux); these tags are what this repo publishes."""

    @classmethod
    def setUpClass(cls):
        import uncharted_linux
        cls.version_key = staticmethod(uncharted_linux.version_key)

    def test_next_linux_build_is_newer(self):
        self.assertGreater(self.version_key('v0.3.14-linux.3'), self.version_key('0.3.14-linux.2'))

    def test_tag_is_not_newer_than_itself(self):
        self.assertFalse(self.version_key('v0.3.14-linux.3') > self.version_key('v0.3.14-linux.3'))


if __name__ == '__main__':
    unittest.main()
