/* Capitolo 9 — i servizi: backup su Drive, sincronia, scrobbling, modelli di linguaggio, aggiornamenti. */
(window.RYMEMBRA_DOCS = window.RYMEMBRA_DOCS || []).push({
  id: "services",
  title: "Online services",
  lead: "Everything optional that talks to a remote service: Drive backup, device sync, scrobbling, language models for the Studio, and the update check.",
  entries: [
    {
      id: "cloud",
      title: "Backup on Google Drive",
      src: ["core/rymembra-cloud/src/servizio.rs", "core/rymembra-cloud/src/drive.rs", "core/rymembra-cloud/src/oauth.rs", "core/rymembra-cloud/src/pacchetti.rs", "core/rymembra-app/src/backup.rs", "core/rymembra-domain/src/restore.rs", "apps/desktop/src-tauri/src/nuvola.rs", "apps/desktop/src-tauri/src/nuvola_app.rs", "core/rymembra-app/src/nuvola_app.rs"],
      body: [
        "The backup stores what a scan cannot rebuild — statistics, playlists, tombstones, watched roots, skins and Studio drafts — in Drive's **app-data folder** (`drive.appdata` scope): invisible in Drive, readable only by the user's Google app that wrote it. Audio files and covers are not uploaded.",
        {
          table: {
            head: ["File in appDataFolder", "Contents"],
            rows: [
              ["`rymembra-libreria.v1.json.gz`", "`Salvataggio { version: 1, generatedAt, generatedBy, content }` — tracks by `track_key` with merged stats, playlists with member keys, roots in path order with when each was switched on, skin and draft hashes with their modification time, tombstones for tracks, playlists, roots, skins and drafts (≈ 70 KB per 1,400 tracks). The active skin is not in it: it stays with each device"],
              ["`rymembra-skin-<id>.ryskin`", "Installed skin packages (up to 20 MB, rarely change)"],
              ["`rymembra-bozza-<id>.json.gz`", "Studio draft manifests"],
            ],
          },
        },
        "Files that the previous version left in the app-data folder under its own names are recognized: during a pass they are renamed in place to the names above (same file, same content, no new upload). If both the old and the new name exist the new one wins and a copy under the old name is removed only when an identical one is kept, and a document of this very device under the old name is never read as another device's.",
        { h: "A pass" },
        {
          ol: [
            "Under a short lock, `backup::snapshot` builds the content; its `blake3` hash over canonical JSON (BTreeMaps, envelope excluded) is the fingerprint.",
            "One `files.list` (fields `id,name,size,appProperties`) tells, from the fingerprints stored in `appProperties`, whether anything changed: on a quiet library a pass is one HTTP request.",
            "If another device wrote the library file, download it, `backup::fondi` it with ours in memory, upload the union, then list again; up to `GIRI_DI_FUSIONE` = 3 rounds. Duplicate copies (Drive allows two files with the same name) are merged and deleted. The remote file never regresses. The merge (`backup::fondi`) is commutative: a playlist's newest version wins and, at equal dates, the more complete one; for each skin or draft the most recently modified version wins; a removed root, an uninstalled skin or a deleted draft travels as a tombstone and leaves the backup. A package is uploaded only when this device's version is the one that wins, identical duplicate package copies are deleted, and the package of a tombstoned skin or draft is deleted from Drive.",
            "Uploads up to 5 MB are multipart; larger ones resumable in 8 MiB chunks (4 consecutive failures allowed, resuming from the `Range` the server confirms). Downloads are capped at 25 MiB and decompression at 64 MiB.",
          ],
        },
        "**The local database is never written by a backup pass.** Restoring is explicit: `nuvola_piano_ripristino` downloads, parses (`interpreta`: bad records dropped, a newer format refused) and shows `rymembra_domain::restore::plan_restore` — values brought back into range (future timestamps clamped to now, out-of-range play counts dropped, `backup::nel_dominio`), stats merged with `merge_stats`, missing tracks only listed, playlists replaced only if the backup's is strictly newer — a playlist restored without some of its tracks takes the backup's date minus one, so a restore after the scan completes it and the merge keeps the complete version; replacing also drops the playlist's held places in `playlist_orfane` —, a playlist here with the same name but a different creation time is a different playlist and is never touched: the backup's goes beside it as «Name (Drive)» (D-11, `rymembra_domain::omonime`), and a later restore finds that copy again by its creation time, roots unioned (tombstoned ones excluded; folders on a network host this PC does not already use are not probed and are shown as not checked), skins/drafts added only if missing, the active skin never changed; `nuvola_ripristina` re-plans and applies it in one transaction. A restored package keeps the modification time it has in the backup.",
        { h: "Schedule and credentials" },
        "Thread `rymembra-nuvola`: first pass 30 s after start, then every 15 min, and 2 min after a burst of changes (any command that changes the library calls `nuvola::se_riuscito`). Drive calls have a 120 s deadline. `nuvola.attivo` switches it.",
        "The Google consent runs in the system browser with PKCE and a loopback redirect, 3 min to complete ([OAuth](#oauth)). A desktop program cannot keep an OAuth secret, so **no Google client is compiled in**, nor any other since 3.2: the backup uses a Google app the user registers in «Your apps» ([Cloud folders](#cloud-folders)) — by default the same app as the Google Drive folders, with a consent of its own — bound by the setting `nuvola.app`. `nuvola_backup_app` chooses it (it replaced `nuvola_credenziali`); without one the backup is «not configured» and Drive sync answers `cloud.clientMissing`. Choosing another app disconnects the backup, because the app-data folder belongs to the app that wrote it: with another app the earlier backups and the other devices' sync documents may not be visible. Disconnecting (`nuvola_scollega`) revokes the token and leaves the app registered. The backup has no consent-time client to compare: a client typed in 3.1, carried over by migration 037 as `google-ereditata`, keeps the refresh token it had.",
      ],
      see: ["sync", "profile", "oauth", "cloud-folders"],
    },
    {
      id: "sync",
      title: "Device sync",
      src: ["core/rymembra-sync/src/lib.rs", "core/rymembra-sync/src/documento.rs", "core/rymembra-sync/src/dominio.rs", "core/rymembra-sync/src/fusione.rs", "core/rymembra-sync/src/contatore.rs", "core/rymembra-sync/src/registro.rs", "core/rymembra-sync/src/sequenza.rs", "core/rymembra-sync/src/magazzino.rs", "core/rymembra-sync/src/motore.rs", "core/rymembra-app/src/sincronia.rs", "apps/desktop/src-tauri/src/sincronia.rs", "core/rymembra-sync/tests/convergenza.rs"],
      body: [
        "No server, no ordering guarantees. Each device writes **only its own** document, `dispositivi/rymembra-<id>.v1.json.gz`, and every device merges all of them. The same layout works on a shared folder (Syncthing, OneDrive, Dropbox, a USB stick) and on Drive, and «conflicted copies» cannot occur. State per device, not an operation log: its size is the library's, not the history's, so nothing ever needs compaction.",
        { h: "Four merge shapes" },
        {
          table: {
            head: ["Data", "Shape", "Rule"],
            rows: [
              ["Play counts", "`Contatore`", "Each device only increments its own counter; per device take the max, total = sum (5 plays on the laptop + 3 on the phone = 8). Imported history is the pseudo-device `importazione`"],
              ["Rating, like, last played, active skin", "`Registro` (`Voto`, `Interruttore`, `Momento`, `Scelta`)", "Newest timestamp wins; deterministic tie-breaks (higher rating, liked = OR, max ms, larger id)"],
              ["Playlist order", "`Sequenza`", "Each element has an identity `<device>:<n>` and an «after» pointer; order is a traversal with children by descending identity; concurrent inserts are both kept; removals are tombstones"],
              ["A track this device does not have", "`Sequenza::riordina_lasciando`", "An entry whose track is not in this library is **invisible** here, not removed: `allinea` leaves it alive, in its place, and the document still carries it, so devices that have the track keep it in their playlist (the phone with part of the computer's music, a file deleted or moved out of the folders on one machine). When the track arrives — scan, watcher, «Download here», a correction that changes its key — the next `allinea` (the first step of every pass and of every profile read) puts its entry into `playlist_tracks` at its place, in its own transaction; with sync off that waits for the next profile read or the next time sync is turned on. `allinea` tells «removed by the user» from «not arrived yet» with the keys that were missing at its last run (setting `sincronia.brani_assenti`, rewritten by `allinea` and `applica_in`): the keys named by any entry, alive or tombstoned, that this library lacks, and a key leaves only when its track is back. A missing setting means «nothing remembered» (normal); one that is there and unreadable (`Ricordo::Illeggibile`) makes that pass bury nothing and reattach every live entry of a present track that is not in `playlist_tracks` — the safe side: an entry that comes back, never a track that vanishes from the other devices — and is rewritten at the end of the pass. The same memory covers the opposite case: a remembered track whose file came back and whose row the migration-28 trigger put back, while the sequence has only tombstones for it (it was removed elsewhere meanwhile), is the return of a removed entry, not an addition — `senza_i_ritorni_di_voci_tolte` drops the extra rows, only for that pass and only where the sequence has tombstones for the track; a real addition of exactly that track to exactly that playlist in the same few minutes is dropped too and must be done again (the safe side). Really removing a track from a playlist is still a gesture that travels as a tombstone. A playlist's sequence follows its key: renaming moves the `sync_sequenza` rows (`sposta_la_sequenza`, also used by the external-import rename), and deleting buries them on the spot in `playlists::cancella` (the only production `DELETE FROM playlists`), which «Undo» (`ripristina`) revives, so a playlist deleted and recreated before a pass does not inherit the old entries. Known limit: a track removed from a playlist and then deleted from the library before any `allinea` reads as «file gone», and its entry stays invisible instead of travelling as a removal — nothing in the tables tells the two apart (the migration-28 trigger writes nothing for tracks without a path or a content key)"],
              ["Deletions", "lapidi", "A playlist tombstone removes the playlist only if it is newer than the playlist's last change; track tombstones never delete rows"],
            ],
          },
        },
        "All merges are commutative, associative and idempotent; `tests/convergenza.rs` checks with seeded random histories that any merge order converges and that concurrent playlist edits are not lost. Documents are canonical JSON (BTreeMaps), gzipped, hashed with `blake3`, read up to 64 MiB decompressed and at most 4,000,000 JSON values (counted before the tree is built); bad records are dropped, a newer version is refused. A document that does not parse is remembered by its hash and not re-read until it changes.",
        "Out-of-domain records are dropped per record by `dominio::nel_dominio`, applied to other devices' documents on every pass: ratings above 5, play counts outside 0..=1,000,000,000, and anything dated more than 10 minutes (`MARGINE_FUTURO_MS`) in the future — deferred, not lost: the cached document is merged once its time comes. Sequence elements whose number is not below the element count are dropped on read (their numbers could never come from real edits, and they made new inserts overwrite each other).",
        "A watched folder that a document turns on is left out — of `library.roots` and of the map — when it is a network path (`\\\\host\\…`, `//host/…`, `\\\\?\\UNC\\…`) to a host this PC does not already use in its roots or download folder: no SMB connection to a host chosen by the document's writer. The pass counts them (`Cambiamenti::cartelle_di_rete`) and logs them.",
        { h: "A pass" },
        {
          ol: [
            "Lock window 1: `sincronia::allinea` brings `sync_ascolti`, `sync_sequenza` and `sync.roots` in step with the library — and puts into `playlist_tracks`, at their place, the tracks that arrived after their playlist entry; `contenuto` builds this device's document.",
            "No lock: `Motore::passata` lists the store (`Magazzino`: list, read, write, rename, delete), reads only documents whose hash changed (remembered in `Memoria`), merges with `fusione::fondi`, and rewrites its own document only if its hash changed.",
            "Lock window 2: `sincronia::applica_se_ferma` re-checks that playlists and roots did not change meanwhile (otherwise it rolls back and retries later) and writes counters, registers, playlists (sequence → `playlist_tracks`), roots and tombstones.",
          ],
        },
        "A device that appears in the store is **unknown** until accepted: `sincronia_accoppia` gives it a name and trusts it (`sync_dispositivi`); `sincronia_rinomina` only renames it; untrusted documents are not merged. `sincronia_dimentica` forgets a device without removing the plays it brought. The store is a folder (`sincronia.cartella`) **or** Drive, never both (`sincronia.dove`). Thread `rymembra-sincronia`: first pass after 20 s, every 5 min, 30 s after a burst; switch `sincronia.attiva`.",
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
        "The durable `scrobble_queue` (unique on service, time, artist, title) survives restarts; a failure that says nothing about the listens (network down, service unavailable, authentication) stops the pass without consuming anything, and the queue is retried later; a rejection of the listens themselves is final for the rejected rows, which show as «stuck». A batch rejected that way is split in halves, for both services, down to the entries actually refused. Thread `rymembra-scrobble` runs once at startup, then wakes on demand or every 300 s. «Now playing» is sent on its own short-lived thread and never queued. With `scrobble.attivo` off the queue stops filling, so turning it back on does not send three days of plays at once. «Send the history too» remembers, per service and connected account, how far in `play_history` (by row id) it has already queued, for Spotify and for all history (`scrobble.<service>.cronologia.<account>.<source>` in `settings`): pressing it again queues only what arrived since, an archive imported later included, and forgetting the imported plays lowers the mark. Tokens, keys, secrets and the session live in the keychain; Last.fm's API key moved there from `settings` in 2.3.1.",
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
            "**Requests**: `conversa` streams with a 10 min timeout, `modelli` lists models with 15 s; no redirects at all (a 307 would resend the conversation). Every request carries `max_tokens`: 4096 unless the profile sets another value (`gettoniMassimi`, 64 to 262,144); a 400 asking for `max_completion_tokens` (OpenAI's reasoning models) gets one retry under that name. A reply stops as «cut» (`Motivo::Tagliato`) at 4 MiB of text or 64 MiB read from the network, and once it has started, 120 s without a byte is `net.timeout`. 401/403, unknown model and a refused local connection get their own codes (`ia.unauthorized`, `ia.modelUnknown`, `ia.localServerDown`), and so do an incomplete profile, a missing key, a key written for another address and the 16-profile cap (`ia.profileIncomplete`, `ia.keyMissing`, `ia.keyForOtherOrigin`, `ia.tooManyProfiles`); error details are capped at 300 characters. `Cliente`'s `Debug` is hand-written so the key cannot leak.",
            "**Streaming**: `sse::Sse` parses `data:` events incrementally across chunk boundaries (CR, LF, CRLF; `[DONE]`), with at most 1 MiB of leftover — past it the reply is `ia.badResponse` at once, not after the timeout. The desktop forwards chunks as `ia:pezzo` throttled to 50 ms, then `ia:fine` or `ia:errore`; one conversation at a time (`ia.busy`), stoppable with `ia_ferma`.",
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
      src: ["apps/desktop/src-tauri/src/aggiornamenti.rs", "apps/desktop/src-tauri/src/mobile/aggiornamenti.rs", "apps/desktop/src-tauri/src/rilasci.rs", "apps/desktop/src-tauri/tauri.conf.json", "apps/desktop/src-tauri/installer/precedente.nsh", "apps/desktop/src-tauri/src/comandi.rs", "apps/desktop/src/parti/Aggiornamenti.tsx", "strumenti/manifesto.js", "strumenti/msix.js"],
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
        "Only on explicit request (`aggiornamenti_installa`; a double click is guarded). The download runs on `rymembra-aggiornamento` with progress events every 200 ms; it is canceled after 60 s without a byte (`net.timeout`) or by `aggiornamenti_annulla`. The plugin verifies the **minisign** signature against the key compiled into the executable, and since 3.0 `requireSignedVersion` also requires the signature to be bound to the version the manifest announces. Playback is paused, the NSIS installer runs in passive mode, and `on_before_exit` runs the normal shutdown sequence ([Startup](#startup)) before the process exits. A build compiled without signing keys refuses to download.",
        "**On Android** (`src/mobile/aggiornamenti.rs`, rules in `rilasci.rs`) the phone reads `releases/latest` from the GitHub API and announces a release only if it carries an APK for its architecture **and** that APK's minisign signature (`<apk>.apk.sig`), both as GitHub release assets; the notes are the `notes` of the same release's `latest.json` (only if its `version` matches), never the release body. On «Update» the signature is fetched first (16 KiB cap), then the APK (redirects only to GitHub's asset hosts), and the file on disk is verified — streamed, before every hand-off, cached copy included — against `plugins.updater.pubkey`, with the trusted-comment `version:` required to equal the tag; on failure the file is deleted. Only then `Installatore.kt` checks package and signing certificate. A copy signed with a different Android key than the release certificate (`IMPRONTA_DEL_RILASCIO`, read once via `Installatore.firmatari`) never downloads: the banner shows `update.otherKey` with the export → uninstall → install → import steps instead of «Update»; a version Android rejected for the key is remembered in `aggiornamenti.rifiutata` and not downloaded again.",
        "The NSIS installer also takes care of the previous version, Aether (`installer/precedente.nsh`, hooked in through `bundle.windows.nsis.installerHooks`): before copying anything it silently uninstalls it, leaving its data folder alone — the data are picked up at the first start ([Data](#data)). Only if it is really Aether: the uninstall entry must say publisher «Federico Baratti» and name «Aether», its folder must be the one in `HKCU\\Software\\Federico Baratti\\Aether`, and `uninstall.exe` and `aether.exe` must be there; anything else called «Aether» is left alone, with no fallback folder. If the previous version is still running, the passive/silent installer waits a few seconds for it to close and a graphical one asks the user to close it; if it stays open the installer goes on and leaves the old version installed.",
        "The installer and the uninstaller speak English and Italian (`bundle.windows.nsis.languages`), chosen from the Windows display language, English otherwise; the hook's own messages are `LangString`s in both. The uninstaller's «Delete the application data» removes `%APPDATA%\\io.github.federicobaratti.rymembra` (library, history, playlists, covers, skins, log), `%LOCALAPPDATA%\\io.github.federicobaratti.rymembra` (WebView data, cloud cache) and `HKCU\\Software\\Federico Baratti\\Rymembra`. It never touches Windows Credential Manager — the tokens of connected accounts and the device identity stay there until you disconnect them in Rymembra — nor `%LOCALAPPDATA%\\io.github.federicobaratti.aether`, the music, the lyrics or the exported profiles; without the box (and in `/P` or `/S`, where the box is never shown) the data stay too.",
        { h: "Logs and documents" },
        "The same Settings card opens the log folder (`diario_apri`; see [Logs](#logs)) and, through `apri_documento`, a closed list of public pages in the system browser — repository, issues, license, third-party notices, privacy, terms, donations, and the three developer consoles where the user registers their apps (`console-microsoft`, `console-google`, `console-dropbox`; see [Cloud folders](#cloud-folders)) — never in the webview. The license, the third-party notices, privacy and terms are read first from the copy that ships with the app (`documento_locale`: next to the executable on the computer, from `bundle.resources`; `assets/legale/` in the APK, read by `Ponte` with a closed list of names) and shown as plain text in a dialog, so they can be read offline; only without that copy does the button open the website, and the dialog keeps a button for the latest version online.",
      ],
      see: ["signing", "logs", "concepts"],
    },
  ],
});
