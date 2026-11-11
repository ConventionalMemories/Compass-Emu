@echo off
rem Compass-Emu 0.3.2: GRiD Compass 1101 booting the merged hard disk.
rem Put this folder inside your GRiD-MAME folder (next to roms, disks and tools).
rem It uses its own copy of the disk, so MAME's disk is never touched.
cd /d "%~dp0"
set PY=python
where python >nul 2>nul || set PY=py
if not exist roms\1101even.rev3.bin powershell -NoProfile -Command "Expand-Archive -Force '..\roms\grid1101.zip' 'roms'"
if not exist roms\300067-01.bin powershell -NoProfile -Command "Expand-Archive -Force '..\roms\grid_keyboard.zip' 'roms'"
set DISK=..\disks\Compass-Emu 1101 disk.img
if not exist "%DISK%" (
  echo Making the 1101 copy of the merged disk...
  copy "..\disks\GRiD merged dev disk.img" "%DISK%" >nul || (pause & exit /b 1)
  %PY% ..\tools\gridmodel.py 1101 "%DISK%" --no-check || (pause & exit /b 1)
)
compass.exe --model 1101 --hd "%DISK%" --bubble "..\disks\Compass-Emu bubble.img" --serial tcp:2323 --folder "PC Folder"
if errorlevel 1 pause
