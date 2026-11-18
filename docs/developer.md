# Developer guide

How the source is laid out, how to build and test it, and every environment variable the
code reads. The hardware model itself is described in [hardware.md](hardware.md) and
[drives.md](drives.md).

## Crates

| Crate | What it holds |
|---|---|
| `crates/lib/marty_core` | The part of [MartyPC](https://github.com/dbalsom/martypc) that is kept: the cycle-exact 8086 core and its bus (see [MARTYPC.md](../MARTYPC.md)), with a few Compass hooks (refresh locked to the frame, the JMP SHORT quirk, INT/WAIT extras) |
| `crates/lib/marty_common` | Shared types used by marty_core |
| `crates/lib/grid_core` | The Compass: board, devices, drives, both CPU cores' glue, debugger, symbols, sessions |
| `crates/lib/i8087` | The 8087: the instruction-level core (`lib.rs`, `trans.rs`, `fptan.rs`) and the microcode core (`micro/`) |
| `crates/bin/compass` | The emulator: window, command line, build mode, ROM import, MCP server |
| `crates/bin/gridemu` | Headless boot to a screenshot, for tests |
| `crates/bin/gridfrag` | Times a code fragment in CPU clocks |

## Code tour

`grid_core` (`crates/lib/grid_core/src`):

| File | What it does |
|---|---|
| `machine.rs` | `GridMachine`: CPU, 8087, board, models (`GridModel`), the per-model timing setup, save/load state, fast/exact core switch |
| `board.rs` | `GridBoard`: memory map, I/O decode, the display RAM contention, ROM sockets, the split board's registers |
| `screen.rs` | `DisplayTiming` and the beam-accurate picture |
| `timers.rs` | 80130 and 8254 timers, interrupt controller variants |
| `keyboard.rs`, `upi41.rs` | The 8741 keyboard controller on an MCS-48 core (also the modem's 8749 in 8048 mode) |
| `gpib.rs` | TMS9914, the DMA window, the protocol-level drives |
| `box2101.rs`, `i8291.rs`, `s1410.rs` | The low-level 2101: the box's 8088, its GPIB chip, the Xebec controller |
| `box2102.rs`, `i8051.rs` | The low-level 2102: the box's 8051 |
| `wd279x.rs` | WD2797 floppy controller (both boxes) |
| `bubble.rs` | 7220 bubble controller with 7110 modules |
| `serial.rs` | 8274: async and SDLC |
| `modem.rs` | The internal modem: 8255, the 8749's pins, the line model |
| `sound.rs`, `rtc.rs` | DACs and DTMF; MM58174 clock |
| `hostdir.rs` | GRiD volumes: the PC folder drive and the media manager's file browser |
| `fast86.rs` | The fast whole-instruction 8086 core |
| `debug.rs`, `marks.rs`, `oscalls.rs`, `symbols.rs` | Debugger and profiler, cycle counter marks, GRiD-OS call names, OMF-86 program symbols |
| `session.rs`, `fragment.rs` | Typing at the prompt and waiting for it (build mode); running code fragments |
| `roms.rs` | Which ROM file each model and device uses |
| `fpu.rs` | The 8087 on the 8086's ESC/WAIT hooks |

The window (`crates/bin/compass/src`):

- `gui/`: an egui/eframe (OpenGL) GUI: menus, setup page with profiles, ROMs and GPIB device
  cards (`setup.rs`), media manager (`media.rs`), floppy library (`library.rs`), debugger
  workspace and chips window (`debugger.rs`), 2101 front panel (`led.rs`), on-screen
  keyboard (`osk.rs`), Project window (`project.rs`), saved states with thumbnails
  (`states.rs`).
- `session.rs`: building a machine from a profile (`MachineConfig`). GPIB units are
  `session::GpibDevice` (address, on/off, level, own firmware); old profiles (`hd`, `flop`,
  `flop2101`, `folder`) turn into units through `MachineConfig::devices()`.
- `runner.rs`: real-time pacing, the speed setting, pasted text and keys. Keys go by printed
  letter: digits and punctuation from typed text become "exact" taps (`runner::EXACT`: pressed
  with the Compass key's own Shift, whatever the PC's modifiers do). `winkeys.rs` reads keys
  on Windows (GetAsyncKeyState, and a hot key for Alt+Esc).
- `build.rs` (`compass build`), `headless.rs` (`compass do`), `mcp.rs` (`compass mcp`),
  `romimport.rs` (`compass import-roms`, ROMs known by CRC-32), `imd.rs` (ImageDisk to .img),
  `host.rs` (serial, GRiDLink and modem connections over TCP or a COM port).
- `classic.rs`: the minifb window of 0.4 (`--classic`, and the fallback when OpenGL 2 is
  missing).

The debugger (`grid_core::debug`) checks code breakpoints before each instruction, gets port
and interrupt breakpoints reported by the board, and finds memory write watches by comparing
bytes after each instruction, so all of it works on both CPU cores; the per-instruction
checks run only while a breakpoint, watch or profile is set (`GridMachine::run_debug`). The
call stack follows the BP chain, checked against CALLs before each return address. The
GRiD-OS call log names calls through vectors 70h/71h/73h from the Compact*Calls libraries.
Program symbols come from the OMF-86 objects of a debug build (BLKDEF, PUBDEF, DEBSYM,
LINNUM, FIXUPP for masking) plus the Pascal/ASM-86 listings; each module is found in RAM by
its fixup-masked code bytes, since the bound .run keeps no symbols. Cycle counter marks
(`grid_core::marks`) are OUT to ports 0F0h/0F2h/0F4h, which a real Compass doesn't decode.

## Building

    cargo build --release -p compass

The toolchain version is pinned in `rust-toolchain.toml`. Linux needs `pkg-config` and
`libasound2-dev` to build, and `libasound2`, `libxkbcommon-x11-0` and OpenGL to run the window. Windows
builds natively with MSVC, or from Linux with `rustup target add x86_64-pc-windows-gnu`,
mingw-w64, and `cargo build --release -p compass --target x86_64-pc-windows-gnu`.

Plain `cargo build --release` builds only the default member; name the crate (`-p compass`,
`-p gridemu`, `-p gridfrag`) to get its binary.

## Tests

    cargo test --release -p grid_core -p i8087 -p compass

The plain run needs no ROMs or disks: tests that need one skip themselves when it isn't
given. With files you supply, more of them run:

| Test | Needs | What it checks |
|---|---|---|
| `grid_core --test lle2101` | `HD2101_ROM=300205-05.bin` | the low-level 2101 against a host driven like GRiD's driver |
| `grid_core --test lle2102` | `FD2102_ROM=300237-02.bin` | the low-level 2102 the same way |
| `grid_core --test modem` | `MODEM_ROM=16020-007.bin` | calls both ways at 300 and 1200 baud, pulse dialing |
| `grid_core --test fast86_sst` | `SST_DIR=` the SingleStepTests 8086 v1 folder | the fast core against the suite (`SST_ONLY=XX` limits the files, `SST_SHOW=n` prints failures) |
| `i8087` micro tests, `bench_real` | `I8087_MICROCODE=8087mc.txt` | the microcode core against the instruction-level core and the real 8087 Bench reports |
| `grid_core` `symbols::real_objects` | `GRID_TEST_SYMS=` a debug build's folder | OMF-86 symbol reading |
| `grid_core` `hostdir::real_images` | `GRID_TEST_HD`, `GRID_TEST_FLOPPY` | the GRiD file browser on real images |

Ignored tests boot a whole Compass (run with `-- --ignored --nocapture`). They take
`ROMS=dir` (a ROM folder), `OUT=dir` (screenshots), `MODEL=` and `SECS=` (how long to run):

| Test | Also needs | What it does |
|---|---|---|
| `hd_boot` | `HD_IMG=` | boots a hard disk in a 2101; `boot_from_2101_floppy` with `FLOP_IMG=` |
| `floppy_boot` | `FLOP_IMG=` | boots a floppy in a 2102 |
| `bubble_boot` | `BUBBLE_IMG=` | boots a 1101 from a bubble image (`BLOG` logs the 7220) |
| `gridlink` | `HD_IMG=` with CableTest~Run~ | runs GRiD's CableTest against a test primary (`P2P`, `EXACT`, `CMD`, `SLOG`) |
| `modem_boot` | `HD_IMG=` with GRiDTerm | types `KEYS` steps (`BOOT_S`, `STEP_S`) and checks a call at 1200 baud |

Example:

    HD_IMG=disk.img ROMS=roms OUT=out MODEL=1139 \
      cargo test --release -p grid_core --test gridlink -- --ignored --nocapture

CI (`.github/workflows/compass.yml`) runs the plain tests on Windows and Linux, builds the
Windows zip and the Linux tarball (on Ubuntu 22.04, for glibc 2.35 and up), and publishes a
release with both for a `v*` tag with that version's CHANGELOG notes.

## Docs

`dist/windows/README.txt` is the user guide that ships in the zip. After editing it, run
`python3 util/gen_docs.py` to make `docs/user-guide.md` from it (the script's docstring
describes the text layout it expects). `CHANGELOG.md` is edited by hand.

## Environment variables

Read by the emulator, the headless tools and the tests. Most exist to try timing models
against real measurements; none is needed for normal use.

**Machine and ROMs**

| Variable | What it does |
|---|---|
| `GRID_MODEL` | default model for headless runs and the MCP server (1129) |
| `GRID_KBD_ROM` | 8741 firmware (gridemu) |
| `GRID_MODEM` | `off` = no modem MCU even with its firmware there |
| `I8087_MICROCODE` | the 8087 microcode ROM file |
| `FPU_CORE` | `table` = the instruction-level 8087 even with the ROM |
| `GRID_E0000_DMA` | split board also answers the 1101's DMA window (gridemu; the GUI sets it when the 1101 PROM stands in) |
| `GRID_DEV_DISK` | development disk for the MCP server's `grid_build` |
| `COMPASS_DATA_DIR` | where profiles, saved states, build states and imported ROMs go (default: next to the program if it can write there, else `$XDG_DATA_HOME/compass-emu` / `%APPDATA%\Compass-Emu`) |
| `GRID_ROOT`, `GRID_EMU` | where the MCP server finds an external `tools/gridbuild.py`, and the exe it tells that script to use |

**Timing models**

| Variable | What it does |
|---|---|
| `GRID_REFRESH`, `GRID_REFRESH_WAIT`, `GRID_REFRESH_FRAME` | DRAM refresh steal: period in clocks (0 = off), wait clocks, and the frame it locks to (0 = free running) |
| `JMP_EXTRA` | the extra clock before a JMP SHORT suspends prefetch (default 1; 0 = plain 8086) |
| `INT_EXTRA`, `WAIT_EXTRA` | extra clocks on INT/IRET and WAIT over MartyPC's model |
| `FPU_EXTRA`, `FPU_WAITMIN` | instruction-level 8087: extra busy clocks, minimum WAIT |
| `GRID_NO_CONTENTION`, `GRID_CONTENTION` | headless: display RAM contention off / on |
| `DISP_CONTENTION`, `DISP_FETCH_LEN`, `DISP_FETCH_PERIOD`, `DISP_FETCH_OFFSET`, `DISP_CPU_LEN`, `DISP_CPU_WRITE_LEN`, `DISP_LINE_CLOCKS`, `DISP_FRAME_LINES`, `DISP_ALL_LINES` | gridfrag: the display timing model (states of 15 MHz, clocks, lines) |
| `RAM_EXTRA_WAIT` | gridfrag: extra wait states on every RAM access |
| `RTC_UPDATE_CLK` | clocks the RTC's digits read 0Fh after each tenth |
| Drive timing | `GRID_2101*`, `GRID_2102*`, `BOX_SLOW`, `S1410_*`, `HD_NOMECH`, `FD_*`: see [drives.md](drives.md#environment-variables) |

**Logs and traces**

| Variable | What it does |
|---|---|
| `GRID_TRACE=file` | clock-by-clock CPU trace (any tool) |
| `GRID_DEBUG` | log the prompt detection of build mode |
| `GPIBLOG`, `GPIBREG`, `GRID_GPIB_TRACE`, `GPIB_STALL` | GPIB logs and the DMA stall test (see [drives.md](drives.md#environment-variables)) |
| `BOXLOG`, `FDCLOG` | drive box and WD2797 logs |
| `GRID_SERIAL_LOG`, `GRID_SERIAL_IN`, `GRID_SERIAL_OUT` | 8274 log; serial input from and output to files (gridemu) |
| `GRID_BUBBLE`, `GRID_BUBBLE_SAVE`, `GRID_BUBBLE_LOG` | bubble image, save it back, log the 7220 (gridemu) |
| `GRID_WAV` | record the sound to a file (gridemu) |
| `GRID_DUMP`, `GRID_KEYS` | save RAM at the end; typed key script (gridemu) |
| `FRAG_MODEL`, `FRAG_PNG`, `FRAG_DUMP` | gridfrag: model, screenshot, memory dump |

**8087 Bench tools** (`i8087` tests, see [8087-microcode.md](8087-microcode.md))

| Variable | What it does |
|---|---|
| `FB_OUT=file`, `FB_DIR`, `FB_SW0`, `FB_OLD` | `bench_dump`: write the emulator's bench report, the folder with the bench tables, the status word at start, and `FB_OLD` = run the instruction-level core instead |
| `FB_TRACE=id` | trace one bench test micro-instruction by micro-instruction |
| `FB_LIST=from-to` | `rom_list`: list micro-instructions of the ROM |
| `FB_FIT`, `FB_USES`, `FB_SENS` | `fit_constants`, `probe_uses`, `probe_sensitivity`: constant ROM fitting and probes |

**Test inputs**: `ROMS`, `OUT`, `MODEL`, `SECS`, `HD_IMG`, `FLOP_IMG`, `BUBBLE_IMG`, `KEYS`,
`BOOT_S`, `STEP_S`, `EXACT`, `P2P`, `CMD`, `SLOG`, `BLOG`, `HD2101_ROM`, `FD2102_ROM`,
`MODEM_ROM`, `SST_DIR`, `SST_ONLY`, `SST_SHOW`, `GRID_TEST_SYMS`, `GRID_TEST_HD`,
`GRID_TEST_FLOPPY` (see Tests above).
