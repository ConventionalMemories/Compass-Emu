@echo off
rem Like build.bat, and keep the symbols for Compass-Emu's debugger (procedure names,
rem source lines, breakpoints by name). Drag a .pas file onto this file, or type:
rem   debug hello   (means projects\hello\hello.pas)
rem Put this file next to build.bat (in GRiD-MAME).
cd /d "%~dp0"
set PY=python
where python >nul 2>nul || set PY=py
if "%~1"=="" (
  echo Drag a .pas file onto debug.bat, or type:  debug hello
  pause
  exit /b 1
)
%PY% tools\gridbuild.py debug "%~1"
pause
