"""Bundle the exact Linux Save Relay release, license and port manifests."""
import hashlib
import json
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
        return hashlib.file_digest(stream, "sha256").hexdigest()


def bundle(stage, cache):
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
    stage.mkdir(parents=True, exist_ok=True)
    target = stage / "save-relay"
    shutil.copyfile(source, target)
    target.chmod(0o755)
    shutil.copyfile(LOCK, stage / LOCK.name)
    (stage / "licenses").mkdir(exist_ok=True)
    shutil.copyfile(ASSETS / "LICENSE", stage / "licenses" / "save-relay-LICENSE.txt")
    manifests = stage / "save-relay-manifests"
    manifests.mkdir(exist_ok=True)
    for number in range(1, 4):
        name = f"uncharted-{number}.json"
        shutil.copyfile(ASSETS / name, manifests / name)


if __name__ == "__main__":
    bundle(Path(sys.argv[1]), Path(sys.argv[2]))
