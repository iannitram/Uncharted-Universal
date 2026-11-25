"""Bundle the exact macOS (arm64) Save Relay release, licence and port manifests into the app.

The helper goes beside the app's executables (Contents/MacOS), where `uncharted-universal play` and the
launcher look for it, and so does `save-relay-manifests`, but as a symlink to the real folder in
Contents/Resources: codesign treats every file inside Contents/MacOS as code and would refuse the JSON
there, and the Resources copy is sealed by the bundle signature. The component lock and the licence go
in Contents/Resources too. Usage: bundle_relay.py <Contents/MacOS> <Contents/Resources> <download cache>.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
from urllib.request import urlopen

HERE = Path(__file__).resolve().parent
ASSETS = HERE.parent / "save-relay"
LOCK = HERE / "components.lock.json"


def sha256(path):
    with path.open("rb") as stream:
        digest = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest.update(chunk)
        return digest.hexdigest()


def bundle(macos_dir, resources_dir, cache):
    component = json.loads(LOCK.read_text())["save-relay"]
    source = cache / component["asset"]
    expected = component["sha256"]
    if not source.exists() or sha256(source) != expected:
        cache.mkdir(parents=True, exist_ok=True)
        url = f'{component["source"]}/releases/download/{component["version"]}/{source.name}'
        with tempfile.TemporaryDirectory(dir=cache) as temp:
            download = Path(temp) / source.name
            with urlopen(url, timeout=90) as response, download.open("wb") as out:
                shutil.copyfileobj(response, out)
            if sha256(download) != expected:
                raise ValueError(f"Save Relay SHA256 mismatch for {source.name}")
            download.replace(source)
    macos_dir.mkdir(parents=True, exist_ok=True)
    target = macos_dir / "save-relay"
    shutil.copyfile(source, target)
    target.chmod(0o755)
    resources_dir.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(LOCK, resources_dir / LOCK.name)
    (resources_dir / "licenses").mkdir(exist_ok=True)
    shutil.copyfile(ASSETS / "LICENSE", resources_dir / "licenses" / "save-relay-LICENSE.txt")
    manifests = resources_dir / "save-relay-manifests"
    manifests.mkdir(exist_ok=True)
    link = macos_dir / "save-relay-manifests"
    if link.is_symlink() or link.exists():
        link.unlink()
    link.symlink_to(os.path.relpath(manifests, macos_dir))
    for number in range(1, 4):
        name = f"uncharted-{number}.json"
        shutil.copyfile(ASSETS / name, manifests / name)


if __name__ == "__main__":
    bundle(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]))
