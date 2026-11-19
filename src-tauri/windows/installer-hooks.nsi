; Release installers only (src-tauri/tauri.release.conf.json): put `tbx` on the user's PATH and take it off again.
;
; The work is done by tbx itself (`tbx self install`), the same code the app's Settings > Command line uses:
; it copies tbx.exe into a `bin` folder that holds nothing else (the app's own folder, with toybox.exe and
; uninstall.exe, stays off PATH) and adds that folder to the user's PATH once, however many times an update
; runs this. A failure here never fails the installer.

!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "$INSTDIR\tbx.exe" 0 +2
    nsExec::Exec '"$INSTDIR\tbx.exe" self install --quiet'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  IfFileExists "$INSTDIR\tbx.exe" 0 +2
    nsExec::Exec '"$INSTDIR\tbx.exe" self uninstall --quiet'
!macroend
