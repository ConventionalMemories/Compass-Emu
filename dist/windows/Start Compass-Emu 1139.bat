@echo off
rem Compass-Emu 0.3.12: GRiD Compass II 1139 (512 x 256 screen) booting the merged hard disk.
rem Put this folder inside your GRiD-MAME folder (next to roms, disks and tools).
rem It uses its own copy of the disk (113x build of GRiD-OS), so MAME's disk is never touched.
cd /d "%~dp0"
set PY=python
where python >nul 2>nul || set PY=py
if not exist roms\1139even.ccprom.bin powershell -NoProfile -Command "Expand-Archive -Force '..\roms\grid1139.zip' 'roms'"
if not exist roms\300067-01.bin powershell -NoProfile -Command "Expand-Archive -Force '..\roms\grid_keyboard.zip' 'roms'"
set DISK=..\disks\Compass-Emu 1139 disk.img
if not exist "%DISK%" (
  echo Making the 1139 copy of the merged disk...
  copy "..\disks\GRiD merged dev disk.img" "%DISK%" >nul || (pause & exit /b 1)
  %PY% ..\tools\gridmodel.py 1139 "%DISK%" --no-check || (pause & exit /b 1)
)
rem Application ROMs (Rom Toolbox dumps) go in the sockets when they are in roms.
set ROMS=
for %%i in (0 1 2 3) do if exist "roms\Compass_Socket_%%i.bin" call set ROMS=%%ROMS%% --rom%%i "roms\Compass_Socket_%%i.bin"
compass.exe --model 1139 --hd "%DISK%" --bubble "..\disks\Compass-Emu bubble.img" --serial tcp:2323 --folder "PC Folder" %ROMS%
if errorlevel 1 pause
