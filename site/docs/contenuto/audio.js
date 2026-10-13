/* Capitolo 5 — il motore audio: struttura, decodifica, uscita, gapless e dissolvenza, livelli, EQ, spettro, analisi. */
(window.AETHER_DOCS = window.AETHER_DOCS || []).push({
  id: "audio",
  title: "The audio engine",
  lead: "`aether-play`: one engine, three roles, lock-free rings, sample-exact track changes, loudness levelling, a 10-band EQ, the spectrum, and the background analysis it powers.",
  entries: [
    {
      id: "engine",
      title: "Engine structure",
      src: ["core/aether-play/src/lib.rs", "core/aether-play/src/motore.rs", "core/aether-play/src/uscita.rs"],
      body: [
        "`aether-play` knows nothing about libraries or paths: it receives a `Sorgente` (bytes behind a `Read + Seek`, an extension, a duration, a level) and makes it audible. Dependencies: symphonia 0.6.1 (mp3, flac, isomp4/aac/alac, ogg/vorbis, wav/pcm, aiff, mkv, caf), `opus-pure` 0.2.1, cpal 0.18.2, rubato 0.16, ebur128 0.1.10, rtrb 0.3.",
        { h: "Three roles" },
        {
          table: {
            head: ["Role", "Thread", "Owns"],
            rows: [
              ["Commander", "any (`Motore` handle)", "Sends `Comando`s over a channel: `Suona`, `Prepara`, `Ferma`, `VaiA`, `Sfuma`, `AnnullaSfuma`, `Volume`, `ReplayGain`, `Dissolvenza`, `Latenza`, `Equalizzatore`, `Chiudi`"],
              ["Decoder", "`aether-decodifica`", "symphonia readers, resampler, channel matrix, per-track gain, crossfade; fills the sample ring; tracks position"],
              ["Callback", "cpal (WASAPI, real-time)", "Pops samples, applies EQ, spectrum tap, volume ramp, gate, limiter; converts to the device format"],
            ],
          },
        },
        "Announcements (`Evento::Iniziato`, `Fermato`, `Errore`) are delivered on a separate thread, `aether-eventi`, so whatever the listener does (database writes, locks) never stalls decoding. The decoder runs inside `catch_unwind` (`filo_sorvegliato`): a panic in symphonia on a malformed file increments a session counter, raises the «lost» flag with cause 3 and announces `playback.stalled` instead of leaving a silent engine.",
        { h: "Three rings, no shared lock" },
        "Between decoder and callback there are three lock-free `rtrb` rings: **samples** (interleaved `f32`), **EQ coefficients** (a whole curve published at once, never half of two), and **spectrum taps** back to the reader. Everything else is atomics in `Condiviso`: `in_pausa`, `svuota` (flush handshake: raised by the decoder, lowered by the callback after emptying), `guadagno`, `fotogrammi` (frames delivered to the device buffer), `vuoti` (underrun samples), `latenza_fotogrammi`, `perso` + `causa_perdita` (1 device gone, 2 backend failure, 3 decoder crashed; the cause is written before the flag), `abbandonato` (a replaced engine stays silent), `spettro`. The only `Mutex` is `in_lettura`, the cancel flag of a network read, which the callback never touches.",
        "`RISERVA_MS` = 200 ms is the guaranteed minimum buffered ahead; the ring is sized for 192 kHz × 8 channels, so at 48 kHz stereo it holds about 3 s.",
        { h: "Position: three delays" },
        "The decoder runs ahead of what is heard. Between the two sit (1) our ring, counted exactly by the callback (`suonati`); (2) the device buffer, measured from cpal timestamps on every block (`annota_latenza`, ignored above `LATENZA_MASSIMA_MS` = 250 ms); (3) the output chain — mixer, driver, DAC, Bluetooth radio — which nothing measures and the user declares in `audio.latenza_ms` (−500…+500 ms). `uditi` = `suonati` − (2) − (3) drives what is reported (current track, position, lyrics); raw `suonati` decides only the end of a track, so latency compensation never delays it.",
        "A `Segno { da, offset_ms, ascolto, formato }` marks the output frame at which a track's samples start and where in the track that is. When `uditi` reaches `da`, that track **is** playing: no event, no reopen — the same mechanism makes gapless exact.",
        { h: "The decode loop" },
        "Each turn: drain pending commands; `compatta` keeps only the last `VaiA` and drops seeks followed by `Suona`/`Ferma` (dragging the scrubber sends many); execute; `riempi` (decode into the ring in chunks); step the EQ fade (`PASSI_EQ` = 10 steps of ≈ 4 ms, interpolated in dB); step the sleep fade; `aggiorna` (position, segments, end). Idle sleep is 4 ms. Pause and resume bypass the queue: `Motore::pausa` writes `in_pausa` directly so the button is instantly true.",
        "`suona` order matters: wake the output, bump the request counter, clear the wanted pause, **raise the cancel flag** of any network read in progress, then send `Suona`. The decoder flushes (`scarta_in_volo`: raise `svuota`, wait up to 100 × 1 ms for the callback to empty the ring behind a closed gate), places a `Segno` at frame 0 and only then lowers the pause — no leftover of the previous track is heard.",
      ],
      see: ["decoding", "output", "crossfade", "player"],
    },
    {
      id: "decoding",
      title: "Decoding, resampling, channels and seeking",
      src: ["core/aether-play/src/decodifica.rs", "core/aether-play/src/decodifica/ricampionatore.rs", "core/aether-play/src/decodifica/canali.rs", "core/aether-play/src/decodifica/opus.rs"],
      body: [
        "`Decodificatore` promises three things: the exact number of output frames (`ceil(n × out / in)`), channels routed by position, and seeks that land on the exact frame.",
        {
          ul: [
            "**Formats**: mp3, flac, m4a, aac, ogg, oga, wav, aiff, aif, opus. WMA is recognised and refused (`Codec::NonSupportato`); Matroska and CAF decode but are not offered because `lofty` cannot read their tags. Gapless trimming (encoder delay/padding) is always on. Rates 8–384 kHz, up to 32 channels.",
            "**Opus**: `decodifica::opus` wraps `opus-pure` (the only pure-Rust decoder that passed a 15-file test set) and applies by hand what symphonia reads but ignores: output gain (RFC 7845 §5.1), pre-skip, channel mapping family and Vorbis channel order. Always 48 kHz.",
            "**Resampling** (`ricampionatore`): rubato `FftFixedIn`, input chunks of `BLOCCO` = 1024. The resampler's output delay is discarded and the tail is flushed exactly, which removed ≈ 26 ms of gaps per seam. Consecutive tracks of the same shape **share one resampler** across the seam (`passa_la_coda`), so a 44.1 kHz album on a 48 kHz device joins without the Gibbs ripple or phase step of two independent resamplers.",
            "**Channels** (`canali::Matrice`): routed by speaker position from symphonia's masks; missing speakers fall back per ITU-R BS.775 (centre and surrounds at −3 dB, LFE dropped); the matrix is normalized by its worst row; mono goes to both fronts at full level.",
            "**Seeking**: `cerca` uses `SeekMode::Accurate` locally and `Coarse` on a remote stream, with an 80 ms pre-roll (`RINCORSA_MS`) for codecs that need it (MP1/2/3, AAC, Vorbis, Opus); targets are capped `MARGINE_FINE_MS` = 120 ms before the declared end. Errors classify as aborted (`internal.aborted`, silent), network (`fs.networkUnavailable`), past the end (treated as track end), or `playback.decodeFailed`.",
          ],
        },
        "`Guasto` keeps the raw OS error that symphonia would drop, so a dead share is reported as a network failure, not «unsupported format». `FormatoSuonato` (codec, sample rate, bit depth for lossless, channels, output rate, average kbps, remote) travels with the `Segno` to feed the format line — see [Playback](#playback).",
      ],
      see: ["engine", "streaming"],
    },
    {
      id: "output",
      title: "Output devices, stream sleep and latency",
      src: ["core/aether-play/src/uscita.rs", "core/aether-play/src/dispositivi.rs", "apps/desktop/src-tauri/src/riproduzione/uscite.rs", "apps/desktop/src-tauri/src/riproduzione/riapertura.rs", "apps/desktop/src/parti/UscitaAudio.tsx"],
      body: [
        { h: "The callback" },
        "Rules: no allocation, no locks, no panics. Per block: count the call, drain the coefficient ring, read flags and gain; per frame: move the **gate** (`DISSOLVENZA_MS` = 12 ms open/close, so pause, resume and seeks never click); when paused with the gate closed, write zeros without consuming the ring; otherwise volume ramp (≈ 40 ms) → pop sample → EQ (skipped when flat) → spectrum tap (channel average) → × gain × gate → `limita` (only when the gain or the EQ can exceed full scale) → format conversion (f32, f64, i32, i24, i16, u16, all clamped). Underruns are counted in `vuoti`.",
        { h: "Stream sleep" },
        "A WASAPI stream that stays open keeps a power request alive («An audio stream is currently in use») and Windows never sleeps. The thread `aether-uscita` therefore pauses the cpal stream (`IAudioClient::Stop`) after `RIPOSO_DOPO` = 30 s of pause, after `RIPOSO_A_VUOTO` = 1 s when paused with nothing left to play, and after `RIPOSO_BLUETOOTH` = 1 s on Bluetooth — so multipoint headphones are released to the phone at once. HDMI/optical receivers keep the 30 s, because they swallow the start of music when re-locking. It never sleeps with a flush pending. `Uscita::sveglia` restarts the stream and waits (≤ 500 ms) for the first callback before any command that needs it.",
        { h: "Device identity and choice" },
        "Devices are identified by their WASAPI friendly name; the default is detected by endpoint id. `player.output` stores the chosen name (absent = follow the system default). `dispositivi::scegli` (opening) falls back to the default if the wanted device is missing; `scegli_per_riaprire` (reopening while playing) does **not** — a pinned device that disappears means wait for it, not «play from the office speakers».",
        { h: "Losing a device" },
        {
          ol: [
            "The cpal error callback maps `DeviceNotAvailable`, `StreamInvalidated`, `HostUnavailable` to cause 1 (others 2; xruns and `DeviceChanged` are ignored) and raises `perso`.",
            "Within 250 ms the clock thread records a resume point (track, ms, was playing, time) and emits `riproduzione:audio` once.",
            "`aether-dispositivi` polls the device list every 2 s (backing off ×2 up to 30 s after failed reopens). With a target available it calls `riapri_su`: a new `Motore` is opened **outside** the lock (`APERTURA_USCITA` = 20 s), then swapped in under it with volume, EQ, ReplayGain, crossfade, latency and spectrum re-applied; the listen in progress is paused, not lost.",
            "`riprendi_dov_era` restarts the track at the saved millisecond only if the bookmark is fresh (`RIPRESA_FRESCA` = 30 min) and music was playing, and not if the decoder-crash counter grew since (to avoid re-seating a file that crashes the decoder).",
          ],
        },
        "Modes of `riapri_su`: `Automatica` (the device watcher: the pinned device or nothing, music resumes as it was), `AMano` («Reopen», or a new output chosen in Settings: falls back if the pinned one is missing), `Suona` (play pressed while the device is lost: falls back and plays — the gesture wins over the state saved at the failure), `SoloDispositivo` (a track is about to start by itself: only a live device is needed). When the pinned device returns, the watcher moves the engine back to it without losing the position. With no device pinned, a change of the Windows default is followed.",
        { h: "Latency" },
        "The measured device-buffer latency plus `audio.latenza_ms` shift only the **reported** position (lyrics, scrubber, SMTC), never the sound or the track change. On Bluetooth the radio dominates, which is why the setting exists.",
      ],
      see: ["engine", "player", "lyrics"],
    },
    {
      id: "crossfade",
      title: "Gapless and crossfade",
      src: ["core/aether-play/src/motore.rs", "core/aether-play/src/lib.rs", "core/aether-play/src/decodifica/ricampionatore.rs", "apps/desktop/src-tauri/src/riproduzione/fili.rs"],
      body: [
        "Both rely on having the next track **already open**: the desktop thread `aether-preparatore` opens it while the current one plays (15 ms debounce) and hands it over with `Motore::prepara`, only if the queue's next item and the output format are unchanged.",
        { h: "Gapless" },
        "When the current decoder ends, `passa_al_prossimo_dopo(rinvio)` starts the prepared one and places its `Segno` at the frame where everything already pushed ends — plus the resampler tail inherited from the previous track when they share a resampler. No event, no reopen, no silence. The finished track is kept as `uscente` until its last sample is heard, so a seek in the final seconds lands on the track actually being heard. With crossfade on, gapless does not apply, even between tracks of the same album.",
        { h: "Crossfade" },
        "`player.crossfade` = 0–`CROSSFADE_MASSIMO_S` = 12 s. `forse_dissolvi` starts the overlap when the remaining frames of the current track (from the database duration, frozen at start) are ≤ the crossfade length; the prepared track moves from `prossimo` to `entrante`, freeing `prossimo` for the track after. Mixing happens in the decoder thread, where both decoders exist — the sample ring has no track boundaries and the callback cannot hold a second decoder.",
        {
          ul: [
            "`dissolvi` uses an **equal-power** curve (cos/sin of the same angle, per frame, identical for all channels): two uncorrelated tracks summed with linear ramps would dip to ≈ 70 % power in the middle.",
            "`coda_entrante` keeps exact sample alignment between codecs with different block sizes.",
            "The incoming track's `Segno` is placed at the **middle** of the overlap, with `offset_ms` = half the overlap, so the window, scrobble and lyrics switch when the new track becomes the one heard.",
            "If the outgoing file ends early (a VBR MP3 without a Xing header misdeclares its duration), `risali` brings the incoming track from its partial gain to full in `RIPRESA_MS` = 40 ms instead of jumping (a click).",
            "ReplayGain is applied per block in the decoder, so two overlapping tracks each keep their own correction.",
          ],
        },
      ],
      see: ["playback", "engine", "sleep"],
    },
    {
      id: "playback",
      title: "Loudness levelling, volume and the format line",
      src: ["core/aether-play/src/lib.rs", "core/aether-play/src/intensita.rs", "core/aether-app/src/playback.rs", "core/aether-app/src/intensita.rs", "apps/desktop/src-tauri/src/riproduzione/suono.rs", "apps/desktop/src/parti/Formato.tsx"],
      body: [
        "Every track carries a `Livello { guadagno_db, picco }`: the dB needed to reach a −18 LUFS reference, and the linear true peak. The gain comes from the ReplayGain tag when present, otherwise from Aether's own EBU R128 measurement in `track_intensita`; the peak only from the measurement.",
        { h: "Targets and modes" },
        {
          table: {
            head: ["Setting", "Target", "Use"],
            rows: [
              ["off", "—", "No correction"],
              ["low (`basso`)", "`BERSAGLIO_BASSO_DB` = −23 dB", "Night, headphones"],
              ["normal", "`BERSAGLIO_PREDEFINITO_DB` = −18 dB", "ReplayGain reference"],
              ["high (`alto`)", "`BERSAGLIO_ALTO_DB` = −14 dB", "Streaming-platform level"],
            ],
          },
        },
        "`ModoLivello`: `brano` (each track to the target), `album` (all tracks of an album shifted by the album gain — tag, else duration-weighted mean of measured energy — keeping the mastering's dynamics), `automatico` (default: album when the queue is not shuffled and a neighbour belongs to the same album, otherwise track). Stored in `player.replaygain`; takes effect from the next track.",
        { h: "The maths" },
        {
          ul: [
            "`correzione`: shift the tag's dB by `target + 18`, clamp to −24…+12 dB, and when **raising** cap the linear gain at `1 / peak` (never below 1) so normalization alone never pushes a track over full scale.",
            "`limita`: a soft limiter in the callback — identity up to −1 dBFS (`GINOCCHIO` = 0.8912509), then `tanh` towards full scale — for what the cap cannot see: EQ boosts, unmeasured tracks, a −14 target on loud masters. Continuous with its derivative, so no added harmonics below the knee.",
            "`volume_uscita`: the slider position **cubed** — half way is −18 dB, 0.8 is −6 dB — so each part of the travel changes loudness by a similar perceived amount. Stored volumes from 2.x (linear gain) are converted once with a cube root by `load_volume`.",
          ],
        },
        { h: "Measuring loudness" },
        "`aether_play::intensita` decodes the whole file at 48 kHz stereo through the same chain as playback and runs `ebur128` for integrated loudness and 4× oversampled true peak; nothing above −70 LUFS is `Muto`. The background pass is described in [Analysis](#analysis); `normalizzazione_avanzamento` reports how many tracks are measured.",
        { h: "The format line" },
        "With `player.fileFormat.visible` on, the player shows «FLAC · 44.1 kHz · 24-bit · Stereo · 1058 kbps» **as the decoder reports it** while playing (`FormatoSuonato` → `FormatoFile::completato_da`): the real codec inside an `.m4a`, the MP3 that an Internet Archive «FLAC» page actually streams. It describes the file, not the output; when the device resamples, the tooltip says «Resampled from 44.1 kHz to 48 kHz».",
      ],
      see: ["eq", "analysis", "crossfade", "accent", "autoplay"],
    },
    {
      id: "eq",
      title: "Equalizer",
      src: ["core/aether-play/src/equalizzatore.rs", "apps/desktop/src-tauri/src/riproduzione/suono.rs", "apps/desktop/src/parti/Equalizzatore.tsx"],
      body: [
        "Ten octave bands centred at 31.25, 62.5, 125, 250, 500 Hz, 1, 2, 4, 8 and 16 kHz (`CENTRI_HZ`), ±`LIMITE_DB` = 12 dB each. Octaves rather than equal-width bands because hearing is logarithmic.",
        {
          ul: [
            "**Filters**: RBJ peaking biquads with Q = √2, computed in `f64`, transposed direct form II, state for up to 8 channels × 10 bands. A band within ±0.05 dB is flat and skipped; bands at or above 0.45 × sample rate are disabled. Filter memory resets on a flat→active transition and on flush.",
            "**Automatic preamp**: the cascade's magnitude response is evaluated on 89 points (8 per octave from 31.25 Hz) and the curve is scaled by `1 / peak`, so raising several bands does not clip.",
            "**Where**: in the audio **callback**, not in the decoder, so a slider is heard while dragging rather than 200 ms later. New coefficients travel through their own ring as a whole curve; the decoder thread interpolates between curves in dB over 10 steps (≈ 40 ms) to avoid clicks when loading a preset.",
            "**Presets**: 9 built in (`PRESET_DI_SERIE`: flat, rock, pop, vocal, bass, acoustic, electronic, classical, night) plus your own in `player.eq.presets` (opaque JSON); commands `eq_preset_elenco`, `eq_preset_salva`, `eq_preset_cancella`. The current curve is `player.eq`; changes emit `riproduzione:eq`.",
          ],
        },
        "The spectrum is tapped **after** the EQ, so curve changes are visible in the bars and volume changes are not.",
      ],
      see: ["spectrum", "playback"],
    },
    {
      id: "spectrum",
      title: "Spectrum: analysis and the 3D scene",
      src: ["core/aether-play/src/spettro.rs", "apps/desktop/src-tauri/src/riproduzione/fili.rs", "apps/desktop/src/parti/Spettro3D.tsx", "apps/desktop/src/parti/SchedaSpettro.tsx", "apps/desktop/src/parti/DettaglioSpettro.tsx"],
      body: [
        "The callback averages channels into the spectrum ring **only while someone is looking** (`Condiviso::spettro`); the ring holds 32,768 samples and drops when full rather than waiting. `Spettro` (behind a mutex on the `Motore`, never touched by the callback) runs a hand-written radix-2 FFT over a 4,096-sample Hann window (≈ 11.7 Hz per bin at 48 kHz), floor −70 dB.",
        "Two reductions: octave bands (smoothing rise 0.55, fall 0.12) and fine log-spaced bands from 20 Hz to 20 kHz at a chosen resolution (`RISOLUZIONI` = 8, 16, 32, 64, 128, 256, 512, 1024; default 64), normalized to 64 bands so bar height does not depend on resolution, interpolated below bin width. The desktop thread `aether-spettro` sends `riproduzione:spettro` (`{ fini: u8[] }`) every 33 ms while visible, polling at 250 ms otherwise.",
        { h: "The scene" },
        "`Spettro3D.tsx` is a WebGL2 bar field: up to 1,024 bars wide with about 30 s of history (`MEMORIA_MS`), sampled every 33 ms. History is a cascade of rings, each level max-pooling two-to-one so depth is logarithmic in time; boxes are instanced (`gl_InstanceID`, up to `SCATOLE` = 32,768) with heights in a one-byte-per-band texture — one or two draw calls (the reflection is optional, `PIANI_RIFLESSI` = 3 levels). Eighteen `canvas.viz.*` skin tokens are read from CSS variables, refreshed by a mutation observer, not per frame.",
        "Quality (`player.spectrum.quality`): `alta`, `bassa` or `auto`, which watches the 95th-percentile frame delay over a 2 s window and changes density (2 / 1.5 / 1) with a 5 s dwell that doubles on each reversal up to 60 s. Quality is a fact of this machine and does not travel in a profile; `player.spectrum.visible` (start with the scene on) and `player.spectrum.bands` do. The Studio feeds the same component from a fake source.",
      ],
      see: ["eq", "skin"],
    },
    {
      id: "analysis",
      title: "Background analysis: fingerprints and loudness",
      src: ["apps/desktop/src-tauri/src/analisi.rs", "core/aether-play/src/impronta.rs", "core/aether-play/src/intensita.rs", "core/aether-app/src/sonora.rs", "core/aether-app/src/intensita.rs"],
      body: [
        "`aether-analisi` is the only thread that reads files at full speed without being asked, so it yields to playback: first pass 2 min after start, then every 20 min (2 min burst after a scan), and if music is playing it backs off for `RESPIRO` = 30 s. It never takes the player lock; the library lock only to read a batch and to write results.",
        { h: "Sonic fingerprints" },
        "`impronta` takes 30 s from the middle of a track (at least 10 s), decodes at 22,050 Hz mono and computes **47 numbers**: 13 MFCC means and variances over 26 mel bands (40 Hz–10 kHz), spectral centroid, rolloff, flatness and flux, RMS, dynamic range, crest, silence share, tempo, beat strength, onset rate and chroma (362–5,000 Hz). `VERSIONE_ESTRATTORE` = 1. Batches of 64 (`sonora::LOTTO`) on 3 threads with a 25 s deadline per track; outcomes `ok`, `illeggibile`, `muto`, `corto`; failures retried after 30 days. Once at least 50 tracks are measured, a normalization scale (`impronta_scala`) is computed. Fingerprints feed [Autoplay](#autoplay) and the weekly collections.",
        { h: "Loudness" },
        "After fingerprints, `intensita` measures integrated loudness and true peak for tracks without them — a **full** decode — in batches of 16 on 2 threads. The deadline is 25 s plus duration / 20 (`VELOCITA_MINIMA` = 20× real time: slower means the share is not answering). Results go to `track_intensita`; the gain is derived when read (`−18 − LUFS`), so changing the target never needs a new measurement. Album values: duration-weighted mean of energy, peak = maximum.",
      ],
      see: ["playback", "autoplay", "weekly"],
    },
  ],
});
