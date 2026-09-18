Var ConsoleRenameMigration

!macro NSIS_HOOK_PREINSTALL
  StrCpy $ConsoleRenameMigration 0

  ; 仅在用户保留 Console 默认目录时，接管 v0.0.7 Runner 的同一份安装。
  ${If} $INSTDIR == "$LOCALAPPDATA\${PRODUCTNAME}"
    ReadRegStr $0 SHCTX "Software\github\Arknights Operation Runner" ""
    ReadRegStr $1 SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\Arknights Operation Runner" "UninstallString"
    ${If} $0 != ""
    ${AndIf} $1 == "$\"$0\uninstall.exe$\""
    ${AndIf} ${FileExists} "$0\${MAINBINARYNAME}.exe"
      StrCpy $INSTDIR $0
      SetOutPath $INSTDIR
      StrCpy $ConsoleRenameMigration 1
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $ConsoleRenameMigration = 1
  ${AndIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    ; 新 Console 卸载信息写入成功后，才移除同一路径的旧 Runner 入口。
    DeleteRegKey SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\Arknights Operation Runner"
    DeleteRegKey SHCTX "Software\github\Arknights Operation Runner"

    !insertmacro IsShortcutTarget "$SMPROGRAMS\Arknights Operation Runner.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      Delete "$SMPROGRAMS\Arknights Operation Runner.lnk"
    ${EndIf}

    !insertmacro IsShortcutTarget "$DESKTOP\Arknights Operation Runner.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      Delete "$DESKTOP\Arknights Operation Runner.lnk"
    ${EndIf}
  ${EndIf}
!macroend
