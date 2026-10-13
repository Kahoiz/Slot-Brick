#!/bin/sh
BIN=/usr/trimui/bin
ROOT=/mnt/SDCARD/Slot
mkdir -p "$ROOT/Logs" || exit 1
exec >> "$ROOT/Logs/restore.log" 2>&1
echo "Restore slot startup hook: $(date)"
if ! grep -q '^# SLOT_BRICK_BOOT_V[12]$' "$BIN/runtrimui.sh"; then
    echo 'Slot does not own the active startup script; no changes made.'
    exit 1
fi
BACKUP="$BIN/runtrimui-slot-backup.sh"
[ -s "$BACKUP" ] || { echo 'Backup missing; no changes made.'; exit 1; }
NEW="$BIN/runtrimui-slot-restore-$$.sh"
cp -p "$BACKUP" "$NEW" || exit 1
"$ROOT/System/slot-filecheck" compare "$BACKUP" "$NEW" || exit 1
mv -f "$NEW" "$BIN/runtrimui.sh" || exit 1
printf 'autoboot disabled\n' > "$ROOT/DISABLE_AUTOBOOT"
sync
echo 'Previous startup restored. Reboot normally when ready. Backup retained.'
