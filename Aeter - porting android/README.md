# Aether

Player musicale desktop con libreria locale, download da YouTube/Spotify e arricchimento automatico dei metadati. Electron + React + TypeScript + Vite, SQLite (better-sqlite3), Tailwind CSS v4, Howler + Web Audio API.

## Funzionalità

- **Libreria**: scansione ricorsiva delle cartelle monitorate (chokidar), formati MP3/FLAC/M4A/OGG/WAV/AIFF/OPUS/WMA, lista virtualizzata (100k+ tracce), viste Album/Artisti/Playlist, ricerca full-text (FTS5) con `Ctrl+F`.
- **Player**: scrubber con waveform, coda con drag-and-drop, shuffle/repeat, gapless, crossfade 0–12s, ReplayGain, equalizzatore a 10 bande con preset, velocità 0.5–2x, sleep timer con fadeout, visualizzatore fullscreen (spettro circolare) con testi sincronizzati da lrclib.net.
- **Design**: tema scuro con accento dinamico estratto dalla copertina in riproduzione, materiali traslucidi, grana animata.
- **Downloader**: incolla un link YouTube o Spotify, anteprima con conferma, coda con download paralleli configurabili (1–5), qualità MP3 320/FLAC/AAC 256.
- **Metadati**: pipeline AcoustID → MusicBrainz → Cover Art Archive → Last.fm in background, editor manuale col tasto destro, rilevamento duplicati nelle Impostazioni.
- **Integrazione OS**: tasti multimediali (SMTC/MediaSession), notifiche al cambio traccia, i18n italiano/inglese.

## Sviluppo

```bash
npm install            # se better-sqlite3 fallisce: npm install --ignore-scripts && npx electron-builder install-app-deps
npm run dev            # avvia l'app in sviluppo (HMR sul renderer)
npm run typecheck      # controllo TypeScript (node + web)
```

## Binari esterni

I binari vanno in `resources/bin/` e vengono impacchettati come `extraResources`:

| Binario | Uso | Fonte |
|---|---|---|
| `yt-dlp.exe` | download YouTube | github.com/yt-dlp/yt-dlp |
| `spotdl.exe` | download Spotify | github.com/spotDL/spotify-downloader (standalone) |
| `ffmpeg.exe` | estrazione/conversione audio | ffmpeg.org |
| `fpcalc.exe` | fingerprint AcoustID | acoustid.org/chromaprint |

Su macOS/Linux usare i binari equivalenti senza estensione (`yt-dlp`, `spotdl`, `ffmpeg`, `fpcalc`).

## Chiavi API (opzionali, in Impostazioni → Integrazioni)

- **Spotify client ID/secret**: anteprime ricche dei link Spotify (senza chiavi viene usato l'oEmbed pubblico; spotdl funziona comunque).
- **AcoustID API key**: identificazione tramite fingerprint audio.
- **Last.fm API key**: generi/tag automatici.

## Build di distribuzione

```bash
npm run build
npx electron-builder --win    # NSIS + portable in release/
npx electron-builder --mac    # DMG
npx electron-builder --linux  # AppImage + deb
```

Il database e le impostazioni vivono in `%APPDATA%/aether` (o equivalente per piattaforma).

## Android (porting in corso)

Porting su **Android 16 (API 36)** via **Capacitor** (renderer React in WebView) +
**nodejs-mobile** (il backend Node rieseguito su ARM, riusando `electron/modules`).
La parte JS/web è completa e verificata; restano gli step nativi (engine nodejs-mobile,
moduli nativi cross-compilati, binari ARM, plugin SAF/MediaSession). Vedi **`MOBILE.md`**.

```bash
npm run cap:sync     # build renderer (dist-mobile) + backend (dist-node) + cap sync android
npm run android:open # apre Android Studio (richiede Android SDK/NDK)
```
