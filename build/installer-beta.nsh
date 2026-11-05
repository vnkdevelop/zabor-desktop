!ifndef BUILD_UNINSTALLER
  Var /GLOBAL KeepAppDataOnUpdate
!endif
!ifdef BUILD_UNINSTALLER
  Var /GLOBAL shouldDeleteAppData
!endif

!macro DeleteAppData
  SetShellVarContext current
  RMDir /r "$APPDATA\zabor-desktop-beta"
  RMDir /r "$LOCALAPPDATA\zabor-desktop-beta"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "zabor-desktop-beta"
  SetShellVarContext all
!macroend

!macro preInit
  !ifndef BUILD_UNINSTALLER
    ${if} ${Silent}
      ${GetParameters} $R0
      ${GetOptions} $R0 "--updated" $R1
      ${IfNot} ${Errors}
        FindWindow $0 "" "ZaborUpdaterBeta"
        ${if} $0 == 0
          InitPluginsDir
          File /oname=$PLUGINSDIR\ZaborUpdaterBeta.exe "${BUILD_RESOURCES_DIR}\ZaborUpdater.exe"
          Exec '"$PLUGINSDIR\ZaborUpdaterBeta.exe" 0 beta'
        ${endif}
      ${EndIf}
    ${endif}
  !endif
!macroend

!macro customCheckAppRunning
  nsExec::Exec 'taskkill /F /IM ZABOR-Beta.exe /T'
  Sleep 500
!macroend

!macro customInit
  !ifndef BUILD_UNINSTALLER
    StrCpy $KeepAppDataOnUpdate "1"
  !endif
!macroend

!macro customUnInstallCheck
  ClearErrors
!macroend

!macro customUnInstallCheckCurrentUser
  ClearErrors
!macroend

!macro customUnInstall
  StrCpy $shouldDeleteAppData "0"
  ${GetParameters} $R0
  ${GetOptions} $R0 "--delete-app-data" $R1
  ${IfNot} ${Errors}
    StrCpy $shouldDeleteAppData "1"
  ${Else}
    ${GetOptions} $R0 "--keep-app-data" $R1
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
      nsExec::Exec 'taskkill /F /IM ZaborUpdaterBeta.exe /T'
      ${StdUtils.ExecShellAsUser} $0 "$INSTDIR\${APP_EXECUTABLE_FILENAME}" "open" "--updated"
      ${if} $0 != 0
        ExecShell "open" "$INSTDIR\${APP_EXECUTABLE_FILENAME}" "--updated" SW_SHOWNORMAL
      ${endif}
    ${endif}
  ${endif}
!macroend
