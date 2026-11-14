@echo off
rem Age of Empires II and The Conquerors: the files are already on C: (see Makefile), the registry and shortcuts
regedit /s "%~dp0install.reg"
call "%SystemRoot%\v86\userreg.cmd" "%~dp0user.reg"
set "GAME=%ProgramFiles%\Microsoft Games\Age of Empires II"
for %%f in (Desktop "Microsoft Games\Age of Empires II") do (
    cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" %%f "Age of Empires II" "%GAME%\EMPIRES2.EXE"
    cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" %%f "Age of Empires II - The Conquerors" "%GAME%\age2_x1\age2_x1.exe"
)
