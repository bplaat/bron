@echo off
rem Civilization III Complete: the files are already on C: (see Makefile), the registry and shortcuts to the final game
regedit /s "%~dp0install.reg"
for %%f in (Desktop "Civilization III") do (
    cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" %%f "Civilization III" "%ProgramFiles%\Civilization III\Conquests\Civ3Conquests.exe"
)
