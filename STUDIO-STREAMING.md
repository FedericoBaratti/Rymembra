# Study: streaming as a library

An analysis of the list [chayotic/Open-Source-Music-Streaming-Apps](https://github.com/chayotic/Open-Source-Music-Streaming-Apps)
and a proposal for how to bring the experience of those apps into Aether — a
library that unites local music and streaming — **without violating the
conditions written in `TERMS.md` and without betraying the program's logic**.

This is a study document: it doesn't describe existing code, it proposes a road.
Last updated: 1 September 2026.

---

## 1. What that list is, and what can be taken from it

The repository is a **curated list**, not an application: around fifty free
programs for listening to music, split by platform (Android, iOS, desktop, web)
and by nature (online streaming clients versus offline players). The list is
published under **CC0**, so its content — the list itself, the classification —
is freely reusable.

There is, however, no «code library» to import. What the list offers Aether is
something else:

1. **A catalog of experience patterns**: how those apps make search, immediate
   listening, queue and personal collection live together.
2. **A compliance test bench**: for each one you can ask «could Aether do it
   this way?» — and the answer, almost always, lights up exactly the boundary
   `TERMS.md` draws.

---

## 2. The triage: how those apps actually stream

The list doesn't say so, but opening the repositories is enough to see it: the
great majority of the *streaming clients* listed take their audio from YouTube
or YouTube Music through the web player's internal APIs, or by leaning on
alternative frontends (Piped, Invidious) that do the same thing one step
further out.

| Group | Examples from the list | How they get the audio | Verdict for Aether |
|---|---|---|---|
| YouTube Music clients | SimpMusic, ViTune, Metrolist, Muzza, Kreate, RiPlay, N-Zik, VIVI Music, Gyawun, Music-You… | YouTube's internal APIs (extracting audio streams from the player) | **Excluded.** `TERMS.md` §3: the YouTube API Developer Policies forbid downloading (§ III.E.1.a), separating audio from video (§ III.I.7) and playing from a player that isn't visible (§ III.I.9). A music player with a library is exactly what those rules rule out. |
| Metadata+audio hybrids | Spotube, Musify, BloomeeTunes, Nuclear, muffon, Pear Desktop | Metadata from Spotify/Last.fm/JioSaavn, audio from YouTube/Piped and similar | **Excluded** for the audio part, same reason. The metadata part isn't needed: Aether already has MusicBrainz, Deezer, iTunes and Cover Art Archive in `core/aether-meta`. |
| Clients for your own server | **Supersonic** (OpenSubsonic protocol: Navidrome, Gonic, Airsonic…) | HTTP streaming from the user's **own** authenticated instance, of the user's **own** music | **Compliant and interesting.** Nobody else's rights in play: the server is the user's, the files are theirs. It's the only streaming pattern in the list importable to the letter. See phase 4. |
| Offline players | Auxio, Namida, Harmonoid, Nora, Tauon Music Box, Retro Music Player, Gramophone… | No streaming: local files | **Already Aether's territory.** Useful only as an interface comparison (library navigation, queues, artist/album views). |
| Downloader tools | yt-dlp, SpotiFLAC | Direct extraction from commercial services | **Excluded and actively blocked**: CI (`verify.yml`, job `niente-di-vietato`) fails the build if those names come back into the code. Naming them here is fine — the check covers `*.rs`/`*.ts`/`*.tsx`, not documents — and this is the right place to explain why they stay out. |

The triage's conclusion is clear-cut: **the list's dominant model («free» audio
from YouTube) is the thing Aether refuses by constitution.** What can be
integrated is the *experience* — search, press play, have everything in a single
library — built on top of the sources Aether already considers lawful, plus at
most one new source (your own server) that touches nobody else's rights.

---

## 3. What Aether already has

The surprising part of the study is how much of the road is already built. The
seams exist, documented and tested; what's missing is the thread that joins
them.

- **The engine already accepts a stream, not a file.**
  `aether_play::Sorgente` carries `media: Box<dyn Flusso>` — readable, seekable
  bytes, from wherever they come (`core/aether-play/src/decodifica.rs`).
- **The HTTP stream exists, is tested, and since 2.2.0 is wired up.**
  `FlussoHttp` (`core/aether-net/src/flusso.rs`) reads a remote file with
  sliding-window `Range` requests, implements `Read + Seek`, and by design never
  touches the disk (Jamendo's terms forbid caching).
  `apps/desktop/src-tauri/src/riproduzione.rs` now builds one when a track's
  path turns out to be a catalog address: that is phase 1 below, done.
- **The vocabulary of sources is already license-proof.**
  `Fonte`, `Licenza`, `Disponibilita` in `core/aether-domain/src/esterno.rs`:
  every external track declares where it comes from, under what license, and
  whether it can be downloaded (`Scaricabile`), only listened to (`SoloAscolto`)
  or only bought (`SoloAcquisto`). `Licenza::Sconosciuta` does not permit
  copying: doubt counts as a no.
- **The gate is before the network.** `core/aether-catalogo/src/prelievo.rs`
  refuses to fetch a candidate that can't be kept *before* any HTTP request —
  the principle of `TERMS.md` §3 turned into code.
- **The sources are a closed list, not a plugin system.**
  `Cataloghi` (`core/aether-catalogo/src/lib.rs`) composes concrete structs
  (Internet Archive, Audius, Jamendo behind a disabled feature); URL recognition
  (`riferimento.rs`) is a rigid domain allowlist. Adding a source is a
  compiler-guided change, and it's meant to be.
- **The gap is declared in the README**, and what is left of it after 2.2.0 is
  the shape of the library, not the playback: a track that isn't a file has no
  place of its own (`tracks.path` is `NOT NULL UNIQUE`, so the address has to
  borrow the column meant for a path) and there is no surface for finding such
  tracks. Those are phases 2 and 3.

One architectural constraint to respect in every phase: `aether-app` (the
library, SQLite) **does not depend** on `aether-net`, and `aether-catalogo`
never sees a `rusqlite::Connection`. The place where a network stream can be
assembled and handed to the engine is `apps/desktop/src-tauri`, which depends on
all four.

---

## 4. The compliant roadmap: «streaming as a library»

Four phases, ordered by return/risk ratio. The first two close the gap declared
in the README; the third builds the streaming-app experience; the fourth is the
only true import from the list.

### Phase 1 — Making what is silent today play — done in 2.2.0

*Listen-only tracks from Audius and Internet Archive become playable.*

In `apps/desktop/src-tauri/src/riproduzione.rs`, where opening a track meant
opening a file and nothing else, there is now a branch: `riferimento_di()` asks
whether the path is in fact a catalog address, `aether_catalogo::riconosci`
answers — the same rigid allowlist that gates every other link, rather than a
second copy of it — and a `FlussoHttp` is built over that catalog's `Rete` and
handed to `motore.suona()` like any other `Sorgente`. Gapless, crossfade,
equalizer, spectrum: it all works the same, because the engine never knew what
was behind the bytes.

It reads `tracks.path` and not the `fonte_url` this study assumed, because that
column arrives with phase 2: until then the address travels in the column meant
for a path. Everything downstream of `riferimento_di()` receives a reference and
does not know which column it came out of, so phase 2 changes that one function
and nothing else.

- **New obligations:** none. The sources are the same, the terms too; if
  anything the listen/own distinction is *honored* better, because nothing gets
  saved.
- **Points watched:** handling network errors mid-track (the
  `riproduzione:errore` channel already existed); the gapless prefetch
  (`prepara_prossimo`) opens the connection ahead of time — the per-service rate
  limit already present in `aether-net` is respected, and the stream is opened
  with the catalog's own `Rete` for exactly that reason.
- **Effort:** contained, as expected. It was wiring, not building.

### Phase 2 — A place in the library for a track that isn't a file

*The library stops presuming every track has a path.*

Today a track's identity is its path (`tracks.path NOT NULL UNIQUE`, migration
`001_baseline.sql`) and `playback::sorgente`
(`core/aether-app/src/playback.rs`) does `SELECT path`. What's needed is a
house-style migration (`core/aether-app/src/db/schema/019_….sql`, consecutive
numbering verified by the tests) that makes the path optional and puts alongside
it the fields already known to `desiderati` from migration 10: `fonte_url`,
`licenza`, `disponibilita`. A track is *either* a file *or* a catalog
reference, never neither.

- **Respecting the boundaries:** `aether-app` continues not to see the network;
  it only has to be able to say «this track opens like this» by returning the
  reference, and whoever holds both the library and the network (`src-tauri`)
  assembles the stream. Alternatively, a `Flusso` factory injected next to
  `MusicFiles` — the same scheme with which `files.rs` prepares the way for
  Android.
- **Knock-on effects to verify:** scanning and moved-file detection in
  `library.rs` (they must ignore pathless tracks), statistics and history (they
  work by id, not by path: they should hold), device-to-device sync (a reference
  track travels well: it's only a row, not a file), on-disk file reorganization
  (it must skip them).
- **Effort:** the most delicate of the four phases. This is where the «as if
  there were a library» this study asks for really lives.

### Phase 3 — «Explore»: the streaming-app surface

*Search the free catalogs from the app, listen at once, keep what you may.*

This is the piece of experience borrowed from the apps in the list, applied to
lawful sources. A new entry in `Vista`
(`apps/desktop/src/parti/Navigazione.tsx`), Tauri commands exposing
`Cataloghi::cerca` to the frontend through `ipc.ts`, results as `Candidato`
(already normalized: title, author, duration, nature, license, availability).

Interface rules that follow from the terms, not from taste:

- every result shows **source and license**, as the queue already does;
- the button is «**Listen**» for everyone; «**Save to library**» appears only if
  `Licenza::permette_copia` — the same gate as in `prelievo.rs`, never
  circumvented by the frontend;
- every track carries the link to its **public page** on the catalog
  (`pagina_url`): the visible attribution some CC licenses require;
- what no catalog has keeps going into «To buy» (§4 of the terms), which is
  Aether's answer to the need the forbidden apps solve badly.

Effort: medium, almost all frontend plus a handful of commands; the catalogs
and the fan-out search already exist (`procura.rs` uses them in batches today).

### Phase 4 (optional) — The source taken from the list: your own server

*OpenSubsonic/Navidrome, in Supersonic's manner.*

The only streaming pattern in the list importable without reservations: the user
points Aether at their **own** instance (Navidrome, Gonic…), and the music they
already have elsewhere joins the same library. Nobody else's rights: their
server, their files, their authentication.

A checklist already implicit in the architecture (the model is the `jamendo`
feature):

1. a `Fonte::Subsonic` variant in `core/aether-domain/src/esterno.rs` with
   `nome`/`etichetta`/`puo_consegnare`;
2. a module in `core/aether-catalogo/` with its own dedicated `Rete` (rate limit
   and circuit breaker for free);
3. recognition in `riferimento.rs` — here not a domain allowlist but the address
   the user configured, and nothing else;
4. a Cargo feature off by default (`aether-catalogo` + `aether-desktop`);
5. a migration for the new `source_service` value (in the style of
   `010_cataloghi.sql`);
6. `ipc.ts`, the `nomeFonte` map, `lingue/it.json` + `lingue/en.json` (CI
   demands parity);
7. credentials in the **system keychain** via `aether-oauth`/keyring, never in
   SQLite — the house rule for secrets.

A note of merit: for classic authentication the Subsonic protocol uses an MD5
salt; OpenSubsonic offers better and Navidrome supports tokens — to be
preferred. And since the files are the user's, here `Disponibilita` can be
`Scaricabile`: downloading from yourself is the cleanest case there is.

Effort: the highest (authentication, configuration, a whole protocol), but
entirely parallel to the other phases.

---

## 5. What stays out, and why it isn't negotiable

It's worth saying without circling: **there is no compliant version of Spotube
inside Aether.** It isn't caution, it's the arithmetic of the terms:

- audio from YouTube is forbidden by the policies cited in `TERMS.md` §3, and
  going through Piped/Invidious doesn't change the substance — it only changes
  who makes the request;
- scraping Spotify is excluded: from Spotify only the GDPR export Spotify hands
  the user is read, and that's already supported (`aether-archivio`);
- the *non commercial* constraint of Live Music Archive and Jamendo (§2) is the
  reason Aether is and stays free: every new source must be read through this
  lens too;
- the forbidden lyrics catalogs (§3-bis) stay forbidden: phase 3 doesn't touch
  lyrics, LRCLIB remains the only road;
- CI enforces all of this mechanically: the `niente-di-vietato` job fails the
  build if downloader tools or excluded lyrics sources come back into the code.
  If a phase of this roadmap seemed to require an exception to those checks, it
  is the phase that's wrong, not the check.

As `TERMS.md` §3 says: if a way is found to make Aether do one of these things,
it's a defect to be reported — and this study introduces none.

---

## 6. In summary

| Phase | What it gives | New legal obligations | Effort |
|---|---|---|---|
| 1. Wire up `FlussoHttp` — **done in 2.2.0** | Listen-only tracks play | None | Low |
| 2. Fileless tracks in the library | Streaming *inside* the library, not beside it | None | Medium-high |
| 3. «Explore» screen | The streaming-app experience, on lawful sources | None (attribution already provided for) | Medium |
| 4. OpenSubsonic (opt.) | Your own remote music in the same library | None (your own music) | High |

Chayotic's list, read to the end, confirms Aether's bet: almost all those apps
buy the experience at the price of violated terms. Aether can have the
experience and keep the terms — because the hard pieces, the engine that plays
bytes and the vocabulary of licenses, are already built.
