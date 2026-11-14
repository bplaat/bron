@echo off
rem Installs every folder in software\ with its install.cmd, then powers off (offline/software.mjs runs it in QEMU)
reg add "HKLM\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer" /v NoDriveTypeAutoRun /t REG_DWORD /d 255 /f >nul
for /d %%d in ("%SystemRoot%\v86\software\*") do (
    echo LOG installing %%~nxd> COM1
    pushd "%%d"
    call install.cmd
    popd
)
del "%ALLUSERSPROFILE%\Start Menu\Programs\Startup\software.cmd"
echo DONE> COM1
shutdown -s -f -t 0
