# Build mode, headless runs and the Claude Code hook

Compass-Emu can build GRiD programs with GRiD's own compilers and linker, run a Compass
without a window, and let Claude Code drive a Compass through MCP. The user side of the
build (Project window) is in the [user guide](user-guide.md#build-in-the-window).

## The fast CPU core

Builds run on a fast whole-instruction 8086 core (`grid_core::fast86`) instead of MartyPC's
cycle-exact one. It executes whole instructions with approximate clock counts: no prefetch
queue, display contention or RAM wait states, and the display is captured once per frame
instead of as the beam passes. Devices still run on the same clock (the GPIB DMA window
keeps its wait states, so disk transfers pace as before), so GRiD-OS, the compiler and the
linker behave the same; only timing differs. `GridMachine::set_fast` switches cores between
two instructions, and the window can switch it while running (Machine menu).

The fast core passes all 630,000 SingleStepTests 8086 v1 tests
(`SST_DIR=.../8086/v1 cargo test -p grid_core --test fast86_sst`), except the flags a divide
error pushes (the real CPU pushes what its divide microcode left, the fast core its own).
hello.run and GRiDTiles (11 modules) build byte-identical on both cores.

## compass build

    compass build PROJECTFOLDER --disk DEVDISK.img [--roms DIR] [--debug] [--exact]

The same build as Project > Build in the window (`crates/bin/compass/src/build.rs`), no
Python needed. The project folder holds NAME.pas, NAME.asm and the other sources; the
development disk needs Pascal~Run~ and DevelopmentExecutive~Run~. The build runs on a hidden
Compass: an 1129, or an 1139 when the disk has the 113x build of GRiD-OS. The first build of
a disk boots it (about a minute) and saves that state in `states/` next to compass.exe;
later builds resume from it in a few seconds.

Results go to the project folder: NAME.run, NAME.lst (and NAME.asm.lst), screen.png, and
`output/` for other new files. `--debug` keeps the compilers' symbols (NAME.pas.obj,
NAME.asm.obj, NAME.map) for the debugger's program symbols. `--exact` builds on the
cycle-exact core. FATAL ERROR 9010 or 9011 means the development disk is full.

Speed: hello.pas builds in about 3.3 s on the fast core (the compile itself is 112
emulated seconds; 15 s on the exact core), GRiDTiles in 48 s (exact: 6 minutes). The 256K
models (1101, 1121, 1131) can't build: the compiler stops with "Main memory (RAM) is full".

## compass do

`compass do` runs one command on the Development Executive command line, headless and as
fast as the PC allows, then saves the screen and writes the disk back:

    compass do [--model M] --hd disk.img [--roms DIR] [--shot screen.png] [--start S] \
               [--max-seconds N] [--bubble F] [--state F] [--exact] "Do build"

It runs on the fast core unless `--exact` is given. The model defaults to the 1129 (512K,
11xx build of GRiD-OS). It waits for a steady `=>` prompt, types the command, and stops
when a new prompt ends the screen ("GRIDBUILD done after N emulated seconds"; exit code 2
at the time limit). Code: `grid_core::session`, `crates/bin/compass/src/headless.rs`.

`--state F` boots once and saves the machine at the prompt into F (`GridMachine::save_state`:
RAM, CPU, all devices and the 8087, as JSON; not the disk image). Later runs resume from F in
under a second with the disk from `--hd` attached, which may have gained files since
(GRiD-OS re-reads the bitmap on every attach).

The disk needs `Do~Run~` from the GRiD-OS 3.1.5 floppy in Programs (some disks only have
`Do~Run Com~`, which gives "33 File is not available").

## Claude Code hook (MCP server)

`compass mcp` is an MCP server on stdin/stdout (`crates/bin/compass/src/mcp.rs`). Add it to
Claude Code with

    claude mcp add compass -- C:\path\to\compass.exe mcp

or in `.mcp.json`:
`{"mcpServers": {"compass": {"command": "C:\\path\\to\\compass.exe", "args": ["mcp"]}}}`.

| Tool | What it does |
|------|--------------|
| `grid_build` | compile, link and test-run a GRiD Pascal (+ ASM-86) program: SOURCE is a .pas file or a project folder. With `disk` (a development disk image, only read) or `GRID_DEV_DISK` set, the build inside compass.exe runs; `debug: true` keeps the symbols. Returns the compiler and linker messages and the GRiD screen at the end. |
| `grid_boot` | start a Compass from a hard disk image (a copy in memory), wait for the prompt (or run `seconds`), return the screen; `fast: true` uses the fast core |
| `grid_type` | type keys (`{ret}`, `{esc}`, `{code+x}` ...), optionally wait for the next `=>` prompt; returns the screen |
| `grid_run` | run N emulated seconds, return the screen |
| `grid_screen` | the screen (PNG), emulated time, CS:IP, screen mode |
| `grid_read_memory` | hex dump at SSSS:OOOO or a linear address |
| `grid_save_disk` | write the running Compass's hard disk to a file |
| `grid_debug` | the debugger console commands on the running Compass (breakpoints, stepping, marks, symbols, crash catcher, GRiD-OS call log) |
| `grid_fragment` | time 8086/8087 machine code (hex or .bin) in CPU clocks, cycle-accurate with display contention |

Without `disk` or `GRID_DEV_DISK`, `grid_build` falls back to an external
`tools/gridbuild.py` build script, found through `GRID_ROOT` (or the folder above
compass.exe). `GRID_MODEL` sets the default model (1129).

## gridemu and gridfrag

Two small command-line tools in `crates/bin`, for tests and measurements:

    cargo build --release -p gridemu -p gridfrag

    GRID_HD=disk.img GRID_KBD_ROM=300067-01.bin \
      target/release/gridemu 1101even.rev3.bin 1101odd.rev3.bin 170 screen.png

`gridemu` boots a model for N emulated seconds and saves the screen. Drives come from
`GRID_HD`, `GRID_FLOP` (2102) and `GRID_FLOP2101`; `GRID_KBD_ROM` names the 8741 firmware;
`GRID_KEYS="seconds:text|..."` types keys (`{ret} {esc} {tab} {bs} {left} {right} {up}
{down} {code+x} {ctrl+x} {shift+x}`); `GRID_GPIB_TRACE=1` writes gpib_trace.txt;
`GRID_DUMP=file` saves RAM at the end; `GRID_BUBBLE` (+ `GRID_BUBBLE_SAVE`,
`GRID_BUBBLE_LOG`), `GRID_SERIAL_IN` / `GRID_SERIAL_OUT` / `GRID_SERIAL_LOG` and `GRID_WAV`
(record the sound) add the rest.

`gridfrag code.bin` runs a code fragment at 1000:0000 until HLT and prints the clocks and
the 8087 state. `FRAG_MODEL` picks the model, `FRAG_PNG` saves the screen, `FRAG_DUMP=addr,len`
prints memory. The display and bus settings can be overridden to try timing models
(`DISP_*`, `RAM_EXTRA_WAIT`, `JMP_EXTRA`, `GRID_REFRESH`...): the full list is in
[developer.md](developer.md#environment-variables). `GRID_TRACE=file` (any tool) writes a
clock-by-clock CPU trace.
