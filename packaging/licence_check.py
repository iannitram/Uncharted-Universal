#!/usr/bin/env python3
"""Licence texts for the libraries a package bundles (#302).

check_licences() is the pure rule: every bundled library has an owning package and at least one
licence file. The two commands below resolve owners with the system package manager, copy the
licence files into the package, and exit 1 naming each library that breaks the rule.

  licence_check.py windows <licenses dir> <dll>...   MSYS2: pacman -Qqo, then the files `pacman -Ql` lists under
                                                     /clang64/share/licenses/ (the folder is not always named
                                                     after the package); if none, packaging/windows/licenses/<short name>-LICENSE.txt
  licence_check.py linux <licenses dir> <library>... Debian: dpkg -S, /usr/share/doc/<package>/copyright
  licence_check.py apple [<apple dir>]               the tree vendored for the Apple build (ADR 0006, #396): no GPL-3.0
                                                     family or bbnet20 path or text, no Apache-2.0, and every
                                                     apple/vendor/<component> has a VENDOR.txt and a LICENSE
"""
import os
import shutil
import subprocess
import sys
from pathlib import Path

# A native (mingw) Python cannot read /clang64: package.sh passes the folder as a Windows path.
MSYS2_LICENSES = Path(os.environ.get('MSYS2_LICENSES', '/clang64/share/licenses'))
DEBIAN_DOCS = Path('/usr/share/doc')
# Texts kept in the repo for packages whose MSYS2 install ships no licence file.
FALLBACK_DIR = Path(__file__).resolve().parent / 'windows' / 'licenses'
PACMAN_PREFIX = 'mingw-w64-clang-x86_64-'
LICENSE_ROOT = '/clang64/share/licenses/'


FFMPEG_STEMS = ('avcodec', 'avdevice', 'avfilter', 'avformat', 'avutil', 'postproc', 'swresample', 'swscale')
_GPL3_MARKERS = ('gplv3', 'gpl-3.0', 'gnu general public license\n                       version 3', 'version 3, 29 june 2007')


def is_ffmpeg_library(name):
    lower = Path(name).name.lower()
    return any(lower.startswith(stem) or lower.startswith(f'lib{stem}') for stem in FFMPEG_STEMS)


def check_ffmpeg_compatibility(entries):
    """Fail closed when a bundled FFmpeg library carries GPL-3.0 / LGPL-3.0 notices (#299)."""
    problems = []
    for name, _package, files in entries:
        if not is_ffmpeg_library(name):
            continue
        for item in files:
            path = Path(item)
            lower_name = path.name.lower()
            text = path.read_text(encoding='utf-8', errors='ignore').lower() if path.is_file() else ''
            if any(m in lower_name for m in ('gplv3', 'gpl-3.0')) or any(m in text for m in _GPL3_MARKERS):
                problems.append(f'{name}: FFmpeg licence {path.name} is GPL-3.0/LGPL-3.0 (incompatible with GPL-2.0-only)')
                break
    return problems


def check_licences(entries):
    """entries: (library file name, owning package or None, licence files found).

    Returns one message per problem; an empty list means the package may ship.
    """
    problems = []
    for name, package, files in entries:
        if not package:
            problems.append(f'{name}: no owning package found')
        elif not files:
            problems.append(f'{name}: package {package} has no licence files')
    return problems


# What may never be under apple/ (feasibility-211 allowlist): the workspace is GPL-2.0-only, and
# gpu/bbnet20 (GPL-3.0) is excluded. Apache-2.0 is incompatible with GPL-2.0-only.
APPLE_FORBIDDEN_DIRS = ('bbnet20',)
APPLE_FORBIDDEN_NAMES = ('gpl-3', 'gplv3', 'agpl', 'lgpl-3', 'lgplv3', 'bbnet20')
APPLE_FORBIDDEN_TEXT = ('spdx-license-identifier: gpl-3', 'spdx-license-identifier: agpl-3',
                        'spdx-license-identifier: lgpl-3', 'spdx-license-identifier: apache-2.0',
                        'gnu general public license\n                       version 3', 'version 3, 29 june 2007',
                        'apache license\n                           version 2.0', 'bbnet20')
APPLE_TEXT_SUFFIXES = ('.c', '.h', '.inc', '.s', '.m', '.mm', '.rs', '.txt', '.md', '.toml', '.py', '.sh', '')


def check_apple_tree(root):
    """Problems in the tree vendored for the Apple build; an empty list means it is allowed.

    Fails on any path with a bbnet20 component or a GPL-3.0-family / AGPL / LGPL-3.0 / bbnet20 name, on
    GPL-3.0-family or Apache-2.0 licence text or SPDX lines in a text file, and on a vendored component
    (a folder of apple/vendor) with no VENDOR.txt or no LICENSE file.
    """
    root = Path(root)
    problems = []
    if not root.is_dir():
        return [f'{root}: not a folder']
    vendor = root / 'vendor'
    if vendor.is_dir():
        for component in sorted(p for p in vendor.iterdir() if p.is_dir()):
            if not (component / 'VENDOR.txt').is_file():
                problems.append(f'{component.relative_to(root)}: no VENDOR.txt (upstream URL, revision, files, licence)')
            if not any((component / name).is_file() for name in ('LICENSE', 'LICENSE.txt', 'COPYING')):
                problems.append(f'{component.relative_to(root)}: no LICENSE file')
    for path in sorted(root.rglob('*')):
        relative = path.relative_to(root)
        parts = [part.lower() for part in relative.parts]
        if any(part in APPLE_FORBIDDEN_DIRS for part in parts) or \
                (path.is_file() and any(mark in path.name.lower() for mark in APPLE_FORBIDDEN_NAMES)):
            problems.append(f'{relative}: GPL-3.0 or bbnet20 path under apple/')
            continue
        if path.is_file() and path.suffix.lower() in APPLE_TEXT_SUFFIXES and path.stat().st_size < 4_000_000:
            text = path.read_text(encoding='utf-8', errors='ignore').lower()
            hit = next((mark for mark in APPLE_FORBIDDEN_TEXT if mark in text), None)
            if hit and not _is_policy_text(path, relative):
                problems.append(f'{relative}: contains {hit.strip().splitlines()[0]!r}')
    return problems


def _is_policy_text(path, relative):
    """A VENDOR.txt may say what it excludes (it names bbnet20 and GPL-3.0 to say they are not here)."""
    return path.name == 'VENDOR.txt'


def _owner(command):
    result = subprocess.run(command, capture_output=True, text=True)
    lines = result.stdout.split('\n') if result.returncode == 0 else []
    return next((line for line in lines if line.strip()), '').strip() or None


def _windows_owner(path):
    return _owner(['pacman', '-Qqo', str(path)])


def _linux_owner(path):
    # "libfoo1:amd64: /usr/lib/x86_64-linux-gnu/libfoo.so.1" -> "libfoo1"
    owner = _owner(['dpkg', '-S', os.path.realpath(path)])
    return owner.split(':')[0] if owner else None


def short_name(package):
    """mingw-w64-clang-x86_64-ffmpeg -> ffmpeg"""
    return package[len(PACMAN_PREFIX):] if package.startswith(PACMAN_PREFIX) else package


def licence_paths(listing):
    """Licence files (paths relative to the licenses root) in `pacman -Ql <package>` output."""
    found = []
    for line in listing.splitlines():
        parts = line.split(None, 1)
        if len(parts) == 2 and parts[1].startswith(LICENSE_ROOT) and not parts[1].endswith('/'):
            found.append(parts[1][len(LICENSE_ROOT):].strip())
    return sorted(found)


def _windows_files(package):
    """(source, path under licenses/<package>/) pairs: pacman's listed files, else the repo fallback."""
    result = subprocess.run(['pacman', '-Ql', package], capture_output=True, text=True)
    listed = licence_paths(result.stdout) if result.returncode == 0 else []
    pairs = [(MSYS2_LICENSES / rel, Path(*Path(rel).parts[1:]) if len(Path(rel).parts) > 1 else Path(rel))
             for rel in listed]
    pairs = [(source, target) for source, target in pairs if source.is_file()]
    if pairs:
        return pairs
    fallback = FALLBACK_DIR / f'{short_name(package)}-LICENSE.txt'
    return [(fallback, Path(fallback.name))] if fallback.is_file() else []


def _linux_files(package):
    copyright_file = DEBIAN_DOCS / package / 'copyright'
    return [copyright_file] if copyright_file.is_file() else []


def run(mode, licenses, libraries):
    owner, files = (_windows_owner, _windows_files) if mode == 'windows' else (_linux_owner, _linux_files)
    licenses = Path(licenses)
    entries, copies = [], []
    for library in filter(None, libraries):  # ldd with several inputs leaves blank lines
        package = owner(library)
        found = files(package) if package else []
        entries.append((Path(library).name, package, found))
        for source in found:
            if mode == 'windows':
                source, target = source
                copies.append((source, licenses / package / target))
            else:
                copies.append((source, licenses / f'{package}-copyright.txt'))
    problems = check_licences(entries)
    for source, target in copies:
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    for problem in problems:
        print(f'licence check: {problem}', file=sys.stderr)
    return 1 if problems else 0


if __name__ == '__main__':
    if sys.argv[1:2] == ['apple'] and len(sys.argv) <= 3:
        found = check_apple_tree(sys.argv[2] if len(sys.argv) == 3 else Path(__file__).resolve().parent.parent / 'apple')
        for problem in found:
            print(f'licence check: {problem}', file=sys.stderr)
        sys.exit(1 if found else 0)
    if len(sys.argv) < 3 or sys.argv[1] not in ('windows', 'linux'):
        sys.exit(__doc__)
    sys.exit(run(sys.argv[1], sys.argv[2], sys.argv[3:]))
