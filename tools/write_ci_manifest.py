#!/usr/bin/env python3
"""Write a package manifest and SHA-256 list for CI artifacts."""

import argparse
import hashlib
import json
import platform
from pathlib import Path


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--smoke-result", type=Path, required=True)
    parser.add_argument("--dependencies", type=Path)
    args = parser.parse_args()
    directory = args.directory.resolve()
    smoke = args.smoke_result.resolve()
    if not directory.is_dir():
        parser.error(f"not a directory: {directory}")
    if not smoke.is_file() or not smoke.stat().st_size:
        parser.error(f"missing or empty smoke result: {smoke}")
    if "FAIL" in smoke.read_text(encoding="utf-8", errors="replace").upper():
        parser.error(f"smoke result reports failure: {smoke}")

    files = sorted(
        path for path in directory.rglob("*")
        if path.is_file() and path.name not in {"manifest.json", "SHA256SUMS"}
    )
    if not files:
        parser.error(f"no package files in {directory}")
    entries = [
        {"path": path.relative_to(directory).as_posix(), "size": path.stat().st_size, "sha256": sha256(path)}
        for path in files
    ]
    dependencies = {}
    if args.dependencies:
        dependencies = json.loads(args.dependencies.read_text(encoding="utf-8"))
        if not isinstance(dependencies, dict):
            parser.error("dependencies file must contain a JSON object")
    manifest = {
        "schema": 1,
        "revision": args.revision,
        "platform": args.platform,
        "runner_os": platform.platform(),
        "dependencies": dependencies,
        "vulkan_smoke": {"result": "passed", "file": smoke.name},
        "artifacts": entries,
    }
    (directory / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    checksum_file = directory / "SHA256SUMS"
    checksum_file.write_text(
        "".join(f"{entry['sha256']}  {entry['path']}\n" for entry in entries), encoding="utf-8"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
