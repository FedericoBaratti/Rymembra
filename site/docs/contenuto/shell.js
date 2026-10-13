/* Capitolo 13 — la shell desktop: avvio e uscita, IPC, vassoio, confini di sicurezza, diario. */
(window.AETHER_DOCS = window.AETHER_DOCS || []).push({
  id: "shell",
  title: "The desktop shell",
  lead: "`aether-desktop`, the Tauri 2 app: the startup order, the IPC surface, closing to the tray, the security boundaries between the page and the core, and the log.",
  entries: [
    {
      id: "startup",
      title: "Startup and shutdown",
      src: ["apps/desktop/src-tauri/src/main.rs", "apps/desktop/src-tauri/src/spegnimento.rs", "apps/desktop/src-tauri/src/stato.rs", "apps/desktop/src-tauri/tauri.conf.json"],
      body: [
        "Before anything else `main` installs the panic hook that writes panics to the log. Plugins: single-instance (skipped when `AETHER_DATI` is set), dialog, opener (with `open_js_links_on_click(false)`: no injected script opens `target=_blank` links behind the core's back), updater (no permission for the page).",
        { h: "Setup order" },
        {
          ol: [
            "mark the UI thread (for `con_libreria`), resolve the data folder (`AETHER_DATI` or `%APPDATA%\\io.github.federicobaratti.aether`), open the log;",
            "register lightweight states that open nothing yet: imports, Explore, account, procurement, lyrics, the Folders index;",
            "open the library (`Stato::apri`) and write the startup line (`dati=… migrazioni=… fts5=… giornale=…`);",
            "register SMTC and taskbar-button states, start the player (opens the output device, restores yesterday's queue), the preparer, clock, spectrum and device-watcher threads, then SMTC and taskbar buttons;",
            "backup, sync, enrichment, maintenance, analysis, scrobbling, AI, updates — each with its own start delay so they do not contend with the first seconds of use;",
            "the tray, the first state to SMTC/tray/taskbar, the folder watcher, and the 2 s safety net that shows the window if the frontend never calls `pronto`.",
          ],
        },
        "Window: 1280×820 (min 880×560), no decorations (the title bar is drawn by the page; drag, minimize, maximize, full screen and close are app commands, so `core:window` permissions are not granted), `backgroundColor` = `plain`'s, created hidden.",
        { h: "Shutdown" },
        "`App::run` observes `CloseRequested`, `ExitRequested` and `Exit`; with «close to tray» on, a close only hides. `spegnimento::spegni` (also run by the updater's `on_before_exit`) is idempotent: raise the static exit flag, stop a scan, stop the folder watchers, save the queue/volume/EQ/position (≤ 400 ms lock wait), the last `PRAGMA optimize` (≤ 250 ms), detach SMTC, remove the tray icon. The flag exists because events posted after `tao`'s event loop is destroyed made it panic (`cannot move state from Destroyed`): every emit goes through `Emette::emetti`, which is silent once exiting, and every background loop checks the flag first.",
      ],
      see: ["runtime", "ipc", "background", "logs"],
    },
    {
      id: "ipc",
      title: "IPC: commands, events, URI schemes",
      src: ["apps/desktop/src-tauri/src/main.rs", "apps/desktop/src-tauri/src/comandi.rs", "apps/desktop/src-tauri/src/errore.rs", "apps/desktop/src/ipc.ts"],
      body: [
        "**262 commands** are registered in one `generate_handler!` list. Commands are thin: they take the right lock, call the core, map errors with `errore()`. By area:",
        {
          table: {
            head: ["Area", "Commands", "Area", "Commands"],
            rows: [
              ["Playback and queue", "46", "Library core (`comandi.rs`)", "38"],
              ["Skin Studio", "19", "Playlists", "17"],
              ["Lyrics", "16", "Scrobbling", "12"],
              ["Explore", "9", "Skins", "9"],
              ["Drive backup", "9", "AI", "8"],
              ["Window", "7", "Device sync", "7"],
              ["Procurement", "7", "Link import", "6"],
              ["Spotify archive", "6", "Folders panel", "6"],
              ["Updates", "6", "Diagnostics", "6"],
              ["Metadata review", "5", "Profile", "4"],
              ["Enrichment", "4", "Tray", "3"],
              ["Deletion", "3", "Zoom", "3"],
              ["Watched folders", "2", "Tour", "2"],
              ["Drag and drop", "1", "Views", "1"],
            ],
          },
        },
        "Anything that may wait on a disk or that reads a lot runs on the blocking pool ([Runtime](#runtime)). Commands that change what the backup covers end with `nuvola::se_riuscito`, which wakes the backup.",
        { h: "Events (37 names)" },
        {
          table: {
            head: ["Prefix", "Events"],
            rows: [
              ["`riproduzione:`", "`stato`, `tempo`, `spettro`, `audio`, `errore`, `dispositivi`, `volume`, `eq`"],
              ["`coda:`", "`sostituita`"],
              ["`scansione:`, `cartelle:`", "`avanzamento`, `finita`; `cambiate`"],
              ["`scarico:`", "`brano`, `avanzamento`, `guasto`, `finito`, `in_libreria`, `riconciliato`"],
              ["`import:`, `profilo:`", "`avanzamento`"],
              ["`arricchimento:`", "`esito`, `avanzamento`, `stato`"],
              ["`nuvola:`, `sincronia:`", "`avanzamento`, `stato`"],
              ["`scrobble:`, `manutenzione:`", "`passata`; `salute`"],
              ["`testi:`", "`avanzamento`, `guasto`, `finito`"],
              ["`aggiornamenti:`", "`stato`, `avanzamento`"],
              ["`ia:`", "`pezzo`, `fine`, `errore`"],
            ],
          },
        },
        { h: "URI schemes" },
        "Two asynchronous custom schemes serve bytes the page asks for as ordinary resources, never through JSON: `aether-cover` ([Covers](#covers)) and `aether-skin` ([Packages](#skin-package)). WebView2 exposes them as `http://<scheme>.localhost/…`. Both accept only validated names, never paths.",
      ],
      see: ["frontend", "errors", "security"],
    },
    {
      id: "background",
      title: "Closing to the tray",
      src: ["apps/desktop/src-tauri/src/vassoio.rs", "apps/desktop/src-tauri/src/main.rs", "core/aether-app/src/preferenze.rs"],
      body: [
        "With `ui.closeToTray` on (default off), the close button **hides** the window instead of exiting: playback continues, because music never lived in the page — it is all in `aether-play`'s threads; the page has no `<audio>`. Minimize is unchanged. `on_window_event(CloseRequested)` prevents the close and hides; `App::run` then skips the shutdown sequence for that close.",
        "The tray icon is the way back, so `vassoio::nasconde` hides only if the icon **actually exists** — if the system refused it, or the menu labels have not arrived yet, the X closes as usual. Labels come from the window (`vassoio_lingua`) so they are translated with everything else. The menu has show, play/pause, next, previous and exit; the tooltip shows the current track (≤ 100 characters). «Exit» raises the exit flag before calling `exit`, so the close is not prevented. A second launch of Aether brings this window back (`vassoio::mostra`) instead of starting a second process.",
      ],
      see: ["startup", "os-media"],
    },
    {
      id: "security",
      title: "Security boundaries",
      src: ["apps/desktop/src-tauri/tauri.conf.json", "apps/desktop/src-tauri/capabilities/default.json", "apps/desktop/src-tauri/src/destinazioni.rs", "apps/desktop/src-tauri/src/copertine.rs", "apps/desktop/src-tauri/src/skin.rs", "apps/desktop/aether-barra/src/finestra.rs", "SECURITY.md"],
      body: [
        "The page touches more third-party text than any other part (tags, shared skins, catalog answers), so it is treated as untrusted. Four fences, each independent of the others:",
        {
          ul: [
            "**CSP**: `default-src 'self'`; images from self, `data:`, `aether-cover` and `aether-skin`; fonts from self and `aether-skin`; `script-src 'self'`; `connect-src` only IPC and `aether-skin`; `object-src`, `base-uri`, `form-action`, `frame-ancestors` all `'none'`. The asset protocol is disabled. Fonts are bundled; no remote resource can load.",
            "**Capabilities**: a closed list of five permissions — listen/unlisten to events, `is-minimized`, the open and save dialogs. No `core:default`, no opener, no `core:window`, no updater.",
            "**Write targets** (`destinazioni.rs`): the four commands that write where the page says (playlist export, skin export, profile export, sync folder) require an absolute drive or UNC path (not `\\\\.\\` or `\\\\?\\GLOBALROOT`), no `..` and no `:` in components (NTFS alternate streams), and the format's extension (`m3u8`/`m3u`/`pls`/`xspf`, `aeskin`, `aeprofile`), else `fs.extensionRefused`; for folders only the last level may be created.",
            "**Reads**: the cover scheme accepts only a hash, the skin scheme only a checked id and a name from the package's closed list, typed by magic bytes. Documents and the docs open in the **system browser** from closed lists (`apri_documento`, `apri_guida` with `ANCORE_GUIDA`), never from a URL the page composes.",
          ],
        },
        "Inside the core: untrusted archives (`.aeskin`, `.aeprofile`, the Spotify zip) use closed name lists and limits checked before decompressing; images have decode limits; HTTP follows redirects only through declared gates; secrets live in the keychain and are never written into errors. `unsafe` is forbidden in every crate except `aether-barra`, where it is confined to `finestra.rs` with a justification on each block (`undocumented_unsafe_blocks` is denied).",
      ],
      see: ["skin-package", "profile", "http", "oauth"],
    },
    {
      id: "logs",
      title: "The log",
      src: ["apps/desktop/src-tauri/src/diario.rs", "apps/desktop/src-tauri/src/errore.rs"],
      body: [
        "`diario.rs` writes plain lines to `<data>/diario/aether.log`, rotated at `TETTO` = 2 MiB into three files (`QUANTI` = 3). Lines use the `nota!` macro and the form `[module] what happened`, so the first bracket says whether the failure is the player's, the network's or the database's. No `log`/`tracing`: thirty lines of text do not need subscribers and levels.",
        {
          ul: [
            "**What goes in**: every error that reaches the window (code, severity, retryability, cause — written by `errore::errore`), the startup line, thread panics with their location, slow-lock warnings.",
            "**What never goes in**: library paths, titles, artist names. User folders in any line are rewritten to `%APPDATA%`, `%LOCALAPPDATA%`, `%USERPROFILE%` (`senza_utente`), so the file can be attached to a report.",
            "Writing never fails the app: errors are ignored (a full disk must not stop the music). The frontend's uncaught errors arrive through `diario_annota`; `diario_apri` opens the folder.",
          ],
        },
      ],
      see: ["errors", "update"],
    },
  ],
});
