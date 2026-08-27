# Aether

Un lettore musicale per la musica che possiedi, e un modo lecito di procurarti
quella che non hai.

Aether suona i file che hai già sul disco. Quando gli dai una playlist — dal tuo
archivio Spotify, da un file M3U, da un link — guarda cosa hai già in libreria e
va a cercare il resto **nei cataloghi liberi**: Internet Archive, e in seguito
Jamendo e Audius. Ogni brano che entra porta con sé la licenza sotto cui è stato
preso. Quel che nessun catalogo libero ha finisce in una lista d'acquisto, con i
link ai negozi dove chi l'ha fatto viene pagato.

Non scarica da YouTube, non fa scraping di Spotify, non impacchetta binari di
terze parti. Il perché è scritto sotto, ed è la cosa più importante di questo
documento.

> **Stato:** in sviluppo. Compila e funziona su Windows; non è ancora firmato e
> il lato mobile non esiste. Vedi *Cosa manca*.

---

## Da dove viene la musica

| Fonte | Cosa dà | Si può tenere una copia? |
| --- | --- | --- |
| **I tuoi file** | La libreria vera. Scansione delle cartelle che scegli. | Sono già tuoi |
| **Internet Archive** | Live Music Archive (oltre 250 000 concerti di artisti che lo consentono), netlabel, pubblico dominio | Sì, quando la licenza dell'item lo dice |
| **Archivio GDPR di Spotify** | Le tue playlist, i preferiti, gli album, e la cronologia d'ascolto per intero | Sono metadati, non audio |
| **File di playlist** | M3U, M3U8, PLS: la fotografia di una libreria che qualcuno aveva | Il percorso, non il file |
| **MusicBrainz + Cover Art Archive** | Tag e copertine | Metadati liberi |

In arrivo (Fase 2): **Jamendo** e **Audius** in streaming. Jamendo vieta
esplicitamente la cache e l'accesso offline nei suoi termini, quindi da lì si
ascolta e basta — e Aether lo rispetta invece di scoprire se il server glielo
lascerebbe fare.

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

---

## Compilare

Serve Rust (la versione sta in `rust-toolchain.toml`) e Node 20 o più recente.

```bash
# Le prove: circa 1 100, e girano senza rete
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
- **Jamendo e Audius** in streaming, con la superficie «Esplora».
- **macOS e Linux**: oggi l'unico bersaglio dell'installer è NSIS, quindi
  Windows.
- **Il lato mobile**, che nel vecchio albero esisteva e qui non è ancora stato
  riscritto.

Due cose vanno chiarite per iscritto prima di pubblicare, e sono segnate qui
perché non si dimentichino: se le **donazioni** contino come uso commerciale per
Jamendo (`licensing@jamendo.com`) e per l'API di Deezer usata nei metadati.
Finché non c'è una risposta, il modulo Jamendo nasce dietro una feature Cargo
che si può spegnere.
