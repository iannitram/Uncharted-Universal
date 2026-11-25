"""tools/check_no_game_data.sh: clean trees pass; generic game-data patterns fail. Synthetic files only."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "check_no_game_data.sh"


def check(*targets, **env):
    return subprocess.run(
        ["bash", str(SCRIPT), *map(str, targets)],
        capture_output=True, text=True, env={**os.environ, **env},
    )


@unittest.skipUnless(shutil.which("bash") and shutil.which("od"), "needs bash")
class CheckNoGameDataTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)

    def assertExit(self, result, code, note=""):
        """Compare the exit status; on a mismatch, show the script's own output."""
        self.assertEqual(result.returncode, code, f"{note} stdout={result.stdout!r} stderr={result.stderr!r}")

    def tree(self, files):
        folder = self.root / "tree"
        folder.mkdir()
        for name, data in files.items():
            path = folder / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        return folder

    def test_a_plain_bundle_passes(self):
        folder = self.tree({"Contents/MacOS/app": b"\xcf\xfa\xed\xfe", "Contents/Info.plist": b"<plist/>"})
        self.assertExit(check(folder), 0)

    def test_executable_container_magic_fails(self):
        for magic in (bytes.fromhex("4f153d1d"), bytes.fromhex("7f434e54")):
            folder = self.tree({"data.bin": magic + b"\0" * 8})
            self.assertExit(check(folder), 1, magic)
            shutil.rmtree(folder)

    def test_names_that_only_dumps_carry_fail(self):
        for name in ("eboot.bin", "x/module.PRX", "a.sprx", "b.self", "c.elf", "sce_sys/param.sfo", "CUSA12345/x.txt"):
            folder = self.tree({name: b"text"})
            self.assertExit(check(folder), 1, name)
            shutil.rmtree(folder)

    def test_size_limits_fail(self):
        folder = self.tree({"big.dat": b"\0" * 2048})
        self.assertExit(check(folder, MAX_FILE_BYTES="1024"), 1)
        self.assertExit(check(folder, MAX_TOTAL_BYTES="1024"), 1)
        self.assertExit(check(folder), 0)

    def test_zip_and_ipa_are_unpacked_and_scanned(self):
        import zipfile
        for suffix, member, ok in (("zip", "Payload/a.app/exe", True), ("ipa", "Payload/a.app/eboot.bin", False)):
            archive = self.root / f"pkg.{suffix}"
            with zipfile.ZipFile(archive, "w") as z:
                z.writestr(member, b"x")
            self.assertExit(check(archive), 0 if ok else 1, suffix)

    def test_missing_target_and_no_argument_fail(self):
        self.assertNotEqual(check(self.root / "absent").returncode, 0)
        self.assertExit(check(), 2)


if __name__ == "__main__":
    unittest.main()
