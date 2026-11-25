"""tools/write_ci_manifest.py lists every platform's package and refuses a release missing one."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "tools" / "write_ci_manifest.py"


def run(directory, platform, smoke):
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--directory", str(directory), "--platform", platform,
         "--revision", "abc", "--smoke-result", str(smoke)],
        capture_output=True, text=True,
    )


class WriteCiManifestTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.dir = Path(self._tmp.name)
        self.smoke = self.dir / "vulkan-smoke.txt"
        self.smoke.write_text("ok\n")

    def add(self, *names):
        for name in names:
            (self.dir / name).write_bytes(name.encode())

    def test_macos_zip_is_listed_beside_linux_and_windows(self):
        self.add("Uncharted-x86_64.AppImage", "uncharted-linux.tar.gz", "uncharted-windows.zip", "uncharted-macos.zip")
        result = run(self.dir, "linux,windows,macos", self.smoke)
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((self.dir / "manifest.json").read_text())
        paths = {entry["path"] for entry in manifest["artifacts"]}
        self.assertIn("uncharted-macos.zip", paths)
        self.assertIn("uncharted-macos.zip", (self.dir / "SHA256SUMS").read_text())

    def test_a_named_platform_without_its_package_fails(self):
        self.add("Uncharted-x86_64.AppImage", "uncharted-linux.tar.gz", "uncharted-windows.zip")
        result = run(self.dir, "linux,windows,macos", self.smoke)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("uncharted-macos.zip", result.stderr)

    def test_unknown_platform_fails(self):
        self.add("uncharted-windows.zip")
        self.assertNotEqual(run(self.dir, "plan9", self.smoke).returncode, 0)


if __name__ == "__main__":
    unittest.main()
