Compass-Emu 0.11.0 - GRiD Compass emulator (1101, 1109, 1121, 1129, 1131, 1137, 1139)
====================================================================================

Setup
  Put the compass-emu folder inside your GRiD-MAME folder, next to roms, disks and
  tools. Then double-click "Start Compass-Emu 1101.bat" or "Start Compass-Emu 1139.bat".
  The first time each one unpacks the ROMs from ..\roms\grid1101.zip (or grid1139.zip)
  and ..\roms\grid_keyboard.zip into compass-emu\roms, and makes
  ..\disks\Compass-Emu 1101 disk.img (or 1139 disk.img): a copy of the merged disk
  switched to the 11xx or 113x build with gridmodel.py. Your MAME disk is never changed.

The window
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

Using it
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

Command line
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
  --bubble   bubble image: raw 7220/7110 contents, 128 KB per module (384K on a
             1101); made blank if the file doesn't exist (format it with
             Initialize Media). Saved on exit like the disks.
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

Build in the window
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

Build mode (replaces MAME for build.bat)
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

Claude Code hook
  compass.exe mcp is an MCP server for Claude Code. Add it once with
    claude mcp add compass -- C:\path\to\GRiD-MAME\compass-emu\compass.exe mcp
  Then Claude can build programs (grid_build), boot a Compass from a disk image
  (grid_boot), type on it (grid_type), look at the screen (grid_screen, grid_run),
  read memory (grid_read_memory), save the disk (grid_save_disk) and time 8086 code
  in CPU clocks (grid_fragment). It finds the GRiD-MAME folder as the folder above
  compass.exe (or set GRID_ROOT).

PC folder
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

Application ROMs
  The Compass II models (1121 to 1139) have 4 sockets for application ROMs.
  Give each chip with --rom0 to --rom3: a dump of 32, 64 or 128 KB, such as the
  "Socket n" files Rom Toolbox saves. GRiD-OS finds them as on the real
  machine: the programs show on the device "Read Only Memory" (subject
  Programs). With a bootable ROM (CCOS) in a socket the PROM starts GRiD-OS
  from that ROM, even with a hard disk (the PROM decides that), and it needs no
  disk at all.
  "Start Compass-Emu 1139.bat" puts roms\Compass_Socket_0.bin to
  Compass_Socket_3.bin in the sockets when those files are there.

Demo tools
  Cycle counter: put marks in your program with OUT to ports a real Compass
  ignores (so they can stay in a release):
      MOV AL,1
      OUT 0F0H,AL      ; start section 1 (AL = section number 0-255)
      ...              ; code to time
      OUT 0F2H,AL      ; end section 1
      OUT 0F4H,AL      ; frame mark: "this frame is drawn"
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

Program symbols and the crash catcher
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
      sym FOLDER       load symbols (sym = status, sym off = unload)
      names [TEXT]     the program's names and where they are now
      k                call stack
      hist [N]         the last N instructions run
      catch on|off     crash catcher (catch errors|div|int3|wild on|off)
      os on|off|N      log of GRiD-OS calls, each with its caller
  The Claude Code hook has the same commands, and grid_build has debug:true.

  Timing is cycle-exact against a real 1101 to about 2% on average; a few
  cases (some short jumps) are still up to 14% off, being measured.

GRiDLink
  GRiDLink is the GRiD Server network: 250 kbit/s synchronous (SDLC) on the
  serial port, with the server polling each Compass. With --gridlink the
  emulated 8274 sends every frame the Compass makes to the TCP connection and
  hands it every frame that comes back (docs/gridlink.md has the format and
  a short Python example). The other end has to play the GRiD Server; none is
  included yet. Without a server the Compass's GRiDLink~Device~ just waits
  for the server's poll. The machine setup page has the same two settings.
  Keep the emulator at real speed while linked: GRiDLink's timeouts run in
  emulated time, and turbo shortens them for the program at the other end.

Two Compasses on a null-modem cable
  Start "Start Compass-Emu 1101.bat", then "Start Compass-Emu 1101 link.bat".
  The second machine connects its serial port to the first one's (TCP port
  2323 on this PC) and has its own disk copy, bubble and "PC Folder B".
  Programs that talk over the serial port (GRiDTerm, a two-player game) then
  see each other as if a null-modem cable joined them. For two PCs, start the
  first with --serial tcp:0.0.0.0:2323 and the second with
  --serial connect:OTHERPC:2323. Turbo (F12) on one side only makes it run
  ahead of the other, so leave it off while linked.

New in 0.11.0
  GUI for everyday use:
  - Full screen: View > Full screen, or double-click the Compass screen
    (again to leave it). Menus, toolbar and status bar hide while it is on.
  - The window size and maximized state, scaling, screen colour, toolbar and
    status bar are kept from one start to the next (in compass-profiles.json).
    View > Toolbar hides the toolbar.
  - Drag and drop: drop a floppy image on the running machine to insert it in
    the 2102 floppy drive (Shift: the 2101 floppy drive), or a .state file to
    load it. On the setup page dropped floppy, hard disk and bubble images and
    folders go into the selected profile.
  - Recent disks: Media > Recent into the 2102 / 2101 drive, and Recent menus
    beside the disk fields on the setup page (the last 8 floppies and hard disks).
  - ImageDisk .IMD floppies: used through the .img beside them, made from the
    .IMD when it is missing (GRiD 2102 and IBM sector numbering).
  - 8087 core per machine: Computer > 8087 on the setup page (and --fpu-micro,
    --fpu-rom) runs the 8087 on its own microcode ROM, which you supply; the
    setup page says which ROM file it found (docs/8087-microcode.md).
  - Help > Keyboard map lists every Compass key and its PC key, and the
    emulator keys.

  Machine setup, drives and keys:
  - ROMs card on the setup page, per profile: each ROM the machine uses (boot PROM,
    keyboard 8741, 2102 firmware, 2101 firmware for the 20 MB and the 10 MB box,
    8087 microcode) with the file found and a field to pick another file. "Import
    ROMs..." takes a folder (zips inside it too), knows the GRiD ROMs by checksum
    whatever their file names, and copies them into the profile's ROM folder under
    the names the emulator looks for (a 16K boot PROM file is split into its even
    and odd halves). Also `compass import-roms SOURCE [ROMFOLDER]`.
  - GPIB devices on the setup page: a card per unit (2101 Disc System, 2102 Floppy,
    PC folder drive) with an on/off box, its GPIB address and GRiD-OS name (changeable),
    Low level (the unit's own firmware runs) or Protocol model, its own firmware file if
    wanted, and its disks. Two 2101 boxes (4 + 5 and 12 + 13), each at its own level.
    A floppy drive with no disk is emulated too, so GRiD-OS lists it and a disk
    inserted later is seen. The PC folder drive is a unit of its own (13 Extra Floppy
    Disk next to a 2101), so the 2101 keeps its floppy drive and its firmware. The
    status bar shows each drive's GRiD-OS name and LL or P; hover for the details.
    Also --gpib, --folder-addr, --2101-protocol, --2102-protocol, --rom-2101,
    --rom-2101-10 and --rom-2102. Older profiles turn into units by themselves.
  - Power on sits beside each profile (double-click a profile to power it on) and at
    the top of the page.
  - 2101 front panel: the box's 4-character red LED display and its drive lights (View >
    2101 front panel, or click the small display in the status bar), one per 2101 box.
    It shows what the 2101 firmware writes to the display, so only with the low-level
    2101.
  - Keys on Windows: a held key no longer goes on after it is let go (Windows key
    repeat queued extra presses), and CODE+Esc / CODE+Space reach the Compass
    instead of switching windows (the keyboard hook now runs on its own thread, so
    Windows no longer drops it when a frame is slow, the window itself refuses
    Alt+Esc, and while the window has focus Alt+Esc is registered as its hot key).
    Help > Keyboard map shows whether these guards are on, how often each caught
    Alt+Esc, and what the hook saw of the last Esc.
  - Write protect per drive instead of one switch for all disks (see The window).
  - Media manager: opens the floppies and hard disks GRiD-OS reads, also unnamed
    volumes (a blank volume name) and boot floppies (boot code in page 0), finds
    the root directory even when it is not right after the bitmap, and says so
    plainly when a disk has none. "Open disk image..." takes .IMD floppies too.
  - Bubble card on the setup page: bubble on or off, the image (raw, 128 KB per
    7110 module, in address order; size and modules shown), New image size
    (128/256/384/512 KB) with "Make blank...", and its own Write protect box.
  - Media > Floppy library: every .img and .IMD in a folder (and sub-folders)
    with its GRiD volume name, file count, size and date, a Find box, and the
    drive to put it in; double-click a floppy (or Insert) to put it in that drive.
  - Keys by the letter printed on the PC key (AZERTY, QWERTZ...), see Using it; View >
    PC keys > By position (US layout) for the old way.

  Build and run in the window:
  - Project > Project window: pick a project folder (NAME.pas, NAME.asm, ...) and a
    development disk (the machine's hard disk by default), then Build, Build and run
    or Debug build. compass.exe builds by itself, no Python: on a hidden Compass
    (1129, or 1139 for a 113x disk) with the fast core while your machine keeps
    running. The first build of a disk boots it (about a minute) and keeps that
    state, later builds take a few seconds. Results go to the project folder like
    build.bat: NAME.run, NAME.lst, screen.png, output\. Also `compass build PROJECT
    --disk DISK [--debug]`. The toolbar has a Build and run button.
  - Build and run copies NAME~Run~ into the running machine's PC folder drive.
  - PC folder drive: files added or changed on the PC show up while the machine
    runs (checked each second, once the GRiD has left the drive alone for two
    seconds). A file the GRiD changed too is left as the GRiD has it.

  Extras:
  - Edit > Paste text types the PC clipboard's text on the Compass keyboard (one key
    at a time, about 14 a second; characters with no Compass key are left out).
    Edit > Copy screen puts the Compass screen on the clipboard as a picture.
  - Speed slider (toolbar and Machine menu): 10 % to 400 % of a real Compass;
    double-click it for 100 %.
  - On-screen Compass keyboard (View, or Keyboard on the toolbar), laid out and
    coloured like the real one: click a key to type it, hold the mouse button to
    hold it; SHIFT, CTRL and CODE stay on for the next key.
  - Saved states with pictures: Machine > Save state keeps the machine and a picture
    of its screen with the profile; Machine > Saved states... shows them as
    thumbnails to load (double-click) or delete. A state holds no disks.
  - The 8087 core switches while the machine runs (Machine > 8087 on its microcode
    ROM), like the fast CPU core.
  - View > EL panel look: glow around lit pixels, dark gaps between pixels (at 3x and
    more) and a short afterglow.

  8087:
  - 8087 on its own microcode (optional): with `FPU_CORE=micro` and a microcode ROM file
    you supply (`I8087_MICROCODE` or `roms/8087mc.txt`, see docs/8087-microcode.md) the
    arithmetic, loads and stores, FSQRT, FPREM, FSCALE, BCD and the five transcendentals
    run from the chip's microcode ROM instead of the instruction-level model. Of 141
    transcendental and square root results dumped from a real 1101, 134 come out bit for
    bit and the other 7 are one unit off.
  - The microcode 8087 fitted to 8087 Bench on a real 1101 and 1137 (2181 tests): of 1588
    real result lines 34 still differ (were 1441), and the busy time now comes from the
    micro-instructions that ran (mean error about 2.5 % over 2068 timed tests, was 40 % with the
    tables). Both 8087 cores: FINIT keeps the condition bits, the B bit shows a pending
    unmasked exception, the IR bit is derived, the control word has 13 bits, reserved
    opcodes alias their neighbours, FSTENV's pointer is the ESC byte after a segment
    prefix. The test `bench_real` holds the real reports.
  - The instruction-level 8087 was corrected wherever the microcode showed it wrong (FSQRT
    exactly rounded, FPREM's loop and quotient bits, FBSTP rounding, NaN and denormal
    loads, FXTRACT and FSCALE special cases; list in docs/8087-microcode.md).

New in 0.10.0
  Each model now has the hardware its real boards carry (from photos of 1101,
  1121, 1137 and 1139 motherboards):
  - New model 1137 (`--model 1137`, Kurt's machine): 113x board with 512K and the
    large screen, the 1139 PROM (as on the real 1137) and the 300001-02 keyboard
    firmware dumped from it. No modem: the 8255 at DFEC0 is missing, so
    Modem~Device~ finds no modem as on the real machine. Its sound DACs and DTMF
    chip are on the motherboard; how the DTMF chip is driven is not known, so
    for now a write-only latch at DFEC2 takes the 8255's port B bits (to check
    with SoundTest on the real 1137).
  - Timers per board (new `timers.rs`). 110x and 112x, 80130: timer 0 counts at
    2.5 MHz (CPU clock / 2), timer 1 counts timer 0's output pulses, timer 2 at
    5 MHz, no read-back, all running from power-on. 113x, discrete 8254: all three
    at 5 MHz, read-back command with status bytes, a counter stands at FFFF until
    a count is written, timer 1 counts only while timer 0 has a count. Replaying
    GRiDProbe's T test gives the real machines' patterns: about twice the count
    per read on the 113x, status B4 (mode 2) and B6 (mode 3), FFFF before
    loading, read-back ignored on the 1101. Before, every model had all timers at
    5 MHz and timer 1 frozen.
  - The 113x board ignores A0 at the 8254: a byte read at 09/0B/0D also steps
    that counter's LSB/MSB flip-flop. With it, GRiDBench 2.2's port rows (u20-u27)
    and timer rows (p30-p52) match the real 1137 (low byte of timers 1 and 2 on
    every row, FFFF on unloaded counters); 0.9.1 gave counting timers there.
  - Interrupt controller per board: the 80130's quirks (non-specific EOI
    ignored, ports 4/6 not decoded, 0Eh reads FFh) now apply to the 110x and
    112x; the 113x's 8259A + 8254 take the non-specific EOI (assumed), mirror the
    PIC at 4/6 and leave 0Eh open, as measured on the real 1137. The 112x used
    the 113x behaviour before.
  - Keyboard firmware per model: 300001-02 on the 1137, 300067-01 on the others
    (each stands in for the other when missing). The 1139 used 300001-02 before.
  - The 8274 already counted from the 5 MHz CPU clock; the photos confirm it (no
    4.032 MHz crystal on any motherboard).
  - The 113x RAM/display timing, refresh and INT fits (from the real 1137) now
    hang on `GridModel::board_113x()`, and stay as they were. GRiDBench 2.2 gives
    the same timing results as 0.9.1 on the 1101 and 1137; only the timer and
    port rows changed.
  - gridfrag: FRAG_DUMP=address,length prints memory after the run.

New in 0.9.1
  Fitted to GRiDProbe 0.0.12-0.0.13 and the Disk Toolkit 0.4.1 box probe on a
  real 1101 and 1137:
  - GRiD-OS's INT 3 stand-in for each 8087 WAIT costs what it costs on the real
    machines: 55.8 clocks over a WAIT on the 1101 (real 55.7) and 57.8 on the
    113x (real 58.7). A WAIT alone takes 3.7 clocks with the 8087 idle (real
    3.75). FNOP, FLD/FSTP, FDIV and short and long FMUL are within about a clock
    of the real chip; a long FMUL that rounds up takes 5 clocks more, an FADD
    whose smaller operand shifts out completely 5 less, as on the real chip.
  - Frame geometry from GRiDProbe's beam finder: the 1101 draws 307 clocks a
    line and 268 lines (60.8 Hz; real 60.8), with IR3 rising on line 3 for 241
    lines; the 113x 316 x 263 (60.2 Hz). Display contention refitted to these
    frames (GRiDBench 2.2 gap rows: 1.2% rms on the 1101, 1.3% on the 113x).
  - 2101 hard disk: interleave 5, read from the real drive's sector IDs. Seek
    times measured inside a real box (Seek, then Test Ready until ready). A Seek
    command is buffered: it answers after 1.5 ms plus 20 us a cylinder, and Test
    Ready reports "not ready" until the heads are in. A read's or write's own seek
    waits that long again plus 8 ms before its first sector, which matches the
    Disk Toolkit per-read seek costs and GRiDBench 2.1b (random pages 132.8 ms,
    real 130.3; steps of 1/16 118.4, real 122.5). Recalibrate steps back at 3 ms
    a cylinder. Commands take 0.44 ms to decode, as on the real box.
  - Bubble: writes end with status 42 (the parity bit, data fine), as on the
    real 1101; GRiD-OS ignores the bit. Read FSA Status gives FF 20 20 20 20 20,
    with 28 for an MBM written since the last Software Reset. Abort takes 0.14 to
    0.16 ms. An Abort between two pages of a read leaves the controller stuck as
    on the real chip: later multi-page reads stop after one page with status 2C
    until a Software Reset (GRiD-OS's error recovery does one). GRiDProbe B7 now
    matches the real report line for line.
  - 2102 floppy: no change needed. The low-level 2102 already matches GRiDProbe's
    floppy test (same page again 181 ms, real 185; power-up after 12 s idle 1.9 s,
    real 1.9).

New in 0.9.0
  - GRiDLink: the 8274 serial chip's synchronous SDLC mode, the one the GRiD
    Server network uses. --gridlink tcp:2400 (or connect:HOST:PORT) carries
    the Compass's frames over TCP, one message per frame (format in
    docs/gridlink.md), for a GRiDLink server program on the PC. No GRiD
    Server is built in: a Compass can only be a GRiDLink client, so two
    emulators can't link to each other this way.
  - Tested with the real GRiDLink client code: CableTest from the GRiD
    Server disks ("Programs for GRiD Compass") signs on, takes its station
    address and passes its 520-byte test exchanges with no errors, against
    a small test script playing the server.
  - Multidrop cable (default) or point-to-point (--gridlink-p2p, RTS looped
    to CTS), as GRiDLink~Device~ checks at start. The serial port select
    latches (DFE10-DFE16 on the Compass II board, DFE00-DFE06 on the 1101)
    are kept and shown to the link.
  - Fix: the 8274's interrupt vector in 8086 mode (WR2A = 10h) now carries
    the cause in bits 0-2, as on the chip; it was in bits 2-4.

New in 0.8.1
  Fitted again to the real 1101 and 1137 runs (GRiDBench 2.2, GRiDProbe 0.0.11):
  - 8087 FPATAN and F2XM1 take as long as the real chip for each input: their
    time follows the chip's digit loop (steps from the first non-zero digit, and
    the digits themselves), within about 6 clocks on 36 and 34 inputs (FPATAN
    was a flat 712 clocks for most inputs, the real chip 380 to 843). FYL2X near
    1 is about 50 clocks longer, a long FMUL whose significands multiply to 2 or
    more about 11 shorter, and a store that has to denormalize its result (FST
    of a number too small for a double) 49 longer, as on the real chip.
  - 8087 loads like the real chip: FLD of a denormal double or single gives an
    unnormal (GRiDBench h33 reads back 3C010093445B87315800 as on the real 1101,
    not the normalized 3BF9...), and FLD m80 of an unnormal keeps it (it used to
    turn into a NaN). FSTP m80, FSAVE and FXAM see the chip's bit pattern.
  - The 113x models (1131, 1139) use the real 1137's memory timing: a RAM read or
    write takes about one clock more than on the 1101, the display slots are
    shorter and the refresh steal is 2 clocks. Gap rows against the real 1137:
    1.2 % rms (was 2.0 %).
  - Bubble: a page's 64 bytes come into the FIFO over about 5 ms (400 clocks a
    byte), so a read gives two FIFO interrupts a page (35 + 29 bytes) as on the
    real 1101. Read FSA Status, Reset FIFO, Software Reset, Abort and MBM Purge
    are shorter; Initialize takes 6 to 9 ms (the real 6.0 to 8.6), but stays
    short (0.5 ms) after an MBM Purge until the next transfer. On a write the
    FIFO takes data only from 1.2 ms before the first page passes, as the real
    7220 (no bytes before the first interrupt).
  - Real-time clock: for a moment after each tenth (one 32.768 kHz period, about
    30 us; the real length isn't measured, RTC_UPDATE_CLK sets it in CPU clocks)
    the MM58174 is updating and its digits read 0Fh, as on the chip, so the
    PROM's retry on 0Fh runs as on hardware.
  - ROMs: the 1109 boots the 1101's PROM, the 1121 the 1129's and the 1131 the
    1139's, as on the real machines (they share PROMs), without a stand-in warning.
  - Results, GRiDBench 2.2 against the real machines: 1101 CPU/bus rows 1.07 %
    rms, 8087 rows 2.5 % (was 5.7 %); 1139 against the real 1137 CPU/bus 1.49 %
    (was 1.96 %). GRiDBench's bubble rows (GRiDBench 2.1b on a real 1101), ms,
    emulated / real: same page 100.2 / 99.9, sequential 126.0 / 127.7, random
    112.6 / 123.6, first/last 98.1 / 100.9.

New in 0.8.0
  - The 2101 hard disk box runs low-level when roms holds its firmware
    (300205-05.bin 20 MB, 300205-06.bin 10 MB): the box's 8088 runs the real
    ROM; the Xebec controller is modelled at command level, timed after a real
    2101. Its 4-character display shows in the status bar.
  - The 2101 floppy goes through the firmware too; an empty slot gives error
    128 as on the real box.
  - Disk speed fitted to GRiDBench on a real 1101 + 2101 (within about 10%).
  - GRID_2101=protocol uses the old protocol-level model instead.

New in 0.7.0
  - The 2102 portable floppy runs low-level when roms holds its firmware
    (300237-02.bin or 300237-01.bin): the box's 8051 runs the real ROM, with a
    WD2797 controller and a 5.25" drive, wired from the 2102 schematic. Reads,
    writes, get status, set status and formatting (whole disk and one track) all go
    through GRiD's own firmware, at its speed: the first access after 10 s idle
    powers the drive up again (about 1.8 s), as the ROM does.
  - The box is switched on 4 s before the Compass, so it has finished its self test
    when the boot PROM looks for it. GRID_2102_EARLY=0 switches both on together.
  - GRID_2102=protocol uses the old protocol-level model instead.

New in 0.6.2
  Fitted to the GRiDProbe runs on a real 1101 and a real 1137:
  - Bubble memory turns like the real one: the loops only move while a command
    runs and stop where it ends, so a read waits for its page to come round.
    The next page costs 12.5 ms, two on 20 ms, half the bubble away 46 ms, the
    same page again a full turn (87 ms); streaming 7.2 ms a page, bytes reach
    the FIFO one at a time (2 bubble interrupts a page, as on the real 1101).
    Initialize, Read FSA Status, Reset FIFO, Software Reset, Abort and Purge
    take their real times; Abort during a transfer drops the FIFO.
  - The interrupt controller ignores a non-specific EOI (OUT 0,20H), as the
    real 1101 does; GRiD-OS uses specific EOIs.
  - Refresh tick (IR3) stays high most of the frame like the real machines
    (1101: low ~8000 clocks a frame; 113x: low only ~1500). 1101 frame 270
    clocks x 306 lines (60.5 Hz).
  - 1131/1139: DFE00 = 1 darkens the screen until it is set back to 0, as on
    the real 1137.
  - Display memory slowdown on every line, blank ones too (the real machines
    show the same slowdown all through the frame), refitted: the full GRiDBench
    2.2 report now matches the real 1101 within 1.04% on average (was 1.14%)
    and the real 1137 within 1.97% (was 2.83%).

New in 0.6.1
  Timing fitted to GRiDBench 2.2 runs on a real 1101 and a real 1137:
  - 1101: the branch, prefetch and REP rows of the gap test now match within
    1.2% on average (4% at most; JMP SHORT chains were 14% too fast). A chain
    of JMP SHORTs runs like a chain of taken conditional jumps, as on the real
    machines. The frame is 271 clocks x 306 lines (60.3 Hz).
  - 1131/1139: frame 281 clocks x 297 lines (59.9 Hz), refresh tick at the top
    of the picture like the 1101, display memory slowdown fitted to the 1137
    (gap rows within 3.3% on average).
  - Compass II split board: empty memory reads the address on the bus (not FF),
    the boot PROM also shows at F8000, ports 4/6 answer like the interrupt
    controller at 0/2, port 0E is not decoded, and the bubble controller repeats
    every 4 bytes up to DFE8F, as on the real 1137.
  - GRID_TRACE=file writes a clock-by-clock CPU trace (for timing work).

New in 0.6.0
  - Program symbols for the debugger from a debug build (debug.bat,
    gridbuild.py debug): names and Pascal source lines in the disassembly,
    breakpoints by name or source line, call stack, names in the profiler.
  - Crash catcher: stops on GRiD-OS errors (with the message text), divide
    errors, INT 3 and wild jumps, with a report.
  - GRiD-OS call log (every OS, CP and Common call with its caller).
  - --symbols DIR and --no-catch; the machine setup page has a Debugging card.

New in 0.5.0
  - A GUI: menus, toolbar, status bar with drive lights, machine setup page
    with saved profiles, media manager with floppy swapping and a GRiD file
    browser (export, import, delete, subjects, blank floppies), debugger
    workspace, chips window, quick save / load, state files, screen colour
    and scaling. The 0.4 window stays as --classic and as the fallback when
    OpenGL 2 is missing.
  - --fast starts with the fast CPU core.

New in 0.4.0
  - Demo tools (see above): cycle counter marks, frame meter overlay (F10),
    debugger console with breakpoints and stepping (F8), profiler.
  - The Claude Code hook has grid_debug with the same debugger, profiler and
    cycle counter.

New in 0.3.12
  - Application ROM sockets on the Compass II models (see Application ROMs).
    Checked with the four ROMs of a real 1137: GRiD-OS boots from the ROMs
    with no disk, and their programs copy out.
  - A disk transfer that hangs now ends the way it does on the real machine:
    the keyboard chip's watchdog frees the CPU after about 4 seconds and
    GRiD-OS reports error 451 (before, the emulator gave up on its own).

New in 0.3.11
  - Two emulators can be linked by a virtual null-modem cable (see above):
    --serial connect:PORT, and the new "Start Compass-Emu 1101 link.bat".
  - Fixed: the .bat files of 0.3.9 and 0.3.10 didn't attach the PC folder
    (the option landed on a line of its own).

New in 0.3.10
  - Floppies run at real speed: 300 rpm, interleave, seeks and the 2102's slow
    controller, fitted to a GRiDBench run on a real 2102 (with a Gotek): page
    reads within about 10% except "steps of 1/16" (23% slow).
  - With a hard disk, the 2101's floppy slot is there too (empty, error 128
    like a real 2101), so programs that look at the Floppy Disk no longer hang.
  - The 2102 reads one page per command, as its ROM does.

New in 0.3.9
  - A folder on the PC as a GRiD drive (see PC folder above): copy sources,
    programs and data in and out with Explorer, no image tools needed.
  - A GRiD "set status" write no longer overwrites page 0 of a disk.

New in 0.3.8
  Timing checked against a GRiDBench 2.1 run on a real 1101 with a 2101:
  - Plain 8086 code and memory access within about 2% of the real machine on
    average (memory refresh and screen fetches now steal bus time as measured).
  - Fixed: a disk read could copy a byte or two too many when the transfer
    ended, so GRiD-OS reset the disk bus (half a second). Reading 8 pages at
    once now takes 65 ms instead of 1.1 s.
  - Hard disk seeks follow the real ST-225: random page reads 130 ms (real
    130.3), steps of 1/16 of the disk 121 ms (122.5), the same page 19 ms (19.0).

New in 0.3.7
  - Disk reads and writes are much faster and closer to a real 2101: the
    interrupt controller now raises an interrupt as soon as its line is
    unmasked. Before, every disk command waited for the next screen refresh,
    so each page took 3 refreshes (50 ms). Boot and file work speed up too.
  - The 2101 hard disk now behaves like the real ST-225 box at real speed:
    rotation, interleave 5, seeks, the box's 6-page read-ahead cache and
    writes that finish in the background. Sequential page reads take about
    23 ms, as the Disk Toolkit measured on the real box. Build mode (fast
    core) skips this.
  - FYL2X and FYL2XP1 take as long as on the real 8087 for each operand.

New in 0.3.6
  Timing and details measured by GRiDBench 2.0 on a real 1101:
  - 8087 math runs at real speed. GRiD-OS turns every 8087 WAIT into an INT 3 call;
    the old timing counted that cost twice, so math was 1.5 to 2 times too slow.
  - FPTAN gives the real chip's numbers (within 3 bits in the last place, was 8),
    and its time depends on the angle as on the chip. FPATAN, F2XM1, FYL2X and FSQRT
    now set the precision flag and treat out-of-range inputs as the real 8087 does.
  - Display: 274 clocks a line, 306 lines (59.6 Hz), the refresh tick at the top of
    the picture.
  - Empty memory and unused ports read what the real bus gives (not FF), the boot
    PROM also shows at F0000, ports 00-7F mirror the interrupt controller, the clock
    chip is slower to read, 80130 timers 0 and 2 count and timer 1 doesn't.
  - The 1101 uses the later rev 4 boot PROM, the one in the real 1101 it was measured on.

New in 0.3.5
  Sound no longer stops after a few notes in games using GRiDSound (BomberGRiD,
  GRiDroids): the serial chip's interrupt now drops between samples as on the
  real chip, so the sample clock keeps running.
  Keyboard (Windows): keys are read straight from Windows' key state only, so
  no key stays stuck down any more (Shift stuck on, or the keyboard chip
  stopping because it thought 3 keys were held).
  Alt+Esc and Alt+Space no longer make Windows act while the emulator has focus;
  they go to the Compass as CODE-Esc / CODE-Space. Alt+Tab and Alt+F4 still work.

New in 0.3.4
  8087 FPTAN works like the real chip: the CORDIC + rational method found in its
  microcode, giving the chip's own Y/X pair (X is about 3, not 1) and a run time
  that depends on the angle. Last bits still to be checked on real hardware.

New in 0.3.3
  Fast CPU core for builds: hello.pas 3.3 s a build (was 15 s), GRiDTiles 48 s
  (was 6 minutes). The programs it builds are byte for byte the same as on the
  cycle-exact core. The window version and timing tests still use the exact core.
  Claude hook: grid_boot takes fast: true for quick file work.

New in 0.3.2
  Faster builds: the first build boots the GRiD once and saves it at the prompt
  (compass-emu\states); later builds start from there. hello.pas: 15 s a build.
  The saved state is made again by itself when your merged disk or compass.exe changes.

New in 0.3.1
  The real 1129 boot PROM is used for the 1129 and 1121 (builds ~27 s instead of 38 s).

New in 0.3.0
  Build mode (compass.exe do, and gridbuild.py using it) and the Claude Code hook
  (compass.exe mcp).

New in 0.2.0
  More models: 1109, 1121, 1129, 1131, 1139 (--model). The Compass II split board
  memory map, 512K RAM, and the 1131/1139 512 x 256 screen (turned on by the 113x
  build of GRiD-OS; before that the 320 x 240 picture is shown centred). Large
  screen timing is estimated from GRiDBench on a real 1137.

New in 0.1.0
  Bubble memory, serial port and sound (sound DAC, volume DAC, DTMF tones).
  Timings are first estimates until measured on real hardware.

New in 0.0.5
  Display timing: the CPU slows down when it shares RAM with the display, and the
  screen shows what the beam saw, so racing the beam and tearing look as on a real
  1101. Settings are first estimates until measured on real hardware.

New in 0.0.4
  8087 math coprocessor: real 80-bit math, timing matched to a real 1101
  (GRiDBench REAL tests within about 2%).

Not there yet
  The modem itself, GRiDLink (synchronous serial), the test ROM socket.

Source: https://github.com/ConventionalMemories/compass-emu
By Conventional Memories (C.M._), MIT license. Built on MartyPC by Daniel Balsom (MIT).
