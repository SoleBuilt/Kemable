; Paboable's additions to Tauri's NSIS installer (docs/decisions/0019-data-folder.md).
; Tauri includes this file before its pages, so a page setting defined here
; reaches the page it belongs to.

; The terms are accepted with a checkbox before Next lights up. The page shows one
; sentence and the two links (terms.txt); its addresses are those of `open_terms` in
; src/commands.rs, and change with them.
!define MUI_LICENSEPAGE_CHECKBOX

!macro NSIS_HOOK_POSTINSTALL
  ; Everything the app remembers lives in data\ beside it; the app finds it there.
  CreateDirectory "$INSTDIR\data"
  ; The terms page showed and was accepted: the app does not ask again for this
  ; version. A silent, passive or update install skips the page, so it marks nothing.
  ${IfNot} ${Silent}
  ${AndIf} $PassiveMode <> 1
  ${AndIf} $UpdateMode <> 1
    FileOpen $0 "$INSTDIR\data\terms-accepted.txt" w
    FileWrite $0 "${VERSION}"
    FileClose $0
  ${EndIf}
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
