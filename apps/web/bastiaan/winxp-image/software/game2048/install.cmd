@echo off
rem 2048 (games/game2048, built by build.sh) with the MiniBlink engine of its web view next to it
cd /d "%~dp0"
set "TARGET=%ProgramFiles%\2048"
if not exist "%TARGET%" mkdir "%TARGET%"
copy /y game2048.exe "%TARGET%\game2048.exe" >nul
copy /y miniblink.dll "%TARGET%\miniblink.dll" >nul
cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" Desktop 2048 "%TARGET%\game2048.exe"
