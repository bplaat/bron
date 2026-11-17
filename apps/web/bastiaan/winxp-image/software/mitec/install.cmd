@echo off
rem MiTeC EXE Explorer, Hexadecimal Editor and Task Manager DeLuxe in Program Files\MiTeC, desktop and Start menu
cd /d "%~dp0"
if not exist files\*.exe goto :eof
echo LOG installing MiTeC tools> COM1
set "TARGET=%ProgramFiles%\MiTeC"
if not exist "%TARGET%" mkdir "%TARGET%"
xcopy /e /q /y files "%TARGET%\" >nul
for %%f in (Desktop MiTeC) do (
    cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" %%f "EXE Explorer" "%TARGET%\EXE.exe"
    cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" %%f "Hexadecimal Editor" "%TARGET%\HEXEdit.exe"
    cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" %%f "Task Manager DeLuxe" "%TARGET%\TMX.exe"
)
