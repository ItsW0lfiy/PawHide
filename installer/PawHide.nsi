; NSIS online bootstrap. Absolutely no app binaries are embedded in the setup.
; Builds on Windows using makensis.exe; installs per-user without UAC.
Unicode True
!include "MUI2.nsh"
!define APP "PawHide"
!define RELEASE_REPO "ItsW0lfiy/PawHide"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\PawHide"
Name "${APP} (Online)"
OutFile "dist\PawHide-Online-Setup.exe"
InstallDir "$LOCALAPPDATA\Programs\PawHide"
RequestExecutionLevel user
SetShellVarContext current
ShowInstDetails show
ShowUninstDetails show
!define MUI_ICON "${__FILEDIR__}\..\ui\assets\PawHide.ico"
!define MUI_UNICON "${__FILEDIR__}\..\ui\assets\PawHide.ico"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Download and install PawHide" Main
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "/oname=bootstrap.ps1" "${__FILEDIR__}\bootstrap.ps1"
  DetailPrint "Downloading the current GitHub release and checking SHA-256..."
  ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\bootstrap.ps1" -Destination "$INSTDIR" -Repository "${RELEASE_REPO}"' $0
  IntCmp $0 0 download_ok
  MessageBox MB_ICONSTOP|MB_OK "PawHide download/verification failed (exit code $0).$
$
Check that a GitHub release exists and is accessible. No offline installer is provided."
  Abort
  download_ok:
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\PawHide.lnk" "$INSTDIR\PawHideUI.exe"
  CreateShortcut "$SMPROGRAMS\PawHide CLI.lnk" "$INSTDIR\pawhide.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "PawHide"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\PawHideUI.exe,0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "ItsW0lfiy"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  ; Stop the user monitor and clear its startup entry before removing binaries.
  IfFileExists "$INSTDIR\pawhide.exe" 0 +2
    ExecWait '"$INSTDIR\pawhide.exe" auto stop'
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "PawHide Auto"
  ; User rules and logs in LOCALAPPDATA\PawHide deliberately survive uninstall.
  Delete "$SMPROGRAMS\PawHide.lnk"
  Delete "$SMPROGRAMS\PawHide CLI.lnk"
  Delete "$INSTDIR\pawhide.exe"
  Delete "$INSTDIR\PawHideUI.exe"
  Delete "$INSTDIR\pawhide_agent.dll"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
