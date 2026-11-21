@echo off
rem Notepad++ 7.9.2, the last version that runs on Windows XP, silent install without the updater
cd /d "%~dp0"
echo LOG installing Notepad++> COM1
npp.7.9.2.Installer.exe /S
rd /s /q "%ProgramFiles%\Notepad++\updater" 2>nul

rem On the desktop, opens text, settings, markup and source files, and files without an extension like Makefile
set "NPP=%ProgramFiles%\Notepad++\notepad++.exe"
cscript //nologo //b "%SystemRoot%\v86\shortcut.vbs" Desktop Notepad++ "%NPP%"
for %%e in (.log .cfg .conf .md .csv .diz .text .yml .yaml .xml .diff .patch .sh .manifest) do assoc %%e=txtfile >nul
for %%e in (.json .css .scss .ts .tsx .jsx) do assoc %%e=txtfile >nul
for %%e in (.c .h .cpp .hpp .cc .asm .s .inc .rc .def .mk .mak) do assoc %%e=txtfile >nul
assoc .=txtfile >nul
for %%t in (txtfile inifile inffile xmlfile) do reg add "HKCR\%%t\shell\open\command" /ve /t REG_EXPAND_SZ /d "\"%NPP%\" \"%%1\"" /f >nul

rem Folders open in the Folder as Workspace panel from their context menu
reg add "HKCR\Directory\shell\notepad++" /ve /d "Open in Notepad++" /f >nul
reg add "HKCR\Directory\shell\notepad++\command" /ve /t REG_EXPAND_SZ /d "\"%NPP%\" -openFoldersAsWorkspace \"%%1\"" /f >nul
