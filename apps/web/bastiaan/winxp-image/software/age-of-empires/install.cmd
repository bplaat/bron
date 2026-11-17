@echo off
rem Age of Empires and The Rise of Rome: the files are already on C: (see Makefile), the registry and shortcuts
regedit /s "%~dp0install.reg"
set "GAME=%ProgramFiles%\Microsoft Games\Age of Empires"
for %%f in (Desktop "Microsoft Games\Age of Empires") do (
    cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" %%f "Age of Empires" "%GAME%\Empires.exe" MIDIMUSIC
    cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" %%f "Age of Empires - The Rise of Rome" "%GAME%\EmpiresX.exe" MIDIMUSIC
)
