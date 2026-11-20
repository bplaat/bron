@echo off
setlocal
set "TOOLCHAIN=%ProgramFiles%\Toolchain"
xcopy /E /I /Q /Y "%~dp0Toolchain" "%TOOLCHAIN%" >nul || exit /b 1
set "PROFILES=%ALLUSERSPROFILE%\.."
xcopy /E /I /Q /Y "%~dp0projects" "%PROFILES%\Default User\My Documents\My Projects" >nul || exit /b 1
if exist "%PROFILES%\bplaat" xcopy /E /I /Q /Y "%~dp0projects" "%PROFILES%\bplaat\My Documents\My Projects" >nul || exit /b 1
cscript //nologo "%~dp0path.vbs" "%TOOLCHAIN%\bin" || exit /b 1
exit /b 0
