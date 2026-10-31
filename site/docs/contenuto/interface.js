/* Capitolo 12 — la finestra: architettura del frontend, viste, elenchi, tastiera, zoom, lingue, giro guidato, finestre di dialogo. */
(window.RYMEMBRA_DOCS = window.RYMEMBRA_DOCS || []).push({
  id: "interface",
  title: "The window (frontend)",
  lead: "React 19 + TypeScript in WebView2: how state flows from Rust into components, how pages and kept-alive views are rendered, lists without pages, the keyboard, zoom, translations, the guided tour and dialogs.",
  entries: [
    {
      id: "frontend",
      title: "Frontend architecture",
      src: ["apps/desktop/src/main.tsx", "apps/desktop/src/ipc.ts", "apps/desktop/src/ipc/errori.ts", "apps/desktop/src/ipc/libreria.ts", "apps/desktop/src/ipc/importazioni.ts", "apps/desktop/src/ipc/skin.ts", "apps/desktop/src/ipc/servizi.ts", "apps/desktop/src/dati.tsx", "apps/desktop/src/archivio.ts", "apps/desktop/src/pagine.ts", "apps/desktop/src/riproduzione.ts", "apps/desktop/src/orologio.ts", "apps/desktop/src/posizione.ts", "apps/desktop/src/parti/Scrubber.tsx", "apps/desktop/src/libreria.ts", "apps/desktop/src/ultima.ts", "apps/desktop/src/Fornitori.tsx", "apps/desktop/vite.config.ts", "apps/desktop/tsconfig.json"],
      body: [
        "**Boot** (`main.tsx`): the language is applied before the first paint; global `error`/`unhandledrejection` handlers forward to the log (`diario_annota`); the app mounts inside an error boundary (`Recinto.tsx`), with the custom title bar outside it so the window always keeps its close button. The window is created hidden and shown when the frontend calls `pronto` after applying skin and theme (with a 2 s safety net in Rust).",
        { h: "The IPC contract" },
        "`ipc.ts` is the single contract with Rust: one `ipc` object whose members are thin `invoke<T>(\"snake_case_command\", args)` calls, plus the TypeScript interfaces mirroring the serialized structs, which live by area in `ipc/` (`errori`, `libreria`, `importazioni`, `skin`, `servizi`) and are re-exported by `ipc.ts`, so callers import from there only. Errors arrive as `ErroreIpc` records ([Errors](#errors)). Events are not wrapped there: components subscribe through `Dati.ascolta` with `useAscolto(event, handler, enabled)`, which keeps the handler in a ref, subscribes once, and handles the race where `listen()` resolves after unmount.",
        { h: "Stores outside React" },
        "`archivio.ts`: `creaArchivio<T>()` returns `{ leggi, scrivi, iscrivi }`; `useArchivio(store, select, equal)` wraps `useSyncExternalStore` with memoized selectors, so only leaves that read a slice re-render — `App` is never subscribed to high-frequency state.",
        {
          ul: [
            "**Player** (`riproduzione.ts`): raw state from `riproduzione:stato`, composed views read with `useLettore(selector)`.",
            "**Playhead**: the backend sends `riproduzione:tempo` 4 times a second; `orologio.ts` anchors on the **minimum** origin over the last 24 samples (`FINESTRA`), ignoring samples older than `ETA` = 8 s (in pause the core is silent) and resetting on a jump larger than `SALTO` = 400 ms to cancel IPC delivery jitter. Between samples a timer in `riproduzione.ts` (`window.setTimeout` every `PASSO_INTERPOLAZIONE` = 50 ms, stopped while paused — not `requestAnimationFrame`, where two frames in three had nothing new to publish yet each asked the compositor for a frame) publishes the estimate, quantized to 50 ms, into `posizione.ts`. Only the lyrics (`usePosizioneMs`, every 50 ms) and the scrubber subscribe; `Scrubber.tsx` uses `usePosizioneAlPasso(passoDellaBarra(durataMs, pixel))` and redraws at the longest step that divides the second without exceeding one device pixel of travel (50 ms to 1 s), so its times change with the real second. Keyboard handlers read `posizioneAdesso()` without subscribing.",
            "**Library** (`libreria.ts`): separate stores for startup data, playlists, scan progress and results; `vinceLUltima` makes the **last requested** answer win, not the last to arrive.",
            "**Long-lived providers** (`Fornitori.tsx`) survive page changes: imports and the download queue, Drive/sync/enrichment state, the «what is playing» context.",
          ],
        },
        "Controls are pessimistic: commands return the full new state and the UI paints from that; ratings and likes are the optimistic exception. Covers are plain `<img>` with `srcset` over the `rymembra-cover` scheme, never IPC.",
        "Build: Vite targeting `chrome125` (View Transitions, `light-dark()`, `@starting-style`), strict TypeScript (`noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `verbatimModuleSyntax`). Runtime dependencies are React, ReactDOM, `@tauri-apps/api`, `@tauri-apps/plugin-dialog` and CodeMirror — no state, i18n, icon or animation library.",
      ],
      see: ["views", "lists", "ipc", "player", "together"],
    },
    {
      id: "views",
      title: "Pages, layout slots and kept-alive views",
      src: ["apps/desktop/src/App.tsx", "apps/desktop/src/pagina.ts", "apps/desktop/src/Impaginazione.tsx", "apps/desktop/src/presenza.tsx", "apps/desktop/src/transizione.ts", "apps/desktop/src/aspetto.ts", "apps/desktop/src/aspettoFinestra.ts", "apps/desktop/src/skinAttiva.ts"],
      body: [
        "`pagina.ts` has one precedence function, `paginaCorrente`: service pages (Explore, Imports, Settings), then search, an open playlist, album, artist, Home, the library view. `App.tsx` builds a header and a body for that page and passes them as **slots** to `Impaginazione`, which renders the skin's layout tree ([Layout](#layout)); widgets that need playback state subscribe themselves, everything else gets one `ContestoScafale` object. All geometry comes from compiled CSS; overlays (dialogs, menus, toasts) stay outside the tree.",
        {
          ul: [
            "**Kept alive**: the track list, favourites, albums and artists (`VISTE_TENUTE`) are mounted on first visit and then kept under React `<Activity mode=\"visible|hidden\">`: hidden views drop their effects (no listeners, no requests) but keep state, scroll position, focus and selection.",
            "**Now Playing** overlays the page, which stays mounted and `inert`. The Studio is a lazy chunk under its own `<Activity>`.",
            "**Transitions**: a view change runs in `startTransition` with the type `rotta`; parts the skin animates are wrapped in `<ViewTransition>` only when the compiled skin declares rules for them ([Motion](#motion)).",
            "**Skin and theme at runtime**: `applicaSkin` writes the compiled CSS into `<style id=\"skin-attiva\">` and sets `data-skin`, `data-density`, `data-motion`; `applicaTema` sets `data-theme`; `applicaMovimento` sets `data-motion-utente`. `skinAttiva.ts` serialises hover previews and choices that resolve out of order so the wrong skin is never painted.",
          ],
        },
        "The right column closes automatically below 1,100 px without overriding a manual choice (`terzaColonna.ts`).",
      ],
      see: ["frontend", "layout", "motion"],
    },
    {
      id: "lists",
      title: "Lists without pages",
      src: ["apps/desktop/src/virtuale.ts", "apps/desktop/src/rada.ts", "apps/desktop/src/finestra.ts", "apps/desktop/src/selezione.ts", "apps/desktop/src/selezioneVista.ts", "apps/desktop/src/fuoco.ts", "apps/desktop/src/riordino.ts", "apps/desktop/src/trascinamento.ts", "apps/desktop/src/trascinati.ts", "apps/desktop/src-tauri/src/viste.rs"],
      body: [
        "Fifty thousand tracks scroll like fifty: the scrollbar is as long as the whole list from the first frame, and only rows on screen are fetched.",
        {
          ul: [
            "**Sparse data** (`rada.ts`, `ArchivioRado`): the count is fetched first, then pages of `MISURA_PAGINA` = 200 rows by offset where the user is looking, `ANTICIPO` = 60 rows ahead; at most `CAPIENZA` = 40 pages are kept; failed pages retry after 2 s; a generation counter drops stale responses, and stale rows stay visible until replaced. Dragging to the end of the list fetches the last page and nothing in between.",
            "**Virtual window** (`virtuale.ts`, pure maths in `finestra.ts`): measures the first real row (fallback 40 px) with a `ResizeObserver`, renders spacers above and below, handles grids (`colonneDa`, `righeDiGriglia`), and restores scroll when a kept-alive view becomes visible.",
            "**Selection** (`selezione.ts`): click semantics with Ctrl/Shift, the anchor being a **view position**; «Select all» selects the whole view — `vista_brani_id` returns every id of the current view in order — not just the loaded rows.",
            "**Keyboard focus** (`fuoco.ts`): one Tab stop per list or grid (roving tabindex); ↑↓ move rows, ←→ controls within a row, PageUp/PageDown, Home/End; scroll first, then focus. Shared with the Folders tree.",
            "**Reordering** (`riordino.ts`) uses pointer events, because Tauri's file drag-and-drop swallows HTML5 drag events. Files dropped on the window arrive as `tauri://drag-drop` and are classified by `smista_trascinati` (`trascinati.ts`: a skin first, then a playlist file, then tracks and folders).",
          ],
        },
      ],
      see: ["search", "folders-view", "shortcuts"],
    },
    {
      id: "shortcuts",
      title: "Keyboard",
      src: ["apps/desktop/src/tastiera.ts", "apps/desktop/src/tasti.ts", "apps/desktop/src/modale.ts", "apps/desktop/src/parti/Scorciatoie.tsx", "core/rymembra-app/src/preferenze.rs"],
      body: [
        "`tastiera.ts` is the single shortcut table. Defaults (`DI_SERIE`):",
        {
          table: {
            head: ["Command", "Keys"],
            rows: [
              ["play / pause (`alterna`)", "Space"],
              ["search (`cerca`)", "/ , Ctrl+F"],
              ["seek ±5 s (`avanti`, `indietro`)", "→, ←"],
              ["Now Playing (`inRiproduzione`)", "F"],
              ["full screen", "F11"],
              ["zoom in / out / reset", "Ctrl++ (also Ctrl+= and Ctrl+Shift++), Ctrl+-, Ctrl+0"],
              ["Imports page, paste a link", "Ctrl+I, Ctrl+L"],
            ],
          },
        },
        "Overrides are stored as opaque JSON in `ui.shortcuts`: a missing entry means the default, an empty list means none, so new defaults appear for people who customised others. Keys are named by **character** first and physical position as a fallback (`tasti.ts::tastoDi`), so non-Latin layouts still work; the Windows key is ignored. Shortcuts do not fire while typing in a field unless Ctrl/Alt or an F-key is held; with a modal open only zoom and full screen pass; **Esc** is fixed — it is the exit from every dialog and field. Assigning the same key twice is reported as a conflict, not refused. The keys inside lists, the queue, the Folders tree and the grids are fixed and listed at the bottom of the card: arrows, Ctrl+A (select the whole view), Space (toggle selection), Enter (play), PageUp/PageDown, Home/End, Alt+↑/↓ (move the item, where reordering applies).",
      ],
      see: ["lists", "zoom", "dialogs"],
    },
    {
      id: "zoom",
      title: "Zoom",
      src: ["apps/desktop/src-tauri/src/zoom.rs", "core/rymembra-app/src/preferenze.rs"],
      body: [
        "The whole window zooms over seven steps, `SCALA_ZOOM` = 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, using the **WebView's** zoom (`WebviewWindow::set_zoom`), not a CSS `zoom` or a scale token. With the browser zoom the CSS pixel stays the unit the page reasons in, so the two modules that measure the DOM — the virtual list and the tour spotlight — keep working; under CSS `zoom`, `getBoundingClientRect` and `scrollTop` would live in different coordinate spaces.",
        "Commands `zoom_passo(su)`, `zoom_normale`, `zoom_avvio`; the window shows the value the core answered. The step is stored in `ui.zoom` (an unknown value falls back to 1.0) and does **not** travel in a profile: it is the correction left after Windows' own scaling on this monitor.",
      ],
      see: ["shortcuts"],
    },
    {
      id: "language",
      title: "Translations",
      src: ["apps/desktop/src/lingue/index.ts", "apps/desktop/src/lingue/plurali.ts", "apps/desktop/src/lingue/Trans.tsx", "apps/desktop/src/lingue/it.json", "apps/desktop/src/lingue/en.json", "strumenti/lingue.js"],
      body: [
        "Languages are the files in `lingue/`, discovered with `import.meta.glob`: adding one is adding a JSON file (its display name in the reserved key `_nome`). `it.json` is the reference — `Chiave = keyof typeof it`, so an unknown key is a compile error — with 2,734 flat dotted keys.",
        {
          ul: [
            "**Choice**: saved `ui.language`, else the system language reduced to its root (`de-DE` → `de`), else English (`RIPIEGO` = `en`). A key missing in a language falls back to English, not to the key; a missing key in dev renders the key and warns.",
            "**`t(key, values)`**: `{name}` placeholders, numbers formatted with `toLocaleString`; a numeric `n` selects the plural: `key.uno` when n = 1, then the CLDR category (`Intl.PluralRules`), then `key.other`, then the bare key — never mixing languages in one sentence. `tSe` resolves runtime keys (error `i18nKey`s). `<Trans>` lets placeholders be React nodes so translators see the whole sentence.",
            "**Checks**: `strumenti/lingue.js` compares every language with `it.json` both ways, checks plural forms per CLDR category, and fails if an error code of the catalog lacks its `errors.<code>` sentence.",
          ],
        },
        "The language is stored in the core (`ui.language`, ISO code checked for shape only), so backup and profile carry it.",
      ],
      see: ["errors", "tooling"],
    },
    {
      id: "tour",
      title: "The guided tour",
      src: ["apps/desktop/src/Giro.tsx", "apps/desktop/src/guida.ts", "apps/desktop/src-tauri/src/giro.rs", "apps/desktop/src/App.tsx"],
      body: [
        "A welcome and 13 spotlights on the **real** controls. Each step finds its anchor (`data-giro` attributes, plus a few documented selectors), cuts a hole in a veil with one `box-shadow: 0 0 0 9999px` element and places the bubble beside it (`posaFumetto`, 12 px away). A step whose anchor does not appear within `ATTESA_ANCORA_MS` = 1.5 s is skipped rather than faked. To show the player it fills the queue without playing (`coda_prepara`); at the end the view and the queue are put back (`codaDaSvuotare` removes the queue it created if nobody used it).",
        "`comeParteIlGiro` decides `parti` / `aspetta` / `no`: it starts the first time there is something to listen to, waits while any dialog or menu (`aria-modal`) or the Studio is open, and counts itself as a modal. The core stores only the script version seen (`ui.tour.seenVersion`, compared with `VERSIONE_COPIONE` = \"3.0.0\" using `!=`), which travels in a profile; «Redo the tour» in Settings starts it again.",
      ],
      see: ["dialogs"],
    },
    {
      id: "dialogs",
      title: "Dialogs and accessibility",
      src: ["apps/desktop/src/Dialogo.tsx", "apps/desktop/src/modale.ts", "apps/desktop/src/finestrella.ts", "apps/desktop/src/parti/AnnuncioScansione.tsx", "apps/desktop/src/parti/Avvisi.tsx"],
      body: [
        "`Dialogo.tsx` is the shared host: `role=\"dialog\"` or `\"alertdialog\"`, `aria-modal`, labelled by its `<h2>`. `finestrella.ts` provides `useFinestrella` (initial focus on `data-fuoco-iniziale`, a Tab trap that recomputes focusable elements on every press, Esc, focus restored on close, modal counting) and `useVeloInerte`, which makes the rest of the window `inert`. The pure rules in `modale.ts` are tested without a DOM: the backdrop closes only on a press **and** release on itself (a drag that starts inside a field and ends outside does not close it), which shortcuts may pass while a modal is open, and initial focus.",
        "Scans announce start and end through an always-mounted live region; error banners use `role=\"alert\"` for blocking errors and `role=\"status\"` otherwise. Lists are `role=\"grid\"` with `aria-rowindex`, the Folders tree flattened `treeitem`s with `aria-level`/`aria-posinset`/`aria-setsize`. Reduced motion is honoured as described in [Motion](#motion).",
      ],
      see: ["shortcuts", "motion"],
    },
  ],
});
