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

La riscrittura su `aether/skin-system-e-core`, iniziata il 26 luglio 2026.
**Non è ancora rilasciabile**: l'app desktop è una fetta verticale diagnostica e
il lato mobile non esiste. Lo stato per fase è in [docs/stato.md](docs/stato.md),
i requisiti per poter rilasciare in [docs/rilascio.md](docs/rilascio.md).

### Aggiunto

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
