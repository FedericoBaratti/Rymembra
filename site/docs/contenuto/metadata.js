/* Capitolo 4 — metadati e testi: arricchimento, correzioni, testi, sincronizzazione a mano. */
(window.RYMEMBRA_DOCS = window.RYMEMBRA_DOCS || []).push({
  id: "metadata",
  title: "Metadata and lyrics",
  lead: "Finding missing metadata online without touching your files, how corrections layer over tags, and where lyrics come from and how they are timed.",
  entries: [
    {
      id: "enrich",
      title: "Enrichment",
      src: ["core/rymembra-domain/src/enrich.rs", "core/rymembra-app/src/enrich.rs", "apps/desktop/src-tauri/src/arricchimento.rs", "core/rymembra-meta/src/lib.rs", "core/rymembra-meta/src/musicbrainz.rs", "core/rymembra-meta/src/copertine.rs", "core/rymembra-meta/src/itunes.rs", "core/rymembra-meta/src/deezer.rs"],
      body: [
        "Enrichment looks up albums whose tags are poor and writes what it proves **into the library only** — the `tracks` columns, plus a record of which fields it set in `track_meta_arricchita`. Since 2.3.1 the music file is never opened for writing, so its mtime never changes and the next scan does not see it as modified.",
        { h: "Who is a candidate" },
        "A file row (`path IS NOT NULL`) with no `mb_recording_id`, and at least one of: artist «Artista sconosciuto», album «Album sconosciuto», no cover, or `source = 'catalogo'`; and whose `enrich_status` is NULL, or `needs-review`/`error` older than `RITENTA_INCERTI_MS` = 7 days, or `no-match` older than `RITENTA_NESSUNO_MS` = 30 days. `ok` and `undone` are final. Candidates are grouped by `album_key`, most recently added first, `LOTTO` = 12 groups per pass; each group is read **whole**, because the track count is evidence.",
        { h: "Phases" },
        { code: "candidati()  lock      read a batch of album groups\ndecidi()     no lock   search MusicBrainz / iTunes / Deezer, decide\napplica()    no lock   download covers into the cover store\nregistra()   lock      write rows, provenance, aggregates (one transaction)" },
        { h: "The decision (pure, in rymembra-domain)" },
        "Every decision has three outcomes: `Applica` writes, `DaRivedere` (`needs-review`) writes **nothing** and retries in a week, `Nessuno` (`no-match`) writes nothing and retries in a month. An unknown axis (duration, artist) is neutral when ranking but never counts as agreement when deciding.",
        {
          ul: [
            "**Album route** (named album, more than one track): `album_distance` weighs album title 3, artist 3, track count 2, missing 0.9, unmatched 0.6, year 1, per-track title 3, duration 2, index 1. `decide_album` applies only if album title ≥ `TITOLO_FORTE` 0.85, artist ≥ `ARTISTA_FORTE` 0.80, **exact** track count, at least 4 of 5 tracks with a strong title, no track off by more than `VETO_DURATA_SEC` = 15 s, no variant mismatch, and distance ≤ `DISTANZA_APPLICA` 0.12; ≤ 0.35 is `DaRivedere`.",
            "**Track route**: candidates from three independent sources (MusicBrainz, iTunes, Deezer) are scored `0.4·title + 0.3·artist + 0.3·duration`. Below `SOGLIA_CANDIDATO` 0.65 → `Nessuno`. `Applica` requires a strong title, an anchor (artist ≥ 0.80 or duration within 4 s), **consensus** of two different declared sources on the same recording, no contradiction (artist or album similarity < 0.35) and no variant mismatch.",
            "**Variants** (live, remix, karaoke, acoustic, …) are compared, not stripped: two different variant masks are a veto, so a karaoke base of the same length cannot take the original's place.",
          ],
        },
        "`plan_write` then fills empty or fallback fields always, replaces existing values only on `Applica`, never erases a value the candidate does not know, never replaces a genre, and only adds identifiers (never over a different one). Fields whose provenance is a manual correction are never rewritten.",
        { h: "Network and cache" },
        "Requests go through `rymembra_meta::Fornitori` (20 s timeout), paced per service — MusicBrainz 1.1 s between requests, Cover Art Archive 250 ms, iTunes 350 ms, Deezer 220 ms — with a circuit breaker; see [Rate limits](#rate-limits). Responses are cached in the `enrich_cache` table (search 7 days, release 30 days, «not found» 3 days) through a secondary connection. If no source answers, the group is `non_raggiungibile`: nothing is written and nothing remembered. Covers come from the Cover Art Archive (`front-500`, release group, then up to 3 releases), iTunes (600 px) or Deezer, at most 8 MiB, magic-byte checked, stored with `CoverSource::CoverArtArchive` / `Provider`.",
        { h: "The thread" },
        "`rymembra-arricchimento`: first pass after 90 s, then every 30 min, and after scans/downloads once a 2 min burst is over. It never takes the player lock and holds the library lock only per group. `enrich.auto` (default on) switches it; events `arricchimento:stato`, `:avanzamento`, `:esito` report progress and the counts applied / held back / without match.",
        { h: "Forgetting and the legacy exit" },
        "`arricchimento_annulla` → `enrich::dimentica`: re-reads tags from the untouched files, clears what enrichment set, sets status `undone`, re-applies manual corrections, and switches `enrich.auto` off so the next pass does not redo it. `arricchimento_riporta_nei_file`, which restored into the files the tags that versions up to 2.3.0 had overwritten (from `enrich_undo`), was removed in 3.2.0: it was promised for one release in 2.3.1, and it was the last code in Rymembra that wrote into the user's files. The `enrich_undo` rows stay in the database, read and written by nobody.",
      ],
      see: ["corrections", "rate-limits", "albums"],
    },
    {
      id: "corrections",
      title: "Tags, enrichment and your corrections",
      src: ["core/rymembra-app/src/incerti.rs", "core/rymembra-app/src/provenienza.rs", "apps/desktop/src-tauri/src/metadati.rs", "core/rymembra-app/src/db/schema/016_metadati.sql", "core/rymembra-app/src/db/schema/023_correzioni_orfane.sql"],
      body: [
        "The effective metadata of a row is the result of three layers, applied in this order:",
        { code: "raw tags          →  track_meta_arricchita  →  track_overrides\n(what the file says)  (what a catalog found)      (what you decided)" },
        {
          ul: [
            "**Raw tags** are written by the scan (`library::update_track`) every time it sees the file changed.",
            "**Enrichment** writes its fields and, in `track_meta_arricchita`, the value the file had underneath (`Sotto`). After an external re-tag, each enriched field is kept only if the file still has the value it replaced (`arricchimento_sopra_i_tag`) — a new tag from another program wins.",
            "**Corrections** (`metadati_correggi` → `incerti::correggi`) are stored as JSON in `track_overrides` and re-applied after every rewrite of the row by `incerti::riapplica`. A correction recomputes `track_key` (other devices must find the track under its corrected name) but **never** `content_key`.",
          ],
        },
        "Corrections never touch the file, so its mtime is stable and it is not re-read every scan. If the file disappears, the correction is parked in `correzioni_orfane` by `content_key` for 90 days and restored when the file comes back ([Removal](#removal)). Profile imports apply corrections **before** anything else, so the rest of the imported data finds tracks under their corrected keys.",
        "`meta_origine` / `meta_problemi` hold per-field provenance (`Tag`, `TagRiparato`, `Percorso`, `Ripiego`, `Manuale`) as JSON, and `meta_salute` a summary used by the review list (`metadati_incerti`). As noted in [Tags](#tags), the scan does not yet fill these columns, so the review list is empty; corrections work regardless.",
      ],
      see: ["enrich", "identity", "tags"],
    },
    {
      id: "lyrics",
      title: "Lyrics",
      src: ["core/rymembra-domain/src/testo.rs", "core/rymembra-app/src/testi.rs", "core/rymembra-app/src/testi/disco.rs", "core/rymembra-app/src/testi/catalogo.rs", "apps/desktop/src-tauri/src/testi.rs", "core/rymembra-meta/src/lrclib.rs", "apps/desktop/src/parti/Testo.tsx", "apps/desktop/src/parti/SceltaTesto.tsx"],
      body: [
        "One LRC parser exists, `rymembra_domain::testo`; the window receives already-parsed lines (`TestoIpc`: lines with ms, optional per-word times, secondary lines such as translations, or a plain text, or «instrumental») and never sees raw LRC.",
        { h: "The source chain" },
        {
          ol: [
            "a sidecar beside the file — `<name>.a2.lrc` (word timing) then `<name>.lrc` — re-read every time so edits in a text editor show up; bytes are decoded with `codifica::leggi_byte` (UTF-8, then windows-1252/1251, Shift_JIS, GBK);",
            "synced tags in the file: `SYLT` (ID3v2), `SYNCEDLYRICS` (Vorbis/APE), read on demand, not by the scan;",
            "the `lyrics` row, keyed by `track_key` (so two formats of the same song share it, and moving the file keeps it): downloaded or hand-timed lyrics;",
            "the plain tag, `tracks.lyrics`.",
          ],
        },
        "What is found in 1, 2 and 4 is copied into 3, which acts as a cache. `Fonte` records where it came from: `sidecar`, `tag`, `lrclib`, `mano` (timed by hand, never overwritten) or none.",
        { h: "LRCLIB" },
        "When nothing local exists, Rymembra asks LRCLIB — public, no key. A request is sent **only** for the track playing and only while a lyrics panel is open (`testi_pannello` counts open panels), unless you start «Fill the library» (`testi_riempi`, batches of `LOTTO` = 50, stoppable). `testi.rete` switches the network off entirely. Requests are paced at one per 250 ms by the LRCLIB cadence. The exact `get` (artist, title, album, duration in seconds) is tried first, then a search, retrying after 500 and 1,500 ms; up to 20 candidates are scored by `testo::scegli`: duration veto (15 s), title veto below `SOGLIA_TITOLO` 0.55, accept above `SOGLIA_SCELTA` 0.66, with a bonus for synced text. A track with no answer is retried after `RIPROVA_MS` = 14 days, or 1 day after a network failure.",
        "`testo_candidati` lists all answers with their first lines; `testo_scegli` pins one (`lyrics.scelta`), `testo_rifiuta` / «none of these» records rejections (`lyrics.scartati`) so they are never proposed again. While a track plays, `precarica_prossimo` (thread `rymembra-testo-dopo`) fetches the next track's lyrics if a panel is open.",
        { h: "Fit and display" },
        "`verifica_durata` returns `Buona`, `Sospetta` (text ends more than `CODA_SOSPETTA_MS` = 75 s before the file) or `Fuori` (times beyond the end by more than 10 s). The active line is found by binary search; a line lights `ANTICIPO_MS` = 120 ms early (eye travel), and progress within it is computed from the real time. An `[offset:]` tag is preserved, not applied. Two buttons shift a constant drift (`testo_scarto`, stored in `lyrics.offset_ms`), and the output latency setting moves the reported position ([Output](#output)). `Testo.tsx` writes progress into a CSS variable through a ref, per line and per word, and shows three filling dots for instrumental gaps of 3 s or more.",
        { h: "Writing" },
        "Lyrics live in the database. A `.lrc` beside the track is written only if `testi.accanto` is on (default off). `testo_pubblica` can publish hand-timed lyrics to LRCLIB, which requires solving its proof-of-work challenge (`TENTATIVI_MASSIMI` = 50,000,000 nonces).",
      ],
      see: ["lyrics-sync", "output", "rate-limits"],
    },
    {
      id: "lyrics-sync",
      title: "Timing lyrics by hand",
      src: ["apps/desktop/src/Sincronizza.tsx", "core/rymembra-play/src/attacchi.rs", "core/rymembra-domain/src/testo.rs", "apps/desktop/src-tauri/src/testi.rs"],
      body: [
        "`Sincronizza.tsx` is a tap editor: paste the lines, play, press Space at the start of each line; optionally a second pass taps each word (single-word lines are skipped, their time is known). Human taps are late by a reaction time that varies, so the backend fixes them against the audio.",
        { h: "Onsets" },
        "`rymembra_play::attacchi` decodes the whole file at `FREQUENZA` = 22,050 Hz mono and computes spectral flux (increases only) between bins 14 and 186 (≈ 300–4,000 Hz, where voice attacks live), frames of 1,024 with hop 512 (≈ 23 ms resolution). A frame is an onset when its flux exceeds 1.5× the local mean over ±20 frames, with at least 50 ms between onsets. The onsets never cross IPC.",
        { h: "Snapping" },
        "`testo_aggancia` passes taps and onsets to the pure `testo::aggancia`:",
        {
          ol: [
            "each tap snaps to the nearest onset within `FINESTRA_PRIMA_MS` = 250 ms before or `FINESTRA_DOPO_MS` = 120 ms after — asymmetric because tapping is late, not early;",
            "an onset can be used by one tap only, so two close lines never collapse onto the same time;",
            "the **median** of (tap − onset) is the person's reaction latency in that session, subtracted from taps that found no onset (median, because one tap two seconds late would ruin a mean);",
            "the result is kept monotonic.",
          ],
        },
        "Without onsets (silent file, undecodable) the taps are returned unchanged. The result is saved with `testo_salva` as source `mano`; with word times it is extended LRC, exported as `.a2.lrc` beside the ordinary `.lrc` when sidecars are enabled.",
      ],
      see: ["lyrics"],
    },
  ],
});
