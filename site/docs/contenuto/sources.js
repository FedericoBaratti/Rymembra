/* Capitolo 7 — cataloghi e importazioni: licenze, Esplora, link, archivio Spotify, file di playlist, coda dei download, 1.0. */
(window.RYMEMBRA_DOCS = window.RYMEMBRA_DOCS || []).push({
  id: "sources",
  title: "Catalogs and imports",
  lead: "Where music comes from besides your disk: the free catalogs and their licences, Explore, imports from links, the Spotify archive and playlist files, the download queue, and the import from version 1.0.",
  entries: [
    {
      id: "catalogs",
      title: "Free catalogs and licences",
      src: ["core/rymembra-catalogo/src/lib.rs", "core/rymembra-catalogo/src/riferimento.rs", "core/rymembra-catalogo/src/archivio_org.rs", "core/rymembra-catalogo/src/audius.rs", "core/rymembra-catalogo/src/jamendo.rs", "core/rymembra-domain/src/esterno.rs", "core/rymembra-domain/src/scelta.rs"],
      body: [
        "`rymembra-catalogo` reads the Internet Archive and Audius (Jamendo is implemented behind the `jamendo` Cargo feature, **off** by default, pending a written answer on whether donations count as commercial use). Everything goes through `rymembra-net`: no child processes, no bundled binaries. Every result carries a `Licenza` and a `Disponibilita`, and `prelievo::preleva` refuses to write what the second does not allow — **before** any request.",
        {
          table: {
            head: ["Licenza", "Copy allowed"],
            rows: [
              ["`PubblicoDominio`", "yes"],
              ["`CreativeCommons(code)`", "yes (attribution written in tags)"],
              ["`OpenMusicLicense` (Audius)", "yes"],
              ["`LiberaNonCommerciale` (Live Music Archive)", "yes, non-commercial"],
              ["`TutteRiservate`, `Sconosciuta`", "no"],
            ],
          },
        },
        "`Disponibilita::decidi(fonte, licenza, consegna_concessa)`: a source that can deliver and a licence that permits copying → `Scaricabile`; deliverable but not copyable (or the artist disabled downloads) → `SoloAscolto`; a source that only names tracks (a Spotify archive, a playlist file) → `SoloAcquisto` (Jamendo → `SoloAscolto`, its terms forbid offline copies).",
        {
          ul: [
            "**Internet Archive**: collections `etree`, `netlabels`, `audio_music`, `78rpm`, sorted by downloads; 8 items found, 4 opened per search; formats preferred FLAC > 24-bit FLAC > VBR MP3 > MP3 for keeping, the reverse for listening.",
            "**Audius**: a content node from `https://api.audius.co`, one per session; `app_name=Rymembra`; up to 8 results, 200 tracks per collection. Stream-gated tracks are excluded, download-gated ones become listen-only; permission requires the download switch **and** no gate.",
            "**Recognising links** (`riferimento::riconosci`, offline): archive.org `details|download|metadata|embed`, audius.co/.org, jamendo.com (with the feature); exact domain or subdomain, so `evilarchive.org` is refused.",
          ],
        },
        "When a catalog is searched for a named track (the download queue), `scelta::scegli_candidato` uses duration as a gate (±`BANDA_STRETTA_MS` = 30 s, widened to 60 s for a second query), then ranks by nature (`Studio` > `DalVivo` > `Alternativa`, which demotes but never discards) and by how much the uploader can be trusted (the artist's own name, verified, unknown). `catalogo.alternative` (default on) accepts live or alternative recordings, and the report says which one was taken.",
      ],
      see: ["explore", "downloads", "streaming"],
    },
    {
      id: "explore",
      title: "Explore",
      src: ["apps/desktop/src-tauri/src/esplora.rs", "core/rymembra-catalogo/src/lib.rs", "core/rymembra-app/src/catalogo.rs", "apps/desktop/src/schermate/Esplora.tsx"],
      body: [
        "A free-text search across the catalogs (`esplora_cerca`, at most `FRASE_MASSIMA` = 250 characters). It runs on **Enter**, not per keystroke: one search is 1–5 requests to the Archive and one to Audius (0.3–3.5 s measured) against public services that host us for free. `esplora_annulla` sets a flag checked between requests. If no catalog answers, the error is `download.externalSearchFailed`, distinct from «no results».",
        "Results stay **in the backend** (`StatoEsplora.ultima`); the window sends back only the URL of the result it wants. Licence and availability are therefore never taken from the window, where a crafted IPC call could forge them.",
        {
          table: {
            head: ["Command", "Effect"],
            rows: [
              ["`esplora_aggiungi` / `_tutti`", "Adds a **reference**: a `tracks` row with `path = NULL`, `source_service`, `fonte_url`, `fonte_pagina`, licence and availability (`catalogo::aggiungi`; `SoloAcquisto` refused). It plays as a stream, is searchable, rateable, sits under its album and artist"],
              ["`esplora_tieni`", "Queues a **file** download in `desiderati` with the exact URL, re-resolved by `Cataloghi::per_tenere` to the best keepable format (on the Archive the original FLAC rather than the MP3 that was streamed). Does not start the queue"],
              ["`esplora_nella_lista`", "Records the track as `introvabile`, i.e. on the shopping list"],
              ["`esplora_togli`, `esplora_identificativo`, `esplora_apri_pagina`", "Remove a reference; the library row of a result, to play it; open the public page"],
            ],
          },
        },
        "Every write goes through `nuvola::se_riuscito`, so the backup notices. Explore's catalog clients are separate from the download queue's and the importer's, so a long download never delays a search.",
      ],
      see: ["catalogs", "streaming", "downloads"],
    },
    {
      id: "import",
      title: "Importing: links, the Spotify archive and playlist files",
      src: ["core/rymembra-app/src/import_esterno.rs", "core/rymembra-app/src/import_account.rs", "core/rymembra-archivio/src/lib.rs", "core/rymembra-domain/src/abbinamento.rs", "core/rymembra-domain/src/spotify_account.rs", "core/rymembra-app/src/import_playlist.rs", "core/rymembra-domain/src/playlist_file.rs", "apps/desktop/src-tauri/src/importa.rs", "apps/desktop/src-tauri/src/account.rs"],
      body: [
        "All imports follow one pattern: resolve outside the database, then **plan = the real import inside a transaction that is rolled back**, then the same code commits. The plan cannot promise one thing and do another.",
        { h: "Matching against the library" },
        "`rymembra_domain::abbinamento::plan_abbinamento` matches each external track with a ladder (`Gradino`), building one `Indice` reused for every playlist:",
        {
          ol: [
            "`Isrc` — 12-character normalized ISRC, no duration check;",
            "`ChiaveEsatta` — the full `TrackKey`;",
            "`ArtistaTitolo` — artist + title, duration within `TOLLERANZA_MS` = 10 s;",
            "`Ripulito` — first credited artist + title without decorations («feat.», edition words), duration-guarded.",
          ],
        },
        "Ambiguity reports the track **missing** rather than guessing. The Spotify archive has no durations, so `Indice::durata` borrows the library's.",
        { h: "From a link" },
        "`import_anteprima` resolves an archive.org or audius.co link into a `ContenutoEsterno` (pages of tracks, with `import:avanzamento`), `import_piano` shows the plan, `import_esegui` writes: create or replace the playlist (a truncated list cannot replace an existing one), fill it with matched tracks, write identifiers (ISRC, Spotify album id), put missing tracks into `desiderati` (state `attesa`, with licence and availability), reconcile, rebuild aggregates if album ids changed, and store the report in `import_reports` — one transaction.",
        { h: "The Spotify archive" },
        "The GDPR zip Spotify sends on request is read by `rymembra_archivio::leggi`: files are recognised by **base-name prefix** (Spotify renamed them at least three times) — `streaming_history_audio*`, `streaminghistory*`, `endsong*` → history (format detected per row: short `endTime` or extended `ts`), `playlist*`, `yourlibrary*`, `identity*`/`userdata*`; video history and podcasts are skipped and counted. Each entry is read up to 200 MiB (counted on bytes actually read); one malformed file does not stop the rest.",
        "`import_account` replaces playlists, marks liked songs (queuing the missing ones), links albums and artists, and imports history: start = end − `ms_played`, counted with the same listen rule ([Listens](#listens)), inserted with `NOT EXISTS (track_id, played_at)`, statistics merged by `merge_stats` (max), so importing twice changes nothing. History never creates wanted tracks. Imported plays keep `source = 'spotify'` and can be removed with `cronologia_dimentica_importati`. Nothing is ever deleted.",
        { h: "Playlist files" },
        "`playlist_file` reads and writes M3U/M3U8, PLS and XSPF; unreadable lines are counted, not fatal. Import matches by path first, then the ladder; missing entries are listed by path, never queued for download. Re-importing replaces the playlist. Export writes **absolute** paths (a library spans several disks) in the chosen format; the destination is validated by `destinazioni` ([Security](#security)).",
      ],
      see: ["downloads", "listens", "identity"],
    },
    {
      id: "downloads",
      title: "The download queue and folder",
      src: ["core/rymembra-app/src/desiderati.rs", "apps/desktop/src-tauri/src/procura.rs", "core/rymembra-catalogo/src/prelievo.rs", "core/rymembra-app/src/tag_scrittura.rs", "core/rymembra-domain/src/destinazione.rs", "apps/desktop/src/parti/Importazioni.tsx"],
      body: [
        "`desiderati` holds wanted tracks: imported but missing, liked on Spotify but missing, or chosen with «Keep» in Explore. State machine: `attesa` → `fatto` | `fallito` (retryable, at most `MASSIMI_TENTATIVI` = 3; a non-retryable error jumps to the cap) | `introvabile` (no lawful source has it: it goes on the shopping list). The worker is called **procurement** (`procura`) because downloading is only one of three outcomes.",
        { h: "The worker" },
        "`scarica_desiderati` starts thread `rymembra-procura`: batches of `LOTTO` = 24, `FILI` = 3 in parallel (one leaves the line idle, ten earn a 429). The library lock is held only to read a batch and to record outcomes — never while a FLAC concert downloads. Per row: search the catalogs, choose with `scegli_candidato`, check `Disponibilita`, then `prelievo::preleva`:",
        {
          ol: [
            "write into a temporary folder **outside** the watched roots (a scan must never see a half-written file);",
            "the relative destination comes from `destinazione`: Windows-forbidden characters replaced, the 22 reserved names (`CON`, `NUL`, `COM1`…) avoided, segments at most 120 characters;",
            "move into the download folder with `rename` (copy + delete across volumes);",
            "`tag_scrittura::scrivi_tag` writes title, artist, album, album artist (falling back to the artist), track/disc, year, cover (≤ 8 MiB) and, in the comment field, the licence attribution — the file outlives the app, and that line is what says under which terms it is there.",
          ],
        },
        "Events: `scarico:brano` (per track: title, outcome, recording nature), `scarico:avanzamento`, `scarico:guasto`, `scarico:finito`, `scarico:riconciliato`, `scarico:in_libreria`. When the queue ends, an automatic **prudent** scan of the download folder (only the part inside the watched roots) runs (waiting its turn rather than giving up), then `desiderati::riconcilia` puts arrived tracks into their playlists at their original positions (`INSERT OR IGNORE`, `placed_at`; idempotent).",
        { h: "Where files land" },
        "`download.folder`, by default the **first watched folder** — a file downloaded outside every watched root is one no scan ever finds. Settings warns when the chosen folder is not under a root. `annulla_scarico` stops between tracks; `riprova_falliti` re-queues failures; `da_comprare` lists `introvabile` rows; `cerca_dove_comprare` opens a search on Bandcamp, Qobuz or Discogs.",
      ],
      see: ["catalogs", "import", "roots"],
    },
    {
      id: "legacy",
      title: "Importing from version 1.0",
      src: ["core/rymembra-app/src/import_legacy.rs", "core/rymembra-domain/src/merge.rs", "apps/desktop/src-tauri/src/comandi.rs", "apps/desktop/src/Importa.tsx"],
      body: [
        "The released 1.0 app kept its database in its own data folder, a different one from this app's, so it stays usable side by side. A scan rebuilds tags, covers and albums; four things exist only in that database — **play counts, ratings, likes and playlists** — plus history and tombstones, and `import_legacy` brings them over.",
        {
          ul: [
            "The old database is opened `mode=ro`: nothing is written into it, not even an «already imported» mark.",
            "Matching is by `TrackKey` computed from tags on both sides, **not** by path: on the reference library none of the 1,707 old paths still existed after reorganising.",
            "Idempotent: statistics merge with `merge_stats` (play count = max, newest rating wins, likes by decision time) and history is inserted only if `(track_id, played_at)` is new — importing twice doubles nothing.",
            "`piano_importazione` shows the plan (a real run rolled back); `importa` commits.",
          ],
        },
      ],
      see: ["import", "sync"],
    },
  ],
});
