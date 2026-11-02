; Hooks for the Tauri NSIS installer (bundle.windows.nsis.installerHooks).
;
; The installer asks for a language in its first dialog. Pass that choice to the app:
; write "ru" or "en" to a marker file next to settings.json. The app reads it on the
; next launch and removes it (see settings::load). NSIS language IDs: 1049 Russian, 1033 English.

; Modern UI remembers the language in the registry (HKCU\Software\<publisher>\StopPic) and by default
; shows the dialog only on the very first install. Ask every time: the remembered language is preselected.
!define MUI_LANGDLL_ALWAYSSHOW

!macro NSIS_HOOK_POSTINSTALL
  StrCpy $R0 "en"
  IntCmp $LANGUAGE 1049 0 +2 +2
    StrCpy $R0 "ru"
  CreateDirectory "$APPDATA\${BUNDLEID}"
  ClearErrors
  FileOpen $R1 "$APPDATA\${BUNDLEID}\install-language" w
  IfErrors +3
    FileWrite $R1 $R0
    FileClose $R1
!macroend
