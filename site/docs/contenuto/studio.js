/* Capitolo 11 — lo Skin Studio: lo spazio di lavoro, l'anteprima sulla libreria finta, la chat. */
(window.RYMEMBRA_DOCS = window.RYMEMBRA_DOCS || []).push({
  id: "studio-chapter",
  title: "Skin Studio",
  lead: "The built-in skin editor: why it edits text, how it validates while you type, how it previews the real pages on a sample library, and how the chat changes a skin.",
  entries: [
    {
      id: "studio",
      title: "How the Studio works",
      src: ["apps/desktop/src/studio/Guscio.tsx", "apps/desktop/src/studio/negozio.ts", "apps/desktop/src/studio/patch.ts", "apps/desktop/src/studio/storia.ts", "apps/desktop/src/studio/mittente.ts", "apps/desktop/src/studio/codice.ts", "apps/desktop/src/studio/riferimenti.ts", "apps/desktop/src-tauri/src/studio.rs", "apps/desktop/src-tauri/src/studio/registro.rs", "apps/desktop/src-tauri/src/studio/validazione.rs"],
      body: [
        "Three rules from `Guscio.tsx`: you cannot write what the format refuses (every control has the shape of a core type; the only free-form place is the code editor, where the parser answers); the preview **is** the application; cost is visible while you spend it. The Studio is a lazily loaded chunk (`lazy(() => import(\"./studio/Guscio\"))`), mounted under `<Activity>`.",
        { h: "Text is the source of truth" },
        "`SkinDocument` cannot be serialized back (a validated colour is four channels; writing `rgb(9 9 13)` would produce input the parser refuses), so the Studio edits the **JSON text**. Controls produce path-based edits through `patch.ts` (`scrivi`, `togliDa`, `rinominaChiave`, `applicaOperazioni`), which preserve key order so diffs stay small and refuse `__proto__`. The draft you save is the file you would put in git.",
        {
          ul: [
            "**State**: a `useReducer` (`negozio.ts`) split across two contexts — state, which changes on every keystroke, and stable actions — so controls do not re-render needlessly.",
            "**Undo** (`storia.ts`): text snapshots, `PROFONDITA` = 200 steps, edits to the same path grouped while less than `PAUSA_MS` = 500 ms apart and up to `DURATA_RAFFICA_MS` = 2 s; each step is labelled. CodeMirror's own undo is routed to this history.",
            "**Validation loop** (`mittente.ts`): 120 ms after the last edit, `studio_valida(text, draft)` runs in the core; one request in flight, only the latest queued. It never fails: it returns errors and warnings with positions, costs, contrast pairs, the layout and the CSS compiled for the canvas scope. While the text is broken the canvas keeps the last valid sheet.",
            "**Code editor** (`EditorCodice.tsx`, `codice.ts`): CodeMirror 6 with completion from the registry by path (keys and allowed values), diagnostics from the core's line/column positions, and external edits applied as minimal diffs.",
            "**References** (`riferimenti.ts`): who uses which palette entry, pattern, token, animation or easing; renaming rewrites the declaration and every reference.",
          ],
        },
        { h: "Panels" },
        "Left: parts by group with the layout tree (`Livelli.tsx`, `AlberoScafale.tsx`), or the variables (palette, tokens, patterns, easings, animations, fonts, images, snapshots). Centre: the canvas ([preview](#studio-preview)). Right: the inspector — style, states, motion, code of the selection, with its cost (`IspezioneParte.tsx`). Bottom dock: the keyframe timeline (played with the Web Animations API, using the same keyframe rules as `compile.rs`), the problem list with «go» and «fix», and the code. A palette generator builds a legible palette from one colour or the playing cover ([Accent](#accent)).",
        { h: "Drafts on disk" },
        "`studio.rs` keeps drafts in `<data>/skin/bozze/<id>/` (manifest, `assets/`, `chat.json` ≤ 2 MB) and up to 20 snapshots per draft in `skin/istantanee/<id>/`. Commands: `studio_registro` (the whole vocabulary, once), `studio_valida`, `studio_documento`, `studio_salva`, `studio_scarta`, `studio_esporta` (writes a `.ryskin`), `studio_istantanee` / `_istantanea` / `_ripristina`, `studio_risorsa_aggiungi` / `_togli`, `studio_migra` (format 1 → 2), `studio_crea` (new draft; ids are 2–48 characters `[a-z0-9-]` starting with a letter), `studio_compila_app`, `studio_tavolozza_da`, `studio_chat_leggi` / `_scrivi`.",
      ],
      see: ["studio-preview", "studio-chat", "skin"],
    },
    {
      id: "studio-preview",
      title: "Real pages on a sample library",
      src: ["apps/desktop/src/studio/Tela.tsx", "apps/desktop/src/studio/mondo.ts", "apps/desktop/src/studio/mondoImpostazioni.ts", "apps/desktop/src/studio/scena.ts", "apps/desktop/src/studio/Centro.tsx", "apps/desktop/src/studio/PillolaProva.tsx", "apps/desktop/src/skinAttiva.ts", "apps/desktop/src/dati.tsx"],
      body: [
        "The canvas (`Tela.tsx`) is an `<iframe>`: the app's own stylesheets are copied in **except** the active skin (`#skin-attiva`), and the draft's compiled sheet is inserted as `#bozza`. The real components — layout, Home, album grid, track list, Folders, Explore, Now Playing, Artists, the Appearance section of Settings, menus, the EQ panel, first run, toasts — are rendered into it with `createPortal`.",
        "They read data through the `Dati` context ([Frontend](#frontend)); the Studio supplies `mondo.ts`: a fake library (36 tracks, albums, artists, Home shelves, a weekly collection, Explore results), a fake player state and position, and `datiFinti()` answering the read-only IPC subset. **Your library is never shown.** Native interaction in the iframe is blocked by a capture-phase listener (click, pointer, key, input, submit, drop…), so nothing in the preview can act.",
        {
          ul: [
            "`scena.ts` decides which page and overlays are on the canvas; its `DOVE` table maps each part to a scene where it is visible («Show me»), tested in `test/scena.test.ts`.",
            "Three widths (`LARGHEZZE`: three-column, two-column, narrow) at a height of 820 px; zoom, a probe that names the part under the pointer, any state forced on the selected part (`tela` scope), dark/light variant toggle and a «playing» toggle.",
            "**Try on the app**: `studio_compila_app` compiles with the `prova` scope and `skinAttiva.iniziaProva` dresses the real window, hiding the Studio under `<Activity>`; `PillolaProva` offers «Back» (Esc) or «Keep» (install).",
            "A snapshot of the canvas (`fotografia.ts`, SVG `foreignObject` into a canvas, data URIs only) can be sent to a vision model in the chat.",
          ],
        },
      ],
      see: ["studio", "frontend"],
    },
    {
      id: "studio-chat",
      title: "The Studio chat",
      src: ["apps/desktop/src/studio/Chat.tsx", "apps/desktop/src/studio/prompt.ts", "apps/desktop/src/studio/conversazione.ts", "apps/desktop/src/studio/tenace.ts", "core/rymembra-ia/src/operazioni.rs", "apps/desktop/src-tauri/src/ia.rs"],
      body: [
        "It works because the format already has what a model needs: a **closed, declared** vocabulary (73 tokens, 101 parts, 12 effects, each with a valid example), text edited by path, and a validator that never fails and suggests the nearest name for every typo.",
        {
          ul: [
            "**Prompt** (`prompt.ts`, in English): a contract built from the registry (names only, no descriptions), the selection («make this rounder» means the selected part, state and theme), a light version of the document and of the history (each capped at 12,000 characters to fit small local contexts), and the current validation errors.",
            "**Answer**: prose plus a fenced block tagged `rymembra-patch` with JSON operations — `scrivi` (set a value at a path, creating what is missing) and `togli` (remove it); JSON Patch spellings such as `path`, `value`, `replace` are accepted because models write them — extracted by `rymembra_ia::operazioni` (≤ 12 operations, reasons ≤ 120 characters).",
            "**Proposal** mode shows the values that would change and waits for «Apply». **Agent** mode (default) applies, re-validates with `studio_valida`, and sends the errors back for correction — at most `GIRI_MASSIMI` = 4 rounds, with Stop; warnings do not trigger a retry; a snapshot is taken before the first round.",
            "Every change goes through the same path as a dragged slider, so Ctrl+Z undoes it. Each answer lists the values it changed, each a link to the thing it names.",
          ],
        },
        "The conversation is saved next to the draft (`conversazione.ts`: version 1, ≤ 1.5 MB, the last 30 changes kept in view, images never saved) with the tokens each turn cost. `tenace.ts` keeps the `ia:*` listeners alive while the Studio is hidden under `<Activity>` and queues events until it is visible again. Provider, keys and what is sent: [Language models](#models).",
      ],
      see: ["models", "studio"],
    },
  ],
});
