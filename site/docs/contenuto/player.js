/* Capitolo 6 — suonare: il lettore nell'app, la coda, gli ascolti, l'autoplay, la settimana, i flussi, il timer, Windows. */
(window.AETHER_DOCS = window.AETHER_DOCS || []).push({
  id: "playing",
  title: "Playing music",
  lead: "How the desktop app drives the engine: the player state and its threads, the queue, what counts as a listen, what plays when the queue ends, streams, the sleep timer and the Windows media controls.",
  entries: [
    {
      id: "player",
      title: "The player in the desktop app",
      src: ["apps/desktop/src-tauri/src/riproduzione/mod.rs", "apps/desktop/src-tauri/src/riproduzione/fili.rs", "core/aether-app/src/playback.rs", "apps/desktop/src/riproduzione.ts"],
      body: [
        "`riproduzione` introduces three parties that do not know each other: the queue (`aether_domain::queue`, pure), the engine (`aether-play`, no library) and the database (`aether_app::playback`). `StatoLettore` holds the `Lettore` behind its own mutex — engine handle, queue, `ListenTracker`, volume, EQ, normalization, autoplay, crossfade, latency, the prepared track (`in_canna`) — plus atomics outside it for things read without the lock (spectrum on/off, sleep timer, resume point, save flags, «opening» guard).",
        "Lock order: **player, then library, never the reverse**; and no lock is held while waiting for a disk or the network.",
        { h: "Opening a track" },
        {
          ol: [
            "`brano_di` reads the row under the library lock (`playback::scheda_da_suonare`, which also picks the gain level from the queue neighbours) and releases it.",
            "On a deadline thread (`aether-scadenza-apertura-brano`) it opens the bytes: a file through `MusicFiles`, or an HTTP stream for catalog rows — see [Streaming](#streaming) — then probes the codec (`BranoAperto::apri`). Deadline: `APERTURA_BRANO` = 5 s, or `APERTURA_FLUSSO` = 15 s for streams and for paths that may be on the network.",
            "Back under the player lock, if the queue and output format did not change meanwhile, `Motore::suona`. `Aprendo` deduplicates concurrent opens of the same track; `avvia_corrente` tries twice.",
          ],
        },
        "A network failure (`fs.networkUnavailable`, `playback.streamInterrupted`) **stops** with a retryable `riproduzione:errore` and keeps the position, so «Retry» (`riprova_corrente`) resumes at the same millisecond; other failures skip to the next track.",
        { h: "Events and the ordered thread" },
        "Engine announcements arrive on `aether-eventi` and are handled by `su_evento` (ignored if the engine was replaced or a newer command superseded them). `iniziato` closes the previous listen, advances the queue if the engine already moved to the prepared track, starts a new `ListenTracker`, sends «now playing» to scrobbling and preloads the next lyrics on their own threads. All slow follow-up — composing `StatoRiproduzione`, updating SMTC, tray and taskbar buttons, writing listens — is snapshotted under the lock and executed **in order** on `aether-seguito`, so the window sees states in the order they happened and nobody waits.",
        {
          table: {
            head: ["Event", "Payload", "Cadence"],
            rows: [
              ["`riproduzione:stato`", "`StatoRiproduzione`: track summary, paused, position, duration, shuffle, repeat, volume, mute, queue ids, queue index, EQ, normalization name, sleep timer, autoplay, crossfade, latency, format", "on change"],
              ["`riproduzione:tempo`", "`{ posizione_ms, durata_ms, in_pausa, in_attesa }`", "every 250 ms while playing"],
              ["`riproduzione:audio`", "device lost / decoder stalled", "once per failure"],
              ["`riproduzione:errore`", "`ErroreIpc`", "on failure"],
              ["`riproduzione:dispositivi`, `:volume`, `:eq`, `:spettro`", "device list, volume, EQ, bands", "on change / 33 ms"],
            ],
          },
        },
        "The clock thread `aether-orologio` (250 ms) also saves what is dirty (queue, volume, EQ; 2 s lock budget, 400 ms when exiting), detects device loss, fires the sleep timer, saves the resume position every 20 beats (5 s) in `player.position`, and sets `in_attesa` after two «dry» beats in which a stream did not advance — the window then says «Waiting for data…». The window interpolates the playhead between beats ([Frontend](#frontend)).",
        "`riprendi_coda` restores yesterday's queue and position at startup without playing; `primo_stato_fuori` then shows it in SMTC, tray and taskbar.",
      ],
      see: ["queue", "engine", "output", "listens"],
    },
    {
      id: "queue",
      title: "The queue",
      src: ["core/aether-domain/src/queue.rs", "apps/desktop/src-tauri/src/riproduzione/coda.rs", "core/aether-app/src/playback.rs", "apps/desktop/src/Coda.tsx"],
      body: [
        "`Queue` keeps `tracks: Vec<i64>` in insertion order and never reorders it; shuffle permutes a separate `order` vector. Turning shuffle off returns to the identity permutation, and the queue panel does not change under your fingers. Shuffling is Fisher–Yates with a SplitMix64 seed passed by the caller (the domain has no randomness), with the current track moved first.",
        {
          ul: [
            "`RepeatMode` cycles off → all → one. `advance(manual)`: «repeat one» restarts only at the natural end, not when Next is pressed.",
            "`previous(position)` restarts the track when position ≥ `RIAVVIO_SOTTO_MS` = 3 s, otherwise goes back (wrapping with repeat-all).",
            "`remove_tracks` keeps the current track consistent; `snapshot`/`restore` validate the permutation and rebuild it if a saved one is invalid.",
          ],
        },
        "Commands: `suona` (replace and play), `coda_accoda` (append), `coda_dopo` (play next), `coda_vai`, `coda_togli`, `coda_riordina`, `coda_svuota`, `ripeti`, `mescola`, `brani_per_id` (rows for the panel; the state carries ids only), `coda_prepara` (fill without playing, used by the tour). Replacing a queue of at least `SOGLIA_DI_UNA_SESSIONE` = 25 tracks (or a curated/emptied one) keeps the old queue and emits `coda:sostituita`, so `coda_ripristina` can undo it.",
        "The queue is persisted as JSON in `player.queue` (ids, order, index, shuffle, repeat). Saving only sets a flag; the clock thread writes it within 250 ms. It does not travel in a profile: ids name rows of this library.",
      ],
      see: ["player", "autoplay"],
    },
    {
      id: "listens",
      title: "What counts as a listen",
      src: ["core/aether-domain/src/listen.rs", "core/aether-app/src/playback.rs", "core/aether-app/src/sincronia.rs", "core/aether-app/src/scrobble.rs"],
      body: [
        "One rule, used for the play count, the history, the weekly collections and scrobbling: `counts_as_play(listened, duration)` —",
        {
          ul: [
            "duration unknown (0): counts at ≥ 4 min (`SOGLIA_ASSOLUTA_MS`);",
            "duration < 30 s (`DURATA_MINIMA_MS`): never;",
            "otherwise: ≥ 4 min **or** at least half the duration (`listened × 2 ≥ duration`).",
          ],
        },
        "`ListenTracker` (`begin`, `pause`, `resume`, `finish`) accumulates only time actually played; a clock going backwards adds nothing. A device change pauses the tracker instead of discarding it, so switching to headphones mid-song keeps the listen.",
        { h: "Recording" },
        "`playback::record_play(conn, &Listen, device)` writes nothing if the listen does not count. Otherwise, in one Immediate transaction: `last_played_at` and `stats_updated_at` = the **start** time; `sincronia::conta_ascolto` increments this device's counter in `sync_ascolti` and sets `play_count` to the sum over devices on every row with the same `track_key`; a `play_history` row (unique on `(track_id, played_at)`, so re-imports cannot double it). Only then are scrobbles queued for each enabled service ([Scrobbling](#scrobbling)).",
      ],
      see: ["sync", "scrobbling"],
    },
    {
      id: "autoplay",
      title: "When the queue ends",
      src: ["core/aether-app/src/autoplay.rs", "core/aether-domain/src/affinita.rs", "core/aether-app/src/vicinanza.rs", "core/aether-app/src/sonora.rs", "core/aether-meta/src/listenbrainz.rs"],
      body: [
        "With `player.autoplay` on (and repeat off), `autoplay::prossimo` picks the next track when the queue runs out. It runs on the preparer thread while the last track still plays, so it **never** asks the network: everything it needs is already in the database.",
        { h: "The cascade" },
        {
          ol: [
            "the rest of the album being played — never reordered;",
            "another album by the same artist;",
            "the same genre, not played in the last 30 days;",
            "a liked track never played;",
            "any track.",
          ],
        },
        "From rung 2 down, up to 200 candidates are **re-ranked** by affinity; the first rung that yields something wins. The reason (`Motivo`: album, artist, genre, liked, random, sound, listens, taste, long-unplayed) crosses IPC as a code and the queue shows it.",
        { h: "Affinity" },
        "`aether_domain::affinita::punteggio` combines three layers with default weights sound 0.40, neighbours 0.25, taste 0.35, minus a repetition penalty 0.50:",
        {
          ul: [
            "**Sound** — distance between 47-number fingerprints, normalized by the library's own scale (needs at least `MINIMO_BRANI` = 50 measured tracks).",
            "**Neighbours** — `brano_vicino`, filled during the enrichment pass from ListenBrainz Labs «similar recordings/artists» (25 MBIDs per request, at most 20 kept per track, artist-level neighbours ×0.7, refreshed every 90 days); only neighbours already in your library are stored.",
            "**Taste** — play count and likes with a half-life of 180 days; **repetition** — a penalty with a 7-day half-life, so something heard an hour ago cannot win.",
          ],
        },
        "A missing layer redistributes its weight instead of counting as zero; with no layer at all the cascade order stands. Thresholds for naming a reason: sound ≥ 0.68, listens ≥ 0.5, taste ≥ 0.6, «long unplayed» = 180 days.",
        { h: "Radio" },
        "`radio` seeds a queue of `RADIO` = 30 tracks similar to one track, with at most `TETTO_ARTISTA` = 3 in a row by the same artist; when they end, autoplay continues as usual.",
      ],
      see: ["analysis", "weekly", "queue"],
    },
    {
      id: "weekly",
      title: "The Monday collections",
      src: ["core/aether-domain/src/settimana.rs", "core/aether-app/src/settimana.rs", "apps/desktop/src/schermate/Home.tsx"],
      body: [
        "Every week Home offers up to four collections of `QUANTI` = 20 tracks (at least `MINIMO` = 8), generated locally and frozen for seven days: one «rediscovered» and up to `ANCORA` = 3 «more like this».",
        {
          ul: [
            "**Rediscovered** (`ripescati`): score = affection × oblivion. Oblivion grows from 60 to 365 days since the last play; affection is full at 6 plays, a heart adds one third.",
            "**More like this** (`ancora`): deterministic «seed and grow» clustering on fingerprints over up to 2,000 candidates — fixed-size groups, next seed = the farthest point — named after the most frequent genre or artist when it covers at least half the group (`MAGGIORANZA` = 0.5), otherwise «Mix N».",
            "The week starts at local Monday midnight (`lunedi(now, utc_offset)`); tracks that appeared last week are excluded; 4 weeks are kept.",
          ],
        },
        "Generated once, in one transaction, and recorded in `settimana.ultimo_lunedi`: recomputing mid-week would change the collections as soon as you played them. Commands `settimana` and `settimana_apri` (marks a collection as opened, so Home stops announcing it).",
      ],
      see: ["autoplay", "analysis"],
    },
    {
      id: "streaming",
      title: "Playing a catalog stream",
      src: ["apps/desktop/src-tauri/src/riproduzione/flusso.rs", "core/aether-net/src/flusso.rs", "core/aether-catalogo/src/riferimento.rs", "core/aether-catalogo/src/archivio_org.rs", "core/aether-catalogo/src/audius.rs", "core/aether-app/src/catalogo.rs"],
      body: [
        "A catalog row (`path IS NULL`, see [Explore](#explore)) plays without ever being written to disk.",
        {
          ol: [
            "`riferimento_di` accepts only `https` URLs whose `aether_catalogo::riconosci` source equals the row's declared source (Audius paths must start with `/v1/tracks/`).",
            "`apri_flusso`: for the Internet Archive, `da_ascoltare` swaps a FLAC for its MP3 sibling (cached per session); for Audius, `prepara` picks a content node (one per session, re-picked once on failure). Jamendo only with the `jamendo` feature.",
            "`FlussoHttp` (in `aether-net`) is a `Read + Seek` over HTTP Range requests with no disk cache: the window starts at `FINESTRA` = 256 KiB and doubles on sequential reads up to 2 MiB; after 64 KiB of sequential reading the next window is prefetched on `flusso-anticipo` (failed prefetch backs off 1 → 16 s). The first request learns the total length (needed to seek) and the node a redirect pointed to, which is then reused.",
          ],
        },
        "**Redirects** go through a gate (`cancello_di`): at most `RIMANDI_FLUSSO` = 5, `https` only, and the target host must still be recognised by `riconosci` for the same catalog (so `archive.org/download/…` → `ia800305.us.archive.org` passes, `evilarchive.org` does not). Audius nodes cannot be allow-listed, so a redirect must at least go to a **public DNS name** — never an IP literal, `localhost` or a home-network suffix.",
        "**Seeking** in a stream uses symphonia's coarse mode and `riallinea_segno` corrects the shown position from the first decoded block. An MP3 without a seek table must be scanned, i.e. downloaded up to the target; while that lasts the scrubber says so. A seek or a new command raises the cancel flag of the read in progress, so it stops waiting for the network (`internal.aborted`, silent). Network errors are mapped to raw Windows codes 64/121/1231 so the decoder classifies them as network failures and playback stops with «Retry».",
      ],
      see: ["http-streams", "catalogs", "player"],
    },
    {
      id: "sleep",
      title: "Sleep timer",
      src: ["apps/desktop/src-tauri/src/riproduzione/fili.rs", "core/aether-play/src/motore.rs"],
      body: [
        "`spegnimento(minuti)`: 0 switches off, `FINE_DEL_BRANO` (−1) stops at the end of the current track, any positive value is minutes from now, clamped to 1…1,440 (24 h). Minutes rather than a timestamp, so the window's and the core's clocks never have to agree.",
        "When the timer is due, the clock thread asks the engine to fade (`Motore::sfuma`) over `SFUMATURA_SPEGNIMENTO` = 6 s — linear in dB down to −60 dB (`SFUMATURA_DB`) — and then pauses, checking again 500 ms after the fade should have ended. The position is kept, so resuming continues from there. «End of track» simply does not prepare a next track. The timer is not saved and does not travel in a profile: it is tonight's decision. The remaining time is part of `StatoRiproduzione` (`spegnimento_ms`, `0` meaning «end of track»).",
      ],
      see: ["crossfade", "player"],
    },
    {
      id: "os-media",
      title: "Windows media controls and taskbar buttons",
      src: ["apps/desktop/src-tauri/src/media.rs", "apps/desktop/src-tauri/src/barra.rs", "apps/desktop/aether-barra/src/lib.rs", "apps/desktop/aether-barra/src/finestra.rs", "apps/desktop/aether-barra/src/aspetto.rs", "apps/desktop/src-tauri/src/vassoio.rs"],
      body: [
        "**SMTC** (`media.rs`, through `souvlaki`): registering `SystemMediaTransportControls` gives the Windows 11 volume flyout card **and** routes media keys and Bluetooth headset buttons to Aether — no global shortcut is registered, so the keys are never stolen from other apps. It is updated from one place only, `manda_stato`, with the cover as a real `file://` path. Presses go to a lazily started ordered thread (`aether-tasti`); seek steps are 5 s.",
        "**Taskbar thumbnail buttons** (previous, play/pause, next under the window preview) live in the crate `aether-barra`, the only code with `unsafe` in Aether, confined to `finestra.rs`: `ITaskbarList3::ThumbBarAddButtons` plus a window subclass (`SetWindowSubclass`) that handles `TaskbarButtonCreated` (Explorer restarting recreates the buttons with the last remembered look), `THBN_CLICKED` and a private update message. Icons are drawn procedurally at the small-icon size (`glifi.rs`); button logic is pure (`aspetto.rs`). `Barra::mostra` can be called from any thread.",
        "The tray icon's transport items use the same `media::premi` path. All three surfaces get their first state from `primo_stato_fuori` at startup, and `media::stacca` / `vassoio::stacca` remove them on exit.",
      ],
      see: ["background", "player"],
    },
  ],
});
