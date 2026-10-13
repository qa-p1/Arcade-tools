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

!macro NSIS_HOOK_PREUNINSTALL
  ; Updates keep shared integration in place; normal uninstall is receipt-owned.
  ${If} $UpdateMode == 1
    StrCpy $ArcadeExitCode 0
  ${Else}
    !insertmacro ArcadeUninstall
  ${EndIf}
!macroend
