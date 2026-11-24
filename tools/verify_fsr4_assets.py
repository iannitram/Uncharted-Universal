#!/usr/bin/env python3
"""Verify FSR 4 v07 assets against their *_shader_manifest.json SHA-256 and size entries."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

MODELS = ("native", "quality", "balanced", "performance", "ultraperf", "drs")
SHARED_FILES = ("LICENSE-FSR4-v07.txt", "rcas.spv", "spd_auto_exposure.spv")


def expected_files(tiers: tuple[str, ...]) -> list[str]:
    files = list(SHARED_FILES)
    for model in MODELS:
        files.extend(
            (
                f"fsr4_model_v07_i8_{model}_initializers.bin",
                f"fsr4_model_v07_i8_{model}_pre_weights.bin",
                f"fsr4_model_v07_i8_{model}_shader_manifest.json",
            )
        )
        for tier in tiers:
            files.append(f"fsr4_model_v07_i8_{model}_{tier}_pre.spv")
            files.append(f"fsr4_model_v07_i8_{model}_{tier}_post.spv")
            for p in range(1, 13):
                files.append(f"fsr4_model_v07_i8_{model}_{tier}_pass{p}.spv")
    return files


def verify_dir(asset_dir: Path, tiers: tuple[str, ...] = ("1080", "2160")) -> list[str]:
    """Return a list of human-readable error strings; empty means all expected assets are valid."""
    errors: list[str] = []
    if not asset_dir.is_dir():
        return [f"missing asset directory: {asset_dir}"]

    required = set(expected_files(tiers))
    for name in sorted(required):
        path = asset_dir / name
        if not path.is_file() or path.stat().st_size == 0:
            errors.append(f"missing or empty: {name}")

    # Collect SHA-256 + size expectations from all model manifests.
    expected_artifacts: dict[str, tuple[int, str]] = {}
    for model in MODELS:
        manifest_name = f"fsr4_model_v07_i8_{model}_shader_manifest.json"
        manifest_path = asset_dir / manifest_name
        if not manifest_path.is_file():
            continue
        try:
            data = json.loads(manifest_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            errors.append(f"invalid manifest {manifest_name}: {exc}")
            continue
        artifacts = data.get("artifacts")
        if not isinstance(artifacts, dict):
            errors.append(f"invalid manifest {manifest_name}: missing artifacts object")
            continue
        for name, info in artifacts.items():
            if name not in required:
                continue
            size = info.get("size")
            sha256 = info.get("sha256")
            if not isinstance(size, int) or not isinstance(sha256, str):
                errors.append(f"invalid manifest entry {name} in {manifest_name}")
                continue
            prev = expected_artifacts.get(name)
            if prev is not None and prev != (size, sha256):
                errors.append(f"conflicting manifest entries for {name}")
                continue
            expected_artifacts[name] = (size, sha256)

    for name, (expected_size, expected_sha256) in sorted(expected_artifacts.items()):
        path = asset_dir / name
        if not path.is_file():
            continue
        raw = path.read_bytes()
        if len(raw) != expected_size:
            errors.append(f"size mismatch for {name}: got {len(raw)}, expected {expected_size}")
            continue
        digest = hashlib.sha256(raw).hexdigest()
        if digest != expected_sha256:
            errors.append(f"sha256 mismatch for {name}: got {digest}, expected {expected_sha256}")

    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "dir",
        nargs="?",
        default=os.environ.get("BB_FSR4_DIR") or "fsr4_shaders",
        help="Asset directory (default: BB_FSR4_DIR or fsr4_shaders)",
    )
    parser.add_argument(
        "--tiers",
        default=os.environ.get("BB_FSR4_TIERS") or "1080 2160",
        help="Space-separated resolution tiers to verify (default: '1080 2160')",
    )
    args = parser.parse_args(argv)
    tiers = tuple(t for t in args.tiers.split() if t)
    errors = verify_dir(Path(args.dir), tiers)
    if errors:
        for err in errors:
            print(f"FSR 4 asset error: {err}", file=sys.stderr)
        return 1
    print(f"FSR 4 assets verified: {len(expected_files(tiers))} files in {args.dir} (tiers: {' '.join(tiers)})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
