' Activates Windows offline: confid.exe turns the installation ID into a confirmation ID. Reports over COM1 and
' fails the build when Windows is still not activated afterwards.
On Error Resume Next
Set fso = CreateObject("Scripting.FileSystemObject")
Set shell = CreateObject("WScript.Shell")
Set com = fso.OpenTextFile("COM1", 2)
dir = fso.GetParentFolderName(WScript.ScriptFullName)
output = shell.ExpandEnvironmentStrings("%TEMP%\confid.txt")
For Each wpa In GetObject("winmgmts:").InstancesOf("Win32_WindowsProductActivation")
    If wpa.ActivationRequired <> 0 Then
        result = wpa.GetInstallationID(iid)
        com.WriteLine "LOG activation: installation ID " & iid & " (" & result & ")"
        shell.Run "cmd /c """"" & dir & "\confid.exe"" " & iid & " > """ & output & """""", 0, True
        cid = Replace(Trim(fso.OpenTextFile(output).ReadLine()), "-", "")
        fso.DeleteFile output
        result = wpa.ActivateOffline(cid)
        com.WriteLine "LOG activation: confirmation ID " & cid & " (" & result & ")"
    End If
Next
If Err.Number <> 0 Then com.WriteLine "LOG activation: error " & Err.Number & " " & Err.Description
For Each wpa In GetObject("winmgmts:").InstancesOf("Win32_WindowsProductActivation")
    If wpa.ActivationRequired = 0 Then com.WriteLine "LOG activation: done" Else com.WriteLine "ERROR activation failed"
Next
