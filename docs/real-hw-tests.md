# Real-1101 measurements for the combined test session

Collected while building Compass-Emu (display timing 0.0.5; bubble, serial and sound 0.1.0). The emulator has
adjustable settings for each item; these tests pin them down. To be bundled with the 8086,
bubble and other checks into one test program later.

| # | What | Why | How (idea) |
|---|------|-----|------------|
| D1 | Frame length in CPU clocks | IR3 rate and everything frame-based | Count iterations of a contention-free loop (run in vblank only is impossible, so: a loop with known emulated cost) between IR3 edges, over 64 frames |
| D2 | Where IR3 fires relative to the visible picture | Racing the beam: when is it safe to draw | "Beam finder": after IR3, wait N clocks, then flip a full-width screen band; the line where the band visibly tears gives the beam line at time N. Sweep N |
| D3 | Line length (clocks per line) and hblank position | Contention phase inside a line, tear column | Same beam finder with fine N steps: the tear moves one line every line length; the column of the tear shows hblank |
| D4 | Contention cost per access in the visible part vs vblank | Wait-state model (fetch slot, CPU window) | Run MOV AX,[SI] / MOV [SI],AX / REP MOVSW blocks right after IR3 (in vblank) and mid-frame; count blocks per frame |
| D5 | Does code fetch from the PROM / ROM socket avoid contention | Which addresses are contended | LOOP and MOV AL,[x] blocks reading PROM (FC000) vs RAM |
| D6 | Is RAM above 20000h (second bank) contended too | Patent says one arbiter for all RAM | MOV AX,[SI] blocks at 1000h vs 30000h |
| D7 | GRiDBench c/m/s numbers again with IR3 sync | Current fit uses one report | Re-run GRiDBench after D1 gives a real time base |

Display: current emulator model (adjustable, see `DisplayTiming` in grid_core/src/screen.rs): 309
clocks/line, 266 lines (60.8 Hz), 240 visible; display fetches one word every 12 clocks
during the first 240 clocks of each visible line and holds RAM 6 states (2 clocks); a CPU
read needs RAM 3 states from T2, a write 2; one that collides waits until the slot ends
(net cost on GRiDBench's memory tests ~3%, not the ~14% the patent's numbers suggest).
IR3 rises at line 240. Fit to GRiDBench 8086/memory numbers: 2.0% mean error; open misfit:
REP STOSW 12.2 vs 11.15 clocks a word.

Note: GRiDBench "c" values are scaled by AAM = 83; the emulator (validated against real
8086 tests) gives AAM 77, so all real-hardware numbers are compared after the same scaling.


## Compass II (split board, 113x large screen)

| # | What | Why | How (idea) |
|---|------|-----|------------|
| L1 | Frame length and IR3 position on a 113x in large-screen mode | Emulator guesses 269 lines of 309 clocks (60.1 Hz from GRiDBench spins 1672 vs 1623) | D1/D2 on the 1137 |
| L2 | Display fetches per line and their spacing in large-screen mode | Emulator: 32 fetches, one every 8 clocks; fits GRiDBench at 3.1% | D3/D4 on the 1137 |
| L3 | Timing in compatibility mode (before the OS turns the large screen on) | Emulator keeps the large-screen timing and shows 320 x 240 centred | D1 + D4 with the 11xx PROM screen up (e.g. PROM boot menu) |
| L4 | What each DFE04 bit does (0Eh PROM, 0Bh/0Ah OS) | Emulator: bit 2 clear = large screen; bits 0-1 look like RAM size | Write values from a test program with DFE00 = 1 and watch the screen |
| L5 | Does a 256K split board mirror RAM above 40000h | Emulator returns FFh there | Write/read 4000:0 vs 0000:0 on a 1131/1121 if one turns up |
| L6 | 1121/1129 boot PROM dumps | The 1139 PROM can't start the 11xx build (its data sits where that CCOS loads) | Dump the PROM pair of a 1129 (MAME lists one: 1129even/odd.ccprom.bin) |

## Bubble memory (7220 + 3 x 7110)

| # | What | Why | How (idea) |
|---|------|-----|------------|
| B1 | Time to read N bubble pages (64 bytes each) in one command, N = 1, 4, 16, 64 | `page_clocks` and `start_clocks` (now MAME's 1.35 ms per 32-byte FSA page, ~24 KB/s) | Drive the 7220 directly like Bubble Fix does: Read Bubble Data, poll the FIFO, count IR3 ticks / loop turns from command to "done" |
| B2 | Same for writes | Write timing can differ from reads | Write Bubble Data of N pages on a scratch area (restore it after) |
| B3 | Seek cost: read 1 page at address 0 then at 2047 vs. at 1 | Start-of-transfer delay depends on how far the loops must turn | Time single-page reads after reads at different addresses |
| B4 | Initialize and Read FSA Status times | `init_clocks_per_mbm` | Time the commands |
| B5 | How often DRQ (IR1) fires during a read and how full the FIFO is then | Confirms the 22-byte threshold the driver relies on | In an IR1 handler, read the status and count FIFO bytes |
| B6 | Abort while busy, status bits after it | Error paths GRiD's driver uses (retries) | Abort in the middle of a read, print status |
| B7 | OS-level: copy a 20K file to and from the bubble | End-to-end check against the emulator | Time it with a stopwatch, compare with the emulator |

## Serial (8274, 80130 baud timer)

| # | What | Why | How (idea) |
|---|------|-----|------------|
| S1 | Baud timer count GRiDTerm programs for 300, 1200, 9600, 19200 | The emulator derives bit rate from the 80130 timer 2 count and the 8274 clock mode | Read the ports after GRiDTerm sets the rate (or log in the emulator and compare the real bit rate with a PC terminal) |
| S2 | Real bit rate at 9600 (is it 9470 = 5 MHz / 33 / 16?) | Character timing | Send a long block to the PC, time it there |
| S3 | Highest rate that works without losing characters, both ways | Interrupt latency of the real driver | GRiDTerm + Tera Term file send at increasing rates |
| S4 | Which ST0-ST3 latch values select which clocks | GRiDLink (external clock) and multidrop later | Try GRiDTerm settings, read back nothing (write-only): note what GRiDTerm writes via the emulator log |
| S5 | DCD/CTS behaviour: does GRiDTerm wait for DCD | Emulator raises DCD/CTS when a TCP client connects | Unplug/plug the cable during a session |

## Sound (modem board)

| # | What | Why | How (idea) |
|---|------|-----|------------|
| A1 | DAC output level vs value (0, 64, 128, 192, 255) | Emulator assumes linear, 0 = silence level | SoundTest test 3 with a recorder or scope on the speaker |
| A2 | Does the volume DAC (DFE4:4) change DTMF loudness | Emulator scales DTMF by the volume DAC | SoundTest 7.2 (key 5 held, volume 255 -> 0) |
| A3 | DTMF loudness compared with a full-scale DAC square wave | Mix level in the emulator | Record both |
| A4 | Speaker frequency response (lowest/highest audible tone) | Optional filter in the emulator | Sweep with SoundTest test 2 |
| A5 | Sample-rate limit: does a 22 kHz PCM loop sound the same | Checks the emulator's per-sample averaging | SoundTest test 5, record and compare |

## 8087 FPTAN (chip's own partial tangent)

FPTAN now follows the chip's method (crates/lib/i8087/src/fptan.rs, after Ken Shirriff's
2026 microcode study): CORDIC with 16 arctan constants, 3r/(3-r^2) for the rest, rotations
applied back. Its Y/X pair and its time depend on the argument. The algorithm is known but
the last bits (constant rounding, register widths, truncation) are assumed; GRiDBench f02 is
the only real data so far (model 6 ulps off after FSTP qword X; the old X = 1 was 73 off).

| # | What | Why | How |
|---|------|-----|-----|
| T1 | ST0 (X) and ST1 (Y) as 80-bit TBYTEs after FPTAN, for the inputs below | Fixes the last bits: constant rounding, widths of X and Y, rounding when stored | FLD TBYTE x; FPTAN; FSTP TBYTE X; FSTP TBYTE Y; print both in hex (FSTP TBYTE keeps all 64 bits, unlike f02's qword) |
| T2 | FPTAN time per input (loop of 100, as GRiDBench g06) | Per-input busy time (data sheet 30-540 clocks; model 36-543) | FLD ST(0); FPTAN; FSTP ST(0); FSTP ST(0) loop, minus the same loop without FPTAN |

Inputs (exact 80-bit values, from the test program's own table): 0.5, 0.95, 0.75, 0.3, 0.1,
0.01, 0.001, 1/4096, 2^-15 * 1.5, 2^-16 * 1.25 (last CORDIC exponent), 2^-17 * 1.5 (first
without CORDIC), 1e-10, 2^-63 * 1.5 (last with 3x/(3-x^2)), 2^-64 * 1.5 (Y = x, X = 1),
0.785 (near pi/4), 1.0, 1.5 (outside the documented range), 0, -0.5. With T1 for these, the
model's free choices can be pinned down and FPTAN made bit-exact.

## Real-time clock (MM58174) as a time base

Kurt's 1101 has a new RTC battery (2026-11-10), so absolute times no longer need the AAM
scaling. The clock's crystal is 32.768 kHz, normally within 0.01 percent; its tenths
register gives 0.1 s steps, so timing over 60 s or more is good to about 0.2 percent.

| # | What | Why | How |
|---|------|-----|-----|
| R1 | RTC drift against a phone clock over 24 h | Trust the time base | Set the time, compare after a day (seconds off / 86400) |
| R2 | CPU clock in Hz (is it 5.000 MHz?) | Every clock count in the emulator assumes 5 MHz | Count a loop of known clocks between two tenths-register changes, over 60 s |
| R3 | Frame rate in Hz (IR3 per second) | 56 vs 61 Hz estimates disagree | Count IR3 ticks over 60 s of RTC time |
| R4 | OsGetTime against the RTC | GRiD-OS time calls were 200-430 ms with a dead clock | Time OsGetTime calls with the RTC running |

## More groups (added 2026-11-11)

| # | What | Why | How (idea) |
|---|------|-----|------------|
| F1 | FPATAN, F2XM1, FYL2X, FYL2XP1, FSQRT results as full 80-bit values on ~20 inputs each | Make the rest of the 8087 bit-exact (GRiDBench f03 FPATAN already differs by 1 bit) | FLD inputs from a table, run the instruction, FSTP TBYTE, print hex |
| F2 | Their times per input, and FADD/FMUL/FDIV/FLD/FST times by operand | 8087 busy times per instruction (only typical values fitted so far) | Loops as in GRiDBench g-tests, one input each |
| P1 | 80130 interrupt inputs: edge or level triggered | The 0.3.5 sound fix assumed edge-triggered IR0 | Enable the 8274 TX interrupt, let it stay pending, EOI without serving the 8274: level = the routine runs again at once, edge = once; count calls over 10 ms |
| P2 | Interrupt latency: from the line rising to the first instruction of the routine | Racing-the-beam code that starts work in the IR3 routine | Spin on a counter in a known-clock loop; the routine stores the loop count it interrupted (IR3 vs refresh counter) |
| P3 | 80130 timers 0-2: what they count and whether any reaches an interrupt | GRiDBench T1 probe gave nonsense; timer 2 is the baud clock | Program each timer, read back, watch IRR bits |
| K1 | 8741 scan rate: how fast a key press shows in the status byte | Keyboard latency in games | Interactive: press a key, count loop turns between status changes |
| K2 | The 2-key limit: hold A + L, press Q; Shift/Code status bits with keys held | BomberGRiD 2-player keys depend on it (not checked on hardware) | Interactive: show the matrix keys and DFFC:2 bits live |
| K3 | Auto-repeat delay and rate | Emulator uses the real 8741 firmware; confirms its timing | GRiDBench keyboard menu already measures it |
| G1 | Hard disk and floppy: read/write speed, seek near vs far | Disk-heavy programs and builds at true speed | OS-level file tests (GRiDBench menu 1) plus page reads at far-apart addresses |
| G2 | Time spent in the GPIB DMA window per 512-byte page | The emulator's DMA wait model | Time OsRead of one page vs many |
| C1 | REP STOSW / REP MOVSW clocks per word in RAM and screen RAM | The one 8086 timing that still misfits (12.2 vs 11.15) | Long REP runs timed with the RTC |
| C2 | MUL/IMUL/DIV/IDIV clocks for several operand values | They depend on the operand bits | Loops with fixed operands |
| C3 | INT n / IRET round trip | Software interrupt cost (GRiD-OS calls) | Loop of INT to a dummy handler |
| U1 | What reads return from empty memory (80000, A0000, C0000, D0000, F0000) and unused I/O ports (10h-FFh) | Programs that probe for hardware; emulator returns FFh | Read bytes and words, print them (skip device registers with side effects) |
| U2 | Which device registers repeat at other addresses (DFxxx mirrors, port 0-0F mirrors) | Address decoding | Compare reads of harmless status registers at mirrored addresses |
| X1 | Boot time: power switch to the Executive, and to the PROM menu | Emulator boot speed check | Stopwatch, or RTC time saved at startup vs when switched on |
