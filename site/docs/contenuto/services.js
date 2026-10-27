/* Capitolo 9 — i servizi: backup su Drive, sincronia, scrobbling, modelli di linguaggio, aggiornamenti. */
(window.RYMEMBRA_DOCS = window.RYMEMBRA_DOCS || []).push({
  id: "services",
  title: "Online services",
  lead: "Everything optional that talks to a remote service: Drive backup, device sync, scrobbling, language models for the Studio, and the update check.",
  entries: [
    {
      id: "cloud",
      title: "Backup on Google Drive",
      src: ["core/rymembra-cloud/src/servizio.rs", "core/rymembra-cloud/src/drive.rs", "core/rymembra-cloud/src/oauth.rs", "core/rymembra-cloud/src/pacchetti.rs", "core/rymembra-app/src/backup.rs", "core/rymembra-domain/src/restore.rs", "apps/desktop/src-tauri/src/nuvola.rs", "apps/desktop/src-tauri/build.rs"],
      body: [
        "The backup stores what a scan cannot rebuild — statistics, playlists, tombstones, watched roots, skins and Studio drafts — in Drive's **app-data folder** (`drive.appdata` scope): invisible in Drive, readable only by this OAuth client. Audio files and covers are not uploaded.",
        {
          table: {
            head: ["File in appDataFolder", "Contents"],
            rows: [
              ["`rymembra-libreria.v1.json.gz`", "`Salvataggio { version: 1, generatedAt, generatedBy, content }` — tracks by `track_key` with merged stats, playlists with member keys, tombstones, roots, active skin, skin and draft hashes (≈ 70 KB per 1,400 tracks)"],
              ["`rymembra-skin-<id>.ryskin`", "Installed skin packages (up to 20 MB, rarely change)"],
              ["`rymembra-bozza-<id>.json.gz`", "Studio draft manifests"],
            ],
          },
        },
        "Files that the previous version left in the app-data folder under its own names are recognised: during a pass they are renamed in place to the names above (same file, same content, no new upload). If both the old and the new name exist the new one wins and a copy under the old name is removed only when an identical one is kept, and a document of this very device under the old name is never read as another device's.",
        { h: "A pass" },
        {
          ol: [
            "Under a short lock, `backup::snapshot` builds the content; its `blake3` hash over canonical JSON (BTreeMaps, envelope excluded) is the fingerprint.",
            "One `files.list` (fields `id,name,size,appProperties`) tells, from the fingerprints stored in `appProperties`, whether anything changed: on a quiet library a pass is one HTTP request.",
            "If another device wrote the library file, download it, `backup::fondi` it with ours in memory, upload the union, then list again; up to `GIRI_DI_FUSIONE` = 3 rounds. Duplicate copies (Drive allows two files with the same name) are merged and deleted. The remote file never regresses.",
            "Uploads up to 5 MB are multipart; larger ones resumable in 8 MiB chunks (4 consecutive failures allowed, resuming from the `Range` the server confirms). Downloads are capped at 25 MiB and decompression at 64 MiB.",
          ],
        },
        "**The local database is never written by a backup pass.** Restoring is explicit: `nuvola_piano_ripristino` downloads, parses (`interpreta`: bad records dropped, a newer format refused) and shows `rymembra_domain::restore::plan_restore` — stats merged with `merge_stats`, missing tracks only listed, playlists replaced only if the backup's is strictly newer, roots unioned, skins/drafts added only if missing; `nuvola_ripristina` re-plans and applies it in one transaction.",
        { h: "Schedule and credentials" },
        "Thread `rymembra-nuvola`: first pass 30 s after start, then every 15 min, and 2 min after a burst of changes (any command that changes the library calls `nuvola::se_riuscito`). Drive calls have a 120 s deadline. `nuvola.attivo` switches it.",
        "The Google consent runs in the system browser with PKCE and a loopback redirect, 3 min to complete ([OAuth](#oauth)). A desktop program cannot keep an OAuth secret, so **release builds ship without a Google client**: Settings asks for your own «Desktop app» client id and secret (`nuvola_credenziali`; the secret goes to the keychain). Builds made with `RYMEMBRA_GOOGLE_CLIENT_ID` / `RYMEMBRA_GOOGLE_CLIENT_SECRET` in the environment embed them via `option_env!` (`build.rs` re-exports them).",
      ],
      see: ["sync", "profile", "oauth"],
    },
    {
      id: "sync",
      title: "Device sync",
      src: ["core/rymembra-sync/src/lib.rs", "core/rymembra-sync/src/documento.rs", "core/rymembra-sync/src/fusione.rs", "core/rymembra-sync/src/contatore.rs", "core/rymembra-sync/src/registro.rs", "core/rymembra-sync/src/sequenza.rs", "core/rymembra-sync/src/magazzino.rs", "core/rymembra-sync/src/motore.rs", "core/rymembra-app/src/sincronia.rs", "apps/desktop/src-tauri/src/sincronia.rs", "core/rymembra-sync/tests/convergenza.rs"],
      body: [
        "No server, no ordering guarantees. Each device writes **only its own** document, `dispositivi/rymembra-<id>.v1.json.gz`, and every device merges all of them. The same layout works on a shared folder (Syncthing, OneDrive, Dropbox, a USB stick) and on Drive, and «conflicted copies» cannot occur. State per device, not an operation log: its size is the library's, not the history's, so nothing ever needs compaction.",
        { h: "Four merge shapes" },
        {
          table: {
            head: ["Data", "Shape", "Rule"],
            rows: [
              ["Play counts", "`Contatore`", "Each device only increments its own counter; per device take the max, total = sum (5 plays on the laptop + 3 on the phone = 8). Imported history is the pseudo-device `importazione`"],
              ["Rating, like, last played, resume position, active skin", "`Registro` (`Voto`, `Interruttore`, `Momento`, `Scelta`)", "Newest timestamp wins; deterministic tie-breaks (higher rating, liked = OR, max ms, larger id)"],
              ["Playlist order", "`Sequenza`", "Each element has an identity `<device>:<n>` and an «after» pointer; order is a traversal with children by descending identity; concurrent inserts are both kept; removals are tombstones"],
              ["Deletions", "lapidi", "A playlist tombstone removes the playlist only if it is newer than the playlist's last change; track tombstones never delete rows"],
            ],
          },
        },
        "All merges are commutative, associative and idempotent; `tests/convergenza.rs` checks with seeded random histories that any merge order converges and that concurrent playlist edits are not lost. Documents are canonical JSON (BTreeMaps), gzipped, hashed with `blake3`, read up to 64 MiB decompressed; bad records are dropped, a newer version is refused.",
        { h: "A pass" },
        {
          ol: [
            "Lock window 1: `sincronia::allinea` brings `sync_ascolti`, `sync_sequenza` and `sync.roots` in step with the library; `contenuto` builds this device's document.",
            "No lock: `Motore::passata` lists the store (`Magazzino`: list, read, write, rename, delete), reads only documents whose hash changed (remembered in `Memoria`), merges with `fusione::fondi`, and rewrites its own document only if its hash changed.",
            "Lock window 2: `sincronia::applica_se_ferma` re-checks that playlists and roots did not change meanwhile (otherwise it rolls back and retries later) and writes counters, registers, playlists (sequence → `playlist_tracks`), roots and tombstones.",
          ],
        },
        "A device that appears in the store is **unknown** until accepted: `sincronia_accoppia` gives it a name and trusts it (`sync_dispositivi`); untrusted documents are not merged. `sincronia_dimentica` forgets a device without removing the plays it brought. The store is a folder (`sincronia.cartella`) **or** Drive, never both (`sincronia.dove`). Thread `rymembra-sincronia`: first pass after 20 s, every 5 min, 30 s after a burst; switch `sincronia.attiva`.",
      ],
      see: ["cloud", "listens", "profile"],
    },
    {
      id: "scrobbling",
      title: "Scrobbling",
      src: ["core/rymembra-scrobble/src/lib.rs", "core/rymembra-scrobble/src/lastfm.rs", "core/rymembra-scrobble/src/listenbrainz.rs", "core/rymembra-app/src/scrobble.rs", "apps/desktop/src-tauri/src/scrobble.rs"],
      body: [
        "Only listens that already passed `counts_as_play` are scrobbled ([Listens](#listens)), so the services count exactly what the library counts. `rymembra-scrobble` never sees the database: the queue is drained by reading a block, releasing the lock, sending, then reopening to delete.",
        {
          table: {
            head: ["Aspect", "ListenBrainz", "Last.fm"],
            rows: [
              ["Credentials", "User token", "Your own API key and secret (Last.fm forbids shipping one), browser consent, session key"],
              ["Per request", "1,000 listens (`import` type for history)", "50 scrobbles, MD5 `api_sig` over sorted params"],
              ["Timeout / attempts", "60 s / 3", "30 s / 3"],
              ["Imported Spotify history", "Can be sent", "Not possible (old timestamps refused, daily cap)"],
            ],
          },
        },
        "The durable `scrobble_queue` (unique on service, time, artist, title) survives restarts; a failed item counts attempts up to `MASSIMI_TENTATIVI` = 10 (a non-retryable rejection jumps to the cap and shows as «stuck»). Thread `rymembra-scrobble` wakes on demand or every 300 s; a ListenBrainz batch rejected with 400 is bisected down to the bad entry. «Now playing» is sent on its own short-lived thread and never queued. With `scrobble.attivo` off the queue stops filling, so turning it back on does not send three days of plays at once. Tokens, keys, secrets and the session live in the keychain; Last.fm's API key moved there from `settings` in 2.3.1.",
      ],
      see: ["listens", "oauth"],
    },
    {
      id: "models",
      title: "Language models",
      src: ["core/rymembra-ia/src/lib.rs", "core/rymembra-ia/src/cliente.rs", "core/rymembra-ia/src/fornitore.rs", "core/rymembra-ia/src/profilo.rs", "core/rymembra-ia/src/sse.rs", "core/rymembra-ia/src/operazioni.rs", "core/rymembra-ia/src/messaggi.rs", "apps/desktop/src-tauri/src/ia.rs"],
      body: [
        "A model is used for one thing: the Skin Studio chat ([Studio chat](#studio-chat)). `rymembra-ia` is a plain OpenAI-compatible client that knows nothing about skins; the Studio passes the vocabulary as messages.",
        {
          table: {
            head: ["Provider", "Endpoint", "Key"],
            rows: [
              ["OpenRouter", "`https://openrouter.ai/api/v1`", "required"],
              ["Ollama", "`http://localhost:11434/v1`", "none — runs on this machine"],
              ["Bionic (LM Studio's local server)", "`http://localhost:1234/v1`", "none"],
              ["Custom", "any `https://` URL, or plain `http` only to `127.0.0.1`/`localhost`", "optional"],
            ],
          },
        },
        {
          ul: [
            "**Profiles** (up to 16) live in `ia.profili`, the active one in `ia.profilo`. A profile never contains the key, only whether one exists and its keychain entry name; the key is bound to the **origin** it was entered for, so changing a profile's URL to another origin forgets the key instead of sending it there.",
            "**Requests**: `conversa` streams with a 10 min timeout, `modelli` lists models with 15 s; no redirects at all (a 307 would resend the conversation). 401/403, unknown model and a refused local connection get their own codes (`ia.unauthorized`, `ia.modelUnknown`, `ia.localServerDown`); error details are capped at 300 characters. `Cliente`'s `Debug` is hand-written so the key cannot leak.",
            "**Streaming**: `sse::Sse` parses `data:` events incrementally across chunk boundaries (CR, LF, CRLF; `[DONE]`), with at most 1 MiB of leftover. The desktop forwards chunks as `ia:pezzo` throttled to 50 ms, then `ia:fine` or `ia:errore`; one conversation at a time (`ia.busy`), stoppable with `ia_ferma`.",
            "**Edits** come back as a fenced JSON block whose info string is `rymembra-patch`, extracted by `operazioni` (at most 12 operations, reasons ≤ 120 characters) — more portable across local models than tool calling. Images up to 8 MB may accompany a message.",
          ],
        },
        "What is sent: the skin document, the selection and what you typed — never the library or your listens.",
      ],
      see: ["studio-chat", "oauth"],
    },
    {
      id: "update",
      title: "Updates, logs and documents",
      src: ["apps/desktop/src-tauri/src/aggiornamenti.rs", "apps/desktop/src-tauri/tauri.conf.json", "apps/desktop/src-tauri/installer/precedente.nsh", "apps/desktop/src-tauri/src/comandi.rs", "apps/desktop/src/parti/Aggiornamenti.tsx", "strumenti/manifesto.js", "strumenti/msix.js"],
      body: [
        "`tauri-plugin-updater` is driven only by `aggiornamenti.rs`; the window has no updater permission. The endpoint is `https://github.com/FedericoBaratti/Rymembra/releases/latest/download/latest.json`.",
        { h: "Checking" },
        {
          ul: [
            "Thread `rymembra-aggiornamenti`: first check `DOPO_PRONTO` = 10 s after the window reports ready (or 60 s if it never does), then every 30 min; skipped at start only if the last success was less than 5 min ago and found nothing. Offline: retry after 1, 5 and 15 min.",
            "The request carries **nothing of yours**: no identifier, not even the installed version (the comparison happens locally) and no Rymembra user agent — GitHub sees an IP fetching a public file.",
            "No public key in `tauri.conf.json` → no check at all (an updater that cannot verify must not download). `aggiornamenti.attivo` off → only «Check now». A found version is remembered (`aggiornamenti.trovata`); «Skip this version» writes `aggiornamenti.saltata`.",
            "**A copy from the Microsoft Store never checks.** `strumenti/msix.js` puts `rymembra-store.txt` next to the executable (`MARCATORE_STORE`), and when the file is there `configurato()` is false: the thread still starts, but every pass ends as `Giro::Saltato` without a request, the switch in Settings › Updates is disabled with `settings.update.store` as its explanation, and «Check now» is disabled. A file and not `GetCurrentPackageFullName`, because that call is `unsafe` and the workspace forbids it; the NSIS installer could not write inside a read-only MSIX package anyway. The Store updates that copy.",
          ],
        },
        { h: "Installing" },
        "Only on explicit request (`aggiornamenti_installa`; a double click is guarded). The download runs on `rymembra-aggiornamento` with progress events every 200 ms; it is cancelled after 60 s without a byte (`net.timeout`) or by `aggiornamenti_annulla`. The plugin verifies the **minisign** signature against the key compiled into the executable, and since 3.0 `requireSignedVersion` also requires the signature to be bound to the version the manifest announces. Playback is paused, the NSIS installer runs in passive mode, and `on_before_exit` runs the normal shutdown sequence ([Startup](#startup)) before the process exits. A build compiled without signing keys refuses to download.",
        "The NSIS installer also takes care of the previous version (`installer/precedente.nsh`, hooked in through `bundle.windows.nsis.installerHooks`): before copying anything it silently uninstalls it, leaving its data folder alone — the data are picked up at the first start ([Data](#data)). If the previous version is still running, the passive/silent installer waits a few seconds for it to close and a graphical one asks the user to close it; if it stays open the installer goes on and leaves the old version installed.",
        { h: "Logs and documents" },
        "The same Settings card opens the log folder (`diario_apri`; see [Logs](#logs)) and, through `apri_documento`, a closed list of public documents in the system browser — repository, issues, licence, third-party notices, privacy, terms, donations — never in the webview.",
      ],
      see: ["signing", "logs", "concepts"],
    },
  ],
});
