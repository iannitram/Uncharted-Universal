#!/usr/bin/env python3
"""Fetch the pinned Save Relay iOS library (apple/ios/components.lock.json) and print its folder.

  apple/fetch_save_relay.py <cache dir>         the release zip, checked against the lock's SHA256
  apple/fetch_save_relay.py <cache dir> --sim   a Simulator build of the pinned tag (local and CI tests)

The folder printed on stdout (the only thing written there) has lib/libsave_relay.a, include/save_relay.h
and bindings/ios/{SaveRelay.swift,module.modulemap}, the layout of the release zip. The library is
always linked by full path: `-L dir -lsave_relay` can pick up a cdylib that crashes on a device.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path
from urllib.request import urlopen

HERE = Path(__file__).resolve().parent
LOCK = HERE / "ios" / "components.lock.json"


def log(message):
    print(message, file=sys.stderr)


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fetch_release(component, cache):
    asset = cache / component["asset"]
    if not asset.exists() or sha256(asset) != component["sha256"]:
        cache.mkdir(parents=True, exist_ok=True)
        url = f'{component["source"]}/releases/download/{component["version"]}/{component["asset"]}'
        log(f"downloading {url}")
        with tempfile.TemporaryDirectory(dir=cache) as temp:
            download = Path(temp) / component["asset"]
            with urlopen(url, timeout=120) as response, download.open("wb") as out:
                shutil.copyfileobj(response, out)
            if sha256(download) != component["sha256"]:
                raise SystemExit(f'Save Relay SHA256 mismatch for {component["asset"]}')
            download.replace(asset)
    folder = cache / "save-relay-ios"
    shutil.rmtree(folder, ignore_errors=True)
    with zipfile.ZipFile(asset) as archive:
        for name in archive.namelist():
            if name.startswith("/") or ".." in Path(name).parts:
                raise SystemExit(f"unsafe path in {asset.name}: {name}")
        archive.extractall(folder)
    library = folder / component["library"]
    if sha256(library) != component["library_sha256"]:
        raise SystemExit("libsave_relay.a does not match the lock")
    return folder


def build_simulator(component, cache):
    source = cache / "save-relay-src"
    if not source.exists():
        log(f'cloning {component["source"]} at {component["version"]}')
        subprocess.run(["git", "clone", "-q", "--depth", "1", "--branch", component["version"],
                        component["source"] + ".git", str(source)], check=True, stdout=sys.stderr)
    head = subprocess.run(["git", "-C", str(source), "rev-parse", "HEAD"], check=True,
                          capture_output=True, text=True).stdout.strip()
    if head != component["source_commit"]:
        raise SystemExit(f"the tag now points at {head}, not the pinned {component['source_commit']}")
    env = dict(os.environ, CARGO_TARGET_DIR=str(cache / "save-relay-sim-target"))
    env.setdefault("CARGO_BUILD_JOBS", "3")
    subprocess.run(["cargo", "build", "--release", "--lib", "--no-default-features", "--features", "ffi",
                    "--target", "aarch64-apple-ios-sim", "--manifest-path", str(source / "Cargo.toml")],
                   check=True, env=env, stdout=sys.stderr)
    folder = cache / "save-relay-ios-sim"
    shutil.rmtree(folder, ignore_errors=True)
    (folder / "lib").mkdir(parents=True)
    shutil.copyfile(cache / "save-relay-sim-target/aarch64-apple-ios-sim/release/libsave_relay.a",
                    folder / "lib/libsave_relay.a")
    shutil.copytree(source / "include", folder / "include")
    shutil.copytree(source / "bindings/ios", folder / "bindings/ios")
    return folder


def main(argv):
    simulator = "--sim" in argv
    args = [a for a in argv if a != "--sim"]
    if len(args) != 1:
        raise SystemExit(__doc__)
    component = json.loads(LOCK.read_text())["save-relay"]
    cache = Path(args[0]).resolve()
    folder = build_simulator(component, cache) if simulator else fetch_release(component, cache)
    print(folder)


if __name__ == "__main__":
    main(sys.argv[1:])
