@echo off
rem Imports a .reg file into the Default User profile, so accounts created later start with it: userreg.cmd <file.reg>
rem Its keys are written as [HKEY_USERS\V86Default\...]
reg load HKU\V86Default "%ALLUSERSPROFILE%\..\Default User\NTUSER.DAT" >nul
regedit /s %1
reg unload HKU\V86Default >nul
