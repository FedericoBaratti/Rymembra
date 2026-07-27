# Rilasciare Aether

Due parti: la **procedura**, che funziona già oggi, e i **requisiti**, cioè le
cose che devono essere vere prima che abbia senso eseguirla.

## Prima di tutto: quale numero

**La 1.0.0 è già stata pubblicata**, dal vecchio albero, il 17 luglio 2026
(`legacy/Aeter/release/Aether Setup 1.0.0.exe`). La riscrittura non può quindi
uscire come 1.0.0: chi ha già installato l'app vedrebbe la stessa versione con
metà delle funzioni, e un eventuale aggiornamento automatico la tratterebbe come
un ritorno indietro.

Quando la riscrittura raggiunge la parità, esce come **1.1.0** — o come **2.0.0**
se a quel punto il formato skin o il protocollo di trasporto avranno rotto la
compatibilità con la 1.0.0, che è la regola in [CHANGELOG.md](../CHANGELOG.md).

I manifest dicono `1.0.0` perché è la versione a cui il prodotto sta ora, non
perché sia il prossimo rilascio. Il numero si cambia con `npm run version:set`.

## Requisiti: cosa manca per poter rilasciare

Nessuno di questi è un dettaglio di impacchettamento; sono le ragioni per cui il
pulsante oggi non va premuto.

### Bloccanti

| requisito | stato |
|---|---|
| L'app desktop riproduce musica | **no** — `apps/desktop/src/App.tsx` è una fetta verticale diagnostica: sette canali IPC (diagnostica, impostazioni, elenco skin, conteggi), nessun lettore, nessuna libreria sfogliabile |
| Esiste l'app mobile | **no** — `apps/mobile/` è un `package.json` |
| Moduli di dominio (libreria, scansione, metadati, download, sync) | **no** — Fase 2 parziale |
| Interfaccia dello Skin Studio | **no** — c'è la logica, non i pannelli |
| Le rotte del trasporto sono montate sui due server reali | **no** — il protocollo è provato su TCP, l'innesto non è fatto |
| Le tre skin convertite a livello di parti, confronto pixel a pixel | **no** — Fase 4 parziale |

### Da fare prima del primo installer pubblico

- **`resources/bin`**: il vecchio albero spedisce `yt-dlp.exe`, `ffmpeg.exe`,
  `spotdl.exe`, `fpcalc.exe` come `extraResources`. La voce non è in
  `electron-builder.yml` perché il modulo download non è ancora portato: va
  riaggiunta insieme a quello, altrimenti l'app si installa e i download non
  partono.

  ```yaml
  extraResources:
    - from: resources/bin
      to: bin
      filter: ['**/*']
  ```

- **Firma del codice.** Su Windows senza certificato l'installer parte con
  l'avviso SmartScreen; su macOS `identity: null` significa che l'utente deve
  autorizzare l'app a mano. Entrambe sono scelte consapevoli, non dimenticanze:
  vanno confermate o risolte prima di dare il link a qualcuno.

- **Provare l'installer su una macchina pulita**, non su quella di sviluppo: è
  l'unico modo di accorgersi di un modulo nativo che si carica solo perché il
  toolchain è installato. L'app impacchettata qui si avvia e apre il database,
  ma questa macchina ha il toolchain: non è la stessa prova.

## Procedura

### 1. Il cancello

```
npm run verify
```

typecheck (node e web), lint, compatibilità Node 12 del codice che gira sul
backend mobile, allineamento delle versioni, e i test. Deve essere verde: il
workflow di rilascio lo rifà comunque e si ferma prima di impacchettare.

### 2. Numero e changelog

```
npm run version:set -- 1.1.0
```

Allinea i sette manifest. Poi si sposta la sezione `[Non rilasciato]` del
changelog sotto il nuovo numero, con la data.

### 3. Moduli nativi

```
npm run rebuild:native
```

**Passo esplicito, e deve restare tale.** `better-sqlite3` va compilato per l'ABI
di Electron, non per quello di Node. electron-builder non lo fa più da solo
(`npmRebuild: false`) perché nel monorepo quel passo esegue una installazione di
sole dipendenze di produzione con `appDir` su `apps/desktop`, e quel che pota
sono le **devDependencies della radice**: la prima esecuzione ha cancellato
`electron`, `electron-vite`, `vite`, `electron-builder` e i `@types` dall'albero.
Se dovesse succedere di nuovo, si rimette con:

```
npm install --offline --ignore-scripts
```

e, se `node_modules/electron/dist` resta vuoto, copiandolo da
`legacy/Aeter/node_modules/electron/dist` (stessa versione, 33.4.11).

### 4. Prova di impacchettamento

```
npm run pack:desktop
```

Produce `apps/desktop/release/win-unpacked/` senza installer e senza firma. È il
controllo veloce, e serve a vedere tre cose:

- `resources/app.asar.unpacked/**/*.node` contiene `better_sqlite3.node` e
  `sharp-win32-x64.node` — se sono finiti dentro l'asar l'app non si avvia;
- nell'asar non c'è nessun `node_modules/@aether/*` — i pacchetti del monorepo
  sono transpilati dentro il bundle e vanno spediti solo da lì. Si controlla con:

  ```
  node -e "console.log(require('@electron/asar').listPackage('apps/desktop/release/win-unpacked/resources/app.asar').filter(f => f.includes('@aether')))"
  ```

- `Aether.exe` si avvia e la finestra diagnostica risponde.

Nota su cosa invece c'è e non serve: l'asar contiene 454 file `.ts`, tutti di
`node-taglib-sharp`, che spedisce i propri sorgenti accanto al compilato. Non è
un guasto — a runtime si carica `lib/` — ed è peso morto per ora doppio, perché
il modulo metadati non è nemmeno portato. Vale la pena escluderli con una
negazione in `files` quando si misurerà la dimensione dell'installer, non prima:
è una modifica da verificare con l'app che usa davvero quella libreria.

### 5. Gli installer

```
npm run dist:desktop            # solo Windows, da Windows
npm run dist:mac -w apps/desktop
npm run dist:linux -w apps/desktop
```

Ogni sistema si impacchetta dal proprio: è quello che fa la matrice del workflow.

### 6. Tag

```
git tag v1.1.0 && git push origin v1.1.0
```

Il workflow `Release` rifà `verify`, costruisce sui tre sistemi e appende gli
artefatti a una **bozza** di release. Pubblicarla resta un clic di una persona,
e va fatto dopo aver provato almeno l'installer di Windows su una macchina
pulita.

## Cosa è stato verificato di questa procedura

Su questa macchina, senza rete:

- `npm run verify` — verde, 631 test su 23 file;
- `npm run check:version` — provato anche in negativo, disallineando un manifest;
- `npm run pack:desktop` — `release/win-unpacked/Aether.exe`, con i due moduli
  nativi fuori dall'asar e nessuna voce `@aether/*` dentro;
- `npm run dist:desktop` — entrambi i target di Windows costruiti:
  `Aether Setup 1.0.0.exe` (90 MB, nsis) e `Aether 1.0.0.exe` (89 MB, portable),
  più il blockmap. Non firmati, come previsto;
- **l'app impacchettata si avvia**, e nel log scrive le tre righe di boot:

  ```
  INFO [boot] avvio paths="…aether-desktop" version="1.0.0"
  INFO [boot] skin di serie caricate count=3 ids=["plain","nothing","cyberpunk"]
  INFO [db]   database aperto version=100 latest=100 fts5=true migrations=0
  ```

  La terza è quella che conta: se il database si apre, `better_sqlite3.node` si è
  caricato da `app.asar.unpacked`. È la prova che `asarUnpack` è giusto, e si
  ottiene solo lanciando il binario prodotto — non quello di sviluppo;
- l'impacchettamento **non tocca `node_modules`**: inventario delle 61
  dipendenze dirette e impronta del binario nativo identici prima e dopo. È il
  controllo che `npmRebuild: false` fa il suo lavoro.

**Non** verificati, e vanno provati quando servono:

- gli installer di macOS e Linux (`dmg`, `AppImage`, `deb`): vanno costruiti dal
  proprio sistema, qui c'è solo Windows;
- la firma del codice, su entrambi i sistemi;
- l'**installazione** vera: `Aether Setup 1.0.0.exe` è stato costruito ma non
  eseguito. Va provato su una macchina pulita, non su questa, dove un modulo
  nativo potrebbe caricarsi solo perché il toolchain è presente;
- i due workflow, che non hanno mai girato — questo repository non ha ancora un
  remoto GitHub.
