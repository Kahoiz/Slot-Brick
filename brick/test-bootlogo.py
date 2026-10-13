#!/usr/bin/env python3
"""Run Apps/BootLogo/launch.sh against a fake boot partition (a plain folder).

Needs host builds of the helpers:
  SLOT_FILECHECK=path/to/host/slot-filecheck  (gcc brick/slot-filecheck.c)
  SLOT_BOOTLOGO=path/to/host/slot-bootlogo    (cargo build in brick/bootlogo-tool)
"""
import os, pathlib, shutil, struct, subprocess, tempfile, unittest, zlib

HERE = pathlib.Path(__file__).parent
APP_SRC = HERE / 'card/Apps/BootLogo'
FILECHECK = os.environ.get('SLOT_FILECHECK')
BOOTLOGO = os.environ.get('SLOT_BOOTLOGO', str(HERE / 'bootlogo-tool/target/debug/slot-bootlogo'))
SLOT_LOGO = (HERE / 'test-fixtures/slot-logo.bmp').read_bytes()

def bmp(w, h, rgb=(10, 20, 30)):
    row = (w * 3 + 3) // 4 * 4
    px = (bytes([rgb[2], rgb[1], rgb[0]]) * w).ljust(row, b'\0') * h
    return b'BM' + struct.pack('<IHHI', 54 + len(px), 0, 0, 54) + struct.pack('<IiiHHIIiiII', 40, w, h, 1, 24, 0, len(px), 2835, 2835, 0, 0) + px

def png(w, h, rgb=(255, 0, 0)):
    raw = b''.join(b'\0' + bytes(rgb) * w for _ in range(h))
    chunk = lambda t, d: struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')

@unittest.skipUnless(FILECHECK and os.path.exists(BOOTLOGO), 'host helper builds not provided')
class BootLogoAppTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); p = pathlib.Path(self.tmp.name)
        self.app = p / 'Apps/BootLogo'; shutil.copytree(APP_SRC, self.app)
        shutil.copy(FILECHECK, self.app / 'slot-filecheck'); shutil.copy(BOOTLOGO, self.app / 'slot-bootlogo')
        self.root = p / 'Slot'; self.root.mkdir()
        self.boot = p / 'boot'; self.boot.mkdir()
        self.original = bmp(64, 48, (1, 2, 3)); (self.boot / 'bootlogo.bmp').write_bytes(self.original)
        self.bin = p / 'bin'; self.bin.mkdir(); self.calls = p / 'calls'
        self.free_kb = 10000
        for name, body in [('mount', 'echo "mount $*" >> "$CALLS"'), ('umount', 'echo "umount $*" >> "$CALLS"'),
                           ('df', 'echo "Filesystem 1K-blocks Used Available Use% Mounted"; echo "/dev/x 20000 1 $FREE_KB 1% $2"')]:
            f = self.bin / name; f.write_text('#!/bin/sh\n' + body + '\n'); f.chmod(0o755)
    def tearDown(self): self.tmp.cleanup()
    def run_app(self):
        env = dict(os.environ, SLOT_LOGO_ROOT=str(self.root), SLOT_LOGO_MNT=str(self.boot), SLOT_LOGO_DEV='/dev/fake-boot',
                   SLOT_LOGO_TEST='1', SLOT_LOGO_MOUNT=str(self.bin/'mount'), SLOT_LOGO_UMOUNT=str(self.bin/'umount'), SLOT_LOGO_DF=str(self.bin/'df'), CALLS=str(self.calls), FREE_KB=str(self.free_kb), PATH=f"{self.bin}:{os.environ.get('SLOT_TEST_PATH', os.environ['PATH'])}")
        r = subprocess.run([os.environ.get('SLOT_TEST_SH', '/bin/sh'), str(self.app / 'launch.sh')], env=env, timeout=30)
        return r.returncode, (self.app / 'status.txt').read_text().strip()
    def logo(self): return (self.boot / 'bootlogo.bmp').read_bytes()
    def put(self, name, data): (self.app / 'Image' / name).write_bytes(data)
    def clear(self):
        for f in (self.app / 'Image').iterdir():
            if f.name != 'README.txt': f.unlink()

    def test_install_then_restore(self):
        self.put('mine.png', png(200, 100))
        code, status = self.run_app()
        self.assertEqual(code, 0, status); self.assertTrue(status.startswith('SUCCESS: mine.png'), status)
        self.assertEqual((self.root / 'BootLogo/original.bmp').read_bytes(), self.original)
        new = self.logo(); self.assertNotEqual(new, self.original)
        self.assertEqual(len(new), len(self.original), 'same geometry as the original')
        self.assertEqual(new[18:30], self.original[18:30])
        self.assertTrue((self.root / 'BootLogo/installed.cksum').exists())
        self.assertFalse((self.app / '.work').exists())
        self.assertIn('umount', self.calls.read_text())
        # Run again with the same picture: nothing to do.
        self.assertEqual(self.run_app(), (0, 'Nothing to change: that logo is already installed.'))
        self.clear()
        code, status = self.run_app()
        self.assertEqual(code, 0, status); self.assertIn('original boot logo restored', status)
        self.assertEqual(self.logo(), self.original)
        self.assertFalse((self.root / 'BootLogo/installed.cksum').exists())
        self.assertEqual(self.run_app(), (0, 'Nothing to change: that logo is already installed.'))

    def test_empty_folder_first_run_changes_nothing(self):
        code, status = self.run_app()
        self.assertEqual(code, 0); self.assertIn('Nothing to do', status)
        self.assertEqual(self.logo(), self.original); self.assertFalse((self.root / 'BootLogo/original.bmp').exists())

    def test_newest_picture_wins_and_other_files_are_ignored(self):
        self.put('old.png', png(10, 10, (0, 255, 0))); self.put('notes.txt', b'hello')
        os.utime(self.app / 'Image/old.png', (1, 1))
        self.put('new.png', png(10, 10, (0, 0, 255)))
        code, status = self.run_app(); self.assertEqual(code, 0, status); self.assertIn('new.png', status)

    def test_unreadable_picture_leaves_logo_untouched(self):
        self.put('broken.png', b'not really a png')
        code, status = self.run_app()
        self.assertEqual(code, 1); self.assertIn('Could not use broken.png', status)
        self.assertEqual(self.logo(), self.original)

    def test_foreign_logo_needs_force(self):
        self.put('mine.png', png(20, 20)); self.assertEqual(self.run_app()[0], 0)
        (self.boot / 'bootlogo.bmp').write_bytes(bmp(64, 48, (9, 9, 9)))  # some other tool changed it
        self.put('second.png', png(30, 30, (0, 0, 255)))
        code, status = self.run_app(); self.assertEqual(code, 1); self.assertIn('changed by something else', status)
        (self.app / 'FORCE').touch()
        code, status = self.run_app(); self.assertEqual(code, 0, status)

    def test_tight_partition_writes_in_place_or_refuses(self):
        self.put('mine.png', png(20, 20))
        self.free_kb = 1  # no room for a second copy, but the old logo's space suffices
        code, status = self.run_app(); self.assertEqual(code, 0, status)
        self.assertEqual(len(self.logo()), len(self.original))

    def test_slot_logo_without_backup_can_be_replaced_not_restored(self):
        (self.boot / 'bootlogo.bmp').write_bytes(SLOT_LOGO)
        code, status = self.run_app(); self.assertEqual(code, 1); self.assertIn('nothing to restore', status)
        self.put('mine.png', png(50, 50))
        code, status = self.run_app(); self.assertEqual(code, 0, status)
        self.assertEqual(len(self.logo()), len(SLOT_LOGO))
        self.assertFalse((self.root / 'BootLogo/original.bmp').exists())

    def test_corrupt_backup_is_refused(self):
        self.put('mine.png', png(20, 20)); self.assertEqual(self.run_app()[0], 0)
        (self.root / 'BootLogo/original.bmp').write_bytes(bmp(64, 48, (7, 7, 7)))
        self.clear()
        code, status = self.run_app(); self.assertEqual(code, 1); self.assertIn('failed verification', status)

if __name__ == '__main__':
    unittest.main()
