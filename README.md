# Slot. for TrimUI Brick

A port of [Slot.](https://github.com/BrandonKowalski/slot), a lightweight frontend for Game Boy, Game Boy Color, and Game Boy Advance games, originally developed by Brandon T. Kowalski for the Anbernic RG SP.

This version brings Slot. to the **original TrimUI Brick**, with support for **stock firmware**.

> [!NOTE]
> This is an unofficial port of Slot. for the TrimUI Brick. Please report any bugs or issues related to this port in this repository rather than in the original upstream project.
> 
> **Work in Progress:** This port is still under active development and may contain bugs, incomplete functionality, or missing features.
>
> Improvements are planned over the coming days, with a particular focus on refining shortcuts combinations.
>
> Thank you for your patience as the port continues to improve!

## Installation

Follow the steps below to install **Slot.** on your original TrimUI Brick.

#### 1. Back Up Your SD Card
Before proceeding, back up any important files from your existing SD card to your PC. **Formatting the card will erase all existing data.**

#### 2. Download the Latest Release
Download `slot-brick-<version>-sd-card.zip` from the [Releases](../../releases) page and extract the archive to your PC.

#### 3. Prepare Your SD Card
Format your SD card using a tool such as [Rufus](https://rufus.ie/) with the following settings:

- **File system:** `FAT32`
- **Volume label:** `SLOT`

#### 4. Copy Slot. to Your SD Card
Copy **all contents** of the extracted `SD-CARD` folder to the **root directory** of your SD card.

> [!IMPORTANT]
> Make sure to include all hidden files and folders, particularly `.slot-boot`.

#### 5. Add Your Games and Saves
Copy your games, save files, save states, and BIOS files from your backup into their respective folders within the `Slot/` directory.

> [!NOTE]
> **Game labels (cartridge artwork)** can be created using [Cart Studio](https://slot-cfw.fyi/) by Brandon T. Kowalski.

#### 6. Launch Slot.
Insert the SD card into your TrimUI Brick and power on the device. **Slot. should launch automatically.**

> [!NOTE]
> **Known issue:** On first launch or after restarting the device, the stock firmware menu may appear instead of Slot.
>
> If this happens, navigate to **Apps → Slot** and launch it manually. This should resolve the issue and allow Slot. to launch automatically.

## Controls

**Buttons:** the three small buttons below the screen are `HOME` (left, labelled Menu), `SELECT`
(middle) and `START` (right). `L2`/`R2` are the rear shoulder buttons. The volume rocker is on the
left side, and the `Fn` slider and `Power` are on the right side. `F1`/`F2` are not used.

### On the carousel

| Input             | Action                                  |
| ----------------- | --------------------------------------- |
| `Left` / `Right`  | Browse the carousel                     |
| `Up` / `Down`     | Jump to the previous or next letter     |
| `L1` / `R1`       | Switch between the GB, GBC and GBA shelves |
| Tap `A`           | Resume the last save state              |
| Hold `A`          | Start the game fresh                    |
| `START`           | Choose the emulator core                |
| `HOME`            | Open the menu (settings, Wi-Fi, About)  |

### In game

| Input                       | Action                                                        |
| --------------------------- | ------------------------------------------------------------- |
| Hold `HOME`                 | Save state, eject the cart and return to the carousel         |
| Double-tap `HOME`           | Save state switcher (load, delete, or undo within 30 seconds) |
| `SELECT` + `B`              | Toggle LEDs                                                   |
| `SELECT` + `HOME`           | multiplayer Host/Join                                         |                    
| `SELECT` + `R1`             | Save state                                                    |
| `SELECT` + `L1`             | Load the most recent save state                               |
| `SELECT` + `A`              | Next / previous shader                                        |
| `SELECT` + `X`              | GBA: switch between 3:2 and 4:3 full screen (saved per game)  |
| `SELECT` + `Y`              | Toggle colour correction                                      |
| `SELECT` + `Up` / `Down`    | Adjust brightness                                             |
| `SELECT` + `Left` / `Right` | Adjust blue light filter                                      |
| `L1` / `R1`                 | GB/GBC: stretched / original picture                          |
| Hold `L2`                   | Rewind                                                        |
| Hold `R2`                   | Fast-forward                                                  |
| Double-tap `R2`             | Lock fast-forward on (press again to unlock)                  |

### System

| Input                  | Action                                    |
| ---------------------- | ----------------------------------------- |
| Volume `+` / `−`       | Change the volume                         |
| `Fn` slider            | Mute / unmute                             |
| Press `Power`          | Sleep / wake (game state is saved first)  |
| Hold `Power`           | Shut down, or exit to the stock OS        |

A `/` means either button; a `+` means hold the first button and press the second.

## Features

- Support for the Brick's screen, controls and LEDs (on/off, brightness, colour and effect)
- Deep sleep with low battery drain, plus auto-sleep after 5 min idle in menus or 15 min in game
- Adaptive CPU clocks for smooth audio
- Wi-Fi, plus a two-player link over Wi-Fi *(experimental)*
- Reliable startup with slow-mounting SD cards

### Apps

These appear in the stock OS under **Apps**:

| App       | What it does                                                  |
| --------- | ------------------------------------------------------------- |
| **Slot**  | Starts Slot and sets it to start automatically                |
| **Logo**  | Sets your own boot logo, or restores the original             |
| **Check** | Reports why Slot did or did not start (changes nothing)       |
| **Stock** | Turns off Slot autostart and restores the stock startup       |

## Multiplayer (experimental)

1. Install the same Slot version on both Bricks, with the same game, core and BIOS.
2. In game, press `SELECT` + `HOME` to open the game menu, then choose Host or Join.
3. Start **Host** on one Brick first, then **Join** on the other.

This hasn't been tested on two physical devices yet. If it fails, please
attach `Slot/Logs/multiplayer.log` and `Slot/Logs/slot.log` to your issue.

## Troubleshooting

- **Stock menu appears instead of Slot:** run **Apps → Check** and read `startup-report.txt` on the card.
- **Skip Slot at startup:** create an empty file named `Slot/DISABLE_AUTOBOOT`.
- **Disable auto-sleep:** create an empty file named `Slot/DISABLE_AUTO_SLEEP`.

## Development

Brick-specific code is gated behind the `brick` Cargo feature. Device scripts and apps live in
[`brick/`](brick/), and the change history is in [`brick/CHANGELOG.md`](brick/CHANGELOG.md).

## Credits

- [Slot.](https://github.com/BrandonKowalski/slot) by Brandon T. Kowalski
- Emulation by [mGBA](https://mgba.io) (endrift) and gpSP, via [libretro](https://www.libretro.com)
- TrimUI Brick port by LordNordyx
