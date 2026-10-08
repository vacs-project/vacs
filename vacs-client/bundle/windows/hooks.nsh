; Releases up to 2.8 installed the binary as vacs-client.exe. The template only checks for and
; removes the current binary, so a running old one would stay locked and be left behind.
!macro NSIS_HOOK_PREINSTALL
  !insertmacro CheckIfAppIsRunning "$INSTDIR\vacs-client.exe" "${PRODUCTNAME}"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\vacs-client.exe"
  RMDir "$INSTDIR"
!macroend
