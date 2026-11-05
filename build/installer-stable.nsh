!ifdef BUILD_UNINSTALLER
  Var /GLOBAL shouldDeleteAppData
!endif

!macro DeleteAppData
  SetShellVarContext current
  RMDir /r "$APPDATA\zabor-desktop"
  RMDir /r "$APPDATA\ZABOR"
  RMDir /r "$LOCALAPPDATA\zabor-desktop"
  RMDir /r "$LOCALAPPDATA\ZABOR"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ZABOR"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "zabor-desktop"
  SetShellVarContext all
!macroend

!macro preInit
  !ifndef BUILD_UNINSTALLER
    ${if} ${Silent}
      ${GetParameters} $R0
      ${GetOptions} $R0 "--updated" $R1
      ${IfNot} ${Errors}
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
  nsExec::Exec 'taskkill /F /IM ${APP_EXECUTABLE_FILENAME} /T'
  Sleep 500
!macroend

!macro customUnInstallCheck
  ClearErrors
!macroend

!macro customUnInstallCheckCurrentUser
  ClearErrors
!macroend

!macro customUnInstall
  SetShellVarContext all
  Delete "$SMPROGRAMS\ZABOR (legacy dsp).lnk"
  StrCpy $shouldDeleteAppData "0"
  ${GetParameters} $R0
  ${GetOptions} $R0 "--delete-app-data" $R1
  ${IfNot} ${Errors}
    StrCpy $shouldDeleteAppData "1"
  ${EndIf}
  ${If} $shouldDeleteAppData == "1"
    !insertmacro DeleteAppData
  ${EndIf}
!macroend

!macro customInstall
  CreateShortCut "$SMPROGRAMS\ZABOR (legacy dsp).lnk" "$INSTDIR\${APP_EXECUTABLE_FILENAME}" "--legacy-dsp" "$INSTDIR\${APP_EXECUTABLE_FILENAME}" 0
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "InstallLocation" "$INSTDIR"
  ${if} ${isUpdated}
    SetAutoClose true
    ${if} ${Silent}
      nsExec::Exec 'taskkill /F /IM ZaborUpdater.exe /T'
      ${StdUtils.ExecShellAsUser} $0 "$INSTDIR\${APP_EXECUTABLE_FILENAME}" "open" "--updated"
      ${if} $0 != 0
        ExecShell "open" "$INSTDIR\${APP_EXECUTABLE_FILENAME}" "--updated" SW_SHOWNORMAL
      ${endif}
    ${endif}
  ${endif}
!macroend
