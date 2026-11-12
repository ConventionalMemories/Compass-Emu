# Compass-Emu user guide

The same text ships as README.txt in the Windows zip.

## Setup

Put the compass-emu folder inside your GRiD-MAME folder, next to roms, disks and
tools. Then double-click "Start Compass-Emu 1101.bat" or "Start Compass-Emu 1139.bat".
The first time each one unpacks the ROMs from ..\roms\grid1101.zip (or grid1139.zip)
and ..\roms\grid_keyboard.zip into compass-emu\roms, and makes
..\disks\Compass-Emu 1101 disk.img (or 1139 disk.img): a copy of the merged disk
switched to the 11xx or 113x build with gridmodel.py. Your MAME disk is never changed.

## The window

The emulator now has menus (Machine, Media, View, Debug, Help), a toolbar and a
status bar with the speed and a light for each drive.
- Started from a .bat (or with machine options on the command line) the machine
powers on right away. Started on its own (double-click compass.exe) it opens
the machine setup page: pick a profile or make one (model, ROM folder, hard
disk, floppies, PC folder, bubble, ROM sockets, serial port, sound), then
Power on. Profiles are kept in compass-profiles.json next to compass.exe.
"Save as shortcut" writes a .bat that starts that machine. Run a .bat once
first so the ROMs are unpacked into compass-emu\roms.
- Machine: power off (back to setup), restart, pause, turbo, the fast CPU core,
quick save / quick load (kept in memory, disks included), save and load a
state file (a state file holds no disks: keep them as they were).
- Media: the media manager. Swap or take out floppies while the machine runs
(a changed disk is written back first), write changed disks back now,
and a GRiD file browser for disk images: hard disk volumes, subjects and
files with sizes and dates. Export files to the PC from any disk; import,
delete and add subjects on image files that are not in use (with the
machine running, use the PC folder drive for that). "Make..." creates a
blank 360K floppy image.
- View: fill the window or whole-pixel scaling, screen colour, the demo tools
overlay, the status bar.
- Debug: the debugger workspace (see Demo tools) and a Chips window
(8259 interrupt controller, 80130 timers, GPIB drives with a traffic trace,
8274 serial, keyboard controller).
Keys go to the Compass while no text field has the focus; click the screen
to give them back after typing in the debugger console.
If the PC has no OpenGL 2 (some virtual machines and very old drivers), the
plain window of 0.4 opens instead; --classic asks for it on purpose.

## Using it

The PC keys map to the Compass keys by position. Alt = CODE, Ctrl = CTRL.
Quick taps are held at least 60 ms so the keyboard chip always sees them.
The Compass keyboard takes at most 2 letter keys at once (3 freeze it, as on
the real machine), so release keys when typing fast.
F12 = turbo on/off (runs as fast as it can, about 10x), F11 = screenshot
(compass-001.png ...). F8, F9 and F10 are the demo tools (see below). Close the window to quit; disk changes are saved to the
image file then (start with --readonly to never save).
Booting to the Executive takes about 2.5 minutes at real speed; press F12 to
skip through it.

## Command line

```
compass.exe [--model 1101|1109|1121|1129|1131|1139] [--hd disk.img] [--flop floppy.img] [--flop2101 floppy.img]
            [--folder DIR] [--rom0 chip.bin .. --rom3 chip.bin]
            [--bubble bubble.img] [--serial tcp:PORT | connect:[HOST:]PORT | COMn] [--mute]
            [--prom-even F --prom-odd F | --prom F] [--kbd F] [--readonly]
            [--scale 1|2|4] [--overlay] [--fast] [--classic] [--symbols DIR] [--no-catch]
--model    1101 (default) and 1109 (512K) have the original board; 1121 (256K),
           1129 (512K), 1131 (256K) and 1139 (512K) the Compass II split board;
           1131 and 1139 the 512 x 256 screen. 1131/1139 need the 113x build of
           GRiD-OS (the 1139 .bat makes that disk), the others the 11xx build.
--prom     boot PROM as one 16K file (even/odd bytes interleaved), e.g. a dump
           from your own machine.
--folder   a folder on the PC as the GRiD "Floppy Disk" (see PC folder below).
--rom0 .. --rom3  a chip in application ROM socket 0-3 (Compass II models;
           see Application ROMs below).
--bubble   384K bubble image; made blank if the file doesn't exist (format it
           with Initialize Media). Saved on exit like the disks.
--serial   the Compass serial port: tcp:2323 listens on 127.0.0.1:2323 (connect a
           terminal such as PuTTY in "Raw" mode), or a real PC
           port like COM3 (the bit rate follows what the GRiD program sets).
--mute     no sound.
--fast     start with the fast CPU core (whole instructions, approximate timing).
--classic  the plain window of 0.4 (no menus); --scale only applies there.
--symbols  program symbols for the debugger: a debug build's project folder.
--no-catch crash catcher off (see Program symbols and the crash catcher).
The start .bat uses ..\disks\Compass-Emu bubble.img and serial on tcp:2323.
Boot PROM defaults, under MAME's names in roms: the model's own pair if it is there
(1129even.ccprom.bin ...), else the nearest: the 1101's for the 1109, the
1129's for the 1121, 1139even/odd.ccprom.bin for the 1131. The 1129 and builds need
the 1129 PROM: put grid1129.zip (MAME set) in GRiD-MAME\roms, or its two files in
compass-emu\roms. Without it the 1101 PROM stands in (slower boot).
The 1101 boots the later rev 4 PROM (1101even/odd.rev4.bin, in MAME's grid1101.zip)
when it is there, else rev 3.
Keyboard: roms\300067-01.bin (split board models use roms\300001-02.bin when present).
2102 floppy: roms\300237-02.bin (or 300237-01.bin), the 2102's own firmware. Not in
the MAME sets: copy it in by hand. With it the 2102 runs low-level (see New in 0.7.0);
without it the protocol model answers as before.
2101 hard disk: roms\300205-05.bin (20 MB box) and/or roms\300205-06.bin (10 MB
box), the 2101's own firmware. With it the 2101 runs low-level (see CHANGELOG
0.8.0); without it the protocol model answers as before.
```

## Build mode (replaces MAME for build.bat)

Copy tools\gridbuild.py from this folder over GRiD-MAME\tools\gridbuild.py
(and tools\debug.bat next to build.bat, see Program symbols).
build.bat then compiles on Compass-Emu, a GRiD Compass 1129 by default (512K,
11xx build of GRiD-OS), instead of MAME: same results (NAME.lst, NAME.run,
screen.png, output\). It unpacks the ROMs from ..\roms the first time.
set GRID_MODEL=1139 (or another model) before build.bat to build on another one.
Without compass-emu\compass.exe it uses MAME as before.
Builds run on a fast CPU core (whole instructions, approximate timing): same
results, about 5x quicker. set GRID_EXACT=1 for the cycle-exact core.
Build on a 512K model: on the 1101, 1121 and 1131 the compiler runs out of RAM.
Headless on its own:  compass.exe do --hd disk.img --shot screen.png "Do build"
(boots, types the command at the => prompt, waits for the prompt to come back).

## Claude Code hook

compass.exe mcp is an MCP server for Claude Code. Add it once with
claude mcp add compass -- C:\path\to\GRiD-MAME\compass-emu\compass.exe mcp
Then Claude can build programs (grid_build), boot a Compass from a disk image
(grid_boot), type on it (grid_type), look at the screen (grid_screen, grid_run),
read memory (grid_read_memory), save the disk (grid_save_disk) and time 8086 code
in CPU clocks (grid_fragment). It finds the GRiD-MAME folder as the folder above
compass.exe (or set GRID_ROOT).

## PC folder

The .bat files start the emulator with the folder "PC Folder" (inside
compass-emu, made if missing) as a GRiD drive. In GRiD-OS it shows as the
volume 'PC Folder' on the Floppy Disk device. Files in the folder are files on
the drive, and sub-folders are subjects (one level deep, as on GRiD).
The folder is read when the emulator starts. When you close the window, every
file GRiD made or changed is written to the folder, and every file erased on
GRiD is deleted from it. Changes made on the PC while the emulator runs are
seen only at the next start.
Names: a PC file like "hello.pas~Text~" keeps its GRiD kind. A file without
a kind, like "hello.pas", shows up as "hello.pas~Text~" with CR LF line
ends, and goes back to "hello.pas". Characters Windows can't have in a file
name (such as / in some GRiD names) are written as %2F and so on.
The drive holds up to 32 MB. With --readonly nothing is written back.

## Application ROMs

The Compass II models (1121 to 1139) have 4 sockets for application ROMs.
Give each chip with --rom0 to --rom3: a dump of 32, 64 or 128 KB, such as the
"Socket n" files Rom Toolbox saves. GRiD-OS finds them as on the real
machine: the programs show on the device "Read Only Memory" (subject
Programs). With a bootable ROM (CCOS) in a socket the PROM starts GRiD-OS
from that ROM, even with a hard disk (the PROM decides that), and it needs no
disk at all.
"Start Compass-Emu 1139.bat" puts roms\Compass_Socket_0.bin to
Compass_Socket_3.bin in the sockets when those files are there.

## Demo tools

Cycle counter: put marks in your program with OUT to ports a real Compass
ignores (so they can stay in a release):
```
MOV AL,1
OUT 0F0H,AL      ; start section 1 (AL = section number 0-255)
...              ; code to time
OUT 0F2H,AL      ; end section 1
OUT 0F4H,AL      ; frame mark: "this frame is drawn"
```
The emulator keeps count, last, min, average and max CPU clocks for each
section. The frame mark tells how far into the frame (counted from the
vertical sync tick, IR3) you got, on which display line the beam was, and
how many frames went by without a mark (missed frames).
F10 shows the table on top of the screen, with a blue tick at the left edge
on the line where the last frame mark came. F9 clears the numbers. The
title bar shows section and frame use, and on exit the full report goes
to compass-marks.txt. --overlay starts with F10 on.

Debugger: Debug > Debugger workspace (or the Debugger button) shows
registers and the 8087 stack, disassembly (click the circle in front of a
line for a breakpoint), breakpoints and watches, the cycle counter table
with a frame bar, the profiler, a memory view and a console. While paused
a blue line on the screen shows where the beam is. "Run to next frame"
stops at the next vertical sync tick. The console takes the same commands
as before (h = help), and so does the console window. F8 pauses and goes on.
```
b 2D00:0123     stop when the CPU gets there (or a linear address)
bio F0          stop on IN/OUT to a port
bi 3            stop on hardware interrupt IR3 (the vertical sync tick)
bw 2000:0400 2  stop when those bytes change
s 10            step 10 instructions, shows the clocks used
r / m ADDR / u  registers, memory dump, disassembly
bl / bc / g     list, clear breakpoints, go on
prof 60         profile the next 60 frames: clocks per routine and
                per instruction, printed when done
marks           the cycle counter report
```

## Program symbols and the crash catcher

Build with debug.bat instead of build.bat (or tools\gridbuild.py debug NAME):
the same program (identical code), and the compilers' symbols are kept in
the project folder (NAME.pas.obj, NAME.asm.obj, NAME.map). Copy debug.bat
from compass-emu\tools next to build.bat once (and the new gridbuild.py
over GRiD-MAME\tools\gridbuild.py). Then Debug > Load program
symbols and pick the project folder (or --symbols projects\NAME, or the
Debugging card of the machine setup page, which remembers it). Compass-Emu
finds the program in memory by its code whenever it runs, so:
- the disassembly shows procedure names, assembler labels and the Pascal
source lines, and calls show the name they go to (OSWRITE, MOVEBYTES...);
- breakpoints can be set by name: b SprEnd, b TlCore.FreeRegion, or a
source line b TLCORE:120 (the listing's statement number). A named
breakpoint is armed whenever the program is in memory, also after you
start it again;
- the call stack (k, and in the Call stack panel) follows the BP chain:
each frame with its procedure and source line;
- the profiler groups clocks by procedure and shows the source line of
the hottest instructions.
The crash catcher is on unless you untick it (Debug menu, debugger panel,
setup page, or --no-catch). It stops the machine with a report (what went
wrong, where, the call stack and the last instructions run) on:
- a GRiD-OS error: an error code turned into its message (OsDecodeException,
used by the Executive and Common code to show errors), with the message
text, stopping where the program gets it back;
- a divide error (DIV/IDIV by zero or a result too big);
- INT 3 in the program (GRiD-OS turns every 8087 WAIT into INT 3; those
are told apart, so a CC byte in your code works as a breakpoint);
- a wild jump: running through empty memory (00 00 or FF FF), with where
it came from.
These work without symbols too, with addresses instead of names. More
console commands:
```
sym FOLDER       load symbols (sym = status, sym off = unload)
names [TEXT]     the program's names and where they are now
k                call stack
hist [N]         the last N instructions run
catch on|off     crash catcher (catch errors|div|int3|wild on|off)
os on|off|N      log of GRiD-OS calls, each with its caller
```
The Claude Code hook has the same commands, and grid_build has debug:true.

Timing is cycle-exact against a real 1101 to about 2% on average; a few
cases (some short jumps) are still up to 14% off, being measured.

## Two Compasses on a null-modem cable

Start "Start Compass-Emu 1101.bat", then "Start Compass-Emu 1101 link.bat".
The second machine connects its serial port to the first one's (TCP port
2323 on this PC) and has its own disk copy, bubble and "PC Folder B".
Programs that talk over the serial port (GRiDTerm, a two-player game) then
see each other as if a null-modem cable joined them. For two PCs, start the
first with --serial tcp:0.0.0.0:2323 and the second with
--serial connect:OTHERPC:2323. Turbo (F12) on one side only makes it run
ahead of the other, so leave it off while linked.
