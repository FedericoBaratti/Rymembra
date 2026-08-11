# Changelog

Formato [Keep a Changelog](https://keepachangelog.com/it/1.1.0/), versioni
[SemVer](https://semver.org/lang/it/).

## Come si versiona qui

Un numero solo per tutto il monorepo: la radice e i sei workspace dichiarano
sempre la stessa versione, e `npm run check:version` fa fallire `verify` se non
è così. Si cambia con `npm run version:set -- <versione>`, mai a mano.

Il motivo di un numero unico invece di sei: la versione finisce in tre posti che
l'utente vede — il nome dell'installer, `app.getVersion()` nel log di avvio, e il
`version` che il telefono legge da `/health` per decidere se il protocollo è
compatibile — e quei tre posti la leggono da `package.json` diversi. Sei numeri
indipendenti sono sei occasioni di divergere; nella cartella `release/` del
vecchio albero ci sono ancora `Aether Setup 0.9.13.7.26.2.exe` e
`0.9.14.7.26.2.exe` accanto a `0.9.14.7.26.3.exe`, cioè questo problema in forma
di artefatti.

Cosa incrementa cosa:

- **patch** — correzioni che non cambiano né l'aspetto né il formato dei dati;
- **minor** — funzionalità nuove, e ogni migrazione del database (le migrazioni
  vanno solo avanti: una minor è il segnale che tornare indietro richiede un
  ripristino);
- **major** — un cambio incompatibile del formato skin (`SKIN_FORMAT_VERSION`) o
  del protocollo di trasporto (`SKIN_TRANSFER_PROTOCOL`), cioè i due punti in cui
  un dispositivo aggiornato smetterebbe di capirsi con uno fermo.

## [Non rilasciato]

Due riscritture in fila, sullo stesso ramo di lavoro. La prima
(`aether/skin-system-e-core`, dal 26 luglio 2026) ha rifatto nucleo e sistema
delle skin; la seconda (`aether/rust-core`) ha portato tutto in Rust dietro una
finestra Tauri. **Non è ancora rilasciabile**: il lato mobile non esiste.

### Aggiunto — il nucleo in Rust

Un workspace di cinque crate più l'applicazione desktop:

- **`aether-domain`** — puro, senza orologio né disco: chiavi di brano e di
  playlist, normalizzazione del testo, raggruppamento degli album, coda con
  shuffle e ripetizione, piano di scansione con riconoscimento degli
  spostamenti, piano di riordino, fusione delle statistiche, regole d'ascolto,
  catalogo degli errori. Provato con file golden e prove di parità.
- **`aether-play`** — il motore audio, uno solo: `cpal` più `symphonia`, tre
  fili e un anello senza lucchetti fra chi decodifica e la callback audio.
  Gapless, ReplayGain, volume. Nel vecchio albero la riproduzione era scritta
  due volte — Howler nel webview, ExoPlayer in Kotlin — e divergeva a ogni
  modifica.
- **`aether-app`** — l'orchestrazione: database e migrazioni, filesystem,
  metadati, copertine, scansione e ricerca FTS5, playlist, esecuzione del
  riordino con giornale e annullamento, importazione dal vecchio database,
  persistenza di coda e volume.
- **`aether-skin`** — registro dei token, effetti parametrici con costo
  dichiarato, parts registry, compilatore, formato `.aeskin` con le guardie di
  un archivio non fidato.
- **`aether-cloud`** — il backup su Google Drive: OAuth 2.0 con PKCE, servitore
  loopback effimero per il consenso, Drive REST v3 ristretto alla cartella
  privata dell'applicazione, portachiavi di sistema per il token. Non riceve mai
  una connessione al database, ed è così che «nessun lucchetto della libreria
  resta preso durante una richiesta di rete» diventa una proprietà che il
  compilatore verifica invece di un commento.

### Aggiunto — il backup su Google Drive

Quel che una scansione **non** sa ricostruire — conteggi d'ascolto, voti,
preferiti, playlist, cartelle sorvegliate, skin installate e bozze dello
Studio — esisteva in un posto solo, e una reinstallazione se lo portava via.

- **Backup automatico, ripristino manuale.** Una passata ogni quarto d'ora e
  dopo ogni modifica, con antirimbalzo: dieci cuoricini di fila sono un
  salvataggio, non dieci. Il database locale non viene mai riscritto senza che
  qualcuno prema «Ripristina».
- **Tre specie di file, non un archivio unico.** I metadati sono settanta
  chilobyte compressi e cambiano di continuo; una skin arriva a venti megabyte e
  non cambia quasi mai. Un archivio solo avrebbe rispedito venti megabyte a ogni
  voto messo.
- **«Non è cambiato niente» in una richiesta sola.** Ogni file porta la sua
  impronta blake3 in `appProperties`: su una libreria ferma, una passata è una
  `files.list` e nient'altro.
- **Il file remoto non regredisce mai.** Prima di sovrascrivere si guarda chi
  l'ha scritto: se è stato un altro computer si scarica il suo, si fonde in
  memoria con `merge_stats` e si carica l'unione.
- **Il ripristino è un piano che si guarda prima.** «1 384 invariati, 17 da
  aggiornare», il delta di ciascuno, le playlist con «22 brani su 30 presenti
  qui», le cartelle che non esistono più. Applicarlo è idempotente: rifarlo
  propone un piano vuoto.
- **Non distrugge.** I conteggi salgono e non scendono, le cartelle sorvegliate
  si uniscono e non si sostituiscono, una skin installata non si sovrascrive, una
  bozza dello Studio non si tocca. I brani del backup senza un file su questo
  disco si **elencano** e non si ricreano: una riga senza file non si apre, e la
  scansione successiva la toglierebbe.
- **Lo scope è `drive.appdata` e basta.** La cartella è privata per applicazione
  e account, invisibile in Drive, e sparisce quando l'utente rimuove i dati
  dell'app. È uno scope non sensibile: nessuna verifica di Google. Il consenso si
  dà nel browser di sistema, mai nella webview; il token di aggiornamento sta nel
  portachiavi del sistema operativo e mai nella tabella `settings`.

### Aggiunto — l'applicazione desktop

Una finestra Tauri 2 sopra il nucleo, senza logica propria:

- **Libreria**: cartelle sorvegliate, scansione con avanzamento, griglia degli
  album, elenco dei brani con quattro ordinamenti, ricerca a tutto testo,
  preferiti e valutazioni.
- **Riproduzione**: barra flottante con cursore, volume, ordine casuale e
  ripetizione a tre stati; pannello della coda con riordino a trascinamento;
  «riproduci dopo» e «accoda» dal menù contestuale. La posizione si interpola
  fra i colpi da 250 ms del nucleo, che manda quattro eventi al secondo e non
  sessanta.
- **Playlist**: creazione, rinomina, cancellazione con lapide di
  sincronizzazione, aggiunta e riordino. L'identità è il nome normalizzato, la
  stessa chiave che usa l'importatore. Le playlist automatiche importate si
  vedono ma non si modificano a mano: la loro appartenenza la decidono le
  regole, e un ricalcolo cancellerebbe qualunque aggiunta manuale.
- **Riordino della libreria**: anteprima di ogni singolo spostamento prima di
  toccare un file, esecuzione con giornale scritto riga per riga, annullamento.
- **Importazione dal vecchio database**: anteprima e poi esecuzione, con
  l'elenco esplicito dei brani non ritrovati.
- **Backup su Drive**: una sezione nelle Impostazioni con l'interruttore
  dell'automatico, l'account collegato, l'ora dell'ultima passata, «Salva
  adesso» e «Ripristina…». Il ripristino è una finestrella con la stessa forma
  del riordino: si vede l'elenco di quel che tornerebbe indietro prima che
  torni. Il filo che salva gira in sottofondo e prende il lucchetto della
  libreria in due sole finestre brevi, così una passata non fa impuntare né la
  riproduzione né una scansione.
- **Skin installate**: `.aeskin` letti dalla cartella dati, selettore, scelta
  persistente. Una skin illeggibile sparisce dall'elenco invece di romperlo.

### Aggiunto — il ridisegno dell'interfaccia

Il documento di disegno sta in `disegno-ux.md`: i cinque principi con la loro
ragione, la mappa dei 47 token, quella delle 51 parti — **quali il markup emette
e quali no, col perché** —, le scale, il movimento, la tastiera, e i contrasti
misurati nei due temi.

- **Sistema di icone**: 38 simboli in uno sprite SVG montato una volta sola,
  `currentColor` a 1,6 di tratto. Prima erano glifi Unicode (`⏮ ⏸ ⏭ ♥ 🔇`): ogni
  sistema li disegna a modo suo e due li disegnano a colori.
- **Inter Variable impacchettato** in `apps/desktop/src/font/`. `--font-sans:
  'Inter Variable'` era una dichiarazione a cui non corrispondeva nessun file.
  Auto-ospitato: la CSP non dichiara `font-src`, quindi vale `default-src 'self'`
  e nessuna richiesta esce dalla finestra.
- **Tre colonne — navigo, guardo, ascolto** (240px · resto · 348px). La barra
  laterale torna a essere sola navigazione: la configurazione ci stava mescolata
  in cinque blocchi. La terza colonna prende il posto della barra in basso;
  sotto 1100px si chiude e il lettore flottante torna identico a prima.
- **Impostazioni** è una pagina, con sei sezioni: cartelle e scansione, aspetto,
  riproduzione, movimento e accesso, dalla versione precedente, libreria e dati.
  Ci atterrano anche le cartelle e i `.aeskin` lasciati cadere sulla finestra —
  `dragDropEnabled` era acceso e non aveva nessun gestore.
- **Tema chiaro, scuro o di sistema**, senza nessun comando nuovo: il
  compilatore emette già il selettore `[data-theme='light']`.
- **Artisti**: vista nuova, con `list_artists` in `aether-app` e `sort_name` in
  `aether-domain` (The Cure sotto C). Il ritratto è un mosaico 2×2 delle
  copertine vere: non c'è rete, e un'immagine d'artista non esisterà mai.
- **In riproduzione**: schermata a due colonne dove la navigazione **resta** —
  coprire tutto costringerebbe a chiudere per cambiare vista. Tre porte per
  aprirla, `Esc` per uscirne. I bottoni dei testi e dell'equalizzatore ci sono
  **spenti, con la ragione scritta**: il nucleo non espone né testi né bande, e
  uno spettro finto sarebbe l'unica bugia dell'interfaccia.
- **Scansione annullabile**: `Scan::run` chiede al chiamante se continuare dopo
  ogni lotto. Annullare salta il blocco delle sparizioni non reclamate — quelle
  righe sono candidate a essere spostamenti che i lotti successivi avrebbero
  appaiato, e cancellarle a metà perderebbe voti e preferiti. Resta una libreria
  giusta e incompleta, come dopo un'interruzione qualsiasi. L'avanzamento segue
  chi esce dalla sezione, come notifica in basso a destra.
- **Selezione multipla**, con una barra che compare al posto del lettore, e la
  **mappa della tastiera in un posto solo**: `Spazio`, `/`, `←→`, `↑↓`, `F`,
  `Esc`, e `Alt+↑↓` per riordinare la coda — il trascinamento HTML5 non è
  raggiungibile da tastiera, ed era il difetto del pannello della coda.
- **Skin Studio**, tre viste. *Ispeziona*: una sonda che, passando sopra
  l'anteprima dal vivo, dice **come si chiama la cosa che si sta guardando** —
  cammina dal bersaglio verso l'alto confrontando le classi col registro, e
  quel che non è una parte non si illumina. *Documento*: il JSON, il CSS
  compilato e il confronto con Plain, con gli errori che portano il
  suggerimento di `nearest_parts()` come bottone; un errore **non** spegne
  l'anteprima, che resta all'ultimo stato valido e lo dichiara. *Tavolozza*:
  conteggio d'uso per colore, sorgenti dinamiche per singolo token (il testo non
  segue mai la copertina), capacità, e la lista di controllo prima
  dell'esportazione — bloccata dagli errori, non dagli avvisi.
- **«Crea tema»**, in Impostazioni › Aspetto. Lo Studio sapeva solo **aprire una
  skin che c'era già**: `studio_documento` risponde `skin.notFound` a un
  identificatore che non conosce, e il pulsante «Deriva…» sulla skin di serie
  passava `plain`, quindi la bozza finiva in `bozze/plain/` e il manifest
  continuava a dire `"id": "plain"` — cioè produceva un pacchetto che
  `skin_installa` rifiuta apposta. Ora una finestrella chiede nome, autore,
  descrizione e da quale skin partire; l'identificatore si **deriva dal nome** e
  si mostra mentre lo si scrive, obbedendo alla regola stretta del formato (da 2
  a 48 fra minuscole, cifre e trattini, e comincia con una lettera) invece che
  alla guardia più larga dei percorsi. Il tema nasce **installato e attivo**, non
  come bozza: senza, l'anteprima dal vivo non sarebbe l'applicazione — che è la
  seconda delle tre regole su cui lo Studio è costruito.
- **`skin_installa_sorgente`** e il pulsante **«Salva e usa»** nello Studio: un
  manifest si installa senza passare da un file sul disco. Prima l'unica strada
  era esportare un `.aeskin` in una cartella qualunque e reinstallarlo dalla
  finestra di dialogo di Impostazioni — due scelte di percorso per un file che
  nessuno voleva. Il pacchetto si **rilegge** prima di posarlo, cioè passa dalle
  stesse guardie di una skin arrivata dalla rete. «Esporta .aeskin» resta, e
  serve a quel che serviva davvero: **dare** un tema a qualcun altro.
- **Tre parti nuove** nel registro, da 48 a 51: `list-row`, `segmented`,
  `selection-bar`. E il markup ora le nomina: **ne emette 42**, dove prima ne
  emetteva zero — cioè c'era un sistema di skin che nessuna skin poteva usare
  oltre ai token.
- **`meta.preview`**: i tre colori della scheda di una skin li sceglie l'autore.
  Prima l'interfaccia li indovinava prendendo `surface.0`, `surface.2` e
  l'accento, che per due skin su tre funzionava per caso.
- **Avviso di contrasto** in `check_skin`: ogni coppia testo/superficie sotto
  4,5:1, in **entrambi** i temi. `contrast_ratio` esisteva e la chiamavano solo i
  test. `cargo run -p aether-skin --example contrasti` stampa la tabella, ed è
  da lì che vengono i numeri del documento.
- **`SkinIpc` porta impaginazione e movimento**: `layout` e `motion.intensity`
  venivano compilati e non li leggeva nessuno. Ora l'interfaccia li onora, con
  `prefers-reduced-motion` del sistema che vince sempre sulla skin.

### Aggiunto — la rifinitura grafica

Il ridisegno aveva finito il **sistema**; questo passaggio fa la **materia**.
Fino a qui il foglio non conteneva un solo `color-mix`, una sola sfocatura e una
sola animazione a fotogrammi: ogni superficie era una tinta piatta appoggiata su
un'altra tinta piatta.

- **Le copertine erano tutte la miniatura da 160 pixel.** `Copertina` chiedeva
  sempre il file `.t`, anche per la copertina di «In riproduzione», che è larga
  trecentosessanta e su uno schermo a 150% sono cinquecentoquaranta pixel veri:
  un JPEG da 160 ingrandito tre volte e mezzo. Ora l'originale arriva nei tre
  posti in cui la copertina **è** il soggetto — schermo intero, terza colonna,
  testata di un album — e la griglia resta su miniatura, dove è la scelta giusta.
  La testata di un album, che dal ridisegno non aveva più nessuna immagine, ne
  ha di nuovo una.
- **L'ambiente è la copertina.** Dietro la copertina grande e dietro la terza
  colonna c'è l'immagine stessa, ingrandita e sfocata di sessantaquattro pixel,
  con sotto la sfumatura `--hero-rgb` di prima per quando la copertina non c'è.
  Meglio di una tinta estratta, perché un disco rosso con la fascia gialla dà un
  ambiente rosso **e** giallo. Il velo `np-scrim` resta neutro e resta sopra: è
  la riga che garantisce il contrasto del titolo.
- **Un modello di luce.** Cinque proprietà nuove in `stile.css` — `--spigolo`,
  `--incavo`, `--alzata`, e le due forme già composte `--luce` e `--filo` —
  scritte con `color-mix()` dentro `light-dark()`, quindi senza un solo colore
  letterale e con la direzione che si inverte da sé fra i due temi. Ogni
  superficie sollevata ha un filo chiaro sul bordo alto e una sfumatura che si
  esaurisce scendendo; ogni solco — piste, campi, barre — ha il suo incavo.
- **Le ombre passano da un livello a due**: un contatto corto e un'ambientale
  larga. Un livello solo non può dire insieme dove un oggetto tocca e quanto sta
  in alto. La prova non è un valore fissato ma la proprietà, in
  `le_ombre_hanno_un_contatto_e_un_ambiente`.
- **I raggi vengono dai token.** Sei numeri fissi — 4, 5, 6, 8, 10 — stavano
  dentro contenitori che usavano `--radius-card`: una skin che squadrava tutto
  lasciava sei angoli tondi. Ora si derivano, e zero resta zero.
- **Le transizioni di vista, che la skin scriveva da sempre.**
  `motion.routeTransition` era dichiarato in `plain.json`, compilato in due
  `@keyframes` e due `::view-transition-*`, e mai eseguito: mancava la chiamata a
  `startViewTransition`. Il foglio dell'applicazione non contiene nessuna durata
  né curva per quel passaggio — sono della skin. Chi non partecipa porta un
  `view-transition-name` proprio.
- **Un anello di fuoco solo e globale**, dove prima ce n'erano sette locali e
  tutto il resto cadeva sul contorno di serie del motore.
- **I segnaposto di caricamento**: `skeleton` era una parte registrata ed emessa
  soltanto dall'anteprima dello Studio. `caricaVista` aspettava e poi
  sostituiva, quindi durante la richiesta restava in piedi il contenuto della
  vista precedente.
- **La testata di pagina va a capo.** Con la terza colonna aperta su una finestra
  da 1280 il contenuto è largo seicento pixel, e il titolo — l'unico `flex: 1` —
  si riduceva a una lettera sola mentre il campo di ricerca restava largo come su
  uno schermo intero.
- **Il titolo dello schermo intero è fluido e sta su due righe**, misurato sul
  contenitore giusto: «Solar Sailer - Remixed by Pretty Lights» si leggeva
  «Solar Sailer - …», cioè senza la parte che distingue una versione dall'altra.
- **Il carattere da titolo arriva dove il titolo è la pagina**: testata di un
  album, numeri delle statistiche, marchio, stati vuoti, titolo della terza
  colonna. E `tabular-nums` dove un numero in sans cambia sul posto.
- **«Sala», la seconda skin.** La sala d'ascolto: neri caldi, ottone, il rosso di
  un'etichetta sul cuore, il monospaziato come carattere da titolo. Una sorgente
  di luce sola — il motivo `lampada` — e nove punti di costo su dieci. È la prova
  che il registro delle parti serve: Plain non ne ridipinge nessuna, Sala sette.

### Aggiunto — l'equalizzatore

Dieci bande a ottave — 31, 62, 125, 250, 500 Hz, 1, 2, 4, 8, 16 kHz — regolabili
da ±12 dB, con preset di serie e preset salvabili. Si trova in due posti: un
pannello dal tasto accanto al volume nella barra del lettore, e una scheda in
Impostazioni › Riproduzione. Un componente solo, in due taglie, come il
trasporto.

- **`aether-play::equalizzatore`**: una cascata di dieci biquad *peaking* dal
  ricettario RBJ, in forma diretta II trasposta con lo stato in `f64`. La forma e
  la precisione non sono pignoleria: la banda dei 31 Hz su un'uscita a 48 kHz ha
  i poli a un millesimo dal cerchio unitario, e in `f32` in forma diretta I quel
  filtro non suona sbagliato — rumoreggia. Niente `mul_add`, che senza FMA
  abilitata diventa una chiamata a `fma()` di libm dentro la callback audio.
- **I filtri stanno nella callback, non prima dell'anello.** Filtrare nel filo
  che decodifica sarebbe stato molto più semplice e avrebbe fatto rispondere ogni
  cursore duecento millisecondi dopo il dito — la riserva dell'anello. Il costo
  di farlo nel posto giusto è un **secondo anello senza lucchetti**, quello dei
  coefficienti: cinquanta numeri in virgola mobile non si pubblicano con delle
  atomiche senza che qualcuno possa leggerne metà di una curva e metà dell'altra.
- **Preamplificazione automatica, misurata invece che indovinata.** La
  scorciatoia ovvia — attenuare della banda più alzata — sbaglia di parecchio:
  dieci campane larghe un'ottava si sovrappongono, e alzarle tutte di 12 dB ne
  produce una ventina al centro dello spettro. La curva si valuta su una griglia
  di 89 frequenze **ancorata ai centri delle bande**, e si attenua del picco
  vero. Con una griglia più rada la misura sbagliava di oltre un decibel e
  l'uscita saturava lo stesso: le due prove che lo dimostrano stanno nel file.
- **Il cambio di curva si interpola in dieci passi** nel filo che decodifica,
  sui decibel e non sui coefficienti — la strada fra due biquad stabili passa per
  biquad che non lo sono. È la `RAMPA` del volume portata dove la rampa non
  arrivava: senza, caricare un preset a musica accesa fa uno schiocco.
- **La coda dei filtri si azzera insieme all'anello**, sullo stesso svuotamento
  che segue un salto: è un rimasuglio del punto di prima esattamente come i
  campioni ancora in viaggio.
- La curva e i preset stanno in `settings` accanto a coda e volume
  (`player.eq`, `player.eq.presets`), quindi nessuna migrazione. Una curva di
  lunghezza sbagliata si normalizza invece di essere buttata, e i valori si
  tagliano in lettura: quel file si può aprire e correggere a mano.
- **`equalizzatore` è l'unico comando di riproduzione che non manda
  `riproduzione:stato`**, e non è una svista: comporre quello stato richiede una
  lettura del brano corrente dal database, e il comando parte una quindicina di
  volte al secondo finché un cursore è sotto il dito. Manda `riproduzione:eq`,
  che sono due campi e nessuna query.

### Corretto — quel che si vedeva solo usandola

Due difetti grossi che nessun documento elencava, perché non si trovano
leggendo: uno si trova contando le righe di una libreria vera, l'altro
guardando la finestra mentre suona.

- **Il brano duecentouno non era raggiungibile da nessuna vista.** L'elenco
  chiedeva una pagina di duecento e non ne chiedeva più: né un tasto, né uno
  scorrimento, né un messaggio — semplicemente finiva. Sulla libreria di chi ci
  lavora, duecentocinquantotto brani, ne mancavano cinquantotto. Idem oltre il
  quattrocentesimo album, e i preferiti si prendevano chiedendo **duemila**
  brani e filtrandoli nella finestra: tutta la libreria letta a ogni visita, e
  chi ne ha di più non vedeva quelli oltre.
  Ora c'è `usePagine`, con una sentinella in fondo all'elenco osservata da un
  `IntersectionObserver`. Nel nucleo tre query nuove: `list_liked` (ordinata per
  quando il cuore è stato messo), `albums_by_artist` — la pagina di un artista
  **filtrava** nella finestra gli album già scaricati, quindi un artista oltre
  il quattrocentesimo dava una griglia vuota sotto un titolo che diceva «tre
  album» — e `search_count`, perché «N risultati» diceva la lunghezza della
  prima pagina: «60 risultati» per una ricerca che ne aveva trecento.
- **La finestra si ridisegnava venti volte al secondo mentre suonava.** La
  posizione interpolata era uno `useState` dentro `App`: ogni colpo da 50 ms
  rifaceva la testata, il corpo con tutte le righe dell'elenco, e l'oggetto
  `contesto` — il cui `useMemo` aveva `posizioneMs` fra le dipendenze, quindi
  non serviva a niente. Anche l'ascoltatore della tastiera si toglieva e si
  rimetteva a ogni colpo. Contraddiceva `disegno-ux.md §9`, che dichiara che
  nessun componente deve chiedere niente a quel ritmo.
  Ora la posizione vive in un archivio esterno letto con `useSyncExternalStore`,
  e la legge **una foglia sola**: `Scrubber`. Misurato con un contatore
  temporaneo in `RigaBrano`, otto secondi di musica: da circa quarantunomila
  disegni di riga a **zero**.
- **La colonna `#` diceva una cosa senza senso fuori da un album.** Mostrava il
  numero di traccia dentro il **suo** album, quindi in un elenco piatto leggeva
  `1, 39, 8, 1, 5, 1, 9…`. Ora è il numero di traccia solo dentro un album,
  dove serve a ritrovare il pezzo sulla custodia, e la posizione in elenco
  altrove.
- **Il riordino dei brani in una playlist era nell'IPC e non si raggiungeva.**
  `playlist_riordina` esisteva dal primo giorno e nessuno lo chiamava: la coda
  aveva sia il trascinamento sia `Alt+↑↓`, l'elenco di una playlist nessuno dei
  due. Ora li ha tutti e due — e tutti e due hanno finalmente un **indicatore di
  rilascio**, che mancava anche alla coda: si trascinava senza sapere dove
  sarebbe finita la riga.
- **I tre numeri di una scheda di statistiche perdevano la linea di base**
  quando un occhiello andava a capo: «SENZA CORRISPONDENZA» su due righe faceva
  scendere il suo numero di quindici pixel sotto gli altri due.

### Aggiunto — lo spettro, e non è finto

Il bottone c'era, spento, e il suggerimento diceva la verità: «il motore audio
non espone né campioni né bande, e disegnarne uno finto sarebbe l'unica bugia
dell'interfaccia». Adesso li espone, quindi la ragione per cui era spento non
c'è più. La regola non è cambiata: quel che si disegna viene dal suono che esce.

- **`aether-play::spettro`**: i campioni si prendono nella callback audio —
  l'unico posto che vede quel che esce davvero — e passano da un **terzo anello
  senza lucchetti**, dopo i due che il crate aveva già. Il `push` che fallisce
  perde il campione, ed è la gerarchia giusta: l'alternativa sarebbe far
  aspettare la callback, cioè l'unica cosa che il tempo reale vieta.
- **Dopo l'equalizzatore e prima del volume.** Dopo, perché una curva che alza i
  bassi si deve vedere; prima, perché uno spettro che si abbassa quando si
  abbassa la manopola descrive la manopola e non la musica.
- **Trasformata radix-2 da 4096 punti, scritta a mano.** La regola del repo è
  che una dipendenza si argomenta, e per una trasformata reale di lunghezza
  fissa l'argomento non regge. Quattromila punti e non mille: a 48 kHz sono
  11,7 Hz per bin, e la banda dei 31 Hz occupa due bin — con mille ne avrebbe
  occupato mezzo, e la prima barra avrebbe mostrato la continua invece del
  basso. Cinque prove più Parseval.
- **La continua si toglie prima di finestrare.** Moltiplicare un valore fisso
  per una finestra di Hann dà la finestra, il cui spettro sborda sui bin accanto
  — che a 48 kHz cadono dentro la banda dei 31 Hz. Senza, un offset qualunque
  nella catena si vedrebbe come un basso enorme che non c'è.
- **Si somma l'energia dell'ottava, non se ne fa la media.** Le bande d'ottava
  hanno larghezza proporzionale: due bin per i 31 Hz, novecento per i 16 kHz.
  Mediando, gli acuti restavano a zero anche su un pezzo che ne è pieno. È stato
  trovato guardandolo.
- **Le bande sono le dieci dell'equalizzatore**, così `eq-bars` ed `eq-slider` —
  due parti che il registro mette una accanto all'altra — descrivono la stessa
  cosa: la barra sopra il cursore dei 250 Hz dice quanta energia c'è a 250 Hz.
- **Trenta eventi al secondo, e solo mentre la schermata è aperta.** Non è una
  contraddizione con i quattro della posizione: quella la finestra la sa
  interpolare, le bande no. Spento, la callback non scrive e il filo non manda.
- **`prefers-reduced-motion` non lo spegne: lo ferma.** Le barre restano vere e
  si ridisegnano quattro volte al secondo.

### Aggiunto — tre buchi del formato skin

- **`blurBehind` è agganciabile a una parte.** `PartAppearance` ha ora `filter`
  accanto a `clip`, letto con `EffectTarget::Filter` — che esisteva già, sapeva
  già di finire in `backdrop-filter`, e non aveva un campo che lo riferisse: una
  skin che ci provava riceveva un errore giusto su una strada che non esisteva.
  E il **costo entra nel budget**: `effects()` sommava i soli sfondi, quindi una
  sfocatura da dieci punti — il budget intero — sarebbe passata in silenzio.
- **Il fotogramma scuro all'avvio con una skin chiara è chiuso.** La finestra
  nasce con `"visible": false` e la mostra il comando `pronto`, due fotogrammi
  dopo che skin e tema sono sul documento. Una rete di sicurezza in Rust la
  mostra comunque dopo due secondi: un'applicazione invisibile sarebbe un
  guasto peggiore del difetto che si stava togliendo.
- **L'accento segue la copertina, e il contrasto lo decide OKLCH.**
  `capabilities.dynamicAccent` era dichiarato, `dynamic_tokens` compilato, e
  nessuno ne estraeva un colore — perché `--accent` è anche un colore di
  **testo** e la tinta viva di un disco non garantisce 4,5:1. Adesso la tinta si
  estrae in Rust dalla miniatura già sul disco (`aether-app::tinta`: sacche di
  dieci gradi sulla tonalità, media circolare, nero, bianco e grigi scartati) e
  `aether-skin::dinamico` la porta in OKLCH, la fa scorrere **in chiarezza** in
  entrambe le direzioni a partire da quella della copertina, e prende la prima
  che regge la soglia su tutte le superfici del tema *e* per il testo che ci va
  sopra. Tonalità e croma restano quelli del disco; se nessuna chiarezza passa,
  la risposta è `null` e vince l'accento della skin. Tre cose che contano più
  della meccanica:
  - **La soglia è la stessa di `check_skin`** — stesse superfici, stessa
    `CONTRASTO_MINIMO`. Un accento scritto dalla copertina supera esattamente le
    prove di uno scritto a mano: non ci sono due definizioni di «accento
    leggibile» nello stesso programma.
  - **Niente canvas.** Leggere i pixel nella finestra avrebbe imposto un
    `Access-Control-Allow-Origin` sul protocollo `aether-cover`, cioè allargare
    per una decorazione un contratto tenuto stretto apposta, e messo
    venticinquemila pixel sul filo dell'interfaccia a ogni cambio di brano.
  - **Si scrivono solo i quattro token della famiglia**, con l'alfa che la skin
    aveva già dichiarato, e solo se la skin dichiara `dynamicAccent: true`. Il
    testo non segue mai la copertina, e un disco in bianco e nero non cambia
    niente. L'interruttore in Impostazioni smette di essere spento; su una skin
    che dice di no resta impedito, con quella ragione a schermo.

### Corretto — in questa riscrittura

- Il riordino di una playlist violava il vincolo `PRIMARY KEY (playlist_id,
  position)` quando lo spostamento andava all'indietro, perché SQLite controlla
  il vincolo a ogni istruzione e l'ordine in cui tocca le righe non è
  documentato. Ora l'ordine si riscrive per intero invece di spostarsi in place.
- Il segnaposto della copertina mancante non aveva misure nelle righe
  dell'elenco né nell'intestazione dell'album: il CSS puntava a una classe
  (`.segnaposto`) che nessun componente emetteva.
- `color.text.3` della skin di serie era `rgba(255,255,255,.38)` su `#09090d`,
  cioè **3,47:1**, sotto la soglia di 4,5:1 che il crate stesso dichiara. Ora
  `.46`, che dà 4,71:1. È il colore delle durate e dei metadati secondari — quasi
  tutti i numeri dell'elenco.
- Scegliere una skin e provarne una col mouse sono **due chiamate asincrone
  senza un ordine fra loro** che finiscono nello stesso posto: il testo di un
  unico `<style>`. Quando il puntatore lascia una scheda parte l'anteprima che
  rimette la skin *attiva in quel momento*, e se quella risposta arriva dopo una
  scelta appena fatta riscrive il foglio con la skin di prima — l'elenco direbbe
  «in uso» sulla nuova e la finestra resterebbe del colore vecchio. Ora un
  contatore di giri, e vince l'ultima richiesta partita: anche quando è
  un'anteprima, altrimenti passare col mouse sulle schede subito dopo aver
  scelto non mostrerebbe più niente. Trovato leggendo, non a schermo: le due
  strade si incrociano solo con un puntatore vero, e il pilota automatico con
  cui il resto è stato provato non sa produrre un `mouseenter`.
- L'anteprima di una skin toglieva l'accento della copertina e non lo rimetteva.
  `applicaSkin` lo azzera apposta — un accento ritagliato sul contrasto di
  un'altra skin non vale niente — ma provare una skin col mouse non cambia né il
  brano né il tema né la skin scelta, cioè nessuna delle dipendenze dell'effetto
  che lo scrive: passare sopra una scheda e andarsene lasciava l'accento della
  skin fino al brano dopo. Ora la **revoca** dell'anteprima lo rimette; durante
  l'anteprima resta tolto, perché il nucleo taglia l'accento sulle superfici
  della skin scelta e a schermo c'è quella provata. Stessa strada dell'altro, e
  stessa nota: verificato leggendo.
- Il tema chiaro dichiarato in `capabilities` non esisteva: `themes.light`
  sovrascriveva otto voci — la barra laterale, il capello e le sei semantiche —
  e lasciava superfici e testo scuri. Chi lo sceglieva otteneva un tema scuro con
  le semantiche sbagliate. Ora dichiara tutti i token che deve, e un test lo
  prova leggendo i contrasti invece di contare le chiavi.

### Corretto — la navigazione, provandola

Sette difetti trovati **usando** la finestra, non leggendola. Tre erano comandi
che si accendevano senza fare niente; gli altri si vedevano solo a schermo.

- **`selection-bar` e `queue` non erano nell'albero di serie.** Erano nel
  registro dei widget, nel registro delle parti e nel renderer, e `default_shell`
  non li montava: il tasto «Coda» della barra si accendeva e non apriva niente, e
  scegliere delle righe faceva **sparire** il lettore — che si nasconde apposta
  per lasciare il posto a una barra che nessuno aveva montato — senza niente al
  suo posto. Un test copre ora tutte e nove le varianti dell'albero.
- **L'intestazione e il corpo decidevano in due ordini diversi quale pagina si
  sta guardando.** Aprendo un album dalla pagina di un artista si vedevano le sue
  tracce sotto il titolo dell'artista — senza copertina e senza «Riproduci» — e
  il tasto «‹ Artisti» portava *avanti*, nella pagina dell'album. I due elenchi
  di casi ora hanno lo stesso ordine, il tasto torna dove si è entrati e lo dice,
  ed `Escape` chiude un livello per volta invece di due.
- **«In riproduzione» a schermo intero lasciava aperta la terza colonna**:
  copertina, titolo, trasporto, giudizio e coda disegnati due volte affiancati,
  con lo schermo intero schiacciato in mezza finestra. E la sua copertina si
  misurava in `vh` — la finestra intera — invece che sullo spazio che ha davvero,
  quindi su una finestra da 820 pixel il titolo si tagliava sulla seconda riga.
- **Un clic su una playlist da Impostazioni non faceva niente**: la playlist si
  accendeva nella barra e la pagina restava quella delle impostazioni.
- **L'interruttore «L'accento segue la copertina» era uno stato locale che
  nessuno leggeva.** Il compilatore elencava i token che avrebbero seguito la
  copertina (`dynamic_tokens`) e nessuno, né nella finestra né in Rust, ne
  estraeva un colore. Ora la funzione c'è davvero, e l'interruttore la comanda:
  vedi «tre buchi del formato skin» qui sopra.
- **Il tasto che toglie una riga da una playlist andava a capo**: `.riga`
  dichiarava sette colonne e si affidava a `grid-auto-columns` per l'ottava, che
  è la proprietà delle tracce implicite di *colonna* — con il flusso per righe
  l'ottavo figlio va su una riga nuova, e la × si vedeva sotto la sua riga.
- **La barra della selezione finiva sotto la terza colonna**, «Chiudi» compreso:
  la notifica aveva già la regola che la ferma prima, questa no.

### Aggiunto — i controlli che c'erano solo come spiegazione

Cinque cose che il nucleo faceva già e che dalla finestra non si potevano né
vedere né cambiare. Non erano funzioni mancanti: erano funzioni **presenti e
non raggiungibili**, il che è peggio, perché il codice che le fa continua a
girare e nessuno può correggerlo se sbaglia.

- **La cartella degli scaricamenti si sceglie.** `download.folder` era letta in
  `scarica.rs` e non la scriveva nessuno: la coda yt-dlp finiva sempre nella
  prima cartella sorvegliata, e quale fosse dipendeva dall'ordine in cui erano
  state aggiunte. La scheda mostra dove i brani finiscono davvero — la scelta o
  il ripiego, distinti — e **avvisa** quando la cartella scelta sta fuori da
  quelle sorvegliate: lì i file arrivano e in libreria non compaiono, che per
  chi guarda è indistinguibile da uno scaricamento fallito.
- **ReplayGain si spegne.** Il motore lo applica da sempre con il riferimento a
  −18 LUFS; l'interruttore era disegnato spento con la sua ragione accanto.
  Adesso c'è `player.replaygain`, e il valore di serie è **acceso** — non per
  preferenza ma per continuità: spegnerlo di nascosto in un aggiornamento
  cambierebbe il volume di chi ha i tag senza che niente lo spieghi.
- **La cronologia d'ascolto si legge.** `play_history` si scriveva dal primo
  giorno e la linguetta accanto a «Coda» era spenta. Era un difetto piccolo
  finché quella tabella conteneva solo gli ascolti fatti qui dentro; dopo
  l'importazione di un account Spotify contiene anni, e una schermata che non li
  mostra è la differenza fra aver importato e non averlo fatto. Ogni riga dice
  anche **da dove viene**, che è l'altra metà di `source`.
- **Il viaggio di ritorno si vede.** `scarico:riconciliato` partiva e non lo
  ascoltava nessuno. Non era solo una notizia mancante: quella passata **chiude**
  delle righe di `spotify_wanted` senza passare per la coda, quindi il pannello
  restava a mostrare dei brani «in attesa» che non esistevano più.
- **Un salvataggio su Drive ha una barra.** `nuvola:avanzamento` lo ascoltava
  solo la finestrella del ripristino: un «Salva adesso» dalle impostazioni
  mostrava «Salvataggio in corso…» e nient'altro, per decine di secondi.

### Aggiunto — `AETHER_DATI`, per provare senza rischiare

La variabile d'ambiente sostituisce la cartella dati dell'applicazione. Esiste
perché quasi tutto ciò che Aether fa di irreversibile — riordinare i file sul
disco, importare un account intero, ripristinare un backup — si può leggere in
una prova unitaria e si può **giudicare** solo guardandolo succedere in una
finestra vera, su una libreria che somiglia a quella di qualcuno. Senza,
quelle due cose sono la stessa libreria: la sola, quella dell'utente. Con,
si copia il database in una cartella qualunque e si rompe pure tutto.

Si legge in un punto solo, all'avvio; il resto del programma vede un percorso e
basta, come prima.

### Corretto — un'assenza scritta come stringa vuota

`nuvola.email`, `nuvola.file_id`, `nuvola.impronta` e `nuvola.client_id` si
«cancellavano» scrivendoci dentro `""`. Stringa vuota e riga assente sono due
stati diversi, e ogni lettore doveva ricordarsi di un `.filter(|e| !e.is_empty())`
per non mostrare un account senza nome al posto di nessun account. Adesso c'è
`settings::forget`, che toglie la riga — e c'è la ragione che basterebbe da
sola: un'identità che l'utente ha chiesto di dimenticare non resta scritta in un
file che il backup copia via.

### Difetti noti

- **La riproduzione si è fermata a 0:08 e la finestra continuava a dire che
  suonava.** Trovato provando la fase 8 sulla libreria vera: il brano parte, la
  posizione avanza per qualche secondo e poi si blocca; pausa e ripresa cambiano
  l'icona, un salto sul cursore non fa niente. Il sospetto è il dispositivo
  audio che smette di consumare — `Sonic Studio Virtual Mixer` è il predefinito
  su questa macchina — e il fatto che **nessuno lo direbbe** è il difetto
  certo: `Motore::perso()` esiste, `uscita.rs` alza quel bit quando `cpal`
  segnala un guasto del flusso, e in tutta l'applicazione non c'è una riga che
  lo legga. Un dispositivo perso oggi è indistinguibile da un brano che non
  parte.
- Una cosa che il nucleo sa fare e l'IPC non espone: i testi in
  `tracks.lyrics`. Il suo controllo è disegnato **spento, con la ragione a
  schermo**, invece di essere omesso o — peggio — finto. Le bande dello spettro,
  la normalizzazione del volume e la cronologia d'ascolto erano le altre tre
  voci di questo elenco e non lo sono più.
- **L'accento dinamico muove i token, non i colori riscritti per esteso.** Le
  quattro voci della famiglia e tutto ciò che il compilatore ha reso
  `var(--accent)` seguono la copertina; una skin che avesse ripetuto lo stesso
  colore **alla lettera** da un'altra parte — un bordo, un'ombra — resterebbe
  indietro, e la finestra si vedrebbe per metà di un colore e per metà
  dell'altro. Non c'è modo di accorgersene da soli: due colori uguali nel
  documento non dichiarano di essere lo stesso colore. Le due skin di serie non
  lo fanno.
- **La coda che parte da un elenco impaginato è lunga quanto le pagine
  scaricate.** «La coda è la lista che si sta guardando» resta la regola, ma
  adesso quella lista cresce mentre si scorre: partire dal primo brano di una
  libreria da duecentocinquanta ne accoda duecento, e dopo aver scorso fino in
  fondo tutti. Non nasconde niente che si potesse vedere — prima il tetto era
  duecento e basta — ma è una differenza che va detta.

### Aggiunto — nella riscrittura precedente (TypeScript/Electron)

Quel che segue è del vecchio albero. Resta qui perché il nucleo Rust ne eredita
le decisioni, non il codice.

- **Nucleo degli errori**: un `AppError` come record serializzabile, con
  catalogo unico di dominio, gravità, ritentabilità e chiave i18n. Attraversa
  l'IPC e la rete senza perdite, al posto delle dieci sottoclassi che nel vecchio
  albero morivano al primo salto.
- **Contratto IPC tipizzato**, logger strutturato, supervisor, e uno strato di
  resilienza (retry, timeout, circuit breaker, rate limiter).
- **Strato database**: driver astratto, catena di migrazioni unificata dalle due
  divergenti, apertura con diagnosi, parità provata invece che concordata.
- **Riproduzione**: stato esplicito, errori dei motori tradotti in codici, recupero.
- **Formato skin `.aeskin`**: una skin è dati, non CSS. Registro dei token,
  effetti parametrici con costo dichiarato, parts registry, compilatore, e un
  formato di pacchetto con le guardie di un archivio non fidato (path traversal,
  zip bomb, tipo mentito).
- **Libreria delle skin installate**, con filesystem iniettato per girare sia su
  `node:fs` sia sullo Storage Access Framework di Android.
- **Trasporto skin**: rotte, esecuzione di un piano di allineamento, e le due
  estremità su HTTP. Provato su una porta vera, con due librerie che si allineano
  nei due sensi.
- **Logica dello Skin Studio**: bozza e verifica di contrasto.
- **Infrastruttura di rilascio**: configurazione electron-builder, workflow di CI
  e di rilascio, guardia sull'allineamento delle versioni.
- **Scansione completa**: dal disco alla libreria interrogabile. Scrittura a
  lotti, ognuno nella sua transazione — una scansione interrotta lascia una
  libreria giusta e incompleta, che la passata dopo finisce, invece di non
  lasciare niente. Aggregati ricostruiti dai brani, ricerca FTS5 con
  virgolettatura degli operatori. Misurata sulla libreria vera: 1421 brani in
  19,5 s la prima volta, 0,1 s la seconda.
- **Un brano che si sposta non è un brano nuovo**: le sparizioni si appaiano ai
  file nuovi per chiave di brano, e la riga si aggiorna invece di essere
  cancellata e ricreata. Senza, riordinare la libreria — la funzione aggiunta
  poco prima — azzererebbe conteggi d'ascolto, preferiti e valutazioni di ogni
  brano, e li toglierebbe da tutte le playlist.
- **Fusione delle statistiche** (`aether-domain::merge`): commutativa e
  idempotente, con ogni regola scelta nella direzione che non distrugge — un
  conteggio sale e non scende, uno zero non cancella un voto, un preferito tolto
  non se lo rimette l'altro dispositivo. Serve all'importatore e servirà identica
  alla sincronia fra dispositivi.
- **Importatore dal vecchio database**: porta conteggi d'ascolto, voti,
  preferiti, cronologia, playlist e lapidi — le uniche cose che una scansione non
  può ricostruire. Il vecchio database si apre in sola lettura e non viene
  toccato. Sulla libreria vera: 261 brani ritrovati su 297, 409 ascolti, 82 righe
  di cronologia, la playlist automatica con le sue regole.

### Corretto

Difetti trovati nel vecchio albero e non riportati nel nuovo:

- `classifyDownloadFailure` aveva default opposti su desktop (`permanent`) e
  mobile (`transient`) per lo stesso guasto.
- Otto chiavi i18n orfane, per una tabella di errori duplicata in tre posti.
- Il codice d'errore di ExoPlayer veniva scartato; Howler mostrava «2» in UI.
- `AppError.from` cercava l'errno solo in cima al valore ricevuto: `fetch`
  riporta un rifiuto di connessione come `TypeError: fetch failed` con
  `ECONNREFUSED` un anello più sotto, e il caso più comune del trasporto LAN
  perdeva dominio e ritentabilità.

## [1.0.0] — 2026-07-17

Pubblicata dal vecchio albero, che resta in `legacy/Aeter/` come riferimento in
sola lettura; l'installer è `legacy/Aeter/release/Aether Setup 1.0.0.exe`. Le
modifiche precedenti a questa riga non sono state ricostruite: la cronologia
è nel git log del vecchio albero.
