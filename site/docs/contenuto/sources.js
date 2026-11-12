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
        "`rymembra-catalogo` reads the Internet Archive and Audius (Jamendo is implemented behind the `jamendo` Cargo feature, **off** by default, pending a written answer on whether donations count as commercial use). Everything goes through `rymembra-net`: no child processes, no bundled binaries. Every result carries a `Licenza` and a `Disponibilita`, and `Cataloghi::preleva` refuses to write what the two together do not allow (`Candidato::si_puo_tenere`: `Scaricabile` **and** a licence that permits copying), or anything that is not at an address of the catalog it claims — **before** any request.",
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
            "**Internet Archive**: collections `etree`, `netlabels`, `audio_music`, `78rpm`, sorted by downloads; 8 items found, 4 opened per search; formats preferred FLAC > 24-bit FLAC > VBR MP3 > MP3 for keeping, the reverse for listening. An item is listen-only when it is in the `stream_only` collection, or when `access-restricted-item`, `access-restricted` or `stream_only` say anything but an explicit no (a list, an object, `null`, an unknown word all count as a prohibition; the same reader decides a file's `private`). A `licenseurl` counts only on `creativecommons.org` (the public-domain mark, CC0, or one of the six CC BY codes); an unrecognised one is `Sconosciuta` and never falls back to the collection. 78 rpm items (`78rpm`, `78rpm_…`, `great78…`, `georgeblood`) get no licence from the collection: listen-only unless the item declares one.",
            "**Audius**: a content node from `https://api.audius.co`, one per session — the first one that is `https` to a **public name** (`indirizzo_pubblico`), never an IP literal or a home-network name; artwork URLs pass the same check, and redirects of API requests and of the queue's cover download are followed only towards public names (`rimandi_pubblici`); `app_name=Rymembra`; up to 8 results, 200 tracks per collection. Stream-gated tracks are excluded (`is_stream_gated` not `false`, a `stream_conditions`, or `access.stream` denied), download-gated ones become listen-only. Permission needs every no to be said: `is_downloadable: true`, `is_download_gated` **and** `is_stream_gated` written as `false`, no `download_conditions`/`stream_conditions`, and `access.download: true` when `access` is sent; a missing or unexpected value never grants a copy.",
            "**Recognising links** (`riferimento::riconosci`, offline): archive.org `details|download|metadata|embed`, audius.co/.org, jamendo.com (with the feature); exact domain or subdomain, so `evilarchive.org` is refused.",
          ],
        },
        "When a catalog is searched for a named track (the download queue), `scelta::scegli_candidato` first requires title **and** artist to match (`scelta::combacia`: brackets, edition tails and «feat.» removed, a trailing year only next to an edition word, `normalize_key` folding, a leading «the» dropped, `&` read as «and» in titles and names, one typo per ten characters but **numbers must match exactly** — «Pt. 1» is not «Pt. 2», «No. 1» is not «No. 10», «II» is not «I» — never mere containment). The artist is compared as the whole name or its main artist (cut at a comma, «feat.», «ft.», «with», «vs», never at `&`), plus «Surname, Name» turned round; a guest or a piece of a duo's name is not the artist. A candidate or a wanted track without an artist does not pass and stays on the shopping list. Among those that pass, the exact title comes first; then duration is a gate (±`BANDA_STRETTA_MS` = 30 s, widened to 60 s for a second query), then ranks by nature (`Studio` > `DalVivo` > `Alternativa`, which demotes but never discards; on the Archive outside `etree` the item title counts too) and by how much the uploader can be trusted (the artist's own name, verified, unknown). `catalogo.alternative` (default on) accepts a live or a reissue **by the same artist**, and the report says which one was taken; a cover by someone else is never an answer.",
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
        "`desiderati` holds wanted tracks: imported but missing, liked on Spotify but missing, or chosen with «Keep» in Explore. State machine: `attesa` → `fatto` | `fallito` (retryable, at most `MASSIMI_TENTATIVI` = 3; a non-retryable error jumps to the cap) | `introvabile` (no lawful source has it: it goes on the shopping list). The worker is called **procurement** (`procura`) because downloading is only one of three outcomes. A track counts as already there only as a **file** (`tracks.path IS NOT NULL`): a catalog reference added by «Listen» in Explore does not close a «Keep» row.",
        { h: "The worker" },
        "`scarica_desiderati` starts thread `rymembra-procura`: batches of `LOTTO` = 24, `FILI` = 3 in parallel (one leaves the line idle, ten earn a 429). The library lock is held only to read a batch and to record outcomes — never while a FLAC concert downloads. Per row: a file already chosen (a link import, «Keep» in Explore) is re-read with `Cataloghi::da_prelevare` — licence, permission and page come from the catalog now, not from the row — and `strada_rapida` takes it only if it is the row's track (`combacia`, so a forged profile row can't name one file after another track) and can be kept; otherwise, and also when the artist has switched downloads off, search the catalogs and choose with `scegli_candidato`. If nothing turns up, a re-read file that can't be kept is still the answer, and the row goes on the shopping list as `download.notPermitted`. A cancelled search is `internal.aborted` (the row stays queued), and an empty choice while a catalog — or one of the Archive's item pages — did not answer is a failure retried as that error says, not `introvabile`. Then `Cataloghi::preleva`:",
        {
          ol: [
            "only from an address of the declared catalog: an `archive.org/download/…` file recognised by `riconosci`, or Audius' path `/v1/tracks/<id>/download` with the node chosen now; redirects are followed one at a time through `bersaglio_ammesso` (`download.unrecognizedUrl` otherwise, before any request);",
            "write into a temporary folder **outside** the watched roots (a scan must never see a half-written file);",
            "check the first bytes (`formato_dei_byte`, skipping an ID3v2 tag): FLAC, MP3, AAC (ADTS), Ogg, WAV, AIFF or MP4, otherwise `download.invalidFiles`, not retried; the extension comes from the content, never from the catalog's metadata;",
            "the relative destination comes from `destinazione`: Windows-forbidden characters replaced, the 32 reserved names (`CON`, `NUL`, `COM0`–`COM9`, `COM¹²³`, `LPT0`–`LPT9`, `LPT¹²³`, `CONIN$`, `CONOUT$`) avoided by marking the root before the first dot, segments at most 120 characters;",
            "move into the download folder with `rename` (copy + delete across volumes);",
            "`tag_scrittura::scrivi_tag` writes title, artist, album, album artist (falling back to the artist), track/disc, year, cover (≤ 8 MiB) and, in the comment field, the licence attribution (plus `LICENSE` in Vorbis comments and MP4, and the page as `WOAS` in ID3v2) — the file outlives the app, and that line is what says under which terms it is there. Only the file the queue has just written is ever opened for writing.",
          ],
        },
        "Events: `scarico:brano` (per track: title, outcome, recording nature), `scarico:avanzamento`, `scarico:guasto`, `scarico:finito`, `scarico:riconciliato`, `scarico:in_libreria`. When the queue ends, an automatic **prudent** scan of the download folder (only the part inside the watched roots) runs (waiting its turn rather than giving up), then `desiderati::riconcilia` puts arrived tracks into their playlists at their original positions (`INSERT OR IGNORE`, `placed_at`; idempotent) and marks the downloaded files `source = 'catalogo'`. A file row cannot hold a licence (the 026 `CHECK`), so `library::read_summary` — the single-track read behind Now Playing — takes it from the `desiderati` row whose `download_path` is that file (most recent first); list queries do not, because `download_path` has no index. The source page stays in the file's tags only.",
        { h: "Where files land" },
        "`download.folder`, by default the **first watched folder** — a file downloaded outside every watched root is one no scan ever finds. Settings warns when the chosen folder is not under a root. `annulla_scarico` stops between tracks; `riprova_falliti` re-queues failures; `riprova_introvabili` («Also retry the ones not found») re-queues the `introvabile` rows too; `da_comprare` lists `introvabile` rows with the bare error code as their reason; `cerca_dove_comprare` opens a search on Bandcamp, Qobuz or Discogs.",
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
