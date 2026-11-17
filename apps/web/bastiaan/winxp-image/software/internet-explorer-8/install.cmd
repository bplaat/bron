@echo off
rem Internet Explorer 8, Microsoft's own installer: silent, without Windows Update or an uninstall backup. It finishes
rem replacing Internet Explorer 6 at the next boot, which is the v86 setup.
cd /d "%~dp0"
IE8-WindowsXP-x86-ENU.exe /quiet /norestart /update-no /nobackup
if errorlevel 1 if not errorlevel 3010 echo LOG Internet Explorer 8 setup failed with %errorlevel%> COM1
if errorlevel 3011 echo LOG Internet Explorer 8 setup failed with %errorlevel%> COM1

rem No first run wizard and no default browser check, links keep opening in the page's browser
reg add "HKLM\Software\Policies\Microsoft\Internet Explorer\Main" /v DisableFirstRunCustomize /t REG_DWORD /d 1 /f >nul
call "%SystemRoot%\v86\userreg.cmd" "%~dp0user.reg"
