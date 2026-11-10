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
