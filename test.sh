
#!/usr/bin/env bash

# G102-Lab diagnostic test runner.
# No physical onboard flash writes are performed.

set -u

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BIN="$SCRIPT_DIR/target/debug/g102-control"
BACKUP="$SCRIPT_DIR/g102-backup.obk"

if [[ ! -x "$BIN" ]]; then
    echo "ERROR: Binary not found: $BIN"
    echo "Run 'cargo build' from the project first."
    exit 1
fi

if [[ ! -f "$BACKUP" ]]; then
    echo "ERROR: Backup not found: $BACKUP"
    exit 1
fi

if ! sudo -v; then
    echo "ERROR: sudo authentication failed."
    exit 1
fi

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/g102-test.XXXXXX")"
trap 'rm -rf -- "$TMP_DIR"' EXIT

OUT="$TMP_DIR/remapped-profile.obk"

run() {
    local heading="$1"
    shift

    printf '\n\n========== %s ==========\n' "$heading"
    printf '$'
    printf ' %q' "$@"
    printf '\n\n'

    "$@"
    local status=$?

    printf '\n[exit code: %s]\n' "$status"
    return 0
}

device() {
    local heading="$1"
    shift
    run "$heading" sudo "$BIN" "$@"
}

echo "G102-Lab diagnostic test"
echo "Binary: $BIN"
echo "Backup: $BACKUP"
echo "Temporary output: $OUT"
echo "Physical flash writes: DISABLED"

# CLI
run "HELP" "$BIN" help

# Hardware inspection
device "DEVICE INSPECT" inspect

device "DPI GET" dpi get
device "POLLING RATE GET" rate get
device "POLLING RATE LIST" rate list

device "RGB INFO" rgb info
device "RGB EFFECTS" rgb effects

device "ONBOARD INFO" onboard info
device "ONBOARD PROFILES" onboard profiles

# Offline profile operations
run "ONBOARD BUTTONS" \
    "$BIN" onboard buttons "$BACKUP"

run "OFFLINE BUTTON REMAP" \
    "$BIN" onboard remap "$BACKUP" "$OUT" \
    --slot 6 --action left

run "REMAPPED PROFILE BUTTONS" \
    "$BIN" onboard buttons "$OUT"

run "BACKUP DIFFERENCE" \
    "$BIN" onboard diff "$BACKUP" "$OUT"

# Physical write safety checks — dry-run only
device "SET BUTTON SAFETY CHECK" \
    onboard set-button 6 left --dry-run

device "RESTORE SAFETY CHECK" \
    onboard restore "$BACKUP" --dry-run

echo
echo "========== TEST RUN COMPLETE =========="
echo "No physical onboard flash writes were requested."
