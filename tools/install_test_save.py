#!/usr/bin/env python3
"""Installs a test save set from tests/fixtures/saves/ into a user directory.

    python tools/install_test_save.py bc-183 out/try/user-museum --slots 0CC 02P 023

copies SAVEDATA0CC, SAVEDATA02P and SAVEDATA023 (each with its .sce_sys folder) of the set
into <user>/savedata/1/CUSA02320/. Without --slots every save folder of the set is copied.
The sets come from other regions too (CUSA02343, CUSA02344, CUSA02826); the game reads them
under CUSA02320 the same way. Folders under .backup/ are skipped. An existing save folder is
kept unless --replace is given. Then run with DRIVE_USER=<user> (tools/drive.py) or --user.
`--list` prints each set's folders with the label the game wrote in param.bin.
"""

import argparse
import re
import shutil
import sys
from pathlib import Path

FIXTURES = Path(__file__).resolve().parent.parent / 'tests' / 'fixtures' / 'saves'
TITLE_ID = 'CUSA02320'
SAVE_DIR = re.compile(r'^SAVEDATA[0-9A-Z]{3}$')


def save_dirs(set_dir):
    """The set's save folders by name (SAVEDATA020 -> its path), skipping .backup copies."""
    found = {}
    for path in sorted(set_dir.rglob('SAVEDATA*')):
        if path.is_dir() and SAVE_DIR.match(path.name) and '.backup' not in path.parts:
            if path.name in found:
                sys.exit(f'{set_dir.name}: {path.name} appears twice ({found[path.name]} and {path})')
            found[path.name] = path
    return found


def label(save):
    """Title and subtitle the game wrote into param.bin (OrbisSaveDataParam), or ''."""
    param = save.with_name(save.name + '.sce_sys') / 'param.bin'
    if not param.is_file():
        return ''
    data = param.read_bytes()
    text = lambda start, size: data[start:start + size].split(b'\0')[0].decode('utf-8', 'replace')
    return f'{text(0, 128)} | {text(128, 128)}'


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('set', nargs='?', help='a folder name in tests/fixtures/saves')
    parser.add_argument('user', nargs='?', help='user directory to install into')
    parser.add_argument('--slots', nargs='+', help='save folder suffixes (023) or names (SAVEDATA023)')
    parser.add_argument('--replace', action='store_true', help='overwrite save folders already there')
    parser.add_argument('--list', action='store_true', help='list the sets and their save folders')
    args = parser.parse_args()
    sys.stdout.reconfigure(encoding='utf-8')

    if args.list:
        for set_dir in sorted(p for p in FIXTURES.iterdir() if p.is_dir()):
            if args.set and set_dir.name != args.set:
                continue
            print(set_dir.name)
            for name, path in save_dirs(set_dir).items():
                print(f'  {name}  {label(path)}')
        return 0
    if not args.set or not args.user:
        parser.error('give a set and a user directory, or --list')

    set_dir = FIXTURES / args.set
    if not set_dir.is_dir():
        parser.error(f'no set {args.set} in {FIXTURES}')
    saves = save_dirs(set_dir)
    wanted = list(saves)
    if args.slots:
        wanted = [s if s.startswith('SAVEDATA') else 'SAVEDATA' + s for s in args.slots]
        missing = [s for s in wanted if s not in saves]
        if missing:
            parser.error(f'{args.set} has no {", ".join(missing)} (it has {", ".join(saves)})')

    target = Path(args.user) / 'savedata' / '1' / TITLE_ID
    target.mkdir(parents=True, exist_ok=True)
    for name in wanted:
        source = saves[name]
        for src in (source, source.with_name(name + '.sce_sys')):
            if not src.is_dir():
                continue
            dst = target / src.name
            if dst.exists():
                if not args.replace:
                    print(f'kept {dst} (already there; --replace overwrites)')
                    continue
                shutil.rmtree(dst)
            shutil.copytree(src, dst)
        print(f'{name}  {label(source)}')
    print(f'installed into {target}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
