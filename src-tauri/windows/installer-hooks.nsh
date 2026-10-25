; FinkMeter NSIS 安装器扩展：安装时选择数据存储位置
;
; 行为：
; - 安装向导第一步询问数据目录，默认值与应用内置默认目录一致（%APPDATA%\FinkMeter）
; - 留空或直接下一步表示使用默认目录，不写任何配置
; - 选择自定义目录时，安装完成后写入 %APPDATA%\com.finkmeter.app\storage.json
;   应用启动时会读取该文件；安装后在「设置 → 数据存储」中仍可修改或恢复默认

!include "nsDialogs.nsh"
!include "LogicLib.nsh"

Var FinkMeterDataDirInput
Var FinkMeterDataDirBrowse
Var FINKMETER_DATA_DIR

; 把路径中的反斜杠替换为正斜杠：JSON 字符串里反斜杠必须转义，
; 而 Windows 与 Rust 都能正常识别正斜杠路径，避免额外的转义处理
Function FinkMeterSlashPath
  ; 输入 $0，输出 $1
  StrCpy $1 ""
  StrLen $2 $0
  StrCpy $3 0
  ${Do}
    ${If} $3 >= $2
      ${ExitDo}
    ${EndIf}
    StrCpy $4 $0 1 $3
    ${If} $4 == "\"
      StrCpy $4 "/"
    ${EndIf}
    StrCpy $1 "$1$4"
    IntOp $3 $3 + 1
  ${Loop}
FunctionEnd

; 语言 ID 直接使用数值（2052 简体中文 / 1033 英语），避免依赖 MUI_LANGUAGE 的加载顺序
LangString FM_DATADIR_TITLE 2052 "数据存储位置"
LangString FM_DATADIR_TITLE 1033 "Data storage location"

LangString FM_DATADIR_SUBTITLE 2052 "选择数据库、请求历史与压测报告的保存目录。"
LangString FM_DATADIR_SUBTITLE 1033 "Choose where the database, request history and stress test reports are stored."

LangString FM_DATADIR_DEFAULT_HINT 2052 "默认目录（留空即使用此项）："
LangString FM_DATADIR_DEFAULT_HINT 1033 "Default directory (leave empty to use this):"

LangString FM_DATADIR_TIP 2052 "安装后也可以在「设置 → 数据存储」中修改或恢复默认目录。"
LangString FM_DATADIR_TIP 1033 "You can change or reset this later in Settings - Data storage."

LangString FM_DATADIR_BROWSE 2052 "选择数据存储目录"
LangString FM_DATADIR_BROWSE 1033 "Select data storage folder"

LangString FM_DATADIR_NOWRITE 2052 "所选目录无法写入，请选择其他目录或留空使用默认目录。"
LangString FM_DATADIR_NOWRITE 1033 "The selected folder is not writable. Choose another folder or leave it empty to use the default."

Function FinkMeterDataDirPageCreate
  !insertmacro MUI_HEADER_TEXT "$(FM_DATADIR_TITLE)" "$(FM_DATADIR_SUBTITLE)"

  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 16u "$(FM_DATADIR_DEFAULT_HINT)"
  Pop $0

  ${NSD_CreateLabel} 0 18u 100% 16u "$APPDATA\FinkMeter"
  Pop $0

  ${NSD_CreateDirRequest} 0 44u 78% 13u ""
  Pop $FinkMeterDataDirInput
  ${NSD_SetText} $FinkMeterDataDirInput "$APPDATA\FinkMeter"

  ${NSD_CreateBrowseButton} 80% 44u 20% 13u "Browse..."
  Pop $FinkMeterDataDirBrowse
  ${NSD_OnClick} $FinkMeterDataDirBrowse FinkMeterBrowseDataDir

  ${NSD_CreateLabel} 0 68u 100% 32u "$(FM_DATADIR_TIP)"
  Pop $0

  nsDialogs::Show
FunctionEnd

Function FinkMeterBrowseDataDir
  ${NSD_GetText} $FinkMeterDataDirInput $0
  nsDialogs::SelectFolderDialog "$(FM_DATADIR_BROWSE)" "$0"
  Pop $0
  ${If} $0 != "error"
    ${NSD_SetText} $FinkMeterDataDirInput "$0"
  ${EndIf}
FunctionEnd

Function FinkMeterDataDirPageLeave
  ${NSD_GetText} $FinkMeterDataDirInput $0
  ${If} $0 == ""
    ; 留空：使用应用默认目录，不写配置
    StrCpy $FINKMETER_DATA_DIR ""
    Return
  ${EndIf}

  CreateDirectory "$0"
  ClearErrors
  FileOpen $1 "$0\.finkmeter-write-test" w
  ${If} $1 == ""
    MessageBox MB_ICONSTOP|MB_OK "$(FM_DATADIR_NOWRITE)"
    Abort
  ${EndIf}
  FileWrite $1 "ok"
  FileClose $1
  Delete "$0\.finkmeter-write-test"

  StrCpy $FINKMETER_DATA_DIR $0
FunctionEnd

; 数据目录选择页放在安装流程最前，语言选择对话框之后
Page custom FinkMeterDataDirPageCreate FinkMeterDataDirPageLeave

!macro NSIS_HOOK_POSTINSTALL
  ${If} $FINKMETER_DATA_DIR != ""
  ${AndIf} $FINKMETER_DATA_DIR != "$APPDATA\FinkMeter"
    ; 配置文件位于固定的应用配置目录，不随数据目录移动
    CreateDirectory "$APPDATA\com.finkmeter.app"

    ; JSON 不接受未转义的反斜杠，路径统一写成正斜杠
    StrCpy $0 $FINKMETER_DATA_DIR
    Call FinkMeterSlashPath
    StrCpy $0 $1

    FileOpen $1 "$APPDATA\com.finkmeter.app\storage.json" w
    ${If} $1 != ""
      FileWrite $1 '{"dataDir": "'
      FileWrite $1 $0
      FileWrite $1 '"}$\r$\n'
      FileClose $1
    ${EndIf}
  ${EndIf}
!macroend
