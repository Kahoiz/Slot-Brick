#!/usr/bin/env python3
"""Exercise System/brick_wifi.sh against stub firmware tools; never touches host networking."""
import os, pathlib, socket, subprocess, tempfile, time, unittest
SCRIPT = pathlib.Path(__file__).parent / 'card/Slot/System/brick_wifi.sh'

class WifiHelperTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); p = pathlib.Path(self.tmp.name)
        self.p = p; self.root = p/'Slot'; (self.root/'Logs').mkdir(parents=True)
        self.etc = p/'etc'; self.net = p/'net'; self.varrun = p/'run'; self.run_dir = p/'slotrun'
        self.bin = p/'bin'; self.bin.mkdir(); self.calls = p/'calls'
        (self.net/'wlan0').mkdir(parents=True); self.etc.mkdir()
        def exe(name, text):
            f = self.bin/name; f.write_text('#!/bin/sh\n' + text); f.chmod(0o755)
        exe('rfkill', 'echo "rfkill $*" >> "$CALLS"\n')
        exe('ifconfig', 'echo "ifconfig $*" >> "$CALLS"\n')
        # Stub supplicant: record args and expose a control socket like the real one.
        exe('wpa_supplicant', 'echo "wpa_supplicant $*" >> "$CALLS"\n'
            'python3 -c "import socket,sys,os;os.makedirs(sys.argv[1],exist_ok=True);s=socket.socket(socket.AF_UNIX);s.bind(sys.argv[1]+\'/wlan0\')" "$SOCKDIR"\n')
        # A DHCP client that would hang if the helper kept our pipes open.
        exe('udhcpc', 'echo "udhcpc $*" >> "$CALLS"\nexec sleep 30\n')
        exe('wpa_cli', 'echo "wpa_cli $*" >> "$CALLS"\n')
        exe('pidof', 'exit 1\n')
    def tearDown(self):
        subprocess.run(['pkill', '-f', 'sleep 30'], check=False)
        self.tmp.cleanup()
    def helper(self, verb, timeout=15):
        env = dict(os.environ, SLOT_ROOT=str(self.root), SLOT_WIFI_RUN=str(self.run_dir), SLOT_WIFI_ETC=str(self.etc),
                   SLOT_WIFI_NET=str(self.net), SLOT_WIFI_VARRUN=str(self.varrun), CALLS=str(self.calls),
                   SOCKDIR=str(self.varrun/'wpa_supplicant'), SLOT_WIFI_PATH=str(self.bin), PATH=str(self.bin) + ':/usr/bin:/bin')
        start = time.monotonic()
        # capture_output keeps pipes open like slot's spawn; return proves no daemon holds them.
        r = subprocess.run(['/bin/sh', str(SCRIPT), verb], env=env, capture_output=True, timeout=timeout)
        return r.returncode, time.monotonic() - start
    def lines(self):
        return self.calls.read_text().splitlines() if self.calls.exists() else []

    def test_start_brings_up_supplicant_socket_and_dhcp_without_blocking(self):
        code, took = self.helper('start')
        self.assertEqual(code, 0); self.assertLess(took, 5)
        calls = self.lines()
        self.assertIn('rfkill unblock wifi', calls); self.assertIn('ifconfig wlan0 up', calls)
        conf = self.root/'Config/wpa_supplicant.conf'
        self.assertTrue(any(c.startswith('wpa_supplicant -B -D nl80211 -i wlan0 -c ' + str(conf)) for c in calls), calls)
        self.assertIn('update_config=1', conf.read_text())
        self.assertTrue(any(c.startswith('udhcpc -i wlan0') for c in calls), calls)
        self.assertTrue((self.run_dir/'udhcpc.script').exists())
    def test_existing_supplicant_is_reused(self):
        d = self.varrun/'wpa_supplicant'; d.mkdir(parents=True)
        s = socket.socket(socket.AF_UNIX); s.bind(str(d/'wlan0'))
        try:
            self.assertEqual(self.helper('start')[0], 0)
            self.assertFalse(any(c.startswith('wpa_supplicant') for c in self.lines()))
        finally:
            s.close()
    def test_stock_profiles_used_when_socket_reachable(self):
        (self.etc/'wifi').mkdir(); (self.etc/'wifi/wpa_supplicant.conf').write_text('ctrl_interface=/etc/wifi/sockets\nupdate_config=1\n')
        self.helper('start', timeout=20)
        self.assertTrue(any('-c ' + str(self.etc/'wifi/wpa_supplicant.conf') in c for c in self.lines()), self.lines())
    def test_firmware_helper_takes_over(self):
        (self.etc/'wifi').mkdir(); f = self.etc/'wifi/wifi_init.sh'; f.write_text('echo "firmware $1" >> "$CALLS"\n')
        self.assertEqual(self.helper('start')[0], 0); self.assertEqual(self.lines(), ['firmware start'])
    def test_missing_interface_fails(self):
        (self.net/'wlan0').rmdir()
        code, took = self.helper('start')
        self.assertNotEqual(code, 0); self.assertLess(took, 10)
        self.assertIn('no wlan0', (self.root/'Logs/wifi.log').read_text())
    def test_stop_turns_everything_off(self):
        self.assertEqual(self.helper('start')[0], 0)
        self.assertEqual(self.helper('stop')[0], 0)
        calls = self.lines()
        self.assertTrue(any(c.startswith('wpa_cli -p') and c.endswith('terminate') for c in calls), calls)
        self.assertIn('rfkill block wifi', calls)
        self.assertFalse((self.run_dir/'udhcpc.pid').exists())
    def test_radio_only_powers_interface(self):
        self.assertEqual(self.helper('radio')[0], 0)
        self.assertEqual(self.lines(), ['rfkill unblock wifi', 'ifconfig wlan0 up'])
    def test_bad_verb(self):
        self.assertEqual(self.helper('restart')[0], 2)

if __name__ == '__main__':
    unittest.main()
