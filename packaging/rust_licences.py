#!/usr/bin/env python3
"""The licences of the Rust crates linked into the shipped programs (roadmap 2.10).

The release packages carry `uncharted-app`, `uncharted-universal` and `uncharted-run`; the
crates statically linked into them (Slint, gilrs and the rest) ask for their licence texts to
travel with the binaries. This writes one text file: every crate the three programs link, on
the platform being packaged, with its version, licence expression and repository, then each
distinct licence text once, with the crates that ship it. A crate whose published package
has no licence file is listed under its Cargo.toml licence expression.

    python3 packaging/rust_licences.py <output file> [--target <triple>]

It reads `cargo metadata` (so run it after the release build: the crate sources are then on
disk). Workspace members are this repository (GPL-2.0-only, LICENSE) and are left out.
"""

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The packages whose programs ship, by Cargo package name.
SHIPPED = ("uncharted-app", "uncharted_cli", "uncharted_runtime")

# Files a crate ships its licence or notice in (matched on the name's start, any case).
LICENCE_PREFIXES = ("licence", "license", "copying", "notice", "unlicense", "copyright")


def linked_packages(metadata, roots=SHIPPED):
    """The ids of every package reachable from `roots` through normal (linked) dependencies,
    proc macros left out. Build scripts and proc macros run at build time and ship nothing, so
    build and dev dependencies are not followed."""
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    names = {package["id"]: package["name"] for package in metadata["packages"]}
    members = set(metadata["workspace_members"])
    proc_macros = {
        package["id"] for package in metadata["packages"]
        if any("proc-macro" in target.get("kind", []) for target in package.get("targets", []))
    }
    start = [pid for pid in members if names.get(pid) in roots]
    missing = set(roots) - {names[pid] for pid in start}
    if missing:
        raise ValueError(f"not workspace packages: {', '.join(sorted(missing))}")
    seen, todo = set(), list(start)
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        # A proc macro runs in the compiler: it and what it links ship nothing.
        if pid in proc_macros:
            continue
        for dep in nodes.get(pid, {}).get("deps", []):
            kinds = dep.get("dep_kinds") or [{"kind": None}]
            if any(kind.get("kind") is None for kind in kinds):
                todo.append(dep["pkg"])
    return seen - members - proc_macros


def licence_files(manifest_dir):
    """The licence and notice files at the top of a crate's folder, sorted by name."""
    folder = Path(manifest_dir)
    if not folder.is_dir():
        return []
    return sorted(
        (path for path in folder.iterdir()
         if path.is_file() and path.name.lower().startswith(LICENCE_PREFIXES)),
        key=lambda path: path.name.lower(),
    )


def render(metadata, ids):
    """The text of the licence file for the packages `ids`."""
    packages = sorted(
        (package for package in metadata["packages"] if package["id"] in ids),
        key=lambda package: (package["name"], package["version"]),
    )
    lines = [
        "Rust crates linked into uncharted-app, uncharted-universal and uncharted-run",
        "",
        "Uncharted Universal itself is GPL-2.0-only (LICENSE). These crates are linked into",
        "its programs; their licences follow. Each distinct licence text appears once, after",
        "the crates that ship it.",
        "",
    ]
    texts = {}  # digest -> (text, [crate labels])
    no_file = []
    for package in packages:
        label = f"{package['name']} {package['version']}"
        expression = package.get("license") or (
            f"see {package['license_file']}" if package.get("license_file") else "not stated")
        repository = package.get("repository") or package.get("homepage") or ""
        lines.append(f"{label}: {expression}" + (f" ({repository})" if repository else ""))
        files = licence_files(Path(package["manifest_path"]).parent)
        if not files:
            no_file.append(label)
        for path in files:
            text = path.read_text(encoding="utf-8", errors="replace").strip() + "\n"
            digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
            texts.setdefault(digest, (text, []))[1].append(f"{label} ({path.name})")
    if no_file:
        lines += ["", "No licence file in the published crate (licence as its Cargo.toml states, above):"]
        lines += [f"  {label}" for label in no_file]
    for text, crates in sorted(texts.values(), key=lambda item: item[1][0]):
        lines += ["", "=" * 78, "Shipped by:"] + [f"  {crate}" for crate in crates]
        lines += ["-" * 78, text.rstrip("\n")]
    return "\n".join(lines) + "\n"


def cargo_metadata(target):
    command = ["cargo", "metadata", "--format-version", "1", "--locked", "--manifest-path",
               str(ROOT / "Cargo.toml")]
    if target:
        command += ["--filter-platform", target]
    # Bytes, decoded as UTF-8: cargo writes UTF-8 whatever the console's code page is.
    return json.loads(subprocess.run(command, check=True, capture_output=True).stdout.decode("utf-8"))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("output", type=Path)
    parser.add_argument("--target", help="the Rust target triple being packaged (default: the host)")
    args = parser.parse_args(argv)
    target = args.target
    if not target:
        host = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True).stdout
        target = next(line.split(":", 1)[1].strip() for line in host.splitlines()
                      if line.startswith("host:"))
    metadata = cargo_metadata(target)
    ids = linked_packages(metadata)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(render(metadata, ids), encoding="utf-8", newline="\n")
    print(f"Rust crate licences: {len(ids)} crates in {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
