' First logon of bplaat: fills the Windows Media Player library with My Music and the Windows MIDI files,
' then runs the player once
Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")
Set wmp = CreateObject("WMPlayer.OCX")

Sub AddFolder(folder)
    For Each file In folder.Files
        If LCase(fso.GetExtensionName(file.Name)) = "mp3" Then wmp.mediaCollection.add file.Path
    Next
    For Each child In folder.SubFolders
        AddFolder child
    Next
End Sub

AddFolder fso.GetFolder(shell.SpecialFolders("MyDocuments") & "\My Music")
media = shell.ExpandEnvironmentStrings("%SystemRoot%\Media\")
For Each name In Array("flourish.mid", "onestop.mid", "town.mid")
    wmp.mediaCollection.add media & name
Next
wmp.close

shell.Run "wmplayer.exe", 7, False
WScript.Sleep 30000
shell.Run "taskkill /im wmplayer.exe", 0, True
