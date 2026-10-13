#!/bin/sh
# GPL-3.0-or-later. Boot Logo app for the original TrimUI Brick.
#
# Put one picture (PNG, JPEG or BMP) in Apps/BootLogo/Image and run the app:
# it becomes the logo shown when the Brick powers on. Run it with the Image
# folder empty to put the original logo back.
#
# The first run saves the device's own logo to Slot/BootLogo/original.bmp.
# Pictures are fitted, centred on black, into the same size and format as that
# original, so the bootloader gets exactly the kind of file it already shows.
# Result: Apps/BootLogo/status.txt (and Slot/Logs/boot-logo.log).
set -u
APP=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=${SLOT_LOGO_ROOT:-/mnt/SDCARD/Slot}
STORE="$ROOT/BootLogo"
IMAGES="$APP/Image"
TOOL="$APP/slot-bootlogo"
CHECK="$APP/slot-filecheck"
STATUS="$APP/status.txt"
WORK="$APP/.work"
DEV=${SLOT_LOGO_DEV:-/dev/mmcblk0p1}
MNT=${SLOT_LOGO_MNT:-/tmp/slot-logo-boot}
TESTING=${SLOT_LOGO_TEST:-0}
# Test hooks (BusyBox runs its own applets before PATH, so stubs need full paths).
MOUNT=${SLOT_LOGO_MOUNT:-mount}
UMOUNT=${SLOT_LOGO_UMOUNT:-umount}
DF=${SLOT_LOGO_DF:-df}
# Checksum of the logo the earlier "Install Slot boot logo" app wrote.
SLOT_LOGO_CKSUM='290061600 44154'
mkdir -p "$ROOT/Logs" "$STORE" || exit 1
exec >> "$ROOT/Logs/boot-logo.log" 2>&1
echo "Boot Logo app: $(date)"
mounted=0
cleanup() {
    if [ "$mounted" = 1 ]; then
        sync
        $UMOUNT "$MNT" || echo 'WARNING: boot partition could not be unmounted.'
    fi
    [ "$TESTING" = 1 ] || rmdir "$MNT" 2>/dev/null || :
    rm -rf "$WORK"
}
trap cleanup 0
done_with() { echo "$1"; printf '%s\n' "$1" > "$STATUS"; exit "$2"; }
fail() { done_with "FAILED: $1" 1; }
chmod +x "$TOOL" "$CHECK" 2>/dev/null
[ "$("$CHECK" --version 2>/dev/null)" = 'slot-filecheck 1' ] || fail 'slot-filecheck is missing from Apps/BootLogo.'
[ "$("$TOOL" --version 2>/dev/null)" = 'slot-bootlogo 1' ] || fail 'slot-bootlogo is missing from Apps/BootLogo.'
checksum() { "$CHECK" checksum "$1"; }
same() { "$CHECK" compare "$1" "$2"; }

# Newest picture in the Image folder, if any.
picture=''
for f in "$IMAGES"/*; do
    [ -f "$f" ] || continue
    case "$(printf '%s' "$f" | tr 'A-Z' 'a-z')" in
        *.png|*.jpg|*.jpeg|*.bmp) ;;
        *) continue ;;
    esac
    if [ -z "$picture" ] || [ "$f" -nt "$picture" ]; then picture=$f; fi
done
if [ -n "$picture" ]; then echo "Picture: $picture"; else echo 'Image folder empty: restoring the original logo.'; fi

if [ "$TESTING" != 1 ]; then
    strings /usr/trimui/bin/MainUI 2>/dev/null | grep -qx 'Trimui Brick' || fail 'Original TrimUI Brick not detected.'
    [ -b "$DEV" ] || fail 'Internal boot partition not found.'
fi
awk -v dev="$DEV" '$1==dev {found=1} END {exit !found}' /proc/mounts && fail 'Boot partition already mounted; no changes made.'
mkdir -p "$MNT" || fail 'Could not create mount point.'
$MOUNT -t vfat -o ro "$DEV" "$MNT" || fail 'Could not open the boot partition.'
mounted=1
CURRENT="$MNT/bootlogo.bmp"
BACKUP="$STORE/original.bmp"
[ -s "$CURRENT" ] || fail 'The device has no bootlogo.bmp; refusing to guess.'
[ "$(dd if="$CURRENT" bs=1 count=2 2>/dev/null)" = BM ] || fail 'The current logo is not a BMP; no changes made.'
current_sum=$(checksum "$CURRENT") || fail 'Could not read the current logo.'

if [ -e "$BACKUP" ]; then
    [ -s "$STORE/original.cksum" ] || fail 'Slot/BootLogo/original.cksum is missing; the backup cannot be verified.'
    value=$(checksum "$BACKUP") || fail 'Could not read the original backup.'
    [ "$value" = "$(cat "$STORE/original.cksum")" ] || fail 'Slot/BootLogo/original.bmp failed verification.'
    # Only replace logos this app (or the old Slot logo app) put there.
    known=0
    [ "$current_sum" = "$value" ] && known=1
    [ "$current_sum" = "$SLOT_LOGO_CKSUM" ] && known=1
    [ -s "$STORE/installed.cksum" ] && [ "$current_sum" = "$(cat "$STORE/installed.cksum")" ] && known=1
    [ "$known" = 1 ] || [ -e "$APP/FORCE" ] || fail 'The current logo was changed by something else. To replace it anyway, create an empty file named FORCE in Apps/BootLogo.'
    REFERENCE=$BACKUP
elif [ "$current_sum" = "$SLOT_LOGO_CKSUM" ]; then
    # The Slot logo is installed but its original was never saved here.
    [ -n "$picture" ] || fail 'No original logo backup exists, so there is nothing to restore. Copy your saved original.bmp and original.cksum into Slot/BootLogo.'
    echo 'No original backup: the Slot logo is installed, so it is not saved as the original.'
    REFERENCE=$CURRENT
else
    [ -n "$picture" ] || done_with 'Nothing to do: the original logo is already installed (Image folder is empty).' 0
    cp "$CURRENT" "$STORE/original.tmp" || fail 'Could not back up the original logo.'
    same "$CURRENT" "$STORE/original.tmp" || fail 'Backup verification failed.'
    mv -f "$STORE/original.tmp" "$BACKUP" || fail 'Could not keep the original logo backup.'
    printf '%s\n' "$current_sum" > "$STORE/original.cksum" || fail 'Could not save the backup checksum.'
    sync
    echo "Original logo saved to $BACKUP ($("$TOOL" info "$BACKUP" 2>&1))"
    REFERENCE=$BACKUP
fi

if [ -n "$picture" ]; then
    rm -rf "$WORK"; mkdir -p "$WORK" || fail 'Could not create a work folder.'
    SOURCE="$WORK/bootlogo.bmp"
    message=$("$TOOL" convert "$picture" "$REFERENCE" "$SOURCE" 2>&1) || fail "Could not use $(basename "$picture"): ${message#slot-bootlogo: }"
    echo "$message"
else
    [ -e "$BACKUP" ] || fail 'No original logo backup exists, so there is nothing to restore.'
    SOURCE=$BACKUP
fi

if same "$CURRENT" "$SOURCE"; then
    [ -n "$picture" ] && printf '%s\n' "$current_sum" > "$STORE/installed.cksum"
    done_with 'Nothing to change: that logo is already installed.' 0
fi
need=$(wc -c < "$SOURCE")
have=$(wc -c < "$CURRENT")
free=$($DF -k "$MNT" 2>/dev/null | awk 'NR==2 {print $4 * 1024}')
[ -n "$free" ] || fail 'Could not check free space on the boot partition.'
$MOUNT -o remount,rw "$DEV" "$MNT" || fail 'Could not enable writing the logo.'
if [ "$free" -ge "$need" ]; then
    # Normal path: write a copy next to the old logo, verify, then swap.
    cp "$SOURCE" "$MNT/bootlogo.tmp" || { rm -f "$MNT/bootlogo.tmp"; fail 'Could not write the new logo.'; }
    same "$SOURCE" "$MNT/bootlogo.tmp" || { rm -f "$MNT/bootlogo.tmp"; fail 'New logo verification failed.'; }
    mv -f "$MNT/bootlogo.tmp" "$CURRENT" || fail 'Could not activate the new logo.'
elif [ $((free + have)) -ge "$need" ]; then
    # Not enough room for two copies: overwrite in place (the original is backed up on the card).
    echo 'Boot partition is nearly full; writing the logo in place.'
    cp "$SOURCE" "$CURRENT" || fail 'Could not write the new logo. Run the app again with the Image folder empty to restore the original.'
else
    fail "The picture needs $need bytes but the boot partition only has room for $((free + have))."
fi
sync
same "$SOURCE" "$CURRENT" || fail 'Written logo verification failed. Run the app again with the Image folder empty to restore the original.'
if [ -n "$picture" ]; then
    checksum "$CURRENT" > "$STORE/installed.cksum"
    result="SUCCESS: $(basename "$picture") is now the boot logo. Restart the Brick to see it."
else
    rm -f "$STORE/installed.cksum"
    result='SUCCESS: original boot logo restored. Restart the Brick to see it.'
fi
$UMOUNT "$MNT" || fail 'Could not unmount the boot partition; do not remove the card yet.'
mounted=0
done_with "$result" 0
