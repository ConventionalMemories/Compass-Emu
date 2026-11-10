# Compass-Emu

A cycle-accurate emulator for the GRiD Compass, built on the 8086 core of
[MartyPC](https://github.com/dbalsom/martypc) by Daniel Balsom (MIT). The original MartyPC
README follows in README.md.

Goals: a complete emulator anyone can use (drives, keyboard, sound, serial), accurate
enough to race the display beam, plus hooks so tools can run and measure code fragments.
All Compass models eventually; the 1101 comes first.

## Status (0.0.1, test build)

- 8086 with prefetch queue and bus timing (MartyPC, `CpuSubType::Intel8086`), with one
  timing fix for ALU r/m,imm writes. SingleStepTests 8086 suite: 0 register/flag/memory
  errors, about 94.5% of tests with exact clock counts.
- 1101 board (`crates/lib/grid_core`): 16K boot PROM, 256K RAM, 80130 interrupt
  controller, IR3 vertical blank, 320x240 screen at 0400h.
- TMS9914 GPIB with the pseudo-DMA window at E0000h; 2101 hard disk (address 4), 2101
  floppy (5) and 2102 floppy (6) at the level of GRiD's disk protocol.
- MM58174 real-time clock.
- 8741 keyboard microcontroller running the real firmware (300067-01 or 300001-02).
- Boots GRiD-OS (1101 build of CCOS) from a 2101 hard disk image to the Executive.

Not yet: bubble memory, serial, modem/sound, display RAM contention, 8087, 1139 and
later models, a window frontend.

## Build and run

    apt install libudev-dev pkg-config     # Linux, for the MartyPC workspace
    cargo build --release -p gridemu

    GRID_HD=disk.img GRID_KBD_ROM=300067-01.bin \
      target/release/gridemu 1101even.rev3.bin 1101odd.rev3.bin 170 screen.png

Environment variables: `GRID_HD`, `GRID_FLOP` (2102), `GRID_FLOP2101` for drive images;
`GRID_KBD_ROM` for the 8741 firmware; `GRID_KEYS="seconds:text|..."` to type keys
(`{ret} {esc} {tab} {bs} {left} {right} {up} {down} {code+x} {ctrl+x}`);
`GRID_GPIB_TRACE=1` writes gpib_trace.txt; `GRID_DUMP=file` saves RAM at the end.

ROMs and disk images are not included.

## Credits

Compass support by Conventional Memories (C.M._), MIT license. Device behaviour is based
on GRiD's own documentation and sources and on MAME's gridcomp, tms9914, grid2102 and
mm58174 drivers (BSD-3-Clause).
