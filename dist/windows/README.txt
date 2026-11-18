Compass-Emu 0.15.0 - GRiD Compass emulator (1101, 1109, 1121, 1129, 1131, 1137, 1139)
====================================================================================

Setup
  Compass-Emu needs the GRiD ROMs and a GRiD-OS disk. Neither is included.

  - ROMs: the MAME ROM sets grid1101.zip (1101/1109), grid1139.zip (113x),
    optionally grid1129.zip (1121/1129), and grid_keyboard.zip, or dumps of your
    own machine. Optional extras: the 2102 floppy firmware (300237-02.bin), the 2101
    box firmware (300205-05.bin, 300205-06.bin), the internal modem's firmware
    (16020-007.bin) and the 8087 microcode (8087mc.txt). See ROM files below.
  - A GRiD-OS hard disk image (for the 2101) or boot floppy image (for the 2102).
    The 1101 to 1129 need the 11xx build of GRiD-OS, the 1131 to 1139 the 113x build.

  1. Unpack the zip anywhere and double-click compass.exe. The machine setup page
     opens.
  2. On the ROMs card press Import ROMs... and pick the folder that holds the MAME
     zips or your own dumps. Every Compass ROM in it (zips included) is known by
     its checksum and copied into the roms folder next to compass.exe, under the
     name the emulator looks for. From a command prompt:
         compass.exe import-roms FOLDER
  3. Pick a model and the disk images, then press Power on.

  "Save as shortcut" on the setup page writes a .bat that starts that machine
  straight away (on Linux a .sh script or a .desktop launcher).

  Where Compass-Emu keeps its own files (compass-profiles.json, saved-states,
  states and imported roms): next to compass.exe, as long as that folder can be
  written to. When it can't (Program Files, /usr/bin, an AppImage), they go to a
  folder of your own: %APPDATA%\Compass-Emu on Windows,
  ~/.local/share/compass-emu on Linux. The setup page shows the profile file's
  path at the bottom. To pick another folder, set COMPASS_DATA_DIR. ROMs are
  also still found in a roms folder next to the program.

Linux
  Download compass-emu-VERSION-linux-x86_64.tar.gz from the releases page and
  unpack it anywhere (in a file manager: right-click > Extract; or
  tar xzf compass-emu-*.tar.gz). Start the compass program inside: double-click
  it, or from a terminal:
      cd compass-emu-VERSION-linux-x86_64
      ./compass
  Everything in this guide works the same way; read compass for compass.exe and /
  for \ in paths.

  - It runs on 64-bit x86 Linux with glibc 2.35 or newer (Ubuntu 22.04, Debian
    12, Fedora 36, Mint 21 or newer). It needs the ALSA sound library and
    xkbcommon, which every desktop has; on a minimal system install them with
        sudo apt install libasound2 libxkbcommon-x11-0
    (Fedora: sudo dnf install alsa-lib libxkbcommon-x11). Sound through
    PulseAudio or PipeWire works through their ALSA plugin.
  - It is built for both X11 and Wayland desktops. The window uses OpenGL, so it
    needs a working graphics driver (Mesa is fine).
  - File dialogs (Insert floppy, Import ROMs...) use the desktop's portal
    (xdg-desktop-portal), or zenity when there is no portal. Without either, drop
    disk images on the window or type paths in the setup page.
  - Real serial ports are named /dev/ttyUSB0, /dev/ttyS0 and so on. To use them
    without root, add yourself to the port's group once and log in again:
        sudo usermod -aG dialout $USER
    (on Arch the group is uucp).
  - The desktop can take some keys before the emulator sees them (Super,
    Alt+Tab, Alt+F4, sometimes Alt+drag or Ctrl+Alt+arrows). The Compass Code key
    is on Alt, so turn off a desktop shortcut that gets in the way, or use the
    on-screen keyboard (View menu).
  - "Save as shortcut" writes a shell script (.sh) or, if the name ends in
    .desktop, a launcher for the desktop's menus (copy it to
    ~/.local/share/applications). Both start the program from where it is now,
    so make them again after moving it.

  Building it yourself: install Rust (https://rustup.rs) and the ALSA headers
  (sudo apt install build-essential pkg-config libasound2-dev), then in the
  source folder:
      cargo build --release -p compass
  The program is target/release/compass.

The window
  The emulator has menus (Machine, Edit, Media, Project, View, Debug, Help), a
  toolbar and a status bar with the speed and a light for each drive.

  - Setup page: started on its own (double-click compass.exe), the emulator opens
    the machine setup page. Pick a profile or make one (model, ROM folder, GPIB
    devices, bubble, ROM sockets, serial port, modem phone line, sound), then
    Power on (the button beside each profile, or double-click the profile).
    Profiles are kept in compass-profiles.json next to compass.exe (see Setup
    for when they go elsewhere). Started from a shortcut .bat, or with machine options on the command line, the machine
    powers on right away.
  - ROMs card: every ROM the profile uses, the file found and a field to pick
    another. Import ROMs... copies GRiD ROMs from a folder or zip.
  - GPIB devices: one card per unit on the Compass's GPIB bus (2101 Disc System,
    2102 Floppy, PC folder drive), "+ Add device" for more. Each card has:
    - an on/off box (off: left out of the machine for now, settings kept),
    - its GPIB address with GRiD-OS's name for it (4 Hard Disk + 5 Floppy Disk,
      12 Extra Hard Disk + 13 Extra Floppy Disk, 6 Portable Floppy),
    - Low level (the unit runs its own firmware) or Protocol model, and an
      optional firmware file of its own,
    - its disks, each with a Write protect box.
    Two 2101 boxes can each have their own level. A floppy drive with no disk is
    still there, so GRiD-OS lists it and sees a disk inserted later. The status bar
    names each drive by its GRiD-OS name and shows LL or P beside it (WP when
    write protected).
  - Machine menu: power off (back to setup), restart, pause, turbo, the fast CPU
    core and the 8087 core (both switch while the machine runs), quick save and
    quick load (kept in memory, disks included), Save state and Saved states...
    (states with a picture of the screen, kept per profile in saved-states\ next
    to compass.exe or in the data folder), and save or load a state file. A state file holds no disks,
    so keep them as they were.
  - Media menu: one entry per floppy drive (Insert, Recent, Take out), the floppy
    library (every floppy image in a folder with its GRiD volume name; double-click
    one to insert it) and
    the media manager. Swap or take out floppies while the machine runs (a changed
    disk is written back first) and write changed disks back now. The media
    manager is a GRiD file browser for disk images: hard disk volumes, subjects
    and files with sizes and dates. Export files to the PC from any disk; import,
    delete and add subjects on images that are not in use (while the machine
    runs, use the PC folder drive for that). "Make..." creates a blank 360K
    floppy image.
  - Edit menu: Paste text (types the clipboard's text on the Compass keyboard)
    and Copy screen (the Compass screen to the clipboard as a picture).
  - Project menu: build a GRiD program from a project folder and run it (see
    Build in the window).
  - View menu: fill the window or whole-pixel scaling, screen colour, the EL
    panel look (glow, pixel gaps, afterglow), the demo tools overlay, the
    toolbar, the status bar, the on-screen Compass keyboard, the 2101 front panel
    (its LED display and drive lights; the small display in the status bar opens
    it too) and full screen (or double-click the Compass screen; double-click
    again to leave it). The window size and these settings are kept for the
    next start.
  - Debug menu: the debugger workspace (see Demo tools) and the Chips window
    (interrupt controller, timers, GPIB drives with a traffic trace, 8274 serial,
    keyboard controller).
  - Help menu: Keyboard map (which PC key is which Compass key) and About.
  - Toolbar: the speed slider (10 % to 400 % of a real Compass, double-click for
    100 %), Paste, the on-screen keyboard and Build and run.
  - Drop files on the window: a floppy image (.img or .IMD) goes into the 2102
    floppy drive (hold Shift for the 2101 floppy drive), a .state file loads that
    state. On the setup page a floppy image, a hard disk image (anything over
    2 MB), a bubble image (a name with "bubble" in it) or a folder (PC folder
    drive) goes into the selected profile.
  - Recent disks: Media > (a floppy drive) > Recent, and a Recent menu beside the
    disk fields on the setup page.
  - Write protect, per drive (setup page, Media menu, media manager, also while
    running): a protected floppy acts like one with its write-protect tab on, so
    GRiD can read it but its writes fail (error 106). A protected hard disk or PC
    folder takes GRiD's writes, but they are never saved. The bubble has its own
    Write protect box. --readonly protects everything.

  Keys go to the Compass while no text field has the focus; click the screen to
  give them back after typing in the debugger console.
  If the PC has no OpenGL 2 (some virtual machines and very old drivers), the
  plain window of 0.4 opens instead; --classic asks for it on purpose.

Using it
  The PC keys go by the letter printed on them, so any layout (AZERTY,
  QWERTZ...) types what its keys say: letters by their letter, digits and
  punctuation by the character they type (AZERTY's & key types 1 only with
  Shift, as printed). View > PC keys > By position (US layout) puts every key
  back where it sits on a US keyboard. Alt = CODE, Ctrl = CTRL; with Ctrl or
  Alt held, digits and punctuation go by position. Help > Keyboard map shows
  the rest.

  Quick taps are held at least 60 ms so the keyboard chip always sees them. The
  Compass keyboard takes at most 2 letter keys at once (3 freeze it, as on the
  real machine), so release keys when typing fast.

  F12 = turbo on/off (as fast as the PC can, about 10x), F11 = screenshot
  (compass-001.png ...). F8, F9 and F10 are the demo tools (see below).

  Booting to the Executive takes about 2.5 minutes at real speed; press F12 to
  skip through it.

  Disk changes are written to the image files when you power off or close the
  window (start with --readonly to never save). If the emulator crashes before
  that, the changes since the last write-back are lost: Media > Write changed
  disks back now saves them on the way.

Command line
  compass.exe [--model 1101|1109|1121|1129|1131|1137|1139]
              [--hd disk.img] [--flop floppy.img] [--flop2101 floppy.img]
              [--folder DIR] [--folder-addr 5|6|13] [--rom0 chip.bin .. --rom3 chip.bin]
              [--gpib UNIT|addr=N|hd=F|disk=F|folder=DIR|rom=F|protocol|protect-hd|protect-disk|off]...
              [--bubble bubble.img] [--bubble-protect] [--readonly]
              [--serial tcp:PORT | connect:[HOST:]PORT | COMn | /dev/ttyX]
              [--gridlink tcp:PORT | connect:[HOST:]PORT] [--gridlink-p2p]
              [--modem-dial NUMBER=HOST:PORT]... [--modem-listen tcp:PORT]
              [--modem-answer-300] [--rom-modem F] [--no-modem] [--mute]
              [--prom-even F --prom-odd F | --prom F] [--kbd F]
              [--rom-2102 F] [--rom-2101 F] [--rom-2101-10 F] [--2101-protocol] [--2102-protocol]
              [--fast] [--fpu-rom F] [--fpu-table]
              [--symbols DIR] [--no-catch] [--overlay] [--classic] [--scale 1|2|4]
  compass.exe build PROJECTFOLDER --disk DEVDISK.img [--roms DIR] [--debug] [--exact]
  compass.exe import-roms FOLDER|ZIP [ROMFOLDER]
  compass.exe do [--model M] --hd disk.img [--shot F] [--max-seconds N] [--exact] "command"
  compass.exe mcp

  --model    1101 (default, 256K) and 1109 (512K): original board, 320 x 240.
             1121 (256K) and 1129 (512K): Compass II split board, 320 x 240.
             1131 (256K), 1137 and 1139 (512K): split board, 512 x 256.
             The 1137 has no internal modem. The 1131 to 1139 need the 113x build
             of GRiD-OS, the others the 11xx build.
  --hd       hard disk image in a 2101 at address 4.
  --flop     floppy image in a 2102 at address 6.
  --flop2101  floppy image in the 2101's floppy drive (address 5).
  --folder   a folder on the PC as a GRiD drive (see PC folder): the Extra Floppy
             Disk (13) next to a 2101, else the Floppy Disk (5).
  --folder-addr  the PC folder drive's address: 5, 6 or 13.
  --gpib     one GPIB unit, as on the setup page (repeat for more; instead of
             --hd, --flop, --flop2101 and --folder): 2101, 2102 or folder, then
             addr=N (2101: 4 or 12, its floppy is at N+1), hd=, disk=, folder=,
             rom= (its own firmware), protocol, protect-hd, protect-disk, off.
             Example:
               --gpib "2101|addr=4|hd=disk.img" --gpib "2101|addr=12|hd=old.img|protocol"
  --rom0 .. --rom3  a chip in application ROM socket 0-3 (Compass II models; see
             Application ROMs).
  --bubble   bubble memory image (see Bubble memory). Saved on exit like the disks.
  --bubble-protect  never save the bubble.
  --readonly  never write any disk, folder or bubble back.
  --serial   the Compass serial port: tcp:2323 listens on 127.0.0.1:2323 (connect a
             terminal such as PuTTY in "Raw" mode), connect:2323 or
             connect:HOST:2323 connects to another emulator (see Two Compasses on
             a null-modem cable), COM3 (Windows) or /dev/ttyUSB0 (Linux) uses a
             real PC port (the bit rate follows what the GRiD program sets).
  --gridlink  the same port with a GRiDLink cable: SDLC frames over TCP. tcp:2400
             waits on 127.0.0.1:2400 for a GRiDLink server program,
             connect:HOST:PORT connects to one. See GRiDLink.
  --gridlink-p2p  point-to-point GRiDLink cable (RTS looped to CTS); default
             multidrop.
  --modem-dial  internal modem phone book entry: NUMBER=HOST:PORT, the address a
             dialed number reaches (repeat it, or separate entries with commas;
             *=HOST:PORT for any other number). See The internal modem.
  --modem-listen  tcp:2400 (or tcp:0.0.0.0:2400 for other PCs): a TCP connection
             to that port rings the Compass's modem.
  --modem-answer-300  a caller the Compass answers connects at 300 baud (default
             1200).
  --rom-modem  the modem's 8749 firmware, if not roms\16020-007.bin.
  --no-modem  leave the modem's microcontroller out even with its firmware there.
  --mute     no sound.
  --prom     boot PROM as one 16K file (even and odd bytes interleaved), for
             example a dump of your own machine. --prom-even and --prom-odd take
             the two 8K halves.
  --kbd      keyboard controller firmware, if not the model's default.
  --rom-2102  2102 firmware, if not the one in the ROM folder. --rom-2101 (20 MB
             box) and --rom-2101-10 (10 MB box) do the same for the 2101.
  --2101-protocol  the protocol model for the 2101 even when its firmware is
             there. --2102-protocol the same for the 2102.
  --fast     start with the fast CPU core (whole instructions, approximate timing).
  --fpu-rom  the 8087 microcode ROM file, if not roms\8087mc.txt or 8087mc.bin.
  --fpu-table  keep the instruction-level 8087 even when the ROM file is there.
  --symbols  program symbols for the debugger: a debug build's project folder.
  --no-catch  crash catcher off (see Program symbols and the crash catcher).
  --overlay  start with the demo tools overlay (F10) on.
  --classic  the plain window of 0.4 (no menus); --scale only applies there.
  compass.exe build  build a GRiD program, as Project > Build (see Build in the
             window).
  compass.exe import-roms  copy the GRiD ROMs found in a folder or zip (known by
             checksum) into the ROM folder under the emulator's names.
  compass.exe do  boot headless, type a command at the => prompt, wait for the
             prompt to come back and save the screen (see Claude Code hook).
  compass.exe mcp  the MCP server for Claude Code (see Claude Code hook).

ROM files
  The ROM folder is roms next to compass.exe, else roms in the data folder (see
  Setup), or the one the profile names.
  Import ROMs... puts every file there under the right name. Which files each
  model and device uses:

  - Boot PROM: the model's own pair under MAME's names (1101even.rev4.bin /
    1101odd.rev4.bin, 1129even.ccprom.bin ..., 1139even.ccprom.bin ...), else the
    one it shares: the 1109 uses the 1101's, the 1121 the 1129's (or the 1101's),
    the 1131 and 1137 the 1139's. Without the 1129 PROM the 1121 and 1129 boot
    the 1101's (slower boot). The 1101 boots the later rev 4 PROM when it is
    there, else rev 3.
  - Keyboard controller: 300067-01.bin. The 1137 uses 300001-02.bin when it is
    there. Each stands in for the other when missing.
  - 2102 floppy (optional): 300237-02.bin or 300237-01.bin. With it the 2102 runs
    its own firmware; without it the protocol model answers.
  - 2101 Disc System (optional): 300205-05.bin (20 MB box) and/or 300205-06.bin
    (10 MB box). The 2101 runs its own firmware with a 20 MB (41,616 pages) or
    10 MB (20,808 pages) hard disk image; other sizes, or no firmware, get the
    protocol model.
  - Internal modem (optional): 16020-007.bin, the modem's 8749 firmware (2 KB).
    Without it the modem has no microcontroller and Modem~Device~ can't start.
  - 8087 microcode (optional): 8087mc.txt or 8087mc.bin (see The 8087 microcode
    ROM).

  The drive, modem and microcode files are not in the MAME sets: copy them in by
  hand, or let Import ROMs... find them (it knows them by checksum, the microcode
  by its name). docs/roms.md in the source lists every file with its checksum.

The 2101 and 2102 drives
  With their firmware in the ROM folder, the 2101 Disc System and the 2102
  Portable Floppy run low-level: the box's own processor (an 8088 in the 2101,
  an 8051 in the 2102) runs GRiD's firmware, so the drives behave and take as
  long as the real boxes. Like the real boxes they start a few seconds before
  the Compass. The low-level 2102 powers its drive down after 10 s without use,
  and the next access then takes about 2 s. The 2101's 4-character display shows
  in the status bar and on its front panel (View menu).
  Without the firmware, or with Protocol model picked, a model of GRiD's disk
  protocol with the real drives' timing answers instead.
  docs/drives.md in the source has the details.

Bubble memory
  The bubble memory board (7220 controller with 7110 bubbles, 384K on the 1101) is
  on the setup page's Bubble card: on/off, image file, size of a new image, Make
  blank and Write protect. The image holds the raw bubble contents, 128 KB per
  module. A missing file is made blank; format a blank one with GRiD-OS's
  Initialize Media. The bubble is saved on exit like the disks.

  Backups made by GRiD Bubble Fix (the "<name> Module N~Bin~" or "~Raw~" files)
  can be the image too: pick the file of any module and the other modules'
  files next to it are read with it, each at its module's place (missing ones
  stay blank). The backups are never written over: the GRiD's changes go to a
  raw image "<name> bubble.img" beside them, and that image is used from then on
  as long as it is newer than the backups.

Build in the window
  Project > Project window: pick the project folder (NAME.pas, NAME.asm and the
  other sources) and a development disk (empty = the machine's hard disk; the
  disk needs Pascal~Run~ and DevelopmentExecutive~Run~). Then Build, Build and
  run, or Debug build.

  The build runs inside compass.exe on a hidden Compass (an 1129, or an 1139 when
  the disk has the 113x build of GRiD-OS) on the fast core, while your machine
  keeps running. The first build of a disk boots it (about a minute) and keeps
  that state in states\ next to compass.exe (or in the data folder); later builds take a few seconds.
  Results go to the project folder: NAME.run, NAME.lst (and NAME.asm.lst),
  screen.png, and output\ for other new files. A debug build adds the .obj and
  .map files for Load program symbols. FATAL ERROR 9010 or 9011 means the
  development disk is full.

  Build and run copies NAME~Run~ into the running machine's PC folder drive:
  files added or changed in the PC folder show up while the machine runs (once
  GRiD has left the drive alone for two seconds). The same from a command prompt:
      compass.exe build PROJECTFOLDER --disk DEVDISK.img [--roms DIR] [--debug] [--exact]
  The 256K models (1101, 1121, 1131) can't run the compiler ("Main memory (RAM)
  is full"), which is why the hidden machine is always a 512K one.

Claude Code hook
  compass.exe mcp is an MCP server for Claude Code. Add it once with
      claude mcp add compass -- C:\path\to\compass.exe mcp
  Then Claude can build programs (grid_build, with disk = a development disk
  image, or set GRID_DEV_DISK), boot a Compass from a disk image (grid_boot), type
  on it (grid_type), look at the screen (grid_screen, grid_run), read memory
  (grid_read_memory), save the disk (grid_save_disk), use the debugger
  (grid_debug) and time 8086 code in CPU clocks (grid_fragment).
  Headless on its own:
      compass.exe do --hd disk.img --shot screen.png "Do build"
  boots, types the command at the => prompt and waits for the prompt to come back.
  docs/build-and-mcp.md in the source has the details.

PC folder
  Pick a folder on the PC on the setup page (PC folder), or start with --folder
  DIR, and it becomes a GRiD drive. In GRiD-OS it shows as a volume named after
  the folder. Files in the folder are files on the drive, and sub-folders are
  subjects (one level deep, as on GRiD).

  The folder is read when the emulator starts. When you power off or close the
  window, every file GRiD made or changed is written to the folder, and every
  file erased on GRiD is deleted from it. Files added or changed on the PC while
  the emulator runs show up on the drive within a few seconds (once GRiD has left
  the drive alone for two seconds; a file GRiD changed too keeps GRiD's version).
  Files deleted on the PC stay on the drive until the next start. The plain
  window (--classic) reads the folder only at the start.

  Names: a PC file like "hello.pas~Text~" keeps its GRiD kind. A file without a
  kind, like "hello.pas", shows up as "hello.pas~Text~" with CR LF line ends,
  and goes back to "hello.pas". Characters Windows can't have in a file name
  (such as / in some GRiD names) are written as %2F and so on.
  The drive holds up to 32 MB. With --readonly nothing is written back.

Application ROMs
  The Compass II models (1121 to 1139) have 4 sockets for application ROMs. Give
  each chip on the setup page or with --rom0 to --rom3: a dump of 32, 64 or
  128 KB, such as the "Socket n" files Rom Toolbox saves. GRiD-OS finds them as
  on the real machine: the programs show on the device "Read Only Memory"
  (subject Programs). With a bootable ROM (CCOS) in a socket the PROM starts
  GRiD-OS from that ROM, even with a hard disk (the PROM decides that), and it
  needs no disk at all.

The 8087 microcode ROM
  The 8087 can run on the real chip's own microcode. Its results then match a
  real 8087 almost bit for bit (all but 34 of 1588 result lines of 8087 Bench on
  a real 1101 and 1137), and its instruction times are 1.45 % off on average
  against the real 1101. Without the ROM an instruction-level 8087 runs: the same
  answers for ordinary arithmetic, less exact in the corner cases and in timing.

  The ROM is not part of Compass-Emu; you supply the file. It is the dump named
  8087mc.txt (1648 lines of the letters A to P) from tools/8087mc/bin in
  github.com/a-mcego/granite. Save it as roms\8087mc.txt (the profile's ROM
  folder), or pick the file on the setup page (ROMs, "8087 microcode"). The setup
  page shows whether the file was found, and the status bar says "8087
  microcode" when the 8087 runs on it. Nothing else to switch on.

  To compare, the setup page's 8087 box ("Instruction-level 8087 even with the
  microcode ROM"), --fpu-table, or unticking Machine > 8087 on its microcode ROM
  while running keeps the
  instruction-level 8087. More in docs/8087-microcode.md in the source.

Demo tools
  Cycle counter: put marks in your program with OUT to ports a real Compass
  ignores (so they can stay in a release):
      MOV AL,1
      OUT 0F0H,AL      ; start section 1 (AL = section number 0-255)
      ...              ; code to time
      OUT 0F2H,AL      ; end section 1
      OUT 0F4H,AL      ; frame mark: "this frame is drawn"
  The emulator keeps count, last, min, average and max CPU clocks for each
  section. The frame mark tells how far into the frame (counted from the vertical
  sync tick, IR3) you got, on which display line the beam was, and how many
  frames went by without a mark (missed frames).

  F10 shows the table on top of the screen, with a blue tick at the left edge on
  the line where the last frame mark came. F9 clears the numbers. The title bar
  shows section and frame use, and on exit the full report goes to
  compass-marks.txt. --overlay starts with F10 on.

  Debugger: Debug > Debugger workspace (or the Debugger button) shows the
  registers and the 8087 stack, disassembly (click the circle in front of a line
  for a breakpoint), breakpoints and watches, the cycle counter table with a
  frame bar, the profiler, a memory view and a console. While paused a blue line
  on the screen shows where the beam is. "Run to next frame" stops at the next
  vertical sync tick. F8 pauses and goes on. Console commands (h = help):
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

  Timing is fitted to a real 1101 and 1137: plain 8086 code within about 1.5 %
  on average. The README in the source lists the measured accuracy.

Program symbols and the crash catcher
  Make a debug build (Project > Debug build, or compass.exe build PROJECT --disk
  DEV.img --debug): the same program (identical code), with the compilers'
  symbols kept in the project folder (NAME.pas.obj, NAME.asm.obj, NAME.map).
  Then Debug > Load program symbols and pick the project folder (or --symbols
  projects\NAME, or the Debugging card of the setup page, which remembers it).
  Compass-Emu finds the program in memory by its code whenever it runs, so:

  - the disassembly shows procedure names, assembler labels and the Pascal
    source lines, and calls show the name they go to (OSWRITE, MOVEBYTES...);
  - breakpoints can be set by name: b SprEnd, b TlCore.FreeRegion, or a source
    line b TLCORE:120 (the listing's statement number). A named breakpoint is
    armed whenever the program is in memory, also after you start it again;
  - the call stack (k, and the Call stack panel) follows the BP chain: each frame
    with its procedure and source line;
  - the profiler groups clocks by procedure and shows the source line of the
    hottest instructions.

  The crash catcher is on unless you untick it (Debug menu, debugger panel,
  setup page, or --no-catch). It stops the machine with a report (what went
  wrong, where, the call stack and the last instructions run) on:

  - a GRiD-OS error: an error code turned into its message (OsDecodeException,
    used by the Executive and Common code to show errors), with the message
    text, stopping where the program gets it back;
  - a divide error (DIV/IDIV by zero or a result too big);
  - INT 3 in the program (GRiD-OS turns every 8087 WAIT into INT 3; those are
    told apart, so a CC byte in your code works as a breakpoint);
  - a wild jump: running through empty memory (00 00 or FF FF), with where it
    came from.

  These work without symbols too, with addresses instead of names. More console
  commands:
      sym FOLDER       load symbols (sym = status, sym off = unload)
      names [TEXT]     the program's names and where they are now
      k                call stack
      hist [N]         the last N instructions run
      catch on|off     crash catcher (catch errors|div|int3|wild on|off)
      os on|off|N      log of GRiD-OS calls, each with its caller
  The Claude Code hook has the same commands (grid_debug), and grid_build has
  debug:true.

GRiDLink
  GRiDLink is the GRiD Server network: 250 kbit/s synchronous (SDLC) over RS-422
  on the serial port, with the server polling each Compass. With --gridlink the
  emulated 8274 sends every frame the Compass makes to the TCP connection and
  hands it every frame that comes back (docs/gridlink.md in the source has the
  format and a short Python example). The other end has to play the GRiD Server;
  none is included yet. Without a server the Compass's GRiDLink~Device~ just
  waits for the server's poll. The setup page has the same settings.
  Keep the emulator at real speed while linked: GRiDLink's timeouts run in
  emulated time, and turbo shortens them for the program at the other end.

The internal modem
  Every model except the 1137 has an internal modem (300 and 1200 baud). The
  emulator runs the modem's own firmware, so GRiDTerm, PhoneLink and other
  programs that use Modem~Device~ dial, answer and connect as on a real Compass.
  The phone line is TCP: a number the Compass dials connects to an address on the
  PC or the network, and a connection to a port on this PC rings the Compass.
  Once the modems have connected, the connection carries the plain data bytes
  both ways, nothing added.

  1. Put the modem firmware in the roms folder as 16020-007.bin (or use Import
     ROMs... on the setup page). The ROMs card shows "Internal modem (8749)" with
     the file it found.
  2. On the setup page's Serial and sound card, "Internal modem phone line":
     - Phone book: which address each number reaches, for example
           5551234=127.0.0.1:2400, *=myserver.example:2400
       * stands for any number not listed. A number written as an address with *
       between the parts dials it directly: 127*0*0*1*2400 reaches
       127.0.0.1:2400. Spaces and dashes in numbers don't matter.
     - Incoming calls: tcp:2400 makes a connection to port 2400 on this PC ring
       the Compass (tcp:0.0.0.0:2400 lets other PCs call too).
     - "Callers connect at 300 baud": the speed of a caller the Compass answers
       (else 1200).
     On the command line:
         --modem-dial 5551234=127.0.0.1:2400 --modem-listen tcp:2400
  3. Something has to answer at the other end: your own server program (it only
     has to accept TCP connections and send and receive bytes), another
     Compass-Emu (below), or for a first test a terminal program such as PuTTY
     waiting for a "Raw" connection on that port.

  Calling out with GRiDTerm: start GRiDTerm (Programs, GRiDTerm, kind Run
  Terminal), type the phone number in the form and confirm with CODE-RETURN
  (Alt+Enter). You hear the touch-tone digits; the call goes out when the modem
  starts its connect handshake, so the far end sees the connection a moment after
  the last digit. The far modem answers at the speed GRiDTerm asked for (Baud
  rate 300 or 1200 in the form), and after a few seconds GRiDTerm is online.
  Pulse dialing works too. Closing the connection at the far end is a hang-up:
  the Compass reports carrier lost after the modem's own carrier-loss delay.
  Hanging up on the Compass closes the connection.

  Calling the Compass: connect to the listen port. The Compass rings (2 s on, 4 s
  off) until a program answers; while it rings the modem takes a moment to go
  off-hook (it waits for the ring burst to end, as the real one does). Bytes sent
  before the answer wait until the modems have connected. A second caller while
  the line is in use is turned away (the connection closes).

  Two Compasses through the modem: give one --modem-listen tcp:2400 and the other
  --modem-dial *=127.0.0.1:2400 (separate disk images), then dial any number on
  the second one and answer on the first.

  Good to know:

  - Data bits and parity: the far end is set to 8 data bits, no parity. When the
    Compass program uses 7 bits with parity (GRiDTerm's default), the far end
    gets the parity bit as bit 7 of each byte, as a real terminal set to 8N1
    would; strip it there if needed.
  - Keep turbo off during calls: the modems' timing runs in emulated time.
  - The modem's speed is real: 300 or 1200 baud, so 30 or 120 bytes a second.
  - docs/modem.md in the source has how it works inside and the tests.

Two Compasses on a null-modem cable
  Make two profiles on the setup page, each with its own disk copy: the first
  with serial port tcp:2323, the second with connect:2323, and start both (two
  compass.exe windows; "Save as shortcut" makes a .bat for each). The second
  machine's serial port then talks to the first one's, as if a null-modem cable
  joined them. Programs that use the serial port (GRiDTerm, a two-player game)
  see each other. For two PCs, start the first with --serial tcp:0.0.0.0:2323 and
  the second with --serial connect:OTHERPC:2323. Turbo (F12) on one side only
  makes it run ahead of the other, so leave it off while linked.

Versions
  What changed in each version is in CHANGELOG.md, next to this file.

Source: https://github.com/ConventionalMemories/compass-emu
By Conventional Memories (C.M._), MIT license. Built on MartyPC by Daniel Balsom (MIT).
