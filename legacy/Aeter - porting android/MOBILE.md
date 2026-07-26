# Aether — porting Android 16

Porting dell'app desktop Electron su **Android 16 (API 36)** tramite **Capacitor**
(renderer React in WebView) + **nodejs-mobile** (il "main process" Node rieseguito
su ARM). Massimo riuso del codice: tutto `src/` e gran parte di `electron/modules`
sono condivisi tra desktop e mobile.

## Architettura

```
WebView (http://localhost)                 nodejs-mobile (Node ARM)
  src/ renderer React                         node-backend/main.ts
  window.aether ──(NodeBackend plugin)──►     electron/* riusati via shim
        ▲  audio/cover http://127.0.0.1:<port>  ├─ DB (better-sqlite3)
        └──────────────────────────────────────┤─ server HTTP locale (range)
                                                └─ spawn binari ARM (jniLibs)
Plugin Kotlin: NodeBackend (+ futuri FileAccess SAF, ImageResize, SecureStore, MediaSession)
```

- **Bridge**: `src/lib/bridge.ts` ricrea `window.aether` (come l'ex `electron/preload.ts`)
  mandando messaggi JSON `{id,channel,args}` al plugin `NodeBackend` e correlando le risposte.
  Gli eventi backend→renderer (`aether:event`) arrivano come messaggi push.
- **Shim electron**: `node-backend/electron-shim.ts` rimpiazza il modulo `electron`
  (alias in `vite.config.node-backend.ts`) così `electron/ipc/*` e `electron/modules/*`
  girano invariati. Mappa `app.getPath`, `ipcMain.handle`, `webContents.send`, `net.fetch`,
  `safeStorage`, `dialog`, `shell`, ecc.
- **Server media**: `node-backend/server.ts` sostituisce il protocollo `aether://`
  (`/media/:id` con Range/206, `/art/:hash`, `/remote`). `src/lib/format.ts` punta a
  `http://127.0.0.1:<port>/…` via `setMediaBase()` (default desktop resta `aether://`).
- **DB**: il build node-backend aliasa **better-sqlite3 → `node-backend/sqlite-shim.ts`**,
  un adapter sincrono su **sql.js (SQLite WASM)**: stessa API (`prepare/run/get/all/
  transaction/pragma`), nessun modulo nativo da compilare. Schema/migrazioni invariati
  in `electron/modules/db.ts`. FTS5 è assente in sql.js: su Android `search()` usa il
  fallback **`afold()` + match multi-token** (funzione SQL registrata in `db.ts`,
  condivisa con il renderer via `shared/text.ts`) — ricerca insensibile ad accenti e
  maiuscole, con AND su tutti i token, vicina alla qualità FTS5 del desktop.
- **Binari**: `electron/modules/binaries.ts` su `process.platform === 'android'` cerca
  `lib*.so` in `nativeLibraryDir` (passato via env `AETHER_NATIVE_LIB_DIR`).

## Build

> ⚠️ **Buildare su Linux/WSL2.** Servono Android SDK 36 + NDK e **JDK 17**.
> Nota: il backend usa **sql.js (WASM)** al posto di better-sqlite3 (vedi
> `node-backend/sqlite-shim.ts`), quindi **non c'è alcun modulo nativo da
> cross-compilare** per arm64 (`BUILD_NATIVE_MODULES.txt = 0`). Il vincolo Linux
> resta per la toolchain Android/NDK e per i binari ARM (yt-dlp/ffmpeg/fpcalc).

```bash
npm install
npm run cap:sync       # build:mobile + build:node + install deps node + cap sync + fix layout www
npm run android:open   # apre Android Studio
# poi: ./gradlew assembleDebug  (o Run da Android Studio)
```

`cap:sync` esegue, in ordine: `build:mobile` (renderer → `dist-mobile/`),
`build:node` (backend → `dist-node/main.js`), `node-project:install`
(installa `better-sqlite3` in `nodejs-assets/nodejs-project`), `cap sync android`,
e `scripts/sync-android-node-project.mjs` (vedi sotto).

`npm run typecheck` resta verde su desktop e mobile.

### Fix layout www (nodejs-mobile-cordova ↔ Capacitor)

nodejs-mobile-cordova è un plugin **Cordova**: sia il suo Gradle sia il runtime
cercano `src/main/assets/www/nodejs-project` **dentro il sottoprogetto generato
`capacitor-cordova-android-plugins`** (Capacitor usa `assets/public`, non `www`).
`scripts/sync-android-node-project.mjs` copia `nodejs-assets/nodejs-project` (con
`node_modules/better-sqlite3` da ricompilare) in quella posizione **dopo ogni
`cap sync`** (gli asset del sottoprogetto vengono uniti nell'APK → a runtime il
progetto si trova in `assets/www/nodejs-project`). Va rieseguito dopo ogni `cap sync`
manuale (è già incluso in `npm run cap:sync`).

## Requisiti toolchain

- **JDK 17** (Capacitor 7 / AGP 8 / `compileSdk 36`). Java 8 NON funziona. In Android Studio:
  *Settings → Build Tools → Gradle → Gradle JDK = jbr-17* (il JDK incluso). Non serve disinstallare altri JDK.
- **Android SDK 36** + **NDK** (per ricompilare i moduli nativi su arm64).

## nodejs-mobile: approccio scelto

Si usa **`nodejs-mobile-cordova`** (NON `nodejs-mobile-react-native`, che funziona solo in React Native).
Capacitor integra i plugin Cordova automaticamente — `cap sync` riporta
`Found 1 Cordova plugin for android: nodejs-mobile-cordova`. Il plugin fornisce il prebuilt
`libnode.so` + JNI, espone `window.nodejs.channel` al renderer (gestito da `src/lib/bridge.ts`,
transport `cordovaTransport`) e lato Node usa `cordova-bridge` (già gestito in `node-backend/main.ts`).

Il progetto Node vive in `nodejs-assets/nodejs-project/` (`main.js` = bundle di `npm run build:node`,
copiato automaticamente). `nodejs-assets/BUILD_NATIVE_MODULES.txt = 1` fa ricompilare i moduli nativi
per arm64 al build Gradle.

> Il plugin custom `NodeBackendPlugin`/`NodeRuntime` (Kotlin) resta come **alternativa** (raw AAR
> nodejs-mobile + JNI glue). NON è registrato in `MainActivity` per non caricare `libnode` due volte.

## Reverse-RPC backend → nativo

Il backend Node non raggiunge il nativo direttamente: `callNative(method, args)`
(`node-backend/runtime.ts`) emette un messaggio `nrpc`, il renderer
(`src/lib/bridge.ts` → `src/lib/nativeRpc.ts`) invoca il plugin Capacitor e
risponde con `nres`. Usato da: `pickFolder`/`showInFolder`/`openExternal`
(FileAccess), `imageResize` (ImageResize), `getSecureKey` (SecureStore).

## Plugin nativi (sorgente in repo, da compilare su macchina build)

I plugin sono registrati in `MainActivity.onCreate` e usano deps androidx
aggiunte in `android/app/build.gradle` (`media`, `browser`, `security-crypto`).

- **FileAccess** (`FileAccessPlugin.kt`): `ACTION_OPEN_DOCUMENT_TREE` + permesso
  persistente; risolve il **path reale** della cartella (storage primario) per il
  backend path-based; `showInFolder`/`openExternal` via Intent/Custom Tabs.
  **Rischio**: SD/USB non risolvibili a path → fallback I/O SAF; lettura audio
  governata da `READ_MEDIA_AUDIO`.
- **ImageResize** (`ImageResizePlugin.kt`): decode → cover-crop → WebP (sostituisce
  il passthrough di `sharp-shim.ts`); replica `fit:'cover'` di coverArt.ts.
- **SecureStore** (`SecureStorePlugin.kt`): chiave AES-256 in
  EncryptedSharedPreferences; il backend la legge a boot e `safeStorage` cifra i
  secret con AES-256-GCM sincrono (`node-backend/electron-shim.ts`).
- **MediaSession** (`MediaSessionPlugin.kt` + `MediaPlaybackService.kt`): notifica
  + lock screen + controlli, foreground service per il playback in background;
  pilotato da `src/lib/mediaSession.ts` (abbonato a `usePlayerStore`).

## Step rimanenti puramente build-machine (Android SDK + NDK)

### Binari ARM — checklist WSL/Linux

**yt-dlp e ffmpeg NON vanno più spediti come `.so`**: il binario ufficiale
yt-dlp è un freeze PyInstaller/glibc che non gira su bionic. Entrambi sono
forniti dalla libreria **youtubedl-android** (plugin `YtDlpPlugin`, dipendenze
`io.github.junkfood02.youtubedl-android` in `app/build.gradle`), pilotata via
reverse-RPC (`ytdlpRun`). L'unico binario che passa ancora dalla rotta
`jniLibs` è **fpcalc** (fingerprint AcoustID).

`electron/modules/binaries.ts` su Android cerca `lib<name>.so` (senza trattini)
in `AETHER_NATIVE_LIB_DIR` = la `nativeLibraryDir` dell'app. **Come la dir
arriva al backend**: `MainActivity.onCreate` esporta
`getApplicationInfo().nativeLibraryDir` con
`Os.setenv("AETHER_NATIVE_LIB_DIR", …)` — lo stesso meccanismo che il plugin
cordova usa per `TMPDIR`; node gira nello stesso processo, quindi la vede in
`process.env` da prima che l'engine parta (`deriveHostConfig()` in
`node-backend/main.ts` la raccoglie da lì). I `.so` messi in
`android/app/src/main/jniLibs/arm64-v8a/` vengono estratti in quella dir
all'installazione (il manifest ha già `extractNativeLibs="true"`).
Mappatura nome → file attesa (vedi `binPath()`):

| binario | file atteso       | uso |
|---------|-------------------|-----|
| fpcalc  | `libfpcalc.so`    | fingerprint AcoustID (arricchimento) |
| yt-dlp  | —                 | **via plugin youtubedl-android**, non jniLibs |
| ffmpeg  | —                 | **via modulo FFmpeg di youtubedl-android** |
| spotdl  | `libspotdl.so`    | **omesso** (vedi sotto) |

Procedura per fpcalc (eseguire in WSL, ABI `arm64-v8a` perché
`android/app/build.gradle` filtra solo quella):

```bash
mkdir -p android/app/src/main/jniLibs/arm64-v8a

# fpcalc (Chromaprint) arm64: compilare chromaprint per aarch64 STATICO per
# Android/bionic (NDK; un build glibc "linux aarch64" NON gira), oppure usare
# un build bionic esistente:
cp /path/fpcalc-arm64-android    android/app/src/main/jniLibs/arm64-v8a/libfpcalc.so

# rendere eseguibile (getBinaries() fa comunque chmod 0755 a runtime)
chmod 0755 android/app/src/main/jniLibs/arm64-v8a/libfpcalc.so
```

Verifica post-build: nell'app, *Impostazioni → Strumenti esterni* mostra
`fpcalc` come *trovato* (deriva da `getBinaryStatus()`, che legge la stessa
`AETHER_NATIVE_LIB_DIR`); l'arricchimento via fingerprint deve completare.
Senza il `.so` tutto degrada come oggi: metadati/cover keyless via testo
(MusicBrainz + iTunes + Deezer), nessun fingerprint.

> **spotdl**: intenzionalmente non portato. È un'app Python con molte dipendenze e
> non esiste un singolo binario arm64 affidabile; i link Spotify degradano già a
> ricerca yt-dlp (`download/sources/spotify.ts`), quindi la funzionalità resta
> coperta senza un runtime Python sul device. `getBinaryStatus` lo segnala assente.

- Le dipendenze JS (`music-metadata`, `node-taglib-sharp`, `chokidar`, `p-queue`,
  `zod`) sono **bundlate** in `main.js` (`ssr.noExternal`): nessun modulo nativo.
- **Compilazione/run APK**: `npm run cap:sync && npm run android:open`, poi
  `./gradlew assembleDebug` (compila i plugin Kotlin sopra).

## UI touch (fatta, lato renderer)

`isMobile` (`src/lib/platform.ts`, via define `__AETHER_MOBILE__`) attiva:
bottom nav (`BottomNav.tsx`) al posto della Sidebar, context menu → long-press
bottom-sheet (`useLongPress.ts` in `TrackList.tsx`), player compatto +
`NowPlaying.tsx` espandibile, QueueDrawer full-width, safe-area insets e niente
titlebar/drag-region (CSS `html[data-mobile]`), scorciatoie tastiera no-op.

## File chiave aggiunti

- `capacitor.config.ts`, `vite.config.mobile.ts`, `index.mobile.html`, `src/main.mobile.tsx`
- `src/lib/{bridge,nativeRpc,mediaSession,platform}.ts`, modifiche a `src/lib/format.ts`
- UI: `src/components/layout/BottomNav.tsx`, `src/components/player/NowPlaying.tsx`, `src/hooks/useLongPress.ts`
- `node-backend/{runtime,electron-shim,server,main,sharp-shim,sqlite-shim}.ts`, `vite.config.node-backend.ts`
- `android/app/src/main/java/com/aether/player/{NodeBackendPlugin,NodeRuntime,FileAccessPlugin,ImageResizePlugin,SecureStorePlugin,MediaSessionPlugin,MediaPlaybackService}.kt`
- `android/app/src/main/assets/nodejs-project/package.json`

## Android Auto + Gemini (2026-07)

Aether è un'app media Android Auto completa, con integrazione vocale
Gemini/Assistant. Gemini (rollout su Auto da aprile 2026) instrada i comandi
media attraverso il framework standard MediaSession/MediaBrowser: l'integrazione
"perfetta" consiste nell'implementare bene tutti quegli hook, non in un'API
Gemini dedicata (AppFunctions di Android 16 è in private preview Google,
inaccessibile a terzi).

### Architettura

```
Node backend (electron/modules/auto/autoCatalog*.ts)
  └─ scrive <filesDir>/auto/catalog.json (snapshot browse: recent/liked/album/artist/playlist)
AetherMediaBrowserService  ← si binda dall'auto anche ad app FREDDA (niente WebView/Node)
  ├─ browse tree + ricerca raggruppata (AutoCatalogNative legge lo snapshot)
  ├─ root EXTRA_RECENT → voce "continua ad ascoltare" (AutoResumeStore)
  └─ sessionToken → MediaSessionCompat condivisa (MediaSessionPlugin.Holder)
AetherSessionCallback (voce/transport)
  ├─ onPlayFromSearch/onPrepareFromSearch → AutoVoice.resolve (focus extras di
  │   Gemini: artista/album/playlist/brano; query vuota → resume/recenti/piaciuti;
  │   nessun match → STATE_ERROR letto ad alta voce)
  ├─ onSetShuffleMode/onSetRepeatMode, onSkipToQueueItem, onPrepare (resume)
  └─ AetherAuto: warm → ExoPlayer diretto (playAuto); cold → launch MainActivity
NativeAudioPlugin
  ├─ playAuto: coda file-path dal catalogo (EQ/crossfade/RG pieni) + startPosition
  ├─ pubblica la coda nella sessione (setQueue, display "in coda" dell'auto)
  └─ persiste il resume (track + posizione) su ogni transizione/pausa
```

Voce lato telefono: `res/xml/shortcuts.xml` (capability `actions.intent.PLAY_MUSIC`)
→ `VoicePlayActivity` (no-UI). **Limite**: Google attiva le App Actions solo per
app distribuite via Play Store; su sideload il file è inerte (testabile con
l'App Actions Test Tool di Android Studio). In auto la voce funziona comunque
(percorso MediaBrowser); sul telefono Gemini controlla la sessione attiva.

### Comandi vocali supportati (in auto)

- "Riproduci <brano/artista/album/playlist> su Aether" (focus extras o freeform)
- "Metti musica" (query vuota → resume → recenti → piaciuti)
- "Riprendi la musica" (onPrepare/EXTRA_RECENT, anche ad app fredda, riparte
  dalla posizione salvata dentro il suo album/playlist)
- "Riproduci i miei preferiti / i brani piaciuti / i recenti" (alias IT/EN)
- "Attiva riproduzione casuale", "ripeti", pause/next/previous/seek
- Nessun match → Gemini legge "Nessun risultato per …" (STATE_ERROR)

Non supportato: generi musicali (il catalogo non ha il campo genere → fallback
a ricerca libera sui brani).

### Test / requisiti

- Sideload: in Android Auto → impostazioni sviluppatore → **"Sorgenti
  sconosciute"**, altrimenti l'app non appare nel launcher dell'auto.
- Emulazione senza auto: **DHU** (Desktop Head Unit, `sdk/extras/google/auto/`):
  telefono in modalità head-unit server + `desktop-head-unit.exe`.
- Il catalogo si rigenera (debounced) su library:changed/like/playlist mentre il
  backend gira; a backend spento l'auto naviga l'ultimo snapshot.
- Log utili: `adb logcat -s MediaSessionCompat AetherAuto MediaBrowserService`.

## Gestione termica (2026-07)

Sistema anti-surriscaldamento a tre strati; senza API termica (API < 29) tutto
resta a piena velocità (assenza di dati = `normal`).

- **Nativo**: `ThermalMonitor.kt` mappa `PowerManager.THERMAL_STATUS_*` →
  `normal` (NONE/LIGHT) / `warning` (MODERATE) / `critical` (SEVERE+);
  `ThermalPlugin.kt` emette `thermalchanged` solo sulle transizioni.
  `NativeAudioPlugin` a caldo porta il tick `timeupdate` da 4 Hz a 1 Hz e a
  `critical` salta la preparazione del crossfade (niente secondo ExoPlayer).
- **Renderer**: `src/lib/thermal.ts` inoltra i campioni al backend via
  `thermalUpdate` (no-op in modalità LAN); seed iniziale con `getState()`.
- **Backend**: `electron/modules/adaptiveConcurrency.ts` (`thermalManager`)
  ritara le code: scan 8→4→1, auto-enrich 3→2→1 (live), maintenance a inizio
  passata; a `critical` `autoEnrichMissing` non parte proprio. Fallback a
  `normal` dopo 120 s senza campioni freschi. Evento `thermal:changed` verso
  il renderer. Identico sul desktop (mai chiamato → inerte).
- **Verifica su device senza scaldarlo**: `adb shell cmd thermalservice
  override-status 3` (SEVERE → critical), `2` (MODERATE → warning), `0` reset;
  logcat tag `ThermalMonitor` + log backend `[thermal]`.

## Pagine da 16 KB e libnode.so — stato (2026-07)

Il prebuilt `libnode.so` (nodejs-mobile-cordova, Node 12, 2019) è allineato a
**4 KB**: su Android 15 con pagine da 16 KB il linker lo rifiuta (crash all'avvio);
su **Android 16** carica comunque grazie a `android:pageSizeCompat="enabled"`
nel manifest (backcompat automatico dell'OS). Mitigazioni attive:

- `-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON` in `android/app/build.gradle` →
  la lib CMake-built (`libnodejs-mobile-cordova-native-lib.so`) è 16 KB-clean.
- `scripts/preflight-android.mjs` → `checkNativeAlignment()` parsa l'ELF:
  warning (non bloccante) finché il prebuilt resta a 4 KB.

**Perché non è ancora risolto alla radice** (verificato 2026-07-15): non esiste
alcuna release nodejs-mobile con `libnode.so` allineato a 16 KB — l'ultima è la
v18.20.4 (ott 2024), senza menzione del flag. L'upgrade non sarebbe comunque uno
swap: significherebbe passare il backend da Node 12 a Node 18 (shim, `check:node12`,
API) e aggiornare in coppia la native-lib Cordova (ABI/NAPI). Strade future:
release nodejs-mobile nuova con NDK r28+, oppure rilink del prebuilt da sorgente
con `-Wl,-z,max-page-size=16384` (build lunga, su WSL).
