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

The emulator now has menus (Machine, Edit, Media, Project, View, Debug, Help), a toolbar and a
status bar with the speed and a light for each drive.
- Started from a .bat (or with machine options on the command line) the machine
powers on right away. Started on its own (double-click compass.exe) it opens
the machine setup page: pick a profile or make one (model, ROM folder, GPIB
devices, bubble, ROM sockets, serial port, sound), then Power on (the button
beside each profile, or double-click the profile). Profiles are kept in compass-profiles.json next to compass.exe.
"Save as shortcut" writes a .bat that starts that machine. Run a .bat once
first so the ROMs are unpacked into compass-emu\roms.
- Machine: power off (back to setup), restart, pause, turbo, the fast CPU core
and the 8087 core (both switch while the machine runs), quick save / quick
load (kept in memory, disks included), Save state and Saved states... (states
with a picture of the screen, kept per profile in saved-states\ next to
compass.exe), save and load a state file (a state holds no disks: keep them
as they were).
- Media: one entry per floppy drive (Insert, Recent, Take out) and the media
manager. Swap or take out floppies while the machine runs
(a changed disk is written back first), write changed disks back now,
and a GRiD file browser for disk images: hard disk volumes, subjects and
files with sizes and dates. Export files to the PC from any disk; import,
delete and add subjects on image files that are not in use (with the
machine running, use the PC folder drive for that). "Make..." creates a
blank 360K floppy image.
- View: fill the window or whole-pixel scaling, screen colour, the demo tools
overlay, the toolbar, the status bar, full screen (or double-click the
Compass screen; double-click again to leave it). The window size and these
settings are kept for the next start (in compass-profiles.json).
- Drop files on the window: a floppy image (.img or .IMD) goes into the 2102
floppy drive (hold Shift for the 2101 floppy drive), a .state file loads
that state. On the setup page a floppy image, a hard disk image (anything
over 2 MB), a bubble image (name with "bubble") or a folder (PC folder
drive) goes into the selected profile.
- Recent disks: Media > (a floppy drive) > Recent, and a Recent menu beside
the hard disk and floppy fields on the setup page.
- .IMD floppy files (ImageDisk) work wherever a floppy image does: the
emulator uses the .img with the same name beside it, and makes that .img
from the .IMD the first time. Changes go to the .img; the .IMD stays as it was.
- Edit: Paste text (types the clipboard's text on the Compass keyboard) and
Copy screen (the Compass screen to the clipboard as a picture).
- Project: build a GRiD program from a project folder on a development disk
and run it, inside compass.exe (see Build in the window below).
- View also has the EL panel look (glow, pixel gaps, afterglow), the on-screen
Compass keyboard and the 2101 front panel (its LED display and drive lights;
the small display in the status bar opens it too).
- Toolbar: the speed slider (10 % to 400 % of a real Compass, double-click for
100 %), Paste, the on-screen keyboard and Build and run.
- Setup page, ROMs: every ROM the profile uses, the file found and a field to
pick another; Import ROMs... copies GRiD ROMs from a folder or zip, known by
checksum, under the names the emulator looks for.
- Setup page, GPIB devices: one card per unit on the Compass's GPIB bus (2101
Disc System, 2102 Floppy, PC folder drive), "+ Add device" for more. Each
card has an on/off box (off: left out of the machine for now, settings
kept), its GPIB address with GRiD-OS's name for it (4 Hard Disk + 5 Floppy
Disk, 12 Extra Hard Disk + 13 Extra Floppy Disk, 6 Portable Floppy...), Low
level (the unit's own firmware) or Protocol model, an optional firmware file
of its own, and its disks, each with a Write protect box. Two 2101 boxes can
each have their own level. A floppy drive with no disk is still there, so
GRiD-OS lists it and sees a disk inserted later. The status bar names each
drive by its GRiD-OS name and shows LL or P beside it (WP when protected).
- Write protect, per drive (setup page, Media menu, media manager, also while
running): a protected floppy acts like one with its write-protect tab on,
GRiD can read it but its writes fail (error 106); a protected hard disk or
PC folder takes GRiD's writes but they are never saved. The bubble has its
own Write protect box. --readonly protects everything.
- Help > Keyboard map: which PC key is which Compass key.
- Debug: the debugger workspace (see Demo tools) and a Chips window
(8259 interrupt controller, 80130 timers, GPIB drives with a traffic trace,
8274 serial, keyboard controller).
Keys go to the Compass while no text field has the focus; click the screen
to give them back after typing in the debugger console.
If the PC has no OpenGL 2 (some virtual machines and very old drivers), the
plain window of 0.4 opens instead; --classic asks for it on purpose.

## Using it

The PC keys go by the letter printed on them, so any layout (AZERTY,
QWERTZ...) types what its keys say: letters by their letter, digits and
punctuation by the character they type (AZERTY's & key types 1 only with
Shift, as printed). View > PC keys > By position (US layout) puts every key
back where it sits on a US keyboard. Alt = CODE, Ctrl = CTRL; with Ctrl or
Alt held, digits and punctuation go by position.
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
compass.exe [--model 1101|1109|1121|1129|1131|1137|1139] [--hd disk.img] [--flop floppy.img] [--flop2101 floppy.img]
            [--folder DIR] [--rom0 chip.bin .. --rom3 chip.bin]
            [--bubble bubble.img] [--serial tcp:PORT | connect:[HOST:]PORT | COMn]
            [--gridlink tcp:PORT | connect:[HOST:]PORT] [--gridlink-p2p] [--mute]
            [--prom-even F --prom-odd F | --prom F] [--kbd F] [--readonly]
            [--scale 1|2|4] [--overlay] [--fast] [--fpu-micro] [--fpu-rom F]
            [--gpib UNIT|addr=N|hd=F|disk=F|folder=DIR|rom=F|protocol|off]...
            [--classic] [--symbols DIR] [--no-catch]
--model    1101 (default) and 1109 (512K) have the original board; 1121 (256K),
           1129 (512K), 1131 (256K) and 1139 (512K) the Compass II split board;
           1131 and 1139 the 512 x 256 screen. 1131/1139 need the 113x build of
           GRiD-OS (the 1139 .bat makes that disk), the others the 11xx build.
--prom     boot PROM as one 16K file (even/odd bytes interleaved), e.g. a dump
           from your own machine.
--folder   a folder on the PC as a GRiD drive (see PC folder below): the "Extra
           Floppy Disk" (13) next to a 2101, else the "Floppy Disk" (5);
           --folder-addr 5|6|13 picks the address.
--gpib     one GPIB unit, as on the setup page (repeat for more; instead of
           --hd, --flop, --flop2101 and --folder): 2101, 2102 or folder, then
           addr=N (2101: 4 or 12, its floppy is at N+1), hd=, disk=, folder=,
           rom= (its own firmware), protocol, protect-hd, protect-disk (write
           protect), off. --bubble-protect: never save the bubble. Example: --gpib
           "2101|addr=4|hd=disk.img" --gpib "2101|addr=12|hd=old.img|protocol".
--rom0 .. --rom3  a chip in application ROM socket 0-3 (Compass II models;
           see Application ROMs below).
--bubble   384K bubble image; made blank if the file doesn't exist (format it
           with Initialize Media). Saved on exit like the disks.
--serial   the Compass serial port: tcp:2323 listens on 127.0.0.1:2323 (connect a
           terminal such as PuTTY in "Raw" mode), or a real PC
           port like COM3 (the bit rate follows what the GRiD program sets).
--gridlink the same port with a GRiDLink cable: SDLC frames over TCP. tcp:2400
           waits on 127.0.0.1:2400 for a GRiDLink server program, connect:HOST:PORT
           connects to one. See GRiDLink below.
--gridlink-p2p  point-to-point GRiDLink cable (RTS looped to CTS); default multidrop.
--mute     no sound.
--fast     start with the fast CPU core (whole instructions, approximate timing).
--fpu-micro  run the 8087 on its own microcode ROM, a file you supply (see
           docs/8087-microcode.md): 8087mc.txt or 8087mc.bin in the ROM folder,
           else I8087_MICROCODE or roms\. Without the ROM the instruction-level
           8087 runs. Also on the setup page (Computer, 8087).
--fpu-rom F  the same, with that microcode ROM file.
--rom-2102 F, --rom-2101 F (20 MB box), --rom-2101-10 F (10 MB box)
           drive firmware files other than the ones in the ROM folder.
--2101-protocol, --2102-protocol  the protocol model for that drive even when
           its firmware is there.
compass build PROJECTFOLDER --disk DEVDISK.img [--roms DIR] [--debug] [--exact]
           build a GRiD program (as Project > Build).
compass import-roms SOURCE [ROMFOLDER]  copy GRiD ROMs found in a folder or zip
           (known by checksum) into the ROM folder under the emulator's names.
--classic  the plain window of 0.4 (no menus); --scale only applies there.
--symbols  program symbols for the debugger: a debug build's project folder.
--no-catch crash catcher off (see Program symbols and the crash catcher).
The start .bat uses ..\disks\Compass-Emu bubble.img and serial on tcp:2323.
Boot PROM defaults, under MAME's names in roms: the model's own pair if it is there
(1129even.ccprom.bin ...), else the PROM it shares: the 1101's for the 1109,
the 1129's for the 1121, 1139even/odd.ccprom.bin for the 1131. The 1129 and builds need
the 1129 PROM: put grid1129.zip (MAME set) in GRiD-MAME\roms, or its two files in
compass-emu\roms. Without it the 1101 PROM stands in (slower boot).
The 1101 boots the later rev 4 PROM (1101even/odd.rev4.bin, in MAME's grid1101.zip)
when it is there, else rev 3.
Keyboard: roms\300067-01.bin (split board models use roms\300001-02.bin when present).
2102 floppy: roms\300237-02.bin (or 300237-01.bin), the 2102's own firmware. Not in
the MAME sets: copy it in by hand. With it the 2102 runs low-level (see New in 0.7.0);
without it the protocol model answers as before.
2101 hard disk: roms\300205-05.bin (20 MB box) and/or roms\300205-06.bin (10 MB
box), the 2101's own firmware. With it the 2101 runs low-level (see New in
0.8.0); without it the protocol model answers as before.
```

## Build in the window

Project > Project window: pick the project folder (NAME.pas, NAME.asm and the
other sources, as for build.bat) and a development disk (empty = the machine's
hard disk; the disk needs Pascal~Run~ and DevelopmentExecutive~Run~). Build,
Build and run, Debug build. The build runs inside compass.exe (no Python) on a
hidden Compass: an 1129, or an 1139 when the disk has the 113x build of
GRiD-OS, with the fast core, while your machine keeps running. The first build
of a disk boots it (about a minute) and keeps that state in states\ next to
compass.exe; later builds take a few seconds. Results go to the project folder:
NAME.run, NAME.lst (and NAME.asm.lst), screen.png, output\ for other new files;
a debug build adds the .obj and .map files for Load program symbols. FATAL ERROR
9010 or 9011 means the development disk is full.
Build and run copies NAME~Run~ into the running machine's PC folder drive: files
added or changed in the PC folder show up while the machine runs (once the GRiD
has left the drive alone for two seconds). The same from a command prompt:
compass build PROJECTFOLDER --disk DEVDISK.img [--roms DIR] [--debug] [--exact]

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
GRiD is deleted from it. Files added or changed on the PC while the emulator
runs show up on the drive within a few seconds (once GRiD has left the drive
alone for two seconds; a file GRiD changed too keeps GRiD's version). Files
deleted on the PC stay on the drive until the next start. The plain window
(--classic) reads the folder only at the start.
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

## GRiDLink

GRiDLink is the GRiD Server network: 250 kbit/s synchronous (SDLC) on the
serial port, with the server polling each Compass. With --gridlink the
emulated 8274 sends every frame the Compass makes to the TCP connection and
hands it every frame that comes back (docs/gridlink.md has the format and
a short Python example). The other end has to play the GRiD Server; none is
included yet. Without a server the Compass's GRiDLink~Device~ just waits
for the server's poll. The machine setup page has the same two settings.
Keep the emulator at real speed while linked: GRiDLink's timeouts run in
emulated time, and turbo shortens them for the program at the other end.

## Two Compasses on a null-modem cable

Start "Start Compass-Emu 1101.bat", then "Start Compass-Emu 1101 link.bat".
The second machine connects its serial port to the first one's (TCP port
2323 on this PC) and has its own disk copy, bubble and "PC Folder B".
Programs that talk over the serial port (GRiDTerm, a two-player game) then
see each other as if a null-modem cable joined them. For two PCs, start the
first with --serial tcp:0.0.0.0:2323 and the second with
--serial connect:OTHERPC:2323. Turbo (F12) on one side only makes it run
ahead of the other, so leave it off while linked.
