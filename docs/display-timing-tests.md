# Real-1101 measurements for the combined test session (display timing part)

Collected while building the display timing model (Compass-Emu 0.0.5). The emulator has
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

Current emulator model (adjustable, see `DisplayTiming` in grid_core/src/screen.rs): 309
clocks/line, 266 lines (60.8 Hz), 240 visible; display fetches one word every 12 clocks
during the first 240 clocks of each visible line and holds RAM 6 states (2 clocks); a CPU
read needs RAM 3 states from T2, a write 2; one that collides waits until the slot ends
(net cost on GRiDBench's memory tests ~3%, not the ~14% the patent's numbers suggest).
IR3 rises at line 240. Fit to GRiDBench 8086/memory numbers: 2.0% mean error; open misfit:
REP STOSW 12.2 vs 11.15 clocks a word.

Note: GRiDBench "c" values are scaled by AAM = 83; the emulator (validated against real
8086 tests) gives AAM 77, so all real-hardware numbers are compared after the same scaling.
