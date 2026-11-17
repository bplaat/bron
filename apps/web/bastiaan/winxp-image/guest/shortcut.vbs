' Creates a shortcut for all users: shortcut.vbs <Desktop | Start menu folder> <name> <target> [arguments]
' The Start menu folder can be nested, like "Microsoft Games\Age of Empires"
Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")
Set args = WScript.Arguments
If args(0) = "Desktop" Then
    folder = shell.SpecialFolders("AllUsersDesktop")
Else
    folder = shell.SpecialFolders("AllUsersPrograms")
    For Each part In Split(args(0), "\")
        folder = folder & "\" & part
        If Not fso.FolderExists(folder) Then fso.CreateFolder folder
    Next
End If
Set link = shell.CreateShortcut(folder & "\" & args(1) & ".lnk")
link.TargetPath = args(2)
If args.Count > 3 Then link.Arguments = args(3)
link.WorkingDirectory = fso.GetParentFolderName(args(2))
link.Save
