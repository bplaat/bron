@echo off
rem Notepad++ 7.9.2, the last version that runs on Windows XP, silent install without the updater
cd /d "%~dp0"
echo LOG installing Notepad++> COM1
npp.7.9.2.Installer.exe /S
rd /s /q "%ProgramFiles%\Notepad++\updater" 2>nul

rem On the desktop, opens text, settings and markup files
set "NPP=%ProgramFiles%\Notepad++\notepad++.exe"
cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" Desktop Notepad++ "%NPP%"
for %%e in (.log .cfg .conf .md .json .csv .diz .text .yml .yaml) do assoc %%e=txtfile >nul
for %%t in (txtfile inifile inffile xmlfile) do reg add "HKCR\%%t\shell\open\command" /ve /t REG_EXPAND_SZ /d "\"%NPP%\" \"%%1\"" /f >nul
