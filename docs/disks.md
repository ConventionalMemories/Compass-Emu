# Disks and media

What Compass-Emu needs to boot, which image files it takes, and when it writes them. The
drives themselves are described in [drives.md](drives.md).

## Which GRiD-OS for which model

GRiD-OS (CCOS) comes in two builds, and each model needs the right one:

| Models | GRiD-OS build |
|---|---|
| 1101, 1109, 1121, 1129 | the 11xx build |
| 1131, 1137, 1139 | the 113x build (it turns on the 512 x 256 screen) |

The wrong build doesn't get to the Executive. The build mode picks its hidden machine to
match the development disk (an 1129 for the 11xx build, an 1139 for the 113x build).

A Compass can boot from a hard disk in a 2101, a floppy in a 2101 or a 2102, the bubble
memory, or a bootable ROM (CCOS) in an application ROM socket on the Compass II models. The
boot PROM decides which; with a bootable ROM in a socket it boots from that ROM even with a
hard disk.

## Image files

| Medium | File | Size |
|---|---|---|
| 2101 hard disk | raw image, 512-byte pages in GRiD page order | 41,616 pages (21,307,392 bytes) for a 20 MB drive, 20,808 pages for a 10 MB one. The low-level 2101 needs one of these two sizes; the protocol model takes others. |
| Floppy (2101 or 2102) | raw image, `.img` | 360 KB: 720 pages (40 cylinders, 2 sides, 9 sectors of 512 bytes) |
| Floppy, ImageDisk | `.IMD` | used through an `.img` with the same name beside it, made from the .IMD the first time; changes go to the .img, the .IMD stays as it was |
| Bubble memory | raw 7220/7110 contents in address order | 128 KB per module (384K on the 1101); a missing file is made blank |
| PC folder | a folder on the PC | built into a GRiD volume of up to 32 MB at start (see [drives.md](drives.md#pc-folder-drive)) |
| Application ROM | raw chip dump | 32, 64 or 128 KB per socket |

A hard disk can hold up to four GRiD volumes (a partition table in page 0, as MediaPartition
makes). The media manager shows each volume with its subjects and files.

Floppy images that were never formatted on a 2101 or 2102 get the layout the drives use
(interleave 5, side 1 skewed by 7 sectors); a format by the low-level 2101 or 2102 writes the
box's own layout (the 2101 interleave 3, the 2102 2:1, no skew) and the emulator keeps it.

## Making blank media

- **Floppy**: Media > media manager, "New blank floppy, named" + Make...: a formatted 360K
  GRiD floppy with that volume name, ready to use.
- **Bubble**: the setup page's Bubble card, Make blank (or name a file that doesn't exist).
  Format it in GRiD-OS with Initialize Media.
- **Hard disk**: there is no blank hard disk maker yet. Start from an image of a real disk,
  or partition and initialize one in GRiD-OS (MediaPartition, Initialize Media).

## Getting files in and out

- The **media manager** (Media menu) browses any image: export files to the PC from any
  disk, and import files, delete files and add subjects on images that are not in use.
- The **PC folder drive** shows a PC folder as a GRiD drive while the machine runs: files
  added on the PC appear within seconds, and what GRiD writes is saved to the folder when the
  machine powers off. This is the way to move files while running.
- **Build and run** (Project window) copies a freshly built program into the running
  machine's PC folder drive.

## When changes are saved

- Hard disks, floppies, the bubble and the PC folder are written back when the machine
  powers off or the window closes, and when a floppy is taken out or swapped. Media > Write
  changed disks back now saves them on the way.
- If the emulator crashes, changes since the last write-back are lost.
- `--readonly` never writes anything back. Write protect per drive: a protected floppy
  refuses GRiD's writes (error 106, like a write-protect tab); a protected hard disk or PC
  folder takes them but never saves them. The bubble has its own Write protect box.
- Quick save and saved states hold the whole machine, disks included for quick save (kept in
  memory) but **not** for state files: a state file expects the disks as they were when it
  was saved.

## Development disks

To build programs (Project window, `compass build`, the MCP `grid_build`), the disk needs
the GRiD compilers: Pascal~Run~ and DevelopmentExecutive~Run~, and for `compass do` also
`Do~Run~` from the GRiD-OS 3.1.5 floppy in Programs. See
[build-and-mcp.md](build-and-mcp.md).

## Disk errors you may see

| Error | Meaning |
|---|---|
| 106 | the floppy is write protected |
| 128 | no disk in the 2101's floppy slot |
| 451 | a GPIB transfer stalled and the keyboard controller's DMA watchdog ended it (about 4 s) |
| FATAL ERROR 9010 / 9011 (build) | the development disk is full |
| "Main memory (RAM) is full" (build) | the compiler needs a 512K model |
