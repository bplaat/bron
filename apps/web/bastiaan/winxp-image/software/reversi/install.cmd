@echo off
rem Reversi (games/reversi, built by build.sh): copy to Program Files and an All Users desktop shortcut
cd /d "%~dp0"
set "TARGET=%ProgramFiles%\Reversi"
if not exist "%TARGET%" mkdir "%TARGET%"
copy /y reversi.exe "%TARGET%\reversi.exe" >nul
cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" Desktop Reversi "%TARGET%\reversi.exe"
