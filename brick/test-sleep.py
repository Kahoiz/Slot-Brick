#!/usr/bin/env python3
"""Exercise service rollback and fallback without accessing host power controls."""
import os, pathlib, subprocess, tempfile, time, unittest
SCRIPT = pathlib.Path(__file__).parent / 'card/Slot/System/sleep.sh'
class SleepTests(unittest.TestCase):
    def run_cycle(self, active=True, fail_stop=False, fail_kernel=False, disabled=False, supported=True, existing_flag=False, card_wifi=False, radios=None, mem_sleep=None, observe=None):
        with tempfile.TemporaryDirectory() as tmp:
            p=pathlib.Path(tmp);root=p/'Slot';root.mkdir();sys=p/'sys';(sys/'power').mkdir(parents=True)
            state=sys/'power/state';state.write_text('freeze mem\n' if supported else 'freeze\n')
            flag=p/'system_suspend'
            if existing_flag:flag.write_text('firmware')
            etc=p/'etc';bin=p/'bin';bin.mkdir();calls=p/'calls'
            def exe(path,text):
                path.parent.mkdir(parents=True,exist_ok=True);path.write_text('#!/bin/sh\n'+text);path.chmod(0o755)
            exe(bin/'pgrep','exit '+('0' if active else '1')+'\n')
            for i,(kind,soft) in enumerate(radios or []):
                r=sys/'class/rfkill'/f'rfkill{i}';r.mkdir(parents=True);(r/'type').write_text(kind+'\n');(r/'soft').write_text(f'{soft}\n')
            if mem_sleep is not None:(sys/'power/mem_sleep').write_text(mem_sleep+'\n')
            b=sys/'class/power_supply/axp2202-battery';b.mkdir(parents=True)
            for n,v in [('capacity','72'),('charge_counter','2160'),('voltage_now','3770000')]:(b/n).write_text(v+'\n')
            # sync runs right before the suspend request: record what the kernel would see.
            seen='for f in "$SLOT_SLEEP_SYSROOT"/class/rfkill/*/soft "$SLOT_SLEEP_SYSROOT"/power/mem_sleep; do [ -f "$f" ] && echo "seen $(basename $(dirname $f))/$(basename $f)=$(cat $f)" >> "$CALLS"; done\n'
            exe(bin/'sync', seen+('rm "$SLOT_SLEEP_SYSROOT/power/state"; mkdir "$SLOT_SLEEP_SYSROOT/power/state"\n' if fail_kernel else 'exit 0\n'))
            for folder,name in [('wifi','wifi_init.sh'),('bluetooth','bt_init.sh')]:
                if folder=='wifi' and card_wifi:
                    # Stock Brick firmware: no /etc/wifi helper, slot ships its own.
                    exe(root/'System/brick_wifi.sh', f'echo "card-wifi $1" >> "$CALLS"\nexit 0\n');continue
                exe(etc/folder/name, f'echo "{folder} $1" >> "$CALLS"\n'+ ('[ "$1" != stop ]\n' if fail_stop and folder=='wifi' else 'exit 0\n'))
            if disabled:(root/'DISABLE_DEEP_SLEEP').touch()
            env=dict(os.environ,SLOT_SLEEP_INPUT_FLAG=str(flag),SLOT_SLEEP_MARKER=str(p/'suspending'),SLOT_ROOT=str(root),SLOT_SLEEP_SYSROOT=str(sys),SLOT_SLEEP_ETC=str(etc),CALLS=str(calls),PATH=str(bin)+':'+os.environ['PATH'])
            result=subprocess.run([os.environ.get('SLOT_TEST_SH','/bin/sh'),str(SCRIPT)],env=env,timeout=5)
            time.sleep(0.3)  # Wi-Fi is restored in the background after wake
            self.assertEqual(flag.exists(),existing_flag)
            if observe is not None:
                observe['after']={str(f.relative_to(sys)):f.read_text().strip() for f in list(sys.glob('class/rfkill/*/soft'))+list(sys.glob('power/mem_sleep'))}
                observe['log']=(root/'Logs/sleep.log').read_text()
            if existing_flag:self.assertEqual(flag.read_text(),'firmware')
            lines=calls.read_text().splitlines() if calls.exists() else []
            lines=[l for l in lines if not l.startswith('seen ')] if observe is None else lines
            # Restores run concurrently (Wi-Fi in the background): order the starts.
            stops=[l for l in lines if not l.endswith(' start')];starts=sorted(l for l in lines if l.endswith(' start'))
            return result.returncode,stops+starts
    def test_resume_restores_running_services(self):
        self.assertEqual(self.run_cycle(),(0,['bluetooth stop','wifi stop','bluetooth start','wifi start']))
    def test_card_wifi_helper_used_without_firmware_helper(self):
        self.assertEqual(self.run_cycle(card_wifi=True),(0,['bluetooth stop','card-wifi stop','bluetooth start','card-wifi start']))
    def test_radios_blocked_for_suspend_and_restored(self):
        o={};code,calls=self.run_cycle(active=False,radios=[('wlan',0),('bluetooth',1)],mem_sleep='[s2idle] deep',observe=o)
        self.assertEqual(code,0)
        self.assertIn('seen rfkill0/soft=1',calls);self.assertIn('seen rfkill1/soft=1',calls);self.assertIn('seen power/mem_sleep=deep',calls)
        self.assertEqual(o['after'],{'class/rfkill/rfkill0/soft':'0','class/rfkill/rfkill1/soft':'1','power/mem_sleep':'s2idle'})
        self.assertIn('Battery before: 72 2160 3770000',o['log'])
    def test_radios_restored_when_kernel_refuses(self):
        o={};code,_=self.run_cycle(active=False,radios=[('wlan',0)],fail_kernel=True,observe=o)
        self.assertNotEqual(code,0);self.assertEqual(o['after'],{'class/rfkill/rfkill0/soft':'0'})
    def test_deep_already_selected_is_left_alone(self):
        o={};self.run_cycle(active=False,mem_sleep='s2idle [deep]',observe=o)
        self.assertEqual(o['after'],{'power/mem_sleep':'s2idle [deep]'});self.assertNotIn('mem_sleep:',o['log'])
    def test_does_not_enable_inactive_services(self): self.assertEqual(self.run_cycle(active=False),(0,[]))
    def test_stop_failure_rolls_back(self):
        code,calls=self.run_cycle(fail_stop=True);self.assertNotEqual(code,0);self.assertEqual(calls,['bluetooth stop','wifi stop','bluetooth start','wifi start'])
    def test_kernel_failure_rolls_back(self):
        code,calls=self.run_cycle(fail_kernel=True);self.assertNotEqual(code,0);self.assertEqual(calls[-2:],['bluetooth start','wifi start'])
    def test_existing_input_marker_is_preserved(self):self.assertEqual(self.run_cycle(existing_flag=True)[0],0)
    def test_opt_out(self):self.assertEqual(self.run_cycle(disabled=True),(1,[]))
    def test_unsupported(self):self.assertEqual(self.run_cycle(supported=False),(1,[]))
class CancelTests(unittest.TestCase):
    """slot sends SIGTERM when Power is pressed before the kernel request."""
    def test_terminate_while_preparing_restores_and_never_suspends(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=pathlib.Path(tmp);root=p/'Slot';root.mkdir();sys=p/'sys';(sys/'power').mkdir(parents=True)
            state=sys/'power/state';state.write_text('freeze mem\n')
            r=sys/'class/rfkill/rfkill0';r.mkdir(parents=True);(r/'type').write_text('wlan\n');(r/'soft').write_text('0\n')
            bin=p/'bin';bin.mkdir();calls=p/'calls'
            for name,body in [('pgrep','exit 1'),('sync','echo preparing >> "$CALLS"; sleep 3')]:
                f=bin/name;f.write_text('#!/bin/sh\n'+body+'\n');f.chmod(0o755)
            marker=p/'suspending'
            env=dict(os.environ,SLOT_SLEEP_INPUT_FLAG=str(p/'flag'),SLOT_SLEEP_MARKER=str(marker),SLOT_ROOT=str(root),SLOT_SLEEP_SYSROOT=str(sys),SLOT_SLEEP_ETC=str(p/'etc'),CALLS=str(calls),PATH=str(bin)+':'+os.environ['PATH'])
            proc=subprocess.Popen([os.environ.get('SLOT_TEST_SH','/bin/sh'),str(SCRIPT)],env=env)
            for _ in range(100):
                if calls.exists():break
                time.sleep(0.02)
            self.assertFalse(marker.exists(),'not yet asking the kernel')
            t0=time.monotonic();proc.terminate();code=proc.wait(timeout=5)
            self.assertLess(time.monotonic()-t0,1.5,'cancel is not held up by sync')
            self.assertNotEqual(code,0)
            self.assertEqual(state.read_text(),'freeze mem\n','the kernel was never asked to suspend')
            self.assertEqual((r/'soft').read_text().strip(),'0','radio restored')
            self.assertFalse((p/'flag').exists());self.assertFalse(marker.exists())
    def test_marker_only_around_the_kernel_request(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=pathlib.Path(tmp);root=p/'Slot';root.mkdir();sys=p/'sys';(sys/'power').mkdir(parents=True)
            (sys/'power/state').write_text('freeze mem\n')
            bin=p/'bin';bin.mkdir()
            for name,body in [('pgrep','exit 1'),('sync','exit 0')]:
                f=bin/name;f.write_text('#!/bin/sh\n'+body+'\n');f.chmod(0o755)
            marker=p/'suspending'
            env=dict(os.environ,SLOT_SLEEP_INPUT_FLAG=str(p/'flag'),SLOT_SLEEP_MARKER=str(marker),SLOT_ROOT=str(root),SLOT_SLEEP_SYSROOT=str(sys),SLOT_SLEEP_ETC=str(p/'etc'),PATH=str(bin)+':'+os.environ['PATH'])
            self.assertEqual(subprocess.run([os.environ.get('SLOT_TEST_SH','/bin/sh'),str(SCRIPT)],env=env,timeout=5).returncode,0)
            self.assertFalse(marker.exists())
            log=(root/'Logs/sleep.log').read_text()
            self.assertIn('Handed back to slot',log)
if __name__=='__main__':unittest.main()
