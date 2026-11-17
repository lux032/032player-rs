; Native NSIS equivalent of 032player.iss for local Linux / macOS builds.
; Example:
;   makensis -DAppVersion=0.2.16 -DSourceDir=/absolute/release \
;     -DOutputDir=/absolute/dist packaging/windows/032player.nsi

Unicode true
SetCompressor /SOLID lzma
CRCCheck on
ManifestDPIAware true

!ifndef AppVersion
  !define AppVersion "0.0.0"
!endif
!ifndef SourceDir
  !define SourceDir "..\..\target\x86_64-pc-windows-msvc\release"
!endif
!ifndef OutputDir
  !define OutputDir "..\..\dist"
!endif

!include "MUI2.nsh"
!include "LogicLib.nsh"

Name "032player"
OutFile "${OutputDir}/032player-${AppVersion}-windows-x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\032player"
RequestExecutionLevel user
Icon "..\..\crates\app\assets\icon.ico"
UninstallIcon "..\..\crates\app\assets\icon.ico"
BrandingText "032"

VIProductVersion "${AppVersion}.0"
VIAddVersionKey "ProductName" "032player"
VIAddVersionKey "ProductVersion" "${AppVersion}"
VIAddVersionKey "FileDescription" "032player installer"
VIAddVersionKey "FileVersion" "${AppVersion}"
VIAddVersionKey "CompanyName" "032"
VIAddVersionKey "LegalCopyright" "032player"

!define MUI_FINISHPAGE_RUN "$INSTDIR\032player.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch 032player"
!insertmacro MUI_PAGE_LICENSE "..\..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  retry:
    FindWindow $0 "" "032player"
    IntCmp $0 0 ready
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "032player is running. Close it before installing." IDRETRY retry
    Abort
  ready:
FunctionEnd

Section "032player" SEC_MAIN
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${SourceDir}/032player.exe"
  File "${SourceDir}/libmpv-2.dll"
  File "/oname=LICENSE.txt" "..\..\LICENSE"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\032player"
  CreateShortcut "$SMPROGRAMS\032player\032player.lnk" "$INSTDIR\032player.exe"
  CreateShortcut "$SMPROGRAMS\032player\Uninstall 032player.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player" "DisplayName" "032player"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player" "DisplayVersion" "${AppVersion}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player" "DisplayIcon" "$INSTDIR\032player.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player" "Publisher" "032"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player" "URLInfoAbout" "https://github.com/lux032/032player-rs"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player" "NoRepair" 1
SectionEnd

Section /o "Create desktop shortcut" SEC_DESKTOP
  CreateShortcut "$DESKTOP\032player.lnk" "$INSTDIR\032player.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\032player.exe"
  Delete "$INSTDIR\libmpv-2.dll"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$SMPROGRAMS\032player\032player.lnk"
  Delete "$SMPROGRAMS\032player\Uninstall 032player.lnk"
  Delete "$DESKTOP\032player.lnk"
  RMDir "$SMPROGRAMS\032player"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player"
SectionEnd
