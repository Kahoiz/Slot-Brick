# Slot. for TrimUI Brick — v0.23

An unofficial port of **Slot.**, Brandon T. Kowalski's GB/GBC/GBA frontend for the Anbernic RG SP,
to the **original TrimUI Brick** on stock firmware. Port by **LordNordyx**.

> **Note:** Report problems at <https://github.com/LordNordyx/slot-brick>, not to the upstream project.

This is a complete SD card install. You don't need any earlier release, and copying the card
doesn't flash anything to the device.

---

## Installation

1. **Back up.** Power off the Brick and copy your entire current SD card to your PC.
2. **Prepare a card.** Use an empty SD card; a spare is safest. If you reuse your current card,
   verify the backup first, then delete everything on it (no reformat needed).
3. **Copy Slot.** Copy everything **inside** the `SD-CARD` folder to the root of the card,
   including the hidden `.slot-boot` folder. Don't copy the `SD-CARD` folder itself.
4. **Restore your files** from the backup into the matching folders:
   - `Slot/Games`, `Slot/Saves`, `Slot/States`, `Slot/BIOS`
   - `Slot/Config`, `Slot/Labels`, `Slot/Wallpapers`
   - Any `Slot/BootLogo/original*` files

   ⚠️ Do **not** restore old `Slot/System`, `trimui`, `Tools` or `Apps` folders.
5. **Start.** Insert the card and power on. If the stock menu appears instead of Slot, open
   **Apps → Slot** once to set up automatic startup.

---

## Controls

`HOME` is the left small button below the screen (labelled Menu) and `SELECT` is the middle one.
`A + B` means hold A and press B; `A / B` means either button.

### On the carousel

| Button           | Action                                  |
| ---------------- | --------------------------------------- |
| `Left` / `Right` | Browse games                            |
| `L1` / `R1`      | Switch between GB, GBC and GBA shelves  |
| Tap `A`          | Resume from the last save state         |
| Hold `A`         | Start the game fresh                    |
| `START`          | Choose the emulator core                |
| `HOME`           | Menu (settings, Wi-Fi, About)           |

### In game

| Button                      | Action                                              |
| --------------------------- | --------------------------------------------------- |
| Hold `HOME`                 | Save, eject and return to the carousel              |
| Double-tap `HOME`           | Save state switcher                                 |
| `HOME + B`                  | LEDs on / off                                       |
| `SELECT + HOME`             | Game menu (multiplayer Host / Join)                 |
| `SELECT + R1`               | Save state                                          |
| `SELECT + L1`               | Load the latest save state                          |
| `SELECT + A` / `B`          | Next / previous shader                              |
| `SELECT + X`                | GBA: 3:2 or 4:3 full screen (saved per game)        |
| `SELECT + Y`                | Colour correction on / off                          |
| `SELECT + L2` / `R2`        | GB: previous / next colour palette                  |
| `SELECT + Up` / `Down`      | Brightness                                          |
| `SELECT + Left` / `Right`   | Blue light filter                                   |
| `L1` / `R1`                 | GB/GBC: stretched / original picture                |
| Hold `L2`                   | Rewind                                              |
| Hold `R2`                   | Fast-forward (double-tap to lock it on)             |

### System

| Button             | Action                                 |
| ------------------ | -------------------------------------- |
| Volume `+` / `−`   | Volume (press both together to mute)   |
| `Fn` slider        | Mute / unmute                          |
| Press `Power`      | Sleep / wake                           |
| Hold `Power`       | Shut down, or exit to the stock OS     |

**Cart art:** connect to Wi-Fi, then open **HOME → Cart Art** (greyed out while Wi-Fi is off).
**GET CART ART** gives every game with no label a scan of its real cartridge label; labels you
already have are never replaced. The scans come from the slot art set behind
[Cart Studio](https://studio.slot-cfw.fyi) (scans from ScreenScraper), downloaded over HTTPS.
A game is found by the ROM's checksum, or for a trimmed or patched ROM by its file name
(helped by the GBA game code in its header). ROM hacks and homebrew keep the label with their
file name, so they never look like the game they are based on. Each ROM is read once and its
checksum kept in `Slot/System/Cache/crc32.txt`.

`Slot/Logs/cart-art.txt` lists each game and what it got. Below GET CART ART, every game with a
label is listed: press A twice on one to delete its label, then GET CART ART fetches a new one.

**RetroAchievements:** **HOME → RetroAchievements** (greyed out while Wi-Fi is off) logs in to
your [RetroAchievements](https://retroachievements.org) account and tests the connection.
Your password is sent once, over HTTPS, and never stored: Slot keeps only the login token, in `Slot/Config/retroachievements.txt`; delete that file or choose **LOG OUT** to forget
it. While logged in, games earn achievements in softcore mode (save states, rewind and
fast-forward stay allowed): a small toast slides into the top-right corner with how many a
game has when it starts, and with each unlock and its badge as it happens. A game's badges
are fetched when it starts and kept in `Slot/System/Cache/Badges`. Unlocks made offline are sent once the Brick is back online, while the game is
still running. Hardcore mode isn't available.

**Auto-sleep:** the Brick sleeps after 5 minutes idle in menus or 15 minutes in a game.
To turn this off, create an empty file named `Slot/DISABLE_AUTO_SLEEP`.

---

## Apps

These appear in the stock OS under **Apps**.

| App       | What it does                                                         |
| --------- | -------------------------------------------------------------------- |
| **Slot**  | Starts Slot and sets it to start automatically                       |
| **Logo**  | Sets your own boot logo, or restores the original (see below)        |
| **Check** | Reports why Slot did or did not start, in `startup-report.txt`       |
| **Stock** | Turns off Slot autostart and restores the stock startup              |

### Custom boot logo

1. Put one image (`.png`, `.jpg`, `.jpeg` or `.bmp`) in `Apps/BootLogo/Image`.
2. Open **Apps → Logo**, then restart.

To restore the original, empty the `Image` folder and open **Logo** again.

> **Keep a backup:** the first run saves the original logo to `Slot/BootLogo/original.bmp`.
> Copy that file to your PC; it's the only way back.

---

## Multiplayer (experimental)

1. Install this same version on both Bricks, with the same game, core and BIOS.
2. In game, press `SELECT + HOME`.
3. Choose **Host** on one Brick first, then **Join** on the other.

No router is needed. This hasn't been tested on two physical devices yet. If it fails, send
`Slot/Logs/multiplayer.log` and `Slot/Logs/slot.log`.

---

## Troubleshooting

| Problem                               | Fix                                                                 |
| ------------------------------------- | ------------------------------------------------------------------- |
| Stock menu appears instead of Slot    | Open **Apps → Check** and read `startup-report.txt` on the card     |
| You want to skip Slot at startup      | Create an empty file `Slot/DISABLE_AUTOBOOT`; delete it to re-enable |
| You need stock emulators or RetroArch | They aren't included; keep your original card backup                |
