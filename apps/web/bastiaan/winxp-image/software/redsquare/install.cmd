@echo off
rem RedSquare (examples/windows/redsquare, built with Make): copy to Program Files and an All Users desktop shortcut
cd /d "%~dp0"
set "TARGET=%ProgramFiles%\RedSquare"
if not exist "%TARGET%" mkdir "%TARGET%"
copy /y redsquare.exe "%TARGET%\redsquare.exe" >nul
cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" Desktop RedSquare "%TARGET%\redsquare.exe"
