' Launches the taskbar widget without a console window.
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh = CreateObject("WScript.Shell")
here = fso.GetParentFolderName(WScript.ScriptFullName)
py = "pythonw"
cand = sh.ExpandEnvironmentStrings("%LOCALAPPDATA%") & "\Python\pythoncore-3.14-64\pythonw.exe"
If fso.FileExists(cand) Then py = """" & cand & """"
sh.Run py & " """ & here & "\claude_quota_bar.py""", 0, False
