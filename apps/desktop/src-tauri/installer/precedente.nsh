; Rymembra - hook dell'installer NSIS per togliere la versione precedente.
;
; Il programma si chiamava in un altro modo e viveva in un'altra cartella, con
; un'altra voce in "App e funzionalità". Un'installazione di Rymembra non la
; toccherebbe mai (nome, cartella e chiavi di registro sono diversi), e
; l'utente si ritroverebbe con due programmi quasi uguali. Questo hook,
; agganciato in tauri.conf.json (bundle.windows.nsis.installerHooks), la
; disinstalla da solo, in silenzio, PRIMA di copiare i file nuovi.
;
; I DATI NON SI TOCCANO. Il disinstallatore di Tauri in modalità silenziosa
; non cancella %APPDATA%\<identificativo>: la casella "elimina i dati" vale 0
; quando non c'è la pagina di conferma. Quella cartella la prende Rymembra al
; primo avvio (trasloco in core/rymembra-app/src/trasloco.rs), quindi qui non
; si cancella niente che non sia il programma installato.
;
; QUESTO È L'UNICO FILE, fuori dalle costanti di eredità, in cui il nome di
; prima può comparire: serve a trovare la vecchia installazione e a dire
; all'utente quale programma chiudere.
;
; Dove gira: la macro NSIS_HOOK_PREINSTALL è espansa dal template di Tauri
; (stesso nome e stessa posizione nel template di cargo-tauri 2.11.4 e in
; quello del CLI npm 2.12.0) dentro `Section Install`, dopo `SetOutPath
; $INSTDIR` e prima di `CheckIfAppIsRunning` e di qualunque `File`. Questo file
; viene incluso PRIMA delle `!define` del template (PRODUCTNAME, INSTALLMODE,
; ...) e PRIMA delle `Var` (PassiveMode, ...): per questo non definisce
; funzioni, che non potrebbero ancora nominarle, e tiene tutta la logica
; dentro le macro, che vengono espanse quando ormai esistono tutte. La seconda,
; NSIS_HOOK_POSTINSTALL, sta in fondo alla stessa sezione, dopo i file e i
; collegamenti del template.
;
; Solo HKCU: l'installazione precedente era "currentUser" (nessun amministratore,
; cartella in %LOCALAPPDATA%), e il registro a 64 bit non c'entra con HKCU.

!ifndef PRECEDENTE_NOME
  !define PRECEDENTE_NOME "Aether"
  ; Voce di "App e funzionalità": contiene UninstallString e MainBinaryName.
  !define PRECEDENTE_CHIAVE_VOCE "Software\Microsoft\Windows\CurrentVersion\Uninstall\Aether"
  ; Chiave "produttore\prodotto": il valore predefinito è la cartella di installazione.
  !define PRECEDENTE_CHIAVE_CARTELLA "Software\Federico Baratti\Aether"
  ; Eseguibile da cercare fra i processi, se la voce non dice altro.
  !define PRECEDENTE_EXE "aether.exe"
  ; Secondi che aspettiamo, in modo passivo o silenzioso, che il vecchio
  ; programma si chiuda da solo (l'aggiornamento automatico lo sta chiudendo
  ; proprio in questo momento).
  !define PRECEDENTE_ATTESA_S 10
  ; Dove la versione precedente aveva i collegamenti (1 = c'era): li guarda
  ; NSIS_HOOK_PREINSTALL prima di disinstallarla, li usa NSIS_HOOK_POSTINSTALL.
  Var PrecedenteMenuStart
  Var PrecedenteDesktop
!endif

!macro NSIS_HOOK_PREINSTALL
  ; I registri $R0-$R4 servono a noi: il resto della sezione non deve
  ; accorgersene, quindi li salviamo sullo stack e li rimettiamo in fondo.
  ; Ogni percorso di uscita passa da `precedente_fine`, che li ripristina.
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  Push $R4

  ; 1. C'è una versione precedente? Se la voce non esiste (installazione
  ;    pulita, o un aggiornamento di Rymembra stesso) non c'è niente da fare.
  ;    UninstallString è scritta da Tauri GIÀ fra virgolette:
  ;    "C:\...\uninstall.exe". Qui serve solo a ritrovare la cartella; il
  ;    comando che poi si esegue lo ricostruiamo noi, più sotto.
  ReadRegStr $R0 HKCU "${PRECEDENTE_CHIAVE_VOCE}" "UninstallString"
  ${If} $R0 == ""
    Goto precedente_fine
  ${EndIf}

  ;    I collegamenti si guardano adesso, prima che il disinstallatore li tolga
  ;    (e anche se poi non si disinstalla niente): vedi NSIS_HOOK_POSTINSTALL.
  ${If} ${FileExists} "$SMPROGRAMS\${PRECEDENTE_NOME}.lnk"
    StrCpy $PrecedenteMenuStart 1
  ${EndIf}
  ${If} ${FileExists} "$DESKTOP\${PRECEDENTE_NOME}.lnk"
    StrCpy $PrecedenteDesktop 1
  ${EndIf}

  ; 2. In quale cartella? Il disinstallatore e la cartella per `_?=` devono
  ;    essere SEMPRE quelli dello stesso posto: un disinstallatore lanciato con
  ;    `_?=` puntato altrove lavorerebbe su una cartella che non è la sua. Per
  ;    questo la prima scelta è quella che dice la voce stessa: la cartella di
  ;    $R0, tolte le virgolette e il nome finale uninstall.exe (il confronto fra
  ;    stringhe di NSIS non distingue maiuscole e minuscole). Se la voce è
  ;    malformata, o quel disinstallatore non c'è più, si passa alle altre
  ;    fonti, nell'ordine: il valore predefinito della chiave produttore\prodotto
  ;    (come fa Tauri), poi la cartella solita in %LOCALAPPDATA%. Ognuna vale
  ;    solo se contiene uninstall.exe. Se nessuna lo contiene non abbiamo
  ;    nulla da eseguire: lasciamo stare, senza rompere l'installazione.
  StrCpy $R1 $R0
  StrCpy $R3 $R1 1
  ${If} $R3 == "$\""
    StrCpy $R1 $R1 "" 1
    StrCpy $R3 $R1 1 -1
    ${If} $R3 == "$\""
      StrCpy $R1 $R1 -1
    ${EndIf}
  ${EndIf}
  ; "\uninstall.exe" sono 14 caratteri: se la voce ne ha meno non può finire così.
  StrLen $R4 $R1
  ${If} $R4 >= 14
    StrCpy $R3 $R1 14 -14
    ${If} $R3 == "\uninstall.exe"
      StrCpy $R1 $R1 -14
    ${Else}
      StrCpy $R1 ""
    ${EndIf}
  ${Else}
    StrCpy $R1 ""
  ${EndIf}
  ${If} $R1 != ""
  ${AndIfNot} ${FileExists} "$R1\uninstall.exe"
    StrCpy $R1 ""
  ${EndIf}

  ${If} $R1 == ""
    ReadRegStr $R1 HKCU "${PRECEDENTE_CHIAVE_CARTELLA}" ""
    ${If} $R1 != ""
    ${AndIfNot} ${FileExists} "$R1\uninstall.exe"
      StrCpy $R1 ""
    ${EndIf}
  ${EndIf}

  ${If} $R1 == ""
    StrCpy $R1 "$LOCALAPPDATA\${PRECEDENTE_NOME}"
    ${IfNot} ${FileExists} "$R1\uninstall.exe"
      DetailPrint "${PRECEDENTE_NOME}: disinstallatore non trovato, la versione precedente non viene rimossa. / uninstaller not found, the previous version is left alone."
      Goto precedente_fine
    ${EndIf}
  ${EndIf}

  ; Il comando è il disinstallatore che abbiamo appena trovato, nella cartella in
  ; cui l'abbiamo trovato, fra virgolette: un percorso con spazi
  ; (C:\Users\Nome Cognome\...) altrimenti si spezzerebbe.
  StrCpy $R0 "$\"$R1\uninstall.exe$\""

  ; 3. Nome del processo da cercare: lo scrive Tauri nella voce
  ;    (MainBinaryName); se manca, quello noto.
  ReadRegStr $R2 HKCU "${PRECEDENTE_CHIAVE_VOCE}" "MainBinaryName"
  ${If} $R2 == ""
    StrCpy $R2 "${PRECEDENTE_EXE}"
  ${EndIf}

  ; 4. Il programma è aperto? IMPORTANTE: il disinstallatore di Tauri, quando
  ;    trova il programma in esecuzione e gira in modo silenzioso (/S), lo
  ;    CHIUDE A FORZA senza chiedere niente, e con lui il lavoro non salvato e
  ;    gli accessi al database. Quindi controlliamo noi, prima, e il
  ;    disinstallatore parte solo a processo chiuso.
  ;    FindProcessCurrentUser (plugin nsis_tauri_utils 0.5.3, lo stesso di
  ;    CheckIfAppIsRunning nel template 2.11.4): 0 = processo trovato,
  ;    qualunque altro valore = non c'è. Cerca solo fra i processi dell'utente
  ;    corrente, quanto basta per un'installazione "currentUser".
  StrCpy $R4 0
  precedente_controlla:
    nsis_tauri_utils::FindProcessCurrentUser "$R2"
    Pop $R3
    ${If} $R3 <> 0
      Goto precedente_chiuso
    ${EndIf}

    ; In modo passivo o silenzioso (è il caso dell'aggiornamento automatico:
    ; /P /UPDATE /R) nessuno può rispondere a una finestra: aspettiamo un
    ; poco, il tempo che il vecchio programma finisca di uscire. Se dopo
    ; PRECEDENTE_ATTESA_S secondi è ancora lì, non lo tocchiamo.
    ${If} $PassiveMode = 1
    ${OrIf} ${Silent}
      IntOp $R4 $R4 + 1
      ${If} $R4 > ${PRECEDENTE_ATTESA_S}
        Goto precedente_aperto
      ${EndIf}
      Sleep 1000
      Goto precedente_controlla
    ${EndIf}

    ; In modo grafico lo chiediamo all'utente, nelle due lingue (non sappiamo
    ; quale preferisca: il testo è qui, fuori dalle LangString del template).
    ; OK = "l'ho chiuso", si ricontrolla (e se è ancora aperto lo si richiede);
    ; Annulla = si installa lo stesso, lasciando la versione precedente dov'è.
    MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "Rymembra sostituisce ${PRECEDENTE_NOME}, che è ancora aperto.$\nChiudi ${PRECEDENTE_NOME} (anche dall'icona vicino all'orologio) e premi OK per continuare. Con Annulla ${PRECEDENTE_NOME} resta installato.$\n$\nRymembra replaces ${PRECEDENTE_NOME}, which is still running.$\nClose ${PRECEDENTE_NOME} (also from the icon next to the clock) and press OK to continue. With Cancel, ${PRECEDENTE_NOME} stays installed." IDOK precedente_controlla
    ; (qui solo Annulla)

  precedente_aperto:
    ; Ancora in esecuzione: non si disinstalla niente. Rymembra si installa
    ; comunque; il vecchio programma si potrà togliere a mano da "App e
    ; funzionalità".
    DetailPrint "${PRECEDENTE_NOME} è ancora aperto: la versione precedente non viene rimossa. / ${PRECEDENTE_NOME} is still running: the previous version is not removed."
    Goto precedente_fine

  precedente_chiuso:
  ; 5. Disinstallazione silenziosa.
  ;    - `/S`: nessuna finestra.
  ;    - `_?=<cartella>`: dice a NSIS di eseguire il disinstallatore sul posto
  ;      invece di copiarsi in %TEMP% e rilanciarsi. Senza, ExecWait tornerebbe
  ;      subito, mentre il lavoro vero è ancora in corso. Va per ultimo, senza
  ;      virgolette (prende tutto il resto della riga, spazi compresi).
  ;    - NIENTE `/UPDATE`: con /UPDATE il disinstallatore lascerebbe le
  ;      scorciatoie (menu Start, desktop) e la voce di avvio automatico, che
  ;      resterebbero a puntare a un eseguibile che non c'è più.
  ;    Il codice d'uscita è valido solo se ExecWait è riuscito a partire: in
  ;    caso contrario NSIS mette "error" nella variabile, che un confronto
  ;    numerico leggerebbe come 0. Per questo si controlla prima ${Errors}.
  DetailPrint "Rimozione della versione precedente (${PRECEDENTE_NOME})... / Removing the previous version (${PRECEDENTE_NOME})..."
  ClearErrors
  ExecWait '$R0 /S _?=$R1' $R4
  ${If} ${Errors}
    StrCpy $R4 -1
  ${EndIf}

  ${If} $R4 = 0
    ; 6. Pulizia. Con `_?=` il disinstallatore non può cancellare se stesso,
    ;    quindi uninstall.exe e la cartella (vuota) restano: li togliamo noi.
    ;    RMDir senza /r toglie la cartella solo se è davvero vuota, e non la
    ;    tocchiamo se è la stessa in cui stiamo per installare Rymembra (un
    ;    utente può scegliere di installare proprio lì).
    Delete "$R1\uninstall.exe"
    ${If} $R1 != $INSTDIR
      RMDir "$R1"
    ${EndIf}
    ;    La chiave "produttore\prodotto" (cartella e lingua dell'installer) il
    ;    disinstallatore la lascia se non si spunta "elimina i dati"; la voce di
    ;    "App e funzionalità" l'ha già tolta lui, ma ripeterlo non costa.
    ;    Il produttore (Federico Baratti) non si tocca: ci vive anche Rymembra.
    DeleteRegKey HKCU "${PRECEDENTE_CHIAVE_CARTELLA}"
    DeleteRegKey HKCU "${PRECEDENTE_CHIAVE_VOCE}"
    DetailPrint "Versione precedente rimossa; i dati restano al loro posto. / Previous version removed; your data stays in place."
  ${Else}
    ; Il disinstallatore è fallito o non è partito. Non si insiste e non si
    ; cancella niente a mano (la cartella contiene ancora il programma e il
    ; suo disinstallatore): si prosegue con l'installazione di Rymembra.
    DetailPrint "Rimozione della versione precedente non riuscita (codice $R4): resta installata. / Removing the previous version failed (code $R4): it stays installed."
  ${EndIf}

  precedente_fine:
  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
!macroend

; I collegamenti di Rymembra, dove la versione precedente aveva i suoi.
;
; Li crea il template (CreateOrUpdateStartMenuShortcut e
; CreateOrUpdateDesktopShortcut, poco prima di questa macro), ma in modalità
; aggiornamento (/UPDATE, quella dell'updater) salta quelli che non esistono
; già. Fra due versioni di Rymembra è giusto: un collegamento tolto dall'utente
; non si rimette. Qui è sbagliato: quelli col nome nuovo non sono mai esistiti,
; e quelli col nome vecchio li ha appena tolti il disinstallatore. Senza questa
; macro chi si aggiorna da solo si ritrova senza Rymembra nel menu Start (visto
; in Windows Sandbox con l'installer 3.0.1 e `/P /UPDATE /R`).
;
; Fuori dalla modalità aggiornamento non fa niente: lì i collegamenti li crea il
; template, quello del desktop secondo la casella della pagina finale, che
; viene DOPO questa macro e che quindi non si deve anticipare. `/NS` (nessun
; collegamento) vale anche qui.
!macro NSIS_HOOK_POSTINSTALL
  ${If} $UpdateMode = 1
  ${AndIf} $NoShortcutMode <> 1
    ${If} $PrecedenteMenuStart == 1
      !if "${STARTMENUFOLDER}" != ""
        ${IfNot} ${FileExists} "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
          CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
          CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
          !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
        ${EndIf}
      !else
        ${IfNot} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
          CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
          !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
        ${EndIf}
      !endif
    ${EndIf}
    ${If} $PrecedenteDesktop == 1
    ${AndIfNot} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
      CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      !insertmacro SetLnkAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
    ${EndIf}
  ${EndIf}
!macroend
