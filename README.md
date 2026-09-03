# Aether

Un lettore musicale per la musica che possiedi, e un modo lecito di procurarti
quella che non hai.

Aether suona i file che hai già sul disco. Quando gli dai una playlist — dal tuo
archivio Spotify, da un file M3U, da un link — guarda cosa hai già in libreria e
va a cercare il resto **nei cataloghi liberi**: Internet Archive e Audius. Ogni
brano che entra porta con sé la licenza sotto cui è stato preso. Quel che nessun
catalogo libero ha finisce in una lista d'acquisto, con i link ai negozi dove
chi l'ha fatto viene pagato.

Non scarica da YouTube, non fa scraping di Spotify, non impacchetta binari di
terze parti. Il perché è scritto sotto, ed è la cosa più importante di questo
documento.

> **Stato:** in sviluppo. Compila e funziona su Windows; non è ancora firmato e
> il lato mobile non esiste. Vedi *Cosa manca* — e *Installare*, se sei qui per
> quello.

![La schermata d'apertura di Aether](immagini/casa.webp)

L'apertura non è un elenco alfabetico. «Riprendi dov'eri» ritrova il brano **e
il secondo** in cui l'avevi lasciato; sotto stanno gli ascoltati di recente,
gli album appena entrati, e i dischi che hai in libreria da mesi e non hai mai
aperto — l'unico ripiano che un servizio in streaming non può avere, perché per
lui non possiedi niente.

*Gli scatti di questo documento vengono da una libreria vera: millequattrocento
brani, novecentotrentacinque album.*

---

## Da dove viene la musica

| Fonte | Cosa dà | Si può tenere una copia? |
| --- | --- | --- |
| **I tuoi file** | La libreria vera. Scansione delle cartelle che scegli. | Sono già tuoi |
| **Internet Archive** | Live Music Archive (oltre 250 000 concerti di artisti che lo consentono), netlabel, pubblico dominio | Sì, quando la licenza dell'item lo dice |
| **Audius** | Quel che gli artisti ci pubblicano sotto licenza aperta | Sì, quando l'artista ha acceso lo scarico **e** la licenza lo consente |
| **Archivio GDPR di Spotify** | Le tue playlist, i preferiti, gli album, e la cronologia d'ascolto per intero | Sono metadati, non audio |
| **File di playlist** | M3U, M3U8, PLS: la fotografia di una libreria che qualcuno aveva | Il percorso, non il file |
| **MusicBrainz + Cover Art Archive** | Tag e copertine | Metadati liberi |

Su Audius decide l'artista, brano per brano: c'è un interruttore per lo scarico,
e sopra di lui ci sono i *cancelli* — un brano può essere chiuso dietro un
seguito o il possesso di un gettone. Aether guarda tutti e tre, e il permesso
che ne esce è l'intersezione dei no, non la somma dei sì.

**Jamendo** è scritto e provato ma **non è acceso** in questa versione, e la
ragione non è tecnica: la loro API è gratuita per i soli usi non commerciali, e
i termini definiscono l'uso commerciale come «any monetary compensation». Vedi
*Cosa manca*. Il modulo sa già che da lì si ascolta e basta — i loro termini
vietano la cache e l'accesso offline — e `aether-net::FlussoHttp` è la parte che
suona un brano senza scriverlo da nessuna parte.

![La sezione «Da dove arriva la musica», nelle impostazioni](immagini/fonti.webp)

Le stesse fonti, dentro il programma. «Da un link» accetta un indirizzo di
archive.org o audius.co e basta: nessun account, nessuna chiave, nessun
incollaggio di gettoni. «Il tuo archivio Spotify» legge lo zip che Spotify ti
consegna su richiesta, e lo legge qui — non viaggia da nessuna parte. Non c'è
un terzo campo dove mettere altro, perché non c'è altro che si possa mettere.

### Il punto onesto

**Nessuna fonte legale regala i download del catalogo commerciale.** Non è un
limite tecnico da aggirare: è il motivo per cui altri programmi fanno cose che
non si possono fare. Aether copre i bisogni per vie lecite, e per quel che
resta dice dove comprarlo. Se cercavi un modo di avere gratis la discografia di
qualcuno, questo non è il programma.

---

## Perché niente YouTube

Le *YouTube API Developer Policies* § III.E.1.a: «You must not… download,
import, backup, cache, or store copies of YouTube audiovisual content without
YouTube's prior written approval». Non c'è configurazione che lo renda lecito.

E non esiste nemmeno una versione corretta: le stesse policy vietano di
separare l'audio dal video (§ III.I.7) e di riprodurlo da un player non visibile
(§ III.I.9), e limitano a trenta giorni la conservazione dei metadati
(§ III.E.4). Un lettore musicale che tiene una libreria è precisamente la cosa
che quelle tre regole escludono. Quindi YouTube non si corregge: si toglie.

Lo stesso vale, con un'altra motivazione, per lo scraping di Spotify — endpoint
privati, gettoni ricostruiti, il client id del loro web player. Era accesso non
autorizzato al servizio. Resta l'archivio GDPR, che Spotify consegna
all'utente perché è un suo diritto (art. 20 del regolamento), e che porta più
cronologia di quanta ne desse l'API.

---

## Cosa fa, in concreto

- **Riproduzione gapless** con ReplayGain, equalizzatore, coda persistente.
  Un motore solo (`cpal` + `symphonia`), tre fili, nessun lucchetto fra chi
  decodifica e la callback audio.
- **Libreria** su SQLite: scansione incrementale che riconosce i file spostati,
  playlist normali e intelligenti, valutazioni, preferiti, cronologia d'ascolto.
- **Riordino dei file** sul disco in `Artista/Album/NN - Titolo`, con anteprima
  del piano e annullamento.
- **Arricchimento dei metadati** da MusicBrainz: cerca l'album intero, non il
  singolo brano, e quando le prove non bastano **non scrive niente**.
- **Skin** con un editor visuale (lo Studio) e un formato pacchettizzato.
- **Backup e sincronia** su Google Drive o su una cartella condivisa.
- **Scrobbling** verso Last.fm e ListenBrainz.
- **Aggiornamenti**: ogni mezz'ora Aether guarda le release su GitHub, e quando
  ne trova una nuova lo dice. Non installa da sé — quello parte da un tasto — e
  verifica la firma dell'installer prima di eseguirlo. Si spegne in
  Impostazioni; `PRIVACY.md` § 6 dice cosa viaggia, che è niente.

### Come si vede

![L'elenco dei brani, con copertina, album, voto e durata](immagini/brani.webp)

L'elenco si scorre e non si sfoglia: non c'è una «pagina 2», perché per
chiederla bisognerebbe già sapere dove sta la cosa che si sta cercando — che è
esattamente quel che non si sa. La pagina successiva se la chiede da sé quando
il fondo si avvicina.

![In riproduzione, con lo spettro tridimensionale acceso](immagini/riproduzione.webp)

Lo spettro prende i campioni **dopo** l'equalizzatore e **prima** del volume:
alzare i bassi si vede nelle barre, abbassare la manopola no — uno spettro che
si abbassa con la manopola descrive la manopola, non la musica. Le bande
arrivano dal motore su un anello che, quando è pieno, **perde**: la callback
che suona non aspetta un disegno, mai.

### Le skin non sono un tema scuro

| Plain, quella di serie | «Cyberpunk Edge», installata |
| --- | --- |
| ![La griglia degli album con la skin Plain](immagini/album.webp) | ![La stessa griglia con la skin Cyberpunk Edge](immagini/skin.webp) |

La stessa schermata, due skin: cambiano i colori d'accento, i bordi, le ombre,
il peso dei caratteri e il raggio degli angoli — cinquantadue parti in tutto,
non un interruttore chiaro/scuro.

![Lo Studio: albero delle parti, anteprima viva, ispettore](immagini/studio.webp)

Lo Studio è dove si scrivono. A sinistra le parti e i cinquantotto token, al
centro l'anteprima viva — che gira su una libreria finta, non sulla tua — a
destra l'ispettore, con la sonda che illumina la superficie sotto il puntatore
e ne dice il nome: il problema di chi fa una skin non è scegliere un colore, è
sapere come si chiama la cosa che sta guardando.

In fondo quattro numeri. Uno conta le coppie di colori che stanno **sotto
4,5:1**, cioè sotto la soglia di leggibilità: una skin che esce con sedici
avvisi l'ha deciso, non l'ha subito.

---

## Installare

L'installer per Windows sta fra gli allegati dell'[ultima
release](https://github.com/FedericoBaratti/Aether/releases/latest): un `.exe`
NSIS. Da lì in avanti Aether guarda le release ogni mezz'ora e dice quando ne
esce una nuova — installarla parte sempre da un tasto, mai da sé.

**SmartScreen lo bloccherà**, ed è giusto che lo faccia: il programma non è
firmato con un certificato, e Windows dice quel che sa, cioè niente. Si passa
da «Ulteriori informazioni» → «Esegui comunque». Il perché non ci sia di meglio
sta in *Cosa manca*, ed è una cifra, non una svista.

Quel che cambia da una versione all'altra sta in `CHANGELOG.md`, per intero e
col motivo accanto. La regola dei numeri è scritta in cima a quel file: una
versione che porta una migrazione del database è sempre una *minor*, perché le
migrazioni vanno solo avanti e tornare indietro vuol dire ripristinare una
copia, non disinstallare.

---

## Compilare

Serve Rust (la versione sta in `rust-toolchain.toml`) e Node 20 o più recente.

```bash
# Le prove: circa 1 370, e girano senza rete
cargo test --workspace

# L'applicazione, in sviluppo
cd apps/desktop
npm install
npm run dev
```

Per l'installer NSIS:

```bash
cd apps/desktop
npm run build
```

Gli avvisi sulle licenze di terze parti si rigenerano con:

```bash
node strumenti/licenze.js
```

L'updater vuole una coppia di chiavi, che si generano una volta sola e non
entrano nel repository (`.chiavi/` è in `.gitignore`):

```bash
cd apps/desktop
npm run tauri signer generate -- -w ../../.chiavi/aether.key
```

Il contenuto della `.pub` va in `plugins.updater.pubkey` dentro
`tauri.conf.json`; quello della privata nei segreti del repository, come
`TAURI_SIGNING_PRIVATE_KEY`. La privata firma gli eseguibili che si installano
da soli sui computer altrui: perderla vuol dire che nessuna installazione
esistente potrà più aggiornarsi. `strumenti/manifesto.js` confronta gli
identificativi delle due metà a ogni release, così un disallineamento si scopre
in CI e non sul computer di qualcun altro.

### Com'è fatto

```
core/
  aether-domain     puro: nessun I/O, nessun orologio, nessuno stato globale
  aether-play       il motore audio (cpal + symphonia)
  aether-app        la libreria: SQLite, scansione, importazioni, coda
  aether-catalogo   i cataloghi liberi — l'unico posto che scarica audio
  aether-net        HTTP bloccante (ureq), rate limit, circuit breaker
  aether-meta       MusicBrainz, Cover Art Archive, Deezer, iTunes
  aether-archivio   la lettura dell'archivio GDPR di Spotify
  aether-skin       il formato delle skin e il suo compilatore
  aether-oauth      PKCE e portachiavi di sistema
  aether-cloud      Google Drive
  aether-sync       la sincronia fra dispositivi
  aether-scrobble   Last.fm e ListenBrainz
apps/desktop        la finestra: Tauri 2 + React 19
```

Il verso delle dipendenze non è casuale: `aether-catalogo` non vede `rusqlite`,
ed è ciò che rende **impossibile** — non solo sconsigliato — tenere preso il
lucchetto della libreria per il minuto che dura un prelievo.

---

## Licenze

Il codice di Aether è **MIT** (`LICENSE`).

Le sue dipendenze non lo sono tutte: la famiglia `symphonia` è **MPL-2.0** e
`cpal` è **Apache-2.0 puro**. `THIRD-PARTY-NOTICES.md` le elenca tutte col loro
testo, e si rigenera con lo script qui sopra. I font Geist e Bricolage
Grotesque sono sotto **SIL OFL 1.1** (`apps/desktop/src/font/OFL.txt`).

La musica che Aether ti procura porta la licenza della sua fonte, che non è
quella del programma. `TERMS.md` dice cosa comporta — compreso il vincolo **non
commerciale** del Live Music Archive, che vale anche per te che ascolti.

---

## Privacy

Aether non ha telemetria, non ha account, e non manda niente a nessun server
che non sia quello a cui stai chiedendo qualcosa. Parla con la rete solo quando
gliel'hai chiesto: i cataloghi, MusicBrainz, Drive se lo colleghi, il servizio
di scrobbling se lo colleghi. `PRIVACY.md` elenca ogni singola richiesta che il
programma può fare e cosa ci mette dentro.

---

## Come si sostiene

**Gratuito, con donazioni.** Non c'è una versione a pagamento e non ce ne sarà
una: due delle fonti — il Live Music Archive e Jamendo — hanno clausole *non
commercial*, e una versione a pagamento le violerebbe. Il vincolo è scritto nei
termini apposta perché non si perda per strada.

---

## Cosa manca

- **Firma del codice** per Windows. Senza, SmartScreen blocca il download: è il
  singolo motivo per cui la maggior parte delle applicazioni indipendenti non
  viene installata. Un certificato OV richiede validazione dell'identità e un
  costo annuo.
- **Jamendo**: il codice c'è e le prove passano (`cargo test -p aether-catalogo
  --features jamendo`), ma la feature nasce spenta finché
  `licensing@jamendo.com` non dice se le donazioni contino come uso
  commerciale. Vedi sotto.
- **La riproduzione dei brani di solo ascolto.** `FlussoHttp` sa leggere un
  file remoto posizionandosi, e il motore audio accetta già un flusso invece di
  un percorso; quel che manca è il posto in libreria per un brano che non è un
  file, e la superficie «Esplora» da cui cercarli. Oggi un brano Audius non
  scaricabile si vede in elenco e non parte.
- **macOS e Linux**: oggi l'unico bersaglio dell'installer è NSIS, quindi
  Windows.
- **Il lato mobile**, che nel vecchio albero esisteva e qui non è ancora stato
  riscritto.

Due cose vanno chiarite per iscritto prima di pubblicare, e sono segnate qui
perché non si dimentichino: se le **donazioni** contino come uso commerciale per
Jamendo (`licensing@jamendo.com`) e per l'API di Deezer usata nei metadati.
Finché non c'è una risposta, il modulo Jamendo nasce dietro una feature Cargo
che si può spegnere.
