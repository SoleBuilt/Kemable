; Paboable's additions to Tauri's NSIS installer (docs/decisions/0019-data-folder.md).
; The terms are asked in the app, not here (src/lib/shell/TermsDialog.svelte).

!macro NSIS_HOOK_POSTINSTALL
  ; Everything the app remembers lives in data\ beside it; the app finds it there.
  CreateDirectory "$INSTDIR\data"
  ; Installed for every user, the folder is the admin's; the app runs without admin
  ; rights, so the users of this PC may change what is in it (BUILTIN\Users, by SID).
  ${If} $MultiUser.InstallMode == "AllUsers"
    nsExec::ExecToLog 'icacls "$INSTDIR\data" /grant *S-1-5-32-545:(OI)(CI)M /T /Q'
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; The data stays, so a reinstall finds it, unless "Delete the application data"
  ; was ticked; an update never deletes it.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /r "$INSTDIR\data"
    RMDir "$INSTDIR"
  ${EndIf}
!macroend
