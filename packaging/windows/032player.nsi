; Native NSIS equivalent of 032player.iss for local Linux / macOS builds.
; Example:
;   makensis -DAppVersion=0.2.36 -DSourceDir=/absolute/release \
;     -DOutputDir=/absolute/dist packaging/windows/032player.nsi

Unicode true
SetCompressor /SOLID lzma
CRCCheck on
ManifestDPIAware true

!ifndef AppVersion
  !define AppVersion "0.0.0"
!endif
!ifndef AppFileVersion
  ; VIProductVersion requires four numeric parts (X.Y.Z.W); strip any
  ; semver prerelease / build suffix (e.g. "0.3.0-rc1" -> "0.3.0.0").
  !searchparse /noerrors "${AppVersion}" "" _AppBaseVersion "-"
  !ifdef _AppBaseVersion
    !define AppFileVersion "${_AppBaseVersion}.0"
  !else
    !define AppFileVersion "${AppVersion}.0"
  !endif
!endif
!ifndef SourceDir
  !define SourceDir "..\..\target\x86_64-pc-windows-msvc\release"
!endif
!ifndef OutputDir
  !define OutputDir "..\..\dist"
!endif

; Share the Inno Setup AppId uninstall key so switching between Inno Setup
; and NSIS upgrades in place instead of creating duplicate uninstall entries.
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\{6F1C3E52-7D0B-4E1A-9C55-032032032032}_is1"
!define LEGACY_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\032player"

!include "MUI2.nsh"
!include "LogicLib.nsh"

Name "032player"
OutFile "${OutputDir}/032player-${AppVersion}-windows-x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\032player"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
Icon "..\..\crates\app\assets\icon.ico"
UninstallIcon "..\..\crates\app\assets\icon.ico"
BrandingText "032"

VIProductVersion "${AppFileVersion}"
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
!insertmacro MUI_PAGE_COMPONENTS
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

Function un.onInit
  retry:
    FindWindow $0 "" "032player"
    IntCmp $0 0 ready
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "032player is running. Close it before uninstalling." IDRETRY retry
    Abort
  ready:
FunctionEnd

Section "032player" SEC_MAIN
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${SourceDir}/032player.exe"
  File "${SourceDir}/libmpv-2.dll"
  File "/oname=LICENSE.txt" "..\..\LICENSE"
  SetOutPath "$INSTDIR\licenses"
  File "..\..\crates\app\assets\fonts\OFL-NotoSansSC.txt"
  File "..\..\crates\app\assets\fonts\OFL-SourceSerif4.txt"
  File "/oname=FONTS.md" "..\..\crates\app\assets\fonts\README.md"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\032player"
  CreateShortcut "$SMPROGRAMS\032player\032player.lnk" "$INSTDIR\032player.exe"
  CreateShortcut "$SMPROGRAMS\032player\Uninstall 032player.lnk" "$INSTDIR\Uninstall.exe"

  DeleteRegKey HKCU "${LEGACY_UNINSTALL_KEY}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "032player"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${AppVersion}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\032player.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "032"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/lux032/032player-rs"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section /o "Create desktop shortcut" SEC_DESKTOP
  CreateShortcut "$DESKTOP\032player.lnk" "$INSTDIR\032player.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\032player.exe"
  Delete "$INSTDIR\libmpv-2.dll"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\licenses\OFL-NotoSansSC.txt"
  Delete "$INSTDIR\licenses\OFL-SourceSerif4.txt"
  Delete "$INSTDIR\licenses\FONTS.md"
  RMDir "$INSTDIR\licenses"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$SMPROGRAMS\032player\032player.lnk"
  Delete "$SMPROGRAMS\032player\Uninstall 032player.lnk"
  Delete "$DESKTOP\032player.lnk"
  RMDir "$SMPROGRAMS\032player"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "${LEGACY_UNINSTALL_KEY}"
SectionEnd
