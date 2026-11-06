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
        FindWindow $0 "" "ZaborUpdaterBetaInstall"
        ${if} $0 == 0
          InitPluginsDir
          File /oname=$PLUGINSDIR\ZaborUpdaterBeta.exe "${BUILD_RESOURCES_DIR}\ZaborUpdater.exe"
          System::Call 'kernel32::GetCurrentProcessId() i .r2'
          Exec '"$PLUGINSDIR\ZaborUpdaterBeta.exe" 0 beta $2'
          StrCpy $3 0
          ${Do}
            FindWindow $0 "" "ZaborUpdaterBetaInstall"
            System::Call 'user32::IsWindowVisible(p r0) i .r4'
            ${If} $4 != 0
              ${ExitDo}
            ${EndIf}
            IntOp $3 $3 + 1
            ${If} $3 >= 100
              ${ExitDo}
            ${EndIf}
            Sleep 50
          ${Loop}
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
  SetShellVarContext all
  Delete "$SMPROGRAMS\ZABOR Beta (legacy dsp).lnk"
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
  CreateShortCut "$SMPROGRAMS\ZABOR Beta (legacy dsp).lnk" "$INSTDIR\${APP_EXECUTABLE_FILENAME}" "--legacy-dsp" "$INSTDIR\${APP_EXECUTABLE_FILENAME}" 0
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${UNINSTALL_APP_KEY}" "InstallLocation" "$INSTDIR"
  ${if} ${isUpdated}
    SetAutoClose true
    ${if} ${Silent}
      ${StdUtils.ExecShellAsUser} $0 "$INSTDIR\${APP_EXECUTABLE_FILENAME}" "open" "--updated"
      ${if} $0 != 0
        ExecShell "open" "$INSTDIR\${APP_EXECUTABLE_FILENAME}" "--updated" SW_SHOWNORMAL
      ${endif}
    ${endif}
  ${endif}
!macroend
