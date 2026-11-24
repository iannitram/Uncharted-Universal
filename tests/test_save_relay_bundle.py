"""Verify Save Relay component locks, manifests, and release bundling scripts."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
REQUIRED_LOCK_KEYS = {
    "version",
    "source_commit",
    "asset",
    "sha256",
    "source",
    "license",
    "license_source",
}


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class SaveRelayBundleTests(unittest.TestCase):
    def test_component_locks_have_valid_pinned_entries(self):
        for platform, expected_asset in (
            ("linux", "save-relay-1.0.0-rc.2-x86_64-unknown-linux-musl"),
            ("windows", "save-relay-1.0.0-rc.2-x86_64-pc-windows-msvc.exe"),
            ("macos", "save-relay-1.0.0-rc.2-aarch64-apple-darwin"),
        ):
            lock_path = ROOT / "packaging" / platform / "components.lock.json"
            self.assertTrue(lock_path.is_file(), f"missing {lock_path}")
            data = json.loads(lock_path.read_text(encoding="utf-8"))
            self.assertIn("save-relay", data)
            entry = data["save-relay"]
            self.assertTrue(REQUIRED_LOCK_KEYS.issubset(entry.keys()), platform)
            self.assertEqual(entry["version"], "v1.0.0-rc.2")
            self.assertEqual(entry["asset"], expected_asset)
            self.assertEqual(len(entry["sha256"]), 64)
            int(entry["sha256"], 16)
            self.assertEqual(entry["source"], "https://github.com/iannitram/save-relay")
            self.assertEqual(entry["license"], "MIT")
            self.assertEqual(len(entry["source_commit"]), 40)

    def test_manifests_follow_launcher_saves_dir_contract(self):
        for number, title in (
            (1, "Uncharted: Drake's Fortune"),
            (2, "Uncharted 2: Among Thieves"),
            (3, "Uncharted 3: Drake's Deception"),
        ):
            path = ROOT / "packaging" / "save-relay" / f"uncharted-{number}.json"
            raw = path.read_text(encoding="utf-8").rstrip("\n")
            self.assertNotIn("\n", raw)
            doc = json.loads(raw)
            self.assertEqual(doc["format"], 1)
            self.assertEqual(doc["id"], f"uncharted-{number}")
            self.assertEqual(doc["name"], title)
            self.assertEqual(doc["saves"], {})
            self.assertEqual(doc["saves_from"], "launcher")
            self.assertEqual(doc["include"], [f"SAVEDATA0{number}*/**"])
            self.assertEqual(doc["exclude"], ["**/*.tmp", "**/.save-relay-*"])

    def test_bundle_relay_stages_binary_lock_license_and_manifests(self):
        for platform, binary_name in (
            ("linux", "save-relay"),
            ("windows", "save-relay.exe"),
        ):
            mod = load_module(
                ROOT / "packaging" / platform / "bundle_relay.py",
                f"bundle_relay_{platform}",
            )
            with tempfile.TemporaryDirectory() as tmp:
                tmp_path = Path(tmp)
                payload = f"synthetic-{platform}-relay".encode()
                digest = hashlib.sha256(payload).hexdigest()
                lock = {
                    "save-relay": {
                        "version": "v1.0.0-rc.2",
                        "asset": f"save-relay-{platform}",
                        "sha256": digest,
                        "source": "https://127.0.0.1:0",
                    }
                }
                fake_lock = tmp_path / "components.lock.json"
                fake_lock.write_text(json.dumps(lock), encoding="utf-8")
                cache = tmp_path / "cache"
                cache.mkdir()
                (cache / f"save-relay-{platform}").write_bytes(payload)
                stage = tmp_path / "stage"

                orig_lock = mod.LOCK
                try:
                    mod.LOCK = fake_lock
                    mod.bundle(stage, cache)
                finally:
                    mod.LOCK = orig_lock

                staged_bin = stage / binary_name
                self.assertTrue(staged_bin.is_file())
                self.assertEqual(staged_bin.read_bytes(), payload)
                if platform == "linux" and os.name == "posix":
                    self.assertTrue(os.access(staged_bin, os.X_OK))
                self.assertEqual(
                    json.loads((stage / "components.lock.json").read_text(encoding="utf-8")),
                    lock,
                )
                self.assertTrue((stage / "licenses" / "save-relay-LICENSE.txt").is_file())
                for number in range(1, 4):
                    self.assertTrue(
                        (stage / "save-relay-manifests" / f"uncharted-{number}.json").is_file()
                    )


    def test_macos_bundle_relay_stages_helper_beside_the_app_and_lock_in_resources(self):
        mod = load_module(ROOT / "packaging" / "macos" / "bundle_relay.py", "bundle_relay_macos")
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            payload = b"synthetic-macos-relay"
            lock = {
                "save-relay": {
                    "version": "v1.0.0-rc.2",
                    "asset": "save-relay-macos",
                    "sha256": hashlib.sha256(payload).hexdigest(),
                    "source": "https://127.0.0.1:0",
                }
            }
            fake_lock = tmp_path / "components.lock.json"
            fake_lock.write_text(json.dumps(lock), encoding="utf-8")
            cache = tmp_path / "cache"
            cache.mkdir()
            (cache / "save-relay-macos").write_bytes(payload)
            macos, resources = tmp_path / "MacOS", tmp_path / "Resources"
            orig_lock = mod.LOCK
            try:
                mod.LOCK = fake_lock
                mod.bundle(macos, resources, cache)
            finally:
                mod.LOCK = orig_lock
            self.assertEqual((macos / "save-relay").read_bytes(), payload)
            self.assertTrue(os.access(macos / "save-relay", os.X_OK))
            for number in range(1, 4):
                self.assertTrue((macos / "save-relay-manifests" / f"uncharted-{number}.json").is_file())
                self.assertTrue((resources / "save-relay-manifests" / f"uncharted-{number}.json").is_file())
            # codesign treats files in Contents/MacOS as code: the folder there is only a link.
            self.assertTrue((macos / "save-relay-manifests").is_symlink())
            self.assertEqual(
                json.loads((resources / "components.lock.json").read_text(encoding="utf-8")), lock
            )
            self.assertTrue((resources / "licenses" / "save-relay-LICENSE.txt").is_file())

    def test_macos_bundle_relay_refuses_a_download_that_does_not_match_the_lock(self):
        mod = load_module(ROOT / "packaging" / "macos" / "bundle_relay.py", "bundle_relay_macos_bad")
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            lock = {"save-relay": {"version": "v1", "asset": "x", "sha256": "0" * 64, "source": "https://127.0.0.1:0"}}
            fake_lock = tmp_path / "components.lock.json"
            fake_lock.write_text(json.dumps(lock), encoding="utf-8")
            cache = tmp_path / "cache"
            cache.mkdir()
            (cache / "x").write_bytes(b"tampered")
            orig_lock = mod.LOCK
            mod.LOCK = fake_lock
            try:
                # The cached file fails the hash, so it would be downloaded again: no network here.
                with self.assertRaises(Exception):
                    mod.bundle(tmp_path / "MacOS", tmp_path / "Resources", cache)
            finally:
                mod.LOCK = orig_lock
            self.assertFalse((tmp_path / "MacOS" / "save-relay").exists())


if __name__ == "__main__":
    unittest.main()
