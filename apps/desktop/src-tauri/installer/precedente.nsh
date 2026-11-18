; Rymembra - hook dell'installer NSIS per togliere la versione precedente.
;
; (Fa anche altro, in fondo al file: registra il protocollo rymembra:// e, alla
; disinstallazione, lo toglie insieme all'avvio con Windows.)
;
; Il programma si chiamava in un altro modo e viveva in un'altra cartella, con
; un'altra voce in "App e funzionalità". Un'installazione di Rymembra non la
; toccherebbe mai (nome, cartella e chiavi di registro sono diversi), e
; l'utente si ritroverebbe con due programmi quasi uguali. Questo hook,
; agganciato in tauri.conf.json (bundle.windows.nsis.installerHooks), la
; disinstalla da solo, in silenzio, PRIMA di copiare i file nuovi.
;
; SOLO SE È PROPRIO LEI. "Aether" è un nome che altri programmi possono avere,
; e un'app NSIS o Tauri di un altro editore chiamata così si installa
; esattamente in %LOCALAPPDATA%\Aether. Fino alla 3.2 il gancio lanciava il
; disinstallatore di qualunque voce "Aether" (con un ultimo ripiego proprio su
; quella cartella), e dopo ne cancellava la voce: l'altro programma restava su
; disco senza disinstallatore e senza voce, e in modo passivo nessuno vedeva
; niente. Adesso si tocca soltanto una voce che il modello di Tauri ha scritto
; per Aether: editore "Federico Baratti" e nome "Aether", come in tutte le
; versioni dalla 0.2.0 alla 3.0.0 (bundle.publisher e productName), con la
; stessa cartella nella voce e nella chiave produttore\prodotto, e dentro
; uninstall.exe e aether.exe. Qualunque altra cosa resta com'è.
;
; I DATI NON SI TOCCANO. Il disinstallatore di Tauri in modalità silenziosa
; non cancella %APPDATA%\<identificativo>: la casella "elimina i dati" vale 0
; quando non c'è la pagina di conferma. Quella cartella la prende Rymembra al
; primo avvio (trasloco in core/rymembra-app/src/trasloco.rs), quindi qui non
; si cancella niente che non sia il programma installato.
;
; QUESTO È L'UNICO FILE, fuori dalle costanti di eredità, in cui il nome di
; prima può comparire: serve a trovare la vecchia installazione e a dire
; all'utente quale programma chiudere. Dove compare in una finestra, dice
; anche che cos'è: "Aether (il nome di prima di Rymembra)".
;
; Dove gira: la macro NSIS_HOOK_PREINSTALL è espansa dal template di Tauri
; (stesso nome e stessa posizione nel template di cargo-tauri 2.11.4 e in
; quello del CLI npm 2.12.0) dentro `Section Install`, dopo `SetOutPath
; $INSTDIR` e prima di `CheckIfAppIsRunning` e di qualunque `File`. Questo file
; viene incluso PRIMA delle `!define` del template (PRODUCTNAME, INSTALLMODE,
; ...), PRIMA delle `Var` (PassiveMode, ...) e PRIMA delle lingue
; (`MUI_LANGUAGE`): per questo non definisce funzioni, che non potrebbero
; ancora nominarle, e tiene tutta la logica dentro le macro, che vengono
; espanse quando ormai esistono tutte. La seconda, NSIS_HOOK_POSTINSTALL, sta
; in fondo alla stessa sezione, dopo i file e i collegamenti del template.
;
; Solo HKCU: l'installazione precedente era "currentUser" (nessun amministratore,
; cartella in %LOCALAPPDATA%), e il registro a 64 bit non c'entra con HKCU.

!ifndef PRECEDENTE_NOME
  !define PRECEDENTE_NOME "Aether"
  ; L'editore che il modello di Tauri scriveva nella voce (Publisher) e nella
  ; chiave produttore\prodotto per ogni Aether: bundle.publisher di tutte le
  ; versioni. È quel che distingue la nostra da un "Aether" di altri.
  !define PRECEDENTE_EDITORE "Federico Baratti"
  ; Voce di "App e funzionalità": UninstallString, Publisher, DisplayName.
  !define PRECEDENTE_CHIAVE_VOCE "Software\Microsoft\Windows\CurrentVersion\Uninstall\Aether"
  ; Chiave "produttore\prodotto": il valore predefinito è la cartella di installazione.
  !define PRECEDENTE_CHIAVE_CARTELLA "Software\Federico Baratti\Aether"
  ; L'eseguibile di Aether ([[bin]] name = "aether"): deve stare nella cartella,
  ; ed è il processo da aspettare.
  !define PRECEDENTE_EXE "aether.exe"
  ; Secondi che aspettiamo, in modo passivo o silenzioso, che il vecchio
  ; programma si chiuda da solo (l'aggiornamento automatico lo sta chiudendo
  ; proprio in questo momento).
  !define PRECEDENTE_ATTESA_S 10
  ; Dove la versione precedente aveva i collegamenti (1 = c'era): li guarda
  ; NSIS_HOOK_PREINSTALL prima di disinstallarla, li usa NSIS_HOOK_POSTINSTALL.
  Var PrecedenteMenuStart
  Var PrecedenteDesktop

  ; I testi, nelle lingue dell'installer (bundle.windows.nsis.languages). Con
  ; l'id in cifre e non con ${LANG_ENGLISH} / ${LANG_ITALIAN}: quelle costanti
  ; le definisce MUI_LANGUAGE, che il template mette DOPO questo file, e una
  ; LangString non si può scrivere dentro una sezione o una macro che la
  ; sezione espande. 1033 = inglese (Stati Uniti), 1040 = italiano. NSIS
  ; sceglie da sé quella della lingua di Windows, e se non c'è la prima.
  ; Un'altra lingua in tauri.conf.json vuole qui le sue righe:
  ; strumenti/installatore.test.js lo controlla.
  LangString precedenteAperto 1033 "Rymembra replaces ${PRECEDENTE_NOME} (Rymembra’s previous name), which is still running.$\nClose ${PRECEDENTE_NOME} (also from the icon next to the clock) and press OK to continue. With Cancel, ${PRECEDENTE_NOME} stays installed."
  LangString precedenteAperto 1040 "Rymembra sostituisce ${PRECEDENTE_NOME} (il nome di prima di Rymembra), che è ancora aperto.$\nChiudi ${PRECEDENTE_NOME} (anche dall'icona vicino all'orologio) e premi OK per continuare. Con Annulla, ${PRECEDENTE_NOME} resta installato."
  LangString precedenteAltroEditore 1033 "An “${PRECEDENTE_NOME}” from another publisher is installed: it is left alone."
  LangString precedenteAltroEditore 1040 "È installato un «${PRECEDENTE_NOME}» di un altro editore: resta com'è."
  LangString precedenteNonRiconosciuta 1033 "${PRECEDENTE_NOME}: the installation doesn’t look like the previous version of Rymembra, so it is left alone."
  LangString precedenteNonRiconosciuta 1040 "${PRECEDENTE_NOME}: l'installazione non sembra la versione precedente di Rymembra, quindi resta com'è."
  LangString precedenteAncoraAperto 1033 "${PRECEDENTE_NOME} is still running: the previous version is not removed."
  LangString precedenteAncoraAperto 1040 "${PRECEDENTE_NOME} è ancora aperto: la versione precedente non viene rimossa."
  LangString precedenteRimozione 1033 "Removing the previous version (${PRECEDENTE_NOME})..."
  LangString precedenteRimozione 1040 "Rimozione della versione precedente (${PRECEDENTE_NOME})..."
  LangString precedenteRimossa 1033 "Previous version removed; your data stays in place."
  LangString precedenteRimossa 1040 "Versione precedente rimossa; i dati restano al loro posto."
  LangString precedenteFallita 1033 "Removing the previous version failed (code $R4): it stays installed."
  LangString precedenteFallita 1040 "Rimozione della versione precedente non riuscita (codice $R4): resta installata."
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

  ; 2. È proprio la nostra? Editore e nome come li scriveva il modello di
  ;    Tauri per Aether (il confronto di NSIS non distingue maiuscole e
  ;    minuscole). Prima di qualunque altra cosa, collegamenti compresi: per
  ;    un "Aether" di altri, NSIS_HOOK_POSTINSTALL non deve creare niente.
  ReadRegStr $R3 HKCU "${PRECEDENTE_CHIAVE_VOCE}" "Publisher"
  ReadRegStr $R4 HKCU "${PRECEDENTE_CHIAVE_VOCE}" "DisplayName"
  ${If} $R3 != "${PRECEDENTE_EDITORE}"
  ${OrIf} $R4 != "${PRECEDENTE_NOME}"
    DetailPrint "$(precedenteAltroEditore)"
    Goto precedente_fine
  ${EndIf}

  ; 3. In quale cartella? Quella della voce ($R2: UninstallString tolte le
  ;    virgolette e il nome finale \uninstall.exe), e deve essere la stessa
  ;    della chiave produttore\prodotto ($R1), che lo stesso modello scrive
  ;    insieme alla voce. Dentro devono esserci il disinstallatore e
  ;    l'eseguibile di Aether. Se una sola di queste cose non torna, la voce è
  ;    malformata o non è nostra: non si esegue niente e non si cancella
  ;    niente, e Rymembra si installa lo stesso. Nessun ripiego su una
  ;    cartella "solita": è proprio lì che starebbe un Aether di altri.
  StrCpy $R2 $R0
  StrCpy $R3 $R2 1
  ${If} $R3 == "$\""
    StrCpy $R2 $R2 "" 1
    StrCpy $R3 $R2 1 -1
    ${If} $R3 == "$\""
      StrCpy $R2 $R2 -1
    ${EndIf}
  ${EndIf}
  ; "\uninstall.exe" sono 14 caratteri: se la voce ne ha meno non può finire così.
  StrLen $R4 $R2
  ${If} $R4 >= 14
    StrCpy $R3 $R2 14 -14
    ${If} $R3 == "\uninstall.exe"
      StrCpy $R2 $R2 -14
    ${Else}
      StrCpy $R2 ""
    ${EndIf}
  ${Else}
    StrCpy $R2 ""
  ${EndIf}

  ReadRegStr $R1 HKCU "${PRECEDENTE_CHIAVE_CARTELLA}" ""
  ${If} $R1 == ""
  ${OrIf} $R1 != $R2
  ${OrIfNot} ${FileExists} "$R1\uninstall.exe"
  ${OrIfNot} ${FileExists} "$R1\${PRECEDENTE_EXE}"
    DetailPrint "$(precedenteNonRiconosciuta)"
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

  ; Il comando è il disinstallatore di quella cartella, fra virgolette: un
  ; percorso con spazi (C:\Users\Nome Cognome\...) altrimenti si spezzerebbe.
  StrCpy $R0 "$\"$R1\uninstall.exe$\""

  ; 4. Il programma è aperto? IMPORTANTE: il disinstallatore di Tauri, quando
  ;    trova il programma in esecuzione e gira in modo silenzioso (/S), lo
  ;    CHIUDE A FORZA senza chiedere niente, e con lui il lavoro non salvato e
  ;    gli accessi al database. Quindi controlliamo noi, prima, e il
  ;    disinstallatore parte solo a processo chiuso. Il processo è quello noto
  ;    (PRECEDENTE_EXE, che il punto 3 ha trovato nella cartella), non quello
  ;    che dice la voce.
  ;    FindProcessCurrentUser (plugin nsis_tauri_utils 0.5.3, lo stesso di
  ;    CheckIfAppIsRunning nel template 2.11.4): 0 = processo trovato,
  ;    qualunque altro valore = non c'è. Cerca solo fra i processi dell'utente
  ;    corrente, quanto basta per un'installazione "currentUser".
  StrCpy $R4 0
  precedente_controlla:
    nsis_tauri_utils::FindProcessCurrentUser "${PRECEDENTE_EXE}"
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

    ; In modo grafico lo chiediamo all'utente, nella lingua dell'installer.
    ; OK = "l'ho chiuso", si ricontrolla (e se è ancora aperto lo si richiede);
    ; Annulla = si installa lo stesso, lasciando la versione precedente dov'è.
    MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "$(precedenteAperto)" IDOK precedente_controlla
    ; (qui solo Annulla)

  precedente_aperto:
    ; Ancora in esecuzione: non si disinstalla niente. Rymembra si installa
    ; comunque; il vecchio programma si potrà togliere a mano da "App e
    ; funzionalità".
    DetailPrint "$(precedenteAncoraAperto)"
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
  DetailPrint "$(precedenteRimozione)"
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
    ;    "App e funzionalità" l'ha già tolta lui, ma ripeterlo non costa: a
    ;    questo punto è certo che sono nostre (punti 2 e 3).
    ;    Il produttore (Federico Baratti) non si tocca: ci vive anche Rymembra.
    DeleteRegKey HKCU "${PRECEDENTE_CHIAVE_CARTELLA}"
    DeleteRegKey HKCU "${PRECEDENTE_CHIAVE_VOCE}"
    DetailPrint "$(precedenteRimossa)"
  ${Else}
    ; Il disinstallatore è fallito o non è partito. Non si insiste e non si
    ; cancella niente a mano (la cartella contiene ancora il programma e il
    ; suo disinstallatore): si prosegue con l'installazione di Rymembra.
    DetailPrint "$(precedenteFallita)"
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
; collegamento) vale anche qui. Le due variabili le accende solo
; NSIS_HOOK_PREINSTALL, e solo per un Aether riconosciuto.
;
; IL PROTOCOLLO rymembra://. Un link `rymembra://collega/...` cliccato in una
; chat o in una pagina deve arrivare a Rymembra: Windows lo manda a chi ha
; registrato il protocollo, e il registro è questo (HKCU\Software\Classes, che
; per l'utente corrente vale come HKCR e non chiede amministratore). Si scrive
; sempre, anche in /UPDATE: un aggiornamento che arriva dal disinstallatore
; della versione di prima non lascia niente da conservare, e un percorso
; d'installazione cambiato deve riscriversi. Il comando è `"<exe>" "%1"`: il
; link arriva come un argomento solo, e il programma lo tratta come un input
; che non è suo (src-tauri/src/argomenti.rs: prefisso, lunghezza, caratteri) e lo
; mostra a chi decide prima di collegare qualcosa.
; Non si usa `plugins.deep-link` della configurazione di Tauri, che il modello
; saprebbe leggere (`deep_link_protocols`, installer.nsi): senza costruire un
; installer non si può dire che il suo gancio parta, e un registro scritto qui
; si legge a occhio.
!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Classes\rymembra" "" "URL:Rymembra"
  WriteRegStr HKCU "Software\Classes\rymembra" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\rymembra\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr HKCU "Software\Classes\rymembra\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""

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

; La disinstallazione: il protocollo, e l'avvio con Windows.
;
; Espansa dal template in fondo a `Section Uninstall` (NSIS_HOOK_POSTUNINSTALL,
; installer.nsi del CLI npm 2.12.0). Niente qui dentro in modalità aggiornamento
; (/UPDATE): il disinstallatore della versione di prima gira per far posto alla
; nuova, e il registro che l'utente ha acceso — l'avvio, il protocollo — è suo.
;
; IL PROTOCOLLO si toglie solo se è il nostro: se il comando punta a questa
; cartella. È la stessa cautela che il modello ha per i suoi protocolli
; (`deep_link_protocols`): un altro programma può essersi registrato su
; `rymembra` dopo di noi, e toglierglielo non spetta a questo disinstallatore.
;
; L'AVVIO CON WINDOWS lo toglie già il modello (`DeleteRegValue HKCU "...\Run"
; "${PRODUCTNAME}"`, sempre fuori da /UPDATE): il valore si chiama come il
; prodotto proprio perché sia lui a toglierlo, e una prova in src-tauri lo tiene
; uguale a `productName`. Qui si ripete, e si toglie anche lo `StartupApproved`
; che Windows scrive quando l'utente lo spegne da Gestione attività e non toglie
; mai da solo: una copia che si disinstalla non deve lasciare né l'avvio né il
; ricordo di averlo spento, che bloccherebbe l'interruttore di una copia nuova.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ReadRegStr $R7 HKCU "Software\Classes\rymembra\shell\open\command" ""
    ${If} $R7 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
      DeleteRegKey HKCU "Software\Classes\rymembra"
    ${EndIf}
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${PRODUCTNAME}"
  ${EndIf}
!macroend
