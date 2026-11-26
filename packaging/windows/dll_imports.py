#!/usr/bin/env python3
"""The DLLs Windows executables import, read with `llvm-objdump -p` (#625).

  dll_imports.py closure <dir>... -- <exe>... every non-system DLL the executables need, directly
                                                   or through another DLL, each from the first folder
                                                   that has it; exit 1 naming any found in none
  dll_imports.py check <package dir> [--crt <dir>]  every non-system import of every .exe and .dll in the
                                                   package is in that file's own folder. With --crt, a
                                                   missing Microsoft C++ runtime DLL (VCRUNTIME140.dll for
                                                   save-relay.exe) is copied from that Visual Studio
                                                   Redist folder first; exit 1 naming each import left missing

A system DLL is an API set (api-ms-*, ext-ms-*) or a file in Windows' System32 that is not one of
the redistributable C++ runtimes (which a clean Windows does not have).
"""
import os
import shutil
import subprocess
import sys
from pathlib import Path

SYSTEM32 = Path(os.environ.get('SystemRoot', 'C:/Windows')) / 'System32'
# Installed by the Visual C++ Redistributable (or other programs), not by Windows itself.
REDISTRIBUTABLE_PREFIXES = ('vcruntime', 'msvcp', 'concrt', 'vccorlib', 'vcomp', 'vcamp', 'mfc', 'ucrtbased')
CRT_NOTICE = 'Microsoft-Visual-C++-runtime-NOTICE.txt'


def parse_imports(text):
    """DLL names in `llvm-objdump -p` output (import and delay-import tables), in order, once each."""
    names = []
    for line in text.splitlines():
        line = line.strip()
        if line.startswith('DLL Name:'):
            name = line[len('DLL Name:'):].strip()
            if name and name.lower() not in (n.lower() for n in names):
                names.append(name)
    return names


def imports(path):
    result = subprocess.run(['llvm-objdump', '-p', str(path)], capture_output=True, text=True)
    if result.returncode != 0:
        raise SystemExit(f'llvm-objdump -p {path} failed: {result.stderr.strip()}')
    return parse_imports(result.stdout)


def is_system(name, system32=SYSTEM32):
    lower = name.lower()
    if lower.startswith(('api-ms-', 'ext-ms-')):
        return True
    if lower.startswith(REDISTRIBUTABLE_PREFIXES):
        return False
    return (Path(system32) / name).is_file()


def closure(exes, dirs, read=imports, system=is_system):
    """(resolved DLL paths in discovery order, names found in no folder)."""
    found, missing, seen = [], [], set()
    queue = list(exes)
    while queue:
        for name in read(queue.pop(0)):
            key = name.lower()
            if key in seen or system(name):
                continue
            seen.add(key)
            path = next((Path(d) / name for d in dirs if (Path(d) / name).is_file()), None)
            if path is None:
                missing.append(name)
                continue
            found.append(path)
            queue.append(path)
    return found, missing


def missing_imports(root, read=imports, system=is_system):
    """{(PE file relative to root): [imports absent from its folder]} for every .exe and .dll."""
    root = Path(root)
    problems = {}
    for pe in sorted(p for p in root.rglob('*') if p.suffix.lower() in ('.exe', '.dll') and p.is_file()):
        present = {p.name.lower() for p in pe.parent.iterdir()}
        absent = [n for n in read(pe) if not system(n) and n.lower() not in present]
        if absent:
            problems[pe.relative_to(root).as_posix()] = absent
    return problems


def fill_crt(root, crt, problems):
    """Copies the absent imports that the Redist folder holds; returns the names copied."""
    copied = []
    available = {p.name.lower(): p for p in Path(crt).iterdir()} if crt and Path(crt).is_dir() else {}
    for pe, absent in problems.items():
        folder = (Path(root) / pe).parent
        for name in absent:
            source = available.get(name.lower())
            if source and not (folder / source.name).exists():
                shutil.copyfile(source, folder / source.name)
                copied.append(f'{(folder / source.name).relative_to(root).as_posix()}')
    return copied


CRT_NOTICE_TEXT = """Microsoft Visual C++ runtime ({files})

These files are Microsoft's Visual C++ runtime, copied unmodified from the Visual Studio
Redist folder ({crt}) for app-local deployment next to the program that imports them
(save-relay.exe). Microsoft lists them as Distributable Code in the Visual Studio licence terms
(REDIST.TXT), which permits distributing them with an application. They remain Microsoft's
property and are not covered by this project's GPL-2.0-only licence or by Save Relay's MIT licence.
See https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files
"""


def main(argv):
    if argv[:1] == ['closure'] and '--' in argv[2:-1]:
        cut = argv.index('--')
        dirs, exes = argv[1:cut], argv[cut + 1:]
        sys.stdout.reconfigure(newline='\n')  # the list is read by bash: no CR on Windows
        found, missing = closure([Path(p) for p in exes], dirs)
        for path in found:
            print(Path(path).as_posix())
        for name in missing:
            print(f'dll imports: {name} is needed but is in none of {", ".join(dirs)}', file=sys.stderr)
        return 1 if missing else 0
    if len(argv) in (2, 4) and argv[0] == 'check' and (len(argv) == 2 or argv[2] == '--crt'):
        root, crt = Path(argv[1]), argv[3] if len(argv) == 4 else None
        problems = missing_imports(root)
        copied = fill_crt(root, crt, problems) if crt else []
        if copied:
            print(f'dll imports: app-local C++ runtime copied: {", ".join(copied)}')
            licenses = root / 'licenses'
            licenses.mkdir(exist_ok=True)
            (licenses / CRT_NOTICE).write_text(CRT_NOTICE_TEXT.format(
                files=', '.join(sorted({Path(c).name for c in copied})), crt=Path(crt).as_posix()))
            problems = missing_imports(root)
        for pe, absent in problems.items():
            for name in absent:
                hint = ''
                if name.lower().startswith(REDISTRIBUTABLE_PREFIXES):
                    hint = (' (the Microsoft C++ runtime: install the Visual Studio 2022 C++ build tools, whose'
                            ' VC/Redist/MSVC/<version>/x64/Microsoft.VC143.CRT folder package.sh copies it from,'
                            ' or set BB_VC_CRT to that folder)')
                print(f'dll imports: {pe} imports {name}, which is not in its folder{hint}', file=sys.stderr)
        return 1 if problems else 0
    sys.exit(__doc__)


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
