# Starting Compass-Emu from a MAME setup

These files are for people who already run the GRiD Compass in MAME with a folder
laid out as `roms\`, `disks\` and `tools\` (the MAME ROM sets as zips, a GRiD-OS hard
disk image, and Python tools such as `gridmodel.py` and `gridfs.py`). They are not part
of the Windows zip; the zip starts from the machine setup page instead.

- `Start Compass-Emu 1101.bat`, `Start Compass-Emu 1139.bat`: put the compass-emu folder
  inside the MAME folder. The first start unpacks the ROMs from `..\roms` and makes a
  copy of the hard disk for the model.
- `Start Compass-Emu 1101 link.bat`: a second 1101 joined to the first by a
  null-modem cable (serial port over TCP 2323).
- `tools\gridbuild.py` and `tools\debug.bat`: the build script that compiles on
  Compass-Emu instead of MAME (`build.bat`, `debug.bat`). Copy `gridbuild.py` over
  `tools\gridbuild.py` and `debug.bat` next to `build.bat`. The same build is built into
  compass.exe (Project > Build, or `compass build`), which needs no Python.

Edit the disk names in the .bat files to match your own images.
