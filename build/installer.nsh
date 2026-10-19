!ifndef BUILD_UNINSTALLER
  Var /GLOBAL KeepAppDataOnUpdate
!endif
!ifdef BUILD_UNINSTALLER
  Var /GLOBAL shouldDeleteAppData
!endif

!macro DeleteAppData
  SetShellVarContext current
  
  nsExec::Exec 'taskkill /F /IM ZABOR.exe /T'
  nsExec::Exec 'taskkill /F /IM zabor-desktop.exe /T'
  nsExec::Exec 'taskkill /F /IM ${PRODUCT_NAME}.exe /T'
  Sleep 1000

  RMDir /r "$APPDATA\zabor-desktop"
  RMDir /r "$LOCALAPPDATA\zabor-desktop"
  RMDir /r "$APPDATA\zabor"
  RMDir /r "$APPDATA\ZABOR"
  RMDir /r "$APPDATA\${PRODUCT_NAME}"
  RMDir /r "$LOCALAPPDATA\zabor"
  RMDir /r "$LOCALAPPDATA\ZABOR"
  RMDir /r "$LOCALAPPDATA\${PRODUCT_NAME}"
  RMDir /r "$LOCALAPPDATA\Temp\zabor*"
  RMDir /r "$LOCALAPPDATA\Temp\${PRODUCT_NAME}*"

  ; Удаление данных для всех пользователей на компьютере (полезно при установке от имени администратора)
  GetFullPathName $R7 "$PROFILE\.."
  ClearErrors
  FindFirst $R8 $R9 "$R7\*"
  _loop:
    IfErrors _done
    StrCmp $R9 "." _next
    StrCmp $R9 ".." _next
    
    RMDir /r "$R7\$R9\AppData\Roaming\zabor-desktop"
    RMDir /r "$R7\$R9\AppData\Local\zabor-desktop"
    RMDir /r "$R7\$R9\AppData\Roaming\zabor"
    RMDir /r "$R7\$R9\AppData\Roaming\ZABOR"
    RMDir /r "$R7\$R9\AppData\Roaming\${PRODUCT_NAME}"
    RMDir /r "$R7\$R9\AppData\Local\zabor"
    RMDir /r "$R7\$R9\AppData\Local\ZABOR"
    RMDir /r "$R7\$R9\AppData\Local\${PRODUCT_NAME}"
    RMDir /r "$R7\$R9\AppData\Local\Temp\zabor*"
    RMDir /r "$R7\$R9\AppData\Local\Temp\${PRODUCT_NAME}*"

  _next:
    FindNext $R8 $R9
    Goto _loop
  _done:
    FindClose $R8

  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCT_NAME}"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ZABOR"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "zabor-desktop"

  DeleteRegKey HKCU "Software\ZABOR"
  DeleteRegKey HKLM "Software\ZABOR"
  DeleteRegKey HKCU "Software\${PRODUCT_NAME}"
  DeleteRegKey HKLM "Software\${PRODUCT_NAME}"
  DeleteRegKey HKCU "Software\zabor-desktop"
  
  SetShellVarContext all
!macroend

!macro preInit
  !ifndef BUILD_UNINSTALLER
    ${if} ${Silent}
      ${GetParameters} $R0
      ${GetOptions} $R0 "--updated" $R1
      ${IfNot} ${Errors}
        ; Запуск окна прогресса ZaborUpdater в самом начале работы инсталлятора, до любых проверок
        FindWindow $0 "" "ZaborUpdater"
        ${if} $0 == 0
          InitPluginsDir
          File /oname=$PLUGINSDIR\ZaborUpdater.exe "${BUILD_RESOURCES_DIR}\ZaborUpdater.exe"
          Exec '"$PLUGINSDIR\ZaborUpdater.exe"'
        ${endif}
      ${EndIf}
    ${endif}
  !endif
!macroend

!macro customCheckAppRunning
  ; Подавление фонового вызова PowerShell из NSIS и быстрое завершение приложения через taskkill
  nsExec::Exec 'taskkill /F /IM ZABOR.exe /T'
  nsExec::Exec 'taskkill /F /IM zabor-desktop.exe /T'
  nsExec::Exec 'taskkill /F /IM ${PRODUCT_NAME}.exe /T'
  Sleep 500
!macroend

!macro customInit
  !ifndef BUILD_UNINSTALLER
    ${if} ${isUpdated}
      ${if} ${Silent}
        ; Запуск окна прогресса ZaborUpdater, если оно еще не открыто
        FindWindow $0 "" "ZaborUpdater"
        ${if} $0 == 0
          InitPluginsDir
          File /oname=$PLUGINSDIR\ZaborUpdater.exe "${BUILD_RESOURCES_DIR}\ZaborUpdater.exe"
          Exec '"$PLUGINSDIR\ZaborUpdater.exe"'
        ${endif}
      ${endif}
    ${endif}
  !endif

  ; === ПРОВЕРКА РЕЖИМА ОБНОВЛЕНИЯ ===
  ${if} ${isUpdated}
    nsExec::Exec 'taskkill /F /IM ZABOR.exe /T'
    nsExec::Exec 'taskkill /F /IM zabor-desktop.exe /T'
    nsExec::Exec 'taskkill /F /IM ${PRODUCT_NAME}.exe /T'
    Sleep 1000
    Goto _zabor_init_done
  ${endif}

  ; === РУЧНАЯ УСТАНОВКА ПОВЕРХ СТАРОЙ ВЕРСИИ ===
  StrCpy $R2 ""

  ; 1) per-machine
  ReadRegStr $R0 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "QuietUninstallString"
  StrCmp $R0 "" _zabor_try_user 0
  ReadRegStr $R2 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "InstallLocation"
  Goto _zabor_run_uninstall

  _zabor_try_user:
  ; 2) per-user
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "QuietUninstallString"
  StrCmp $R0 "" _zabor_try_name 0
  ReadRegStr $R2 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "InstallLocation"
  Goto _zabor_run_uninstall

  _zabor_try_name:
  ReadRegStr $R0 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}" "QuietUninstallString"
  StrCmp $R0 "" _zabor_try_name_user 0
  ReadRegStr $R2 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}" "InstallLocation"
  Goto _zabor_run_uninstall

  _zabor_try_name_user:
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}" "QuietUninstallString"
  StrCmp $R0 "" _zabor_try_us 0
  ReadRegStr $R2 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}" "InstallLocation"
  Goto _zabor_run_uninstall

  _zabor_try_us:
  ReadRegStr $R0 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "UninstallString"
  StrCmp $R0 "" _zabor_try_us_user 0
  ReadRegStr $R2 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "InstallLocation"
  StrCpy $R0 "$R0 /S"
  Goto _zabor_run_uninstall

  _zabor_try_us_user:
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "UninstallString"
  StrCmp $R0 "" _zabor_init_done 0
  ReadRegStr $R2 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "InstallLocation"
  StrCpy $R0 "$R0 /S"
  Goto _zabor_run_uninstall

  _zabor_run_uninstall:
  ${GetParameters} $R3
  ${GetOptions} $R3 "--delete-app-data" $R4
  ${IfNot} ${Errors}
    Goto _delete_data
  ${EndIf}
  ${GetOptions} $R3 "--keep-app-data" $R4
  ${IfNot} ${Errors}
    Goto _keep_data
  ${EndIf}

  ${IfNot} ${Silent}
    MessageBox MB_YESNO|MB_ICONQUESTION "Обнаружена предыдущая версия.$\nСохранить ваши локальные данные?$\n$\nНажмите 'Да', чтобы оставить.$\nНажмите 'Нет', чтобы полностью удалить конфигурацию." /SD IDYES IDYES _keep_data IDNO _delete_data
  ${Else}
    Goto _keep_data
  ${EndIf}
  
  _keep_data:
  StrCpy $KeepAppDataOnUpdate "1" 
  StrCpy $R0 "$R0 /S --keep-app-data"
  nsExec::Exec 'taskkill /F /IM ZABOR.exe /T'
  nsExec::Exec 'taskkill /F /IM zabor-desktop.exe /T'
  nsExec::Exec 'taskkill /F /IM ${PRODUCT_NAME}.exe /T'
  Sleep 1000
  RMDir /r "$TEMP\zabor_backup"
  CreateDirectory "$TEMP\zabor_backup\ZABOR"
  CopyFiles /SILENT "$APPDATA\ZABOR\*.*" "$TEMP\zabor_backup\ZABOR"
  CreateDirectory "$TEMP\zabor_backup\zabor-desktop"
  CopyFiles /SILENT "$APPDATA\zabor-desktop\*.*" "$TEMP\zabor_backup\zabor-desktop"
  Goto _run_now
  
  _delete_data:
  StrCpy $KeepAppDataOnUpdate "0"
  StrCpy $R0 "$R0 /S --delete-app-data"
  Goto _run_now

  _run_now:
  StrCmp $R2 "" _run_now_standard
  StrCpy $R0 '$R0 _?=$R2'
  _run_now_standard:

  ExecWait $R0 $R1

  Sleep 1000
  RMDir /r "$PROGRAMFILES64\${PRODUCT_NAME}"
  RMDir /r "$LOCALAPPDATA\Programs\${PRODUCT_NAME}"

  ${If} $KeepAppDataOnUpdate == "0"
    !insertmacro DeleteAppData
  ${EndIf}

  ${If} $KeepAppDataOnUpdate == "1"
    CreateDirectory "$APPDATA\ZABOR"
    CopyFiles /SILENT "$TEMP\zabor_backup\ZABOR\*.*" "$APPDATA\ZABOR"
    CreateDirectory "$APPDATA\zabor-desktop"
    CopyFiles /SILENT "$TEMP\zabor_backup\zabor-desktop\*.*" "$APPDATA\zabor-desktop"
    RMDir /r "$TEMP\zabor_backup"
  ${EndIf}

  _zabor_init_done:
!macroend

!macro customUnInstallCheck
  ClearErrors
!macroend

!macro customUnInstallCheckCurrentUser
  ClearErrors
!macroend

!macro customUnInstall
  StrCpy $shouldDeleteAppData "0"

  ; Проверяем консольные флаги, переданные инсталлятором (во время установки новой версии)
  ${GetParameters} $R0
  ${GetOptions} $R0 "--delete-app-data" $R1
  ${IfNot} ${Errors}
    StrCpy $shouldDeleteAppData "1"
  ${Else}
    ${GetOptions} $R0 "--keep-app-data" $R1
    ${IfNot} ${Errors}
      StrCpy $shouldDeleteAppData "0"
    ${Else}
      ; Запущено стандартное удаление пользователем
      ${IfNot} ${Silent}
        MessageBox MB_YESNO|MB_ICONQUESTION "Удалить локальные данные пользователя?" /SD IDNO IDYES _delete_un IDNO _keep_un
        Goto _continue_un
        _delete_un:
          StrCpy $shouldDeleteAppData "1"
          Goto _continue_un
        _keep_un:
          StrCpy $shouldDeleteAppData "0"
        _continue_un:
      ${EndIf}
    ${EndIf}
  ${EndIf}

  ${If} $shouldDeleteAppData == "1"
    !insertmacro DeleteAppData
  ${EndIf}
!macroend

!macro customInstall
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "InstallLocation" "$INSTDIR"
  ${if} ${isUpdated}
    SetAutoClose true
    ${if} ${Silent}
      nsExec::Exec 'taskkill /F /IM ZaborUpdater.exe /T'
      nsExec::Exec 'taskkill /F /FI "WINDOWTITLE eq ZaborUpdater" /T'
      ${StdUtils.ExecShellAsUser} $0 "$INSTDIR\ZABOR.exe" "open" "--updated"
      ${if} $0 != 0
        ExecShell "open" "$INSTDIR\ZABOR.exe" "--updated" SW_SHOWNORMAL
      ${endif}
    ${endif}
  ${endif}
!macroend

!macro customFinishPage
  Function StartApp
    nsExec::Exec 'taskkill /F /IM ZaborUpdater.exe /T'
    nsExec::Exec 'taskkill /F /FI "WINDOWTITLE eq ZaborUpdater" /T'
    ${if} ${isUpdated}
      StrCpy $1 "--updated"
    ${else}
      StrCpy $1 ""
    ${endif}
    ${if} "$launchLink" != ""
      ${StdUtils.ExecShellAsUser} $0 "$launchLink" "open" "$1"
    ${else}
      ${StdUtils.ExecShellAsUser} $0 "$INSTDIR\${APP_EXECUTABLE_FILENAME}" "open" "$1"
    ${endif}
    ${if} $0 != 0
      ${if} "$launchLink" != ""
        ExecShell "open" "$launchLink" "$1"
      ${else}
        ExecShell "open" "$INSTDIR\${APP_EXECUTABLE_FILENAME}" "$1"
      ${endif}
    ${endif}
  FunctionEnd

  Function finishPagePre
    ${if} ${isUpdated}
      Call StartApp
      Abort
    ${endif}
  FunctionEnd

  !define MUI_PAGE_CUSTOMFUNCTION_PRE finishPagePre
  !define MUI_FINISHPAGE_RUN
  !define MUI_FINISHPAGE_RUN_FUNCTION "StartApp"
  !insertmacro MUI_PAGE_FINISH
!macroend