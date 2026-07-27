# Stato della riscrittura

Aggiornato al 27 luglio 2026. Undici commit sul ramo `aether/skin-system-e-core`.

**556 test** su 19 file. `npm run verify` (typecheck node + web, lint, check:node12,
test) verde a ogni commit.

## Fatto e verificato

| fase | stato |
|---|---|
| **0** Monorepo | completa. npm workspaces, i due alberi in `legacy/` come riferimento |
| **1** Nucleo core | completa. Errori tipizzati end-to-end, contratto IPC, logger, supervisor, resilienza |
| **2** Core | **parziale**: DB, riproduzione, capacità, adapter desktop, app avviabile |
| **3** Skin engine | completa. Registro token, effetti, parts registry, compilatore, pacchetto |
| **4** Conversione | **parziale**: livello token di tutte e tre le skin |
| **5** Studio | **parziale**: la logica (bozza, contrasto) |
| **6** Trasporto | **parziale**: l'allineamento delle librerie |
| **7** Animazioni | non iniziata |

### L'app si avvia

Verificato sul database reale creato dall'avvio:

```
user_version: 100          la baseline: la catena unificata ha girato
tabelle: 18 + tracks_fts   (+4 tabelle ombra di FTS5)
colonne di tracks: 40      tutte quelle accumulate nei 18 passi
```

E dal log su file, che prima non esisteva affatto:

```
INFO [boot] avvio paths="…aether-desktop" version="1.0.0"
INFO [boot] skin di serie caricate count=3 ids=["plain","nothing","cyberpunk"]
INFO [db]   database aperto version=100 latest=100 fts5=true migrations=0
```

Il binario nativo di better-sqlite3 e il runtime Electron sono stati recuperati
dall'albero legacy (stesse versioni: 11.10.0 e 33.4.11), senza rete. Il binario è
compilato per l'ABI di Electron e non si carica da Node semplice — è il motivo per
cui l'adapter separa `adaptBetterSqlite` (puro, provato con un doppio) da
`openBetterSqlite` (l'unico punto che carica il nativo).

## Da fare

### Fase 4 — livello parti
Dichiarate 6 parti per `nothing` e 11 per `cyberpunk`, contro 1.084 e 1.709 righe di
CSS nei sette file di partial ciascuna. Il criterio di accettazione è il **confronto
pixel a pixel**, superficie per superficie, chiaro e scuro: richiede l'app aperta
accanto alla versione legacy.

### Fase 5 — interfaccia dello Studio
Pannelli generati dal registro (la funzione `panelFor` c'è già), anteprima dal vivo
con `adoptedStyleSheets` (`applySkinCss` c'è già), trascinamento di un'immagine per
estrarre la palette (riusare `extractPalette` da `usePalette.ts` nel legacy),
esportazione e invio al telefono.

### Fase 6 — trasporto vero
Quattro rotte sul server del telefono (`POST /api/skin/upload`, `/commit`,
`GET /api/skins`, `DELETE /api/skin/:id`) e tre sul server LAN del desktop. I
protocolli, l'accoppiamento con QR e token, mDNS e il rate limiting esistono già in
`legacy/`: è un innesto, non un protocollo nuovo.

**Due bug reali da chiudere qui**, individuati durante l'esplorazione:
1. `legacy/…/src/lib/lanClient.ts:209-221` tiene le impostazioni **solo in memoria**
   in modalità LAN, quindi la scelta della skin si perde al riavvio.
2. `MainActivity.java:74` e `capacitor.config.ts:21` cablano `#09090d`, che è il
   `surface-0` di *plain*: con un'altra skin l'avvio a freddo lampeggia del colore
   sbagliato. `activeSurfaceColor()` in `apps/desktop/src/skinRuntime.ts` mostra come
   leggerlo dalla skin attiva invece di duplicarlo.

### Fase 2 — quel che resta
Moduli di dominio (libreria, scansione, metadati, download, sync), adapter mobile
(riusare `sqlite-shim.ts`, `net-polyfill.ts`, `electron-shim.ts` dal legacy),
spezzare `NativeAudioPlugin.kt` (1.174 righe, 2 catch) e collegare il suo
`onPlayerError` alla macchina a stati — `fromExoPlayerError` è già pronta.

### Fase 7
Motore spring interno (~150 righe, nessuna libreria), transizioni a elemento
condiviso sopra `RouteTransitions.tsx`, gesture inerziali, e budget **misurato su un
dispositivo Android reale**. Più la convenzione `{data, status, error}` su tutti gli
store, la regola di lint che vieta `void window.aether.*` senza catch (30 siti nel
legacy), e la rimozione di `legacy/`.

## Cose apprese che valgono per il lavoro futuro

**Dove il messaggio d'errore conta, zod va guidato a mano.** Si è presentato tre
volte: `z.union` riporta «Invalid input» e inghiotte il messaggio del ramo;
`z.record` con chiavi enumerate le rende tutte obbligatorie; `z.record` con un
`refine` sulle chiavi riporta «Invalid key in record» senza dire quale chiave. In
questo formato il messaggio preciso è l'intero valore dello strato di validazione,
quindi in quei punti c'è un dispatch scritto a mano.

**La forma interna non è serializzabile come sorgente.** Un colore validato è
`{r,g,b,a}`, non `"#8b7cf6"`. Chi scrive un pacchetto o un editor deve tenere la
sorgente.

**Ogni pezzo verificabile trova difetti in quelli precedenti.** Il formato di
pacchetto ha corretto il modello di modifica dello Studio; la verifica di contrasto
ha corretto un arrotondamento che rendeva inutile il suo stesso suggerimento;
l'avvio dell'app ha corretto il sink dei log (i log di avvio non arrivavano su disco)
e la configurazione della build (i pacchetti del monorepo venivano esternalizzati, e
a runtime Electron avrebbe fatto `require` di un `.ts`). Costruire la logica
provabile prima dell'interfaccia è ciò che ha reso economici questi ritrovamenti.

**Non usare `Get-Content`/`Set-Content` di PowerShell per riscrivere file.** Legge
con la codepage ANSI e corrompe l'UTF-8: `Café` è diventato `CafÃ©` in un file di
test, e i test sono passati da verdi a rossi con messaggi che sembravano un bug della
logica di normalizzazione. Usare uno script Node.
