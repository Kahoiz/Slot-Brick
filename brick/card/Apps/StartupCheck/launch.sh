#!/bin/sh
# Startup Check: read-only report on why the Brick did or did not start Slot.
# Changes nothing on the device. Result: startup-report.txt at the top of the card
# (also Apps/StartupCheck/report.txt and Slot/Logs/startup-check.txt).
CARD=${SLOT_CHECK_CARD:-/mnt/SDCARD}
BIN=${SLOT_CHECK_BIN:-/usr/trimui/bin}
APP=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
OUT=/tmp/slot-startup-check.txt
ROOT="$CARD/Slot"
MARK='^# SLOT_BRICK_BOOT_V[12]$'
show() { [ -e "$1" ] && { echo "### $1 ($(wc -c < "$1") bytes)"; tail -n "${2:-40}" "$1"; } || echo "### $1: MISSING"; }
yesno() { if [ -e "$1" ]; then echo "present"; else echo "MISSING"; fi; }
{
    echo 'Slot startup check v4'
    t="$CARD/.slot-write-test"; if ( : > "$t" ) 2>/dev/null; then rm -f "$t"; echo 'SD card writable: yes'; else echo 'SD card writable: NO (read-only or damaged)'; fi
    date; uptime
    cat /etc/version 2>/dev/null || cat /usr/trimui/version 2>/dev/null || :
    echo
    echo '=== Brick startup script'
    S="$BIN/runtrimui.sh"; B="$BIN/runtrimui-slot-backup.sh"
    if [ -e "$S" ]; then
        ls -l "$S"
        if grep -q "$MARK" "$S"; then echo 'RESULT: Slot startup hook IS installed'; else echo 'RESULT: Slot startup hook is NOT installed (stock script active)'; fi
        echo '--- first lines'; head -n 15 "$S"
    else
        echo "RESULT: $S MISSING"
    fi
    echo "backup of original: $(yesno "$B")"
    [ ! -e "$B" ] || { ls -l "$B"; grep -q "$MARK" "$B" && echo 'WARNING: backup is itself a Slot hook'; }
    [ -w "$BIN" ] && echo "$BIN writable: yes" || echo "$BIN writable: NO"
    echo
    echo '=== Card'
    echo "Slot/AUTOBOOT: $(yesno "$ROOT/AUTOBOOT")"
    echo "Slot/DISABLE_AUTOBOOT: $(yesno "$ROOT/DISABLE_AUTOBOOT") (should be MISSING)"
    echo ".slot-boot/boot.sh: $(yesno "$CARD/.slot-boot/boot.sh")"
    echo "trimui installer folder: $(yesno "$CARD/trimui")"
    echo "Slot/System/slot: $(yesno "$ROOT/System/slot")"
    echo "Slot/System/runtrimui-slot.sh: $(yesno "$ROOT/System/runtrimui-slot.sh")"
    echo '--- Slot/Installer'; ls -l "$ROOT/Installer" 2>&1
    echo
    echo '=== Files on internal storage hidden under the card (should be empty)'
    find /overlay/upper/mnt/SDCARD 2>/dev/null | head -n 40
    echo
    echo '=== Mounts'
    grep -E 'SDCARD|mmcblk| / |/usr' /proc/mounts 2>/dev/null
    echo
    echo '=== Logs'
    show /tmp/slot-hook.log 10
    show "$ROOT/Logs/hook.log" 20
    show /tmp/slot-boot.log 10
    show "$ROOT/Logs/autoboot-status.txt" 5
    show "$ROOT/Logs/install.log" 40
    show "$ROOT/Logs/restore.log" 15
    show "$ROOT/Logs/boot.log" 15
    show "$ROOT/Logs/boot.previous.log" 15
    show "$ROOT/Logs/slot.log" 25
    echo
    echo '=== Kernel messages about the card'
    dmesg 2>/dev/null | grep -iE 'mmcblk1|fat|exfat|sdcard|read-only|I/O error' | tail -n 25
} > "$OUT" 2>&1
# Copy to several places so it can be found even if one is not writable.
saved=0
for dest in "$CARD/startup-report.txt" "$APP/report.txt" "$ROOT/Logs/startup-check.txt"; do
    cp "$OUT" "$dest" 2>/dev/null && saved=1
done
[ "$saved" = 1 ] || echo 'SD card not writable' > /dev/null
sync
