#!/usr/bin/env python3
"""Unit tests for tools/verify_fsr4_assets.py."""

import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "tools"))
import verify_fsr4_assets  # noqa: E402


def build_valid_tree(root: Path, tiers: tuple[str, ...] = ("1080",)) -> None:
    root.mkdir(parents=True, exist_ok=True)
    artifacts: dict[str, dict[str, object]] = {}
    for name in verify_fsr4_assets.expected_files(tiers):
        path = root / name
        if name.endswith("_shader_manifest.json"):
            continue
        payload = f"payload:{name}".encode("utf-8")
        path.write_bytes(payload)
        if name != "LICENSE-FSR4-v07.txt":
            artifacts[name] = {
                "size": len(payload),
                "sha256": hashlib.sha256(payload).hexdigest(),
            }
    for model in verify_fsr4_assets.MODELS:
        manifest = root / f"fsr4_model_v07_i8_{model}_shader_manifest.json"
        manifest.write_text(json.dumps({"artifacts": artifacts}), encoding="utf-8")


class VerifyFsr4AssetsTests(unittest.TestCase):
    def test_valid_tree_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "fsr4_shaders"
            build_valid_tree(root, ("1080", "2160"))
            self.assertEqual(verify_fsr4_assets.verify_dir(root, ("1080", "2160")), [])

    def test_missing_directory_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            errors = verify_fsr4_assets.verify_dir(Path(tmp) / "missing", ("1080",))
            self.assertEqual(len(errors), 1)
            self.assertIn("missing asset directory", errors[0])

    def test_missing_and_empty_files_fail(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "fsr4_shaders"
            build_valid_tree(root, ("1080",))
            (root / "rcas.spv").unlink()
            (root / "spd_auto_exposure.spv").write_bytes(b"")
            errors = verify_fsr4_assets.verify_dir(root, ("1080",))
            joined = "\n".join(errors)
            self.assertIn("missing or empty: rcas.spv", joined)
            self.assertIn("missing or empty: spd_auto_exposure.spv", joined)

    def test_sha256_and_size_mismatch_fail(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "fsr4_shaders"
            build_valid_tree(root, ("1080",))
            (root / "rcas.spv").write_bytes(b"corrupted-different-size")
            same_len = b"X" * len(b"payload:spd_auto_exposure.spv")
            (root / "spd_auto_exposure.spv").write_bytes(same_len)
            errors = verify_fsr4_assets.verify_dir(root, ("1080",))
            joined = "\n".join(errors)
            self.assertIn("size mismatch for rcas.spv", joined)
            self.assertIn("sha256 mismatch for spd_auto_exposure.spv", joined)

    def test_corrupt_manifest_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "fsr4_shaders"
            build_valid_tree(root, ("1080",))
            (root / "fsr4_model_v07_i8_native_shader_manifest.json").write_text("{bad", encoding="utf-8")
            errors = verify_fsr4_assets.verify_dir(root, ("1080",))
            self.assertTrue(any("invalid manifest" in e for e in errors))


if __name__ == "__main__":
    unittest.main()
