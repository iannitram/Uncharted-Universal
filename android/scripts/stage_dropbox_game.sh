#!/usr/bin/env bash
# Stages and uploads per-game files as a single resumable game.tar + manifest.json
# for Stage 7.04 (#525) in-app Dropbox downloads into Save Relay's Dropbox app folder
# (uncharted-universal-game/<game>/).
# Only for the owner's own Dropbox: never commit game files or manifests to git (M9).
set -euo pipefail

GAME=""
DUMP_DIR=""
RCLONE_DEST="dropbox:Apps/save-relay"
LOCAL_DEST=""
RCLONE_BIN=""
DRY_RUN=0

usage() {
    cat <<'EOF'
Usage: stage_dropbox_game.sh --game <u1|u2|u3> --dump <path> [options]

Options:
  --game <u1|u2|u3>         Which game to stage (required)
  --dump <path>             Path to extracted Nathan Drake Collection dump (required)
  --rclone-dest <remote>    rclone destination root (default: dropbox:Apps/save-relay)
  --local-dest <dir>        Stage to a local directory instead of rclone
  --rclone <path>           Path to rclone binary (auto-detected if omitted)
  --dry-run                 Build game.tar + manifest.json and print size without uploading
  -h, --help                Show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --game) GAME="$2"; shift 2 ;;
        --dump) DUMP_DIR="$2"; shift 2 ;;
        --rclone-dest) RCLONE_DEST="$2"; shift 2 ;;
        --local-dest) LOCAL_DEST="$2"; shift 2 ;;
        --rclone) RCLONE_BIN="$2"; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

case "$GAME" in
    u1) EXEC_FILE="eboot.bin"; DATA_DIR="u1data" ;;
    u2) EXEC_FILE="big2-ps4_Shipping.elf"; DATA_DIR="u2data" ;;
    u3) EXEC_FILE="big3-ps4_Shipping.elf"; DATA_DIR="u3data" ;;
    *) echo "Invalid or missing --game (expected u1, u2, or u3)" >&2; usage >&2; exit 2 ;;
esac

if [[ -z "$DUMP_DIR" || ! -d "$DUMP_DIR" ]]; then
    echo "Invalid or missing --dump directory: $DUMP_DIR" >&2
    exit 2
fi
DUMP_DIR="$(cd "$DUMP_DIR" && pwd)"

if [[ -z "$LOCAL_DEST" ]]; then
    if [[ -z "$RCLONE_BIN" ]]; then
        if command -v rclone >/dev/null 2>&1; then
            RCLONE_BIN="$(command -v rclone)"
        elif [[ -x "$HOME/.local/opt/rclone/rclone" ]]; then
            RCLONE_BIN="$HOME/.local/opt/rclone/rclone"
        else
            echo "rclone not found on PATH or at \$HOME/.local/opt/rclone/rclone" >&2
            exit 1
        fi
    fi
fi

for req in "$EXEC_FILE" "$DATA_DIR"; do
    if [[ ! -e "$DUMP_DIR/$req" ]]; then
        echo "Missing required dump entry: $DUMP_DIR/$req" >&2
        exit 1
    fi
done

STAGE_BASE="${STAGE_TMP:-$HOME/.cache/uncharted-stage}"
mkdir -p "$STAGE_BASE"
WORK_DIR="$(mktemp -d "$STAGE_BASE/${GAME}.XXXXXX")"
STAGE_DIR="$WORK_DIR/links"
TAR_PATH="$WORK_DIR/game.tar"
MANIFEST_PATH="$WORK_DIR/manifest.json"
mkdir -p "$STAGE_DIR"
cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

for item in "$EXEC_FILE" "sce_discmap.plt" "sce_module" "shareddata" "$DATA_DIR"; do
    if [[ -e "$DUMP_DIR/$item" ]]; then
        ln -s "$DUMP_DIR/$item" "$STAGE_DIR/$item"
    fi
done

echo "== Packing $GAME into $TAR_PATH"
tar -chf "$TAR_PATH" --format=gnu -C "$STAGE_DIR" .

echo "== Hashing $GAME archive"
python3 - "$TAR_PATH" "$MANIFEST_PATH" <<'PY'
import hashlib, json, pathlib, sys

tar_path = pathlib.Path(sys.argv[1])
manifest_path = pathlib.Path(sys.argv[2])
h = hashlib.sha256()
size = 0
with tar_path.open("rb") as fh:
    while True:
        chunk = fh.read(8 * 1024 * 1024)
        if not chunk:
            break
        size += len(chunk)
        h.update(chunk)

manifest_path.write_text(json.dumps({
    "version": 1,
    "files": [{"path": "game.tar", "size": size, "sha256": h.hexdigest()}],
}, separators=(",", ":")))
print(f"Manifest ready: game.tar ({size / (1024 ** 3):.2f} GiB, sha256={h.hexdigest()[:12]}...)")
PY

if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "== Dry run complete (no files uploaded)"
    exit 0
fi

if [[ -n "$LOCAL_DEST" ]]; then
    DEST_DIR="$LOCAL_DEST/uncharted-universal-game/$GAME"
    mkdir -p "$DEST_DIR"
    cp "$TAR_PATH" "$DEST_DIR/game.tar"
    cp "$MANIFEST_PATH" "$DEST_DIR/manifest.json"
    chmod 644 "$DEST_DIR/manifest.json"
    echo "== Staged locally to $DEST_DIR"
    exit 0
fi

REMOTE_TARGET="${RCLONE_DEST%/}/uncharted-universal-game/$GAME"
echo "== Uploading game.tar to $REMOTE_TARGET/game.tar"
"$RCLONE_BIN" copyto "$TAR_PATH" "$REMOTE_TARGET/game.tar" \
    --dropbox-chunk-size 128M \
    --stats 15s \
    --stats-one-line

echo "== Uploading manifest.json last to $REMOTE_TARGET/manifest.json"
"$RCLONE_BIN" copyto "$MANIFEST_PATH" "$REMOTE_TARGET/manifest.json"
echo "== Upload complete: $REMOTE_TARGET"
