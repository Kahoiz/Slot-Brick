from pathlib import Path
import tempfile,subprocess
script=Path(__file__).parent/'CLEAN-SD-CARD.ps1'
with tempfile.TemporaryDirectory() as td:
 p=Path(td);card=p/'card';backup=p/'backup'
 fixtures={
 'Slot/System/slot':'runtime','.slot-boot/boot.sh':'boot','Slot/Saves/test.sav':'save','Slot/BootLogo/original.bmp':'logo','Emus/GBA/config.json':'stock',
 'Tools/tg5040/Slot.pak/launch.sh':'exec sh "$SD/Slot/System/launch.sh"','Tools/Other/launch.sh':'unrelated',
 'Apps/SlotBootRestore/launch.sh':'runtrimui-slot-backup.sh','Apps/Slot/launch.sh':'required launcher',
 'Slot/Installer/trimui-installed-123/app/runtrimui.sh':'# SLOT_BRICK_BOOT_V2',
 'Slot/Installer/unrecognized/data':'keep'}
 for name,value in fixtures.items():f=card/name;f.parent.mkdir(parents=True,exist_ok=True);f.write_text(value)
 subprocess.run(['/tmp/slot-pwsh/pwsh','-NoProfile','-File',str(script),'-Card',str(card),'-Backup',str(backup)],check=True)
 removed=('Tools/tg5040/Slot.pak/','Apps/SlotBootRestore/','Slot/Installer/trimui-installed-123/')
 for name,value in fixtures.items():
  if name.startswith(removed):assert not (card/name).exists();assert (backup/name).read_text()==value
  else:assert (card/name).read_text()==value
 # Refuse a backup on the card; originals remain.
 r=subprocess.run(['/tmp/slot-pwsh/pwsh','-NoProfile','-File',str(script),'-Card',str(card),'-Backup',str(card/'badbackup')],capture_output=True)
 assert r.returncode!=0 and not (card/'badbackup').exists()
print('PASS: verified backup/removal, unrelated files retained, in-card backup rejected')
