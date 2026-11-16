# Compass hardware model

How Compass-Emu models each part of the GRiD Compass, what it is based on and what was
measured. The code is in `crates/lib/grid_core` (the machine and its devices),
`crates/lib/i8087` (the 8087) and `crates/lib/marty_core` (MartyPC's 8086). The disk drives
have their own page, [drives.md](drives.md), and so do the [modem](modem.md),
[GRiDLink](gridlink.md) and the [8087 microcode](8087-microcode.md). How each number was
measured is in [real-hw-tests.md](real-hw-tests.md), the results in
[accuracy.md](accuracy.md).

## Models

`GridModel` in `grid_core/src/machine.rs`; `--model` in the window, `GRID_MODEL` headless.

| Model | Board | RAM | Screen | Timers + PIC | Timing fit | Sound | Boot PROM used | Keyboard 8741 |
|-------|-------|-----|--------|--------------|------------|-------|----------------|---------------|
| 1101 | original | 256K | 320 x 240 | 80130 | real 1101 | modem | 1101 rev 4 (rev 3 if rev 4 is missing) | 300067-01 |
| 1109 | original | 512K | 320 x 240 | 80130 | real 1101 | modem | 1101 (same PROM) | 300067-01 |
| 1121 | split | 256K | 320 x 240 | 80130 | real 1101 | modem | 1129 (same PROM; if missing, 1101) | 300067-01 |
| 1129 | split | 512K | 320 x 240 | 80130 | real 1101 | modem | 1129 (if missing, 1101) | 300067-01 |
| 1131 | split | 256K | 512 x 256 | 8254 + 8259A | real 1137 | modem | 1139 (same PROM) | 300067-01 |
| 1137 | split | 512K | 512 x 256 | 8254 + 8259A | real 1137 | motherboard, no modem | 1139 (same PROM, as on the measured 1137) | 300001-02 |
| 1139 | split | 512K | 512 x 256 | 8254 + 8259A | real 1137 | modem | 1139 | 300067-01 |

The 112x carry the 1101's 80130 and the 300063-00 gate array of the later 1101 boards, so
they take the 1101's timers, interrupts and RAM/display timing, with the Compass II ROM
sockets, PROM and plug-in bubble board. The 113x share the 1137's 300415-00 gate array
and 8254, so they take the timing fitted to a real 1137. Each keyboard firmware
stands in for the other when missing (the early 1101's 300027-01 is not dumped).

The 1101 board: 16K boot PROM, 256K RAM (512K on the 1109),
80130 interrupt controller and timers, IR3 at vertical blank, 320 x 240 screen at 0400h.

Split board (Compass II): GPIB pseudo-DMA window at DFA00h, video/memory control at
DFE00h (DFE00 = 1 selects the video chip, DFE04 bit 2 clear = large screen; the 113x OS
writes 0Bh or 0Ah, the PROM 0Eh; dark screen while DFE00 = 1), serial port latches at DFE10h, application ROM
sockets at 80000h (`--rom0..3`, from Cp/Rom.Asm and Cp/BankSwitch.PLM): DFEA8h = 1
enables the window, DFE08/0A/0C/0E pick the socket (bits 0-1; bits 2-3 = 3 - wait
states) for each 32K quarter, DFEA2h = 32K-part mode (stored, not needed for the
mapping). A 128K chip shows its quarter q in window quarter q, a 64K chip fills the top
half (lower half FFh, as Rom Toolbox found on a real 1137), a 32K part shows in every
quarter. With the four ROMs of a real 1137 the PROM boots CCOS from socket 1 (with or
without a disk) and the programs show on "Read Only Memory". The 113x build of GRiD-OS turns on the 512 x 256 screen; until then a
113x shows the 320 x 240 picture centred. Large-screen timing is from GRiDProbe and
GRiDBench 2.2 on a real 1137 (see Display below). The split board's bus also follows the 1137: empty memory reads the address, the
PROM shows at F8000-FBFFF, the 7220 repeats up to DFE8F; on the 113x (8259A + 8254)
ports 4/6 mirror the PIC and 0E is open, on the 112x (80130) they behave as on the 1101.
All seven boot to the Executive (1131/1137/1139 with the 113x build, the others with the 11xx
build). The 1139 PROM can't stand in for the 1121/1129 PROM: it keeps its data
at 0440:0, where the 11xx build of CCOS loads (02D8:0 up), and stops with "Checksum
error in CCOS". The 1129 PROM (data at 0298:0, like the 1101's) boots the 11xx build on
the plain split-board map, and is also the 1121's. Without it the 1101 PROM
stands in, with the split board also answering the 1101's DMA window at E0000h
(`GridBoard::e0000_dma`). ROM choice: grid_core/src/roms.rs.

## CPU

- 8086 with prefetch queue and bus timing (MartyPC, `CpuSubType::Intel8086`), with one
  timing fix for ALU r/m,imm writes. A fitted Compass quirk adds one clock before a JMP SHORT
  suspends prefetch (`cpu_808x::JMP_SHORT_EXTRA`, set by grid_core, `JMP_EXTRA` overrides). SingleStepTests 8086 suite: 0 register/flag/memory
  errors, about 94.5% of tests with exact clock counts.
- A fast whole-instruction 8086 core for build mode (`grid_core::fast86`), switchable at any
  instruction boundary; same SingleStepTests results for registers, flags and memory.

- 8259 interrupt controller: a request that is pending when its IR is unmasked (or a lower
  one after an EOI) raises INT at once. Before 0.3.7 it waited for the next unrelated
  interrupt, so each disk command waited for an IR3 tick (3 refreshes a page).
- Interrupts are sampled inside REP string instructions (`ExternalBoard::intr_now`) and
  INTR is updated before the next instruction starts, so a pseudo-DMA `REP MOVSB` stops
  right after the byte that raised the GPIB END interrupt (it used to copy 1-2 more bytes,
  which made the BB2 driver reinitialize the bus for half a second).

## 8087

There are two 8087 cores. With the chip's microcode ROM file supplied, the emulator runs the
**microcode core**: [8087-microcode.md](8087-microcode.md) describes it, its timing and what
a real 8087 answered. Without the file it runs the **instruction-level core** described
here (the microcode core uses its control instructions and is checked against it).

- 8087 (`crates/lib/i8087`) on the 8086's ESC/WAIT hooks: 80-bit registers with bit-exact
  arithmetic in all rounding modes, all instructions, exceptions to IR6. Transcendentals
  are computed in 113-bit precision and rounded once, except FPTAN, which follows the
  chip's own CORDIC + rational method (`fptan.rs`), fitted to FPTAN dumps from a real 1101:
  its partial Y/X pair (X near 1.5, an unnormal X for out-of-range 1.5) within 3 ulps and its
  argument-dependent time. Also as on the real chip: FPATAN with 0 < x < y returns y,
  F2XM1 of a negative x gives -(2^|x| - 1), FYL2X of a power of two is exact and fast, and
  the precision flag follows the real one (FSQRT, FYL2X, F2XM1 0). Busy time per instruction is the data-sheet
  time (a few adjusted to a real 1101); FPATAN and F2XM1 time follows the chip's digit loop
  (16 steps from the first non-zero digit, plus each digit), fitted to GRiDBench 2.2 within
  ~6 clocks. Like the chip, FLD of a denormal single or double gives an unnormal (exponent
  3F81h / 3C01h, integer bit 0), and FLD m80 keeps an unnormal; FSTP m80, FSAVE and FXAM see
  that pattern, arithmetic uses its value (the chip's unnormal arithmetic is not modelled). 8087 code is slow on a real Compass because GRiD-OS's
  loader turns the WAIT before each 8087 instruction into INT 3 (handler: WAIT, IRET):
  55.8 clocks over a plain WAIT on the 1101 and 57.8 on the 113x (GRiDProbe on a real 1101
  and 1137: 55.7 and 58.7; INT/IRET take 1 (113x: 2) clocks more than MartyPC's 8088
  model, a WAIT 2 more, `INT_EXTRA` / `WAIT_EXTRA`). GRiDProbe's 8087 rows (FNOP, FLD/FSTP,
  FDIV, short and long FMUL, a long FMUL that rounds up +5) are within about a clock of
  the real chip; FADD is ~5 clocks fast. Raw fragments with
  real WAIT bytes (gridfrag) are faster than GRiD programs for the same reason.

## Display

- Display timing (`DisplayTiming` in grid_core/src/screen.rs): 307 clocks a line, 268
  lines (60.8 Hz), IR3 rising on line 3 of the picture and high for 241 lines (113x: 316
  clocks x 263 lines, 60.2 Hz, IR3 high 259 lines) (GRiDProbe 0.0.13's beam finder on a
  real 1101 and 1137, with its own row loop counted, and d16/l54 for the IR3 level), and
  display RAM contention:
  the display takes one RAM slot (6 states) every 32 states (10.7 clocks, 20 a line; 113x:
  every 16 states, 32 a line, 3 states each, and a CPU access needs the RAM for 4 states, so
  data accesses are ~1 clock slower than on the 1101 as on a real 1137) on every line, blank
  ones too (GRiDProbe's contention map is flat across the frame), and a CPU access that
  starts inside a slot waits for its end; plus a 4-clock bus steal every 48 clocks (113x: 2).
  Fitted with the JMP SHORT quirk to GRiDBench 2.2's gap rows (c50-c61, j01-j30, REP per
  word: 1.2% rms on the 1101, 1.3% on the 113x) and the whole 1101 report.
  The window shows each screen word as the beam fetched it, so racing the beam (and
  tearing) looks as on the real screen.

## Interrupts, timers and the bus

- The 80130 ignores a non-specific EOI (OCW2 20h) as the real 1101 does (GRiDProbe B5);
  the 113x's discrete 8259A takes it (assumed, not tested on a real 1137).
- Bus details measured on a real 1101: empty memory and ports read the address still on
  the bus (low byte on even addresses, A8-A15 on odd ones), D0000-DEFFF reads FF on even
  bytes, the PROM also shows at F0000-FBFFF, ports 00-7F mirror the 80130 every 10h,
  RTC/ID reads have wait states.
- Timers (ports 08h-0Eh, `grid_core/src/timers.rs`), per board as the motherboard photos
  and GRiDProbe's T tests show. 110x/112x, 80130: timer 0 counts at 2.5 MHz (CPU clock / 2),
  timer 1 counts timer 0's output pulses (GRiD's 32-bit stopwatch chain), timer 2 at 5 MHz;
  they run from power-on, there is no read-back, 0Eh reads FFh. 113x, discrete 8254: all
  three at the 5 MHz CPU clock, read-back command (status B4/B6 as on the real 1137), a
  counter stands at FFFF until a count is written, 0Eh reads the bus, A0 is ignored (a read
  at 09/0B/0D steps the counter's byte flip-flop, as GRiDBench's port rows show on a 1137); timer 1 counts only
  while timer 0 has a count (how the board gates it is not known). Timer 2 is the 8274's
  baud clock on every model, and the 8274 runs from the 5 MHz CPU clock (no 4.032 MHz
  crystal on any motherboard; that one is on the modem, for its TMS99532).

## Keyboard and clock

- 8741 keyboard microcontroller running the real firmware (300067-01 or 300001-02). Its
  P2.4 (kbdDma) is wired to the GPIB wait: a DMA window access holds the CPU until the
  9914 is ready, its interrupt is up, or the firmware's watchdog drops kbdDma (about 4 s
  after GRiD-OS arms it), then the FDh it sends makes GRiD-OS end the transfer with
  error 451 and set kbdDma again. While kbdDma is low the window doesn't wait.
  `dma_wait_limit` (20 s) is only a backstop. `GPIB_STALL=N` holds the Nth byte a drive
  sends for 10 s, to watch this happen.
- MM58174 real-time clock (digits read 0Fh for ~30 us after each tenth while it updates).

## Serial port

- 8274 serial controller, asynchronous mode, IR0, bit rate from the 80130 baud timer;
  the window version connects it to a TCP port (`--serial tcp:2323`) or a real serial
  port (`--serial COM3`). Two emulators link like a null-modem cable: one listens
  (`--serial tcp:2323`, or `tcp:0.0.0.0:2323` for another PC), the other connects
  (`--serial connect:2323` or `connect:HOST:2323`, retried every second); DCD and CTS are
  up while connected. Received bytes are paced at the receiver's own bit rate.
  SDLC (GRiDLink, 0.9.0): channel A sends and takes whole frames (`sdlc_out` / `sdlc_in`),
  CRC by underrun after WR0 = C0h, address search, End of Frame, CTS looped to RTS for the
  point-to-point cable, DCD following the line for multidrop; `--gridlink` carries the
  frames over TCP (`docs/gridlink.md`). Tested against the real client code (CableTest)
  with `grid_core/tests/gridlink.rs`. No GRiD Server is built in.

## Sound

- Sound: the sound DAC (DFE4:2), volume DAC (DFE4:4) and the DTMF chip on 8255 port B,
  played through the PC's sound card (`--mute` to turn it off). All on the modem board,
  except on the 1137: no modem and no 8255 (DFEC0-DFECF read FFh, so Modem~Device~ finds no
  modem), DACs and DTMF chip on the motherboard. ASSUMED for the 1137 until SoundTest runs
  on one: the DACs answer at the same addresses and a write-only latch at DFEC2 drives the
  DTMF chip with the 8255 port B bits.

## Bubble memory

- 7220 bubble memory controller with a 384K bubble image (`--bubble`); GRiD's
  interrupt-driven driver (DRQ on IR1) reads and writes it. A blank image must be
  formatted with Initialize Media. Timing (`BubbleTiming`) fitted to GRiDProbe on a real
  1101: each MBM's loops turn only during a command (2048 slots of 199 clocks, 81.5 ms a
  turn) and stop where it ends; address `a` passes at slot `a * 181`, so consecutive pages
  stream 7.2 ms apart and a 1-page read costs 5.3 ms start + the wait for its slot + 3.3 ms
  (next page 12.5 ms, same page 87 ms, half a turn away 46 ms). Bytes move one every 400
  clocks while a page passes (two FIFO interrupts a page, 35 + 29 bytes, as on the real 1101);
  a write's FIFO takes data from 1.2 ms before the first page. Initialize takes ~6 ms and
  turns to the next of 40 stop positions (0.5 ms after an MBM Purge, until the next
  transfer); the short commands take their measured 0.08-0.43 ms. As on the real 1101
  (GRiDProbe B7): writes end with status 42 (parity bit; GRiD-OS ignores it), Read FSA
  Status gives FF 20 20 20 20 20 (28 for an MBM written since the last Software Reset),
  Abort takes 0.14-0.16 ms, and an Abort between two pages of a read leaves multi-page
  reads stopping after one page with status 2C until a Software Reset. `tests/devices.rs`
  replays GRiDProbe's seek patterns and the B7 states against the real numbers.

## Internal modem

- Internal modem (0.12.0, `grid_core/src/modem.rs`, [modem.md](modem.md)): the 8749 runs GRiD's
firmware (16020-007.bin) as an 8048-mode `Upi41` behind a mode 2 8255 at DFEC0, IRQ on
IR2; a line model drives its pins (dial tone, DTMF and pulse dialing, ring, the far modem
at 300 and 1200 baud with the 212A scrambler) and the frontend connects calls over TCP
(`--modem-dial NUMBER=HOST:PORT`, `--modem-listen tcp:PORT`, `host::ModemLine`).

## Not emulated yet

The test ROM socket at C0000h, a GRiD Server for GRiDLink, the modem's busy and ringback
tones and speaker, and the 8087's arithmetic on unnormal operands.
