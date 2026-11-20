Set shell = CreateObject("WScript.Shell")
key = "HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\Path"
path = shell.RegRead(key)
bin = WScript.Arguments(0)

For Each entry In Split(path, ";")
    If StrComp(shell.ExpandEnvironmentStrings(Trim(entry)), bin, vbTextCompare) = 0 Then WScript.Quit 0
Next

If Len(path) > 0 And Right(path, 1) <> ";" Then path = path & ";"
shell.RegWrite key, path & bin, "REG_EXPAND_SZ"
