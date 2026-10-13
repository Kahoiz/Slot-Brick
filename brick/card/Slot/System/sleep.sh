#!/bin/sh
# Original TrimUI Brick: transient suspend-to-RAM, using NextUI's service order.
# Never change firmware, wake sources, governors, or persistent network settings.
# Everything changed for the suspend (radios, mem_sleep mode) is put back after.
set -u
ROOT=${SLOT_ROOT:-/mnt/SDCARD/Slot}
SYS=${SLOT_SLEEP_SYSROOT:-/sys}
ETC=${SLOT_SLEEP_ETC:-/etc}
# Brick firmware has no $ETC/wifi/wifi_init.sh; use slot's own helper instead.
WIFI_HELPER="$ETC/wifi/wifi_init.sh"
[ -f "$WIFI_HELPER" ] || WIFI_HELPER="$ROOT/System/brick_wifi.sh"
mkdir -p "$ROOT/Logs"
LOG="$ROOT/Logs/sleep.log"
if [ -f "$LOG" ] && [ "$(wc -c < "$LOG")" -gt 131072 ]; then mv "$LOG" "$ROOT/Logs/sleep.previous.log"; fi
exec >> "$LOG" 2>&1
printf '\nSlot v0.11 sleep: %s\n' "$(date)"
[ ! -e "$ROOT/DISABLE_DEEP_SLEEP" ] || { echo 'Deep sleep disabled; using standby'; exit 1; }
[ -w "$SYS/power/state" ] && grep -qw mem "$SYS/power/state" || { echo 'Kernel mem suspend unavailable'; exit 1; }
wifi=0
bt=0
radios=''
mem_sleep_was=''
BATTERY="$SYS/class/power_supply/axp2202-battery"
if [ ! -d "$BATTERY" ]; then
    for d in "$SYS"/class/power_supply/*; do
        [ "$(cat "$d/type" 2>/dev/null)" = Battery ] && { BATTERY=$d; break; }
    done
fi
battery() { # capacity% charge_counter voltage_uv
    printf '%s %s %s' "$(cat "$BATTERY/capacity" 2>/dev/null || echo ?)" \
        "$(cat "$BATTERY/charge_counter" 2>/dev/null || echo ?)" "$(cat "$BATTERY/voltage_now" 2>/dev/null || echo ?)"
}
# Radios the firmware left powered (soft-unblocked) still draw current in
# suspend. Block them for the sleep and unblock exactly those afterwards.
block_radios() {
    for r in "$SYS"/class/rfkill/rfkill*; do
        [ -w "$r/soft" ] && [ "$(cat "$r/soft" 2>/dev/null)" = 0 ] || continue
        if printf '1\n' > "$r/soft"; then radios="$radios $r"; echo "Blocked radio $(cat "$r/type" 2>/dev/null) ($r)"; fi
    done
}
unblock_radios() {
    for r in $radios; do printf '0\n' > "$r/soft" || echo "Radio restore failed: $r"; done
    radios=''
}
# Kernels offering a choice: use real suspend-to-RAM, not the costlier s2idle.
choose_deep() {
    f="$SYS/power/mem_sleep"
    [ -w "$f" ] || return 0
    current=$(sed -n 's/.*\[\([a-z0-9]*\)\].*/\1/p' "$f")
    if [ -n "$current" ] && [ "$current" != deep ] && grep -qw deep "$f"; then
        printf 'deep\n' > "$f" && mem_sleep_was=$current && echo "mem_sleep: $current -> deep"
    fi
}
INPUT_FLAG=${SLOT_SLEEP_INPUT_FLAG:-/tmp/system_suspend}
# Present only while the kernel is being asked to suspend: slot cancels this
# script on a Power press before that point, and treats a later press as the wake key.
SUSPENDING=${SLOT_SLEEP_MARKER:-/tmp/slot-suspending}
woke_at=''
owned_flag=0
restore() {
    status=$?
    trap - EXIT HUP INT TERM
    rm -f "$SUSPENDING"
    [ "$owned_flag" = 0 ] || rm -f "$INPUT_FLAG"
    [ -z "$mem_sleep_was" ] || printf '%s\n' "$mem_sleep_was" > "$SYS/power/mem_sleep" || echo 'mem_sleep restore failed'
    unblock_radios
    # Wi-Fi reconnects in the background: the screen should not wait for it.
    if [ "$wifi" != 0 ]; then
        ( sh "$WIFI_HELPER" start || echo 'Wi-Fi restore failed' ) < /dev/null &
    fi
    [ "$bt" = 0 ] || "$ETC/bluetooth/bt_init.sh" start || echo 'Bluetooth restore failed'
    [ -z "$woke_at" ] || echo "Handed back to slot $(awk -v a="$woke_at" '{printf "%d", ($1 - a) * 1000}' /proc/uptime 2>/dev/null) ms after the kernel woke"
    echo "Sleep cycle finished: $status"
    exit "$status"
}
trap restore EXIT
trap 'exit 1' HUP INT TERM
if [ -x "$ETC/bluetooth/bt_init.sh" ] && pgrep -x bluetoothd >/dev/null; then
    bt=1
    "$ETC/bluetooth/bt_init.sh" stop || { echo 'Bluetooth stop failed'; exit 1; }
fi
if [ -f "$WIFI_HELPER" ] && pgrep -x wpa_supplicant >/dev/null; then
    wifi=1
    sh "$WIFI_HELPER" stop || { echo 'Wi-Fi stop failed'; exit 1; }
fi
# The stock daemon cooperatively parks input polling while this flag exists.
# Own only the flag we create; preserve a pre-existing firmware marker.
if [ ! -e "$INPUT_FLAG" ]; then
    (set -C; : > "$INPUT_FLAG") || { echo 'Input suspend handshake failed'; exit 1; }
    owned_flag=1
fi
block_radios
choose_deep
sleep 0.05
# In the background so a cancel (SIGTERM from slot) is not held up by a slow card.
sync & wait $!
for node in "$SYS/power/suspend_stats/success" "$SYS/kernel/debug/suspend_stats/success"; do
    [ ! -r "$node" ] || echo "Before $node: $(cat "$node")"
done
started=$(date +%s)
before=$(battery)
echo "Battery before: $before (capacity% charge_counter voltage_uv)"
echo 'Requesting mem suspend'
: > "$SUSPENDING"
printf 'mem\n' > "$SYS/power/state"
status=$?
woke_at=$(cut -d' ' -f1 /proc/uptime 2>/dev/null)
rm -f "$SUSPENDING"
elapsed=$(( $(date +%s) - started ))
echo "Kernel returned $status after $elapsed seconds"
after=$(battery)
echo "Battery after: $after"
set -f; set -- $before $after; set +f
if [ "$elapsed" -ge 60 ] && [ "$2" != ? ] && [ "$5" != ? ]; then
    used=$(( $2 - $5 ))
    echo "Sleep drain: $used charge units over $elapsed s (~$(( used * 3600 / elapsed )) per hour); capacity $1% -> $4%"
fi
[ ! -r "$SYS/power/pm_wakeup_irq" ] || echo "Wake IRQ: $(cat "$SYS/power/pm_wakeup_irq")"
for node in "$SYS/power/suspend_stats/success" "$SYS/kernel/debug/suspend_stats/success"; do
    [ ! -r "$node" ] || echo "After $node: $(cat "$node")"
done
# The frontend independently compares BOOTTIME and MONOTONIC to verify sleep.
# Some tg5040 kernels return an error despite having slept. Never re-suspend
# immediately after the user wakes the device (same workaround as NextUI).
if [ "$status" -ne 0 ] && [ "$elapsed" -gt 5 ]; then status=0; fi
exit "$status"
