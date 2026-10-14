#!/usr/bin/env python3
"""Regenerate card/Slot/System/cart-art-games.txt from libretro-database's No-Intro DATs.

Cart Art looks a ROM up by its CRC32 first. When that misses (a trimmed or patched ROM, a
hack), it falls back to the GBA header's game code, then to the file name, and needs this
list to turn either into the CRCs the label server knows. It ships on the card so the
Brick never downloads the multi-megabyte DATs itself.

Usage: python3 brick/make-cart-art-games.py [COMMIT]
"""
import pathlib, re, sys, urllib.parse, urllib.request

COMMIT = sys.argv[1] if len(sys.argv) > 1 else '12c21cf79f1e20eff6ac68b80875816c717d0a7a'
SYSTEMS = {'GBA': 'Game Boy Advance', 'GB': 'Game Boy', 'GBC': 'Game Boy Color'}
OUT = pathlib.Path(__file__).parent / 'card/Slot/System/cart-art-games.txt'
GAME = re.compile(r'game \(\s*name "([^"]+)"(.*?)\n\)', re.S)

lines = [f'# No-Intro via libretro-database {COMMIT}: platform, CRC32, serial, name']
for platform, system in SYSTEMS.items():
    url = (f'https://raw.githubusercontent.com/libretro/libretro-database/{COMMIT}/'
           f'metadat/no-intro/{urllib.parse.quote(f"Nintendo - {system}")}.dat')
    text = urllib.request.urlopen(url, timeout=60).read().decode('utf-8', 'replace')
    count = 0
    for name, body in GAME.findall(text):
        crc = re.search(r'\scrc ([0-9A-Fa-f]{8})', body)
        if not crc or '\t' in name:
            continue
        serial = re.search(r'serial "([^"]*)"', body)
        serial = serial.group(1) if serial and re.fullmatch(r'[0-9A-Z]{4}', serial.group(1)) else '-'
        lines.append(f'{platform}\t{crc.group(1).upper()}\t{serial}\t{name}')
        count += 1
    print(f'{platform}: {count} games', file=sys.stderr)
OUT.write_text('\n'.join(lines) + '\n')
print(f'wrote {OUT}', file=sys.stderr)
