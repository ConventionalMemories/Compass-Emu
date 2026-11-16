# Real-hardware measurements

How Compass-Emu's timing and behaviour were pinned down, and what is still open. I measured
everything here on my own machines: a Compass **1101** (with a 2101 hard disk and a 2102
floppy) and a **1137**, using test programs written for the purpose (GRiDBench, GRiDProbe,
8087 Bench, Disk Toolkit, SoundTest). The results were fitted into the emulator from 0.3.6
on; [CHANGELOG.md](../CHANGELOG.md) says what each version changed and
[accuracy.md](accuracy.md) has the resulting numbers.

**Done** means measured and used by the emulator. **Partly** means measured on one machine
only or not fully used. **Open** means not measured yet: the emulator has a setting or an
assumption there that a measurement would settle. If you have a Compass and want to help,
the open rows are the list.

## Display and bus

| # | What | Status | Result |
|---|---|---|---|
| D1 | Frame length in CPU clocks | Done | 1101: 307 clocks x 268 lines (60.8 Hz at 5 MHz). GRiDProbe 0.0.13 beam finder. |
| D2 | Where IR3 fires relative to the picture | Done | Rises on line 3 of the picture, high for 241 lines. |
| D3 | Line length and the tear column | Done | 307 clocks a line (1101). |
| D4 | Display RAM contention per access | Done | One display slot (6 states) every 32 states on every line, blank ones too (GRiDProbe's contention map is flat across the frame). |
| D5 | Does code from the PROM or a ROM socket avoid contention | Partly | GRiDBench 2.2's PROM rows match on both machines; ROM sockets not measured. |
| D6 | Is RAM above 20000h contended too | Open | The emulator contends all RAM, as the patent's single arbiter suggests. |
| D7 | GRiDBench 8086 and memory rows with an IR3 time base | Done | Fitted in 0.6.1/0.6.2 with the JMP SHORT quirk: 1.2 % rms on the 1101. |
| U1 | What empty memory and unused ports read | Done | The address still on the bus (low byte on even addresses, A8-A15 on odd); D0000-DEFFF reads FF on even bytes. |
| U2 | Mirrors of device registers | Done | The PROM also at F0000-FBFFF; ports 00-7F mirror the 80130 every 10h. |

## Compass II (split board, 113x large screen)

| # | What | Status | Result |
|---|---|---|---|
| L1 | Frame length and IR3 position on a 113x | Done | 1137: 316 clocks x 263 lines (60.2 Hz), IR3 high 259 lines. |
| L2 | Display fetches per line in large-screen mode | Done | One every 16 states, 32 a line, 3 states each; a CPU access needs the RAM 4 states, so data accesses are about 1 clock slower than on the 1101. 1.4 % rms on GRiDBench's gap rows. |
| L3 | Timing before GRiD-OS turns the large screen on | Open | The emulator keeps the large-screen timing and shows 320 x 240 centred. |
| L4 | What each DFE04 bit does | Open | Emulator: bit 2 clear = large screen; bits 0-1 look like RAM size. |
| L5 | Does a 256K split board mirror RAM above 40000h | Open | The emulator returns FFh there. Needs a 1121 or 1131. |
| L6 | 1129 boot PROM dump | Done | A real 1129 PROM, the same as MAME's grid1129. |
| L7 | Does the 113x's 8259A take a non-specific EOI | Open | Assumed yes (the 1101's 80130 ignores it, measured). |
| L8 | How the 8254's timer 1 is gated | Open | The emulator counts it only while timer 0 has a count. |

## CPU, interrupts and timers

| # | What | Status | Result |
|---|---|---|---|
| C1 | REP STOSW / REP MOVSW clocks per word | Partly | Measured; the emulator still gives 12.2 clocks a word against 11.15 real. REP LODSB on the 113x is about 10 % slow. |
| C2 | MUL/IMUL/DIV/IDIV clocks by operand | Open | From MartyPC's cycle-exact 8086 (SingleStepTests). |
| C3 | INT n / IRET round trip | Done | INT/IRET take 1 clock (113x: 2) more than MartyPC's model, a WAIT 2 more. |
| P1 | 80130 inputs edge or level triggered | Open | The emulator treats IR0 as edge-triggered. |
| P2 | Interrupt latency to the first instruction of the handler | Partly | GRiDProbe measures it; the emulator's IR3 handler may start before a poll loop sees IRR. |
| P3 | What the timers count | Done | 80130: timer 0 at 2.5 MHz, timer 1 counts timer 0's pulses, timer 2 at 5 MHz, no read-back. 113x 8254: all three at 5 MHz, read-back status B4/B6, counters stand at FFFF until loaded. |
| R1 | Real-time clock drift over 24 h | Open | |
| R2 | CPU clock in Hz | Partly | About 5.0 MHz from disk and keyboard rates; not yet timed against the RTC. Every clock count assumes exactly 5 MHz. |
| R3 | Frame rate in Hz | Partly | About 60 Hz; follows from D1 if R2 is 5.000 MHz. |
| R4 | OsGetTime against a running RTC | Open | Measured only with a stopped clock (200-430 ms a call). |

## 8087

| # | What | Status | Result |
|---|---|---|---|
| T1 | FPTAN's 80-bit X and Y on chosen inputs | Done | 19 real results; the model follows the chip's CORDIC method, 18 bit-exact. |
| T2 | FPTAN time per input | Done | Fitted with the microcode timing. |
| F1 | Transcendental and square root results as 80-bit values | Done | 141 results from the real 1101: 134 bit for bit, 7 one unit off. |
| F2 | Every instruction's time by operand | Done | 8087 Bench, 2068 timed tests on the 1101 and 1137: microcode timing 1.45 % mean error (1101). |
| F3 | The constant ROM (8087 Bench 3, 1584 tests) | Open | Ready to run (8087 Bench 0.0.5). Should settle most of the 34 result lines that still differ. |
| F4 | Power-on register contents | Done | All eight registers FCA9AFEAA3BE028FA027 on the 1101 (FXAM of empty = 4700); the 1137's differ per register. |

## Disk drives

| # | What | Status | Result |
|---|---|---|---|
| G1 | 2101 and 2102 page read times by pattern | Done | GRiDBench 2.1b on the 1101; see [accuracy.md](accuracy.md). |
| G2 | Time in the GPIB DMA window per 512-byte page | Open | |
| G3 | 2101 box timing from inside (Disk Toolkit Box probe) | Done | Heads-in times 1 cylinder 9.5 ms, 38 21.5 ms, 611 70.4 ms; Seek buffered; command decode 0.44 ms. The low-level 2101's sequential reads are still slow (26.8 ms against 21.1). |
| G4 | 2102 idle power-down | Done | The next read after 12-30 s idle takes 1.9-2.0 s (emulator 1.8-1.9 s). |

## Bubble memory (7220 + 7110)

| # | What | Status | Result |
|---|---|---|---|
| B1 | Time to read N pages in one command | Done | 1101: pages stream 7.2 ms apart; loops turn only during a command (81.5 ms a turn). |
| B2 | Write timing | Partly | The real board gave one IR1 per write where GRiD-OS's driver needs several; the emulator keeps the driver working. |
| B3 | Seek cost by distance | Done | 1101: 5.3 ms start + the wait for the page's slot + 3.3 ms. |
| B4 | Initialize and Read FSA Status | Done | Initialize about 6 ms; short commands 0.08-0.43 ms. |
| B5 | FIFO interrupts during a read | Done | Two a page (35 + 29 bytes). |
| B6 | Abort while busy | Done | 0.14-0.16 ms; a later multi-page read stops after one page with status 2C until a Software Reset. |
| B7 | The 1137's bubble | Open | It looked slower than the 1101's; the emulator uses the 1101's timing on every model. |

## Serial, GRiDLink and modem

| # | What | Status | Result |
|---|---|---|---|
| S1 | Baud timer counts GRiDTerm programs | Open | Derived from GRiD's code. |
| S2 | Real bit rate at 9600 | Open | 5 MHz / 33 / 16 = 9470 would be expected. |
| S3 | Highest rate without lost characters | Open | |
| S4 | Which ST0-ST3 latch values select which clocks | Partly | Read from GRiDLink's code (multidrop 00 FF FF 00, point-to-point FF 00 FF 00); not checked on a cable. |
| S5 | DCD/CTS behaviour | Open | The emulator raises DCD and CTS while a TCP client is connected. |
| N1 | GRiDLink against a real cable and server | Open | Tested only against GRiD's CableTest playing the server's side in the emulator. The 8274's SDLC receive FIFO depth (5) is assumed. |
| M1 | The modem's 8749 pins P1.3/P1.4 and the ring line to IR7 | Open | Needs tracing on a real modem board (firmware commands 7 and 8). |
| M2 | A real call between a Compass and a PC modem at 300 and 1200 baud | Open | The emulator's line model follows the firmware and GRiD's driver. |

## Sound

| # | What | Status | Result |
|---|---|---|---|
| A1 | DAC output level against value | Open | The emulator assumes linear, 0 = silence level. |
| A2 | Does the volume DAC change DTMF loudness | Open | The emulator scales DTMF by it. |
| A3 | DTMF loudness against a full-scale DAC wave | Open | |
| A4 | Speaker frequency response | Open | |
| A5 | Does a 22 kHz PCM loop sound the same | Open | |
| A6 | Where the 1137's DACs and DTMF chip sit | Open | Assumed: DACs at the same addresses, a write-only latch at DFEC2 for the DTMF chip (the 1137 has no 8255). SoundTest on the 1137 would settle it. |

## Keyboard and boot

| # | What | Status | Result |
|---|---|---|---|
| K1 | Keyboard scan rate | Open | The emulator runs GRiD's own 8741 firmware, so it should follow. |
| K2 | The 2-key limit and Shift/Code bits with keys held | Open | |
| K3 | Auto-repeat delay and rate | Open | From the firmware. |
| X1 | Boot time from power on to the Executive | Partly | Bubble boot screen 3.73 s real, 2.85-3.7 s emulated; the Executive time not measured. |

## How the open rows could be measured

- **D6, L3, L4, L5:** GRiDProbe-style loops after IR3, with the screen register changed from
  the test program.
- **R1-R4:** with a working RTC battery, count IR3 ticks and a known-clock loop between
  changes of the RTC's tenths register over 60 s or more (good to about 0.2 %).
- **F3:** run 8087 Bench 0.0.5's third program and send the report.
- **B7, A6:** GRiDProbe's bubble tests and SoundTest on the 1137.
- **S1-S5, N1:** GRiDTerm and a PC terminal at increasing rates; a real GRiDLink cable to a
  GRiD Server or a PC with an RS-422 adapter.
- **M1, M2:** tracing the modem board, and a call to a PC modem on a phone line simulator.
- **K1-K3, X1:** interactive tests and a stopwatch.
