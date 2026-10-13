; Tauri owns its payload and uninstaller; Link owns the PATH shim and receipt.
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "nsDialogs.nsh"
!define ARCADE_ID "arcade.tools"
!define ARCADE_TOOLS_HELPER "${__FILEDIR__}\arcade-integration.ps1"
Var ArcadeToolsChannel
Var ArcadeToolsManagedBy
Var ArcadeToolsStartup
Var ArcadeToolsExit
Var ArcadeToolsOutput
Var ArcadeToolsOptions
Var ArcadeToolsLoginCheckbox
Var ArcadeToolsLoginChoice
Page custom ArcadeToolsOptionsCreate ArcadeToolsOptionsLeave
!macro NSIS_HOOK_PREINSTALL
  ${If} $INSTDIR == "$LOCALAPPDATA\Arcade Tools"
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\Arcade Tools"
  ${EndIf}
!macroend
Function ArcadeToolsOptionsCreate
  ${If} $INSTDIR == "$LOCALAPPDATA\Arcade Tools"
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\Arcade Tools"
  ${EndIf}
  nsDialogs::Create 1018
  Pop $ArcadeToolsOptions
  ${NSD_CreateCheckbox} 0 20u 100% 12u "Start Arcade Tools at login (hosts app tray icons)"
  Pop $ArcadeToolsLoginCheckbox
  ${NSD_Check} $ArcadeToolsLoginCheckbox
  nsDialogs::Show
FunctionEnd
Function ArcadeToolsOptionsLeave
  ${NSD_GetState} $ArcadeToolsLoginCheckbox $ArcadeToolsLoginChoice
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File "${ARCADE_TOOLS_HELPER}"
  StrCpy $ArcadeToolsChannel "stable"
  StrCpy $ArcadeToolsManagedBy "self"
  StrCpy $ArcadeToolsStartup "none"
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/ARCADECHANNEL=" $1
  ${IfNot} ${Errors}
    StrCpy $ArcadeToolsChannel $1
  ${EndIf}
  ClearErrors
  ${GetOptions} $0 "/ARCADEMANAGEDBY=" $1
  ${IfNot} ${Errors}
    StrCpy $ArcadeToolsManagedBy $1
  ${EndIf}
  ; Hosting defaults on, so login defaults on. Both UI and /S honor the override.
  StrCpy $2 "1"
  ${If} $ArcadeToolsLoginChoice != ""
    StrCpy $2 $ArcadeToolsLoginChoice
  ${EndIf}
  ClearErrors
  ${GetOptions} $0 "/STARTATLOGIN=" $1
  ${IfNot} ${Errors}
    StrCpy $2 $1
  ${EndIf}
  ${If} $2 == "1"
    CreateShortcut "$SMSTARTUP\Arcade Tools.lnk" "$INSTDIR\arcade-tools.exe" "--background"
    StrCpy $ArcadeToolsStartup "$SMSTARTUP\Arcade Tools.lnk"
  ${EndIf}
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy RemoteSigned -File "$INSTDIR\arcade-integration.ps1" -Action Install -Id "arcade.tools" -CliName "arcade-tools" -Version "${VERSION}" -Channel "$ArcadeToolsChannel" -ManagedBy "$ArcadeToolsManagedBy" -ExePath "$INSTDIR\arcade-tools.exe" -DesktopEntry "$SMPROGRAMS\Arcade Tools.lnk" -Autostart "$ArcadeToolsStartup" -Uninstaller "$INSTDIR\uninstall.exe"'
  Pop $ArcadeToolsExit
  Pop $ArcadeToolsOutput
  ${If} $ArcadeToolsExit != 0
    SetErrorLevel $ArcadeToolsExit
    Abort "Arcade Tools integration failed."
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  SetShellVarContext current
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy RemoteSigned -File "$INSTDIR\arcade-integration.ps1" -Action Uninstall -Id "arcade.tools" -CliName "arcade-tools" -ExePath "$INSTDIR\arcade-tools.exe"'
  Pop $ArcadeToolsExit
  Pop $ArcadeToolsOutput
  ${If} $ArcadeToolsExit != 0
  ${AndIf} $ArcadeToolsExit != 10
    SetErrorLevel $ArcadeToolsExit
    Abort "Arcade Tools uninstall integration failed."
  ${EndIf}
  Delete "$INSTDIR\arcade-integration.ps1"
!macroend
