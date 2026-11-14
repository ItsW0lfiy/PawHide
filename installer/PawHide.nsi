; NSIS online bootstrap. Absolutely no app binaries are embedded in the setup.
; Builds on Windows using makensis.exe; installs per-user without UAC.
Unicode True
!include "MUI2.nsh"
!define APP "PawHide"
!define RELEASE_REPO "ItsW0lfiy/PawHide"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\PawHide"
Name "${APP} (Online)"
OutFile "${__FILEDIR__}\..\dist\PawHide-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\PawHide"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
ShowInstDetails show
ShowUninstDetails show
!define MUI_ICON "${__FILEDIR__}\..\assets\PawHide.ico"
!define MUI_UNICON "${__FILEDIR__}\..\assets\PawHide.ico"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\PawHide.cmd"
!define MUI_FINISHPAGE_RUN_TEXT "Open PawHide command prompt"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  SetShellVarContext current
  SetRegView 64
FunctionEnd

Function un.onInit
  SetShellVarContext current
  SetRegView 64
FunctionEnd

Section "Download and install PawHide" Main
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "/oname=bootstrap.ps1" "${__FILEDIR__}\bootstrap.ps1"
  File "/oname=maintenance.ps1" "${__FILEDIR__}\maintenance.ps1"
  DetailPrint "Downloading the current GitHub release and checking SHA-256..."
  ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\bootstrap.ps1" -Destination "$INSTDIR" -Repository "${RELEASE_REPO}" -ErrorFile "$PLUGINSDIR\error.txt"' $0
  IntCmp $0 0 download_ok
  StrCpy $1 "Download or verification failed (exit code $0). Check the connection and public release availability."
  FileOpen $2 "$PLUGINSDIR\error.txt" r
  IfErrors +3
  FileRead $2 $1
  FileClose $2
  MessageBox MB_ICONSTOP|MB_OK "PawHide installation failed.$\r$\n$\r$\n$1"
  Abort
  download_ok:
  SetOutPath "$INSTDIR"
  CopyFiles /SILENT "$PLUGINSDIR\maintenance.ps1" "$INSTDIR\maintenance.ps1"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  ; A small interactive launcher opens a console in PawHide's installation
  ; directory, prints help, and keeps the session available for commands.
  FileOpen $2 "$INSTDIR\PawHide.cmd" w
  FileWrite $2 "@echo off$\r$\n"
  FileWrite $2 "cd /d $\"%~dp0$\"$\r$\n"
  FileWrite $2 "pawhide.exe --help$\r$\n"
  FileWrite $2 "cmd /k$\r$\n"
  FileClose $2
  CreateShortcut "$SMPROGRAMS\PawHide.lnk" "$INSTDIR\PawHide.cmd"
  ; Also replace the legacy GUI-era CLI shortcut with the same console.
  CreateShortcut "$SMPROGRAMS\PawHide CLI.lnk" "$INSTDIR\PawHide.cmd"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "PawHide"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\pawhide.exe,0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "ItsW0lfiy"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "/oname=maintenance.ps1" "${__FILEDIR__}\maintenance.ps1"
  ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\maintenance.ps1" -Destination "$INSTDIR" -RemoveStartup -ErrorFile "$PLUGINSDIR\error.txt"' $0
  IntCmp $0 0 uninstall_ready
  StrCpy $1 "Close PawHide and protected applications, then retry uninstall. No application files were removed."
  FileOpen $2 "$PLUGINSDIR\error.txt" r
  IfErrors +3
  FileRead $2 $1
  FileClose $2
  MessageBox MB_ICONSTOP|MB_OK "$1"
  Abort
  uninstall_ready:
  ; User rules and logs in LOCALAPPDATA\PawHide deliberately survive uninstall.
  Delete "$SMPROGRAMS\PawHide.lnk"
  Delete "$SMPROGRAMS\PawHide CLI.lnk"
  Delete "$INSTDIR\pawhide.exe"
  Delete "$INSTDIR\PawHide.cmd"
  Delete "$INSTDIR\PawHideUI.exe"
  Delete "$INSTDIR\pawhide_agent.dll"
  Delete "$INSTDIR\maintenance.ps1"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
