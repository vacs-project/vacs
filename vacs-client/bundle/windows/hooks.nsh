; The template derives the install location key from the publisher. 3.0.0 set one and stored its
; location under this key instead of Software\vacs\vacs, so updates lost a custom install folder.
!define VACS300INSTALLKEY "Software\The vacs Project\vacs"

Var VacsLegacyDir
Var Vacs300Dir
Var VacsLegacyInUse

; The older key survives an uninstall of 3.0.0, so the 3.0.0 folder only wins over it when the
; older folder holds no vacs.
!macro VacsCheckLegacyInUse
  StrCpy $VacsLegacyInUse 0
  ${If} $VacsLegacyDir != ""
    ${If} ${FileExists} "$VacsLegacyDir\vacs.exe"
    ${OrIf} ${FileExists} "$VacsLegacyDir\vacs-client.exe"
      StrCpy $VacsLegacyInUse 1
    ${EndIf}
  ${EndIf}
!macroend

; Runs before the first page, so the reinstall page uninstalls from and the directory page offers
; the 3.0.0 folder. The template's defines follow this file, hence the literal key and $(^Name).
!define MUI_CUSTOMFUNCTION_GUIINIT VacsGuiInit
Function VacsGuiInit
  ${If} "$(^Name)" == "vacs"
    ReadRegStr $VacsLegacyDir SHCTX "Software\vacs\vacs" ""
    ReadRegStr $Vacs300Dir SHCTX "${VACS300INSTALLKEY}" ""
    !insertmacro VacsCheckLegacyInUse
    ${If} $Vacs300Dir != ""
    ${AndIf} $VacsLegacyInUse = 0
      StrCpy $INSTDIR $Vacs300Dir
      WriteRegStr SHCTX "Software\vacs\vacs" "" $Vacs300Dir
    ${EndIf}
  ${EndIf}
FunctionEnd

; Must list every file any release installed: copies at a previous location are not tracked by the
; uninstall entry once this install overwrites it.
!macro VacsRemoveInstall dir
  ${If} ${FileExists} "${dir}\vacs.exe"
  ${OrIf} ${FileExists} "${dir}\vacs-client.exe"
    Delete "${dir}\vacs.exe"
    Delete "${dir}\vacs-client.exe"
    Delete "${dir}\uninstall.exe"
    Delete "${dir}\LICENSE-APACHE"
    Delete "${dir}\LICENSE-MIT"
    Delete "${dir}\README.md"
    Delete "${dir}\licenses\OFL-noto.txt"
    Delete "${dir}\licenses\THIRD_PARTY_LICENSES.html"
    RMDir "${dir}\licenses"
    RMDir "${dir}"
  ${EndIf}
!macroend

; Like the template's SetShortcutTarget, but also moves the working directory, which still points
; at the old folder that may just have been removed.
!macro VacsRetargetIfTarget lnk old
  !insertmacro IsShortcutTarget "${lnk}" "${old}"
  Pop $0
  ${If} $0 = 1
    !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
    ${If} $0 P<> 0
      ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}",.r1)'
      ${If} $1 P<> 0
        ${IPersistFile::Load} $1 '("${lnk}", ${STGM_READWRITE})'
        ${IShellLink::SetPath} $0 '(w "$INSTDIR\${MAINBINARYNAME}.exe")'
        ${IShellLink::SetWorkingDirectory} $0 '(w "$INSTDIR")'
        ${IPersistFile::Save} $1 '("${lnk}",1)'
        ${IUnknown::Release} $1 ""
      ${EndIf}
      ${IUnknown::Release} $0 ""
    ${EndIf}
  ${EndIf}
!macroend

; This 32-bit installer reads links that 64-bit Explorer made under Program Files, such as taskbar
; pins, as pointing into Program Files (x86).
!macro VacsRetargetFrom lnk old
  StrCpy $R6 "${old}"
  !insertmacro VacsRetargetIfTarget "${lnk}" $R6
  StrLen $R8 "$PROGRAMFILES64"
  StrCpy $R9 $R6 $R8
  ${If} $R9 == "$PROGRAMFILES64"
    StrCpy $R9 $R6 "" $R8
    !insertmacro VacsRetargetIfTarget "${lnk}" "$PROGRAMFILES32$R9"
  ${EndIf}
!macroend

; The template only migrates shortcuts that point at $INSTDIR\<recorded main binary>, which misses
; both the renamed 2.8 binary after 3.0.0 recorded vacs.exe and every copy at another location.
!macro VacsRetargetShortcut lnk
  ${If} ${FileExists} "${lnk}"
    !insertmacro VacsRetargetFrom "${lnk}" "$INSTDIR\vacs-client.exe"
    !insertmacro VacsRetargetFrom "${lnk}" "$VacsLegacyDir\vacs-client.exe"
    !insertmacro VacsRetargetFrom "${lnk}" "$VacsLegacyDir\vacs.exe"
    !insertmacro VacsRetargetFrom "${lnk}" "$Vacs300Dir\vacs.exe"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $VacsLegacyDir SHCTX "${MANUPRODUCTKEY}" ""
  StrCpy $Vacs300Dir ""
  ; RC builds share these hooks under another product name and must not touch a release install.
  !if "${PRODUCTNAME}" == "vacs"
    !if "${MANUPRODUCTKEY}" != "Software\vacs\vacs"
      !error "VacsGuiInit hardcodes Software\vacs\vacs, keep bundle.publisher unset on Windows"
    !endif
    ReadRegStr $Vacs300Dir SHCTX "${VACS300INSTALLKEY}" ""
  !endif

  ; Silent updates skip VacsGuiInit. A manual install must keep the folder the user picked, and by
  ; now its uninstall-first choice may already have emptied the older folder.
  !insertmacro VacsCheckLegacyInUse
  ${If} $Vacs300Dir != ""
  ${AndIf} $UpdateMode = 1
  ${AndIf} $VacsLegacyInUse = 0
    StrCpy $R7 $INSTDIR
    StrCpy $INSTDIR $Vacs300Dir
    SetOutPath $INSTDIR
    RMDir $R7
  ${EndIf}

  ${If} $VacsLegacyDir != ""
  ${AndIf} $VacsLegacyDir != $INSTDIR
    !insertmacro CheckIfAppIsRunning "$VacsLegacyDir\vacs-client.exe" "${PRODUCTNAME}"
    !insertmacro CheckIfAppIsRunning "$VacsLegacyDir\vacs.exe" "${PRODUCTNAME}"
    !insertmacro VacsRemoveInstall $VacsLegacyDir
  ${EndIf}

  ${If} $Vacs300Dir != ""
    ${If} $Vacs300Dir != $INSTDIR
    ${AndIf} $Vacs300Dir != $VacsLegacyDir
      !insertmacro CheckIfAppIsRunning "$Vacs300Dir\vacs.exe" "${PRODUCTNAME}"
      !insertmacro VacsRemoveInstall $Vacs300Dir
    ${EndIf}
    DeleteRegKey SHCTX "${VACS300INSTALLKEY}"
    DeleteRegKey /ifempty SHCTX "Software\The vacs Project"
  ${EndIf}
  ; 3.0.0 also stored its installer language under this key in HKCU, even for per-machine installs.
  !if "${PRODUCTNAME}" == "vacs"
    DeleteRegValue HKCU "${VACS300INSTALLKEY}" "Installer Language"
    DeleteRegKey /ifempty HKCU "${VACS300INSTALLKEY}"
    DeleteRegKey /ifempty HKCU "Software\The vacs Project"
  !endif

  ; Releases up to 2.8 installed the binary as vacs-client.exe. The template only checks for and
  ; removes the current binary, so a running old one would stay locked and be left behind.
  !insertmacro CheckIfAppIsRunning "$INSTDIR\vacs-client.exe" "${PRODUCTNAME}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; The publisher is unset on Windows to keep the install location key, but Apps keeps showing it.
  WriteRegStr SHCTX "${UNINSTKEY}" "Publisher" "The vacs Project"
  ; 3.0.0 already recorded vacs.exe as the main binary, so the template's cleanup skips this one.
  Delete "$INSTDIR\vacs-client.exe"

  !insertmacro VacsRetargetShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !insertmacro VacsRetargetShortcut "$DESKTOP\${PRODUCTNAME}.lnk"

  ; Taskbar pins live in the installing user's profile even for per-machine installs.
  SetShellVarContext current
  FindFirst $R5 $R4 "$APPDATA\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\*.lnk"
  ${DoWhile} $R4 != ""
    !insertmacro VacsRetargetShortcut "$APPDATA\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\$R4"
    FindNext $R5 $R4
  ${Loop}
  FindClose $R5
  ${If} $MultiUser.InstallMode == "AllUsers"
    SetShellVarContext all
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\vacs-client.exe"
  RMDir "$INSTDIR"
!macroend
