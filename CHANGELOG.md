# Changelog

Compass-Emu versions (newest first). MartyPC's own changelog is in [MARTYPC-CHANGELOG.md](MARTYPC-CHANGELOG.md).

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
