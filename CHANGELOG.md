# Changelog

Compass-Emu versions (newest first). MartyPC's own changelog is in [MARTYPC-CHANGELOG.md](MARTYPC-CHANGELOG.md).

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
