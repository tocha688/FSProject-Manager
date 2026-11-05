; 项目快搜 NSIS 安装钩子
; 目的：安装包与可执行文件用 ASCII 名（ProjectFinder），
; 但桌面/开始菜单快捷方式的显示名用中文「项目快搜」。
;
; Tauri 会把 {$PRODUCTNAME} 等变量注入，这里用自定义宏覆盖快捷方式名。

!macro NSIS_HOOK_POSTINSTALL
  ; 删除安装器默认创建的英文快捷方式
  Delete "$DESKTOP\${PRODUCTNAME}.lnk"
  Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"

  ; 重新创建中文名快捷方式
  CreateShortcut "$DESKTOP\项目快搜.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  CreateShortcut "$SMPROGRAMS\项目快搜.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"

  ; 开始菜单文件夹里也放一份
  CreateDirectory "$SMPROGRAMS\项目快搜"
  CreateShortcut "$SMPROGRAMS\项目快搜\项目快搜.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$DESKTOP\项目快搜.lnk"
  Delete "$SMPROGRAMS\项目快搜.lnk"
  RMDir /r "$SMPROGRAMS\项目快搜"
!macroend
