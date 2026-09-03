# Aether

Lettore musicale per Windows, scritto in Rust. Riproduce i file audio presenti
sul disco e integra la libreria locale con i cataloghi musicali ad accesso
libero.

> **Stato del progetto:** in sviluppo. Compila e funziona su Windows.
> L'eseguibile non è firmato digitalmente e le versioni per macOS, Linux e
> dispositivi mobili non esistono. Vedi [Limiti noti e sviluppi
> previsti](#limiti-noti-e-sviluppi-previsti).

## Indice

- [Panoramica](#panoramica)
- [Fonti della musica](#fonti-della-musica)
- [Fonti escluse](#fonti-escluse)
- [Funzionalità](#funzionalità)
- [Interfaccia](#interfaccia)
- [Personalizzazione](#personalizzazione)
- [Installazione](#installazione)
- [Compilazione](#compilazione)
- [Architettura](#architettura)
- [Licenze](#licenze)
- [Privacy](#privacy)
- [Sostenibilità del progetto](#sostenibilità-del-progetto)
- [Limiti noti e sviluppi previsti](#limiti-noti-e-sviluppi-previsti)
- [Documenti correlati](#documenti-correlati)

---

## Panoramica

Aether riproduce i file audio già presenti sul disco. A partire da una playlist
— esportata dall'archivio Spotify, letta da un file M3U o indicata tramite un
collegamento — confronta i brani con la libreria locale e recupera quelli
mancanti dai cataloghi ad accesso libero: Internet Archive e Audius. Ogni brano
acquisito conserva la licenza della fonte da cui proviene. I brani non
disponibili in alcun catalogo libero vengono raccolti in una lista d'acquisto,
con i collegamenti ai negozi che remunerano gli autori.

Il programma non scarica contenuti da YouTube, non effettua scraping di Spotify
e non distribuisce binari di terze parti. Le motivazioni sono documentate in
[Fonti escluse](#fonti-escluse) e costituiscono un vincolo di progetto, non una
limitazione temporanea.

![La schermata iniziale di Aether](immagini/casa.webp)

La schermata iniziale. Il riquadro «Riprendi dov'eri» ripristina l'ultimo brano
interrotto e la posizione esatta al suo interno. Sotto sono elencati i brani
ascoltati di recente, gli album aggiunti per ultimi e i dischi presenti in
libreria da tempo e mai riprodotti.

*Le immagini di questo documento provengono da una libreria reale di 1465 brani
e 935 album.*

---

## Fonti della musica

| Fonte | Contenuti | Copia locale |
| --- | --- | --- |
| **File locali** | La libreria principale. Scansione delle cartelle indicate dall'utente. | Già in possesso dell'utente |
| **Internet Archive** | Live Music Archive (oltre 250 000 concerti di artisti che ne hanno autorizzato la registrazione), netlabel, pubblico dominio | Sì, quando la licenza dell'item lo consente |
| **Audius** | Brani pubblicati dagli artisti sotto licenza aperta | Sì, quando l'artista ha abilitato il download **e** la licenza lo consente |
| **Archivio GDPR di Spotify** | Playlist, brani e album salvati, artisti seguiti, cronologia d'ascolto completa | Metadati, non audio |
| **File di playlist** | M3U, M3U8, PLS | Il percorso, non il file |
| **MusicBrainz + Cover Art Archive** | Tag e copertine | Metadati liberi |

Su Audius il permesso è determinato dall'artista per singolo brano. Esistono tre
controlli distinti: l'interruttore del download e due *cancelli*, che possono
subordinare l'accesso al seguito dell'artista o al possesso di un token. Aether
li verifica tutti e tre e considera il permesso concesso solo in assenza di
divieti.

**Jamendo** è implementato e coperto da test, ma disabilitato in questa
versione. La causa non è tecnica: l'API è gratuita per i soli usi non
commerciali e i termini definiscono l'uso commerciale come «any monetary
compensation». Il modulo è già predisposto per il solo ascolto in streaming — i
termini vietano la cache e l'accesso offline — attraverso `aether-net::FlussoHttp`,
che riproduce un brano senza scriverlo su disco. Vedi [Limiti
noti](#limiti-noti-e-sviluppi-previsti).

![La sezione «Da dove arriva la musica» nelle impostazioni](immagini/fonti.webp)

Le stesse fonti nell'interfaccia del programma. Il campo «Da un link» accetta
indirizzi di archive.org e audius.co senza richiedere account o chiavi API. «Il
tuo archivio Spotify» elabora localmente l'archivio ZIP che Spotify consegna
all'utente su richiesta.

### Cosa non copre

Nessuna fonte ad accesso libero distribuisce il catalogo commerciale. Non si
tratta di un limite tecnico aggirabile: è la ragione per cui altri programmi
svolgono operazioni non consentite dai termini dei servizi che utilizzano.
Aether copre il fabbisogno attraverso canali leciti e, per il resto del
catalogo, indica dove acquistarlo. Chi cerca uno strumento per ottenere
gratuitamente discografie commerciali non troverà in Aether questa funzione.

---

## Fonti escluse

### YouTube

Le *YouTube API Developer Policies* vietano lo scaricamento e la memorizzazione
dei contenuti audiovisivi (§ III.E.1.a: «You must not… download, import, backup,
cache, or store copies of YouTube audiovisual content without YouTube's prior
written approval»). Non esiste una configurazione che renda l'operazione
conforme.

Le stesse policy vietano inoltre la separazione dell'audio dal video
(§ III.I.7), la riproduzione da un player non visibile (§ III.I.9) e la
conservazione dei metadati oltre trenta giorni (§ III.E.4). Un lettore musicale
che mantiene una libreria persistente è incompatibile con tutte e tre le
clausole. La fonte è quindi esclusa integralmente, e non in una versione
ridotta.

### Accesso non autorizzato a Spotify

L'uso degli endpoint privati di Spotify, di token ricostruiti e del client id
del player web costituisce accesso non autorizzato al servizio ed è escluso.

Resta disponibile l'archivio GDPR, che Spotify è tenuto a consegnare all'utente
su richiesta ai sensi dell'art. 20 del Regolamento UE 2016/679, e che contiene
una cronologia d'ascolto più estesa di quella offerta dall'API pubblica.

---

## Funzionalità

- **Riproduzione gapless** con ReplayGain, equalizzatore e coda persistente. Un
  solo motore audio (`cpal` + `symphonia`) distribuito su tre thread, senza
  alcun lock condiviso fra il decodificatore e la callback audio.
- **Libreria su SQLite**: scansione incrementale con riconoscimento dei file
  spostati, playlist manuali e automatiche, valutazioni, preferiti e cronologia
  d'ascolto.
- **Riorganizzazione dei file** sul disco secondo lo schema
  `Artista/Album/NN - Titolo`, con anteprima del piano di rinomina e
  annullamento.
- **Arricchimento dei metadati** da MusicBrainz. La ricerca è effettuata
  sull'album completo anziché sul singolo brano; in assenza di corrispondenze
  sufficientemente affidabili non viene scritto alcun dato.
- **Skin** con editor visuale integrato (lo Studio) e formato di distribuzione
  pacchettizzato.
- **Backup e sincronizzazione** su Google Drive o su una cartella condivisa.
- **Scrobbling** verso Last.fm e ListenBrainz.
- **Aggiornamenti automatici**: verifica delle release su GitHub ogni trenta
  minuti, con notifica all'utente. L'installazione richiede sempre una conferma
  esplicita e la firma dell'installer viene verificata prima dell'esecuzione. La
  funzione è disattivabile dalle impostazioni; il traffico generato è
  documentato in [`PRIVACY.md`](PRIVACY.md) § 6.

---

## Interfaccia

![L'elenco dei brani](immagini/brani.webp)

L'elenco dei brani, con copertina, album, valutazione e durata. Il caricamento è
progressivo: le pagine successive vengono richieste automaticamente
all'avvicinarsi della fine dell'elenco, senza paginazione numerata.

![La schermata di riproduzione con l'analizzatore di spettro](immagini/riproduzione.webp)

La schermata di riproduzione con l'analizzatore di spettro attivo. I campioni
sono prelevati dopo l'equalizzatore e prima del controllo di volume: le
modifiche alla curva di equalizzazione sono visibili nelle barre, quelle al
volume no. Le bande raggiungono l'interfaccia attraverso un buffer circolare che
scarta i campioni in eccesso quando è pieno, in modo da non introdurre attese
nella callback audio.

---

## Personalizzazione

| Skin «Plain», predefinita | Skin «Cyberpunk Edge», installata |
| --- | --- |
| ![La griglia degli album con la skin Plain](immagini/album.webp) | ![La stessa griglia con la skin Cyberpunk Edge](immagini/skin.webp) |

La stessa schermata con due skin differenti. Una skin ridefinisce colori,
bordi, ombre, pesi tipografici e raggi di curvatura su 52 componenti
dell'interfaccia: non si tratta di un'alternanza fra tema chiaro e tema scuro.

![Lo Studio, l'editor delle skin](immagini/studio.webp)

Lo Studio è l'editor integrato. A sinistra l'albero dei componenti e dei 58
token; al centro l'anteprima aggiornata in tempo reale, che opera su una
libreria di prova e non su quella dell'utente; a destra l'ispettore, con una
sonda che identifica il componente sotto il puntatore.

La barra inferiore riporta errori, avvisi, componenti ridisegnati e il numero di
coppie di colori con rapporto di contrasto inferiore a 4,5:1, la soglia di
leggibilità raccomandata. Il dato è calcolato prima dell'esportazione, così che
una skin distribuita con avvisi di contrasto sia il risultato di una scelta
consapevole.

---

## Installazione

L'installer per Windows è disponibile fra gli allegati dell'[ultima
release](https://github.com/FedericoBaratti/Aether/releases/latest): un
eseguibile NSIS.

**Avviso di Windows SmartScreen.** L'eseguibile non è firmato con un
certificato di code signing, quindi SmartScreen ne blocca l'esecuzione al primo
avvio. Per procedere: «Ulteriori informazioni» → «Esegui comunque». Le ragioni
dell'assenza della firma sono indicate in [Limiti
noti](#limiti-noti-e-sviluppi-previsti).

Le modifiche introdotte da ciascuna versione sono elencate in
[`CHANGELOG.md`](CHANGELOG.md). Il criterio di numerazione è descritto in cima a
quel file: ogni versione che introduce una migrazione del database comporta
l'incremento del numero *minor*, poiché le migrazioni non sono reversibili e il
ritorno a una versione precedente richiede il ripristino di una copia del
database antecedente all'aggiornamento.

---

## Compilazione

### Requisiti

- **Rust** — versione indicata in `rust-toolchain.toml`
- **Node.js** 20 o superiore
- **Windows** — unica piattaforma supportata dall'installer

### Comandi

```bash
# Suite di test: circa 1370 test, eseguibili senza connessione di rete
cargo test --workspace
```

```bash
# Avvio in modalità sviluppo
cd apps/desktop
npm install
npm run dev
```

```bash
# Compilazione dell'installer NSIS
cd apps/desktop
npm run build
```

```bash
# Rigenerazione degli avvisi sulle licenze di terze parti
node strumenti/licenze.js
```

### Chiavi di firma dell'updater

Il sistema di aggiornamento richiede una coppia di chiavi, generata una sola
volta. Le chiavi non vengono versionate: la cartella `.chiavi/` è esclusa
tramite `.gitignore`.

```bash
cd apps/desktop
npm run tauri signer generate -- -w ../../.chiavi/aether.key
```

Il contenuto del file `.pub` va inserito in `plugins.updater.pubkey` all'interno
di `tauri.conf.json`; la chiave privata va registrata fra i segreti del
repository con il nome `TAURI_SIGNING_PRIVATE_KEY`.

La chiave privata firma gli eseguibili che vengono installati automaticamente
sui sistemi degli utenti: la sua perdita impedisce definitivamente
l'aggiornamento di tutte le installazioni esistenti. Lo script
`strumenti/manifesto.js` confronta gli identificativi delle due metà a ogni
release, in modo che un eventuale disallineamento emerga in CI.

---

## Architettura

```
core/
  aether-domain     logica pura: nessun I/O, nessun accesso all'orologio,
                    nessuno stato globale
  aether-play       motore audio (cpal + symphonia)
  aether-app        libreria: SQLite, scansione, importazioni, coda
  aether-catalogo   cataloghi ad accesso libero; unico modulo che scarica audio
  aether-net        HTTP bloccante (ureq), rate limiting, circuit breaker
  aether-meta       MusicBrainz, Cover Art Archive, Deezer, iTunes
  aether-archivio   lettura dell'archivio GDPR di Spotify
  aether-skin       formato delle skin e relativo compilatore
  aether-oauth      PKCE e portachiavi di sistema
  aether-cloud      Google Drive
  aether-sync       sincronizzazione fra dispositivi
  aether-scrobble   Last.fm e ListenBrainz
apps/desktop        interfaccia: Tauri 2 + React 19
```

La direzione delle dipendenze è vincolante. `aether-catalogo` non ha accesso a
`rusqlite`: è quindi strutturalmente impossibile, e non semplicemente
sconsigliato, mantenere il lock della libreria per l'intera durata di un
download.

---

## Licenze

Il codice di Aether è distribuito sotto licenza **MIT** ([`LICENSE`](LICENSE)).

Le dipendenze adottano licenze differenti: la famiglia `symphonia` è
**MPL-2.0**, `cpal` è **Apache-2.0**. L'elenco completo con i testi integrali è
in [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md), rigenerabile con lo
script indicato in [Compilazione](#compilazione). I font Geist e Bricolage
Grotesque sono distribuiti sotto **SIL OFL 1.1**
(`apps/desktop/src/font/OFL.txt`).

La musica acquisita tramite Aether è soggetta alla licenza della fonte di
provenienza, distinta da quella del programma. Le implicazioni sono descritte in
[`TERMS.md`](TERMS.md), inclusa la clausola **non commerciale** del Live Music
Archive, che vincola anche l'ascoltatore.

---

## Privacy

Aether non raccoglie dati di telemetria, non richiede la creazione di un account
e non comunica con server diversi da quelli espressamente interrogati
dall'utente: i cataloghi musicali, MusicBrainz, Google Drive e il servizio di
scrobbling, questi ultimi due solo se collegati.

[`PRIVACY.md`](PRIVACY.md) elenca ogni singola richiesta di rete che il
programma può effettuare e i dati che ciascuna trasmette.

---

## Sostenibilità del progetto

Aether è gratuito e finanziato tramite donazioni. Non esiste una versione a
pagamento e non ne è prevista una: due delle fonti integrate, il Live Music
Archive e Jamendo, sono soggette a clausole *non commercial* che una versione a
pagamento violerebbe. Il vincolo è riportato esplicitamente nelle condizioni
d'uso.

---

## Limiti noti e sviluppi previsti

- **Firma del codice per Windows.** In assenza di un certificato, SmartScreen
  blocca l'esecuzione dell'installer, che è la principale causa di mancata
  installazione delle applicazioni indipendenti. Un certificato OV richiede la
  validazione dell'identità del richiedente e un costo annuale.
- **Jamendo.** Il codice è presente e i test passano
  (`cargo test -p aether-catalogo --features jamendo`), ma la funzionalità è
  disabilitata in attesa di un chiarimento formale da parte di
  `licensing@jamendo.com` sul trattamento delle donazioni come uso commerciale.
- **Riproduzione dei brani in solo streaming.** `FlussoHttp` è in grado di
  leggere un file remoto con accesso posizionale e il motore audio accetta già
  un flusso al posto di un percorso. Mancano la rappresentazione in libreria di
  un brano privo di file locale e la sezione «Esplora» da cui effettuarne la
  ricerca. Allo stato attuale un brano Audius non scaricabile compare
  nell'elenco ma non viene riprodotto.
- **macOS e Linux.** L'unico bersaglio dell'installer è NSIS: la piattaforma
  supportata è pertanto Windows.
- **Applicazione mobile.** Presente nella versione precedente del progetto, non
  ancora riscritta.

Due questioni richiedono un chiarimento scritto prima della pubblicazione: se le
**donazioni** costituiscano uso commerciale ai fini dei termini di Jamendo
(`licensing@jamendo.com`) e dell'API Deezer impiegata per i metadati. In assenza
di risposta il modulo Jamendo resta dietro una feature Cargo disattivabile.

---

## Documenti correlati

| Documento | Contenuto |
| --- | --- |
| [`CHANGELOG.md`](CHANGELOG.md) | Modifiche di ogni versione e criterio di numerazione |
| [`PRIVACY.md`](PRIVACY.md) | Informativa sul trattamento dei dati e richieste di rete |
| [`TERMS.md`](TERMS.md) | Condizioni d'uso e licenze dei contenuti acquisiti |
| [`SECURITY.md`](SECURITY.md) | Procedura di segnalazione delle vulnerabilità |
| [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) | Licenze delle dipendenze |
| [`STUDIO-STREAMING.md`](STUDIO-STREAMING.md) | Analisi progettuale sull'integrazione dello streaming nella libreria |
