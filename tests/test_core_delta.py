#!/usr/bin/env python3
"""Boundary and delta-isolation checks for gpu/shadps4 vs gpu/shim (#198)."""

from pathlib import Path
import re
import unittest

REPO_ROOT = Path(__file__).resolve().parent.parent
GPU_DIR = REPO_ROOT / "gpu"
SHADPS4_DIR = GPU_DIR / "shadps4"
BOUNDARY_HEADER = GPU_DIR / "shim" / "bbport_core_boundary.h"
CMAKE_LISTS = GPU_DIR / "CMakeLists.txt"

INCLUDE_RE = re.compile(r'^\s*#\s*include\s+"([^"]+)"', re.MULTILINE)
ALLOWED_LIST_RE = re.compile(
    r'AllowedShimHeaders\[\]\s*=\s*\{(.*?)\};', re.DOTALL
)
STRING_LITERAL_RE = re.compile(r'"([^"]+)"')


def parse_allowed_shim_headers() -> set[str]:
    text = BOUNDARY_HEADER.read_text(encoding="utf-8")
    match = ALLOWED_LIST_RE.search(text)
    if not match:
        raise AssertionError("Could not find AllowedShimHeaders[] in bbport_core_boundary.h")
    return set(STRING_LITERAL_RE.findall(match.group(1)))


class TestCoreDeltaBoundary(unittest.TestCase):
    def test_no_escaping_relative_includes_in_shadps4(self) -> None:
        """gpu/shadps4 must not use ../../../ includes that break BB_SHADPS4_CORE_DIR."""
        offenders: list[str] = []
        for path in sorted(SHADPS4_DIR.rglob("*")):
            if path.suffix not in {".h", ".hpp", ".c", ".cpp", ".inc"}:
                continue
            text = path.read_text(encoding="utf-8", errors="replace")
            for match in INCLUDE_RE.finditer(text):
                inc = match.group(1)
                if inc.startswith("../../../"):
                    offenders.append(f"{path.relative_to(REPO_ROOT)}: {inc}")
        self.assertEqual(offenders, [], f"Escaping relative includes found: {offenders}")

    def test_shim_includes_are_allowlisted(self) -> None:
        """Every bbport/shim boundary header included by gpu/shadps4 is in AllowedShimHeaders."""
        allowed = parse_allowed_shim_headers()
        unlisted: list[str] = []
        for path in sorted(SHADPS4_DIR.rglob("*")):
            if path.suffix not in {".h", ".hpp", ".c", ".cpp", ".inc"}:
                continue
            text = path.read_text(encoding="utf-8", errors="replace")
            for match in INCLUDE_RE.finditer(text):
                inc = match.group(1)
                is_boundary = (
                    inc == "bbgpu.h"
                    or inc.startswith("bbport_")
                    or inc.startswith("dlss_bridge/")
                    or inc in {"sdl_window.h", "vertex_input_deny.h", "equeue_request_gate.h"}
                )
                if is_boundary and inc not in allowed:
                    unlisted.append(f"{path.relative_to(REPO_ROOT)}: {inc}")
        self.assertEqual(unlisted, [], f"Unlisted boundary includes: {unlisted}")

    def test_cmake_uses_bb_shadps4_core_dir(self) -> None:
        """gpu/CMakeLists.txt must route all core/net paths through BB_SHADPS4_{CORE,NET}_DIR."""
        text = CMAKE_LISTS.read_text(encoding="utf-8")
        self.assertIn("BB_SHADPS4_CORE_DIR", text)
        self.assertIn("BB_SHADPS4_NET_DIR", text)
        hardcoded: list[str] = []
        for lineno, line in enumerate(text.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("#") or (stripped.startswith('"') and stripped.endswith('")')):
                continue
            if "shadps4" not in stripped:
                continue
            if "BB_SHADPS4_CORE_DIR" in stripped or "BB_SHADPS4_NET_DIR" in stripped:
                continue
            hardcoded.append(f"L{lineno}: {stripped}")
        self.assertEqual(hardcoded, [], f"Hardcoded shadps4 paths in CMakeLists.txt: {hardcoded}")


if __name__ == "__main__":
    unittest.main()
