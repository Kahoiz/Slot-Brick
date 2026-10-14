"""Execute shipped shell backend against fake device commands (no hardware writes)."""
from pathlib import Path
import tempfile, subprocess, os, time
source=Path(__file__).parent/'card/Slot/System/slotlink.sh'
# card: slot's own Wi-Fi helper on the card (else a firmware one); the card cases also have
# normal Wi-Fi running before the link, which must come back after it.
for role,fail,card in [('host',False,False),('join',False,False),('host',True,False),('join',True,False),('down',False,False),('cool',False,True),('warm',False,False),('host',False,True),('join',False,True),('host',True,True)]:
 with tempfile.TemporaryDirectory() as td:
  p=Path(td);cmd=p/'bin';cmd.mkdir();etc=p/'etc/wifi';etc.mkdir(parents=True);run=p/'run';root=p/'Slot'
  calls=p/'calls';calls.touch()
  def script(path,text):path.write_text('#!/bin/sh\n'+text);path.chmod(0o755)
  if card:
   # Stock Brick firmware: no /etc/wifi/wifi_init.sh; slot ships System/brick_wifi.sh.
   (root/'System').mkdir(parents=True);script(root/'System/brick_wifi.sh',f'echo "card $*" >> "{calls}"\n')
  else:script(etc/'wifi_init.sh',f'echo "wifi $*" >> "{calls}"\n')
  # A station supplicant is running when the card cases start.
  script(cmd/'pgrep',f'[ -n "{"yes" if card else ""}" ]\n')
  for name in ['rfkill.elf','ifconfig','hostapd','wpa_supplicant']:
   script(cmd/name,f'echo "{name} $*" >> "{calls}"\nexit {1 if fail and name in ["hostapd","wpa_supplicant"] else 0}\n')
  script(p/'test.sh',source.read_text().replace('PATH=/usr/trimui/bin:/usr/sbin:/sbin:/usr/bin:/bin',f'PATH={cmd}:/usr/bin:/bin'))
  env=dict(os.environ,SLOT_ROOT=str(root),SLOT_LINK_RUN=str(run),SLOT_LINK_ETC=str(p/'etc'))
  r=subprocess.run(['sh',str(p/'test.sh'),'link',role],env=env,timeout=5)
  assert (r.returncode!=0)==fail,(role,r.returncode)
  history=calls.read_text()
  # Closing a game asks for down/cool: with no link session, Wi-Fi must not be touched.
  if role in ('warm','down','cool'):assert not history,history;continue
  if fail:assert history.rstrip().endswith('rfkill.elf block wifi'),history
  else:
   assert f'ifconfig wlan0 10.42.0.{1 if role=="host" else 2} netmask 255.255.255.0 up' in history
   if card:assert 'card radio' in history and 'card start' not in history,history
   else:assert 'wifi start' in history,history
   config=(run/('hostapd.conf' if role=='host' else 'join.conf')).read_text()
   assert 'slotlink-brick' in config
   assert not (etc/'wpa_supplicant.conf').exists()
   before=len(calls.read_text().splitlines())
   subprocess.run(['sh',str(p/'test.sh'),'link','down'],env=env,check=True,timeout=5)
   assert not (run/'hostapd.conf').exists() and not (run/'join.conf').exists()
   time.sleep(0.3)  # Wi-Fi comes back in the background
   after=calls.read_text().splitlines()[before:]
   assert 'rfkill.elf block wifi' in after,after
   assert (('card start' in after) if card else ('wifi start' not in after)),after
   # A second down (the next game closing) leaves the restored Wi-Fi alone.
   before=len(calls.read_text().splitlines())
   subprocess.run(['sh',str(p/'test.sh'),'link','down'],env=env,check=True,timeout=5)
   assert calls.read_text().splitlines()[before:]==[],calls.read_text()
print('PASS: host/join setup (firmware and card Wi-Fi helper), failure radio-off, down/cool leave Wi-Fi alone without a link, Wi-Fi restored after a link, no radio warmup, temporary profiles, cleanup')
