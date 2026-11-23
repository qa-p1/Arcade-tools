; Tauri NSIS hooks backed by Arcade Link's checksum-pinned shared macros.
; The adapted installer.nsi installs to %LOCALAPPDATA%\Programs\Arcade Tools
; from .onInit, so a plain /S and the wizard use the same folder.
!define ARCADE_ID "arcade.tools"
!define ARCADE_NAME "${PRODUCTNAME}"
!define ARCADE_VERSION "${VERSION}"
!define ARCADE_EXE "${MAINBINARYNAME}.exe"
!define ARCADE_CLI "arcade-tools"
!define ARCADE_HOMEPAGE "https://github.com/qa-p1/Arcade-catalog"
; Tools hosts the apps' tray icons, so start at login defaults on.
!define ARCADE_START_AT_LOGIN_DEFAULT 1
!include "${__FILEDIR__}\arcade.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ; Writes the shared receipt, Start menu/optional shortcuts, CLI shim, PATH
  ; and Apps & features entry.
  !insertmacro ArcadeInstall
  ; Tools 0.1 put its Start menu shortcut at the top level; the shared one
  ; lives in a folder. Remove the old one only when it targets this copy.
  !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 = 1
    Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  ${EndIf}
!macroend

; The Arcade helper (arcade-helper.exe, Arcade's actions in Explorer and the browser) runs from this folder, and
; Windows can't replace or delete a running program. Tools' own self-update stops it once its jobs are done; a
; manual reinstall or an uninstall stops it regardless.
!macro ArcadeStopHelper
  ${If} ${FileExists} "$INSTDIR\arcade-helper.exe"
    nsExec::Exec '"$INSTDIR\arcade-helper.exe" quit --force'
    Pop $R9
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro ArcadeStopHelper
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Updates keep shared integration in place; normal uninstall is receipt-owned.
  ${If} $UpdateMode == 1
    StrCpy $ArcadeExitCode 0
  ${Else}
    ; Its Explorer menu, browser connections and card go first, while it can still say what it wrote.
    ${If} ${FileExists} "$INSTDIR\arcade-helper.exe"
      nsExec::Exec '"$INSTDIR\arcade-helper.exe" integrate --remove'
      Pop $R9
    ${EndIf}
    !insertmacro ArcadeUninstall
  ${EndIf}
  !insertmacro ArcadeStopHelper
!macroend
