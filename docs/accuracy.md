# Accuracy

Compass-Emu's timing and behaviour, compared with my real Compass **1101** (with a 2101 hard
disk and a 2102 floppy with a Gotek) and **1137**. The test programs were written for this:

- **GRiDBench** 2.x: 8086 code, memory, jumps and REP strings in clocks; disk and floppy page
  reads; 8087 results and times.
- **GRiDProbe**: display geometry (a "beam finder"), contention map, interrupt and timer
  behaviour, bubble memory, WAIT overhead.
- **8087 Bench**: 2181 tests on the 8087 in two programs (results and busy time), plus a
  third program of 1584 tests for the constant ROM that has not been run on a real machine
  yet.
- **Disk Toolkit**: seek and command timing measured inside a real 2101 box.

How each item was measured, and what is still open, is in
[real-hw-tests.md](real-hw-tests.md). Figures are for the current version (0.13.0) unless a
version is given. The real 8087 Bench reports are in `crates/lib/i8087/testdata/`
(`bench-real.txt`); the tests `bench_real` and the device tests replay them.

## Summary

| Area | Measured on | Result |
|---|---|---|
| 8086 instruction set (SingleStepTests 8086 v1, 630,000 tests) | | No register, flag or memory errors on either core. About 94.5 % of tests have exact clock counts on the cycle-exact core. |
| 8086 timing on the Compass (GRiDBench 2.2 jump, memory and REP rows) | 1101, 1137 | 1.2 % rms on the 1101, 1.4 % on the 113x |
| Whole GRiDBench 2.2 report | 1101 | about 1 % rms (0.6.2) |
| 8087 busy time, microcode core (8087 Bench, 2068 timed tests) | 1101 | 1.45 % mean error (about 1.5 clocks) |
| 8087 busy time, instruction-level core | 1101 | data-sheet times, a few adjusted: about 40 % mean error on 8087 Bench, but GRiDBench's typical cases within about 5 % |
| 8087 results, microcode core (8087 Bench, 1588 result lines) | 1101, 1137 | all but 34 lines identical (33 tests, last-bit differences in transcendentals, mostly far out of range) |
| 8087 transcendentals as 80-bit values | 1101 | 141 results: 134 bit for bit, 7 one unit off |
| Display frame | 1101, 1137 | as measured: 1101 307 x 268, 113x 316 x 263 |
| 2101 page reads | 1101 + 2101 | within a few % except sequential and multi-page on the low-level model (below) |
| 2102 page reads | 1101 + 2102 | within about 10 % for most patterns (below) |
| Bubble memory | 1101 | seek patterns, FIFO interrupts and status bytes as measured (below) |

## CPU and bus

| Item | Real | Emulator |
|---|---|---|
| GRiD-OS's WAIT-to-INT 3 per 8087 instruction, over a plain WAIT | 1101 55.7 clocks, 1137 58.7 | 55.8, 57.8 |
| INT/IRET round trip over MartyPC's 8088 model | 1101 +1 clock, 113x +2 | same (fitted) |
| JMP SHORT chains | as fast as taken Jcc | one clock added before prefetch is suspended (fitted) |
| Data access on the 113x against the 1101 | about 1 clock slower | same (3-state display slots, 4-state CPU accesses) |
| Timer rate, 113x against 1101 (GRiDProbe T test) | 2.0 | 2.05 |
| 8254 read-back status on the 1137 | B4 / B6 | B4 / B6 |
| REP STOSW per word (1101) | 11.15 clocks | 12.2 (open) |
| REP LODSB (113x) | | about 10 % slow (open) |

The DRAM refresh steal is locked to the display frame (the display's own fetches refresh the
DRAM), so a polling loop's turns per frame no longer depend on when it started. This took
8087 Bench 1 on the 1101 from 2.6 % to 1.9 % mean error before the microcode timing fit.

## Display

GRiDProbe 0.0.13's beam finder on the real machines, with its own row loop counted:

| | 1101 | 113x (1137) |
|---|---|---|
| Clocks a line | 307 | 316 |
| Lines a frame | 268 | 263 |
| Frame rate at 5 MHz | 60.8 Hz | 60.2 Hz |
| IR3 | rises on line 3 of the picture, high 241 lines | high 259 lines |
| Display RAM slots | one every 32 states (6 states), every line | one every 16 states (3 states), every line |

The contention map is flat across the frame on both machines (blank lines too), and the
emulator draws each screen word as the beam fetched it, so racing the beam and tearing look
as on the real screen.

## 8087

8087 Bench on the real 1101 and 1137 (2181 tests in two programs), against the microcode
core:

- **Results**: of 1588 result lines, all but 34 come out the same (`bench-known.txt` lists
  the 33 tests). About twenty are FYL2X, FYL2XP1 and F2XM1 results where the value before
  rounding is a fraction of a unit off; the rest are arguments far outside the documented
  range. They hang on the constant ROM's last bits, which 8087 Bench 3 is made to settle.
- **Busy time**: the time comes from the micro-instructions that ran, with costs fitted to
  the real 1101's clocks. Mean error 1.45 % (about 1.5 clocks) over 2068 timed tests. The
  1137 came out at about 2.3 % in the last check before the final fit. The control
  instructions, FSAVE and FRSTOR are within about 1.5 clocks. The largest errors left are
  FCOMPP, far out-of-range transcendental arguments and FBLD/FBSTP of some values.
- **WAIT with BUSY held**: a WAIT hangs like the real machine's while an unmasked exception
  is pending; 8087 Bench 0.0.3's risky part stops at the same test (y005) as on the real 1101.
- **Power-on registers**: as on the real 1101 (FXAM of an empty register answers 4700).

Results dumped from the real 1101 with GRiDBench 2.2 (the 8087 on many inputs):

| Instruction | Real results | Bit for bit | One unit off |
|---|---:|---:|---:|
| FPTAN | 19 | 18 | 1 |
| FPATAN | 36 | 34 | 2 |
| F2XM1 | 34 | 31 | 3 |
| FYL2X | 15 | 15 | 0 |
| FYL2XP1 | 25 | 24 | 1 |
| FSQRT | 12 | 12 | 0 |

More in [8087-microcode.md](8087-microcode.md).

## 2101 Disc System

Page reads in ms, GRiDBench 2.1b on the real 1101 + 2101:

| Pattern | Real | Low level | Protocol model |
|---|---:|---:|---:|
| Same page | 19.0 | 21.1 | 19.4 |
| Sequential | 21.1 | 26.8 | 19.4 |
| 8 pages | 53.5 | 63.8 | 65 |
| Random | 130.3 | 132.8 | 130.0 |
| Steps of 1/16 | 122.5 | 118.4 | 120.6 |

Inside the box (Disk Toolkit 0.4.1 Box probe on a real 2101): the heads arrive 9.5 ms after
a 1-cylinder seek, 21.5 ms after 38 cylinders and 70.4 ms after 611; Seek is buffered (it
answers in a few ms and Test Ready says not ready until the heads are in); commands decode
in 0.44 ms. The low-level model uses these. An empty floppy slot answers error 128 as on the
real box.

## 2102 Portable Floppy

Page reads in ms, GRiDBench 2.1b on the real 2102 (with a Gotek), motor running:

| Pattern | Real | Low level | Protocol model |
|---|---:|---:|---:|
| Same page | 212 | 200 | 197 |
| Sequential | 133 | 121 | 121 |
| Random | 235 | 243 | |
| First/last | 415 | 403 | 395 |
| Steps of 1/16 | 172 | 196 | 211 |

GRiDProbe 0.0.13's floppy test: the same page again 181 ms (real 185); after 12-30 s idle,
when the box has powered its drive down, 1.8-1.9 s (real 1.9-2.0). The low-level 2102 also
reproduces a real 2-second "glitch" GRiDBench saw on the first read after idle.

## Bubble memory

GRiDProbe on the real 1101, replayed in `crates/lib/grid_core/tests/devices.rs`:

| Item | Emulator (fitted to the real board) |
|---|---|
| Loops | turn only during a command: 2048 slots of 199 clocks, 81.5 ms a turn |
| Single-page read | 5.3 ms start + the wait for the page's slot + 3.3 ms |
| Consecutive pages | 7.2 ms apart (next page 12.5 ms, same page 87 ms, half a turn away 46 ms) |
| FIFO interrupts | two a page (35 + 29 bytes) |
| Initialize | about 6 ms |
| Short commands | 0.08-0.43 ms |
| Abort | 0.14-0.16 ms; a later multi-page read stops after one page with status 2C until a Software Reset |
| Status after a write | 42 (parity bit; GRiD-OS ignores it) |
| Booting a real 1101's bubble | boot screen 2.85-3.7 s (real 3.73 s) |

The 1137's bubble looked slower than the 1101's; all models use the 1101's timing for now.

Read FSA Status gives two bytes per module, 20H each (28H once the module was written).
GRiD's drivers that size the bubble, the Compass II ROM's (GRiD's 1984 sized
bigbubbles.plm) and MS-DOS 2.11C's, count the bytes that are not FFH and halve the count. So
every channel answers here, although GRiDProbe read FFH for the first channel on the real
1101 (b01/b82). With that FFH, MS-DOS would format only 256K of the 1101's 384K.
`Bubble::fsa_first_ff` brings it back. A command also points the register address counter
back at the FIFO, or those drivers would count five parametric registers as well. An
Initialize whose address register names a module that isn't fitted stays busy until an Abort.
To be checked on the real 1101: MS-DOS 2.11C's FORMAT E: should report 768 sectors.

## Not compared yet

The serial port and GRiDLink, the internal modem, sound levels, the keyboard scan rate and
the absolute CPU clock have not been checked against the real machines; see the open rows in
[real-hw-tests.md](real-hw-tests.md).
