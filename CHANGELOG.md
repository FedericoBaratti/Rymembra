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

## [2.1.0] — 2026-09-03

**Minor e non patch, per via del database.** Questa versione porta due
migrazioni, `015_riconcilia` e `016_metadati`, e per la regola in cima a questo
file ogni migrazione impone una minor. Non è un dettaglio contabile: le
migrazioni vanno solo avanti, quindi tornare da qui alla 2.0.1 non è
disinstallare e reinstallare — è ripristinare una copia del database fatta
prima di aggiornare. Il numero è l'unico posto in cui quella differenza si vede
senza aver letto questo file.

`SKIN_FORMAT_VERSION` resta 1, come nella 2.0.1: una skin scritta da una copia
aggiornata si apre ancora su una ferma.

### Corretto — la rete che se ne va non racconta più di essere un file rotto

Il giro precedente aveva chiuso le tre strade da cui il guasto di rete entrava
dichiarandosi altro. Ne restavano cinque aperte, e tre di loro riaprivano
esattamente il difetto che si era appena chiuso.

**Il salto diceva «file danneggiato».** Trascinare il cursore mentre la share
moriva usciva come `playback.decodeFailed`, che il catalogo dichiara mai
ritentabile: niente «Riprova», nessun punto annotato a cui tornare, e il
consiglio di sostituire un file sano. Su FLAC è la strada più esposta —
`SeekMode::Accurate` va al punto e poi ridecodifica fino al fotogramma esatto,
passando anche per la seek table — ed era pure la strada che «Riprova» stesso
percorre. Adesso il salto passa dallo stesso riconoscimento della lettura.

**E l'apertura diceva «formato non supportato».** Peggio: symphonia, quando il
riconoscimento del contenitore non riesce, dice «nessun lettore adatto» e
**butta via** l'errore di sistema che glielo ha impedito. Da fuori, una
condivisione morta e un file che non è musica arrivavano identici. Adesso il
numero del sistema viene conservato di lato mentre il flusso passa, e alla fine
si guarda: se sotto c'era la rete, la rete si dice.

**Il riconoscimento del contenitore stava fuori dalla scadenza.** I cinque
secondi coprivano la `File::open` e basta, cioè la parte veloce; tutta la
lettura dell'intestazione — su FLAC anche la copertina incorporata, spesso
centinaia di kilobyte — avveniva dopo, sul filo della decodifica, dove non c'è
nessun posto in cui infilare una deadline. Il motore adesso riceve un brano
**già aperto**: aprirlo è passato di qua dal confine, dentro la scadenza.

**Il brano successivo si apriva sul filo che suona.** L'osservatore degli
eventi gira sul filo della decodifica, e a ogni cambio di traccia chiedeva di
preparare il prossimo: fino a cinque secondi di stallo contro un anello che a
48 kHz stereo vale poco più di tre secondi, cioè un buco udibile a ogni cambio
di traccia su una rete che risponde male. Adesso è un colpetto su un canale, e
il lavoro lo fa un filo suo. Per la stessa ragione `avvia_corrente` non tiene
più il lucchetto del lettore mentre apre: decide, lascia, apre, riprende e
controlla che la coda non sia cambiata nel frattempo.

**Una scansione a metà cancellava brani.** `walkdir` salta in silenzio i rami
che non rispondono, e la risonda che protegge le radici morte scattava **solo
se la camminata era tornata completamente vuota**. Su una libreria grande la
share fa in tempo a rispondere per i primi mille file e a morire sugli altri
centomila: quei centomila diventavano «spariti», cioè da cancellare, voti e
cronologia compresi — e la scansione chiesta a mano non ha guardia anti-strage,
apposta. Adesso una camminata dichiara se ha perso dei rami, e una parziale
vale come una vuota: si risonda, e se la radice non risponde più non si toglie
niente. Nello stesso giro, una radice che muore a lettura iniziata viene
**abbandonata** al primo guasto invece di essere interrogata file per file: una
sola attesa lunga invece di una per ognuno dei file rimasti.

**E le prove.** Nessuna decodificava un FLAC vero: adesso c'è un campione da
centocinquantaquattro byte in `aether-play/tests/campioni`, e tre prove che ci
passano sopra — decodifica con ricampionamento, salto, e share che muore a metà
intestazione. Più: la sonda delle radici, quella vera con la scadenza, che i
doppi in memoria avevano sempre sovrascritto; e un controllo che ogni `.sql`
sul disco stia davvero nell'elenco delle migrazioni — il verso che mancava, ed
è quello da cui era passata `016_metadati`.

### Corretto — un cavo di rete staccato non porta più via né la finestra né la libreria

Aether apre i file musicali con `std::fs`. Su una condivisione SMB che smette di
rispondere — server spento, VPN caduta, lettera di unità mappata senza più
niente dietro — quelle chiamate non falliscono: **aspettano**, per i quaranta
secondi del timeout di Windows. Per tutti e quaranta la finestra era dichiarata
«non risponde». Questo giro chiude il buco da tutti i lati da cui entrava.

**La libreria non si svuota più per un NAS spento.** Una scansione che non
trovava più la cartella del NAS concludeva che i brani lì dentro non
esistevano, e li cancellava. Adesso una radice che non risponde viene
**saltata** invece che considerata vuota, e la scansione non automatica lo dice
a schermo: quali radici non hanno risposto e quante rimozioni ha trattenuto. Le
scansioni che nessuno sta guardando — quella che parte da sé dopo gli
scaricamenti — sono per di più *prudenti*: nel dubbio non cancellano niente e lo
lasciano scritto nel diario. Una libreria persa per un cavo staccato costa ore
di rifacimento; una riga in più nel diario non costa niente.

**La finestra non si pianta più aprendo un brano.** L'apertura del file usciva
da sotto i due lucchetti — libreria e lettore — e girava sul filo principale:
premere play su un brano di un NAS spento congelava tutto per quaranta secondi.
Adesso la riga di database si legge sotto lucchetto, il file si apre **fuori**,
con una scadenza di cinque secondi, e i comandi che possono finire su un
percorso di rete girano su un filo di lavoro: riproduzione, testi, tag,
copertine, skin, Studio, sincronia. Passata la scadenza compare l'avviso di
rete, e la finestra ha risposto tutto il tempo.

**Una condivisione che muore a metà brano non dice più che il file è rotto.**
L'errore 64 di Windows (`ERROR_NETNAME_DELETED`) arrivava fino in fondo travestito
da «questo file è danneggiato: sostituiscilo e rifai la scansione» — il
consiglio peggiore possibile, perché manda a buttare un file sano. E subito
dopo il motore passava al brano successivo, che sta sulla stessa condivisione
morta: altri quaranta secondi, un altro avviso, e così via **lungo tutta la
coda**. Adesso il guasto di rete si riconosce per quel che è, la riproduzione si
**ferma** dove stava invece di scorrere, e l'avviso porta un «Riprova» che
riprende dal punto in cui la musica si era interrotta — non dall'inizio del
brano. Un file davvero rotto continua a costare un fruscio e a far saltare al
brano dopo: la distinzione la fa il codice dell'errore, non il fatto che ci sia
un errore.

**Quel che resta fuori, dichiarato:** i quaranta secondi di silenzio prima che
l'errore arrivi, quando la share muore **mentre si legge un brano già
avviato**. Quella lettura è dentro il decodificatore, non c'è un punto in cui
infilare una scadenza, e interrompere un filo fermo in una `ReadFile` non lo
consente nessun sistema operativo. Sta scritto anche nel commento del codice,
dove qualcuno lo cercherà. L'apertura, invece, adesso è coperta: vedi il giro
successivo qui sotto.

**Il messaggio mancava.** Dei 107 codici del catalogo, `fs.networkUnavailable`
era l'unico senza una frase tradotta: chi lo incontrava leggeva il testo di
sistema di Windows invece della frase che dice cosa fare. `strumenti/lingue.js`
adesso confronta il catalogo Rust con `it.json` e fa fallire `verify` se un
codice resta senza frase — prima confrontava solo le lingue fra loro, e una
chiave mancante da *tutte* era invisibile.

### Aggiunto — la finestra lascia scritto anche quando è lei a cadere

Il diario raccoglieva tutto quel che succede nel nucleo e niente di quel che
succede nell'interfaccia. Un errore JavaScript non catturato smontava l'albero
React e lasciava una finestra **bianca**: nessun messaggio, nessun tasto, e in
rilascio nemmeno una console da aprire.

Adesso c'è un recinto attorno all'applicazione, che al posto della finestra
bianca disegna cos'è successo e un tasto per ricaricare, e due ascoltatori
globali per i guasti che il recinto non può vedere — quelli dei gestori d'evento
e le promesse rifiutate. Tutti e tre scrivono nel diario attraverso lo stesso
comando.

Il recinto sta attorno alla sola applicazione: i simboli e i tre comandi della
finestra restano fuori, perché una finestra che perde il tasto di chiusura
entrando in una schermata di guasto sarebbe un guasto peggiore di quello che sta
raccontando. E quel che finisce nel diario è **una riga sola**, tagliata sui
caratteri: uno stack trace del davanti si porta dietro gli URL dei moduli, che
in sviluppo sono percorsi del disco di chi sta lavorando, e `PRIVACY.md` promette
che nel diario i percorsi non ci finiscono.

### Corretto — le copertine che non si salvano tornano a contarsi

La schermata di scansione ha sempre avuto una riga per le copertine che non si è
riusciti a salvare, e da qualche tempo quella riga non compariva mai: l'errore
veniva buttato via appena sotto, e il conto arrivava a zero per costruzione. La
strada è stata rimessa in piedi per intero, dalla lettura del tag fino al numero
a schermo.

Nello stesso giro, la migrazione `016_metadati` — le colonne su cui poggia la
scheda della salute dei metadati — era sul disco ma non nell'elenco di quelle da
applicare: esisteva come file e non è mai stata eseguita da nessuna libreria.
Adesso c'è.

### Corretto — trentotto guasti, trovati leggendo invece che aspettando

Una passata su tutto l'albero, crate per crate, con una regola sola: niente
stile, solo cose che si comportano diversamente da come sono scritte. Quel che
segue è raggruppato per quanto costa a chi ascolta, non per file.

**Perdita di dati.** Il backup su Drive rifondeva solo i file firmati da un
altro dispositivo: dopo la prima fusione l'unione portava la *propria* firma, e
la volta dopo veniva sovrascritta invece che fusa — i conteggi degli altri
dispositivi sparivano al secondo salvataggio. Adesso si fonde sempre, che è
un'operazione idempotente e non aveva mai avuto bisogno di quella guardia. Nello
stesso file, un backup remoto illeggibile veniva messo da parte come
`.corrotto-…` e poi sovrascritto dal salvataggio successivo, che puntava ancora
al suo identificativo: la copia di sicurezza durava meno di un minuto. Ora un
remoto messo da parte fa nascere un file nuovo, e la copia resta.

`riconcilia` — il passo che rimette in playlist i brani arrivati dalla coda di
scarico — non aveva modo di sapere quali righe avesse già ricollocato: nessun
filtro sullo stato, nessun controllo di presenza. Un brano tolto a mano da una
playlist ci rientrava alla riconciliazione dopo, per sempre, e con le posizioni
compattate ci rientrava *due volte*. La migrazione `015_riconcilia` aggiunge
`desiderati.placed_at` (le righe già chiuse vengono marcate all'aggiornamento,
così nessuna libreria esistente si trova i brani rimessi tutti insieme), e
adesso ogni riga ricolloca al più una volta. Nello stesso giro, l'`updated_at`
delle playlist si tocca solo se qualcosa è davvero rientrato — prima bastava una
riconciliazione a vuoto per far vincere il lato sbagliato della sincronia.

La potatura delle cartelle vuote dopo un riordino risaliva da una radice comune
che, per due brani su dischi diversi, è il percorso **vuoto**: `starts_with("")`
è vero per tutti, e la risalita arrivava alla radice del disco. Una radice vuota
adesso non pota niente. E il nome del file scaricato passava da
`with_extension`, che tratta come estensione tutto quel che segue l'ultimo
punto: «02 - Mr. Brightside» diventava «02 - Mr.mp3», e il ramo anti-collisione
generava novantotto volte lo stesso percorso.

**La finestra che si congela.** Quindici comandi lunghi — scansione, riordino,
importazioni, testi, sincronia — erano dichiarati senza `(async)`, cioè giravano
sul filo principale della finestra. La regola sta scritta da sempre in
`nuvola.rs` e non era applicata: per i venti secondi della prima scansione gli
eventi di avanzamento partivano e nessuno li disegnava, e `annulla_scansione`
non veniva nemmeno ricevuto finché la scansione non era finita da sola.

**La riproduzione che diceva una cosa e ne faceva un'altra.** Togliere dalla
coda il brano in corso mostrava subito il titolo del successivo mentre le casse
continuavano con quello tolto — e alla sua fine la coda avanzava ancora,
saltando il brano appena annunciato. A coda finita il motore lasciava scritto
l'ultimo `track_id`: «riprendi» chiedeva a un motore vuoto di ripartire, cioè
non faceva niente, mentre il pulsante diventava «pausa». Lo scrubber rimbalzava
al punto di partenza a ogni trascinamento, perché i comandi al motore sono
accodati e lo stato partiva con la posizione *di prima* del salto. I salti
relativi dalle cuffie tornavano a inizio brano, perché la posizione che il
sistema operativo usa come «da dove» la scriveva solo il cambio di stato. E il
timer di spegnimento, nel quarto di secondo fra la scadenza e la sua raccolta,
mostrava lo zero che significa «alla fine di questo brano».

**Cataloghi e metadati.** Su Archive.org le durate `H:MM:SS` non si leggevano —
il veto sulla durata non si applicava proprio ai concerti interi in cui serve —,
due tracce omonime dello stesso item collassavano in una, e l'album veniva
calcolato e poi buttato: tutto quel che veniva dall'archivio finiva in
«Singoli». Un 503 su una scheda buttava via i candidati già raccolti invece di
proseguire. Il riconoscimento dei domini si faceva per suffisso, quindi
`evilarchive.org` passava per `archive.org`. Su Audius la copertina dei brani
risolti da link era sempre assente. Nell'arricchimento, due fonti potevano
«concordare» quando una delle due era `None`, e l'identificativo MusicBrainz
della *registrazione* faceva da veto anche a quelli di pubblicazione: le
edizioni non si fondevano mai.

**Il resto.** Le playlist legacy si importavano senza cancellare prima, creando
playlist ibride; un `file://` POSIX perdeva la barra iniziale e diventava
relativo; il giornale del riordino, su una riga troncata a metà valore,
restituiva stringa vuota invece di dire che non si leggeva; le miniature già
presenti dichiaravano dimensioni quadrate fittizie; `create_smart` scriveva due
statement senza transazione; un'impronta di copertina arrivava a comporre un
percorso senza essere validata, mentre il controllo esisteva già a due file di
distanza; e l'unica cosa che la schermata delle importazioni sapeva dire di un
guasto della coda era «non è partita», perché leggeva il carico con nomi di
campo che quel record non ha.

- **La chiave di firma si guarda prima di compilare** (`strumenti/firma.js`, un
  passo nuovo in `release.yml`). `tauri build` la tocca per ultima: alla 2.0.1 il
  segreto era arrivato spezzato e la corsa è morta dopo tredici minuti, con un
  messaggio che parlava di password mentre il guasto era nella chiave. Adesso
  quel che si può controllare senza compilare — che sia base64 intera e non
  mandata a capo, che dentro ci sia una chiave *privata* e non la pubblica, che
  la password non porti un a capo in coda — si controlla in un secondo. Non
  stampa mai niente che venga dai segreti.

### Aggiunto — il README fa vedere il programma invece di descriverlo

Un lettore musicale con zero immagini nel documento che lo presenta: chi
arrivava dal motore di ricerca doveva fidarsi di duecento righe di prosa per
sapere se valeva la pena scaricare un `.exe` non firmato. Adesso in
`immagini/` ci sono sette scatti presi da una libreria vera — millequattrocento
brani — e ognuno porta accanto la cosa che l'immagine da sola non dice: perché
la home non è un elenco alfabetico, perché lo spettro si legge dopo
l'equalizzatore e prima del volume, cosa cambia una skin che non è un tema
scuro.

Insieme a loro, la sezione che mancava del tutto: **«Installare»**. Il
documento spiegava come si compila e non dove si scarica, e non diceva da
nessuna parte che SmartScreen blocca l'installer — che è il primo schermo che
una persona vede, e senza una riga che lo preveda sembra un antivirus che ha
trovato qualcosa. Il conteggio delle prove, fermo a «circa 1 260», è tornato
vero.

## [2.0.1] — 2026-08-27

**Il numero salta da 0.2.0, e non perché sia cambiato qualcosa di
incompatibile.** `SKIN_FORMAT_VERSION` e `SKIN_TRANSFER_PROTOCOL` sono quelli
della 0.2.0, il database non ha migrazioni nuove, e una copia ferma continua a
capirsi con una aggiornata. È una scelta di numerazione — la 1.0.0 del vecchio
albero sta più in basso in questo stesso file, e due alberi che si contendono la
stessa riga di versioni non è una cosa che si spiega due volte. Le regole di
sopra valgono da qui in avanti.

È anche la **prima release che passa dall'updater**: chi ha la 0.2.0 installata
la vede comparire da sé entro mezz'ora, e da quel momento la catena — tag, CI,
firma, `latest.json` — è quella che porterà tutte le prossime.

### Aggiunto — un posto per sostenere il lavoro

Un tasto **Sostieni** in fondo alla barra di navigazione, sopra Impostazioni e
sotto la riga sottile che separa «dove sono» dalle cose che non sono pagine.
Porta a `github.com/sponsors/…` nel browser di sistema.

Non apre niente dentro la finestra, e infatti non ha mai lo stato attivo né
`aria-current`: annunciarlo come «pagina corrente» a un lettore di schermo
sarebbe dire una cosa falsa. Nella barra in fondo — quella stretta, cinque voci
su una riga — il tasto non c'è: là lo spazio serve a chi sta andando da qualche
parte.

L'indirizzo non è scritto a mano. `Documento::Donazioni` lo ricava da
`CARGO_PKG_REPOSITORY` (`github.com/OWNER/REPO` → `github.com/sponsors/OWNER`),
cioè dalla stessa riga da cui vengono gli altri link pubblici, e se un domani
quella riga non avesse più la forma attesa il ripiego è il repository — non una
pagina inventata, non un 404. Le donazioni stanno nell'elenco chiuso dei
documenti pur non essendo un documento: quel che l'elenco tiene davvero non sono
i testi legali, sono **gli indirizzi che la finestra ha il permesso di far
aprire**, e una seconda serratura identica accanto alla prima sarebbe stata la
stessa cosa montata due volte.

Sul sito la stessa cosa, detta per esteso: «Gratuito, e resta gratuito», col
motivo — il Live Music Archive ha una clausola non commerciale, e far pagare il
programma la violerebbe. Le donazioni sostengono il lavoro, mai la musica. In
tutte e quattro le lingue del sito, più la voce nel piè di pagina.

### Corretto — i tasti dei documenti aprivano il vuoto

Il crate della finestra non ereditava `repository` dal workspace. Cargo non se
ne lamenta: definisce `CARGO_PKG_REPOSITORY` lo stesso, **vuota**. Il risultato
è che ogni indirizzo costruito da quella riga era un percorso senza radice —
licenza, avvisi sulle licenze, privacy, condizioni e segnalazioni chiedevano al
browser di aprire `/issues` e simili, e nessuno di quei tasti portava da nessuna
parte.

Una riga di manifesto per la correzione, e tre prove perché non torni: ogni nome
dell'elenco deve produrre un indirizzo che comincia per `https://`, un nome
fuori elenco non deve produrne nessuno, e le donazioni devono finire sul profilo
con un proprietario dentro. È il genere di guasto che il compilatore non vede e
che nessuno segnala, perché chi clicca pensa di aver sbagliato lui.

## [0.2.0] — 2026-08-27

**La prima versione che si può dare a qualcuno.**

Tre riscritture in fila, sullo stesso ramo di lavoro. La prima
(`aether/skin-system-e-core`, dal 26 luglio 2026) ha rifatto nucleo e sistema
delle skin; la seconda (`aether/rust-core`) ha portato tutto in Rust dietro una
finestra Tauri; la terza ha tolto tutto quel che rendeva Aether non
distribuibile — `yt-dlp` impacchettato nell'installer e lo scraping degli
endpoint privati di Spotify — e ha messo al loro posto i cataloghi liberi.

Due cose restano fuori, e sono dichiarate invece che nascoste.

**L'installer non è firmato.** Windows SmartScreen mostrerà «Windows ha
protetto il PC», e per procedere serve *Ulteriori informazioni* → *Esegui
comunque*. Non è una formalità da liquidare: un installer non firmato è un
installer di cui non si può verificare la provenienza, e l'unica cosa che si
può offrire in cambio è che il sorgente sta qui e si compila da sé. Un
certificato OV richiede la validazione dell'identità e un costo annuo.

**Il lato mobile non esiste.** Nel vecchio albero c'era; qui non è stato
riscritto, e `bundle.targets` produce solo NSIS — cioè Windows.

Quel che invece **c'è** e regge: 1264 prove che girano senza rete, un motore
audio gapless con ReplayGain ed equalizzatore, una libreria su SQLite che
riconosce i file spostati, il riordino con anteprima e annullamento,
l'arricchimento da MusicBrainz che davanti a prove insufficienti non scrive
niente, le skin col loro editor, il backup e la sincronia, lo scrobbling, i
testi da LRCLIB, gli aggiornamenti con verifica della firma, e due cataloghi
liberi da cui prendere musica che si può prendere.

### Aggiunto — un file remoto che si legge e si posiziona, e il client di Jamendo

Jamendo è **solo ascolto**: i loro termini vietano espressamente la cache e
l'accesso fuori linea, quindi `Fonte::puo_consegnare()` è `false` e da lì non
esce mai un brano scaricabile — non «di solito», ma sempre, perché lo decide
`Disponibilita::decidi` un gradino sotto e il modulo non ha modo di scavalcarla.

Ascoltare senza tenere vuol dire suonare qualcosa che non è un file, e Aether
non lo sapeva fare. Adesso c'è `aether_net::FlussoHttp`.

**Il motore audio non è stato toccato.** `aether_play::Sorgente` prende un
`Box<dyn Flusso>` — `Read + Seek + Send + Sync` — e non un percorso; il
commento che lo dice parlava di Android, «là non esiste un percorso da aprire
ma una concessione del sistema», e vale identico per un catalogo di solo
ascolto. La giuntura c'era già, aspettava qualcuno.

**Posizionabile, non in avanti.** Un decodificatore non legge dall'inizio alla
fine: cerca i tag in testa, poi in coda — ID3v1 e APE stanno *dopo* l'audio —
poi torna al primo fotogramma, e quando qualcuno sposta il cursore salta a
metà. Con un flusso in avanti ognuno di quei gesti sarebbe il file intero
scaricato per leggerne quattro kilobyte, tre volte prima di sentire una nota.
Da qui una finestra di 256 KB che scorre, e `Rete::intervallo`, che chiede quei
byte con `Range` invece di chiedere tutto.

**Niente disco, e non è un dettaglio implementativo.** Nessun file temporaneo,
e non ci sarà: il tetto della finestra è il vincolo dei termini di Jamendo, non
un'ottimizzazione. Un buffer che tenesse tutto quel che è passato sarebbe una
copia del brano in memoria, e una copia in memoria è una copia. Spostarsi non
chiede niente alla rete — la finestra si riempie alla prima lettura che ne ha
bisogno — ed è quel che rende gratis il «vai alla fine e torna» che ogni
lettore di tag fa all'apertura.

Sette prove, tutte senza rete: la logica della finestra è tutto quel che il
modulo contiene, e provarla contro un servizio vero vorrebbe dire non provarla.

Del client di Jamendo vale la pena dire due cose. Si prende `audio` e **mai**
`audiodownload`, che pure è pieno per certi brani: leggerlo sarebbe scoprire se
il server ce li lascerebbe prendere, che è una domanda diversa da «si può». E
Jamendo risponde `200` anche quando rifiuta — il verdetto sta in
`headers.status` — quindi leggere solo il codice HTTP vorrebbe dire dire a chi
ha sbagliato a incollare la chiave che il catalogo non ha quel brano.

**La feature `jamendo` nasce spenta**, ed è la parte che conta di più. La loro
API è gratuita per i soli usi non commerciali e i termini definiscono l'uso
commerciale come «any monetary compensation»; Aether si sostiene con le
donazioni. Se contino deve dirlo Jamendo — la domanda è a
`licensing@jamendo.com`, vedi `TERMS.md` § 2 — e finché non ha risposto, un
installer che usa quell'API a condizioni ignote è precisamente il genere di
cosa per cui questo programma è stato riscritto. Il codice c'è ed è provato
(`cargo test -p aether-catalogo --features jamendo`); il giorno della risposta
si accende con una riga.

### Aggiunto — Audius, il secondo catalogo

Aether ne aveva uno solo. `Cataloghi::cerca` raccoglieva i guasti in un vettore
e falliva «solo se nessuno ha risposto»; il pannello «Perché non funziona?»
distingueva «non risponde nessuno» da «chi risponde funziona»; la finestra
mappava già i nomi di tre fonti. Tutto scritto al plurale, per un catalogo solo.

Adesso sono due. Audius entra **senza toccare il motore audio**, e la ragione
sta nel dominio: `Fonte::puo_consegnare()` dice `true` per Audius perché là è
l'artista a decidere, brano per brano, se il suo pezzo si scarichi o si ascolti
soltanto. Quando dice di sì, il brano fa la stessa strada di un item
dell'Internet Archive — `preleva` scrive un file, la scansione lo porta in
libreria, e da lì è un brano come gli altri.

**I permessi si moltiplicano, non si sommano.** `Disponibilita::decidi` mette
insieme quel che la fonte consente e quel che la licenza consente; sopra i due
c'è un terzo veto che la licenza non conosce, e sono i brani **con cancello**.
Audius permette di chiudere lo streaming o lo scarico dietro un seguito o il
possesso di un gettone: un brano col cancello sullo streaming non entra
nemmeno nell'elenco — non lo si potrebbe sentire, e una playlist di brani che
non partono è peggio di una playlist più corta — e uno col cancello sullo
scarico scende a solo ascolto qualunque cosa dica la sua licenza. Il permesso
di un catalogo non è la somma dei suoi sì: è l'intersezione dei suoi no.

La licenza di Audius è un **campo di testo** che scrive l'artista, non un
codice: quel che arriva è «Attribution ShareAlike CC BY-SA», e il `by-sa` va
estratto da lì. Una prova percorre tutte le voci del loro menu, perché se una
smettesse di essere riconosciuta i brani che la portano diventerebbero
silenziosamente non scaricabili. Quel che non si riconosce vale
`Licenza::Sconosciuta`, che non permette la copia — il verso giusto in cui
sbagliare.

**Il nodo si sceglie una volta.** Audius non ha un server ma una rete di
discovery node, e `api.audius.co` dice quali sono in salute. Se ne prende uno e
lo si tiene per la sessione; se tace lo si dimentica e se ne prende un altro,
una volta sola. Il secondo silenzio non è del catalogo — è della rete di chi
ascolta — e insistere su venti nodi vorrebbe dire venti timeout prima di
poterglielo dire.

Da lì è venuta la cosa meno ovvia di tutto il modulo. Quel che finisce in
`desiderati.fonte_url` **non è un indirizzo**: è un percorso. Un indirizzo con
dentro il nodo di oggi, riletto fra un mese dalla coda che procura, punterebbe
a una macchina che non c'è più. Il nodo lo rimette `prepara`, al momento di
andare a prendere i byte, e sarà uno che risponde adesso.

Nello stesso percorso viaggia l'estensione del file originale, e anche questo
non è un vezzo: `Candidato::estensione` non sopravvive al giro in tabella —
la coda ricostruisce il candidato con `..Default::default()` — e il punto di
scarico di Audius finisce per `/download`, senza estensione. Senza il
suggerimento, un wav originale sarebbe finito sul disco chiamato `.mp3`. Il
suggerimento non viene spedito: serviva a noi.

Resta fuori Jamendo, che è solo ascolto e quindi vuole lo streaming — cioè un
lettore che sappia suonare qualcosa che non è un file sul disco. È il pezzo
dopo.

### Aggiunto — un diario, perché «non si apre» diventi una diagnosi

In rilascio Aether non aveva **nessun** modo di raccontare cos'era andato
storto. `main.rs` dichiara `windows_subsystem = "windows"`, che toglie la
console; i trentacinque `eprintln!` sparsi per l'applicazione — il filo che non
parte, il dispositivo audio che sparisce, il catalogo che rifiuta — finivano
quindi nel nulla appena fuori da `cargo run`. Finché la finestra si apre non è
grave, perché quasi ogni guasto ha una faccia. Il caso che conta è l'altro:
davanti a «non si apre» c'erano due informazioni, «non si apre» e «Windows 11»,
e nessuna delle due si può usare.

Adesso c'è `diario/` nella cartella dati: tre file di testo che si danno il
cambio a due megabyte, e il più vecchio se ne va. I `[modulo] cosa è successo`
non cambiano forma — sono la ragione per cui il file si legge in diagonale — e
`nota!` ha preso il posto di `eprintln!` una riga per una.

Due cose sono state aggiunte perché senza di loro il diario avrebbe coperto
solo i guasti che già si vedevano.

**Il gancio dei panici.** Il profilo di rilascio dichiara `panic = "abort"`:
niente svolgimento dello stack, nessun `catch_unwind`, nessun `Drop`. Il gancio
è letteralmente l'ultimo codice nostro che gira, e scrive filo, posizione e
messaggio. I panici propri restano vietati con `deny` in tutto l'albero — quelli
che arriveranno qui vengono dalle 429 crate sotto, che quella regola non la
rispettano.

**Ogni errore che attraversa l'IPC.** `errore.rs` diceva da mesi: «il giorno in
cui gli errori andranno anche nel log, il posto dove aggiungerlo è questo». Ci
va il codice e la causa, non il messaggio: `db.openFailed` è la stessa parola in
ogni lingua, mentre il messaggio è tradotto e scritto per chi ascolta.

Il diario **non lo manda mai nessuno**, e non c'è un comando che possa: un
programma che sa spedire i propri log da sé ha la telemetria, e `PRIVACY.md`
promette che qui non ce n'è. Dalla finestra si può solo aprire la cartella, con
un bottone in *Impostazioni → Aggiornamenti* — accanto a «Versione installata»,
che è dove arriva chi sta per segnalare qualcosa.

E siccome un file che si spedisce va guardato prima di prometterci sopra
qualcosa, due righe convertite sono state riscritte: una stampava il percorso
completo del file appena scaricato — che porta dentro il nome dell'account di
Windows e, per come `riordino` costruisce le cartelle, l'artista e l'album — e
adesso stampa la sola estensione, che è quel che serve a capire perché un tag
non si scrive; l'altra nominava il brano, e adesso ne dà il numero di riga.
`PRIVACY.md` § 8 elenca cosa ci finisce e cosa no, e adesso quelle due liste
sono vere.

Per scrivere l'istante davanti a ogni riga serviva convertire dei millisecondi
in una data, cosa che nel dominio non c'era: `giorni_dall_epoca` esisteva senza
la sua inversa. `data_dall_epoca` e `istante_iso` le stanno accanto, e una prova
percorre un secolo giorno per giorno per verificare che le due si annullino.

### Corretto — il testo restava fermo su brani di cui i tempi esistevano

Certe canzoni mostravano le parole con l'etichetta «senza tempi» e non
scorrevano, mentre altre dello stesso disco scorrevano benissimo. Non era una
questione di lingua, di formato o di durata: era quale voce del catalogo
rispondeva per prima.

LRCLIB tiene **più voci per lo stesso brano** — una per ogni edizione che
qualcuno ha caricato — e `/api/get`, la domanda esatta, ne restituisce una sola:
quella la cui firma (titolo, artista, album, durata al secondo) coincide con
quella del file. Non è detto che sia quella con i tempi. Per «The Auditels
Family» di Caparezza la firma esatta trova la voce caricata nel 2020, che porta
il solo testo piatto; le altre sei voci dello stesso brano, stessa durata al
decimo di secondo, l'LRC ce l'hanno. Aether si fermava alla prima e non chiedeva
mai le altre.

Adesso una risposta esatta **senza tempi** non chiude più la ricerca: si tiene da
parte e si fa comunque la domanda generosa, e se fra le voci che tornano ce n'è
una con i tempi che passa i veti di durata e di titolo, vince lei. Con i tempi,
o strumentale, la prima risposta resta l'ultima parola e la seconda richiesta non
si spende.

Da lì sono venute le altre tre metà dello stesso difetto. Un testo **piatto** non
conta più come «brano già fatto»: né per la coda della passata — che così smette
di saltare per sempre chi ha le parole dentro i tag dei propri mp3, cosa che il
codice dichiarava di non voler fare — né per il pannello, che va a chiedere
quando quel che ha in mano non scorre. Un `.lrc` senza tempi accanto al file non
copre più, e soprattutto non sovrascrive più, dei tempi già in tabella. E una
migrazione rimette in coda le vecchie risposte del catalogo che si erano fermate
al piatto: il testo che si ha resta lì da leggere intanto, ma la domanda torna
aperta.

### Aggiunto — una porta d'ingresso, invece di un elenco alfabetico

Aether apriva sulla griglia degli album. Le quattro destinazioni della libreria
— album, artisti, brani, preferiti — sono tutte e quattro un elenco ordinato, e
nessuna rispondeva alla domanda che uno si fa davvero aprendo un lettore:
*cosa stavo ascoltando*. Per riprendere il disco di ieri sera bisognava
ricordarsi come si chiamava e andarlo a cercare.

La Home ha quattro ripiani da dodici brani: **riprendi dov'eri**, con la
posizione dentro il brano; **ascoltati di recente**; **aggiunti di recente**; e
**angoli trascurati**, cioè dischi in libreria da più di sei mesi e mai
toccati. L'ultimo è il ripiano che solo una libreria locale può avere: nessun
servizio in streaming sa cosa possiedi e non ascolti, perché per lui non
possiedi niente.

«Riprendi dov'eri» ha richiesto un fatto che non era conservato da nessuna
parte: `QueueSnapshot` sapeva a che brano si era, non a che punto del brano.
Adesso la posizione sta in una chiave sua di `settings` e non dentro la coda —
`aether_domain::queue` descrive quali brani e in che ordine, e non sa cosa sia
un millisecondo.

**Nessun widget nuovo e nessuna modifica al registro delle skin**: la pagina
passa dallo slot `contenuto` che già esiste, quindi ogni skin già scritta la
mostra senza essere ritoccata. Una migrazione (`012_home`) per il solo indice
parziale su `last_played_at`, senza il quale «ascoltati di recente» era una
scansione di tutta la tabella.

Scartato: lo scorrimento infinito. Un ripiano si guarda, non si scorre — dodici
brani presi una volta sola all'apertura, e `usePagine` resta per le viste che
sono elenchi veri.

### Aggiunto — i tasti multimediali della tastiera, e la scheda di Windows

Il tasto play/pausa della tastiera non faceva niente se la finestra non era in
primo piano, e il riquadro che Windows 11 mostra nel flyout del volume era
vuoto: Aether suonava e il sistema operativo non sapeva cosa.

Adesso Aether si registra come sessione SMTC. Il riquadro mostra copertina,
titolo e artista, i suoi pulsanti funzionano, e — questo è il punto — **è
Windows a instradare i tasti multimediali**, che quindi arrivano anche da
un'altra applicazione a schermo intero.

Scartato `tauri-plugin-global-shortcut`: registrare i tasti multimediali come
scorciatoie globali li **ruberebbe a ogni altro programma**, e chi apre un
video mentre Aether è aperto si troverebbe il tasto pausa che mette in pausa la
cosa sbagliata. Registrare una sessione media è il contrario — è il sistema che
decide a chi tocca, come per ogni altro lettore.

La dipendenza è `souvlaki`. In tutto l'albero non c'è una riga di `unsafe` e il
`Cargo.toml` la vieta con `forbid`, che nessun `#[allow]` scavalca: l'`hwnd`
della finestra si passa a souvlaki senza dereferenziarlo, e il dereferenziamento
avviene di là.

### Aggiunto — la normalizzazione ha tre livelli, non un interruttore

Il bersaglio della normalizzazione ReplayGain c'era già dentro il nucleo, ma la
finestra poteva solo accendere e spegnere: il valore restava −18 dBFS
qualunque cosa si volesse. Chi ascolta in cuffia di sera e chi ascolta in
macchina non hanno lo stesso problema.

Adesso i livelli sono tre — **basso** (−23), **normale** (−18), **alto** (−14) —
più spento. Un valore con un nome e non il booleano di prima: uno spento che non
dice a quale livello tornerebbe è uno spento che chi riaccende deve scoprire per
tentativi.

Trovato provandolo: l'uscita in virgola mobile — che è il formato con cui
WASAPI si apre quasi sempre — **non tagliava i campioni fuori scala**. Con la
normalizzazione ferma a −18 non si notava; a −14 su un brano già forte il
moltiplicatore supera uno, e sarebbe diventata la regola invece dell'eccezione.
Adesso taglia, con una prova che lo tiene tagliato.

### Aggiunto — un timer di spegnimento, e «alla fine di questo brano»

Nel backend e non nella finestra: un `setTimeout` muore a ogni ricarica della
UI, e un timer che sopravvive solo finché nessuno tocca niente non è un timer.
Sta in un intero atomico accanto al lettore, letto quattro volte al secondo dal
filo dell'orologio che già c'era — fuori dal lucchetto, perché prenderlo quattro
volte al secondo per scoprire quasi sempre che non c'è niente da fare vorrebbe
dire contendere il lucchetto col tasto pausa.

Allo scadere **mette in pausa e non ferma**, così la posizione resta e la
mattina dopo si riprende da lì.

«Alla fine di questo brano» non passa dall'orologio: dice al motore di non
preparare il successivo, e la musica finisce dove sarebbe finita comunque invece
di essere tagliata a metà.

Il timer **non viaggia nel profilo esportabile**, al contrario delle altre
preferenze: è una decisione di stasera, e ritrovarlo acceso domani su un altro
computer sarebbe una musica che si spegne da sola senza che nessuno ricordi di
averlo chiesto.

### Aggiunto — la coda che non finisce, scelta dalla tua libreria

Finito l'ultimo brano, Aether ne sceglie un altro invece di fermarsi. La
cascata è: il resto del disco, poi un altro album dello stesso artista, poi
qualcosa dello stesso genere che non si ascolta da un mese, poi un preferito
mai sentito, poi uno qualunque.

**La scelta non chiede niente a nessuno**: esce dalla libreria locale
riusando il motore delle playlist intelligenti, quindi eredita anche la sua
difesa dall'iniezione — colonne e operatori escono da `match` su enum chiusi, i
valori viaggiano come parametri.

L'aggancio non è la fine del brano ma il momento in cui il motore prepara il
successivo e la coda non ne ha uno. Accodare **lì** vuol dire che l'autoplay
eredita gratis il gapless — e adesso la dissolvenza — e non produce mai un
istante di silenzio.

Spento di serie: un aggiornamento che lo accendesse da sé farebbe partire musica
che nessuno ha chiesto, magari a notte fonda, in una casa in cui l'ultimo album
era finito apposta. Con «ripeti tutto» o «ripeti questo» non entra mai in gioco,
perché la coda un dopo ce l'ha già — ed è detto sotto l'interruttore invece di
essere lasciato scoprire.

### Aggiunto — la dissolvenza incrociata, fino a dodici secondi

Un brano sfuma dentro il successivo invece di finire e ricominciare. Da zero a
dodici secondi, e zero è il gapless esatto al campione di prima — con una prova
che dice proprio questo, cioè che a durata zero il blocco che esce resta
identico campione per campione.

La curva è a **energia costante** (coseno/seno), non due rette. Su materiale
scorrelato — che è il caso di due brani diversi — le potenze si sommano, quindi
`cos² + sin² = 1` tiene il volume percepito fermo; due rampe lineari darebbero
metà dell'energia a metà della dissolvenza, cioè un buco udibile proprio nel
punto in cui la dissolvenza esiste per non farne.

Il lavoro è stato possibile perché il motore teneva **già due decodificatori
aperti insieme** — è quel che rende il gapless gapless. La miscelazione sta nel
filo del decodificatore e non nella callback audio: l'anello fra i due fili è un
flusso piatto di `f32` senza confini di traccia, e farla di là avrebbe voluto
dire un secondo anello e un secondo decodificatore dentro il percorso realtime,
dove non si può né allocare né prendere un lucchetto.

Due conseguenze, entrambe volute:

- Il **ReplayGain si è spostato**. Era un unico scalare globale che la callback
  applicava a tutto quel che usciva dall'anello; due brani sovrapposti hanno
  bisogno di due guadagni diversi, quindi adesso la correzione si applica ai
  campioni di ciascun brano al momento in cui vengono decodificati. Il prezzo:
  cambiare il livello di normalizzazione ha effetto dopo la riserva dell'anello,
  qualche secondo, invece che all'istante. Alla callback resta il solo volume,
  con la rampa che aveva già.
- Il brano entrante viene annunciato **a metà** sovrapposizione, non
  all'inizio. Prima è un sottofondo sotto quello vecchio, e annunciarlo allora
  vorrebbe dire una finestra che cambia titolo mentre si sente ancora l'altro —
  e uno scrobble attribuito al brano sbagliato.

La prima versione della miscelazione aveva due difetti, tutti e due nel filo del
decodificatore e tutti e due corretti qui.

Il primo si sentiva come un raspare. I due decodificatori consegnano blocchi di
lunghezza diversa — un pacchetto MP3 sono 1152 fotogrammi, uno FLAC anche 4096,
e un brano da ricampionare quel che decide il ricampionatore — e la
sovrapposizione ne mescolava uno contro l'altro, buttando via quel che avanzava
del brano entrante. Anche tre quarti dei suoi campioni nel cestino a ogni
blocco: non una dissolvenza, il brano nuovo mandato avanti a scatti. Adesso i
campioni decodificati stanno in una coda e la miscelazione ne prende esattamente
quanti gliene servono, tenendo il resto per il blocco dopo.

Il secondo saltava un brano. A metà sovrapposizione il motore annuncia il brano
che entra — vedi qui sopra — e chi sta sopra risponde a quell'annuncio
preparando il brano **ancora dopo**: finché «preparato» ed «entrante» sono stati
la stessa casella, quella preparazione scippava il brano a metà curva. La
seconda metà della dissolvenza faceva entrare il brano sbagliato e alla fine del
passaggio il motore attaccava quello, così chi ascoltava si ritrovava un brano
più avanti nella coda. Adesso il brano che entra ha una casella sua per tutta la
sovrapposizione, e «il prossimo» torna a essere quel che dice di essere. Ne
segue anche che una dissolvenza sopravvive alla manopola: la durata si congela
quando comincia, perché cambiare il denominatore a metà curva sposterebbe il
guadagno di colpo.

Un terzo caso non era un guasto ma un limite, e adesso è coperto. La
sovrapposizione comincia quando alla fine del brano manca quanto dura la
dissolvenza, e «quanto manca» si sa dalla durata **dichiarata** dal database —
che per un MP3 a bitrate variabile senza intestazione Xing è una stima, e la
stima può essere lunga. Quando lo è, il decodificatore del brano uscente finisce
a metà curva e quello entrante resta solo a mezza ampiezza: saltava di colpo a
piena ampiezza, cioè un clic sul brano che poi si ascolta per intero. Adesso la
sua salita riprende dal punto esatto in cui la curva si è interrotta — il
guadagno lo dà la stessa funzione a chi mescola e a chi riprende, perché due
formule scritte in due posti sarebbero due gradini — e si completa in quaranta
millisecondi, gli stessi della rampa del volume. Non il resto della curva:
quella era tarata su una durata che si è appena scoperta falsa, e continuarla
vorrebbe dire un brano che parte a mezza voce e ci mette dei secondi a venire su
da solo, che è più udibile del clic che si voleva togliere.

Tre prove nuove li tengono chiusi, e sono le prime che fanno girare il filo
della decodifica per intero: WAV costruiti in memoria, decodificatori veri,
nessun dispositivo audio. La prima controlla che il brano entrante si senta **da
solo** — se al suo posto entrasse quello dopo non ci sarebbe un solo campione al
suo livello; la seconda che due brani da un secondo sovrapposti per quattro
decimi durino un secondo e sei, non uno e due; la terza fa dichiarare a un file
mille millisecondi e contenerne ottocento, e guarda che il brano entrante
riprenda da dove era invece di saltare.

Spenta di serie, e detto nella scheda: finché è accesa il passaggio esatto al
campione non c'è più, comprese le tracce di un disco scritte per attaccarsi.

### Cambiato — lo Studio mostra l'applicazione, non una sua imitazione

L'anteprima dello Skin Studio aveva un elenco solo, di nove scene, e mescolava
due cose che nell'applicazione sono indipendenti: **quale pagina** si sta
guardando e **cosa c'è sopra**. La scena «modale» *era* la coda aperta più due
brani selezionati; la scena «avvisi» era menù, notifica e fumetto messi in fila
dentro il contenuto, cioè in un posto in cui nell'app non compaiono mai. Chi
ridipingeva la barra della selezione poteva vederla solo sopra la libreria; chi
ridipingeva la notifica non poteva vederla mai sopra Impostazioni, che è l'unico
posto in cui la notifica della scansione compare davvero.

Adesso gli assi sono due, come nell'app. Una **pagina** si sceglie fra nove — la
griglia della libreria, un album, Impostazioni, Importazioni, l'account, «In
riproduzione» a schermo intero, i tre pannelli grandi, il vuoto, il caricamento —
e sopra si accendono a piacere sei **sovrapposizioni**: terza colonna, coda,
barra della selezione, menù contestuale, notifica, finestrella. Ogni
combinazione che l'applicazione sa produrre si può guardare.

**E nessuna che non sappia produrre.** Un interruttore che qui non avrebbe
effetto — la coda a schermo intero, la terza colonna senza un brano che suoni —
si spegne e dice perché nel suggerimento, invece di accendersi e non far
succedere niente. Le ragioni sono scritte contro i predicati `visibile` di
`Impaginazione.tsx`, cioè contro il codice che decide davvero, non contro
un'idea di come dovrebbe comportarsi.

Le sovrapposizioni si disegnano **accanto** allo scafale e non dentro il buco del
contenuto, che è dove stavano: sono `position: fixed` e si riferiscono alla
finestra, e nel riquadro dell'anteprima si riferiscono al riquadro perché
`.anteprima` porta già `contain: layout paint`. I loro veli, che nell'app
prendono il clic per chiudere, qui non prendono il puntatore: restando bersagli
la sonda avrebbe letto `.velo` dovunque, e la superficie sotto — quella che si
sta ridipingendo — sarebbe diventata irraggiungibile.

**Tre pagine nuove, e sono le tre che mancavano.** «In riproduzione» a schermo
intero non c'era affatto: `np-screen`, `np-scrim` e la copertina piena vivono
solo là, e la tabella «portami dove si vede» mandava a cercarle nella terza
colonna, dove non sono mai state. Importazioni porta la seconda forma di
`list-row` — fitta, senza copertina, con una barra dentro — e una skin accordata
sull'elenco della libreria scopriva poi che lì la riga è alta la metà.
L'account porta `stat-number` in fila, l'interruttore con la sua pista, un campo
di testo e uno stato vuoto dentro una scheda invece che al posto di una pagina.

### Cambiato — le scene copiano il markup vero invece di somigliargli

La regola dei frammenti dell'anteprima era «si scrivono le classi del registro,
mai una geometria che l'app non ha», e non bastava: le classi del registro erano
giuste e tutto il resto era inventato. La copertina di un album era
`scheda section-card` dove l'applicazione scrive `scheda list-row` — chi
ridipingeva `list-row` non vedeva cambiare la libreria, chi ridipingeva
`section-card` la vedeva cambiare qui e non nella finestra. Il titolo di una
scheda era `.titolo-scheda` dove l'app dice `.titolo`. In Impostazioni c'erano
un cursore di dissolvenza incrociata e un campo per la cartella della libreria
che in Impostazioni non esistono.

Adesso la regola è più stretta: **si copia il markup vero**, classe per classe,
dal componente che disegna quella schermata. Dove il componente si può montare —
`Interruttore`, `Segmentato`, `Copertina`, `Trasporto`, `Scrubber`, `Giudizio`,
`Stelle` — non si copia affatto, si usa. Venti regole di `stile.css` che
disegnavano l'imitazione se ne sono andate: quel che resta lo disegnano le regole
dell'applicazione, che sono le stesse.

L'intestazione di pagina riceve `query` e `ordinamento` dove l'app li passa, ed
è l'unico posto in cui vivono `field-input` e uno dei `btn-ghost`: senza, il
riquadro mostrava una testata senza ricerca, cioè senza il campo di testo che una
skin deve poter ridipingere.

### Corretto — «non si vede» aveva due risposte e i casi sono tre

Scegliendo una parte che non compare nella scena aperta, lo Studio rispondeva o
«sta di là, ti ci porto» o «sta nella cornice, comparirà quando l'albero la
monta». Chi sceglieva `tour-tooltip` — che il registro dichiara e **nessuna**
schermata disegna — si sentiva dare la seconda, cioè un'attesa che non finisce
mai.

Le risposte adesso sono tre, e la terza dice il vero: cinque parti del registro
non le disegna ancora nessuno, e ognuna porta il suo perché. `strumenti/classi.js`
controlla che l'elenco dello Studio e il suo — `ATTESE`, quello che gira in CI —
restino la stessa cosa: sono la stessa informazione detta a due destinatari, e
divergendo lo Studio tornerebbe a dare la risposta sbagliata.

### Corretto — due parti del registro che quasi nessuno emetteva

`glass-modal` è dichiarata come «la finestra modale» e la portava una sola
superficie: il pannello della coda. Le undici finestrelle vere — l'account, le
regole di una playlist intelligente, il ripristino, il riordino, la conferma —
erano `.finestrella` e basta, cioè non ridipingibili. Adesso la portano tutte, e
la parte significa quel che il registro dice che significa.

`field-input` è «il campo di testo» e stava su uno solo: quello della ricerca in
testata. Gli altri quindici — il token dello scrobbling, il nome di una playlist
nuova, il nome di un preset dell'equalizzatore — erano `.campo` e basta. Sono la
stessa cosa e adesso lo dicono.

Nessuna delle due è una scelta estetica: erano due promesse del registro che
l'applicazione non manteneva, e si scoprivano scrivendo una skin — cioè dopo.

### Aggiunto — `F11`, il quinto gesto della finestra

Lo schermo intero c'era già, ma era quello dell'applicazione: «In riproduzione»
con `F` riempie la **finestra** col brano, e la finestra resta com'era, dentro il
suo desktop. Mancava l'altro — quello che il resto del desktop lo toglie di mezzo
— cioè il tasto che ha ogni programma che si apra in una finestra.

Sta nella tabella delle scorciatoie (`tastiera.ts`) come tutti gli altri: si
riassegna, si legge nelle impostazioni accanto agli altri sette, e se qualcuno gli
mette addosso un tasto già preso il conflitto si vede invece di succedere e
basta. Di serie è `F11`, che non è una lettera e quindi non toglie niente a
nessuno.

**I tasti funzione passano anche mentre si scrive.** La regola era che un tasto
nudo non arriva se il fuoco è in un campo — chi cerca «space oddity» non vuole
mettere in pausa a metà parola — e senza un'eccezione lo schermo intero sarebbe
stato l'unico comando della finestra a spegnersi mentre si cerca un disco.
`F1`…`F12` non finiscono dentro nessuna parola, e adesso hanno l'esenzione che
avevano già `Ctrl` e `Alt`.

Il comando è nostro — `finestra_schermo_intero` — e non il permesso
`core:window`: aprire quell'elenco per un tasto darebbe alla pagina anche
`set_position`, `set_size` e `set_always_on_top`, ed è la stessa ragione per cui
passano da comandi nostri gli altri quattro gesti. Quel che si rifiuta è il mazzo,
non lo schermo intero.

Il bottone di mezzo della fascia, a schermo intero, **ne esce**: la sua icona lì
dice «rimpicciolisci», e ingrandire una finestra che occupa già lo schermo
sarebbe un clic che non fa niente mentre l'icona promette il contrario. È anche
la via d'uscita per chi si ritrova a schermo intero e non ricorda quale tasto ha
premuto.

### Cambiato — la fascia in cima non è più di Windows

La finestra nasce senza decorazioni. La striscia grigia che il sistema operativo
disegnava sopra l'applicazione — l'icona, il nome ripetuto a chi aveva appena
aperto Aether, i tre quadrati — era di un altro programma: portava i suoi colori
dentro una finestra che ha una skin, restava chiara mentre tutto il resto era
scuro, e si prendeva trentadue pixel su tutta la larghezza.

La strada corta era ridisegnarne una identica coi colori giusti. Sarebbe costata
di nuovo trentadue pixel per tre bottoni, e avrebbe messo il marchio due volte:
la navigazione ce l'ha già in cima.

**La riga in alto l'applicazione ce l'aveva di suo.** Il marchio ha il centro a
32 pixel dal bordo, la testa della terza colonna a 30, l'occhiello
dell'intestazione a 29: è la stessa riga, e i tre bottoni si siedono lì invece di
aprirsene una. Chi arriva sotto quell'angolo — la colonna, l'intestazione della
pagina quando la colonna è chiusa, la testata dello schermo intero e quella dello
Studio — cede in **larghezza**: cedere in altezza avrebbe voluto dire abbassare
ogni schermata di sessanta pixel, cioè la fascia di prima, trasparente.

Il bersaglio di ogni bottone arriva allo spigolo, il disegno no. Chiudere una
finestra si fa sbattendo il puntatore contro il vertice dello schermo senza
guardare, ed è l'unico gesto dell'interfaccia che si può fare a occhi chiusi;
quel che si vede però è un riquadro alto trentadue con gli angoli
dell'applicazione, perché tre rettangoli pieni alti sessanta sarebbero la fascia
di Windows, ridipinta.

Trascinare non passa da un riquadro trasparente sopra tutto il resto: quello
avrebbe mangiato ogni clic dei primi sessanta pixel — il tasto che richiude la
navigazione, i due della colonna, la ricerca quando va a capo. C'è invece un
ascoltatore che guarda dove è caduto il `mousedown`: in alto e non su qualcosa
che si clicca, la finestra si trascina. Si afferra anche il titolo della pagina,
che a guardarlo è esattamente la barra del titolo.

Ridimensionare dai bordi non si perde: lo fa `tao` da sé in `WM_NCHITTEST` per
ogni finestra senza decorazioni che sia ridimensionabile. `Alt`+`Spazio` continua
ad aprire il menù di sistema, e `Alt`+`F4` a chiudere.

I tre comandi passano da comandi nostri — `finestra_trascina`, `finestra_riduci`,
`finestra_ingrandisci`, `finestra_chiudi` — e non dai permessi `core:window:*`:
`capabilities/default.json` è un elenco chiuso, e aprirlo a `core:window`
darebbe alla pagina anche `set_position`, `set_size`, `set_always_on_top` e
`set_fullscreen`, cioè quattro modi di far sparire una finestra da sotto le dita
di chi la sta guardando, in cambio di tre bottoni. Per la stessa ragione non
passano dallo scafale delle skin: una skin che potesse nascondere il tasto di
chiusura arriva da un file.

### Sicurezza — la catena di rilascio non si fida più di un'etichetta

Un passaggio con uno scanner sul ramo: di venticinque segnalazioni, dodici sono
state corrette, dieci non hanno retto alla verifica, e tre riguardano un file
generato che non nasce in questo repository.

Le dodici stanno tutte in `.github/`, e tutte e dodici passano accanto alla
stessa cosa: `TAURI_SIGNING_PRIVATE_KEY`, la chiave che firma gli aggiornamenti
e che — sta scritto nel preambolo di `release.yml` — non è recuperabile e non è
sostituibile.

**Le undici azioni della CI sono inchiodate a un hash.** `actions/checkout@v4`
non nomina una versione: nomina un'**etichetta**, e chi possiede quel repository
può spostarla su un altro commit senza che qui cambi una riga. Non è un timore di
scuola — nel marzo del 2025 le etichette di `tj-actions/changed-files` sono state
riscritte, e ventitremila repository hanno eseguito codice nuovo credendo di
eseguire quello di prima. Il conto di quel giorno, dentro `release.yml`, non
sarebbe una build sporca: sarebbe la chiave privata dell'updater in mano a
qualcun altro, cioè ogni copia installata di Aether che non si aggiorna mai più.
La versione resta leggibile nel commento accanto all'hash. Il prezzo è che le
azioni non si aggiornano più da sole, e quel commento è l'unico posto in cui si
vede che sono vecchie.

**Il tag non entra più nello script.** Era
`tag="${GITHUB_REF_NAME:-${{ inputs.tag }}}"`, e quel `${{ }}` dentro un `run:`
non è una variabile: è una sostituzione di testo che avviene **prima** che bash
veda la riga, quindi un tag che contiene `$(…)` smetteva di essere un tag.
Adesso passa dall'ambiente, dove resta un dato qualunque cosa ci sia scritto. Non
era un varco verso l'interno — per lanciare a mano un workflow servono già i
permessi di scrittura sul repository — ma era un varco che passava accanto a
quella chiave, e per una chiave che non si può sostituire è tutto quel che serve.

Correggendolo è venuto fuori che **il rilascio a mano non ha mai funzionato**: su
`workflow_dispatch` `GITHUB_REF_NAME` c'è comunque, ed è il ramo da cui si
lancia. Il valore di riserva non cadeva quindi mai sull'input, e un rilascio
lanciato da `main` confrontava la versione dei tre file con la parola «main» e si
fermava lì. Adesso il tag lo decide `github.ref_type`, e lo stesso testo va in
`tag_name:` e nel titolo della release — che leggevano anche loro il ref, e su un
rilascio a mano avrebbero pubblicato «Aether main».

Le tre segnalazioni che restano sono i `postMessage` di `site/support.js`, che
dichiarano `"*"` come origine e non controllano quella di chi scrive. Il file
comincia con «GENERATED — do not edit» e la sua sorgente non sta qui: una
correzione a mano sparirebbe alla prima rigenerazione, e l'origine giusta da
scrivere non è nemmeno fissa. Quel che passa di lì sono nomi di componenti verso
la cornice che contiene la pagina, e solo se una cornice c'è; se un giorno conta,
la risposta è un `frame-ancestors` sull'header del sito, non una patch al bundle.

Le altre dieci vale la pena scriverle, se non altro perché lo scanner le ridirà:
il token OAuth «hardcoded» è `ya29.finto` dentro un `#[cfg(test)]`; i due path
traversal compongono percorsi con nomi che arrivano da `readdirSync`, che un
separatore non lo restituisce; l'SRI mancante è su un favicon `data:`, dove non
c'è nessuna richiesta da verificare; le quattro «format string» sono
concatenazioni passate a `console.error`. Le ultime due dicevano *prototype
pollution*, e sono la voce qui sotto: quel che hanno trovato è vero, ma non è
quello.

### Corretto — un colore che si chiamava `__proto__` spariva mentre lo scrivevi

Non è l'inquinamento del prototipo globale che il nome fa temere.
`Object.prototype` non si tocca mai: l'oggetto che lo Studio riscrive nasce
sempre da `JSON.parse` o da uno `spread` di `patch.ts`, e il prototipo che cambia
è quello di una copia che un istante dopo viene buttata. È un difetto di un'altra
specie, e si vede invece di essere teorico.

`__proto__` è l'unico nome che un autore di skin può battere in un campo di testo
— il nome di un colore della tavolozza, di un motivo, una rinomina — per cui
`oggetto[nome] = valore` **non scrive una chiave**. `Object.prototype` espone un
accessore con quel nome e l'assegnazione chiama quello; `JSON.stringify` poi non
stampa niente. Il colore spariva dal documento senza che nulla lo dicesse, e
l'editor mostrava un file che non conteneva quel che si era appena scritto.

Il difetto aveva una faccia asimmetrica, ed è il modo in cui si è visto:
`rinominaChiave` passa da `Object.fromEntries`, che una proprietà propria la crea
davvero. Rinominare un colore in `__proto__` funzionava; cambiargli il valore un
istante dopo lo cancellava.

Le tre funzioni che camminano un percorso — `valoreIn`, `scriviIn`, `togliDa` —
adesso scrivono con `defineProperty` e leggono dietro un `hasOwnProperty`. La
prima è esattamente quel che fa `JSON.parse` quando incontra `"__proto__"` dentro
un oggetto, quindi le due direzioni del documento tornano a coincidere. La
seconda chiude anche il caso di lettura: un percorso che passava per `__proto__`
restituiva `Object.prototype`, e i controlli dello Studio si mettevano a disegnare
le proprietà di quello invece che del documento.

### Aggiunto — i testi, e il bottone che era spento

In `InRiproduzione` il bottone «testo» era disegnato e spento, con la ragione
scritta accanto: «il nucleo non legge ancora i testi». Adesso li legge, e il
pannello scorre.

Le fonti si consultano in quest'ordine, e la prima che risponde vince: un `.lrc`
(o `.a2.lrc`) accanto al brano, la riga già presa per lo stesso `track_key`, il
tag dentro il file, LRCLIB, e infine l'editor a battute. **Nessuna fonte
scaricata sovrascrive mai una sincronizzazione fatta a mano** — la condizione
sta nella `WHERE` dell'`UPSERT`, cioè nel database, non in un `if` che il
prossimo punto di chiamata potrebbe dimenticare.

Quel che si può dire e quel che non si può:

- **non esiste una fonte gratuita che abbia i testi di tutte le canzoni.** Chi
  lo promette o raschia pagine o ha una licenza editoriale, e nessuna delle due
  è una cosa che questo programma faccia. Quel che c'è invece è che ogni brano
  finisce in uno di quattro stati **dichiarati** — sincronizzato, piatto,
  strumentale, da sincronizzare — e mai in una schermata bianca. I quattro
  numeri si vedono in Impostazioni › Cartelle, invece di essere una speranza;
- l'ultimo stato si chiude a mano, e l'editor a battute è fatto perché costi tre
  minuti: si preme Spazio a ogni riga mentre il brano suona, ogni battuta viene
  agganciata all'attacco vero più vicino nella finestra `[-250 ms, +120 ms]` —
  asimmetrica, perché la mano è sempre in ritardo — e la **mediana** degli
  scarti sulle righe agganciate corregge quelle che un attacco non l'hanno
  trovato. È la latenza di reazione di quella persona in quel momento, misurata
  invece che indovinata. Gli attacchi escono dal flusso spettrale calcolato con
  la FFT che `aether-play` aveva già per lo spettro: nessuna dipendenza nuova,
  nessun binario da impacchettare.

L'LRC lo legge **il nucleo**, in `aether_domain::testo`: la finestra riceve
righe già in ordine e già in millisecondi, e non sa cosa sia un `[mm:ss.xx]`. Un
secondo lettore scritto in TypeScript sarebbe divergito dal primo su tutto quel
che il formato non dice — e un formato del 1998 non dice quasi niente.

L'illuminazione che attraversa la riga accesa è una variabile CSS scritta su un
`ref`, fuori da React: farne uno stato vorrebbe dire ricostruire l'albero venti
volte al secondo per muovere un gradiente di un pixel. Dove i tempi delle parole
ci sono — l'LRC esteso `.a2.lrc` — il fronte lascia la riga e passa alle parole,
ed è l'unico caso in cui quell'illuminazione dice qualcosa di vero invece di
essere una bugia gentile.

### Corretto — la riga accesa si accendeva e spariva

`--color-accent` non esisteva. Il token dell'accento in questo programma si
chiama `--accent`, e il foglio di stile lo chiamava con l'altro nome in nove
punti — due dei quali erano il gradiente che illumina la riga di testo che sta
suonando.

Una variabile CSS che non esiste non lascia scoperta la sua proprietà: rende
invalida **tutta** la dichiarazione. Quindi `background-image` tornava a `none`,
e siccome quella riga affida il proprio colore al gradiente e mette
`color: transparent`, il risultato era che la riga accesa — e su un `.a2.lrc`
ogni parola della riga accesa — diventava perfettamente invisibile. Il testo si
leggeva tutto tranne il verso che si stava ascoltando: si accendeva, e spariva.

Gli altri sette usi degradavano in silenzio, che è il motivo per cui nessuno se
n'era accorto: `color` e `border-color` invalidi ereditano o ricadono su
`currentColor`, quindi l'elenco delle battute e il tasto di scorciatoia
perdevano solo il loro accento. Due `border-radius` invece cadevano a zero, e i
due controlli che li portavano erano squadrati in un'interfaccia che non ha un
solo angolo vivo.

Corretti tutti e quindici gli usi delle quattro variabili morte del foglio
(`--color-accent`, `--color-border`, `--color-warning`, `--radius-control`). Nel
gradiente della riga accesa il nome giusto porta con sé un ripiego annidato,
`var(--accent, var(--color-text-1))`: è l'unico punto in cui una variabile
mancante non vuol dire «senza accento» ma «senza testo», e un difetto che
cancella quel che si sta leggendo non deve poter tornare per una rinomina.

### Corretto — su un verso che va a capo il fronte si sdoppiava

Rimessa in piedi la riga accesa, si vedeva il difetto che ci stava sotto. Su

    Guardo nel retrovisore, dietro me si sta
    scuencendo l'autostrada

a un quarto del percorso erano accesi «Guardo nel» **e** «scuencendo»: due
frammenti staccati, e la riga di sotto che si illuminava in parallelo a quella
di sopra invece che dopo di lei. In una colonna stretta va a capo un verso su
due, quindi il fronte diceva la cosa sbagliata quasi sempre.

La causa: il gradiente stava sul **blocco**, e un blocco che va a capo resta
largo uno solo. Lo stesso taglio orizzontale cadeva su tutte le sue righe
visive nello stesso istante.

Adesso l'illuminazione vive su un elemento **inline** dentro la riga. Su un
inline che va a capo vale `box-decoration-break: slice`: il fondo si dipinge
come se i frammenti fossero uno in fila all'altro, e solo dopo si affetta riga
per riga — cioè esattamente l'ordine che serve. Costa un elemento e nessuna
scrittura in più per fotogramma; l'alternativa, una parola per elemento come
nell'LRC esteso, sarebbe stata `N` scritture ogni cinquantesimo di secondo per
lo stesso risultato.

L'LRC esteso non aveva questo difetto e non l'ha mai avuto, per la stessa
ragione per cui adesso non ce l'ha nemmeno l'altro: lì ogni parola è già un
elemento inline con il suo gradiente.

### Corretto — il pannello del testo, che non inseguiva e non si leggeva

Lo stesso screenshot diceva altre tre cose, ognuna abbastanza piccola da non
farsi notare da sola: il pannello era fermo in cima dopo mezzo minuto di brano,
e tutto il testo stava al 46% di bianco sopra le barre viola dello spettro.

- **Lo scorrimento automatico si spegneva da solo.** La pausa che il pannello si
  prende quando lo scorri a mano era appesa all'evento `scroll` — che emette
  anche lo scorrimento chiesto dal pannello stesso. Cioè: inseguiva la riga
  una volta, quell'inseguimento lo metteva in pausa, e nella pausa passavano
  tutte le righe successive. Adesso la pausa è appesa al **gesto** — la
  rotella, il dito, i tasti che scorrono — che è l'unica cosa che una
  persona fa e il browser no.
- **`padding-block: 40%` non era «metà pannello».** Una percentuale nel
  padding si risolve sulla larghezza del contenitore anche in verticale: su una
  colonna da trecentosessanta pixel erano centotrenta invece di seicento, e la
  riga accesa non poteva salire al suo posto finché la canzone non era a
  metà. Il respiro adesso lo fanno due distanziatori, dove la stessa
  percentuale si misura sull'altezza — e compaiono solo per un testo che
  scorre, invece di spingere in giù anche i testi senza tempi.
- **Il pannello era trasparente sopra una tela che si ridisegna.** Il velo al 45%
  è la ricetta giusta sopra una copertina sfocata e quella sbagliata sopra lo
  spettro, che lascia le barre piene proprio nella metà bassa — dov'è
  quasi tutto il testo. Adesso è all'82%, e le righe non ancora cantate
  passano da `--color-text-3` a `--color-text-2`: sopra il caso peggiore stanno
  a 6.6:1 invece che a 4.4:1.

E tre cose che mancavano:

- l'**ultima riga** di ogni brano restava bianca e ferma, perché l'avanzamento
  si misura fino alla riga dopo e una riga dopo non c'era. Adesso il fondo lo
  dà la durata del file, che il dominio non conosce e la finestra sì;
- nell'**introduzione e negli stacchi** fra due strofe non era acceso niente, e
  un pannello immobile sembra rotto proprio quando sta funzionando. Al loro posto
  ci sono tre puntini che si riempiono, guidati dallo stesso `--avanzamento` di
  tutto il resto: nessun meccanismo nuovo, la stessa variabile letta da tre
  elementi. Sotto i tre secondi non compaiono, perché sarebbero un lampeggio;
- **si clicca una riga per saltarci**. Gli scarti si disfano invece di essere
  applicati — i tempi delle righe non si toccano mai, è la regola di tutto
  il modulo — e una sola riga per volta sta nell'ordine di tabulazione, perché
  duecento fermate dentro un pannello che si legge non sono accessibilità.

Intorno: la colonna del testo è diventata fluida (`clamp(340px, 26vw, 460px)`)
perché a trecentosessanta pixel fissi quasi ogni verso andava a capo mentre al
centro dello schermo restavano milleottocento pixel di scena quasi vuota; le
righe sfumano ai bordi invece di essere tranciate dall'angolo del pannello; e
«torna al brano» dice che l'inseguimento è in pausa, che è quel che
permette alla pausa di durare sei secondi invece di tre senza lasciare perso
nessuno.

### Aggiunto — restituire a LRCLIB, e perché è un pulsante e non una casella

Dopo aver sincronizzato un testo a mano si può mandarlo a LRCLIB. Vale la pena
essere espliciti su come, perché la forma **è** la sostanza qui:

- è un gesto, un brano alla volta, e si preme dopo aver visto cosa si sta
  mandando. Mai in blocco, mai automatico, mai attaccato al salvataggio — perché
  si salva sempre, e tutto quel che sta attaccato al salvataggio è automatico
  per definizione;
- **non si pubblica mai il testo che stava dentro il tag di un file**, né quel
  che dal catalogo è appena arrivato. La regola sta in `testi::da_restituire`,
  non nella finestra: vale anche se un giorno il pulsante fosse in un altro
  posto;
- il testo senza tempi che accompagna l'invio è **ricavato** dall'LRC che si sta
  mandando, mai preso altrove: così è per costruzione lo stesso testo, e non c'è
  modo di spedire due versioni che dicono cose diverse;
- con l'interruttore dei testi spento il pulsante non compare.

`PRIVACY.md` § 2-bis elenca adesso riga per riga cosa esce quando lo si preme, e
`TERMS.md` § 3-bis dice la cosa che di solito si tace: quel che metti in comune
sono i tempi, le parole restano dell'avente diritto, e mandarle a un catalogo
pubblico è una decisione tua e non una che il programma prende al posto tuo.

La pubblicazione ci mette qualche secondo, e non per la rete: LRCLIB chiede una
prova di lavoro SHA-256 a ogni invio. È una difesa onesta — non tocca chi manda
un testo alla volta e rende impraticabile mandarne centomila — e la finestra lo
dice **prima** che si prema, non dopo.

Musixmatch, Genius, AZLyrics, LyricFind e i cataloghi cinesi restano fuori, e
adesso la CI lo verifica a ogni modifica: passo «Nessuna fonte di testi vietata»
nel lavoro `niente-di-vietato`, accanto a quello che tiene fuori gli strumenti
di scaricamento. La decisione vive nella CI, non nella memoria di chi rivede.

### Aggiunto — l'updater, e la richiesta che non c'era

Chi installava la 0.1.0 restava sulla 0.1.0. Adesso, due minuti dopo l'avvio e
poi ogni mezz'ora, un filo di sottofondo chiede a GitHub se è uscita una
versione più nuova, e quando c'è lo dice con una fascia sopra il contenuto —
versione, note di rilascio, «Aggiorna» e «Non ora».

**È una richiesta di rete che parte da sola, e questo changelog è il posto in
cui dirlo.** Fino a ieri `PRIVACY.md` § 6 diceva «nessuna richiesta all'avvio:
non c'è un controllo aggiornamenti», e quella frase è diventata falsa con questo
commit — quindi è stata riscritta nello stesso commit, e il controllo ha adesso
una sezione numerata sua. Dentro la richiesta non va niente: nessun
identificativo, nessun conteggio, nemmeno la versione installata. È una GET a un
file pubblico di trecento byte, e il confronto fra i due numeri avviene qui. Si
spegne in *Impostazioni → Aggiornamenti*, e spento non parte nulla.

Non installa da sé. Installare vuol dire chiudere l'applicazione, e farlo mentre
qualcuno sta ascoltando — o mentre sta riordinando trentamila file sul disco —
è il genere di cosa che si perdona una volta sola. Il filo trova e aspetta; lo
scaricamento parte da un tasto, e prima che l'installer venga eseguito la sua
firma minisign viene verificata contro una chiave pubblica compilata dentro
l'eseguibile. Un «non ora» vale per quella versione sola e si scrive nel
database, non in memoria: un avviso che ricompare a ogni riavvio è un avviso che
si impara a chiudere senza leggerlo.

La cosa che poteva andare storta in silenzio è un'altra, ed è quella per cui
esiste `strumenti/manifesto.js`: se la chiave privata con cui la CI firma non è
la metà di quella che gli eseguibili già installati conoscono, il `latest.json`
è perfetto, l'installer si scarica, la verifica fallisce e **nessuno si
aggiorna** — e non se ne accorge nessuno, perché il guasto succede sul computer
di altri. Lo script confronta gli otto byte di identificativo che minisign mette
sia nella chiave sia nella firma, e fa fallire la release se non coincidono.

La release nasce bozza, e `/releases/latest` salta le bozze: pubblicarla **è** il
gesto che spedisce l'aggiornamento a tutti. Automatizzarlo si può in due righe;
non si fa, perché è l'unico punto della catena in cui una persona guarda la cosa
prima che parta.

Non si è usata una cartella condivisa su MEGA, che era la prima idea. Un link
`mega.nz/folder/<id>#<chiave>` non è un file scaricabile: la chiave sta nel
frammento `#`, che per definizione non arriva mai al server; l'URL vero si
ottiene con una POST all'API ed è temporaneo e legato a un nodo che ruota; i
byte che tornano sono cifrati AES-128-CTR con un meta-MAC da verificare; e i
download anonimi sono contati per indirizzo IP, quindi dietro un NAT condiviso
gli utenti si brucerebbero la quota a vicenda. `tauri-plugin-updater` non ha
un punto in cui sostituire il proprio downloader: MEGA avrebbe voluto dire
rinunciare al plugin e riscrivere a mano anche la verifica della firma, cioè
l'unica parte che non si deve sbagliare.

### Aggiunto — italiano e inglese, e il tedesco costa un file

L'interfaccia era scritta in italiano e basta: le stringhe stavano dentro il
JSX, `«1 brano» / «2 brani»` era cucito in `formato.ts`, `<html lang="it">` era
fisso in `index.html`, e `toLocaleString("it")` compariva a mano in quarantacinque
punti su diciassette file — più tre volte senza argomento, cioè tre numeri che
seguivano il sistema operativo mentre tutti gli altri seguivano l'italiano.

Adesso parla due lingue, e — questa è la parte che conta — aggiungerne una terza
costa **un file**: si lascia cadere `de.json` in `apps/desktop/src/lingue/` e il
programma lo trova. Nessun registro da aggiornare, nessuno `switch` da
allungare, nessun import da scrivere. Il perno è `import.meta.glob` di Vite:
l'elenco delle lingue disponibili **è** il contenuto della cartella, e il nome
nativo con cui la lingua compare nel selettore viaggia dentro il file stesso,
sotto la chiave riservata `_nome`. Provato: copiato `en.json` in `de.json`,
cambiato `_nome` in «Deutsch», tradotte tre chiavi — il tedesco compare senza
aver toccato un solo `.ts`, e le chiavi non tradotte si mostrano in inglese.

Niente `i18next`: il progetto ha quattro dipendenze runtime in tutto, e un
modulo di duecentosessanta righe fa questo lavoro meglio di quaranta kilobyte di
libreria generica. Nessun `<ProviderLingua>` nemmeno: il repo non usa i Context
— `grep` di `createContext` su tutto `src` dà zero risultati — e la lingua segue
la stessa forma della riproduzione, un modulo con `useSyncExternalStore`. Serviva
anche in pratica: `Impostazioni` riceve già sessantuno prop, e far scendere `t`
per prop drilling attraverso settanta file non era sostenibile.

Le scelte, in breve:

- **La lingua all'avvio** si rileva dal sistema, con `de-DE` ridotto a `de`; se
  per quella lingua non c'è un file, **inglese** — anche se la lingua di sviluppo
  è l'italiano. Sono due ruoli distinti e il codice li tiene separati: `"en"` per
  chi arriva con un sistema in svedese, e ancora `"en"` per le chiavi che mancano
  dentro un `de.json` incompleto.
- **La scelta vive nelle preferenze del nucleo** (`ui.language`), accanto al
  tema, quindi finisce da sé nel backup su Drive e nel profilo esportabile —
  `profilo.rs` è un elenco di inclusione, e dimenticarlo là avrebbe dato un
  profilo che ripristina tutto tranne una cosa. Nessuna migrazione: `settings` è
  una tabella chiave/valore.
- **Niente lampo di lingua sbagliata**: la finestra nasce invisibile
  (`"visible": false`) e la mostra il frontend col comando `pronto`, che adesso
  aspetta anche `Avvio.lingua`. La lingua di sistema si applica prima del primo
  disegno, quella salvata prima che la finestra si veda.

Il campo `i18nKey` era già lì e non lo leggeva nessuno: `ErroreIpc` lo porta dal
nucleo da sempre, generato in `errors/catalog.rs` come `concat!("errors.", <codice>)`,
e il frontend usava invece una tabella scritta a mano di venticinque messaggi
italiani chiavati per codice — venticinque voci contro centoquattro codici. Adesso
la chiave la decide il nucleo e il testo lo decide il catalogo delle lingue, dove
ci sono tutti e centoquattro: gli **otto orfani** che l'utente vedeva col messaggio
grezzo del backend sono chiusi.

Tradotti anche i duecentoventinove `aria-label`, `title` e `placeholder`:
guardare solo il testo dei nodi avrebbe lasciato l'accessibilità monolingue. E
le diciassette tabelle `const` con dentro delle etichette sono diventate
funzioni, perché una costante di modulo congela la lingua al primo import.

Due cose restano di proposito nella lingua in cui nascono, e non è una
dimenticanza: i segnaposti «Album sconosciuto» e «Artista sconosciuto» del
nucleo — che `library.rs` scrive **nelle righe del database** e `organize.rs` usa
per **nomi di cartelle su disco** — e la riga di attribuzione che
`prelievo.rs` scrive **nei tag di un file scaricato**. Tradurle dove nascono
darebbe chiavi d'album diverse fra due avvii con lingue diverse, cartelle di due
lingue affiancate, e un'attribuzione che cambia lingua a seconda del mese. Sono
valori, non etichette: il frontend riconosce i due segnaposti e li sostituisce al
momento di disegnare, e i percorsi già scritti non si toccano.

Fuori portata, e vale dirlo: `tauri.conf.json` (`title`, `shortDescription`,
`longDescription`) resta italiano. Titolo della finestra e testi dell'installer
non si localizzano senza `bundle.windows.nsis.languages`, che non c'è.

**Un guardiano in CI.** `strumenti/lingue.js` confronta le chiavi di ogni
`lingue/*.json` con `it.json` e fallisce elencando le mancanti — e anche quelle
che avanzano, che sono chiavi rinominate altrove e non qui, cioè testo che
nessuno disegnerà mai. Senza, il secondo `de.json` si scopre incompleto quando lo
usa qualcuno: il motore ripiega sull'inglese e disegna lo stesso, ed è proprio
questo a rendere invisibile una traduzione a metà. `node strumenti/lingue.js
--scrivi` mette in fila le chiavi mancanti col testo italiano, così restano da
tradurre invece che da cercare.

### Tolto — YouTube, lo scraping di Spotify, e il binario impacchettato

La cosa più grande di questa tornata è una sottrazione, e vale la pena scrivere
perché.

**`aether-yt` — cancellato per intero.** Cinquemilacinquecento righe che
invocavano `yt-dlp.exe` per prendere i byte dei video, più l'API interna di
YouTube Music. Le *YouTube API Developer Policies* § III.E.1.a lo vietano in
termini che non lasciano margine: «You must not… download, import, backup,
cache, or store copies of YouTube audiovisual content». Non esisteva una
configurazione che lo rendesse lecito.

Non esisteva nemmeno una versione corretta del modulo. Le stesse policy vietano
di separare l'audio dal video (§ III.I.7) e di riprodurlo da un player non
visibile (§ III.I.9), e limitano a trenta giorni la conservazione dei metadati
(§ III.E.4). Un lettore musicale che tiene una libreria è esattamente la cosa
che quelle tre regole escludono: YouTube non si correggeva, si toglieva.

**`aether-spotify` — cancellato per intero.** Parlava con endpoint privati: il
GraphQL interno con gli hash delle query persistite, i gettoni anonimi
ricostruiti con HMAC-SHA1, la stretta di mano anonima, e il **client id del web
player di Spotify** — non nostro — con uno User-Agent di Chrome falsificato. Era
accesso non autorizzato al servizio, ed era la violazione che nessuno aveva
dichiarato.

Con lui se n'è andata anche la metà **lecita**: il consenso OAuth alla Web API.
Quella funzionava, ma lo *Spotify Developer Policy* (III.5, III.9) limita cosa
si può fare dei dati che restituisce, e una libreria che li tiene per anni non
sta dentro quei limiti. Resta l'archivio GDPR, che è dell'utente per diritto
(art. 20) e porta la cronologia **completa** invece degli ultimi cinquanta
ascolti.

**`resources/bin/yt-dlp.exe`** — diciotto megabyte, e la riga
`bundle.resources` che li impacchettava. L'installer adesso non contiene
**nessun eseguibile di terze parti**.

Conseguenze visibili: sparisce l'importazione da un link di Spotify o di
YouTube, e sparisce il selettore fra i due servizi. Restano l'archivio GDPR e i
file di playlist, e arrivano i cataloghi liberi.

### Aggiunto — i cataloghi liberi

**`aether-catalogo`**, un crate nuovo che parla solo con `aether-net`: nessun
processo figlio, nessun binario da impacchettare, richieste HTTP che si leggono
nel codice. Nella prima tornata c'è l'**Internet Archive** — Live Music Archive,
netlabel, pubblico dominio — con ricerca, lettura dell'item e prelievo dei file.

Il concetto che porta il peso è `Disponibilita`, e non esisteva prima: la
risposta strutturata alla domanda «questo brano lo posso prendere?», al posto
dell'assunto implicito di prima («su YouTube c'è tutto»). Accanto sta
`Licenza`, e in tutti e due i casi **il valore restrittivo è quello di
serie**: una licenza che non si conosce vale «non si copia», e `preleva` rifiuta
prima di fare qualunque richiesta.

**«Da comprare»**, una sezione nuova nella pagina delle importazioni. Lo stato
`Introvabile` della coda cambia significato — non più «su YouTube non c'è» ma
«nessuna fonte lecita ce l'ha» — e diventa una lista d'acquisto con i link a
Bandcamp, Qobuz e Discogs. È la sostituzione onesta di uno scaricamento che non
si può fare: dire **dove** prendere quel brano, in posti dove chi l'ha fatto
viene pagato.

**La natura della registrazione**, detta a schermo. I cataloghi liberi non hanno
le versioni in studio del catalogo commerciale: hanno concerti, riedizioni,
riletture — il Live Music Archive è fatto **solo** di concerti. Aether le
accetta, e mette accanto al brano una pastiglia che dice quale ha preso. Chi
ricostruisce un disco preciso può spegnere quel comportamento
(`catalogo.alternative` nelle impostazioni).

**La licenza di ogni brano**, scritta in `desiderati` (migrazione 10) e mostrata
nella coda. Un file che entra in libreria senza che nessuno dica a quali
condizioni ci è entrato è un file che fra un anno nessuno saprà se può
condividere.

### Aggiunto — quel che serve a distribuire

- **`LICENSE`** in radice: il testo MIT che quattordici manifest dichiaravano e
  che non esisteva da nessuna parte.
- **`THIRD-PARTY-NOTICES.md`**, generato da `strumenti/licenze.js`: copre i
  diciotto crate **MPL-2.0** (tutta la famiglia symphonia, più cssparser e
  selectors), `cpal` e `tao` che sono **Apache-2.0 senza alternativa**, `ring`,
  e i tredici crate `Unicode-3.0`.
- **`apps/desktop/src/font/OFL.txt`**: Geist e Bricolage Grotesque erano
  impacchettati nudi, e la SIL OFL obbliga a distribuire la licenza con loro.
- **`publish = false`** in tutti e tredici i manifest: nessuno di questi crate va
  su crates.io, e fino a ieri un `cargo publish` distratto ce li avrebbe messi.
- **`README.md`**, **`PRIVACY.md`**, **`TERMS.md`**: non esistevano. I termini
  riportano il vincolo non commerciale del Live Music Archive, che è una
  condizione d'uso e non una nota a piè di pagina.
- **Due workflow di CI** (`.github/workflows/`). Il primo prova tutto; il
  secondo, da un tag, produce l'installer. Il primo ha un lavoro a parte che
  fallisce se qualcuno rimette un binario in `resources/bin` o reintroduce uno
  strumento di scaricamento non lecito: la regola vale se qualcosa la fa
  rispettare.
- **`strumenti/versione.js`** e **`strumenti/licenze.js`**, con gli script npm
  `check:version`, `version:set`, `licenze` e `verify` — che il CHANGELOG dava
  per esistenti da mesi senza che ci fossero.
- **Il modulo email del sito è stato tolto.** Chiedeva un indirizzo, prometteva
  «una mail quando si apre la tua ondata» e **non lo mandava da nessuna parte**:
  nessun backend, nessuna informativa, e il tasto diventava verde lo stesso.
  Raccogliere un indirizzo e buttarlo via è peggio che non chiederlo. Con lui
  sono state corrette le tre affermazioni false del sito: «Nothing here is
  fetched from a network», «macOS · Windows · Linux» quando l'unico bersaglio è
  NSIS, e «no stream».

### Cambiato — l'identificativo dell'applicazione

`dev.aether.desktop` → **`io.github.federicobaratti.aether`**.

Il primo è un reverse-DNS su `aether.dev`, un dominio che non è mio. Su una cosa
che si pubblica non è una sottigliezza, e cambiarlo **prima** del primo
rilascio costa a una persona sola; cambiarlo dopo costa a tutte.

**Se avevi già una libreria in prova**, si sposta anche la cartella dati e il
nome del servizio nel portachiavi. In pratica:

- la libreria sta in `%APPDATA%\dev.aether.desktop` e Aether adesso la cerca in
  `%APPDATA%\io.github.federicobaratti.aether`: rinominare la cartella la
  ritrova, oppure si lancia con `AETHER_DATI=...` puntato alla vecchia;
- i collegamenti a Google Drive e ai servizi di scrobbling vanno rifatti: i
  token stanno nel Credential Manager sotto il vecchio nome del servizio, e
  cercarli sotto il nuovo dà «non collegato».

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

### Aggiunto — l'account Spotify intero, per due strade

Fin qui da Spotify si importava **un link alla volta**. Quel che mancava era il
gesto grande: portare dentro tutte le playlist, i brani salvati, gli album, gli
artisti seguiti e la cronologia d'ascolto senza incollare trenta indirizzi a
mano.

Le strade sono due perché sono complementari, non alternative, e nessuna delle
due basta:

- **Il consenso OAuth** (`aether-spotify::account`) dà l'ISRC — il gradino zero
  dell'abbinamento, che il lettore keyless non riceve più — ed è ripetibile
  quando si vuole. Ma dal febbraio 2026 un'applicazione in Development Mode
  richiede che il proprietario abbia **Premium attivo**: se scade, smette di
  funzionare e Spotify non avvisa nessuno. La schermata lo dice **prima** del
  collegamento.
- **L'archivio ZIP** che Spotify manda su richiesta (`aether-archivio`) non
  chiede niente a nessuno e porta **anni** di cronologia, dove l'API ne dà
  cinquanta righe. In cambio arriva in due pezzi separati da settimane, e se ne
  può aprire uno solo: quel che manca resta vuoto e l'anteprima dice quale metà.

Il perno è che tutte e due producono lo **stesso valore di dominio**
(`AccountSnapshot`), quindi da lì in poi il codice è uno solo: un piano, una
conferma, una transazione. La finestra ha una schermata sola, e non sa da dove
viene quel che sta mostrando.

Quel che ne segue:

- **`aether-oauth`**, estratto: PKCE, il servitore di loopback e il portachiavi
  stavano dentro `aether-cloud` e sapevano di Google. Adesso servono a due
  padroni e non ne nominano nessuno.
- **Gli scope sono tutti di sola lettura**, con una prova che ne vieta
  l'allargamento. Aether non deve poter toccare l'account di nessuno.
- **`play_history.source`** distingue gli ascolti importati da quelli veri, ed è
  ciò che rende l'importazione **annullabile**: «dimentica gli ascolti
  importati» è una `DELETE` mirata più un ricalcolo, invece di un ripristino da
  backup — cioè invece di perdere anche tutto quel che si è fatto nel frattempo.
- **`playlists.spotify_playlist_id`**: senza, rinominare una playlist su Spotify
  ne creerebbe una seconda qui alla sincronizzazione successiva, perché
  `PlaylistKey` nasce dal nome.
- I brani mancanti finiscono in `spotify_wanted` e la coda yt-dlp parte da sé,
  come già faceva per un link singolo.

Tre difetti che solo il farlo girare ha mostrato:

- **La durata che l'archivio non scrive mai.** `SpotifyTrack.duration_ms` è
  sempre `None` là dentro — nessuno dei quattro formati ha un campo per la
  durata — e `counts_as_play` senza durata ricade sulla soglia dei quattro
  minuti: avrebbe scartato **ogni ascolto di ogni canzone più corta di quattro
  minuti**, in silenzio e sotto l'etichetta «troppo breve». Adesso l'abbinamento
  viene prima della soglia, e la durata la dà la libreria. Nessuna prova
  unitaria l'avrebbe presa: gli snapshot scritti a mano ce l'avevano tutti.
- **Il totale dichiarato conta anche i podcast, i brani no.** Una playlist di
  cinquanta canzoni più un podcast risultava «50 su 51» a ogni lettura, e
  `prepara_playlist` **rifiuta di sostituire** una playlist arrivata monca:
  quella playlist sarebbe diventata impossibile da reimportare, per sempre, a
  causa di un podcast.
- **Dal marzo 2026 anche il corpo delle risposte** ha rinominato `tracks` in
  `items` e `track` in `item`, e la guida lo dichiara per le playlist tacendo
  sugli altri elenchi. Si leggono tutti e due i nomi: sbagliare quale sia quello
  giusto importa zero brani da un account pieno, **senza nessun errore**.

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

### Corretto — un dispositivo audio perso adesso lo dice, e si riapre

Era il difetto noto della fase precedente: la riproduzione si fermava e la
finestra continuava a dire che suonava. `Motore::dispositivo_perso()` e
`causa_perdita()` esistevano dal primo giorno e **non li leggeva nessuno**,
quindi una scheda audio scomparsa — un dispositivo virtuale che si spegne, una
cuffia USB staccata — era indistinguibile da un brano che non parte.

Adesso l'orologio se ne accorge sul fronte, la finestra mostra una fascia con la
causa, e c'è un tasto **Riapri** che apre il motore nuovo **prima** di buttare
il vecchio, ripristinando volume, equalizzatore e normalizzazione. Funziona
anche nel caso in cui il motore non si è mai aperto: un'applicazione avviata
senza scheda audio adesso lo dichiara invece di restare muta, e
`riproduzione_stato` risponde uno stato fermo con la ragione invece di fallire.

### Aggiunto — playlist da file, e playlist che si scrivono da sole

- **M3U, M3U8, PLS e XSPF**, in lettura e scrittura (`playlist_file.rs`, puro).
  BOM, CRLF, percorsi relativi e assoluti, `file://` con le sequenze `%NN`.
  L'importazione abbina prima per percorso — esatto, poi per nome di file — e
  poi ricade sulla **stessa scala a quattro gradini** dell'importazione da
  Spotify, quindi una playlist esportata da un altro programma trova i brani
  anche se i file sono stati spostati. Quel che non trova lo **elenca**, con il
  percorso che c'era scritto.
- **Le playlist intelligenti.** Le colonne `is_smart` e `rules` erano nello
  schema dal primo giorno e non le scriveva nessuno. Adesso un insieme di regole
  su undici campi — artista, album, genere, anno, voto, preferito, ascolti,
  durata, aggiunto, ultimo ascolto, formato — diventa **SQL parametrizzato**:
  mai una concatenazione di stringhe, e una prova ci mette dentro
  `'; DROP TABLE tracks; --` per dimostrarlo. L'anteprima è viva mentre si
  scrive: si vede il conteggio scendere da 1421 a 5 mentre si digita.

Tre difetti trovati facendo girare le cose, non leggendo il codice:

- **`<trackList>` comincia con `<track`.** Cercare la sottostringa nuda faceva
  prendere l'apertura dell'elenco per una traccia, il cui blocco finiva al primo
  `</track>` vero: **il primo brano di ogni XSPF spariva**, sempre, su file
  perfettamente validi.
- Le playlist intelligenti dicevano «0 brani» nella barra laterale: il conteggio
  veniva da `playlist_tracks`, dove per loro non c'è nessuna riga.
- Una condizione appena aggiunta veniva dichiarata «non sta in piedi» prima di
  averci scritto dentro. Le regole incomplete adesso non si mandano e non si
  salvano.

### Aggiunto — lo scrobbling: ListenBrainz e Last.fm

Mandare fuori quel che si ascolta, con la **stessa regola** che conta tutto il
resto: metà brano o quattro minuti, quel che viene prima, mai sotto i trenta
secondi. Non è una comodità che si riusa una funzione — è che `play_count`, la
cronologia e lo scrobble devono contare la **stessa cosa**, e una seconda misura
scritta qui sarebbe il difetto che `listen.rs` esiste per correggere, rientrato
dalla porta di servizio.

- **`aether-scrobble`**, crate nuovo, non vede `rusqlite`. Non è igiene: la coda
  si svuota mille ascolti alla volta, e se questo crate potesse toccare la
  connessione la cosa naturale da scrivere sarebbe «leggi una riga, mandala,
  cancellala» — cioè tenere il lucchetto della libreria per tutta la durata
  della rete, con la riproduzione ferma dietro.
- **Una coda su disco** (`scrobble_queue`, migrazione 6). Quel che non parte
  perché non c'è rete non si perde e riparte da solo, anche dopo una chiusura.
  Le righe si portano dietro **i tag di allora**, non un `track_id`: fra
  l'ascolto e l'invio il brano può essere stato cancellato, spostato o
  ritaggato, e quel che va mandato è cosa si è ascoltato allora.
- **I due codici d'errore orfani trovano chi li usa.**
  `settings.lastfmNotConfigured` e `settings.lastfmNoPendingToken` stavano nel
  catalogo senza un solo chiamante: il secondo è precisamente il consenso di
  Last.fm chiesto quando il primo tempo non è mai stato fatto — perché quel
  consenso è a due tempi e in mezzo c'è una persona che torna dal browser.
- **La cronologia importata da Spotify si può mandare a ListenBrainz in
  blocco**, ed è il cerchio che si chiude: anni di ascolti diventano la propria
  cronologia su un servizio che non appartiene a nessuna piattaforma. Solo là, e
  non è una preferenza — Last.fm rifiuta le date vecchie e ha un tetto
  giornaliero.

Due cose che valgono più della somma delle righe che costano:

- **Un blocco rifiutato si divide invece di essere buttato.** ListenBrainz
  valuta il documento intero: un solo ascolto con una data impossibile fa
  rispondere `400` a tutti e mille. Segnare il blocco come fallito butterebbe
  novecentonovantanove ascolti buoni per colpa di uno, in silenzio e secondo le
  regole. Si dimezza finché non resta il colpevole.
- **Un ascolto ignorato è un ascolto consegnato.** Se Last.fm risponde «accettati
  3, ignorati 2», quei due non torneranno mai accettati — la data è quella che
  è. Rimetterli in coda vorrebbe dire rimandarli per sempre.

`aether-net` ha imparato una seconda intestazione: ListenBrainz non manda
`Retry-After` ma `X-RateLimit-Reset-In`, e senza leggerla un `429` aspetterebbe
alla cieca trenta secondi quando ne bastavano due — moltiplicato per ogni blocco
di una coda che si svuota.

### Aggiunto — le impostazioni si cercano, e si portano via

Nove sezioni sono oltre il punto in cui una cosa si trova scorrendo.

- **Una ricerca fra le sezioni.** L'indice è scritto a mano e non ricavato dal
  testo della pagina, perché il testo della pagina è quello della sezione
  **aperta**: un motore che vede un nono di quel che c'è direbbe «non c'è» di
  cose che ci sono. Metà dell'indice sono i sinonimi — nessuno cerca
  «normalizzazione», si cerca «volume» o «replaygain».
- **Il profilo delle impostazioni**: un file JSON con le proprie scelte, da
  riaprire su un altro computer o dopo una reinstallazione. Con il **piano**
  prima di applicare, come ogni altra cosa irreversibile qui: `profilo_piano` è
  `profilo_importa` in una transazione che viene abbandonata, quindi l'elenco
  che si legge non è una previsione, è il risultato.
- **Le scorciatoie si riassegnano.** `tastiera.ts` era uno `switch`: cambiarne
  una voleva dire ricompilare, e mostrarle a schermo voleva dire riscriverle a
  mano in un secondo elenco che prima o poi si scosta. Adesso la tabella che
  l'ascoltatore consulta è quella che la scheda disegna. Si preme il tasto
  invece di scriverne il nome, e `Esc` non si assegna: è l'uscita da ogni campo
  e da ogni finestrella.
- **Il tema è tornato nel nucleo.** Stava in `localStorage` per una ragione che
  era buona quando è stato scritto — è una preferenza della finestra, non un
  dato della libreria. Nel frattempo sono nati due lettori di *tutte* le
  preferenze, il backup su Drive e il profilo, e una preferenza in
  `localStorage` non finisce in nessuno dei due: chi ripristinava un backup si
  ritrovava la skin giusta e il tema sbagliato. Adesso è `ui.theme`, e la prima
  apertura dopo l'aggiornamento recupera la scelta vecchia e la riscrive dentro.

Tre decisioni che si vedono solo se qualcosa va storto:

- **Il profilo porta un elenco di inclusioni, non di esclusioni.**
  `nuvola.dispositivo` non deve viaggiare: due computer con lo stesso
  identificativo si rovinano il backup a vicenda, e ci si accorge mesi dopo.
  Neanche `player.queue`, che contiene identificativi di righe di `tracks` — su
  un'altra libreria nominano canzoni diverse. Il rovescio di un elenco di
  inclusioni è che dimenticarsi una chiave è silenzioso, quindi l'esportazione
  **dichiara** quali chiavi ha lasciato indietro.
- **Un profilo di una versione futura si rifiuta invece di essere letto a
  metà.** Leggerne le chiavi che si riconoscono sembra generoso e produce una
  macchina configurata a metà, senza dire quale metà.
- **Un tasto assegnato a due comandi si dichiara, non si rifiuta.** Il momento
  in cui succede è a metà di uno spostamento — per un istante ce l'hanno tutti
  e due — e rifiutare la seconda assegnazione obbligherebbe a fare i passi
  nell'ordine giusto senza dirlo. Nel frattempo vince il primo dell'elenco,
  quindi non esiste un istante di comportamento imprevedibile.

### Difetti noti

- **Il consenso OAuth di Spotify non è mai stato provato contro Spotify vero.**
  Serve un client id registrato e le mani di una persona sulla schermata di
  consenso, quindi resta l'unica parte dell'importazione dell'account che non è
  passata per una rete vera. L'altra via — l'archivio ZIP — sì, su un account
  intero.
- **Lo stesso vale per i due servizi di scrobbling.** Il documento che si manda,
  la firma di Last.fm, la lettura delle risposte e il destino di ogni riga in
  coda sono provati; il primo `200` da `api.listenbrainz.org` richiede un token
  di qualcuno.
- **Un profilo importato si vede subito solo per metà.** Tema, skin e
  scorciatoie cambiano mentre si guarda; volume, equalizzatore e
  normalizzazione li legge il motore audio quando si apre, e restano quelli di
  prima fino al riavvio. La scheda lo scrive invece di lasciarlo scoprire.
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
