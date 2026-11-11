# Compass-Emu

A cycle-accurate emulator for the GRiD Compass, built on the 8086 core of
[MartyPC](https://github.com/dbalsom/martypc) by Daniel Balsom (MIT). The original MartyPC
README follows in README.md.

Goals: a complete emulator anyone can use (drives, keyboard, sound, serial), accurate
enough to race the display beam, plus hooks so tools can run and measure code fragments.
Models: 1101, 1109, 1121, 1129, 1131 and 1139 (`--model`).

## Status (0.3.3)

- 8086 with prefetch queue and bus timing (MartyPC, `CpuSubType::Intel8086`), with one
  timing fix for ALU r/m,imm writes. SingleStepTests 8086 suite: 0 register/flag/memory
  errors, about 94.5% of tests with exact clock counts.
- A fast whole-instruction 8086 core for build mode (`grid_core::fast86`), switchable at any
  instruction boundary; same SingleStepTests results for registers, flags and memory.
- 1101 board (`crates/lib/grid_core`): 16K boot PROM, 256K RAM, 80130 interrupt
  controller, IR3 vertical blank, 320x240 screen at 0400h.
- TMS9914 GPIB with the pseudo-DMA window at E0000h; 2101 hard disk (address 4), 2101
  floppy (5) and 2102 floppy (6) at the level of GRiD's disk protocol.
- MM58174 real-time clock.
- 8741 keyboard microcontroller running the real firmware (300067-01 or 300001-02).
- 8087 (`crates/lib/i8087`) on the 8086's ESC/WAIT hooks: 80-bit registers with bit-exact
  arithmetic in all rounding modes, all instructions, exceptions to IR6. Transcendentals
  are computed in 113-bit precision and rounded once, except FPTAN, which follows the
  chip's own CORDIC + 3r/(3-r^2) method (`fptan.rs`): its partial Y/X pair (X near 3) and
  a busy time that depends on the argument (last bits still to be pinned down on hardware,
  test T1 in docs/real-hw-tests.md). Busy time per instruction is the data-sheet
  time (a few adjusted to a real 1101). 8087 code is slow on a real Compass because GRiD-OS's
  loader turns the WAIT before each 8087 instruction into INT 3 (handler: WAIT, IRET), about
  95 clocks a time; with that, the full GRiDBench run in the emulator matches a real 1101
  within 3% on 14 of its 16 8087 lines (FCOM+FSTSW and FWAIT 6% fast). Raw fragments with
  real WAIT bytes (gridfrag) are faster than GRiD programs for the same reason.
- Display timing (`DisplayTiming` in grid_core/src/screen.rs): 274 clocks a line, 306
  lines (59.6 Hz), IR3 as the picture starts at the top (all from GRiDBench 2.0 on a real
  1101: frame length and beam finder), and display RAM contention: the
  display takes one RAM slot every 12 clocks in the visible part of each line, and a CPU
  access that collides waits. The settings are adjustable and fitted to GRiDBench's 8086
  and memory numbers (mean error 2.0% over 27 tests) until they are measured directly.
  The window shows each screen word as the beam fetched it, so racing the beam (and
  tearing) looks as on the real screen.
- 7220 bubble memory controller with a 384K bubble image (`--bubble`); GRiD's
  interrupt-driven driver (DRQ on IR1) reads and writes it. A blank image must be
  formatted with Initialize Media.
- Bus details measured on a real 1101: empty memory and ports read the address still on
  the bus (low byte on even addresses, A8-A15 on odd ones), D0000-DEFFF reads FF on even
  bytes, the PROM also shows at F0000-FBFFF, ports 00-7F mirror the 80130 every 10h,
  RTC/ID reads have wait states, timers 0 and 2 count and timer 1 doesn't.
- 8274 serial controller, asynchronous mode, IR0, bit rate from the 80130 baud timer;
  the window version connects it to a TCP port (`--serial tcp:2323`) or a real serial
  port (`--serial COM3`). Synchronous (GRiDLink SDLC) modes are not modelled.
- Sound: the sound DAC (DFE4:2), volume DAC (DFE4:4) and the DTMF chip on 8255 port B,
  played through the PC's sound card (`--mute` to turn it off).
- Boots GRiD-OS (1101 build of CCOS) from a 2101 hard disk image to the Executive.
- Models (`GridModel` in grid_core/src/machine.rs, `--model` in the window version,
  `GRID_MODEL` headless):

  | Model | Board | RAM | Screen | Boot PROM used |
  |-------|-------|-----|--------|----------------|
  | 1101 | original | 256K | 320 x 240 | 1101 rev 3 |
  | 1109 | original | 512K | 320 x 240 | its own if present, else 1101 rev 3 |
  | 1121 | split | 256K | 320 x 240 | its own if present (never dumped), else 1129, else 1101 rev 3 |
  | 1129 | split | 512K | 320 x 240 | 1129 (if missing, 1101 rev 3) |
  | 1131 | split | 256K | 512 x 256 | its own if present (never dumped), else 1139 |
  | 1139 | split | 512K | 512 x 256 | 1139 |

  Split board (Compass II): GPIB pseudo-DMA window at DFA00h, video/memory control at
  DFE00h (DFE00 = 1 selects the video chip, DFE04 bit 2 clear = large screen; the 113x OS
  writes 0Bh or 0Ah, the PROM 0Eh), serial port latches at DFE10h, empty application ROM
  sockets at 80000h. The 113x build of GRiD-OS turns on the 512 x 256 screen; until then a
  113x shows the 320 x 240 picture centred. Large-screen timing is estimated (the same
  309-clock line, 269 lines, 32 display fetches a line, one every 8 clocks) and fits
  GRiDBench on a real 1137 at 3.1% mean error.
  All six boot to the Executive (1131/1139 with the 113x build, the others with the 11xx
  build). The 1139 PROM can't stand in for the missing 1121/1129 PROMs: it keeps its data
  at 0440:0, where the 11xx build of CCOS loads (02D8:0 up), and stops with "Checksum
  error in CCOS". The 1129 PROM (data at 0298:0, like the 1101's) boots the 11xx build on
  the plain split-board map, and also serves the 1121. Without it the 1101 rev 3 PROM
  stands in, with the split board also answering the 1101's DMA window at E0000h
  (`GridBoard::e0000_dma`). ROM choice: grid_core/src/roms.rs.

Not yet: the modem itself (its microcontroller was never dumped), GRiDLink, application
ROM sockets.

## Build mode

`compass do` runs one command on the Development Executive command line, headless and as
fast as the PC allows, then saves the screen and writes the disk back:

    compass do [--model M] --hd disk.img [--roms DIR] [--shot screen.png] [--start S] \
               [--max-seconds N] [--bubble F] [--state F] [--exact] "Do build"

It runs on the fast CPU core (`grid_core::fast86`, about 60x real time) unless `--exact` is
given (MartyPC's cycle-exact core, about 8x). The fast core executes whole instructions with
approximate clock counts: no prefetch queue, display contention or RAM wait states, and the
display is captured once per frame instead of as the beam passes. Devices still run on the
same clock (the GPIB DMA window keeps its wait states, so disk transfers pace as before), so
GRiD-OS, the compiler and the linker behave the same; only timing differs. It passes all
630,000 SingleStepTests 8086 v1 tests (`SST_DIR=.../8086/v1 cargo test -p grid_core --test
fast86_sst`), except the flags a divide error pushes (the real CPU pushes what its divide
microcode left, the fast core its own). hello.run and GRiDTiles (11 modules) build
byte-identical on both cores. `GridMachine::set_fast` switches cores between instructions.

The model defaults to the 1129 (512K, 11xx build of GRiD-OS). It waits for a steady `=>`
prompt, types the command, and stops when a new prompt ends the screen ("GRIDBUILD done
after N emulated seconds"; exit code 2 at the time limit). Code: `grid_core::session`.
`--state F` boots once and saves the machine at the prompt into F (`GridMachine::save_state`:
RAM, CPU, all devices, 8087; not the disk image); later runs resume from F in under a
second with the disk from `--hd` attached, which may have gained files since (GRiD-OS
re-reads the bitmap on every attach).

`tools/gridbuild.py` (the one-click build of the dev environment) uses it in place of MAME
when `compass-emu\compass.exe` is in the GRiD-MAME folder: it switches its temporary disk
copy to the build of GRiD-OS the model needs (gridmodel.py), unpacks MAME's ROM sets into
`compass-emu\roms` and calls `compass do`. `GRID_MODEL` picks another model, `GRID_EMU`
another compass.exe; without compass.exe it falls back to MAME's grid1139 as before. The
gridbuild.py keeps the saved state and the prepared disk it booted from in
`compass-emu\states` (one pair per model, merged disk and compass.exe), and checks the disk
with `gridfs check` after each resumed build (on failure it builds again with a full boot).
hello.pas: 3.3 s per build on the fast core (the compile itself is 112 emulated seconds;
15 s with `GRID_EXACT=1`, which passes `--exact`). GRiDTiles: 48 s (exact: 6 min). 256K
models (1101, 1121, 1131) can't build: the compiler stops with "Main memory (RAM) is full".
The
disk needs `Do~Run~` from the 3.1.5 floppy in Programs (the disk's own `Do~Run Com~` gives
"33 File is not available"). 

## Claude Code hook (MCP server)

`compass mcp` is an MCP server on stdin/stdout. Add it to Claude Code with

    claude mcp add compass -- C:\GRiD-MAME\compass-emu\compass.exe mcp

or in `.mcp.json`: `{"mcpServers": {"compass": {"command": "C:\\GRiD-MAME\\compass-emu\\compass.exe", "args": ["mcp"]}}}`.

| Tool | What it does |
|------|--------------|
| `grid_build` | gridbuild.py build SOURCE: compiler/linker messages plus the GRiD screen at the end |
| `grid_boot` | start a Compass from a hard disk image (a copy in memory), wait for the prompt, return the screen; `fast: true` uses the fast core |
| `grid_type` | type keys (`{ret}`, `{code+x}` ...), optionally wait for the next `=>` prompt; returns the screen |
| `grid_run` | run N emulated seconds, return the screen |
| `grid_screen` | the screen (PNG), emulated time, CS:IP, screen mode |
| `grid_read_memory` | hex dump at SSSS:OOOO or a linear address |
| `grid_save_disk` | write the running Compass's disk to a file |
| `grid_fragment` | time 8086/8087 machine code (hex or .bin) in CPU clocks, cycle-accurate with display contention |

`GRID_ROOT` = the GRiD-MAME folder (default: the folder above compass.exe), `GRID_MODEL` =
default model (1129). Code: crates/bin/compass/src/mcp.rs.

## Window version

    cargo build --release -p compass
    compass --hd disk.img      # ROMs from a roms folder next to the program
    compass --model 1139 --hd disk113x.img

Boot PROMs are looked for under MAME's names (`1139even.ccprom.bin`, `1139odd.ccprom.bin`,
`1101even.rev4.bin` ..., the 1101 falling back to `1101even.rev3.bin`), or given with `--prom-even`/`--prom-odd` or `--prom` (one 16K
file, bytes interleaved). The split board models use the 300001-02 keyboard firmware when
it is in the roms folder.

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
`DISP_FETCH_LEN`, `DISP_CPU_LEN`, `DISP_CPU_WRITE_LEN` the display contention; `FRAG_MODEL`
the model, `RAM_EXTRA_WAIT` extra wait states on every RAM access).

Headless extras: `GRID_BUBBLE` (+ `GRID_BUBBLE_SAVE`, `GRID_BUBBLE_LOG`), `GRID_SERIAL_IN`
/ `GRID_SERIAL_OUT` files, `GRID_WAV` to record the sound.

Timings still to be measured on real hardware are listed in docs/real-hw-tests.md.

ROMs and disk images are not included.

## Credits

Compass support by Conventional Memories (C.M._), MIT license. Device behaviour is based
on GRiD's own documentation and sources and on MAME's gridcomp, tms9914, grid2102 and
mm58174, i7220 and z80sio drivers (BSD-3-Clause). 8087 arithmetic uses rustc_apfloat (Apache-2.0 WITH
LLVM-exception).
