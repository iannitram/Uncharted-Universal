#!/usr/bin/env python3
"""Licence texts for the libraries a package bundles (#302).

check_licences() is the pure rule: every bundled library has an owning package and at least one
licence file. The two commands below resolve owners with the system package manager, copy the
licence files into the package, and exit 1 naming each library that breaks the rule.

  licence_check.py windows <licenses dir> <dll>...   MSYS2: pacman -Qqo, /clang64/share/licenses/<package>/
  licence_check.py linux <licenses dir> <library>... Debian: dpkg -S, /usr/share/doc/<package>/copyright
"""
import os
import shutil
import subprocess
import sys
from pathlib import Path

# A native (mingw) Python cannot read /clang64: package.sh passes the folder as a Windows path.
MSYS2_LICENSES = Path(os.environ.get('MSYS2_LICENSES', '/clang64/share/licenses'))
DEBIAN_DOCS = Path('/usr/share/doc')


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


def _windows_files(package):
    folder = MSYS2_LICENSES / package
    return sorted(p for p in folder.rglob('*') if p.is_file()) if folder.is_dir() else []


def _linux_files(package):
    copyright_file = DEBIAN_DOCS / package / 'copyright'
    return [copyright_file] if copyright_file.is_file() else []


def run(mode, licenses, libraries):
    owner, files = (_windows_owner, _windows_files) if mode == 'windows' else (_linux_owner, _linux_files)
    licenses = Path(licenses)
    entries, copies = [], []
    for library in libraries:
        package = owner(library)
        found = files(package) if package else []
        entries.append((Path(library).name, package, found))
        for source in found:
            if mode == 'windows':
                copies.append((source, licenses / package / source.relative_to(MSYS2_LICENSES / package)))
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
    if len(sys.argv) < 3 or sys.argv[1] not in ('windows', 'linux'):
        sys.exit(__doc__)
    sys.exit(run(sys.argv[1], sys.argv[2], sys.argv[3:]))
