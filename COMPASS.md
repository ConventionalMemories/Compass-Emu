# Compass-Emu

A cycle-accurate emulator for the GRiD Compass, built on the 8086 core of
[MartyPC](https://github.com/dbalsom/martypc) by Daniel Balsom (MIT). The original MartyPC
README follows in README.md.

Goals: a complete emulator anyone can use (drives, keyboard, sound, serial), accurate
enough to race the display beam, plus hooks so tools can run and measure code fragments.
Models: 1101, 1109, 1121, 1129, 1131, 1137 and 1139 (`--model`).

## Status (0.11.0)

- Frontend (`crates/bin/compass`): an egui/eframe (OpenGL) GUI in `src/gui/` (menus, machine
  setup with profiles, drag and drop, recent disks, full screen and saved view settings, media
  manager with a GRiD file browser built on `grid_core::hostdir::Volume`,
  debugger workspace, chips window, ROMs card and per-drive emulation level in `setup.rs`, the
  2101 LED display in `led.rs`, the on-screen keyboard in `osk.rs`, the Project window in
  `project.rs`); machine building in `session.rs`, real-time pacing, the speed setting, pasted
  text and keys in `runner.rs`; the minifb window of 0.4 is `classic.rs` (`--classic`, and the
  fallback when the GUI can't start).
- Builds inside compass.exe (`src/build.rs`, `compass build`): the gridbuild.py flow on a hidden
  1129/1139 with the fast core, booted state cached per disk under `states/`. ROM import by
  CRC-32 (`src/romimport.rs`, `compass import-roms`). The PC folder drive takes in files added
  or changed on the PC while running (`hostdir::HostDir::sync_in`).
- ImageDisk floppies (`src/imd.rs`): an .IMD is used through the .img beside it, made from it
  when missing.
- Debugger (`grid_core::debug`): breakpoints, watches, profiler, call stack (BP chain checked
  against CALLs before each return address), recent instructions, a GRiD-OS call log (calls
  through vectors 70h/71h/73h named from the Compact*Calls libraries, `grid_core::oscalls`)
  and a crash catcher (OsDecodeException with the message text, divide error, INT 3 that is
  not the loader's stand-in for an 8087 WAIT, running through 00 00 / FF FF). Program symbols
  (`grid_core::symbols`) come from the OMF-86 objects of a DEBUG build (BLKDEF, PUBDEF,
  DEBSYM, LINNUM, FIXUPP for masking) plus the Pascal/ASM-86 listings; each module is found in
  RAM by its fixup-masked code bytes, since the bound .run keeps no symbols.

- 8086 with prefetch queue and bus timing (MartyPC, `CpuSubType::Intel8086`), with one
  timing fix for ALU r/m,imm writes. A fitted Compass quirk adds one clock before a JMP SHORT
  suspends prefetch (`cpu_808x::JMP_SHORT_EXTRA`, set by grid_core, `JMP_EXTRA` overrides). SingleStepTests 8086 suite: 0 register/flag/memory
  errors, about 94.5% of tests with exact clock counts.
- A fast whole-instruction 8086 core for build mode (`grid_core::fast86`), switchable at any
  instruction boundary; same SingleStepTests results for registers, flags and memory.
- 1101 board (`crates/lib/grid_core`): 16K boot PROM, 256K RAM, 80130 interrupt
  controller (8259A on the 113x), IR3 vertical blank, 320x240 screen at 0400h.
- TMS9914 GPIB with the pseudo-DMA window at E0000h; 2101 hard disk (address 4), 2101
  floppy (5) and 2102 floppy (6) at the level of GRiD's disk protocol. In exact mode the
  hard disk has mechanics like a real 2101 with an ST-225: 3600 rpm, 17 sectors a track,
  4 heads, interleave 5, seeks interpolated from real measurements (1 cylinder 26.4 ms,
  38 cylinders 80 ms, full stroke 148 ms), a 6-page read-ahead cache that writes drop, and
  writes answered at once and done while the next command waits (`HD_NOMECH=1` turns this
  off; fast mode never uses it). A multi-page read replies in 512-byte pages, each ending
  with EOI, as GRiD's BB2 driver accepts only 512-byte replies. Against a real 1101 + 2101
  (GRiDBench 2.1b): same page 19.4 ms (real 19.0), sequential 19.4 (21.1), random 130.0
  (130.3), steps of 1/16 120.6 (122.5), 8 pages 65 (53.5).
- Floppies (exact mode): 300 rpm, 9 sectors, interleave 5 with side 1 skewed by 7 sectors,
  3 ms a cylinder + 30 ms settle, one WD279x command a page. The 2102 reads one page a
  command and is slow (about 40 ms to take a command, 60 us a byte over its bit-banged GPIB).
  Against a real 2102 with a Gotek (GRiDBench 2.1b): same page 197 ms (real 212),
  sequential 121 (133), first/last 395 (415), steps of 1/16 211 (172). With a 2101 hard
  disk and no floppy image, the box's floppy slot answers as empty (error 128, as on a
  real 2101), so GRiD-OS's polls of address 5 don't hang. `GPIBREG=clock` prints every
  9914 register access from that clock on.
- 2102 low-level (when `roms` holds the 2102 firmware `300237-02.bin` or `300237-01.bin`):
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
- 2101 low-level (when `roms` holds `300205-05.bin` (20 MB) or `300205-06.bin` (10 MB), or
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
- A PC folder as a GRiD drive (`--folder DIR`, `grid_core::hostdir`): at start the folder is
  built into a GRiD volume (files in the root, sub-folders as subjects, up to 32 MB with 4 MB
  free) on the 2101 floppy address (GPIB 5); the volume is named after the folder. On quit,
  files GRiD made or changed are written to the folder and files GRiD erased are deleted
  there. Host files without a GRiD kind become `name~Text~` (LF line ends to CR LF) and keep
  their PC name; characters Windows can't store are written as %XX. The drive answers
  with its real size in the status block, which GRiD-OS uses for a volume without a
  partition table. Writes with control byte 1 (set status) or 2 (box upload) no longer land
  on page 0 of a drive.
- 8259 interrupt controller: a request that is pending when its IR is unmasked (or a lower
  one after an EOI) raises INT at once. Before 0.3.7 it waited for the next unrelated
  interrupt, so each disk command waited for an IR3 tick (3 refreshes a page).
- Interrupts are sampled inside REP string instructions (`ExternalBoard::intr_now`) and
  INTR is updated before the next instruction starts, so a pseudo-DMA `REP MOVSB` stops
  right after the byte that raised the GPIB END interrupt (it used to copy 1-2 more bytes,
  which made the BB2 driver reinitialize the bus for half a second).
- Demo tools (0.4.0): cycle counter marks in `grid_core::marks` (OUT 0F0h/0F2h,AL start/end
  section AL, OUT 0F4h,AL frame mark; the board catches writes to ports >= 80h, which a real
  Compass doesn't decode), debugger and profiler in `grid_core::debug` (code breakpoints
  checked before each instruction, port and IR breakpoints reported by the board, memory
  write watches by comparing the bytes after each instruction, so they work on both cores;
  disassembly with MartyPC's decoder). Window: F10 overlay, F9 clear, F8 pause, console
  commands on stdin. MCP: `grid_debug`. The per-instruction checks run only while a
  breakpoint, watch or profile is set (`GridMachine::run_debug`).
- MM58174 real-time clock (digits read 0Fh for ~30 us after each tenth while it updates).
- 8741 keyboard microcontroller running the real firmware (300067-01 or 300001-02). Its
  P2.4 (kbdDma) is wired to the GPIB wait: a DMA window access holds the CPU until the
  9914 is ready, its interrupt is up, or the firmware's watchdog drops kbdDma (about 4 s
  after GRiD-OS arms it), then the FDh it sends makes GRiD-OS end the transfer with
  error 451 and set kbdDma again. While kbdDma is low the window doesn't wait.
  `dma_wait_limit` (20 s) is only a backstop. `GPIB_STALL=N` holds the Nth byte a drive
  sends for 10 s, to watch this happen.
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
- Sound: the sound DAC (DFE4:2), volume DAC (DFE4:4) and the DTMF chip on 8255 port B,
  played through the PC's sound card (`--mute` to turn it off). All on the modem board,
  except on the 1137: no modem and no 8255 (DFEC0-DFECF read FFh, so Modem~Device~ finds no
  modem), DACs and DTMF chip on the motherboard. ASSUMED for the 1137 until SoundTest runs
  on one: the DACs answer at the same addresses and a write-only latch at DFEC2 drives the
  DTMF chip with the 8255 port B bits.
- Boots GRiD-OS (1101 build of CCOS) from a 2101 hard disk image to the Executive.
- Models (`GridModel` in grid_core/src/machine.rs, `--model` in the window version,
  `GRID_MODEL` headless):

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
  113x shows the 320 x 240 picture centred. Large-screen timing is from GRiDBench 2.2 on a
  real 1137: 281 clocks a line, 297 lines (59.9 Hz; AAM loop in refreshes, the 1137's RTC
  was dead), IR3 at the top, 32 display fetches on every line, one every 16 states (gap rows 1.2%
  rms). The split board's bus also follows the 1137: empty memory reads the address, the
  PROM shows at F8000-FBFFF, the 7220 repeats up to DFE8F; on the 113x (8259A + 8254)
  ports 4/6 mirror the PIC and 0E is open, on the 112x (80130) they behave as on the 1101.
  All seven boot to the Executive (1131/1137/1139 with the 113x build, the others with the 11xx
  build). The 1139 PROM can't stand in for the 1121/1129 PROM: it keeps its data
  at 0440:0, where the 11xx build of CCOS loads (02D8:0 up), and stops with "Checksum
  error in CCOS". The 1129 PROM (data at 0298:0, like the 1101's) boots the 11xx build on
  the plain split-board map, and is also the 1121's. Without it the 1101 PROM
  stands in, with the split board also answering the 1101's DMA window at E0000h
  (`GridBoard::e0000_dma`). ROM choice: grid_core/src/roms.rs.

Not yet: the modem itself (its microcontroller was never dumped), GRiDLink, the test ROM
socket at C0000h.

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
`GRID_GPIB_TRACE=1` writes gpib_trace.txt; `GPIBLOG=1` prints disk commands, SRQs and
transfer ends with their clock on stderr; `GRID_DUMP=file` saves RAM at the end.

`gridfrag code.bin` runs a code fragment at 1000:0000 until HLT and prints the clocks and
the 8087 state (`FPU_EXTRA`, `FPU_WAITMIN` override the 8087 timing fit; `DISP_CONTENTION=0`,
`DISP_FETCH_LEN`, `DISP_FETCH_PERIOD`, `DISP_CPU_LEN`, `DISP_CPU_WRITE_LEN`,
`DISP_LINE_CLOCKS`, `DISP_FRAME_LINES` the display; `FRAG_MODEL` the model, `RAM_EXTRA_WAIT`
extra wait states on every RAM access, `JMP_EXTRA` the JMP SHORT quirk, `GRID_REFRESH` /
`GRID_REFRESH_WAIT` the bus steal). `GRID_TRACE=file` (any tool) writes a clock-by-clock
CPU trace.

Headless extras: `GRID_BUBBLE` (+ `GRID_BUBBLE_SAVE`, `GRID_BUBBLE_LOG`), `GRID_SERIAL_IN`
/ `GRID_SERIAL_OUT` files, `GRID_WAV` to record the sound.

Timings still to be measured on real hardware are listed in docs/real-hw-tests.md.

ROMs and disk images are not included.

## Credits

Compass support by Conventional Memories (C.M._), MIT license. Device behaviour is based
on GRiD's own documentation and sources and on MAME's gridcomp, tms9914, grid2102 and
mm58174, i7220 and z80sio drivers (BSD-3-Clause). 8087 arithmetic uses rustc_apfloat (Apache-2.0 WITH
LLVM-exception).
