#!/bin/sh
HERE="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
SD="$(CDPATH= cd -- "$HERE/../.." && pwd)"
sh "$SD/Slot/System/install-autoboot.sh"
exec sh "$SD/Slot/System/launch.sh"
