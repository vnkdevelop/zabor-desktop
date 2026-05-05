!macro customInit
  ; === УДАЛЕНИЕ СТАРОЙ ВЕРСИИ (АВТОМАТИЧЕСКИ) ===

  ; 1) per-machine
  ReadRegStr $R0 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "QuietUninstallString"
  StrCmp $R0 "" _zabor_try_user 0
  Goto _zabor_run_uninstall

  _zabor_try_user:
  ; 2) per-user
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "QuietUninstallString"
  StrCmp $R0 "" _zabor_try_name 0
  Goto _zabor_run_uninstall

  _zabor_try_name:
  ReadRegStr $R0 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}" "QuietUninstallString"
  StrCmp $R0 "" _zabor_try_name_user 0
  Goto _zabor_run_uninstall

  _zabor_try_name_user:
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}" "QuietUninstallString"
  StrCmp $R0 "" _zabor_try_us 0
  Goto _zabor_run_uninstall

  _zabor_try_us:
  ReadRegStr $R0 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "UninstallString"
  StrCmp $R0 "" _zabor_try_us_user 0
  StrCpy $R0 "$R0 /S"
  Goto _zabor_run_uninstall

  _zabor_try_us_user:
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "UninstallString"
  StrCmp $R0 "" _zabor_init_done 0
  StrCpy $R0 "$R0 /S"
  Goto _zabor_run_uninstall

  _zabor_run_uninstall:
  ; Спрашиваем пользователя: хочет ли он сохранить свои данные при обновлении
  Var /GLOBAL KeepAppDataOnUpdate
  MessageBox MB_YESNO|MB_ICONQUESTION "Обнаружена предыдущая версия.$\nСохранить ваши локальные данные?$\n$\nНажмите 'Да', чтобы оставить.$\nНажмите 'Нет', чтобы полностью удалить конфигурацию." IDYES _keep_data IDNO _delete_data
  
  _keep_data:
  StrCpy $KeepAppDataOnUpdate "1" 
  StrCpy $R0 "$R0 /S --keep-app-data"
  Goto _run_now
  
  _delete_data:
  StrCpy $KeepAppDataOnUpdate "0"
  StrCpy $R0 "$R0 /S --delete-app-data"
  Goto _run_now

  _run_now:
  ; Тихое удаление
  ExecWait $R0 $R1
  StrCmp $R1 "" 0 _zabor_wait
  nsExec::ExecToLog 'cmd /c $R0'

  _zabor_wait:
  Sleep 3000
  RMDir /r "$PROGRAMFILES64\${PRODUCT_NAME}"
  RMDir /r "$LOCALAPPDATA\Programs\${PRODUCT_NAME}"

  _zabor_init_done:
!macroend

!macro customUnInstall
  Var /GLOBAL shouldDeleteAppData
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
        MessageBox MB_YESNO|MB_ICONQUESTION "Удалить локальные данные пользователя?" IDYES _delete_un IDNO _keep_un
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
    SetShellVarContext current
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
    SetShellVarContext all
  ${EndIf}

  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCT_NAME}"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ZABOR"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "zabor-desktop"

  DeleteRegKey HKCU "Software\ZABOR"
  DeleteRegKey HKLM "Software\ZABOR"
  DeleteRegKey HKCU "Software\${PRODUCT_NAME}"
  DeleteRegKey HKLM "Software\${PRODUCT_NAME}"
  DeleteRegKey HKCU "Software\zabor-desktop"
!macroend