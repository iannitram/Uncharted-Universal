#!/bin/sh
# Build and run the synthetic no-JIT test with the default (no interpreter) backend, or with
# BACKEND_SRC=<file.c> implementing nojit_abi.h. Report goes to $1 (default nojit-report.json).
set -eu
d=$(dirname "$0")
out=${1:-nojit-report.json}
shift 2>/dev/null || true
cc -std=c11 -O2 -Wall -Wextra -Werror -Wno-missing-field-initializers \
  "$d/test_nojit.c" "${BACKEND_SRC:-$d/backend_none.c}" -o "${TMPDIR:-/tmp}/nojit_test"
"${TMPDIR:-/tmp}/nojit_test" "$out" "$@"
