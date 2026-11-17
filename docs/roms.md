# ROM files

Compass-Emu ships no GRiD ROMs. You supply them, from MAME's ROM sets or dumps of your own
machine. The setup page's **Import ROMs...** (or `compass import-roms FOLDER|ZIP`) looks at
every file in a folder and in the zips inside it, recognises the Compass ROMs by size and
CRC-32, and copies them into the ROM folder (`roms` next to compass.exe, or the folder the
profile names) under the names below. A 16K boot PROM dump with the even and odd bytes
interleaved is recognised too and split into its two halves.

## The list

| File | Size | CRC-32 | What it is | Used by | Needed |
|---|---:|---|---|---|---|
| `1101even.rev3.bin`, `1101odd.rev3.bin` | 8K each | `bd7a385a`, `751c893e` | 1101 boot PROM, rev 3 | 1101, 1109 (when rev 4 is missing) | one 1101 pair for the 110x |
| `1101even.rev4.bin`, `1101odd.rev4.bin` | 8K each | `051b11e5`, `898d7ce2` | 1101 boot PROM, rev 4 (later boards) | 1101, 1109; 1121/1129 without their own | preferred over rev 3 |
| `1129even.ccprom.bin`, `1129odd.ccprom.bin` | 8K each | `0ede3636`, `07b29b05` | 1129 boot PROM | 1121, 1129 | recommended for the 112x (without it the 1101 PROM stands in, slower boot) |
| `1139even.ccprom.bin`, `1139odd.ccprom.bin` | 8K each | `67071849`, `13ed4bf0` | 1139 boot PROM | 1131, 1137, 1139 | yes for the 113x |
| `300067-01.bin` | 1K | `48cb0b57` | 8741 keyboard controller firmware | every model but the 1137 | yes (either keyboard file) |
| `300001-02.bin` | 1K | `cebbccdc` | 8741 keyboard controller firmware, as on the 1137 | 1137 | optional; each keyboard file stands in for the other |
| `300237-02.bin` | 4K | `85850e38` | 2102 floppy firmware (8051), newer | low-level 2102 | optional |
| `300237-01.bin` | 4K | `6ea21f23` | 2102 floppy firmware, older (format doesn't wait for the end of the track) | low-level 2102 | optional |
| `300205-05.bin` | 8K | `769c3bab` | 2101 firmware (8088), 20 MB box | low-level 2101 with a 20 MB image | optional |
| `300205-06.bin` | 8K | `34e191f5` | 2101 firmware, 10 MB box | low-level 2101 with a 10 MB image | optional |
| `16020-007.bin` | 2K | `8b526845` | internal modem's 8749 firmware | the modem on every model but the 1137 | optional (without it Modem~Device~ can't start) |
| `8087mc.txt` or `8087mc.bin` | 1648 lines / 3296 bytes | (by name) | 8087 microcode ROM | the microcode 8087 | optional (without it the instruction-level 8087 runs) |

The emulator also looks for `2101.bin` (as the 20 MB 2101 firmware), `2102.bin` (as the
2102 firmware) and `modem.bin` (as the modem firmware) when the numbered files are missing.

## Where they come from

- **MAME ROM sets**: `grid1101.zip` (1101 PROM), `grid1129.zip` (1129 PROM), `grid1139.zip`
  (1139 PROM) and `grid_keyboard.zip` (300067-01). Older sets carry only the rev 3 1101 PROM.
- **Dumps of a real machine**: the boot PROMs and application ROMs can be read on a Compass
  with Rom Toolbox. The keyboard controller, the modem's 8749 and the drive boxes' EPROMs (a
  2764 in the 2101, a 2732 in the 2102) are read from the chips.
- **The 8087 microcode**: the dump named 8087mc.txt in `tools/8087mc/bin` of
  [github.com/a-mcego/granite](https://github.com/a-mcego/granite) (1648 lines of the letters
  A to P, one per 16-bit word). Import ROMs takes it by its file name. See
  [8087-microcode.md](8087-microcode.md).

## Which file each model and device uses

- **Boot PROM**: the model's own pair if it is there, else the one it shares: the 1109 uses
  the 1101's, the 1121 the 1129's (or else the 1101's), the 1131 and 1137 the 1139's. The
  1101 takes rev 4 before rev 3. The 1139 PROM can't stand in for the 1129's: it keeps its
  data where the 11xx build of GRiD-OS loads. When the 1101 PROM stands in on a split board,
  the board also answers the 1101's DMA window. `--prom FILE` (one 16K interleaved file) or
  `--prom-even`/`--prom-odd` override it.
- **Keyboard**: 300001-02 on the 1137, 300067-01 on the others; each stands in for the other.
  (The early 1101's 300027-01 has not been dumped.) `--kbd FILE` overrides it.
- **Drives**: the 2101 picks the -05 firmware for a 41,616-page (20 MB) hard disk image and
  the -06 for a 20,808-page (10 MB) one; any other size runs the protocol model.
  `--rom-2101`, `--rom-2101-10`, `--rom-2102` or a GPIB device's own firmware field override
  it. See [drives.md](drives.md).
- **Modem**: `--rom-modem FILE` overrides it; `--no-modem` leaves the MCU out.
- **8087**: `--fpu-rom FILE`, the setup page's field, or `I8087_MICROCODE`.

The setup page's ROMs card shows, for the selected profile, every ROM it uses and the file
found for each. The code that picks them is `grid_core/src/roms.rs`; the import table is
`crates/bin/compass/src/romimport.rs`.

## Application ROMs

The Compass II models have four sockets for application ROMs (GRiD software on ROM, such as
CCOS or GRiDWrite). These are not looked for in the ROM folder: give each chip on
the setup page or with `--rom0` to `--rom3`, as a 32, 64 or 128 KB dump (for example the
"Socket n" files Rom Toolbox saves). See the user guide's
[Application ROMs](user-guide.md#application-roms).
