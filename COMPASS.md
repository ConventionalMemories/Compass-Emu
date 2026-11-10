# Compass-Emu

A cycle-accurate emulator for the GRiD Compass, built on the 8086 core of
[MartyPC](https://github.com/dbalsom/martypc) by Daniel Balsom (MIT). The original MartyPC
README follows in README.md.

Goals: a complete emulator anyone can use (drives, keyboard, sound, serial), accurate
enough to race the display beam, plus hooks so tools can run and measure code fragments.
All Compass models eventually; the 1101 comes first.

## Status (0.1.0)

- 8086 with prefetch queue and bus timing (MartyPC, `CpuSubType::Intel8086`), with one
  timing fix for ALU r/m,imm writes. SingleStepTests 8086 suite: 0 register/flag/memory
  errors, about 94.5% of tests with exact clock counts.
- 1101 board (`crates/lib/grid_core`): 16K boot PROM, 256K RAM, 80130 interrupt
  controller, IR3 vertical blank, 320x240 screen at 0400h.
- TMS9914 GPIB with the pseudo-DMA window at E0000h; 2101 hard disk (address 4), 2101
  floppy (5) and 2102 floppy (6) at the level of GRiD's disk protocol.
- MM58174 real-time clock.
- 8741 keyboard microcontroller running the real firmware (300067-01 or 300001-02).
- 8087 (`crates/lib/i8087`) on the 8086's ESC/WAIT hooks: 80-bit registers with bit-exact
  arithmetic in all rounding modes, all instructions, exceptions to IR6. Transcendentals
  are computed in 113-bit precision and rounded once. Busy time per instruction is fitted
  to GRiDBench measurements of a real 1101 (mean error 1.8% over 16 tests; a bare FWAIT
  costs ~90 clocks there).
- Display timing (`DisplayTiming` in grid_core/src/screen.rs): 309 clocks a line, 266
  lines (60.8 Hz), IR3 at the start of vertical blank, and display RAM contention: the
  display takes one RAM slot every 12 clocks in the visible part of each line, and a CPU
  access that collides waits. The settings are adjustable and fitted to GRiDBench's 8086
  and memory numbers (mean error 2.0% over 27 tests) until they are measured directly.
  The window shows each screen word as the beam fetched it, so racing the beam (and
  tearing) looks as on the real screen.
- 7220 bubble memory controller with a 384K bubble image (`--bubble`); GRiD's
  interrupt-driven driver (DRQ on IR1) reads and writes it. A blank image must be
  formatted with Initialize Media.
- 8274 serial controller, asynchronous mode, IR0, bit rate from the 80130 baud timer;
  the window version connects it to a TCP port (`--serial tcp:2323`) or a real serial
  port (`--serial COM3`). Synchronous (GRiDLink SDLC) modes are not modelled.
- Sound: the sound DAC (DFE4:2), volume DAC (DFE4:4) and the DTMF chip on 8255 port B,
  played through the PC's sound card (`--mute` to turn it off).
- Boots GRiD-OS (1101 build of CCOS) from a 2101 hard disk image to the Executive.

Not yet: the modem itself (its microcontroller was never dumped), GRiDLink, 1139 and
later models.

## Window version

    cargo build --release -p compass
    compass --hd disk.img      # ROMs from a roms folder next to the program

Windows build from Linux: `rustup target add x86_64-pc-windows-gnu`, install mingw-w64, then
`cargo build --release -p compass --target x86_64-pc-windows-gnu`. PC keys map to Compass
keys by position (Alt = CODE), F12 turbo, F11 screenshot.

## Headless runner

    apt install libudev-dev pkg-config     # Linux, for the MartyPC workspace
    cargo build --release -p gridemu

    GRID_HD=disk.img GRID_KBD_ROM=300067-01.bin \
      target/release/gridemu 1101even.rev3.bin 1101odd.rev3.bin 170 screen.png

Environment variables: `GRID_HD`, `GRID_FLOP` (2102), `GRID_FLOP2101` for drive images;
`GRID_KBD_ROM` for the 8741 firmware; `GRID_KEYS="seconds:text|..."` to type keys
(`{ret} {esc} {tab} {bs} {left} {right} {up} {down} {code+x} {ctrl+x}`);
`GRID_GPIB_TRACE=1` writes gpib_trace.txt; `GRID_DUMP=file` saves RAM at the end.

`gridfrag code.bin` runs a code fragment at 1000:0000 until HLT and prints the clocks and
the 8087 state (`FPU_EXTRA`, `FPU_WAITMIN` override the 8087 timing fit; `DISP_CONTENTION=0`,
`DISP_FETCH_LEN`, `DISP_CPU_LEN`, `DISP_CPU_WRITE_LEN` the display contention).

Headless extras: `GRID_BUBBLE` (+ `GRID_BUBBLE_SAVE`, `GRID_BUBBLE_LOG`), `GRID_SERIAL_IN`
/ `GRID_SERIAL_OUT` files, `GRID_WAV` to record the sound.

Timings still to be measured on real hardware are listed in docs/real-hw-tests.md.

ROMs and disk images are not included.

## Credits

Compass support by Conventional Memories (C.M._), MIT license. Device behaviour is based
on GRiD's own documentation and sources and on MAME's gridcomp, tms9914, grid2102 and
mm58174, i7220 and z80sio drivers (BSD-3-Clause). 8087 arithmetic uses rustc_apfloat (Apache-2.0 WITH
LLVM-exception).
