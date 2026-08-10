# Prompt per Claude: ridisegno UX/UI di Aether desktop

## Chi sei e cosa devi fare

Sei un designer di prodotto che sa scrivere il codice del proprio disegno. Devi
ridisegnare l'interfaccia di **Aether**, un lettore musicale locale per desktop:
Tauri 2 + React 19 sopra un nucleo Rust. L'obiettivo non è un restyling
cosmetico — è portare l'interfaccia al livello del nucleo che ha sotto, che è
molto più maturo di quel che si vede.

Il risultato deve essere **moderno, sobrio e finito**: non un mood board, non
uno schizzo, ma un sistema di disegno completo più il codice che lo realizza,
dentro i vincoli elencati sotto. Vincoli che non sono negoziabili: sono la
ragione per cui questa riscrittura esiste.

---

## 1. Il contesto tecnico, in breve

Monorepo, workspace Rust alla radice:

| Crate / cartella | Cosa fa |
|---|---|
| `core/aether-domain` | Dominio puro: chiavi brano/playlist, normalizzazione testo, album, coda, piani di scansione e riordino, fusione statistiche, catalogo errori. Niente orologio, niente disco. |
| `core/aether-play` | Motore audio: `cpal` + `symphonia`, gapless, ReplayGain, volume. |
| `core/aether-app` | Orchestrazione: SQLite + migrazioni, scansione, FTS5, copertine, playlist, riordino con giornale, importazione dal vecchio DB. |
| `core/aether-skin` | **Il sistema delle skin**: registro token, registro parti, effetti parametrici, compilatore, formato `.aeskin`. |
| `apps/desktop/src-tauri` | La finestra: comandi Tauri, nessuna logica propria. |
| `apps/desktop/src` | Il frontend React — **quello che devi ridisegnare**. |

Dipendenze del frontend, per intero: `react`, `react-dom`, `@tauri-apps/api`,
`@tauri-apps/plugin-dialog`. Build: Vite 7, TypeScript strict con
`exactOptionalPropertyTypes`. **Nessuna** libreria di componenti, di icone, di
animazione, nessun CSS framework. Un solo foglio di stile: `src/stile.css`,
~1200 righe, classi in italiano.

La CSP della finestra è chiusa: `default-src 'self'`, immagini solo da `self`,
`data:` e `http://aether-cover.localhost`, script solo da `self`. **Nessun CDN,
nessun font remoto, nessuna fetch esterna.** Qualunque asset deve essere
impacchettato nel bundle.

Finestra: 1280×820 di default, minimo 880×560. Lingua dell'interfaccia:
**italiano**, e i nomi di classi, file e componenti sono in italiano — è una
convenzione del repo, si mantiene.

---

## 2. Il vincolo che governa tutto: il sistema delle skin

Questa è la parte che devi capire prima di disegnare una singola schermata.

**In Aether i colori non stanno nel CSS.** Una skin è un documento dati
(`.aeskin`, JSON validato) che il crate Rust `aether-skin` compila in un foglio
di stile iniettato a runtime da `applicaSkin()` in `src/ipc.ts`. Il CSS
dell'applicazione consuma proprietà personalizzate; non ne dichiara i valori.

### 2.1 Il registro dei token (`core/aether-skin/src/tokens.rs`)

È il contratto. Sono ~45 token, raggruppati. I nomi CSS sono **contratto
stabile**: i componenti li leggono, e cambiarli rompe le skin già scritte.

- **Tipografia** — `--font-sans` (obbligatorio), `--font-mono`, `--font-dot`
  (carattere da display per titoli grandi, numeri, eyebrow).
- **Superfici** — `--color-surface-0` (fondo app, e colore dell'avvio a
  freddo), `--color-surface-1` (pannelli e barre), `--color-surface-2` (schede
  e righe), `--color-surface-3` (elementi sollevati, stati attivi).
- **Testo** — `--color-text-1`, `--color-text-2`, `--color-text-3`.
- **Accento** — `--accent` (+ tripla `--accent-rgb`), `--accent-soft`,
  `--accent-glow`, `--accent-like` (il cuore, separato apposta),
  `--hero-rgb` (solo tripla: i gradienti la usano dentro `rgba()` calcolate a
  runtime).
- **Semantici** — `--danger`, `--danger-soft`, `--success`, `--success-soft`,
  `--warning`, `--warning-soft`.
- **Chrome** — `--sidebar-bg`, `--hairline`, `--ambient-1`, `--ambient-2`.
- **Layout** — `--rail-w` (barra laterale chiusa, 68px), `--rail-w-expanded`
  (240px), `--player-h` (92px), `--player-gap` (14px), `--content-x`
  (`clamp(16px, 3cqw, 48px)`).
- **Geometria** — `--radius-panel` (20px), `--radius-card` (14px).
- **Elevazione** — `--shadow-1`, `--shadow-2`, `--shadow-3`, `--shadow-player`
  (con hairline interna), `--glow-accent`.
- **Movimento** — `--ease-out-expo`, `--ease-spring`, `--dur-1` (150ms),
  `--dur-2` (280ms), `--dur-3` (450ms).
- **Canvas** — `--viz-primary`, `--viz-secondary`, `--viz-glow`,
  `--scrubber-glow`, `--scrubber-rest`.

Calcolati dal compilatore, non dichiarabili da una skin: `--shell-left`,
`--player-clearance`, `--transition-fast`, `--transition-med`, `--surface-0-rgb`.

**La spaziatura non è un token, ed è deliberato.** `--spazio-1..5` vivono in
`stile.css` perché una skin che potesse cambiarla potrebbe far uscire le cose
dallo schermo. Se ti serve una scala di spaziatura più ricca, la estendi **lì**,
non nel registro.

### 2.2 Regole ferree

1. **Mai un colore letterale nel CSS o nel JSX.** Ogni colore, raggio, ombra,
   durata e curva viene da un token. Se ti serve qualcosa che il registro non
   ha, la risposta di default è comporlo dai token esistenti; proporre un token
   nuovo è una decisione da argomentare a parte (vedi §7).
2. **Il blocco `:root` in cima a `stile.css` è generato.** Fra i marcatori
   `inizio blocco generato` / `fine blocco generato` c'è l'uscita di
   `cargo run -p aether-skin --example compila`, e un test del crate desktop
   confronta le due cose. Non si tocca a mano: si rigenera.
3. Il testo sopra `--accent` usa `--color-surface-0`, non un nero scelto a mano:
   è così che una skin chiara ottiene contrasto senza dichiarare un token in
   più. Mantieni questa regola ovunque introduci superfici accentate.

### 2.3 Il registro delle parti (`core/aether-skin/src/parts.rs`) — qui c'è il lavoro grosso

Sono **48 classi CSS** che una skin può ridisegnare (sfondo, bordo, ritaglio,
colore testo, spaziatura, stati — non CSS libero). Divise in gruppi: `Shell`,
`Nav`, `Page`, `Controls`, `Lists`, `Player`, `NowPlaying`, `Overlays`.

**L'interfaccia attuale non ne emette quasi nessuna.** Usa un vocabolario suo
(`.telaio`, `.barra`, `.laterale`, `.riga`, `.scheda`, `.lettore`,
`.finestrella`, `.menu`, `.blocco`, `.voce`…) che le skin non conoscono. Il
risultato è che il motore delle skin, che è la parte più curata del progetto,
oggi può cambiare solo i token — non una singola superficie.

Parti disponibili e a cosa corrispondono nel disegno che farai:

```
Shell      app-shell, ambient-backdrop*
Nav        bottom-nav*, nav-pill
Page       page-header, page-title, page-subtitle, hero-eyebrow, hero-art*,
           section-card*, section-heading, section-icon, stat-number
Controls   icon-btn, play-btn-primary*, btn-accent, btn-ghost, switch,
           switch-track, field-input, range-accent, tooltip-pill
Lists      track-grid, queue-list, home-shortcuts, empty-state*, empty-icon,
           skeleton*
Player     player-shell*, player-progress*, progress-sheen
NowPlaying np-screen*, np-art*, np-title, np-meta, np-transport, np-scrim,
           lyrics-screen*, lyric-line, viz-screen*, viz-title, eq-bars,
           eq-slider
Overlays   glass-modal*, menu-pop*, toast-card*, toast-progress, tour-tooltip*
```

`*` = la parte ha uno pseudo-elemento libero per un livello aggiuntivo (è così
che le skin fanno le scanline CRT, le griglie a punti, i pavimenti prospettici).

**Compito esplicito**: il tuo ridisegno deve far combaciare il markup con questo
registro. Ogni superficie che disegni porta la classe della parte corrispondente
in aggiunta (non al posto) alla classe semantica italiana, oppure — se è più
pulito — la classe della parte diventa la classe semantica. Decidi tu quale
delle due, ma dichiara la regola e applicala ovunque. Le parti che il disegno
non usa vanno elencate come «non implementate», non ignorate in silenzio.

### 2.4 Quel che la skin dichiara e l'interfaccia oggi ignora

`plain.json` dichiara `capabilities: { light: true, mobile: false,
dynamicAccent: true }`, un tema `light` completo, e:

```json
"layout": { "player": "floating", "sidebar": "rail", "density": "comfortable" }
"motion": { "intensity": "full", "routeTransition": { … } }
```

Nessuno di questi è letto dall'interfaccia. `Skin.dynamicTokens` arriva
dall'IPC e viene buttato. `--font-dot`, `--rail-w`, `--content-x` non compaiono
in nessuna regola. Il tema chiaro non è raggiungibile da nessun comando.

Il tuo disegno deve **onorarli**, o dire per iscritto perché no.

---

## 3. Cosa c'è oggi: inventario completo

Leggi il codice, non fidarti di questo riassunto — ma questo è il perimetro.

### Impalcatura (`App.tsx`, `.telaio`)
Griglia CSS: barra superiore 58px a tutta larghezza + colonna laterale fissa
240px + contenuto. Player flottante `position: fixed` in basso (92px, margine
14px). Pannello coda `position: fixed` a destra (340px).

### Barra superiore
Marchio (un anello CSS bicolore + «Aether»), campo di ricerca a pillola
(max 520px, debounce 140ms), e — solo nella vista «brani», solo se non si sta
cercando — un `<select>` con quattro ordinamenti (scaffale, recenti, ascoltati,
titolo).

### Barra laterale — cinque blocchi in una colonna sola
1. Navigazione: Album / Brani / Preferiti, ognuno col suo conteggio.
2. Playlist: elenco con conteggio, `⚙` per le automatiche, tasto «Nuova
   playlist…», menù contestuale (Riproduci / Rinomina / Elimina).
3. Cartelle: elenco dei percorsi sorvegliati con troncamento RTL, tasto `⇅` per
   riordinare quella cartella, «Aggiungi cartella…», «Scansiona» + barra di
   avanzamento + esito (`+12 ~3 ↦1 −0 in 19.5 s`).
4. «Dalla versione precedente»: importa ascolti e playlist.
5. «Aspetto»: `<select>` delle skin installate + «Installa una skin…».
6. «Libreria»: artisti, ore d'ascolto, percorso della cartella dati.

### Viste del contenuto
- **Griglia album**: `repeat(auto-fill, minmax(164px, 1fr))`, copertina quadrata
  + titolo + artista. Menù contestuale: riproduci dopo / accoda.
- **Elenco brani**: griglia a 7 colonne `34px 34px 1fr 1fr 78px 52px 34px` —
  indice (che diventa ▶ al sorvolo), miniatura, titolo, artista·album, stelle,
  durata, cuore. Ottava colonna «togli» dentro una playlist.
- **Album aperto**: copertina 168px + h1 + meta + «▶ Riproduci», poi l'elenco.
- **Playlist aperta**: come sopra senza copertina; nota esplicativa per le
  automatiche.
- **Ricerca**: «N risultati per «query»» + elenco.
- **Vuoti**: «Nessuna cartella sorvegliata» e «Libreria vuota», entrambi con
  una call to action.

### Player (`Lettore.tsx`)
Tre colonne: [copertina 52px + titolo/artista + cuore] · [⇄ ⏮ ⏸ ⏭ ↻ sopra
scrubber con tempi monospace] · [☰ coda, 🔇/🔊, volume]. La posizione si
interpola a 50ms fra i colpi da 250ms del nucleo (`riproduzione.ts`); lo
scrubber è un `input[type=range]` nativo con riempimento via `--avanzamento`.

### Coda (`Coda.tsx`)
Pannello fisso a destra: intestazione con conteggio e «Svuota», righe
trascinabili (HTML5 drag&drop), doppio clic per saltare, ✕ per togliere. Un
brano sparito dalla libreria si mostra come «brano non più in libreria» e resta
rimovibile.

### Finestrelle
`Riordino.tsx` (larga, 900px: rapporto + gruppi da rivedere + elenco degli
spostamenti percorso→percorso, esecuzione con avanzamento, annullamento),
`Importa.tsx`, `AggiungiAPlaylist.tsx`, `Chiedi.tsx` (prompt), `Menu.tsx`
(menù contestuale ancorato al puntatore).

### Il contratto IPC (`src/ipc.ts`) — è ricco, leggilo tutto
Comandi disponibili che **l'interfaccia non usa o usa a metà**:
`playlistRiordina` (riordino brani dentro una playlist: c'è nell'IPC, non c'è
nell'UI), `codaDopo`/`codaAccoda` (solo da menù contestuale, mai da tastiera o
da un tasto visibile), `skin(id)` (anteprima di una skin senza sceglierla).

---

## 4. I problemi da risolvere — questo è il brief

Non è una lista di desiderata: è quel che ho trovato leggendo. Affrontali tutti,
e se decidi di non affrontarne uno scrivi perché.

### Struttura e navigazione
1. **La barra laterale è insieme navigazione e pannello di controllo.** Scansione,
   importazione, riordino del disco, scelta della skin e statistiche stanno
   nella stessa colonna delle playlist. Non esiste una superficie
   «Impostazioni». Serve una separazione fra *dove vado* e *cosa configuro*.
2. **Gli artisti non sono navigabili.** Il conteggio c'è nelle statistiche, la
   vista no. `albumKey` e la normalizzazione del dominio ci sono già.
3. **Non esiste una schermata «In riproduzione».** Il registro delle parti ne
   prevede una intera (`np-screen`, `np-art`, `np-title`, `np-meta`,
   `np-transport`, `np-scrim`), e nel vecchio albero c'era.
4. **La barra laterale non si può chiudere**, anche se `--rail-w: 68px` esiste
   apposta e la skin dichiara `"sidebar": "rail"`.

### Interazione
5. **Zero scorciatoie da tastiera.** L'unico `keydown` registrato è `Escape` in
   due modali. Manca il minimo sindacale: spazio = pausa, `/` = ricerca, frecce
   = brano precedente/successivo, `Ctrl+F`, `Esc` per chiudere pannelli.
6. **Nessuna selezione multipla**, benché `apriMenu(e, elenco: number[])` accetti
   già un array e ogni comando di coda e playlist lavori su liste. L'API è
   pronta, l'interfaccia passa sempre `[id]`.
7. **Il menù contestuale è l'unica porta** per «riproduci dopo», «accoda» e
   «aggiungi a playlist»: azioni che chi non prova il tasto destro non scopre.
8. **Il riordino dei brani in una playlist non è raggiungibile**, pur essendo
   nell'IPC. Il trascinamento nella coda non ha alcun indicatore di rilascio.
9. **`dragDropEnabled: true`** sulla finestra, ma nessun gestore: trascinare una
   cartella o un `.aeskin` sulla finestra non fa niente.

### Stato, errori, tempo
10. **Gli errori perdono tutto.** `ErroreIpc` porta `code`, `domain`,
    `severity`, `retryable`, `i18nKey` — l'interfaccia ne mostra il `message`
    in un rettangolo rosso in cima al contenuto e butta il resto. Non c'è
    distinzione fra un avviso e un guasto, non c'è «riprova», e un secondo
    errore sostituisce il primo. Il registro delle parti prevede
    `toast-card` e `toast-progress`, mai usati.
11. **Nessuno stato di caricamento.** Il contenuto compare di colpo; `skeleton`
    è una parte registrata e mai emessa.
12. **La scansione (≈20 s su 1400 brani) vive in un angolo della barra
    laterale** e non si può annullare. L'avanzamento non è annunciato a chi usa
    uno screen reader (manca `aria-live`).
13. **Nessuna virtualizzazione né paginazione visibile**: `PAGINA = 200` brani,
    400 album, e i preferiti si prendono chiedendo 2000 brani e filtrandoli nel
    browser. Non c'è modo di vedere il brano 201.

### Presentazione
14. **Le icone sono glifi Unicode misti** (`▶ ⏸ ⏮ ⏭ ⇄ ↻ ♥ ♡ ☰ ⇅ ✕ ⚙ ♪ ↦`), e
    due sono emoji a colori (`🔇 🔊`) che su Windows si disegnano in un altro
    stile e un'altra dimensione rispetto a tutto il resto. Peso ottico e
    metafore incoerenti.
15. **I caratteri dichiarati non esistono nel bundle.** `--font-sans` chiede
    `'Inter Variable'` e `--font-mono` `'Cascadia Mono'`, ma non c'è nessun
    `@font-face` e la CSP vieta i font remoti: in pratica l'app gira su
    `system-ui`. O si impacchettano i file, o si cambia la dichiarazione — la
    situazione attuale è la peggiore delle due.
16. **Nessuna risposta alla larghezza.** Le colonne dell'elenco sono in pixel
    fissi; a 880px (il minimo della finestra) titolo e artista si strozzano.
    `--content-x`, il token che esiste per questo, non è usato.
17. **Il tema chiaro è irraggiungibile** e `backgroundColor` in
    `tauri.conf.json` è fisso a `#09090d`: chi sceglie una skin chiara vede un
    fotogramma scuro a ogni avvio (difetto già noto, annotato in `skin.rs`).
18. **Il fuoco da tastiera non è stilizzato**: nessuna regola `:focus-visible`
    globale, su un'app dove `user-select: none` sta sul `body`.

### Cosa invece è già giusto — non peggiorarlo
- `aria-current` per il brano in riproduzione e per la voce di navigazione:
  una sola sorgente per chi guarda e per chi ascolta lo schermo.
- `aria-pressed` sugli interruttori (cuore, shuffle, muto, ripeti).
- Scrubber e volume come `input[type=range]` **nativi**: frecce, ruolo, valore
  annunciato, gratis.
- Aggiornamenti ottimistici su cuore e stelle, con ricarica se la scrittura
  fallisce.
- Le stelle non votate quasi invisibili finché il dito non passa sulla riga.
- Il numero di traccia che diventa ▶ al sorvolo.
- Il player che sparisce quando non c'è niente in coda, con lo spazio in fondo
  riservato solo quando serve.
- Il troncamento RTL sui percorsi (la parte che distingue due percorsi è la fine).
- Il testo dell'interfaccia non selezionabile, i titoli dei brani sì.

---

## 5. Direzione di disegno

Non partire da zero sull'identità: l'app ne ha già una, scritta nei commenti e
nella skin di serie. Vale la pena esplicitarla e portarla fino in fondo.

- **Sobrio, non decorativo.** La skin di serie si chiama *Plain* e si descrive
  come «scuro, sobrio, senza effetti: il riferimento per tutte le altre».
  L'interfaccia è cromo attorno alla musica, non spettacolo.
- **La copertina è l'unico colore che conta.** Le uniche immagini dell'app sono
  le copertine: l'impalcatura deve stare indietro e lasciarle parlare. Il
  meccanismo `dynamicAccent` esiste proprio per questo (`--hero-rgb`,
  `--accent` che seguono la tinta della copertina in riproduzione) ed è
  inutilizzato.
- **Densità alta ma respirabile.** È un'app da libreria: mille e passa righe si
  scorrono, non si contemplano. La skin dichiara `"density": "comfortable"` e
  il registro prevede che una skin possa cambiarla.
- **Palette di partenza**: fondo `#09090d`, accento viola `#8b7cf6`, ciano
  `#4fd6e0` come secondo colore della tavolozza locale. Puoi proporre una
  revisione della skin `plain` — ma come *documento skin*, non come CSS.
- **Il movimento è funzionale**: `--dur-1` per gli stati, `--dur-2` per i
  pannelli, `--dur-3` per gli overlay, `--ease-out-expo` come curva principale.
  `motion.intensity` è un valore della skin: prevedi il caso `reduced` e
  rispetta `prefers-reduced-motion`.

Puoi proporre un'identità più forte — ma se lo fai, argomentala e mostrala come
skin alternativa accanto a *Plain*, non al posto suo.

---

## 6. Cosa devi consegnare

In quest'ordine.

### A. Documento di disegno (`disegno-ux.md`)
1. **Principi** — cinque, non venti, ognuno con la conseguenza pratica.
2. **Mappa dei token**: per ogni token del registro, dove si usa nel disegno
   nuovo. I token oggi inutilizzati (`--font-dot`, `--rail-w`, `--content-x`,
   `--ambient-*`, `--glow-accent`, `--viz-*`, `--sidebar-bg`) o trovano un
   posto o vengono dichiarati fuori uso, con motivo.
3. **Mappa delle parti**: per ognuna delle 48 parti, il componente che la emette
   — o «non implementata» col perché.
4. **Scala tipografica** completa (dimensione, peso, interlinea, spaziatura
   fra lettere, e quale dei tre caratteri) e **scala di spaziatura**, con la
   proposta per `--spazio-*` se la estendi.
5. **Sistema di icone**: un insieme coerente, monocromatico, come SVG inline
   (niente librerie esterne, niente emoji, niente font di icone). Una griglia
   sola, un peso solo. Elenca ogni icona e a cosa corrisponde.
6. **Specifica del movimento**: cosa si anima, con che durata e curva, e cosa
   succede con `prefers-reduced-motion: reduce`.
7. **Mappa della tastiera** completa.
8. **Regole di risposta alla larghezza**: cosa succede a 880, 1100, 1280,
   1600+ px.
9. **Elenco di accessibilità**: ruoli, gestione del fuoco nelle modali,
   `aria-live`, contrasto minimo verificato **anche col tema chiaro** e con
   `--color-text-3`, che è il token al limite della leggibilità.

### B. Schermate, una per una
Per ognuna — impalcatura, libreria (album/brani/artisti/preferiti), album
aperto, playlist aperta, ricerca, «in riproduzione», coda, impostazioni,
riordino, importazione, gestione skin — consegna:
- il disegno (descrizione strutturale precisa + schema ASCII o SVG delle
  proporzioni);
- **tutti gli stati**: vuoto, in caricamento, popolato, errore, offline/senza
  dispositivo audio (`riproduzione.disponibile === false` è un caso reale
  gestito dal nucleo), a larghezza minima;
- le decisioni prese e le alternative scartate, in una riga ciascuna.

### C. Il codice
Modifiche reali a `apps/desktop/src/`:
- `stile.css` riorganizzato — mantenendo intatto il blocco generato e i suoi
  marcatori;
- i componenti esistenti aggiornati e quelli nuovi scritti per intero, in
  TypeScript strict (attenzione a `exactOptionalPropertyTypes`: una proprietà
  assente e una uguale a `undefined` sono cose diverse);
- **niente dipendenze nuove** senza una richiesta esplicita e argomentata (una
  libreria di virtualizzazione è probabilmente l'unico caso difendibile:
  argomentala prima di usarla);
- lo stile dei commenti del repo si mantiene: commenti in italiano che
  spiegano **perché**, non cosa. Guarda `App.tsx` e `stile.css` e scrivi come
  scrivono loro.

### D. Il piano
Ordine di esecuzione in fasi, ognuna che lascia l'app funzionante e
compilabile. La prima fase deve dare un risultato visibile.

---

## 7. Regole di ingaggio

- **Non toccare il Rust** se non per rigenerare il blocco CSS
  (`cargo run -p aether-skin --example compila`) o per un documento skin. Se il
  tuo disegno richiede un token o una parte nuova, **fermati e chiedi**: sono
  modifiche al registro, cioè al contratto, e vanno decise a parte. Elencale in
  un capitolo «richieste al registro» con, per ognuna, cosa non si può fare
  senza.
- **Non spostare regole nel dominio.** Il frontend chiede al nucleo e disegna
  quel che torna. Quali file sono musica, quando due brani sono lo stesso, cosa
  si può cancellare: sono decisioni del dominio, e ogni regola che comparisse
  qui sarebbe una regola che Android dovrà riscrivere — il modo esatto in cui
  i due alberi del vecchio progetto sono divergiti.
- **Mobile arriva dopo.** Non progettare per Android adesso, ma non incastrarti:
  dove la scelta è a costo zero, prendi quella che regge anche uno schermo
  stretto (`bottom-nav` è una parte già registrata).
- **Le prestazioni fanno parte del disegno.** Il nucleo manda quattro eventi al
  secondo apposta per non serializzarne sessanta; il pannello coda usa
  `stato.coda.join(",")` come chiave apposta per non richiedere le righe quattro
  volte al secondo. Un disegno che costringe a ridisegnare la finestra a ogni
  colpo di posizione è un disegno sbagliato, per quanto bello. Il costo degli
  effetti è dichiarato in `effects.rs` con un budget per superficie
  (`SURFACE_COST_BUDGET = 10`, cioè **un solo** `backdrop-filter`): rispettalo.
- **Verifica quel che affermi.** Prima di dire che qualcosa manca o è rotto,
  cercalo. Prima di dire che una modifica funziona, compilala
  (`npm run build:web` in `apps/desktop`, `cargo test --workspace` alla radice).

---

## 8. Come inizi

0. **Carica prima la skill `frontend-design`** (`/frontend-design`, o via lo
   strumento Skill). È la guida al disegno visivo intenzionale: direzione
   estetica, tipografia, e come non finire in scelte da template. Va letta
   prima di disegnare, non dopo.
1. Leggi in quest'ordine: `core/aether-skin/src/tokens.rs`,
   `core/aether-skin/src/parts.rs`, `core/aether-skin/skins/plain.json`,
   `apps/desktop/src/ipc.ts`, `apps/desktop/src/App.tsx`,
   `apps/desktop/src/stile.css`, poi gli altri componenti.
2. Guarda `legacy/Aeter/src/pages/` e `legacy/Aeter/src/styles/skins/`: il
   vecchio albero aveva le schermate che qui mancano (Artists, ArtistDetail,
   Stats, Settings, LyricsView, FullscreenVisualizer, EqualizerPanel,
   QueueDrawer) e due skin complete (`nothing.css`, `cyberpunk.css`) che sono
   la prova di cosa il sistema deve reggere. È **riferimento in sola lettura**:
   il nucleo nuovo ne eredita le decisioni, non il codice.
3. Leggi `CHANGELOG.md`, sezione «Non rilasciato»: dice cosa è appena stato
   costruito e cosa manca ancora.
4. **Poi fermati e presentami**: i cinque principi, la mappa delle parti e uno
   schizzo dell'impalcatura. Aspetta il via prima di scrivere codice.

Se qualcosa di questo prompt contraddice quel che trovi nel codice, **vince il
codice**: dimmelo e vai avanti.
