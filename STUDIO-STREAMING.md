# Studio: lo streaming come libreria

Analisi della lista [chayotic/Open-Source-Music-Streaming-Apps](https://github.com/chayotic/Open-Source-Music-Streaming-Apps)
e proposta di come portare in Aether l'esperienza di quelle app — una libreria
che unisce musica locale e streaming — **senza violare le condizioni scritte in
`TERMS.md` e senza tradire la logica del programma**.

Questo è un documento di studio: non descrive codice esistente, propone una
strada. Ultimo aggiornamento: 1 settembre 2026.

---

## 1. Cos'è quella lista, e cosa se ne può prendere

Il repository è una **lista curata**, non un'applicazione: circa cinquanta
programmi liberi per ascoltare musica, divisi per piattaforma (Android, iOS,
desktop, web) e per natura (client di streaming online contro lettori offline).
La lista è pubblicata sotto **CC0**, quindi il suo contenuto — l'elenco, la
classificazione — è liberamente riutilizzabile.

Non c'è però una «libreria di codice» da importare. Quello che la lista offre
ad Aether è altro:

1. **Un catalogo di pattern d'esperienza**: come quelle app fanno convivere
   ricerca, ascolto immediato, coda e collezione personale.
2. **Un banco di prova di conformità**: per ognuna ci si può chiedere «Aether
   potrebbe fare così?» — e la risposta, quasi sempre, illumina esattamente il
   confine che `TERMS.md` traccia.

---

## 2. Il triage: come streammano davvero quelle app

La lista non lo dichiara, ma basta aprire i repository per vederlo: la grande
maggioranza dei *client di streaming* elencati prende l'audio da YouTube o
YouTube Music attraverso le API interne del player web, oppure appoggiandosi a
frontend alternativi (Piped, Invidious) che fanno la stessa cosa un passo più
in là.

| Gruppo | Esempi dalla lista | Come ottengono l'audio | Verdetto per Aether |
|---|---|---|---|
| Client YouTube Music | SimpMusic, ViTune, Metrolist, Muzza, Kreate, RiPlay, N-Zik, VIVI Music, Gyawun, Music-You… | API interne di YouTube (estrazione dei flussi audio dal player) | **Escluso.** `TERMS.md` §3: le YouTube API Developer Policies vietano il download (§ III.E.1.a), la separazione dell'audio dal video (§ III.I.7) e la riproduzione da player non visibile (§ III.I.9). Un lettore musicale con libreria è esattamente ciò che quelle regole escludono. |
| Ibridi metadati+audio | Spotube, Musify, BloomeeTunes, Nuclear, muffon, Pear Desktop | Metadati da Spotify/Last.fm/JioSaavn, audio da YouTube/Piped e simili | **Escluso** per la parte audio, stessa ragione. La parte metadati non serve: Aether ha già MusicBrainz, Deezer, iTunes e Cover Art Archive in `core/aether-meta`. |
| Client del proprio server | **Supersonic** (protocollo OpenSubsonic: Navidrome, Gonic, Airsonic…) | Streaming HTTP dalla **propria** istanza, autenticata, della **propria** musica | **Conforme e interessante.** Nessun diritto altrui in gioco: il server è dell'utente, i file sono i suoi. È l'unico pattern di streaming della lista importabile alla lettera. Vedi fase 4. |
| Lettori offline | Auxio, Namida, Harmonoid, Nora, Tauon Music Box, Retro Music Player, Gramophone… | Nessuno streaming: file locali | **Già il territorio di Aether.** Utili solo come confronto di interfaccia (navigazione della libreria, code, viste artista/album). |
| Strumenti di scaricamento | yt-dlp, SpotiFLAC | Estrazione diretta dai servizi commerciali | **Escluso e attivamente bloccato**: la CI (`verify.yml`, lavoro `niente-di-vietato`) fa fallire la build se quei nomi rientrano nel codice. Nominarli qui è lecito — il controllo copre `*.rs`/`*.ts`/`*.tsx`, non i documenti — ed è il posto giusto per spiegare perché restano fuori. |

La conclusione del triage è netta: **il modello dominante della lista
(l'audio «gratis» da YouTube) è la cosa che Aether rifiuta per costituzione.**
Quello che si può integrare è l'*esperienza* — cercare, premere play, avere
tutto in un'unica libreria — costruita sopra le fonti che Aether già considera
lecite, più al massimo una fonte nuova (il proprio server) che di diritti
altrui non ne tocca.

---

## 3. Cosa Aether possiede già

La parte sorprendente dello studio è quanta strada sia già fatta. Le cuciture
esistono, documentate e testate; manca il filo che le unisce.

- **Il motore accetta già un flusso, non un file.**
  `aether_play::Sorgente` trasporta `media: Box<dyn Flusso>` — byte leggibili e
  posizionabili, da dovunque arrivino (`core/aether-play/src/decodifica.rs`).
- **Il flusso HTTP esiste ed è testato, ma non è collegato a niente.**
  `FlussoHttp` (`core/aether-net/src/flusso.rs`) legge un file remoto con
  richieste `Range` a finestra scorrevole, implementa `Read + Seek`, e per
  scelta non tocca mai il disco (i termini di Jamendo vietano la cache). Oggi
  nessun modulo fuori da `aether-net` lo usa: è il pezzo costruito in anticipo.
- **Il vocabolario delle fonti è già a prova di licenza.**
  `Fonte`, `Licenza`, `Disponibilita` in `core/aether-domain/src/esterno.rs`:
  ogni brano esterno dichiara da dove viene, con che licenza, e se si può
  scaricare (`Scaricabile`), solo ascoltare (`SoloAscolto`) o solo comprare
  (`SoloAcquisto`). `Licenza::Sconosciuta` non permette la copia: il dubbio
  vale un no.
- **Il cancello è prima della rete.** `core/aether-catalogo/src/prelievo.rs`
  rifiuta il prelievo di un candidato non conservabile *prima* di qualunque
  richiesta HTTP — il principio di `TERMS.md` §3 fatto codice.
- **Le fonti sono un elenco chiuso, non un sistema di plugin.**
  `Cataloghi` (`core/aether-catalogo/src/lib.rs`) compone strutture concrete
  (Internet Archive, Audius, Jamendo dietro feature spenta); il riconoscimento
  degli URL (`riferimento.rs`) è una allowlist rigida di domini. Aggiungere una
  fonte è una modifica guidata dal compilatore, ed è voluto così.
- **Il buco è dichiarato nel README** («Cosa manca»): `FlussoHttp` sa leggere,
  il motore sa suonare un flusso, ma la libreria non ha un posto per una
  traccia che non è un file (`tracks.path` è `NOT NULL UNIQUE`) e non esiste
  una superficie per cercarle. Oggi un brano Audius solo-ascolto compare in
  lista e non suona.

Un vincolo architetturale da rispettare in ogni fase: `aether-app` (la
libreria, SQLite) **non dipende** da `aether-net`, e `aether-catalogo` non vede
mai `rusqlite::Connection`. Il punto dove un flusso di rete può essere
assemblato e consegnato al motore è `apps/desktop/src-tauri`, che dipende da
tutti e quattro.

---

## 4. La roadmap conforme: «streaming come libreria»

Quattro fasi, ordinate per rapporto resa/rischio. Le prime due chiudono il buco
dichiarato nel README; la terza costruisce l'esperienza da app di streaming; la
quarta è l'unico vero import dalla lista.

### Fase 1 — Far suonare ciò che oggi tace

*Le tracce solo-ascolto di Audius e Internet Archive diventano riproducibili.*

In `apps/desktop/src-tauri/src/riproduzione.rs`, dove oggi `sorgente_di()`
apre un file, si aggiunge il ramo: se la traccia ha un `fonte_url` e
disponibilità `SoloAscolto`, si costruisce un `FlussoHttp` sopra la `Rete` del
catalogo giusto e lo si consegna a `motore.suona()` come `Sorgente` qualunque.
Gapless, crossfade, equalizzatore, spettro: tutto funziona uguale, perché il
motore non ha mai saputo cosa ci fosse dietro i byte.

- **Obblighi nuovi:** nessuno. Le fonti sono le stesse, i termini pure; anzi si
  *onora* meglio la distinzione ascolto/possesso, perché niente viene salvato.
- **Punti d'attenzione:** gestione degli errori di rete a metà brano (il canale
  `riproduzione:errore` esiste già); il prefetch gapless (`prepara_prossimo`)
  apre la connessione in anticipo — va rispettato il rate limit per servizio
  già presente in `aether-net`.
- **Sforzo:** contenuto. È cablaggio, non costruzione.

### Fase 2 — Un posto in libreria per una traccia che non è un file

*La libreria smette di presumere che ogni brano abbia un percorso.*

Oggi l'identità di una traccia è il suo percorso (`tracks.path NOT NULL
UNIQUE`, migrazione `001_baseline.sql`) e `playback::sorgente`
(`core/aether-app/src/playback.rs`) fa `SELECT path`. Serve una migrazione in
stile casa (`core/aether-app/src/db/schema/016_….sql`, numerazione consecutiva
verificata dai test) che renda il percorso facoltativo e affianchi i campi già
noti a `desiderati` dalla migrazione 10: `fonte_url`, `licenza`,
`disponibilita`. Una traccia è *o* un file *o* un riferimento a catalogo, mai
nessuno dei due.

- **Rispetto dei confini:** `aether-app` continua a non vedere la rete; deve
  solo saper dire «questa traccia si apre così» restituendo il riferimento, e
  chi ha in mano sia la libreria sia la rete (`src-tauri`) assembla il flusso.
  In alternativa, una fabbrica di `Flusso` iniettata accanto a `MusicFiles` —
  lo stesso schema con cui `files.rs` prepara la strada ad Android.
- **Effetti a catena da verificare:** scansione e rilevamento spostamenti in
  `library.rs` (devono ignorare le tracce senza percorso), statistiche e
  cronologia (funzionano per id, non per percorso: dovrebbero reggere),
  sincronia fra dispositivi (una traccia-riferimento viaggia bene: è solo una
  riga, non un file), riordino dei file su disco (deve saltarle).
- **Sforzo:** il più delicato delle quattro fasi. È qui che vive davvero il
  «come se ci fosse una libreria» chiesto da questo studio.

### Fase 3 — «Esplora»: la superficie da app di streaming

*Cercare nei cataloghi liberi dall'app, ascoltare subito, tenere ciò che si può.*

È il pezzo d'esperienza preso in prestito dalle app della lista, applicato alle
fonti lecite. Una nuova voce in `Vista`
(`apps/desktop/src/parti/Navigazione.tsx`), comandi Tauri che espongono
`Cataloghi::cerca` al frontend attraverso `ipc.ts`, risultati come `Candidato`
(già normalizzati: titolo, autore, durata, natura, licenza, disponibilità).

Regole d'interfaccia che discendono dai termini, non dal gusto:

- ogni risultato mostra **fonte e licenza**, come già fa la coda;
- il pulsante è «**Ascolta**» per tutti; «**Salva in libreria**» compare solo
  se `Licenza::permette_copia` — lo stesso cancello di `prelievo.rs`, mai
  aggirato dal frontend;
- ogni brano porta il collegamento alla sua **pagina pubblica** sul catalogo
  (`pagina_url`): l'attribuzione visibile che alcune licenze CC richiedono;
- ciò che nessun catalogo ha continua a finire in «Da comprare» (§4 dei
  termini), che è la risposta di Aether al bisogno che le app vietate risolvono
  male.

Sforzo: medio, quasi tutto frontend più una manciata di comandi; i cataloghi e
la ricerca fan-out esistono già (`procura.rs` li usa oggi in batch).

### Fase 4 (opzionale) — La fonte presa dalla lista: il proprio server

*OpenSubsonic/Navidrome, alla maniera di Supersonic.*

L'unico pattern di streaming della lista importabile senza riserve: l'utente
punta Aether alla **propria** istanza (Navidrome, Gonic…), e la musica che ha
già altrove entra nella stessa libreria. Nessun diritto altrui: server suo,
file suoi, autenticazione sua.

Checklist già implicita nell'architettura (il modello è la feature `jamendo`):

1. variante `Fonte::Subsonic` in `core/aether-domain/src/esterno.rs` con
   `nome`/`etichetta`/`puo_consegnare`;
2. modulo in `core/aether-catalogo/` con la sua `Rete` dedicata (rate limit e
   interruttore di circuito gratis);
3. riconoscimento in `riferimento.rs` — qui non una allowlist di domini ma
   l'indirizzo configurato dall'utente, e nient'altro;
4. feature Cargo spenta per default (`aether-catalogo` + `aether-desktop`);
5. migrazione per il nuovo valore di `source_service` (stile `010_cataloghi.sql`);
6. `ipc.ts`, mappa `nomeFonte`, `lingue/it.json` + `lingue/en.json` (la CI
   pretende la parità);
7. credenziali nel **portachiavi di sistema** via `aether-oauth`/keyring, mai
   in SQLite — la regola di casa per i segreti.

Nota di merito: il protocollo Subsonic usa per l'autenticazione classica un
condimento MD5; OpenSubsonic offre di meglio e Navidrome supporta token — da
preferire. E siccome i file sono dell'utente, qui `Disponibilita` può essere
`Scaricabile`: scaricare da sé stessi è il caso più pulito che esista.

Sforzo: il più alto (autenticazione, configurazione, un protocollo intero),
ma interamente parallelo alle altre fasi.

---

## 5. Cosa resta fuori, e perché non è negoziabile

Vale la pena dirlo senza giri: **non esiste una versione conforme di Spotube
dentro Aether.** Non è prudenza, è aritmetica dei termini:

- l'audio da YouTube è vietato dalle policy citate in `TERMS.md` §3, e il
  passaggio per Piped/Invidious non cambia la sostanza — cambia solo chi fa la
  richiesta;
- lo scraping di Spotify è escluso: di Spotify si legge solo l'archivio GDPR
  che Spotify consegna all'utente, ed è già supportato (`aether-archivio`);
- il vincolo *non commerciale* di Live Music Archive e Jamendo (§2) è la
  ragione per cui Aether è e resta gratuito: ogni fonte nuova va letta anche
  con questa lente;
- i cataloghi di testi vietati (§3-bis) restano vietati: la fase 3 non tocca i
  testi, LRCLIB resta l'unica strada;
- la CI fa rispettare tutto questo meccanicamente: il lavoro
  `niente-di-vietato` fallisce la build se nel codice rientrano strumenti di
  scaricamento o fonti di testi escluse. Se una fase di questa roadmap
  sembrasse richiedere un'eccezione a quei controlli, è la fase a essere
  sbagliata, non il controllo.

Come dice `TERMS.md` §3: se si trova un modo di far fare ad Aether una di
queste cose, è un difetto da segnalare — e questo studio non ne introduce.

---

## 6. In sintesi

| Fase | Cosa dà | Nuovi obblighi legali | Sforzo |
|---|---|---|---|
| 1. Cablare `FlussoHttp` | Le tracce solo-ascolto suonano | Nessuno | Basso |
| 2. Tracce senza file in libreria | Streaming *dentro* la libreria, non accanto | Nessuno | Medio-alto |
| 3. Schermata «Esplora» | L'esperienza da app di streaming, sulle fonti lecite | Nessuno (attribuzione già prevista) | Medio |
| 4. OpenSubsonic (opz.) | La propria musica remota nella stessa libreria | Nessuno (musica propria) | Alto |

La lista di chayotic, letta fino in fondo, conferma la scommessa di Aether:
quasi tutte quelle app comprano l'esperienza al prezzo di termini violati.
Aether può avere l'esperienza tenendosi i termini — perché i pezzi difficili,
il motore che suona byte e il vocabolario delle licenze, li ha già costruiti.
