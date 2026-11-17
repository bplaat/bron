@echo off
rem Minesweeper and Pinball on the desktop, the card games stay in the Start menu Games folder
cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" Desktop Minesweeper "%SystemRoot%\system32\winmine.exe"
cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" Desktop Pinball "%ProgramFiles%\Windows NT\Pinball\PINBALL.EXE"
