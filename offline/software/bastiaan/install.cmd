@echo off
rem Install bastiaan.exe: copy to Program Files, All Users desktop shortcut and start at every logon
cd /d "%~dp0"
set "TARGET=%ProgramFiles%\Bastiaan"
if not exist "%TARGET%" mkdir "%TARGET%"
copy /y bastiaan.exe "%TARGET%\bastiaan.exe" >nul
cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" Desktop Bastiaan "%TARGET%\bastiaan.exe"
reg add "HKLM\Software\Microsoft\Windows\CurrentVersion\Run" /v Bastiaan /t REG_SZ /d "\"%TARGET%\bastiaan.exe\"" /f >nul
