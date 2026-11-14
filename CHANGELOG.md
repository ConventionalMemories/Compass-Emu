# Changelog

Compass-Emu versions (newest first). MartyPC's own changelog is in [MARTYPC-CHANGELOG.md](MARTYPC-CHANGELOG.md).

## 0.11.0

GUI for everyday use:
- Full screen: View > Full screen, or double-click the Compass screen
  (again to leave it). Menus, toolbar and status bar hide while it is on.
- The window size and maximized state, scaling, screen colour, toolbar and
  status bar are kept from one start to the next (in compass-profiles.json).
  View > Toolbar hides the toolbar.
- Drag and drop: drop a floppy image on the running machine to insert it in
  the 2102 floppy drive (Shift: the 2101 floppy drive), or a .state file to
  load it. On the setup page dropped floppy, hard disk and bubble images and
  folders go into the selected profile.
- Recent disks: Media > Recent into the 2102 / 2101 drive, and Recent menus
  beside the disk fields on the setup page (the last 8 floppies and hard disks).
- ImageDisk .IMD floppies: used through the .img beside them, made from the
  .IMD when it is missing (GRiD 2102 and IBM sector numbering).
- 8087 core per machine: Computer > 8087 on the setup page (and --fpu-micro,
  --fpu-rom) runs the 8087 on its own microcode ROM, which you supply; the
  setup page says which ROM file it found (docs/8087-microcode.md).
- Help > Keyboard map lists every Compass key and its PC key, and the
  emulator keys.

Machine setup, drives and keys:
- ROMs card on the setup page, per profile: each ROM the machine uses (boot PROM,
  keyboard 8741, 2102 firmware, 2101 firmware for the 20 MB and the 10 MB box,
  8087 microcode) with the file found and a field to pick another file. "Import
  ROMs..." takes a folder (zips inside it too), knows the GRiD ROMs by checksum
  whatever their file names, and copies them into the profile's ROM folder under
  the names the emulator looks for (a 16K boot PROM file is split into its even
  and odd halves). Also `compass import-roms SOURCE [ROMFOLDER]`.
- Emulation level per drive: Drives > "2101 box" and "2102 floppy" choose Low level
  (the drive's own firmware runs) or Protocol model. The status bar shows LL or P
  beside each drive; hover for the details. Also --2101-protocol, --2102-protocol,
  --rom-2101, --rom-2101-10 and --rom-2102.
- 2101 front panel: the box's 4-character LED display and its drive lights (View >
  2101 front panel, or click the small display in the status bar). It shows what
  the 2101 firmware writes to the display, so only with the low-level 2101.
- Keys on Windows: a held key no longer goes on after it is let go (Windows key
  repeat queued extra presses), and CODE+Esc / CODE+Space reach the Compass
  instead of switching windows (the keyboard hook now runs on its own thread, so
  Windows no longer drops it when a frame is slow).

Build and run in the window:
- Project > Project window: pick a project folder (NAME.pas, NAME.asm, ...) and a
  development disk (the machine's hard disk by default), then Build, Build and run
  or Debug build. compass.exe builds by itself, no Python: on a hidden Compass
  (1129, or 1139 for a 113x disk) with the fast core while your machine keeps
  running. The first build of a disk boots it (about a minute) and keeps that
  state, later builds take a few seconds. Results go to the project folder like
  build.bat: NAME.run, NAME.lst, screen.png, output\. Also `compass build PROJECT
  --disk DISK [--debug]`. The toolbar has a Build and run button.
- Build and run copies NAME~Run~ into the running machine's PC folder drive.
- PC folder drive: files added or changed on the PC show up while the machine
  runs (checked each second, once the GRiD has left the drive alone for two
  seconds). A file the GRiD changed too is left as the GRiD has it.

Extras:
- Edit > Paste text types the PC clipboard's text on the Compass keyboard (one key
  at a time, about 14 a second; characters with no Compass key are left out).
  Edit > Copy screen puts the Compass screen on the clipboard as a picture.
- Speed slider (toolbar and Machine menu): 10 % to 400 % of a real Compass;
  double-click it for 100 %.
- On-screen Compass keyboard (View, or Keyboard on the toolbar): click a key to
  type it, hold the mouse button to hold it; SHIFT, CTRL and CODE stay on for the
  next key.
- View > EL panel look: glow around lit pixels, dark gaps between pixels (at 3x and
  more) and a short afterglow.

8087:
- 8087 on its own microcode (optional): with `FPU_CORE=micro` and a microcode ROM file
  you supply (`I8087_MICROCODE` or `roms/8087mc.txt`, see docs/8087-microcode.md) the
  arithmetic, loads and stores, FSQRT, FPREM, FSCALE, BCD and the five transcendentals
  run from the chip's microcode ROM instead of the instruction-level model. Of 141
  transcendental and square root results dumped from a real 1101, 134 come out bit for
  bit and the other 7 are one unit off.
- The microcode 8087 fitted to 8087 Bench on a real 1101 and 1137 (2181 tests): of 1588
  real result lines 34 still differ (were 1441), and the busy time now comes from the
  micro-instructions that ran (mean error about 2.5 % over 2068 timed tests, was 40 % with the
  tables). Both 8087 cores: FINIT keeps the condition bits, the B bit shows a pending
  unmasked exception, the IR bit is derived, the control word has 13 bits, reserved
  opcodes alias their neighbours, FSTENV's pointer is the ESC byte after a segment
  prefix. The test `bench_real` holds the real reports.
- The instruction-level 8087 was corrected wherever the microcode showed it wrong (FSQRT
  exactly rounded, FPREM's loop and quotient bits, FBSTP rounding, NaN and denormal
  loads, FXTRACT and FSCALE special cases; list in docs/8087-microcode.md).

## 0.10.0

Each model now has the hardware its real boards carry (from photos of 1101,
1121, 1137 and 1139 motherboards):
- New model 1137 (`--model 1137`, Kurt's machine): 113x board with 512K and the
  large screen, the 1139 PROM (as on the real 1137) and the 300001-02 keyboard
  firmware dumped from it. No modem: the 8255 at DFEC0 is missing, so
  Modem~Device~ finds no modem as on the real machine. Its sound DACs and DTMF
  chip are on the motherboard; how the DTMF chip is driven is not known, so
  for now a write-only latch at DFEC2 takes the 8255's port B bits (to check
  with SoundTest on the real 1137).
- Timers per board (new `timers.rs`). 110x and 112x, 80130: timer 0 counts at
  2.5 MHz (CPU clock / 2), timer 1 counts timer 0's output pulses, timer 2 at
  5 MHz, no read-back, all running from power-on. 113x, discrete 8254: all three
  at 5 MHz, read-back command with status bytes, a counter stands at FFFF until
  a count is written, timer 1 counts only while timer 0 has a count. Replaying
  GRiDProbe's T test gives the real machines' patterns: about twice the count
  per read on the 113x, status B4 (mode 2) and B6 (mode 3), FFFF before
  loading, read-back ignored on the 1101. Before, every model had all timers at
  5 MHz and timer 1 frozen.
- The 113x board ignores A0 at the 8254: a byte read at 09/0B/0D also steps
  that counter's LSB/MSB flip-flop. With it, GRiDBench 2.2's port rows (u20-u27)
  and timer rows (p30-p52) match the real 1137 (low byte of timers 1 and 2 on
  every row, FFFF on unloaded counters); 0.9.1 gave counting timers there.
- Interrupt controller per board: the 80130's quirks (non-specific EOI
  ignored, ports 4/6 not decoded, 0Eh reads FFh) now apply to the 110x and
  112x; the 113x's 8259A + 8254 take the non-specific EOI (assumed), mirror the
  PIC at 4/6 and leave 0Eh open, as measured on the real 1137. The 112x used
  the 113x behaviour before.
- Keyboard firmware per model: 300001-02 on the 1137, 300067-01 on the others
  (each stands in for the other when missing). The 1139 used 300001-02 before.
- The 8274 already counted from the 5 MHz CPU clock; the photos confirm it (no
  4.032 MHz crystal on any motherboard).
- The 113x RAM/display timing, refresh and INT fits (from the real 1137) now
  hang on `GridModel::board_113x()`, and stay as they were. GRiDBench 2.2 gives
  the same timing results as 0.9.1 on the 1101 and 1137; only the timer and
  port rows changed.
- gridfrag: FRAG_DUMP=address,length prints memory after the run.

## 0.9.1

Fitted to GRiDProbe 0.0.12-0.0.13 and the Disk Toolkit 0.4.1 box probe on a
real 1101 and 1137:
- GRiD-OS's INT 3 stand-in for each 8087 WAIT costs what it costs on the real
  machines: 55.8 clocks over a WAIT on the 1101 (real 55.7) and 57.8 on the
  113x (real 58.7). A WAIT alone takes 3.7 clocks with the 8087 idle (real
  3.75). FNOP, FLD/FSTP, FDIV and short and long FMUL are within about a clock
  of the real chip; a long FMUL that rounds up takes 5 clocks more, an FADD
  whose smaller operand shifts out completely 5 less, as on the real chip.
- Frame geometry from GRiDProbe's beam finder: the 1101 draws 307 clocks a
  line and 268 lines (60.8 Hz; real 60.8), with IR3 rising on line 3 for 241
  lines; the 113x 316 x 263 (60.2 Hz). Display contention refitted to these
  frames (GRiDBench 2.2 gap rows: 1.2% rms on the 1101, 1.3% on the 113x).
- 2101 hard disk: interleave 5, read from the real drive's sector IDs. Seek
  times measured inside a real box (Seek, then Test Ready until ready). A Seek
  command is buffered: it answers after 1.5 ms plus 20 us a cylinder, and Test
  Ready reports "not ready" until the heads are in. A read's or write's own seek
  waits that long again plus 8 ms before its first sector, which matches the
  Disk Toolkit per-read seek costs and GRiDBench 2.1b (random pages 132.8 ms,
  real 130.3; steps of 1/16 118.4, real 122.5). Recalibrate steps back at 3 ms
  a cylinder. Commands take 0.44 ms to decode, as on the real box.
- Bubble: writes end with status 42 (the parity bit, data fine), as on the
  real 1101; GRiD-OS ignores the bit. Read FSA Status gives FF 20 20 20 20 20,
  with 28 for an MBM written since the last Software Reset. Abort takes 0.14 to
  0.16 ms. An Abort between two pages of a read leaves the controller stuck as
  on the real chip: later multi-page reads stop after one page with status 2C
  until a Software Reset (GRiD-OS's error recovery does one). GRiDProbe B7 now
  matches the real report line for line.
- 2102 floppy: no change needed. The low-level 2102 already matches GRiDProbe's
  floppy test (same page again 181 ms, real 185; power-up after 12 s idle 1.9 s,
  real 1.9).

## 0.9.0

- GRiDLink: the 8274 serial chip's synchronous SDLC mode, the one the GRiD
  Server network uses. --gridlink tcp:2400 (or connect:HOST:PORT) carries
  the Compass's frames over TCP, one message per frame (format in
  docs/gridlink.md), for a GRiDLink server program on the PC. No GRiD
  Server is built in: a Compass can only be a GRiDLink client, so two
  emulators can't link to each other this way.
- Tested with the real GRiDLink client code: CableTest from the GRiD
  Server disks ("Programs for GRiD Compass") signs on, takes its station
  address and passes its 520-byte test exchanges with no errors, against
  a small test script playing the server.
- Multidrop cable (default) or point-to-point (--gridlink-p2p, RTS looped
  to CTS), as GRiDLink~Device~ checks at start. The serial port select
  latches (DFE10-DFE16 on the Compass II board, DFE00-DFE06 on the 1101)
  are kept and shown to the link.
- Fix: the 8274's interrupt vector in 8086 mode (WR2A = 10h) now carries
  the cause in bits 0-2, as on the chip; it was in bits 2-4.

## 0.8.1

Fitted again to the real 1101 and 1137 runs (GRiDBench 2.2, GRiDProbe 0.0.11):
- 8087 FPATAN and F2XM1 take as long as the real chip for each input: their
  time follows the chip's digit loop (steps from the first non-zero digit, and
  the digits themselves), within about 6 clocks on 36 and 34 inputs (FPATAN
  was a flat 712 clocks for most inputs, the real chip 380 to 843). FYL2X near
  1 is about 50 clocks longer, a long FMUL whose significands multiply to 2 or
  more about 11 shorter, and a store that has to denormalize its result (FST
  of a number too small for a double) 49 longer, as on the real chip.
- 8087 loads like the real chip: FLD of a denormal double or single gives an
  unnormal (GRiDBench h33 reads back 3C010093445B87315800 as on the real 1101,
  not the normalized 3BF9...), and FLD m80 of an unnormal keeps it (it used to
  turn into a NaN). FSTP m80, FSAVE and FXAM see the chip's bit pattern.
- The 113x models (1131, 1139) use the real 1137's memory timing: a RAM read or
  write takes about one clock more than on the 1101, the display slots are
  shorter and the refresh steal is 2 clocks. Gap rows against the real 1137:
  1.2 % rms (was 2.0 %).
- Bubble: a page's 64 bytes come into the FIFO over about 5 ms (400 clocks a
  byte), so a read gives two FIFO interrupts a page (35 + 29 bytes) as on the
  real 1101. Read FSA Status, Reset FIFO, Software Reset, Abort and MBM Purge
  are shorter; Initialize takes 6 to 9 ms (the real 6.0 to 8.6), but stays
  short (0.5 ms) after an MBM Purge until the next transfer. On a write the
  FIFO takes data only from 1.2 ms before the first page passes, as the real
  7220 (no bytes before the first interrupt).
- Real-time clock: for a moment after each tenth (one 32.768 kHz period, about
  30 us; the real length isn't measured, RTC_UPDATE_CLK sets it in CPU clocks)
  the MM58174 is updating and its digits read 0Fh, as on the chip, so the
  PROM's retry on 0Fh runs as on hardware.
- ROMs: the 1109 boots the 1101's PROM, the 1121 the 1129's and the 1131 the
  1139's, as on the real machines (they share PROMs), without a stand-in warning.
- Results, GRiDBench 2.2 against the real machines: 1101 CPU/bus rows 1.07 %
  rms, 8087 rows 2.5 % (was 5.7 %); 1139 against the real 1137 CPU/bus 1.49 %
  (was 1.96 %). GRiDBench's bubble rows (GRiDBench 2.1b on a real 1101), ms,
  emulated / real: same page 100.2 / 99.9, sequential 126.0 / 127.7, random
  112.6 / 123.6, first/last 98.1 / 100.9.

## 0.8.0

- The 2101 hard disk box runs low-level when roms holds its firmware
  (300205-05.bin for the 20 MB box, 300205-06.bin for the 10 MB one; 2101.bin
  also works): the box's 8088 (4 MHz) runs the real ROM, with an 8291A GPIB
  chip, the 74LS259 latch, a WD2797 floppy controller and a Xebec S1410 SASI
  controller (the S1410 at command level, timed after its ROM and a real 2101
  with an ST-225). The ROM is picked from the hard disk image size.
- The 2101's 4-character display shows in the status bar and in the debugger.
- The 2101 floppy goes through the box's firmware too: a format writes
  interleave 3, and an empty slot gives error 128 as on the real box.
- Disk timing fitted to GRiDBench 2.1b on a real 1101 + 2101 (ms, emulated vs
  real): same page 17.0 / 19.0, sequential 23.8 / 21.1, random 55.8 / 53.5,
  first/last 133.7 / 130.3, steps 117.8 / 122.5. The drive is taken as formatted
  at interleave 4 (S1410_IL changes it; S1410_SEEK the seek curve).
- GRID_2101=protocol uses the protocol-level model; it is also used when no
  2101 ROM is there, or when a PC folder is the 2101 floppy.

## 0.7.0

- The 2102 portable floppy runs low-level when roms holds its firmware
  (300237-02.bin or 300237-01.bin): the box's 8051 runs the real ROM, with a
  WD2797 controller and a 5.25" drive, wired from the 2102 schematic. Reads,
  writes, get status, set status and formatting (whole disk and one track) all go
  through GRiD's own firmware, at its speed: the first access after 10 s idle
  powers the drive up again (about 1.8 s), as the ROM does.
- The box is switched on 4 s before the Compass, so it has finished its self test
  when the boot PROM looks for it. GRID_2102_EARLY=0 switches both on together.
- GRID_2102=protocol uses the old protocol-level model instead.

## 0.6.2

Fitted to the GRiDProbe runs on a real 1101 and a real 1137:
- Bubble memory turns like the real one: the loops only move while a command
  runs and stop where it ends, so a read waits for its page to come round.
  The next page costs 12.5 ms, two on 20 ms, half the bubble away 46 ms, the
  same page again a full turn (87 ms); streaming 7.2 ms a page, bytes reach
  the FIFO one at a time (2 bubble interrupts a page, as on the real 1101).
  Initialize, Read FSA Status, Reset FIFO, Software Reset, Abort and Purge
  take their real times; Abort during a transfer drops the FIFO.
- The interrupt controller ignores a non-specific EOI (OUT 0,20H), as the
  real 1101 does; GRiD-OS uses specific EOIs.
- Refresh tick (IR3) stays high most of the frame like the real machines
  (1101: low ~8000 clocks a frame; 113x: low only ~1500). 1101 frame 270
  clocks x 306 lines (60.5 Hz).
- 1131/1139: DFE00 = 1 darkens the screen until it is set back to 0, as on
  the real 1137.
- Display memory slowdown on every line, blank ones too (the real machines
  show the same slowdown all through the frame), refitted: the full GRiDBench
  2.2 report now matches the real 1101 within 1.04% on average (was 1.14%)
  and the real 1137 within 1.97% (was 2.83%).

## 0.6.1

Timing fitted to GRiDBench 2.2 runs on a real 1101 and a real 1137:
- 1101: the branch, prefetch and REP rows of the gap test now match within
  1.2% on average (4% at most; JMP SHORT chains were 14% too fast). A chain
  of JMP SHORTs runs like a chain of taken conditional jumps, as on the real
  machines. The frame is 271 clocks x 306 lines (60.3 Hz).
- 1131/1139: frame 281 clocks x 297 lines (59.9 Hz), refresh tick at the top
  of the picture like the 1101, display memory slowdown fitted to the 1137
  (gap rows within 3.3% on average).
- Compass II split board: empty memory reads the address on the bus (not FF),
  the boot PROM also shows at F8000, ports 4/6 answer like the interrupt
  controller at 0/2, port 0E is not decoded, and the bubble controller repeats
  every 4 bytes up to DFE8F, as on the real 1137.
- GRID_TRACE=file writes a clock-by-clock CPU trace (for timing work).

## 0.6.0

- Program symbols for the debugger from a debug build (debug.bat,
  gridbuild.py debug): names and Pascal source lines in the disassembly,
  breakpoints by name or source line, call stack, names in the profiler.
- Crash catcher: stops on GRiD-OS errors (with the message text), divide
  errors, INT 3 and wild jumps, with a report.
- GRiD-OS call log (every OS, CP and Common call with its caller).
- --symbols DIR and --no-catch; the machine setup page has a Debugging card.

## 0.5.0

- A GUI: menus, toolbar, status bar with drive lights, machine setup page
  with saved profiles, media manager with floppy swapping and a GRiD file
  browser (export, import, delete, subjects, blank floppies), debugger
  workspace, chips window, quick save / load, state files, screen colour
  and scaling. The 0.4 window stays as --classic and as the fallback when
  OpenGL 2 is missing.
- --fast starts with the fast CPU core.

## 0.4.0

- Demo tools (see above): cycle counter marks, frame meter overlay (F10),
  debugger console with breakpoints and stepping (F8), profiler.
- The Claude Code hook has grid_debug with the same debugger, profiler and
  cycle counter.

## 0.3.12

- Application ROM sockets on the Compass II models (see Application ROMs).
  Checked with the four ROMs of a real 1137: GRiD-OS boots from the ROMs
  with no disk, and their programs copy out.
- A disk transfer that hangs now ends the way it does on the real machine:
  the keyboard chip's watchdog frees the CPU after about 4 seconds and
  GRiD-OS reports error 451 (before, the emulator gave up on its own).

## 0.3.11

- Two emulators can be linked by a virtual null-modem cable (see above):
  --serial connect:PORT, and the new "Start Compass-Emu 1101 link.bat".
- Fixed: the .bat files of 0.3.9 and 0.3.10 didn't attach the PC folder
  (the option landed on a line of its own).

## 0.3.10

- Floppies run at real speed: 300 rpm, interleave, seeks and the 2102's slow
  controller, fitted to a GRiDBench run on a real 2102 (with a Gotek): page
  reads within about 10% except "steps of 1/16" (23% slow).
- With a hard disk, the 2101's floppy slot is there too (empty, error 128
  like a real 2101), so programs that look at the Floppy Disk no longer hang.
- The 2102 reads one page per command, as its ROM does.

## 0.3.9

- A folder on the PC as a GRiD drive (see PC folder above): copy sources,
  programs and data in and out with Explorer, no image tools needed.
- A GRiD "set status" write no longer overwrites page 0 of a disk.

## 0.3.8

Timing checked against a GRiDBench 2.1 run on a real 1101 with a 2101:
- Plain 8086 code and memory access within about 2% of the real machine on
  average (memory refresh and screen fetches now steal bus time as measured).
- Fixed: a disk read could copy a byte or two too many when the transfer
  ended, so GRiD-OS reset the disk bus (half a second). Reading 8 pages at
  once now takes 65 ms instead of 1.1 s.
- Hard disk seeks follow the real ST-225: random page reads 130 ms (real
  130.3), steps of 1/16 of the disk 121 ms (122.5), the same page 19 ms (19.0).

## 0.3.7

- Disk reads and writes are much faster and closer to a real 2101: the
  interrupt controller now raises an interrupt as soon as its line is
  unmasked. Before, every disk command waited for the next screen refresh,
  so each page took 3 refreshes (50 ms). Boot and file work speed up too.
- The 2101 hard disk now behaves like the real ST-225 box at real speed:
  rotation, interleave 5, seeks, the box's 6-page read-ahead cache and
  writes that finish in the background. Sequential page reads take about
  23 ms, as the Disk Toolkit measured on the real box. Build mode (fast
  core) skips this.
- FYL2X and FYL2XP1 take as long as on the real 8087 for each operand.

## 0.3.6

Timing and details measured by GRiDBench 2.0 on a real 1101:
- 8087 math runs at real speed. GRiD-OS turns every 8087 WAIT into an INT 3 call;
  the old timing counted that cost twice, so math was 1.5 to 2 times too slow.
- FPTAN gives the real chip's numbers (within 3 bits in the last place, was 8),
  and its time depends on the angle as on the chip. FPATAN, F2XM1, FYL2X and FSQRT
  now set the precision flag and treat out-of-range inputs as the real 8087 does.
- Display: 274 clocks a line, 306 lines (59.6 Hz), the refresh tick at the top of
  the picture.
- Empty memory and unused ports read what the real bus gives (not FF), the boot
  PROM also shows at F0000, ports 00-7F mirror the interrupt controller, the clock
  chip is slower to read, 80130 timers 0 and 2 count and timer 1 doesn't.
- The 1101 uses the later rev 4 boot PROM, the one in the real 1101 it was measured on.

## 0.3.5

Sound no longer stops after a few notes in games using GRiDSound (BomberGRiD,
GRiDroids): the serial chip's interrupt now drops between samples as on the
real chip, so the sample clock keeps running.
Keyboard (Windows): keys are read straight from Windows' key state only, so
no key stays stuck down any more (Shift stuck on, or the keyboard chip
stopping because it thought 3 keys were held).
Alt+Esc and Alt+Space no longer make Windows act while the emulator has focus;
they go to the Compass as CODE-Esc / CODE-Space. Alt+Tab and Alt+F4 still work.

## 0.3.4

8087 FPTAN works like the real chip: the CORDIC + rational method found in its
microcode, giving the chip's own Y/X pair (X is about 3, not 1) and a run time
that depends on the angle. Last bits still to be checked on real hardware.

## 0.3.3

Fast CPU core for builds: hello.pas 3.3 s a build (was 15 s), GRiDTiles 48 s
(was 6 minutes). The programs it builds are byte for byte the same as on the
cycle-exact core. The window version and timing tests still use the exact core.
Claude hook: grid_boot takes fast: true for quick file work.

## 0.3.2

Faster builds: the first build boots the GRiD once and saves it at the prompt
(compass-emu\states); later builds start from there. hello.pas: 15 s a build.
The saved state is made again by itself when your merged disk or compass.exe changes.

## 0.3.1

The real 1129 boot PROM is used for the 1129 and 1121 (builds ~27 s instead of 38 s).

## 0.3.0

Build mode (compass.exe do, and gridbuild.py using it) and the Claude Code hook
(compass.exe mcp).

## 0.2.0

More models: 1109, 1121, 1129, 1131, 1139 (--model). The Compass II split board
memory map, 512K RAM, and the 1131/1139 512 x 256 screen (turned on by the 113x
build of GRiD-OS; before that the 320 x 240 picture is shown centred). Large
screen timing is estimated from GRiDBench on a real 1137.

## 0.1.0

Bubble memory, serial port and sound (sound DAC, volume DAC, DTMF tones).
Timings are first estimates until measured on real hardware.

## 0.0.5

Display timing: the CPU slows down when it shares RAM with the display, and the
screen shows what the beam saw, so racing the beam and tearing look as on a real
1101. Settings are first estimates until measured on real hardware.

## 0.0.4

8087 math coprocessor: real 80-bit math, timing matched to a real 1101
(GRiDBench REAL tests within about 2%).

## Not there yet

The modem itself, GRiDLink (synchronous serial), the test ROM socket.

Source: https://github.com/ConventionalMemories/compass-emu
By Conventional Memories (C.M._), MIT license. Built on MartyPC by Daniel Balsom (MIT).
