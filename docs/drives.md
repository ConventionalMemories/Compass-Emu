# Disk drives

The Compass reaches its drives over GPIB: a TMS9914 on the Compass side, and a pseudo-DMA
window (E0000h on the 1101, DFA00h on the split board) that holds the CPU until the next
byte is there. Every drive is a GPIB unit with an address; GRiD-OS's names for them are 4
Hard Disk, 5 Floppy Disk (the 2101's floppy), 6 Portable Floppy (the 2102), 12 Extra Hard
Disk and 13 Extra Floppy Disk.

Each drive comes in two levels:

- **Low level**: the drive box's own processor runs GRiD's firmware, and the emulator wires
  it to the chips the box has. The firmware decides the protocol and the timing. Used when
  the firmware file is in the ROM folder (see [roms.md](roms.md)).
- **Protocol model**: GRiD's disk protocol answered directly, with the timing of the real
  drives' mechanics added. Used without the firmware, or when the profile, `--gpib ...|protocol`,
  `--2101-protocol`/`--2102-protocol` or `GRID_2101=protocol`/`GRID_2102=protocol` asks
  for it.

Real-vs-emulator numbers for both levels are in [accuracy.md](accuracy.md). Code:
`grid_core/src/gpib.rs` (the 9914 and the protocol model), `box2101.rs`, `box2102.rs`,
`i8291.rs`, `i8051.rs`, `wd279x.rs`, `s1410.rs`, `hostdir.rs`.

## Protocol model

### 2101 hard disk

TMS9914 GPIB with the pseudo-DMA window at E0000h; 2101 hard disk (address 4), 2101
floppy (5) and 2102 floppy (6) at the level of GRiD's disk protocol. In exact mode the
hard disk has mechanics like a real 2101 with an ST-225: 3600 rpm, 17 sectors a track,
4 heads, interleave 5, seeks interpolated from real measurements (1 cylinder 26.4 ms,
38 cylinders 80 ms, full stroke 148 ms), a 6-page read-ahead cache that writes drop, and
writes answered at once and done while the next command waits (`HD_NOMECH=1` turns this
off; fast mode never uses it). A multi-page read replies in 512-byte pages, each ending
with EOI, as GRiD's BB2 driver accepts only 512-byte replies. Against a real 1101 + 2101
(GRiDBench 2.1b): same page 19.4 ms (real 19.0), sequential 19.4 (21.1), random 130.0
(130.3), steps of 1/16 120.6 (122.5), 8 pages 65 (53.5).

### Floppies

Floppies (exact mode): 300 rpm, 9 sectors, interleave 5 with side 1 skewed by 7 sectors,
3 ms a cylinder + 30 ms settle, one WD279x command a page. The 2102 reads one page a
command and is slow (about 40 ms to take a command, 60 us a byte over its bit-banged GPIB).
Against a real 2102 with a Gotek (GRiDBench 2.1b): same page 197 ms (real 212),
sequential 121 (133), first/last 395 (415), steps of 1/16 211 (172). With a 2101 hard
disk and no floppy image, the box's floppy slot answers as empty (error 128, as on a
real 2101), so GRiD-OS's polls of address 5 don't hang. `GPIBREG=clock` prints every
9914 register access from that clock on.

## Low-level 2102 Portable Floppy

Built from the 2102 schematic (drawn by JDat) and the firmware.

2102 low-level (when `roms` holds the 2102 firmware `300237-02.bin` or `300237-01.bin`):
the box's own 8051 runs the real ROM (`grid_core::i8051`, `box2102`), wired from the 2102
schematic: 12 MHz (1 us a machine cycle), 2 KB RAM at 1000h, WD2797 at 2000h-2C00h
(`wd279x`, 1 MHz clock: 5.25" step rates and 30 ms settle, MFM 32 us a byte, READY and
HLT tied high), DIP switches at 3000h (address 6), GPIB data latch at 4000h, handshake
lines bit-banged on P1 through a DS75161 (IFC, DRQ, ATN, SRQ, DAV, NRFD, NDAC, EOI), drive
power/select, motor and precompensation on P3, WD INTRQ on INT1. The 9914 meets it at
the handshake level (DAV/NRFD/NDAC/EOI/SRQ/ATN/IFC), so timing comes from the firmware:
get status and write answer without a service request, a read or format with one;
the first access after 10 s idle powers the drive up again (about 1.8 s); the error
block is error code, 0, drive, page (low, high), 0, 0; page 720 passes the ROM's range
check and gives record not found (68h), 721 and up invalid page (66h). Unformatted
images get the protocol model's layout (interleave 5, side 1 skewed 7, IDs 10-18 on
side 1, or 1-9 for a PC boot sector; `FD_IL`, `FD_SKEW`); a format by the 2102 writes
2:1 interleave with no skew and keeps the IDs and positions it wrote. The box starts
4 s ahead of the Compass, as if switched on first (`GRID_2102_EARLY=0`: together, and the
boot PROM's first try fails while the box runs its self test). Against the real 2102
(GRiDBench 2.1b, a Gotek in the box) with the motor running: same page 200 ms (real 212),
sequential 121 (133), random 243 (235), first/last 403 (415), steps of 1/16 196 (172).
GRiDProbe 0.0.13's floppy test: the same page again 181 ms (real 185), after 12-30 s idle
(the box powers the drive down) 1.8-1.9 s (real 1.9-2.0).
`GRID_2102=protocol` uses the protocol model instead; `BOXLOG=1` prints the bytes the box
moves, `FDCLOG=1` every WD2797 command. Bench test: `FD2102_ROM=rom cargo test --test
lle2102`.

## Low-level 2101 Disc System

Built from the 2101's firmware and its Disc Controller II board (P8088, 8284A with a
12 MHz crystal, 8K RAM, Intel P8291A GPIB chip, WD2797, 74LS259 latch) and a Xebec S1410
SASI controller with an ST-225.

2101 low-level (when `roms` holds `300205-05.bin` (20 MB) or `300205-06.bin` (10 MB), or
`2101.bin`; picked from the hard disk image size): the box's own 8088 runs the real ROM
(`box2101`, on `fast86` with bus holds), 4 MHz (12 MHz crystal, 8284A), 8 KB RAM at 0,
ROM at E000h, WD2797 at 4000h-5800h, display at 6000h-6003h (reading 6000h gives the
DIP switches: address 4), Intel 8291A at 8000h-9C00h (`i8291`, line-level handshake),
74LS259 latch at A000h-A00Eh (floppy light, display reset, floppy select, READY hold for
pseudo-DMA, HD light, SASI SEL and RST), SASI status at A000h, SASI data at C000h. INTR =
8291A INT or WD INTRQ (no 8259), NMI = the back-panel test button. The Xebec S1410 stays at
command level (`s1410`: TUR, recal, sense, format, check, read, read long, write, seek,
init params, buffer commands, diagnostics; format fill 6Ch), timed after an ST-225:
3600 rpm, 17 sectors, interleave 5 (the real drive's sector IDs; `S1410_IL`), heads-in
times measured inside a real box (Disk Toolkit 0.4.1: 1 cylinder 9.5 ms, 38 21.5 ms, 611
70.4 ms; `S1410_SEEK`). The Seek command is buffered (it answers after 1.5 ms + 20 us a
cylinder, and Test Ready says not ready until the heads are in); a read's or write's own
seek waits that long again plus 8 ms (`S1410_SETTLE`) before its first sector;
Recalibrate steps back at 3 ms a cylinder; commands decode in 0.44 ms. The box's CPU time
is stretched 4.7% (`BOX_SLOW`). Against a real 1101 + 2101 (GRiDBench 2.1b): same page
21.1 (real 19.0), sequential 26.8 (21.1), 8 pages 63.8 (53.5), random 132.8 (130.3),
steps of 1/16 118.4 (122.5). The 2101 floppy (address 5) goes through the firmware too: its format
writes interleave 3 with no skew, and an empty slot gives error 128 (the WD READY line).
The 4-character display shows in the status bar. `GRID_2101=protocol` uses the protocol
model; `GRID_2101_EARLY` (default 4 s), `GRID_2101_MHZ`. Bench test: `HD2101_ROM=rom
cargo test --test lle2101`.

## PC folder drive

A PC folder as a GRiD drive (`--folder DIR`, `grid_core::hostdir`): at start the folder is
built into a GRiD volume (files in the root, sub-folders as subjects, up to 32 MB with 4 MB
free) as a GPIB unit of its own (13 Extra Floppy Disk next to a 2101, else 5 Floppy Disk;
`--folder-addr`); the volume is named after the folder. On quit,
files GRiD made or changed are written to the folder and files GRiD erased are deleted
there. Host files without a GRiD kind become `name~Text~` (LF line ends to CR LF) and keep
their PC name; characters Windows can't store are written as %XX. The drive answers
with its real size in the status block, which GRiD-OS uses for a volume without a
partition table. Writes with control byte 1 (set status) or 2 (box upload) no longer land
on page 0 of a drive.

## Environment variables

| Variable | What it does |
|---|---|
| `GRID_2101`, `GRID_2102` | `protocol` = the protocol model even with the firmware there |
| `GRID_2101_EARLY`, `GRID_2102_EARLY` | how many seconds the box starts before the Compass (default 4; 0 = together) |
| `GRID_2101_MHZ` | the 2101 box's CPU clock (default 4) |
| `BOX_SLOW` | stretch of the 2101 box's CPU time in per mille (default 47 = 4.7 %) |
| `S1410_IL`, `S1410_SEEK`, `S1410_SETTLE`, `S1410_CMD`, `S1410_BYTE`, `S1410_ECC`, `S1410_SEL`, `S1410_PHASE` | the Xebec S1410's interleave, seek table, settle time and command, byte, ECC, select and phase times |
| `HD_NOMECH` | protocol-model hard disk without mechanics (instant seeks) |
| `FD_NOMECH` | protocol-model floppies without mechanics |
| `FD_IL`, `FD_SKEW` | interleave and side-1 skew for images the emulator lays out itself |
| `FD_ROT`, `FD_STEP`, `FD_SETTLE`, `FD_OV`, `FD_TALK` | protocol-model floppy rotation (us), step (us a cylinder), settle (us), 2102 command overhead (us) and clocks a byte |
| `GPIBLOG` | print disk commands, SRQs and transfer ends with their clock |
| `GPIBREG=clock` | print every 9914 register access from that clock on |
| `GPIB_STALL=N` | hold the Nth byte a drive sends for 10 s (to watch the keyboard controller's DMA watchdog) |
| `BOXLOG` | print the bytes the 2102 box moves and the S1410's commands |
| `FDCLOG` | print every WD2797 command |
