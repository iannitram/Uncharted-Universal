#!/usr/bin/env python3
"""Run the Python suite and reject skips not listed for the current CI platform."""

import argparse
import json
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SKIPS = Path(__file__).with_name("expected-test-skips.json")


class Result(unittest.TextTestResult):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.skipped_ids = set()

    def addSkip(self, test, reason):
        super().addSkip(test, reason)
        self.skipped_ids.add(test.id())


class Runner(unittest.TextTestRunner):
    resultclass = Result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=("linux", "windows"), required=True)
    args = parser.parse_args()
    expected = json.loads(SKIPS.read_text(encoding="utf-8"))[args.platform]
    if len(expected) != len(set(expected)):
        parser.error(f"duplicate entries in {SKIPS}")

    sys.path.insert(0, str(ROOT))
    suite = unittest.defaultTestLoader.discover(str(ROOT / "tests"))
    result = Runner(stream=sys.stdout, verbosity=1).run(suite)
    unexpected = sorted(result.skipped_ids - set(expected))
    if unexpected:
        print("Unexpected unittest skips:", file=sys.stderr)
        for test_id in unexpected:
            print(f"  {test_id}", file=sys.stderr)
    else:
        print(f"{len(result.skipped_ids)} unittest skip(s), all explicitly expected on {args.platform}")
    return 0 if result.wasSuccessful() and not unexpected else 1


if __name__ == "__main__":
    raise SystemExit(main())
