@echo off
rem 7-Zip: the files are already on C: (see Makefile), register the Explorer menu like its installer does
set "TARGET=%ProgramFiles%\7-Zip"
regsvr32 /s "%TARGET%\7-zip.dll"
reg add "HKLM\Software\7-Zip" /v Path /t REG_SZ /d "%TARGET%" /f >nul
cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" 7-Zip "7-Zip File Manager" "%TARGET%\7zFM.exe"
