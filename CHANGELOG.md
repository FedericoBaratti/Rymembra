# Changelog

Format [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versions
[SemVer](https://semver.org/).

## How versioning works here

One number for the whole monorepo: the root and the six workspaces always
declare the same version, and `npm run check:version` fails `verify` if they
don't. It changes with `npm run version:set -- <version>`, never by hand.

Why one number instead of six: the version ends up in three places the user sees
— the installer's name, `app.getVersion()` in the startup log, and the `version`
the phone reads from `/health` to decide whether the protocol is compatible —
and those three places read it from different `package.json` files. Six
independent numbers are six chances to diverge; in the old tree's `release/`
folder there are still `Aether Setup 0.9.13.7.26.2.exe` and `0.9.14.7.26.2.exe`
sitting next to `0.9.14.7.26.3.exe`, which is this problem in the form of
artifacts.

What increments what:

- **patch** — fixes that change neither the appearance nor the shape of the
  data;
- **minor** — new features, and every database migration (migrations only go
  forward: a minor is the signal that going back requires a restore);
- **major** — an incompatible change to the skin format
  (`SKIN_FORMAT_VERSION`) or to the transport protocol
  (`SKIN_TRANSFER_PROTOCOL`), that is, the two points at which an updated device
  would stop understanding one that stayed put.

## [2.2.0] — 2026-09-05

**Minor and not patch, because of the database.** This version brings two
migrations, `017_affinita` and `018_settimana`, and by the rule at the top of
this file every migration forces a minor. Migrations only go forward: coming
back from here to 2.1.0 is not uninstalling and reinstalling, it is restoring a
copy of the database made before updating.

### Added — affinity: Aether starts to know what your music is made of

When a record ended, Aether proposed the next track through a cascade of five
rules — same album, same artist, same genre, an unplayed favorite, any track at
all — which in its own comment declared of itself: «because it isn't a
recommender». That was true. Now it is one, and the cascade has stayed
underneath as a floor: when there is nothing to reorder on, it answers exactly
as it answered before, and there is a test that verifies it.

The score comes from three independent layers, and each one that is missing
takes itself out of the way without breaking the others:

- **the sound** — a background thread decodes thirty seconds from the middle of
  each track, in mono at 22,050 Hz, and derives forty-seven numbers from it:
  means and variances of the mel-cepstral coefficients, centroid, rolloff,
  flatness and spectral flux, dynamics, tempo, and six transposition-invariant
  chroma magnitudes. All local, offline, and **no byte of audio leaves the
  house**. The 22,050 Hz is not a saving: above 11 kHz a 256 kbps MP3 and its
  FLAC diverge, whereas at that frequency they land on the same point — which in
  a mixed library is the difference between an engine that works and one that
  splits in two over the duplicates;
- **the culture** — «people who listen to this also listen to that», from
  ListenBrainz Labs, whose data is CC0. **Only the neighbors already in the
  library** are kept: a track you don't own isn't a recommendation, it's a shop
  window. A new section 2-ter of `PRIVACY.md` says at length what goes out, when,
  and what that request lets someone glimpse;
- **your listening** — `play_history` knows not only what you heard but for how
  long. Fraction completed, a six-month decay, and a penalty for what has just
  been heard.

The blend lives in `aether-domain`, with no I/O and no clock, and it
**redistributes the weights** instead of summing them: an absent layer doesn't
lower everyone's score, it takes itself out of the reckoning. With no network,
no MusicBrainz identifiers and no analysis done yet, the result stays defined —
three tests say so.

### Added — Monday: the first thing in Aether that changes on its own

The Home had four shelves — Resume, Recent, Added, Neglected — computed in one
query, and the same today as in a month's time if nothing was touched. They were
useful and they were not a reason to reopen the program.

Now at the top there is **Monday**: up to four collections of twenty tracks,
computed locally while the computer is idle, which stay put for seven days and
then become other ones.

- **Fished back** — what you loved and haven't heard for months, with past
  affection multiplied by forgetting, not added to it: a track played forty
  times and untouched for a year beats one played once and untouched for three.
  It is the collection no streaming service can write, because as far as it is
  concerned you own nothing.
- **More like this** — three coherent groups found on the sound fingerprints,
  named after what they actually contain: a genre if one is in the majority, an
  artist as a fallback, and nothing when there is no majority inside — because a
  name that describes two tracks out of ten is worse than no name.

The seed for the grouping is the Monday itself, and nothing else: next week the
collections are different ones without this week's being kept anywhere. What
came out seven days ago doesn't come out again, and after a month the old weeks
throw themselves away.

Two choices are worth declaring. **The collections are written once and not
recomputed**: `Fished back` looks at the last play, which changes precisely by
listening to the collection, and a collection recomputed on every opening would
consume itself while being used. And **the announcement disappears, not the
shelf**: once they have all been opened, Monday stays there like the other
shelves until the following Monday — making it disappear because you looked at
it would mean that opening it is the way to lose it.

No system notifications: `capabilities/default.json` is a closed list, and one
extra permission for a convenience is the wrong direction. Monday lives inside
the program.

The project's third collection — **Outside**, new music from the free catalogs —
isn't there, and the database constraint doesn't even admit its name: the
catalogs Aether knows can answer «do you have this exact track?», not «what
resembles this?».

### Added — «Radio from here», and a queue that says why

There was no entry point to a radio, on any screen. Now the right-click menu on
a track or an album opens **Radio from here**: a queue of thirty tracks seeded
from there, chosen by affinity and not by artist.

It is deliberately different from autoplay. Autoplay continues a **session**,
and its first step is the rest of the record — someone who put an album on wants
to hear the album. A radio continues nothing: it is an explicit gesture, the
record gets no precedence, and the pool is the whole library. With a ceiling of
three tracks per artist, because without one, the tracks closest to a track are
almost always the others on the same record — true, and useless. The ceiling
gives way when there is nothing else: better a monotonous radio than a truncated
one.

And the queue **says why**. Next to the track autoplay has just queued, the
reason appears: «continues the record», «sounds like this one», «people who
listen to this also listen to that», «you haven't heard it for months». No
streaming service does this, and none can: it would have to admit when it is
pushing something. Here there is nothing to push.

The three layers are not on the same scale — sonic similarity is worth 0.5
between two unrelated tracks, proximity is worth zero for almost everyone, taste
is worth zero for what has never been heard — so each has its own threshold, and
above that threshold the winner is whichever contributed most to the score. If
none reaches its own threshold, time remains, which is the most verifiable
explanation of them all; and if there isn't even that, the sentence is the step
of the cascade, which is always honest because it is how the pool was chosen.
The sentence is written by `lingue/`: the core sends a code, so «sounds like this
one» stays English for whoever has the interface in English.

### Added — the first launch reaches as far as the first note

Aether opened onto an empty room saying «add a folder from the settings», while
`%USERPROFILE%\Music` exists on a hundred per cent of Windows installations and
nobody was looking at it. Now the music folders the system declares are counted
one by one and offered **already ticked**: the gesture becomes «Continue». As
soon as the library has something in it, the big button stops saying «Continue»
and says «Listen» — and it remains the only click between installing and sound.
Anyone whose music is on a switched-off NAS skips and gets back the same empty
states as always, which have not been removed.

### Added — listen-only tracks now play

`FlussoHttp` was written, tested and imported by nobody: an Audius track
appeared in the list and didn't start, which is a broken promise displayed by
the interface itself. It is now wired into `riproduzione.rs`, and what reaches
the engine is indistinguishable from a file — identifier, duration, gain,
declared length, and therefore the cursor in its rightful place.

### Fixed

- `estensione_da_url` read the top-level domain as the extension:
  `https://archive.org` suggested an «org» container to the engine.
- Analysis yields the disk to the thread preparing the next track. An analysis
  that contends for the head produces exactly the gap between two tracks that
  2.1.0 had closed — reintroduced by a feature nobody asked for.

### Fixed — the application no longer dies while being closed

In the logs of people using Aether there were eight identical panics, on 2.0.1
and on 2.1.0, always the same: `cannot move state from Destroyed`. It was not a
defect of this version — it was there before — and it was the reason the
application «vanished» instead of closing, leaving behind a four-and-a-half
megabyte `aether.db-wal` that was never reabsorbed.

Background threads send events to the window: the clock four times a second, the
spectrum thirty. Sending an event, on Windows, means leaving a message in a
hidden window's queue. If the message was left an instant before the window was
torn down and delivered an instant after, whoever received it had no graceful
way out: it fell over. And since the release profile asked for
`panic = "abort"`, that fall took the whole process with it, without closing the
database.

Now the exit is a fact the program observes — `main` listens to the loop's
events, which it did not do before — and every emission goes through a single
point that goes quiet as soon as the shutdown begins. The system tray detaches
while the window is still there, instead of after. The release profile has moved
to `panic = "unwind"`: any panic, on any thread, no longer takes the process away
without closing the database — and a malformed file that brought the decoder down
no longer takes the application with it.

### Fixed — the window no longer freezes while you use it

Three places where the **thread that draws the window** was doing work that
wasn't its own, and for all that time the window did not respond:

- **the volume and equalizer sliders** opened a transaction on SQLite for every
  pixel — a dozen a second for as long as the slider stays under your finger —
  holding the player's lock and the library's together. The sound still changes
  immediately; the row on disk is written by the clock thread within a quarter of
  a second, and twelve changes become a single write;
- **the Monday collections** regenerated themselves from scratch on every entry
  into the Home. The mark for «already done» was the rows written, and a library
  that hasn't yet got enough to say writes none of them: for those libraries the
  whole computation — up to two hundred thousand vectors, three passes — was
  redone every time. Now what gets recorded is having run, not having written;
- **the tint taken from the cover art** held the library's lock while it opened
  the cover from disk, read the skin file and analyzed it. Now it holds it for
  the two questions that concern the database and lets it go before touching the
  disk.

### Fixed — various

- The automatic choice of the next track stopped with a database error if the
  current track had disappeared from `tracks` while it was playing — which
  happens when a rescan finds the file renamed or removed. Now it moves on to
  the next criterion, which is what the other steps of the cascade already did.
- A skin dragged into the window could install itself **twice**: the drag
  listener rewrote itself on every reload, and the asynchronous cleanup left two
  listeners alive for an instant. The listeners are now opened only once.
- A lyrics sweep that fell over halfway left the progress bar stuck forever: the
  failure channel had nobody listening to it.

### Changed — the README moves from essay to documentation

The document said the right things in the wrong register. It was written in the
second person, it addressed the reader, and it entrusted technical facts to
aphorisms that illustrated them instead of stating them: about forty em dashes in
three hundred lines, and paragraphs built as arguments rather than as manual
entries. On a music player that asks you to install an unsigned executable, that
tone works against the content — which is, for the most part, verifiable legal
reasoning.

Same content, impersonal register: no fact removed, the citations of YouTube's
policies and article 20 of the GDPR in their place, the em dashes down to six.
Plus the three things a README was missing to be consultable: a table of
contents, a **Requirements** subsection at the head of the build instructions,
and a table of **related documents** linking `CHANGELOG`, `PRIVACY`, `TERMS`,
`SECURITY` and `THIRD-PARTY-NOTICES`, until now reachable only by browsing the
repository root.

### Changed — the documentation is in English

The interface has spoken two languages since 1.0.0. The documents spoke one, and
it was not the language a repository on GitHub is read in. Whoever lands on the
README from a search decides in half a minute whether the program does what they
need, and that decision does not get made in a language one does not have.

`README.md`, `PRIVACY.md`, `TERMS.md`, `SECURITY.md`, the whole of this file
from 0.2.0 onwards, the two issue templates and the release notes in
`release.yml` are now in English, as is the header that
`strumenti/licenze.js` writes at the top of `THIRD-PARTY-NOTICES.md` — a file
regenerated for the occasion, 429 crates and 249 distinct license texts.

It is a replacement and not a second copy. Two parallel documents are two
documents that diverge, and the one that diverges is always the one nobody
rereads. The site is the exception and keeps both languages, because there the
choice belongs to the visitor and the texts are short enough to stay aligned.

Nothing of substance moved in the passage: the citations of YouTube's policies
and of article 20 of the GDPR, the retention windows in `PRIVACY.md`, the
sections of `TERMS.md` are the same statements in another language. What did
change is the Studio screenshot, which showed an Italian interface in the middle
of an English document, and now shows the inspector open on a selected part.

What stays in Italian, and deliberately, is the code: its comments, the file
names, the identifiers, the names of the CI jobs, the eight groups of the part
registry that the Studio prints in its tree. That vocabulary is the domain
language of this project — it is what `riproduzione.rs` and `niente-di-vietato`
are called — and translating half of it would leave a codebase speaking two
languages badly instead of one well.

### Removed — the study on streaming leaves the repository

`STUDIO-STREAMING.md` was a design study: an analysis of a list of free
streaming apps, a triage of how they actually get their audio, and a four-phase
road for bringing that experience into Aether without breaking the terms. It
described no existing code — it proposed one — and a document that proposes is
read as a promise by whoever finds it in the repository root.

Its first phase is done and is written above; the rest was a plan, and a plan
that is not a commitment does not belong next to `LICENSE` and `PRIVACY.md`.
What still needs saying is said where it is checked: the limit is in the README's
known limits, and the reason `riproduzione.rs` reads `tracks.path` and not a
column of its own is in the comment on the function that reads it.

## [2.1.0] — 2026-09-03

**Minor and not patch, because of the database.** This version brings two
migrations, `015_riconcilia` and `016_metadati`, and by the rule at the top of
this file every migration forces a minor. It isn't an accounting detail:
migrations only go forward, so coming back from here to 2.0.1 is not
uninstalling and reinstalling — it is restoring a copy of the database made
before updating. The number is the only place where that difference is visible
without having read this file.

`SKIN_FORMAT_VERSION` stays 1, as in 2.0.1: a skin written by an updated copy
still opens on one that stayed put.

### Fixed — a network that goes away no longer claims to be a broken file

The previous round closed the three routes by which network failure came in
declaring itself something else. Five stayed open, and three of them reopened
exactly the defect that had just been closed.

**Seeking said «damaged file».** Dragging the cursor while the share was dying
came out as `playback.decodeFailed`, which the catalog declares never
retryable: no «Retry», no noted point to come back to, and the advice to replace
a healthy file. On FLAC it is the most exposed route — `SeekMode::Accurate` goes
to the point and then re-decodes as far as the exact frame, passing through the
seek table too — and it was also the route «Retry» itself takes. Seeking now
goes through the same recognition as reading.

**And opening said «unsupported format».** Worse: symphonia, when container
recognition fails, says «no suitable reader» and **throws away** the system
error that prevented it. From outside, a dead share and a file that isn't music
arrived identical. Now the system's number is kept aside while the stream passes,
and at the end it is looked at: if underneath there was the network, the network
is what gets said.

**Container recognition sat outside the deadline.** The five seconds covered
`File::open` and nothing else, that is, the fast part; all of the header reading
— on FLAC including the embedded cover art, often hundreds of kilobytes —
happened afterwards, on the decoding thread, where there is nowhere to slip a
deadline in. The engine now receives a track **already open**: opening it has
moved to this side of the boundary, inside the deadline.

**The next track was opened on the thread that plays.** The event observer runs
on the decoding thread, and on every track change it asked for the next one to
be prepared: up to five seconds of stall against a ring that at 48 kHz stereo is
worth a little over three seconds, that is, an audible gap at every track change
on a network that answers badly. Now it's a tap on a channel, and the work is
done by a thread of its own. For the same reason `avvia_corrente` no longer
holds the player's lock while it opens: it decides, lets go, opens, takes it back
and checks that the queue hasn't changed in the meantime.

**A half-finished scan deleted tracks.** `walkdir` silently skips branches that
don't answer, and the re-probe that protects dead roots fired **only if the walk
had come back completely empty**. On a large library the share has time to answer
for the first thousand files and to die on the other hundred thousand: those
hundred thousand became «gone», that is, to be deleted, ratings and history
included — and a scan asked for by hand has no anti-massacre guard, deliberately.
Now a walk declares whether it lost branches, and a partial one counts as an
empty one: it re-probes, and if the root no longer answers nothing is removed. In
the same round, a root that dies once reading has started is **abandoned** at the
first failure instead of being questioned file by file: a single long wait
instead of one for each of the remaining files.

**And the tests.** None of them decoded a real FLAC: now there is a
hundred-and-fifty-four-byte sample in `aether-play/tests/campioni`, and three
tests that go over it — decoding with resampling, seeking, and a share that dies
halfway through the header. Plus: the root probe, the real one with the deadline,
which the in-memory doubles had always overridden; and a check that every `.sql`
on disk really is in the list of migrations — the direction that was missing, and
the one `016_metadati` had slipped through.

### Fixed — an unplugged network cable no longer takes away either the window or the library

Aether opens music files with `std::fs`. On an SMB share that stops answering —
server off, VPN dropped, mapped drive letter with nothing behind it any more —
those calls don't fail: they **wait**, for the forty seconds of Windows'
timeout. For all forty the window was declared «not responding». This round
closes the hole from every side it came in by.

**The library no longer empties itself because of a switched-off NAS.** A scan
that could no longer find the NAS folder concluded that the tracks inside it did
not exist, and deleted them. Now a root that doesn't answer is **skipped**
instead of being considered empty, and a non-automatic scan says so on screen:
which roots didn't answer and how many removals it held back. Scans nobody is
watching — the one that starts by itself after downloads — are moreover
*cautious*: in doubt they delete nothing and leave it written in the log. A
library lost to an unplugged cable costs hours of rebuilding; an extra line in
the log costs nothing.

**The window no longer freezes when opening a track.** Opening the file came out
from under the two locks — library and player — and ran on the main thread:
pressing play on a track from a switched-off NAS froze everything for forty
seconds. Now the database row is read under lock, the file is opened **outside**,
with a five-second deadline, and the commands that can end up on a network path
run on a worker thread: playback, lyrics, tags, cover art, skins, Studio, sync.
Once the deadline passes the network warning appears, and the window has been
responding the whole time.

**A share that dies halfway through a track no longer says the file is broken.**
Windows error 64 (`ERROR_NETNAME_DELETED`) arrived all the way disguised as «this
file is damaged: replace it and rescan» — the worst possible advice, because it
sends a healthy file to the bin. And immediately afterwards the engine moved on
to the next track, which is on the same dead share: another forty seconds,
another warning, and so on **all the way down the queue**. Now network failure is
recognized for what it is, playback **stops** where it was instead of scrolling
on, and the warning carries a «Retry» that picks up from the point where the
music was interrupted — not from the start of the track. A file that really is
broken still costs a crackle and still skips to the next track: the distinction
is made by the error's code, not by there being an error.

**What stays outside, declared:** the forty seconds of silence before the error
arrives, when the share dies **while a track that has already started is being
read**. That reading is inside the decoder, there is no point at which to slip in
a deadline, and interrupting a thread parked in a `ReadFile` is something no
operating system permits. It is written in the code comment too, where somebody
will look for it. Opening, on the other hand, is now covered: see the following
round below.

**The message was missing.** Of the catalog's 107 codes, `fs.networkUnavailable`
was the only one without a translated sentence: whoever met it read Windows'
system text instead of the sentence that says what to do.
`strumenti/lingue.js` now compares the Rust catalog with `it.json` and fails
`verify` if a code is left without a sentence — before, it compared only the
languages with each other, and a key missing from *all* of them was invisible.

### Added — the window leaves a record even when it is the one falling over

The log collected everything that happens in the core and nothing of what
happens in the interface. An uncaught JavaScript error tore down the React tree
and left a **white** window: no message, no button, and in release not even a
console to open.

Now there is a boundary around the application, which in place of the white
window draws what happened and a button to reload, and two global listeners for
the failures the boundary cannot see — those in event handlers and rejected
promises. All three write into the log through the same command.

The boundary sits around the application alone: the chrome and the window's
three buttons stay outside, because a window that loses its close button on
entering a failure screen would be a worse failure than the one it is reporting.
And what ends up in the log is **a single line**, cut on characters: a stack
trace from the front carries the modules' URLs with it, which in development are
paths on the disk of whoever is working, and `PRIVACY.md` promises that paths
don't end up in the log.

### Fixed — cover art that fails to save gets counted again

The scan screen has always had a line for the cover art it did not manage to
save, and for some time that line never appeared: the error was thrown away just
below, and the count reached zero by construction. The route has been put back
together in full, from reading the tag to the number on screen.

In the same round, the `016_metadati` migration — the columns the metadata health
card rests on — was on disk but not in the list of those to be applied: it
existed as a file and had never been run by any library. Now it is there.

### Fixed — thirty-eight faults, found by reading instead of waiting

One pass over the whole tree, crate by crate, with a single rule: no style, only
things that behave differently from how they are written. What follows is
grouped by what it costs the listener, not by file.

**Data loss.** The Drive backup only re-merged files signed by another device:
after the first merge the union carried its *own* signature, and the next time it
was overwritten instead of merged — the other devices' counts vanished on the
second save. Now it always merges, which is an idempotent operation and had never
needed that guard. In the same file, an unreadable remote backup was set aside as
`.corrotto-…` and then overwritten by the next save, which still pointed at its
identifier: the safety copy lasted less than a minute. Now a remote set aside
gives rise to a new file, and the copy stays.

`riconcilia` — the step that puts tracks arriving from the download queue back
into playlists — had no way of knowing which rows it had already relocated: no
filter on state, no presence check. A track removed by hand from a playlist came
back into it at the next reconciliation, forever, and with compacted positions it
came back *twice*. The `015_riconcilia` migration adds `desiderati.placed_at`
(rows already closed are marked on update, so no existing library finds its
tracks all put back at once), and now every row relocates at most once. In the
same round, playlists' `updated_at` is touched only if something really did come
back — before, an empty reconciliation was enough to make the wrong side of the
sync win.

Pruning empty folders after a reorganization walked up from a common root which,
for two tracks on different records, is the **empty** path: `starts_with("")` is
true for everything, and the walk up reached the root of the disk. An empty root
now prunes nothing. And the name of the downloaded file went through
`with_extension`, which treats as an extension everything after the last dot:
«02 - Mr. Brightside» became «02 - Mr.mp3», and the anti-collision branch
generated the same path ninety-eight times.

**The window that freezes.** Fifteen long commands — scanning, reorganizing,
imports, lyrics, sync — were declared without `(async)`, that is, they ran on the
window's main thread. The rule has been written in `nuvola.rs` all along and was
not applied: for the twenty seconds of the first scan the progress events went out
and nobody drew them, and `annulla_scansione` wasn't even received until the scan
had finished on its own.

**Playback saying one thing and doing another.** Removing the current track from
the queue immediately showed the next one's title while the speakers carried on
with the removed one — and when it ended the queue advanced again, skipping the
track just announced. With the queue finished, the engine left the last
`track_id` written: «resume» asked an empty engine to start again, that is, did
nothing, while the button became «pause». The scrubber bounced back to its
starting point on every drag, because commands to the engine are queued and the
state started with the position from *before* the seek. Relative seeks from
headphones went back to the start of the track, because the position the
operating system uses as «from where» was only written by a state change. And the
sleep timer, in the quarter of a second between expiry and its collection, showed
the zero that means «at the end of this track».

**Catalogs and metadata.** On Archive.org, `H:MM:SS` durations weren't read — the
veto on duration didn't apply precisely to the full concerts where it is needed
—, two tracks with the same name from the same item collapsed into one, and the
album was computed and then thrown away: everything coming from the archive ended
up in «Singles». A 503 on one page threw away the candidates already collected
instead of carrying on. Domain recognition was done by suffix, so
`evilarchive.org` passed as `archive.org`. On Audius, the cover art of tracks
resolved from a link was always absent. In enrichment, two sources could «agree»
when one of them was `None`, and the MusicBrainz identifier of the *recording*
also acted as a veto on the release ones: editions never merged.

**The rest.** Legacy playlists were imported without deleting first, creating
hybrid playlists; a POSIX `file://` lost its leading slash and became relative;
the reorganization journal, on a row truncated mid-value, returned an empty string
instead of saying it couldn't be read; thumbnails already present declared
fictitious square dimensions; `create_smart` wrote two statements without a
transaction; a cover art fingerprint got as far as composing a path without being
validated, while the check already existed two files away; and the only thing the
imports screen knew how to say about a queue failure was «it didn't start»,
because it read the payload with field names that record doesn't have.

- **The signing key is checked before building** (`strumenti/firma.js`, a new
  step in `release.yml`). `tauri build` touches it last: at 2.0.1 the secret had
  arrived broken and the run died after thirteen minutes, with a message that
  talked about passwords while the fault was in the key. Now what can be checked
  without building — that it's whole base64 and not wrapped, that inside there's
  a *private* key and not the public one, that the password doesn't carry a
  trailing newline — is checked in a second. It never prints anything that comes
  from the secrets.

### Added — the README shows the program instead of describing it

A music player with zero images in the document that introduces it: whoever
arrived from a search engine had to trust two hundred lines of prose to know
whether it was worth downloading an unsigned `.exe`. Now `immagini/` holds seven
shots taken from a real library — fourteen hundred tracks — and each one carries
next to it the thing the image alone doesn't say: why the home isn't an
alphabetical list, why the spectrum is read after the equalizer and before the
volume, what a skin that isn't a dark theme changes.

Along with them, the section that was missing entirely: **«Installation»**. The
document explained how to build and not where to download, and it said nowhere
that SmartScreen blocks the installer — which is the first screen a person sees,
and without a line that anticipates it, it looks like an antivirus that found
something. The test count, stuck at «about 1,260», has become true again.

## [2.0.1] — 2026-08-27

**The number jumps from 0.2.0, and not because anything incompatible has
changed.** `SKIN_FORMAT_VERSION` and `SKIN_TRANSFER_PROTOCOL` are 0.2.0's, the
database has no new migrations, and a copy that stayed put still understands an
updated one. It is a numbering choice — the old tree's 1.0.0 sits further down in
this same file, and two trees competing for the same line of versions is not
something you explain twice. The rules above apply from here onwards.

It is also the **first release that goes through the updater**: anyone with 0.2.0
installed sees it appear by itself within half an hour, and from that moment the
chain — tag, CI, signature, `latest.json` — is the one that will carry all the
next ones.

### Added — a place to support the work

A **Support** button at the bottom of the navigation bar, above Settings and
below the thin line that separates «where I am» from the things that aren't
pages. It leads to `github.com/sponsors/…` in the system browser.

It opens nothing inside the window, and accordingly it never has the active state
or `aria-current`: announcing it as «current page» to a screen reader would be
saying something false. In the bottom bar — the narrow one, five entries on one
row — the button isn't there: space there is for people going somewhere.

The address isn't written by hand. `Documento::Donazioni` derives it from
`CARGO_PKG_REPOSITORY` (`github.com/OWNER/REPO` → `github.com/sponsors/OWNER`),
that is, from the same line the other public links come from, and if one day that
line no longer had the expected shape the fallback is the repository — not an
invented page, not a 404. Donations sit in the closed list of documents despite
not being a document: what the list really holds isn't the legal texts, it's **the
addresses the window has permission to open**, and a second identical lock next
to the first would have been the same thing fitted twice.

The same thing on the site, said at length: «Free, and staying free», with the
reason — the Live Music Archive has a non-commercial clause, and charging for the
program would violate it. Donations support the work, never the music. In all four
of the site's languages, plus the entry in the footer.

### Fixed — the document buttons opened nothing

The window's crate did not inherit `repository` from the workspace. Cargo doesn't
complain: it defines `CARGO_PKG_REPOSITORY` all the same, **empty**. The result is
that every address built from that line was a path without a root — license,
license notices, privacy, terms and reporting asked the browser to open `/issues`
and the like, and none of those buttons led anywhere.

One manifest line for the fix, and three tests so it doesn't come back: every name
in the list must produce an address beginning with `https://`, a name outside the
list must produce none, and donations must land on the profile with an owner
inside. It's the kind of fault the compiler doesn't see and nobody reports,
because whoever clicks thinks they got it wrong themselves.

## [0.2.0] — 2026-08-27

**The first version that can be given to somebody.**

Three rewrites in a row, on the same working branch. The first
(`aether/skin-system-e-core`, from 26 July 2026) redid the core and the skin
system; the second (`aether/rust-core`) took everything into Rust behind a Tauri
window; the third took out everything that made Aether undistributable —
`yt-dlp` packaged into the installer and the scraping of Spotify's private
endpoints — and put the free catalogs in their place.

Two things stay outside, and they're declared rather than hidden.

**The installer isn't signed.** Windows SmartScreen will show «Windows protected
your PC», and to proceed you need *More info* → *Run anyway*. It isn't a
formality to be waved away: an unsigned installer is an installer whose
provenance cannot be verified, and the only thing that can be offered in exchange
is that the source is here and builds itself. An OV certificate requires identity
validation and an annual cost.

**The mobile side does not exist.** It was in the old tree; here it hasn't been
rewritten, and `bundle.targets` produces only NSIS — that is, Windows.

What is **there** and does hold: 1264 tests that run without a network, a gapless
audio engine with ReplayGain and an equalizer, a library on SQLite that
recognizes moved files, reorganization with preview and undo, enrichment from
MusicBrainz that writes nothing in the face of insufficient evidence, skins with
their editor, backup and sync, scrobbling, lyrics from LRCLIB, updates with
signature verification, and two free catalogs to take music from that can be
taken.

### Added — a remote file that can be read and seeked, and the Jamendo client

Jamendo is **listen only**: their terms expressly forbid caching and offline
access, so `Fonte::puo_consegnare()` is `false` and no downloadable track ever
comes out of there — not «usually», but always, because it is decided by
`Disponibilita::decidi` a step below and the module has no way to step over it.

Listening without keeping means playing something that isn't a file, and Aether
didn't know how. Now there is `aether_net::FlussoHttp`.

**The audio engine has not been touched.** `aether_play::Sorgente` takes a
`Box<dyn Flusso>` — `Read + Seek + Send + Sync` — and not a path; the comment
that says so talked about Android, «there is no path to open there but a grant
from the system», and it holds identically for a listen-only catalog. The seam
was already there, waiting for somebody.

**Seekable, not forward-only.** A decoder does not read from beginning to end: it
looks for the tags at the head, then at the tail — ID3v1 and APE come *after* the
audio — then goes back to the first frame, and when somebody moves the cursor it
jumps into the middle. With a forward-only stream each of those gestures would be
the whole file downloaded to read four kilobytes of it, three times before
hearing a note. Hence a sliding 256 KB window, and `Rete::intervallo`, which asks
for those bytes with `Range` instead of asking for everything.

**No disk, and that isn't an implementation detail.** No temporary file, and there
won't be one: the window's ceiling is the constraint of Jamendo's terms, not an
optimization. A buffer holding everything that had passed would be a copy of the
track in memory, and a copy in memory is a copy. Seeking asks nothing of the
network — the window fills at the first read that needs it — and that's what
makes free the «go to the end and come back» that every tag reader does on
opening.

Seven tests, all without a network: the window's logic is all the module
contains, and testing it against a real service would mean not testing it.

Two things about the Jamendo client are worth saying. It takes `audio` and
**never** `audiodownload`, which is populated for some tracks: reading it would
be discovering whether the server would let us take them, which is a different
question from «are we allowed to». And Jamendo answers `200` even when it refuses
— the verdict is in `headers.status` — so reading only the HTTP code would mean
telling somebody who pasted the key wrong that the catalog doesn't have that
track.

**The `jamendo` feature is born switched off**, and that's the part that matters
most. Their API is free for non-commercial use only and the terms define
commercial use as «any monetary compensation»; Aether supports itself with
donations. Whether they count is for Jamendo to say — the question is at
`licensing@jamendo.com`, see `TERMS.md` § 2 — and until they have answered, an
installer that uses that API on unknown conditions is precisely the kind of thing
this program was rewritten to avoid. The code is there and is tested
(`cargo test -p aether-catalogo --features jamendo`); on the day of the answer it
turns on with one line.

### Added — Audius, the second catalog

Aether had only one. `Cataloghi::cerca` collected the failures in a vector and
failed «only if nobody answered»; the «Why isn't it working?» panel distinguished
«nobody is answering» from «whoever answers works»; the window already mapped the
names of three sources. All written in the plural, for a single catalog.

Now there are two. Audius comes in **without touching the audio engine**, and the
reason is in the domain: `Fonte::puo_consegnare()` says `true` for Audius because
there it is the artist who decides, track by track, whether their piece can be
downloaded or only listened to. When it says yes, the track takes the same road
as an Internet Archive item — `preleva` writes a file, the scan brings it into
the library, and from there it is a track like the others.

**Permissions multiply, they don't add up.** `Disponibilita::decidi` puts
together what the source allows and what the license allows; above the two there
is a third veto the license knows nothing about, and that is **gated** tracks.
Audius allows streaming or downloading to be closed off behind a follow or the
holding of a token: a track with a gate on streaming doesn't even enter the list
— it couldn't be heard, and a playlist of tracks that don't start is worse than a
shorter playlist — and one with a gate on downloading drops to listen-only
whatever its license says. A catalog's permission is not the sum of its yeses: it
is the intersection of its noes.

Audius's license is a **text field** written by the artist, not a code: what
arrives is «Attribution ShareAlike CC BY-SA», and the `by-sa` has to be extracted
from it. A test goes through every entry in their menu, because if one stopped
being recognized, the tracks carrying it would silently become undownloadable.
What isn't recognized counts as `Licenza::Sconosciuta`, which does not permit
copying — the right direction in which to be wrong.

**The node is chosen once.** Audius has no server but a network of discovery
nodes, and `api.audius.co` says which ones are healthy. One is taken and kept for
the session; if it goes quiet it is forgotten and another is taken, once only.
The second silence isn't the catalog's — it's the listener's network — and
insisting on twenty nodes would mean twenty timeouts before being able to say so.

From there came the least obvious thing in the whole module. What ends up in
`desiderati.fonte_url` **is not an address**: it's a path. An address with today's
node inside it, read back in a month by the queue that fetches, would point at a
machine that is no longer there. The node is put back by `prepara`, at the moment
of going to get the bytes, and it will be one that answers now.

Along the same path travels the original file's extension, and that isn't an
affectation either: `Candidato::estensione` doesn't survive the round through the
table — the queue rebuilds the candidate with `..Default::default()` — and
Audius's download point ends in `/download`, with no extension. Without the
hint, an original wav would have ended up on disk called `.mp3`. The hint isn't
sent anywhere: it was for us.

Jamendo stays outside, being listen-only and therefore wanting streaming — that
is, a player that can play something that isn't a file on disk. That's the next
piece.

### Added — a log, so that «it won't open» becomes a diagnosis

In release, Aether had **no** way of reporting what had gone wrong. `main.rs`
declares `windows_subsystem = "windows"`, which removes the console; the
thirty-five `eprintln!`s scattered through the application — the thread that
doesn't start, the audio device that disappears, the catalog that refuses — thus
ended up nowhere as soon as you were outside `cargo run`. As long as the window
opens it isn't serious, because almost every failure has a face. The case that
matters is the other one: faced with «it won't open» there were two pieces of
information, «it won't open» and «Windows 11», and neither of the two can be
used.

Now there is `diario/` in the data folder: three text files that rotate at two
megabytes, and the oldest one goes. The `[module] what happened` lines don't
change shape — they're the reason the file can be read diagonally — and `nota!`
has taken `eprintln!`'s place one line at a time.

Two things were added because without them the log would have covered only the
failures that were already visible.

**The panic hook.** The release profile declares `panic = "abort"`: no stack
unwinding, no `catch_unwind`, no `Drop`. The hook is literally the last code of
ours that runs, and it writes thread, position and message. Our own panics stay
forbidden with `deny` across the whole tree — the ones that will arrive here come
from the 429 crates below, which don't respect that rule.

**Every error that crosses the IPC.** `errore.rs` had said for months: «on the
day errors also go into the log, the place to add it is here». The code and the
cause go in there, not the message: `db.openFailed` is the same word in every
language, whereas the message is translated and written for the listener.

The log **is never sent by anyone**, and there is no command that could: a
program that can post its own logs by itself has telemetry, and `PRIVACY.md`
promises there is none here. From the window you can only open the folder, with a
button in *Settings → Updates* — next to «Installed version», which is where
someone about to report something ends up.

And since a file that gets sent to someone must be looked at before promising
anything about it, two converted lines were rewritten: one printed the full path
of the just-downloaded file — which carries the Windows account name inside it
and, given how `riordino` builds the folders, the artist and the album — and now
prints the extension alone, which is what's needed to understand why a tag isn't
being written; the other named the track, and now gives its row number.
`PRIVACY.md` § 8 lists what ends up in there and what doesn't, and now those two
lists are true.

To write the instant in front of every line, milliseconds had to be converted
into a date, something the domain didn't have: `giorni_dall_epoca` existed
without its inverse. `data_dall_epoca` and `istante_iso` sit next to it, and a
test walks a century day by day to verify that the two cancel each other out.

### Fixed — lyrics stayed still on tracks whose timings existed

Certain songs showed the words with the «untimed» label and didn't scroll, while
others from the same record scrolled perfectly. It wasn't a question of language,
format or duration: it was which entry in the catalog answered first.

LRCLIB keeps **several entries for the same track** — one for each edition
somebody has uploaded — and `/api/get`, the exact question, returns only one: the
one whose signature (title, artist, album, duration to the second) matches the
file's. It isn't necessarily the one with the timings. For Caparezza's «The
Auditels Family» the exact signature finds the entry uploaded in 2020, which
carries only the flat text; the other six entries for the same track, same
duration to a tenth of a second, do have the LRC. Aether stopped at the first and
never asked for the others.

Now an exact answer **without timings** no longer closes the search: it is kept
aside and the generous question is asked all the same, and if among the entries
that come back there is one with timings that passes the duration and title
vetoes, it wins. With timings, or instrumental, the first answer remains the last
word and the second request isn't spent.

From there came the other three halves of the same defect. A **flat** lyric no
longer counts as «track already done»: neither for the sweep's queue — which thus
stops skipping forever the people who have the words inside their own mp3s' tags,
something the code declared it didn't want to do — nor for the panel, which goes
and asks when what it holds doesn't scroll. An `.lrc` without timings next to the
file no longer covers, and above all no longer overwrites, timings already in the
table. And a migration re-queues the catalog's old answers that had stopped at
the flat one: the lyric you have stays there to be read in the meantime, but the
question is open again.

### Added — a front door, instead of an alphabetical list

Aether opened onto the album grid. The library's four destinations — albums,
artists, tracks, favorites — are all four an ordered list, and none of them
answered the question you actually ask yourself when opening a player: *what was
I listening to*. To pick up last night's record you had to remember what it was
called and go and look for it.

The Home has four shelves of twelve tracks: **pick up where you left off**, with
the position inside the track; **recently played**; **recently added**; and
**neglected corners**, that is, records in the library for more than six months
and never touched. The last is the shelf only a local library can have: no
streaming service knows what you own and don't listen to, because as far as it is
concerned you own nothing.

«Pick up where you left off» required a fact that was stored nowhere:
`QueueSnapshot` knew which track you were on, not what point in the track. The
position now lives in a key of its own in `settings` and not inside the queue —
`aether_domain::queue` describes which tracks and in what order, and doesn't know
what a millisecond is.

**No new widget and no change to the skin registry**: the page goes through the
`contenuto` slot that already exists, so every skin already written displays it
without being retouched. One migration (`012_home`) for the partial index on
`last_played_at` alone, without which «recently played» was a scan of the whole
table.

Discarded: infinite scrolling. A shelf is looked at, not scrolled — twelve tracks
taken once at opening, and `usePagine` stays for the views that are real lists.

### Added — the keyboard's media keys, and the Windows card

The keyboard's play/pause key did nothing if the window wasn't in the foreground,
and the panel Windows 11 shows in the volume flyout was empty: Aether was playing
and the operating system didn't know what.

Now Aether registers itself as an SMTC session. The panel shows cover art, title
and artist, its buttons work, and — this is the point — **it is Windows that
routes the media keys**, which therefore arrive even from another full-screen
application.

Discarded: `tauri-plugin-global-shortcut`. Registering the media keys as global
shortcuts would **steal them from every other program**, and anyone opening a
video while Aether is open would find the pause key pausing the wrong thing.
Registering a media session is the opposite — it's the system that decides whose
turn it is, as for every other player.

The dependency is `souvlaki`. In the whole tree there isn't a line of `unsafe`
and `Cargo.toml` forbids it with `forbid`, which no `#[allow]` overrides: the
window's `hwnd` is passed to souvlaki without dereferencing it, and the
dereferencing happens over there.

### Added — normalization has three levels, not a switch

The ReplayGain normalization target was already inside the core, but the window
could only switch it on and off: the value stayed at −18 dBFS whatever you
wanted. Someone listening on headphones in the evening and someone listening in
the car don't have the same problem.

Now there are three levels — **low** (−23), **normal** (−18), **high** (−14) —
plus off. A value with a name and not the boolean it was before: an off that
doesn't say what level it would come back to is an off that whoever switches it
on again has to discover by trial.

Found by trying it: the floating-point output — which is the format WASAPI opens
with almost always — **did not clip out-of-range samples**. With normalization
stuck at −18 it wasn't noticeable; at −14 on an already loud track the multiplier
exceeds one, and it would have become the rule instead of the exception. It clips
now, with a test that keeps it clipped.

### Added — a sleep timer, and «at the end of this track»

In the backend and not in the window: a `setTimeout` dies on every UI reload, and
a timer that survives only as long as nobody touches anything is not a timer. It
lives in an atomic integer next to the player, read four times a second by the
clock thread that was already there — outside the lock, because taking it four
times a second to discover almost always that there's nothing to do would mean
contending for the lock with the pause button.

On expiry it **pauses and does not stop**, so the position stays and the next
morning you pick up from there.

«At the end of this track» doesn't go through the clock: it tells the engine not
to prepare the next one, and the music ends where it would have ended anyway
instead of being cut in half.

The timer **does not travel in the exportable profile**, unlike the other
preferences: it's a decision about tonight, and finding it on tomorrow on another
computer would be music switching itself off with nobody remembering having asked
for it.

### Added — the queue that doesn't end, chosen from your library

When the last track ends, Aether chooses another one instead of stopping. The
cascade is: the rest of the record, then another album by the same artist, then
something in the same genre not listened to for a month, then an unplayed
favorite, then any track at all.

**The choice asks nothing of anybody**: it comes out of the local library by
reusing the smart-playlist engine, so it also inherits its defense against
injection — columns and operators come out of `match` on closed enums, values
travel as parameters.

The hook isn't the end of the track but the moment the engine prepares the next
one and the queue hasn't got one. Queuing **there** means autoplay inherits
gapless for free — and now the crossfade — and never produces an instant of
silence.

Off by default: an update that switched it on by itself would start music
nobody asked for, perhaps in the middle of the night, in a house where the last
album had ended on purpose. With «repeat all» or «repeat this» it never comes
into play, because the queue already has a next one — and this is said under the
switch instead of being left to be discovered.

### Added — crossfade, up to twelve seconds

A track fades into the next one instead of ending and starting again. From zero
to twelve seconds, and zero is the exact sample-accurate gapless of before — with
a test that says precisely this, namely that at zero duration the block that
comes out stays identical sample by sample.

The curve is **constant-power** (cosine/sine), not two straight lines. On
uncorrelated material — which is the case of two different tracks — the powers
add, so `cos² + sin² = 1` holds the perceived volume still; two linear ramps
would give half the energy at the middle of the fade, that is, an audible hole
precisely at the point the fade exists to prevent one.

The work was possible because the engine already **kept two decoders open at
once** — that's what makes gapless gapless. The mixing lives in the decoder
thread and not in the audio callback: the ring between the two threads is a flat
stream of `f32` with no track boundaries, and doing it over there would have
meant a second ring and a second decoder inside the realtime path, where you can
neither allocate nor take a lock.

Two consequences, both intended:

- **ReplayGain has moved.** It was a single global scalar the callback applied to
  everything coming out of the ring; two overlapping tracks need two different
  gains, so now the correction is applied to each track's samples at the moment
  they are decoded. The price: changing the normalization level takes effect
  after the ring's reserve, a few seconds, instead of instantly. What's left to
  the callback is the volume alone, with the ramp it already had.
- The incoming track is announced **halfway** through the overlap, not at the
  start. Before that it is a background under the old one, and announcing it then
  would mean a window changing title while you can still hear the other — and a
  scrobble attributed to the wrong track.

The first version of the mixing had two defects, both in the decoder thread and
both fixed here.

The first sounded like a rasping. The two decoders deliver blocks of different
lengths — an MP3 packet is 1152 frames, a FLAC one can be 4096, and a track that
needs resampling whatever the resampler decides — and the overlap mixed one
against the other, throwing away what was left over of the incoming track. As
much as three quarters of its samples in the bin on every block: not a fade, but
the new track pushed forward in jerks. Now the decoded samples sit in a queue and
the mixing takes exactly as many as it needs, keeping the rest for the next
block.

The second skipped a track. Halfway through the overlap the engine announces the
incoming track — see above — and whoever sits above answers that announcement by
preparing the track **after that one**: as long as «prepared» and «incoming» were
the same slot, that preparation snatched the track away halfway through the
curve. The second half of the fade brought in the wrong track and at the end of
the crossing the engine latched onto that one, so the listener found themselves a
track further down the queue. Now the incoming track has a slot of its own for
the whole overlap, and «the next one» goes back to being what it says it is. It
also follows that a fade survives the knob: the duration freezes when it begins,
because changing the denominator halfway through the curve would shift the gain
abruptly.

A third case wasn't a fault but a limit, and it is covered now. The overlap
begins when what remains of the track is as long as the fade, and «how much
remains» is known from the duration **declared** by the database — which for a
variable-bitrate MP3 with no Xing header is an estimate, and the estimate can be
long. When it is, the outgoing track's decoder finishes halfway through the curve
and the incoming one is left alone at half amplitude: it jumped abruptly to full
amplitude, that is, a click on the track you then listen to in full. Now its rise
resumes from the exact point at which the curve was interrupted — the gain is
given by the same function to whoever mixes and to whoever resumes, because two
formulas written in two places would be two steps — and completes in forty
milliseconds, the same as the volume ramp. Not the rest of the curve: that was
calibrated on a duration that has just been discovered false, and continuing it
would mean a track that starts at half voice and takes seconds to come up on its
own, which is more audible than the click it was meant to remove.

Three new tests keep them closed, and they are the first that run the decoding
thread in full: WAVs built in memory, real decoders, no audio device. The first
checks that the incoming track is heard **on its own** — if the one after came in
instead there wouldn't be a single sample at its level; the second that two
one-second tracks overlapped by four tenths last one point six seconds, not one
point two; the third has a file declare a thousand milliseconds and contain eight
hundred, and watches the incoming track resume from where it was instead of
jumping.

Off by default, and said on the card: while it is on, the sample-exact crossing
is gone, including the tracks of a record written to run together.

### Changed — the Studio shows the application, not an imitation of it

The Skin Studio's preview had a single list, of nine scenes, and it mixed two
things that in the application are independent: **which page** you are looking at
and **what is on top of it**. The «modal» scene *was* the queue open plus two
selected tracks; the «alerts» scene was a menu, a notification and a tooltip put
in a row inside the content, that is, in a place where they never appear in the
app. Whoever was repainting the selection bar could only see it over the library;
whoever was repainting the notification could never see it over Settings, which
is the only place the scan notification actually appears.

Now there are two axes, as in the app. A **page** is chosen from nine — the
library grid, an album, Settings, Imports, the account, «Now playing» full
screen, the three big panels, the empty state, the loading state — and on top of
it six **overlays** are switched on at will: third column, queue, selection bar,
context menu, notification, dialog. Every combination the application can produce
can be looked at.

**And none it cannot produce.** A switch that would have no effect here — the
queue in full screen, the third column with no track playing — switches itself
off and says why in the tooltip, instead of switching on and making nothing
happen. The reasons are written against the `visibile` predicates of
`Impaginazione.tsx`, that is, against the code that actually decides, not against
an idea of how it ought to behave.

The overlays are drawn **next to** the shell and not inside the content's hole,
which is where they used to be: they are `position: fixed` and refer to the
window, and in the preview frame they refer to the frame because `.anteprima`
already carries `contain: layout paint`. Their scrims, which in the app take the
click to close, don't take the pointer here: had they stayed targets, the probe
would have read `.velo` everywhere, and the surface underneath — the one being
repainted — would have become unreachable.

**Three new pages, and they are the three that were missing.** «Now playing» full
screen wasn't there at all: `np-screen`, `np-scrim` and the full cover art live
only there, and the «take me where it's visible» table sent you looking for them
in the third column, where they have never been. Imports carries the second form
of `list-row` — dense, without cover art, with a bar inside — and a skin tuned on
the library list would then discover that the row there is half as tall. The
account carries `stat-number` in a row, the switch with its track, a text field
and an empty state inside a card instead of in place of a page.

### Changed — the scenes copy the real markup instead of resembling it

The rule for the preview's fragments was «write the registry's classes, never a
geometry the app doesn't have», and it wasn't enough: the registry's classes were
right and everything else was invented. An album's cover art was
`scheda section-card` where the application writes `scheda list-row` — whoever
repainted `list-row` didn't see the library change, whoever repainted
`section-card` saw it change here and not in the window. A card's title was
`.titolo-scheda` where the app says `.titolo`. In Settings there were a crossfade
slider and a field for the library folder that don't exist in Settings.

Now the rule is tighter: **copy the real markup**, class by class, from the
component that draws that screen. Where the component can be mounted —
`Interruttore`, `Segmentato`, `Copertina`, `Trasporto`, `Scrubber`, `Giudizio`,
`Stelle` — it isn't copied at all, it's used. Twenty `stile.css` rules that drew
the imitation have gone: what remains is drawn by the application's rules, which
are the same ones.

The page header receives `query` and `ordinamento` where the app passes them, and
it's the only place `field-input` and one of the `btn-ghost` live: without it the
frame showed a header without a search, that is, without the text field a skin
must be able to repaint.

### Fixed — «it isn't visible» had two answers and there are three cases

When choosing a part that doesn't appear in the open scene, the Studio answered
either «it's over there, let me take you» or «it's in the chrome, it will appear
when the tree mounts it». Whoever chose `tour-tooltip` — which the registry
declares and **no** screen draws — got the second, that is, a wait that never
ends.

The answers are now three, and the third says the truth: five parts of the
registry are drawn by nobody yet, and each carries its own why.
`strumenti/classi.js` checks that the Studio's list and its own — `ATTESE`, the
one that runs in CI — stay the same thing: they are the same information told to
two recipients, and by diverging the Studio would go back to giving the wrong
answer.

### Fixed — two registry parts almost nobody emitted

`glass-modal` is declared as «the modal window» and one surface alone carried it:
the queue panel. The eleven real dialogs — the account, a smart playlist's rules,
the restore, the reorganization, the confirmation — were `.finestrella` and
nothing else, that is, not repaintable. Now they all carry it, and the part means
what the registry says it means.

`field-input` is «the text field» and sat on one alone: the search field in the
header. The other fifteen — the scrobbling token, a new playlist's name, an
equalizer preset's name — were `.campo` and nothing else. They are the same thing
and now they say so.

Neither of the two is an aesthetic choice: they were two promises of the registry
the application did not keep, and they were discovered by writing a skin — that
is, afterwards.

### Added — `F11`, the window's fifth gesture

Full screen was already there, but it was the application's: «Now playing» with
`F` fills the **window** with the track, and the window stays as it was, inside
its desktop. The other one was missing — the one that gets the rest of the
desktop out of the way — that is, the key every program that opens in a window
has.

It sits in the shortcuts table (`tastiera.ts`) like all the others: it can be
reassigned, it is read in the settings next to the other seven, and if someone
puts an already-taken key on it the conflict is visible instead of just
happening. By default it is `F11`, which isn't a letter and therefore takes
nothing away from anyone.

**Function keys pass through even while typing.** The rule was that a key
bare key doesn't arrive if the focus is in a field — someone searching for «space
oddity» doesn't want to pause halfway through a word — and without an exception
full screen would have been the only window command to switch itself off while
you look for a record. `F1`…`F12` don't end up inside any word, and now they have
the exemption `Ctrl` and `Alt` already had.

The command is ours — `finestra_schermo_intero` — and not the `core:window`
permission: opening that list for one key would also give the page
`set_position`, `set_size` and `set_always_on_top`, and it's the same reason the
window's other four gestures go through commands of ours. What is refused is the
bundle, not full screen.

The middle button of the band, in full screen, **steps out**: its icon there says
«restore down», and maximizing a window that already fills the screen would be a
click that does nothing while the icon promises the opposite. It is also the way
out for anyone who finds themselves in full screen and doesn't remember which key
they pressed.

### Changed — the band at the top is no longer Windows'

The window is born without decorations. The gray strip the operating system drew
above the application — the icon, the name repeated to someone who had just
opened Aether, the three squares — belonged to another program: it brought its
colors inside a window that has a skin, it stayed light while everything else was
dark, and it took thirty-two pixels across the whole width.

The short road was to redraw an identical one with the right colors. It would have
cost thirty-two pixels again for three buttons, and it would have put the mark
twice: the navigation already has it at the top.

**The application had the top line of its own.** The mark's center is 32 pixels
from the edge, the head of the third column at 30, the header's eyebrow at 29:
it's the same line, and the three buttons sit there instead of opening one of
their own. Whatever comes under that corner — the column, the page header when
the column is closed, the full-screen header and the Studio's — gives way in
**width**: giving way in height would have meant lowering every screen by sixty
pixels, that is, the old band, transparent.

Each button's target reaches the corner, its drawing doesn't. Closing a window is
done by slamming the pointer into the screen's corner without looking, and it's
the only gesture in the interface you can perform with your eyes shut; what you
see, however, is a thirty-two-tall frame with the application's corners, because
three filled rectangles sixty tall would be Windows' band, repainted.

Dragging doesn't go through a transparent frame over everything else: that would
have eaten every click in the first sixty pixels — the button that closes the
navigation, the column's two, the search when it wraps. There is instead a
listener that looks at where the `mousedown` fell: at the top and not on
something clickable, the window drags. The page title can also be grabbed, and
looking at it, it is exactly the title bar.

Resizing from the edges isn't lost: `tao` does it by itself in `WM_NCHITTEST` for
every undecorated window that is resizable. `Alt`+`Space` still opens the system
menu, and `Alt`+`F4` still closes.

The three commands go through commands of ours — `finestra_trascina`,
`finestra_riduci`, `finestra_ingrandisci`, `finestra_chiudi` — and not through
the `core:window:*` permissions: `capabilities/default.json` is a closed list,
and opening it to `core:window` would also give the page `set_position`,
`set_size`, `set_always_on_top` and `set_fullscreen`, that is, four ways of making
a window disappear from under the fingers of whoever is looking at it, in
exchange for three buttons. For the same reason they don't go through the skins'
shell: a skin that could hide the close button arrives in a file.

### Security — the release chain no longer trusts a label

A pass with a scanner over the branch: of twenty-five findings, twelve were
fixed, ten didn't survive verification, and three concern a generated file that
isn't born in this repository.

The twelve all sit in `.github/`, and all twelve pass next to the same thing:
`TAURI_SIGNING_PRIVATE_KEY`, the key that signs updates and that — it's written
in `release.yml`'s preamble — is neither recoverable nor replaceable.

**The CI's eleven actions are pinned to a hash.** `actions/checkout@v4` doesn't
name a version: it names a **label**, and whoever owns that repository can move it
onto another commit without a line changing here. It isn't a textbook fear — in
March 2025 the labels of `tj-actions/changed-files` were rewritten, and
twenty-three thousand repositories ran new code believing they were running the
old. The bill for that day, inside `release.yml`, would not be a dirty build: it
would be the updater's private key in somebody else's hands, that is, every
installed copy of Aether never updating again. The version stays readable in the
comment next to the hash. The price is that the actions no longer update
themselves, and that comment is the only place where it's visible that they're
old.

**The tag no longer enters the script.** It was
`tag="${GITHUB_REF_NAME:-${{ inputs.tag }}}"`, and that `${{ }}` inside a `run:`
is not a variable: it's a text substitution that happens **before** bash sees the
line, so a tag containing `$(…)` stopped being a tag. Now it goes through the
environment, where it stays data whatever is written in it. It wasn't a way in —
launching a workflow by hand already requires write permissions on the repository
— but it was a way that passed next to that key, and for a key that cannot be
replaced, that's all it takes.

Fixing it brought out that **the manual release has never worked**: on
`workflow_dispatch`, `GITHUB_REF_NAME` is there anyway, and it's the branch you
launch from. The fallback value therefore never fell through to the input, and a
release launched from `main` compared the three files' version against the word
«main» and stopped there. Now the tag is decided by `github.ref_type`, and the
same text goes into `tag_name:` and into the release title — which were reading
the ref too, and on a manual release would have published «Aether main».

The three remaining findings are `site/support.js`'s `postMessage` calls, which
declare `"*"` as the origin and don't check the writer's. The file begins with
«GENERATED — do not edit» and its source isn't here: a fix by hand would vanish at
the first regeneration, and the right origin to write isn't even fixed. What
passes through there are component names towards the frame containing the page,
and only if a frame is there; if one day it matters, the answer is a
`frame-ancestors` on the site's header, not a patch to the bundle.

The other ten are worth writing down, if only because the scanner will say them
again: the «hardcoded» OAuth token is `ya29.finto` inside a `#[cfg(test)]`; the
two path traversals compose paths with names coming from `readdirSync`, which
doesn't return a separator; the missing SRI is on a `data:` favicon, where there
is no request to verify; the four «format strings» are concatenations passed to
`console.error`. The last two said *prototype pollution*, and they are the entry
below: what they found is real, but it isn't that.

### Fixed — a color named `__proto__` vanished as you typed it

It is not the global prototype pollution the name makes you fear.
`Object.prototype` is never touched: the object the Studio rewrites is always
born from `JSON.parse` or from a `patch.ts` spread, and the prototype that
changes is that of a copy thrown away an instant later. It's a defect of another
species, and it can be seen instead of being theoretical.

`__proto__` is the only name a skin author can type into a text field — the name
of a palette color, of a pattern, a rename — for which `object[name] = value`
**does not write a key**. `Object.prototype` exposes an accessor with that name
and the assignment calls it; `JSON.stringify` then prints nothing. The color
vanished from the document with nothing saying so, and the editor showed a file
that didn't contain what had just been written.

The defect had an asymmetric face, and that's how it was spotted:
`rinominaChiave` goes through `Object.fromEntries`, which really does create an
own property. Renaming a color to `__proto__` worked; changing its value an
instant later deleted it.

The three functions that walk a path — `valoreIn`, `scriviIn`, `togliDa` — now
write with `defineProperty` and read behind a `hasOwnProperty`. The first is
exactly what `JSON.parse` does when it meets `"__proto__"` inside an object, so
the document's two directions coincide again. The second also closes the reading
case: a path that went through `__proto__` returned `Object.prototype`, and the
Studio's controls started drawing that object's properties instead of the
document's.

### Added — lyrics, and the button that was disabled

In `InRiproduzione` the «lyrics» button was drawn and disabled, with the reason
written next to it: «the core doesn't read lyrics yet». Now it does, and the
panel scrolls.

The sources are consulted in this order, and the first that answers wins: an
`.lrc` (or `.a2.lrc`) next to the track, the row already taken for the same
`track_key`, the tag inside the file, LRCLIB, and finally the tap editor. **No
downloaded source ever overwrites a hand-made synchronization** — the condition
lives in the `UPSERT`'s `WHERE`, that is, in the database, not in an `if` the
next call site might forget.

What can be said and what cannot:

- **there is no free source that has the lyrics of every song.** Anyone
  promising that is either scraping pages or holds a publishing license, and
  neither of the two is something this program does. What there is instead is
  that every track ends up in one of four **declared** states — synced, flat,
  instrumental, to be synced — and never in a blank screen. The four numbers are
  visible in Settings › Folders, instead of being a hope;
- the last state is closed by hand, and the tap editor is built to cost three
  minutes: you press Space on each line while the track plays, each tap is
  snapped to the nearest real onset within the `[-250 ms, +120 ms]` window —
  asymmetric, because the hand is always late — and the **median** of the offsets
  on the snapped lines corrects the ones that didn't find an onset. It is that
  person's reaction latency at that moment, measured instead of guessed. The
  onsets come out of the spectral flux computed with the FFT `aether-play`
  already had for the spectrum: no new dependency, no binary to package.

The LRC is read by **the core**, in `aether_domain::testo`: the window receives
lines already in order and already in milliseconds, and doesn't know what a
`[mm:ss.xx]` is. A second reader written in TypeScript would have diverged from
the first on everything the format doesn't say — and a 1998 format says almost
nothing.

The light that crosses the lit line is a CSS variable written on a `ref`, outside
React: making it a state would mean rebuilding the tree twenty times a second to
move a gradient by one pixel. Where the words' timings are there — the extended
`.a2.lrc` — the front leaves the line and moves to the words, and it's the only
case in which that light says something true instead of being a kind lie.

### Fixed — the lit line lit up and vanished

`--color-accent` did not exist. The accent token in this program is called
`--accent`, and the stylesheet called it by the other name in nine places — two
of which were the gradient that lights the lyric line currently playing.

A CSS variable that doesn't exist doesn't leave its property uncovered: it makes
the **whole** declaration invalid. So `background-image` fell back to `none`, and
since that line entrusts its color to the gradient and sets
`color: transparent`, the result was that the lit line — and on an `.a2.lrc`
every word of the lit line — became perfectly invisible. All of the lyric could
be read except the line being listened to: it lit up, and vanished.

The other seven uses degraded silently, which is why nobody had noticed: invalid
`color` and `border-color` inherit or fall back to `currentColor`, so the tap
list and the shortcut key only lost their accent. Two `border-radius` values, on
the other hand, fell to zero, and the two controls carrying them were square in
an interface that doesn't have a single sharp corner.

All fifteen uses of the stylesheet's four dead variables were fixed
(`--color-accent`, `--color-border`, `--color-warning`, `--radius-control`). In
the lit line's gradient the right name brings a nested fallback with it,
`var(--accent, var(--color-text-1))`: it's the only place where a missing
variable means not «without accent» but «without text», and a defect that erases
what you're reading must not be able to come back through a rename.

### Fixed — on a wrapping line the front split in two

With the lit line put back on its feet, the defect underneath became visible. On

    Guardo nel retrovisore, dietro me si sta
    scuencendo l'autostrada

a quarter of the way through, «Guardo nel» **and** «scuencendo» were both lit:
two detached fragments, and the line below lighting up in parallel with the one
above instead of after it. In a narrow column every second line wraps, so the
front said the wrong thing almost always.

The cause: the gradient sat on the **block**, and a block that wraps stays a
single one wide. The same horizontal cut fell on all its visual lines in the same
instant.

Now the lighting lives on an **inline** element inside the line. On an inline
that wraps, `box-decoration-break: slice` applies: the background is painted as
if the fragments were one after another, and only then sliced line by line — that
is, exactly the order needed. It costs one element and no extra writes per frame;
the alternative, one element per word as in the extended LRC, would have been `N`
writes every fiftieth of a second for the same result.

The extended LRC didn't have this defect and never had, for the same reason the
other one doesn't have it now: there every word is already an inline element with
its own gradient.

### Fixed — the lyrics panel, which didn't follow and couldn't be read

The same screenshot said three other things, each small enough not to be noticed
on its own: the panel was stuck at the top after half a minute of a track, and
all of the lyric sat at 46% white over the spectrum's purple bars.

- **Automatic scrolling switched itself off.** The pause the panel takes when you
  scroll it by hand was hung on the `scroll` event — which is also emitted by the
  scrolling the panel itself asks for. That is: it followed the line once, that
  following put it on pause, and during the pause all the subsequent lines went
  by. Now the pause is hung on the **gesture** — the wheel, the finger, the keys
  that scroll — which is the only thing a person does and the browser doesn't.
- **`padding-block: 40%` was not «half a panel».** A percentage in padding
  resolves against the container's width, vertically too: on a three-hundred-and-
  sixty-pixel column that was a hundred and thirty instead of six hundred, and
  the lit line couldn't rise to its place until the song was halfway through. The
  breathing room is now provided by two spacers, where the same percentage is
  measured against the height — and they appear only for a lyric that scrolls,
  instead of pushing untimed lyrics down too.
- **The panel was transparent over a canvas that redraws itself.** A 45% scrim is
  the right recipe over blurred cover art and the wrong one over the spectrum,
  which leaves the bars full precisely in the lower half — where almost all of
  the lyric is. It is now 82%, and the not-yet-sung lines move from
  `--color-text-3` to `--color-text-2`: over the worst case they sit at 6.6:1
  instead of 4.4:1.

And three things that were missing:

- the **last line** of each track stayed white and still, because progress is
  measured up to the following line and there was no following line. The bottom
  is now given by the file's duration, which the domain doesn't know and the
  window does;
- in the **intro and the breaks** between two verses nothing was lit, and a
  motionless panel looks broken precisely when it's working. In their place there
  are three dots that fill up, driven by the same `--avanzamento` as everything
  else: no new mechanism, the same variable read by three elements. Below three
  seconds they don't appear, because they'd be a flicker;
- **you click a line to jump to it**. The offsets are undone instead of being
  applied — the lines' timings are never touched, that's the rule of the whole
  module — and only one line at a time is in the tab order, because two hundred
  stops inside a panel you read is not accessibility.

Around that: the lyric column has become fluid (`clamp(340px, 26vw, 460px)`)
because at a fixed three hundred and sixty pixels almost every line wrapped while
eighteen hundred pixels of nearly empty scene were left in the middle of the
screen; the lines fade at the edges instead of being sheared off by the panel's
corner; and «back to the track» says that following is paused, which is what lets
the pause last six seconds instead of three without losing anybody.

### Added — giving back to LRCLIB, and why it's a button and not a checkbox

After syncing a lyric by hand you can send it to LRCLIB. It's worth being
explicit about how, because here the form **is** the substance:

- it's a gesture, one track at a time, and it's pressed after seeing what is
  being sent. Never in bulk, never automatic, never attached to saving — because
  saving always happens, and everything attached to saving is automatic by
  definition;
- **the lyric that was inside a file's tag is never published**, nor is what has
  just arrived from the catalog. The rule lives in `testi::da_restituire`, not in
  the window: it holds even if one day the button were somewhere else;
- the untimed text accompanying the submission is **derived** from the LRC being
  sent, never taken from elsewhere: that way it is by construction the same text,
  and there's no way to send two versions saying different things;
- with the lyrics switch off, the button doesn't appear.

`PRIVACY.md` § 2-bis now lists line by line what goes out when it is pressed, and
`TERMS.md` § 3-bis says the thing usually left unsaid: what you're sharing are the
timings, the words stay the rights holder's, and sending them to a public catalog
is your decision and not one the program takes for you.

Publishing takes a few seconds, and not because of the network: LRCLIB asks for
an SHA-256 proof of work on every submission. It's an honest defense — it doesn't
touch someone sending one lyric at a time and makes sending a hundred thousand
impractical — and the window says so **before** you press, not after.

Musixmatch, Genius, AZLyrics, LyricFind and the Chinese catalogs stay out, and
now CI verifies it on every change: the «No forbidden lyrics source» step in the
`niente-di-vietato` job, next to the one that keeps the downloader tools out. The
decision lives in CI, not in the memory of whoever is reviewing.

### Added — the updater, and the request that wasn't there

Whoever installed 0.1.0 stayed on 0.1.0. Now, two minutes after startup and then
every half hour, a background thread asks GitHub whether a newer version has come
out, and when there is one it says so with a band above the content — version,
release notes, «Update» and «Not now».

**It is a network request that starts on its own, and this changelog is the place
to say so.** Until yesterday `PRIVACY.md` § 6 said «no request at startup: there
is no update check», and that sentence became false with this commit — so it was
rewritten in the same commit, and the check now has a numbered section of its
own. Nothing goes into the request: no identifier, no counter, not even the
installed version. It's a GET to a public three-hundred-byte file, and the
comparison between the two numbers happens here. It switches off in *Settings →
Updates*, and switched off nothing goes out.

It doesn't install by itself. Installing means closing the application, and doing
it while somebody is listening — or while they're reorganizing thirty thousand
files on disk — is the kind of thing that is forgiven once only. The thread finds
and waits; downloading starts from a button, and before the installer is run its
minisign signature is verified against a public key compiled into the executable.
A «not now» applies to that version alone and is written to the database, not to
memory: a warning that comes back on every restart is a warning people learn to
dismiss without reading.

The thing that could have gone wrong in silence is something else, and it's why
`strumenti/manifesto.js` exists: if the private key CI signs with isn't the half
of the one the already-installed executables know, the `latest.json` is perfect,
the installer downloads, verification fails and **nobody updates** — and nobody
notices, because the failure happens on other people's computers. The script
compares the eight identifier bytes minisign puts both in the key and in the
signature, and fails the release if they don't match.

The release is born as a draft, and `/releases/latest` skips drafts: publishing it
**is** the gesture that sends the update to everyone. Automating that is two
lines; it isn't done, because it's the only point in the chain where a person
looks at the thing before it goes out.

A shared MEGA folder wasn't used, which was the first idea. A
`mega.nz/folder/<id>#<key>` link isn't a downloadable file: the key sits in the
`#` fragment, which by definition never reaches the server; the real URL is
obtained with a POST to the API and is temporary and tied to a rotating node; the
bytes that come back are AES-128-CTR encrypted with a meta-MAC to verify; and
anonymous downloads are counted per IP address, so behind a shared NAT users
would burn each other's quota. `tauri-plugin-updater` has no point at which to
substitute its own downloader: MEGA would have meant giving up the plugin and
rewriting the signature verification by hand too, that is, the one part that must
not be got wrong.

### Added — Italian and English, and German costs one file

The interface was written in Italian and nothing else: the strings were inside
the JSX, `«1 brano» / «2 brani»` was stitched into `formato.ts`,
`<html lang="it">` was fixed in `index.html`, and `toLocaleString("it")` appeared
by hand in forty-five places across seventeen files — plus three times without an
argument, that is, three numbers following the operating system while all the
others followed Italian.

Now it speaks two languages, and — this is the part that matters — adding a third
costs **one file**: you drop `de.json` into `apps/desktop/src/lingue/` and the
program finds it. No registry to update, no `switch` to lengthen, no import to
write. The pivot is Vite's `import.meta.glob`: the list of available languages
**is** the folder's contents, and the native name the language appears under in
the selector travels inside the file itself, under the reserved `_nome` key.
Tested: copied `en.json` to `de.json`, changed `_nome` to «Deutsch», translated
three keys — German appears without a single `.ts` having been touched, and the
untranslated keys show in English.

No `i18next`: the project has four runtime dependencies in total, and a
two-hundred-and-sixty-line module does this job better than forty kilobytes of
generic library. No `<ProviderLingua>` either: the repo doesn't use Contexts —
`grep` for `createContext` across all of `src` gives zero results — and the
language follows the same shape as playback, a module with
`useSyncExternalStore`. It was needed in practice too: `Impostazioni` already
receives sixty-one props, and passing `t` down by prop drilling through seventy
files wasn't sustainable.

The choices, in brief:

- **The language at startup** is detected from the system, with `de-DE` reduced
  to `de`; if there's no file for that language, **English** — even though the
  development language is Italian. They are two distinct roles and the code keeps
  them separate: `"en"` for someone arriving with a Swedish system, and again
  `"en"` for the keys missing inside an incomplete `de.json`.
- **The choice lives in the core's preferences** (`ui.language`), next to the
  theme, so it ends up by itself in the Drive backup and in the exportable
  profile — `profilo.rs` is an inclusion list, and forgetting it there would have
  given a profile that restores everything except one thing. No migration:
  `settings` is a key/value table.
- **No flash of the wrong language**: the window is born invisible
  (`"visible": false`) and the frontend shows it with the `pronto` command, which
  now also waits for `Avvio.lingua`. The system language is applied before the
  first paint, the saved one before the window is seen.

The `i18nKey` field was already there and nobody read it: `ErroreIpc` has carried
it from the core all along, generated in `errors/catalog.rs` as
`concat!("errors.", <code>)`, and the frontend instead used a hand-written table
of twenty-five Italian messages keyed by code — twenty-five entries against a
hundred and four codes. Now the key is decided by the core and the text by the
language catalog, where all one hundred and four are: the **eight orphans** the
user saw with the backend's raw message are closed.

Also translated: the two hundred and twenty-nine `aria-label`, `title` and
`placeholder` attributes — looking only at the nodes' text would have left
accessibility monolingual. And the seventeen `const` tables with labels inside
have become functions, because a module constant freezes the language at the
first import.

Two things deliberately stay in the language they are born in, and it isn't an
oversight: the core's «Unknown album» and «Unknown artist» placeholders — which
`library.rs` writes **into the database rows** and `organize.rs` uses for **folder
names on disk** — and the attribution line `prelievo.rs` writes **into the tags of
a downloaded file**. Translating them where they are born would give different
album keys between two startups in different languages, folders in two languages
side by side, and an attribution that changes language depending on the month.
They are values, not labels: the frontend recognizes the two placeholders and
substitutes them at drawing time, and paths already written are not touched.

Out of scope, and worth saying: `tauri.conf.json` (`title`, `shortDescription`,
`longDescription`) stays Italian. The window title and the installer's texts
can't be localized without `bundle.windows.nsis.languages`, which isn't there.

**A guard in CI.** `strumenti/lingue.js` compares the keys of every
`lingue/*.json` with `it.json` and fails listing the missing ones — and also the
leftover ones, which are keys renamed elsewhere and not here, that is, text
nobody will ever draw. Without it, the second `de.json` is discovered incomplete
when somebody uses it: the engine falls back to English and draws all the same,
and that is precisely what makes a half-finished translation invisible.
`node strumenti/lingue.js --scrivi` lines up the missing keys with the Italian
text, so they're left to be translated instead of to be found.

### Removed — YouTube, Spotify scraping, and the packaged binary

The biggest thing in this round is a subtraction, and it's worth writing down
why.

**`aether-yt` — deleted in full.** Five thousand five hundred lines that invoked
`yt-dlp.exe` to take the videos' bytes, plus YouTube Music's internal API. The
*YouTube API Developer Policies* § III.E.1.a forbid it in terms that leave no
margin: «You must not… download, import, backup, cache, or store copies of
YouTube audiovisual content». There was no configuration that made it lawful.

Nor was there a corrected version of the module. The same policies forbid
separating audio from video (§ III.I.7) and playing it from a player that isn't
visible (§ III.I.9), and limit metadata retention to thirty days (§ III.E.4). A
music player that keeps a library is exactly the thing those three rules rule
out: YouTube wasn't fixed, it was removed.

**`aether-spotify` — deleted in full.** It talked to private endpoints: the
internal GraphQL with the persisted queries' hashes, anonymous tokens
reconstructed with HMAC-SHA1, the anonymous handshake, and **Spotify's web
player client id** — not ours — with a forged Chrome User-Agent. It was
unauthorized access to the service, and it was the violation nobody had declared.

The **lawful** half went with it: OAuth consent to the Web API. That one worked,
but the *Spotify Developer Policy* (III.5, III.9) limits what can be done with
the data it returns, and a library that keeps it for years doesn't fit inside
those limits. What remains is the GDPR archive, which is the user's by right
(art. 20) and carries the **complete** history instead of the last fifty listens.

**`resources/bin/yt-dlp.exe`** — eighteen megabytes, and the `bundle.resources`
line that packaged them. The installer now contains **no third-party
executable**.

Visible consequences: importing from a Spotify or YouTube link disappears, and
the selector between the two services disappears. What remains is the GDPR
archive and the playlist files, and the free catalogs arrive.

### Added — the free catalogs

**`aether-catalogo`**, a new crate that talks only to `aether-net`: no child
process, no binary to package, HTTP requests you can read in the code. In the
first round there is the **Internet Archive** — Live Music Archive, netlabels,
public domain — with search, item reading and file fetching.

The concept that carries the weight is `Disponibilita`, and it didn't exist
before: the structured answer to the question «can I take this track?», in place
of the earlier implicit assumption («YouTube has everything»). Next to it sits
`Licenza`, and in both cases **the restrictive value is the default**: a license
that isn't known counts as «no copying», and `preleva` refuses before making any
request at all.

**«To buy»**, a new section in the imports page. The queue's `Introvabile` state
changes meaning — no longer «it isn't on YouTube» but «no lawful source has it» —
and becomes a shopping list with links to Bandcamp, Qobuz and Discogs. It is the
honest substitute for a download that cannot be done: saying **where** to get that
track, in places where the people who made it get paid.

**The nature of the recording**, said on screen. The free catalogs don't have the
commercial catalog's studio versions: they have concerts, reissues, reworkings —
the Live Music Archive is made **only** of concerts. Aether accepts them, and puts
a pill next to the track saying which one it took. Anyone rebuilding a precise
record can switch that behavior off (`catalogo.alternative` in the settings).

**Each track's license**, written into `desiderati` (migration 10) and shown in
the queue. A file that enters the library without anyone saying on what
conditions it did is a file nobody will know in a year whether they can share.

### Added — what it takes to distribute

- **`LICENSE`** at the root: the MIT text that fourteen manifests declared and
  that existed nowhere.
- **`THIRD-PARTY-NOTICES.md`**, generated by `strumenti/licenze.js`: it covers
  the eighteen **MPL-2.0** crates (the whole symphonia family, plus cssparser and
  selectors), `cpal` and `tao` which are **Apache-2.0 with no alternative**,
  `ring`, and the thirteen `Unicode-3.0` crates.
- **`apps/desktop/src/font/OFL.txt`**: Geist and Bricolage Grotesque were
  packaged bare, and the SIL OFL requires the license to be distributed with
  them.
- **`publish = false`** in all thirteen manifests: none of these crates goes to
  crates.io, and until yesterday an absent-minded `cargo publish` would have put
  them there.
- **`README.md`**, **`PRIVACY.md`**, **`TERMS.md`**: they didn't exist. The terms
  carry the Live Music Archive's non-commercial constraint, which is a condition
  of use and not a footnote.
- **Two CI workflows** (`.github/workflows/`). The first tests everything; the
  second, from a tag, produces the installer. The first has a separate job that
  fails if somebody puts a binary back in `resources/bin` or reintroduces an
  unlawful downloader tool: the rule holds if something enforces it.
- **`strumenti/versione.js`** and **`strumenti/licenze.js`**, with the npm
  scripts `check:version`, `version:set`, `licenze` and `verify` — which the
  CHANGELOG had been assuming existed for months without their existing.
- **The site's email module has been removed.** It asked for an address, promised
  «an email when your wave opens» and **sent it nowhere**: no backend, no privacy
  notice, and the button turned green all the same. Collecting an address and
  throwing it away is worse than not asking for it. With it, the site's three
  false claims were corrected: «Nothing here is fetched from a network», «macOS ·
  Windows · Linux» when the only target is NSIS, and «no stream».

### Changed — the application identifier

`dev.aether.desktop` → **`io.github.federicobaratti.aether`**.

The first is a reverse-DNS on `aether.dev`, a domain that isn't mine. On
something that gets published that isn't a subtlety, and changing it **before**
the first release costs one person; changing it afterwards costs everyone.

**If you already had a trial library**, the data folder and the keychain service
name move too. In practice:

- the library is in `%APPDATA%\dev.aether.desktop` and Aether now looks for it in
  `%APPDATA%\io.github.federicobaratti.aether`: renaming the folder finds it
  again, or you launch with `AETHER_DATI=...` pointed at the old one;
- the connections to Google Drive and to the scrobbling services have to be
  redone: the tokens sit in Credential Manager under the old service name, and
  looking for them under the new one gives «not connected».

### Added — the core in Rust

A workspace of five crates plus the desktop application:

- **`aether-domain`** — pure, with neither clock nor disk: track and playlist
  keys, text normalization, album grouping, queue with shuffle and repeat, scan
  plan with move detection, reorganization plan, statistics merging, listening
  rules, error catalog. Tested with golden files and parity tests.
- **`aether-play`** — the audio engine, one only: `cpal` plus `symphonia`, three
  threads and a lock-free ring between the decoder and the audio callback.
  Gapless, ReplayGain, volume. In the old tree playback was written twice —
  Howler in the webview, ExoPlayer in Kotlin — and diverged with every change.
- **`aether-app`** — the orchestration: database and migrations, filesystem,
  metadata, cover art, scanning and FTS5 search, playlists, executing the
  reorganization with a journal and an undo, importing from the old database,
  persistence of the queue and the volume.
- **`aether-skin`** — the token registry, parametric effects with a declared
  cost, the parts registry, the compiler, the `.aeskin` format with the guards
  for an untrusted archive.
- **`aether-cloud`** — the Google Drive backup: OAuth 2.0 with PKCE, an ephemeral
  loopback server for consent, Drive REST v3 restricted to the application's
  private folder, the system keychain for the token. It never receives a database
  connection, and that is how «no library lock stays held during a network
  request» becomes a property the compiler verifies instead of a comment.

### Added — the Google Drive backup

What a scan **cannot** rebuild — play counts, ratings, favorites, playlists,
watched folders, installed skins and Studio drafts — existed in one place only,
and a reinstall carried it away.

- **Automatic backup, manual restore.** A pass every quarter of an hour and after
  every change, with debouncing: ten hearts in a row are one save, not ten. The
  local database is never rewritten without somebody pressing «Restore».
- **Three kinds of file, not one archive.** The metadata is seventy kilobytes
  compressed and changes continuously; a skin reaches twenty megabytes and almost
  never changes. A single archive would have re-sent twenty megabytes for every
  rating given.
- **«Nothing has changed» in a single request.** Every file carries its blake3
  fingerprint in `appProperties`: on a library at rest, a pass is one
  `files.list` and nothing else.
- **The remote file never regresses.** Before overwriting, it looks at who wrote
  it: if it was another computer, that one is downloaded, merged in memory with
  `merge_stats` and the union is uploaded.
- **The restore is a plan you look at first.** «1,384 unchanged, 17 to update»,
  each one's delta, the playlists with «22 of 30 tracks present here», the folders
  that no longer exist. Applying it is idempotent: doing it again proposes an
  empty plan.
- **It doesn't destroy.** Counts go up and not down, watched folders are united
  and not replaced, an installed skin isn't overwritten, a Studio draft isn't
  touched. Tracks in the backup without a file on this disk are **listed** and not
  recreated: a row with no file doesn't open, and the next scan would remove it.
- **The scope is `drive.appdata` and nothing else.** The folder is private per
  application and per account, invisible in Drive, and disappears when the user
  removes the app's data. It is a non-sensitive scope: no verification from
  Google. Consent is given in the system browser, never in the webview; the
  refresh token lives in the operating system's keychain and never in the
  `settings` table.

### Added — the desktop application

A Tauri 2 window over the core, with no logic of its own:

- **Library**: watched folders, scanning with progress, the album grid, the track
  list with four sort orders, full-text search, favorites and ratings.
- **Playback**: a floating bar with a slider, volume, shuffle and three-state
  repeat; a queue panel with drag reordering; «play next» and «add to queue» from
  the context menu. The position is interpolated between the core's 250 ms ticks,
  which sends four events a second and not sixty.
- **Playlists**: creation, renaming, deletion with a sync tombstone, adding and
  reordering. Identity is the normalized name, the same key the importer uses.
  Imported automatic playlists can be seen but not edited by hand: their
  membership is decided by the rules, and a recomputation would wipe out any
  manual addition.
- **Library reorganization**: a preview of every single move before touching a
  file, execution with a journal written row by row, an undo.
- **Import from the old database**: preview and then execution, with an explicit
  list of the tracks not found again.
- **Drive backup**: a section in the Settings with the automatic switch, the
  connected account, the time of the last pass, «Save now» and «Restore…». The
  restore is a dialog with the same shape as the reorganization: you see the list
  of what would come back before it comes back. The thread that saves runs in the
  background and takes the library lock in two short windows only, so a pass
  doesn't stall either playback or a scan.
- **Installed skins**: `.aeskin` files read from the data folder, a selector, a
  persistent choice. An unreadable skin disappears from the list instead of
  breaking it.

### Added — the interface redesign

The design document is in `disegno-ux.md`: the five principles with their reasons,
the map of the 47 tokens, that of the 51 parts — **which ones the markup emits and
which it doesn't, with the why** —, the scales, the motion, the keyboard, and the
contrasts measured in both themes.

- **Icon system**: 38 symbols in an SVG sprite mounted once only, `currentColor`
  at 1.6 stroke. Before they were Unicode glyphs (`⏮ ⏸ ⏭ ♥ 🔇`): every system
  draws them its own way and two of them draw them in color.
- **Inter Variable packaged** in `apps/desktop/src/font/`. `--font-sans: 'Inter
  Variable'` was a declaration with no file behind it. Self-hosted: the CSP
  doesn't declare `font-src`, so `default-src 'self'` applies and no request
  leaves the window.
- **Three columns — I navigate, I look, I listen** (240px · the rest · 348px). The
  sidebar goes back to being navigation only: the configuration had been mixed
  into it in five blocks. The third column takes the bottom bar's place; below
  1100px it closes and the floating player goes back to exactly what it was.
- **Settings** is a page, with six sections: folders and scanning, appearance,
  playback, motion and access, from the previous version, library and data. The
  folders and the `.aeskin` files dropped onto the window land there too —
  `dragDropEnabled` was on and had no handler at all.
- **Light, dark or system theme**, with no new command: the compiler already
  emits the `[data-theme='light']` selector.
- **Artists**: a new view, with `list_artists` in `aether-app` and `sort_name` in
  `aether-domain` (The Cure under C). The portrait is a 2×2 mosaic of the real
  cover art: there's no network, and an artist image will never exist.
- **Now playing**: a two-column screen where the navigation **stays** — covering
  everything would force you to close it to change view. Three doors to open it,
  `Esc` to leave. The lyrics and equalizer buttons are there **disabled, with the
  reason written**: the core exposes neither lyrics nor bands, and a fake spectrum
  would be the interface's only lie.
- **Cancellable scan**: `Scan::run` asks the caller whether to continue after
  every batch. Cancelling skips the block of unclaimed disappearances — those rows
  are candidates for moves that the later batches would have paired up, and
  deleting them halfway would lose ratings and favorites. What remains is a
  correct and incomplete library, as after any interruption. Progress follows
  whoever leaves the section, as a notification at the bottom right.
- **Multiple selection**, with a bar that appears in the player's place, and the
  **keyboard map in one place**: `Space`, `/`, `←→`, `↑↓`, `F`, `Esc`, and
  `Alt+↑↓` to reorder the queue — HTML5 dragging isn't reachable from the
  keyboard, and that was the queue panel's defect.
- **Skin Studio**, three views. *Inspect*: a probe that, passing over the live
  preview, says **what the thing you're looking at is called** — it walks up from
  the target comparing the classes with the registry, and what isn't a part
  doesn't light up. *Document*: the JSON, the compiled CSS and the comparison with
  Plain, with the errors carrying `nearest_parts()`'s suggestion as a button; an
  error does **not** switch off the preview, which stays at the last valid state
  and declares it. *Palette*: usage count per color, dynamic sources per
  individual token (the text never follows the cover art), capabilities, and the
  checklist before export — blocked by the errors, not by the warnings.
- **«Create theme»**, in Settings › Appearance. The Studio only knew how to **open
  a skin that already existed**: `studio_documento` answers `skin.notFound` to an
  identifier it doesn't know, and the «Derive…» button on the stock skin passed
  `plain`, so the draft ended up in `bozze/plain/` and the manifest went on saying
  `"id": "plain"` — that is, it produced a package `skin_installa` deliberately
  refuses. Now a dialog asks for name, author, description and which skin to start
  from; the identifier is **derived from the name** and shown as you type it,
  obeying the format's strict rule (2 to 48 of lowercase letters, digits and
  hyphens, beginning with a letter) rather than the looser guard on paths. The
  theme is born **installed and active**, not as a draft: without that, the live
  preview wouldn't be the application — which is the second of the three rules the
  Studio is built on.
- **`skin_installa_sorgente`** and the **«Save and use»** button in the Studio: a
  manifest gets installed without going through a file on disk. Before, the only
  road was exporting an `.aeskin` into some folder and reinstalling it from the
  Settings' dialog — two path choices for a file nobody wanted. The package is
  **read back** before being put down, that is, it goes through the same guards as
  a skin that arrived over the network. «Export .aeskin» stays, and serves what it
  really served: **giving** a theme to somebody else.
- **Three new parts** in the registry, from 48 to 51: `list-row`, `segmented`,
  `selection-bar`. And the markup now names them: **it emits 42 of them**, where
  before it emitted zero — that is, there was a skin system no skin could use
  beyond the tokens.
- **`meta.preview`**: the three colors on a skin's card are chosen by its author.
  Before, the interface guessed them by taking `surface.0`, `surface.2` and the
  accent, which for two skins out of three worked by accident.
- **Contrast warning** in `check_skin`: every text/surface pair below 4.5:1, in
  **both** themes. `contrast_ratio` existed and only the tests called it.
  `cargo run -p aether-skin --example contrasti` prints the table, and that's
  where the document's numbers come from.
- **`SkinIpc` carries layout and motion**: `layout` and `motion.intensity` were
  compiled and nobody read them. The interface now honors them, with the system's
  `prefers-reduced-motion` always beating the skin.

### Added — the graphic finishing

The redesign had finished the **system**; this pass does the **material**. Up to
here the stylesheet contained not one `color-mix`, not one blur and not one
keyframe animation: every surface was a flat tint laid on another flat tint.

- **The cover art was all the 160-pixel thumbnail.** `Copertina` always asked for
  the `.t` file, even for the «Now playing» cover, which is three hundred and
  sixty wide and on a 150% screen is five hundred and forty real pixels: a 160
  JPEG enlarged three and a half times. Now the original arrives in the three
  places where the cover art **is** the subject — full screen, third column, an
  album's header — and the grid stays on the thumbnail, where it's the right
  choice. An album's header, which had had no image at all since the redesign, has
  one again.
- **The ambience is the cover art.** Behind the big cover and behind the third
  column there is the image itself, enlarged and blurred by sixty-four pixels,
  with the earlier `--hero-rgb` gradient underneath for when there is no cover.
  Better than an extracted tint, because a red record with a yellow band gives a
  red **and** yellow ambience. The `np-scrim` veil stays neutral and stays on top:
  it's the line that guarantees the title's contrast.
- **A model of light.** Five new properties in `stile.css` — `--spigolo`,
  `--incavo`, `--alzata`, and the two already-composed forms `--luce` and `--filo`
  — written with `color-mix()` inside `light-dark()`, so with not a single literal
  color and with the direction inverting itself between the two themes. Every
  raised surface has a bright thread on the top edge and a gradient that runs out
  going down; every groove — tracks, fields, bars — has its own recess.
- **Shadows go from one layer to two**: a short contact and a wide ambient. A
  single layer cannot say at once where an object touches and how high it sits.
  The test isn't a fixed value but the property, in
  `le_ombre_hanno_un_contatto_e_un_ambiente`.
- **The radii come from the tokens.** Six fixed numbers — 4, 5, 6, 8, 10 — sat
  inside containers that used `--radius-card`: a skin that squared everything off
  left six rounded corners. They are derived now, and zero stays zero.
- **The view transitions, which the skin had been writing all along.**
  `motion.routeTransition` was declared in `plain.json`, compiled into two
  `@keyframes` and two `::view-transition-*`, and never executed: the call to
  `startViewTransition` was missing. The application's stylesheet contains no
  duration
  or curve for that crossing — they belong to the skin. Whatever doesn't take
  part carries a `view-transition-name` of its own.
- **A single global focus ring**, where before there were seven local ones and
  everything else fell back to the engine's default outline.
- **The loading placeholders**: `skeleton` was a part registered and emitted only
  by the Studio's preview. `caricaVista` waited and then substituted, so during
  the request the previous view's content stayed standing.
- **The page header wraps.** With the third column open on a 1280 window the
  content is six hundred pixels wide, and the title — the only `flex: 1` — shrank
  to a single letter while the search field stayed as wide as on a full screen.
- **The full-screen title is fluid and sits on two lines**, measured against the
  right container: «Solar Sailer - Remixed by Pretty Lights» read «Solar Sailer -
  …», that is, without the part that distinguishes one version from another.
- **The display typeface arrives where the title is the page**: an album's
  header, the statistics numbers, the mark, the empty states, the third column's
  title. And `tabular-nums` where a number in sans changes in place.
- **«Sala», the second skin.** The listening room: warm blacks, brass, the red of
  a label at the heart, monospace as the display face. A single light source — the
  `lampada` pattern — and nine cost points out of ten. It's the proof that the
  parts registry earns its place: Plain repaints none of them, Sala seven.

### Added — the equalizer

Ten octave bands — 31, 62, 125, 250, 500 Hz, 1, 2, 4, 8, 16 kHz — adjustable by
±12 dB, with stock presets and savable presets. It's in two places: a panel from
the button next to the volume in the player bar, and a card in Settings ›
Playback. One component, in two sizes, like the transport.

- **`aether-play::equalizzatore`**: a cascade of ten *peaking* biquads from the
  RBJ cookbook, in transposed direct form II with the state in `f64`. The form and
  the precision aren't pedantry: the 31 Hz band on a 48 kHz output has its poles a
  thousandth from the unit circle, and in `f32` in direct form I that filter
  doesn't sound wrong — it hisses. No `mul_add`, which without FMA enabled becomes
  a call to libm's `fma()` inside the audio callback.
- **The filters live in the callback, not before the ring.** Filtering in the
  decoding thread would have been much simpler and would have made every slider
  respond two hundred milliseconds after the finger — the ring's reserve. The cost
  of doing it in the right place is a **second lock-free ring**, the coefficients'
  one: fifty floating-point numbers can't be published with atomics without
  somebody being able to read half of one curve and half of another.
- **Automatic pre-amplification, measured instead of guessed.** The obvious
  shortcut — attenuating by the most-boosted band — is off by a lot: ten bells an
  octave wide overlap, and raising them all by 12 dB produces about twenty in the
  middle of the spectrum. The curve is evaluated on a grid of 89 frequencies
  **anchored to the bands' centers**, and attenuated by the real peak. With a
  sparser grid the measurement was off by over a decibel and the output clipped
  anyway: the two tests that prove it are in the file.
- **A curve change is interpolated in ten steps** in the decoding thread, on the
  decibels and not on the coefficients — the road between two stable biquads
  passes through biquads that aren't. It's the volume's `RAMPA` carried to where
  the ramp didn't reach: without it, loading a preset with the music on makes a
  crack.
- **The filters' state is cleared along with the ring**, on the same flush that
  follows a seek: it's a remnant of the earlier point exactly like the samples
  still in flight.
- The curve and the presets live in `settings` next to the queue and the volume
  (`player.eq`, `player.eq.presets`), so no migration. A curve of the wrong length
  is normalized instead of being thrown away, and the values are clamped on
  reading: that file can be opened and corrected by hand.
- **`equalizzatore` is the only playback command that doesn't send
  `riproduzione:stato`**, and that isn't an oversight: composing that state
  requires reading the current track from the database, and the command goes out
  about fifteen times a second for as long as a slider is under a finger. It sends
  `riproduzione:eq`, which is two fields and no query.

### Fixed — what could only be seen by using it

Two big defects no document listed, because they aren't found by reading: one is
found by counting the rows of a real library, the other by looking at the window
while it plays.

- **Track two hundred and one was unreachable from any view.** The list asked for
  a page of two hundred and asked for no more: no button, no scrolling, no message
  — it simply ended. On the library of the person who works on it, two hundred and
  fifty-eight tracks, fifty-eight were missing. Likewise beyond the four hundredth
  album, and favorites were fetched by asking for **two thousand** tracks and
  filtering them in the window: the whole library read at every visit, and whoever
  has more didn't see the ones beyond.
  Now there is `usePagine`, with a sentinel at the end of the list watched by an
  `IntersectionObserver`. Three new queries in the core: `list_liked` (ordered by
  when the heart was given), `albums_by_artist` — an artist's page **filtered** in
  the window the albums already downloaded, so an artist beyond the four hundredth
  gave an empty grid under a title saying «three albums» — and `search_count`,
  because «N results» stated the length of the first page: «60 results» for a
  search that had three hundred.
- **The window redrew itself twenty times a second while playing.** The
  interpolated position was a `useState` inside `App`: every 50 ms tick redid the
  header, the body with all the list's rows, and the `contesto` object — whose
  `useMemo` had `posizioneMs` among its dependencies, so it was doing nothing. The
  keyboard listener was removed and put back on every tick too. It contradicted
  `disegno-ux.md §9`, which declares that no component must ask for anything at
  that rate.
  The position now lives in an external store read with `useSyncExternalStore`,
  and **one leaf alone** reads it: `Scrubber`. Measured with a temporary counter in
  `RigaBrano`, eight seconds of music: from about forty-one thousand row draws to
  **zero**.
- **The `#` column said something meaningless outside an album.** It showed the
  track number inside **its** album, so in a flat list it read `1, 39, 8, 1, 5, 1,
  9…`. Now it's the track number only inside an album, where it serves to find the
  piece on the sleeve, and the list position elsewhere.
- **Reordering tracks in a playlist was in the IPC and unreachable.**
  `playlist_riordina` had existed since day one and nobody called it: the queue had
  both dragging and `Alt+↑↓`, a playlist's list had neither. Now it has both — and
  both finally have a **drop indicator**, which the queue lacked too: you dragged
  without knowing where the row would end up.
- **The three numbers on a statistics card lost their baseline** when an eyebrow
  wrapped: «NO MATCH» over two lines dropped its number fifteen pixels below the
  other two.

### Added — the spectrum, and it isn't fake

The button was there, disabled, and the tooltip told the truth: «the audio engine
exposes neither samples nor bands, and drawing a fake one would be the interface's
only lie». Now it exposes them, so the reason it was disabled is gone. The rule
hasn't changed: what is drawn comes from the sound that comes out.

- **`aether-play::spettro`**: the samples are taken in the audio callback — the
  only place that sees what actually comes out — and pass through a **third
  lock-free ring**, after the two the crate already had. A `push` that fails loses
  the sample, and that's the right hierarchy: the alternative would be making the
  callback wait, that is, the one thing realtime forbids.
- **After the equalizer and before the volume.** After, because a curve that
  raises the bass must be visible; before, because a spectrum that drops when you
  turn the knob down describes the knob and not the music.
- **A hand-written 4096-point radix-2 transform.** The repo's rule is that a
  dependency has to be argued for, and for a real transform of fixed length the
  argument doesn't hold. Four thousand points and not one thousand: at 48 kHz
  that's 11.7 Hz per bin, and the 31 Hz band occupies two bins — with a thousand it
  would have occupied half of one, and the first bar would have shown DC instead of
  the bass. Five tests plus Parseval.
- **DC is removed before windowing.** Multiplying a fixed value by a Hann window
  gives the window, whose spectrum spills onto the neighboring bins — which at 48
  kHz fall inside the 31 Hz band. Without it, any offset in the chain would show up
  as an enormous bass that isn't there.
- **The octave's energy is summed, not averaged.** Octave bands have proportional
  width: two bins for 31 Hz, nine hundred for 16 kHz. Averaging, the highs stayed
  at zero even on a piece full of them. It was found by looking at it.
- **The bands are the equalizer's ten**, so `eq-bars` and `eq-slider` — two parts
  the registry puts side by side — describe the same thing: the bar above the 250
  Hz slider says how much energy there is at 250 Hz.
- **Thirty events a second, and only while the screen is open.** It isn't a
  contradiction with the position's four: the window knows how to interpolate that
  one, the bands it doesn't. Switched off, the callback doesn't write and the
  thread doesn't send.
- **`prefers-reduced-motion` doesn't switch it off: it slows it.** The bars stay
  true and are redrawn four times a second.

### Added — three holes in the skin format

- **`blurBehind` can be hooked to a part.** `PartAppearance` now has `filter` next
  to `clip`, read with `EffectTarget::Filter` — which already existed, already knew
  it ended up in `backdrop-filter`, and had no field referring to it: a skin that
  tried received a correct error on a road that didn't exist. And the **cost
  enters the budget**: `effects()` summed the backgrounds alone, so a ten-point
  blur — the whole budget — would have gone through in silence.
- **The dark frame at startup with a light skin is closed.** The window is born
  with `"visible": false` and the `pronto` command shows it, two frames after skin
  and theme are on the document. A safety net in Rust shows it anyway after two
  seconds: an invisible application would be a worse failure than the defect being
  removed.
- **The accent follows the cover art, and OKLCH decides the contrast.**
  `capabilities.dynamicAccent` was declared, `dynamic_tokens` compiled, and nobody
  extracted a color from it — because `--accent` is also a **text** color and a
  record's vivid tint doesn't guarantee 4.5:1. Now the tint is extracted in Rust
  from the thumbnail already on disk (`aether-app::tinta`: ten-degree buckets on
  hue, circular mean, black, white and grays discarded) and `aether-skin::dinamico`
  carries it into OKLCH, slides it **in lightness** in both directions starting
  from the cover's, and takes the first that holds the threshold on all the theme's
  surfaces *and* for the text that goes on top of it. Hue and chroma stay the
  record's; if no lightness passes, the answer is `null` and the skin's accent
  wins. Three things that matter more than the mechanics:
  - **The threshold is the same as `check_skin`'s** — same surfaces, same
    `CONTRASTO_MINIMO`. An accent written by the cover art passes exactly the tests
    of one written by hand: there aren't two definitions of «legible accent» in the
    same program.
  - **No canvas.** Reading the pixels in the window would have imposed an
    `Access-Control-Allow-Origin` on the `aether-cover` protocol, that is,
    widening for a decoration a contract kept narrow on purpose, and putting
    twenty-five thousand pixels on the interface thread at every track change.
  - **Only the family's four tokens are written**, with the alpha the skin had
    already declared, and only if the skin declares `dynamicAccent: true`. The text
    never follows the cover art, and a black-and-white record changes nothing. The
    switch in the Settings stops being disabled; on a skin that says no it stays
    blocked, with that reason on screen.

### Fixed — in this rewrite

- Reordering a playlist violated the `PRIMARY KEY (playlist_id, position)`
  constraint when the move went backwards, because SQLite checks the constraint on
  every statement and the order in which it touches the rows isn't documented. Now
  the order is rewritten in full instead of being moved in place.
- The missing-cover placeholder had no dimensions in the list rows or in the
  album header: the CSS pointed at a class (`.segnaposto`) no component emitted.
- The stock skin's `color.text.3` was `rgba(255,255,255,.38)` on `#09090d`,
  that is **3.47:1**, below the 4.5:1 threshold the crate itself declares. It's
  now `.46`, which gives 4.71:1. It's the color of durations and secondary
  metadata — almost every number in the list.
- Choosing a skin and previewing one with the mouse are **two asynchronous calls
  with no ordering between them** that end up in the same place: the text of a
  single `<style>`. When the pointer leaves a card, the preview that puts back the
  skin *active at that moment* goes out, and if that answer arrives after a choice
  just made, it rewrites the sheet with the previous skin — the list would say «in
  use» on the new one and the window would stay the old color. Now there's a round
  counter, and the last request sent wins: even when it's a preview, otherwise
  moving the mouse over the cards straight after choosing would show nothing at
  all. Found by reading, not on screen: the two roads cross only with a real
  pointer, and the autopilot the rest was tested with can't produce a
  `mouseenter`.
- Previewing a skin removed the cover art's accent and didn't put it back.
  `applicaSkin` clears it on purpose — an accent tailored to another skin's
  contrast is worth nothing — but previewing a skin with the mouse changes neither
  the track nor the theme nor the chosen skin, that is, none of the dependencies of
  the effect that writes it: passing over a card and leaving kept the skin's accent
  until the next track. Now the preview's **revocation** puts it back; during the
  preview it stays removed, because the core tailors the accent to the chosen
  skin's surfaces and what's on screen is the previewed one. Same road as the
  other, and the same note: verified by reading.
- The light theme declared in `capabilities` did not exist: `themes.light`
  overrode eight entries — the sidebar, the hairline and the six semantics — and
  left surfaces and text dark. Whoever chose it got a dark theme with the wrong
  semantics. It now declares all the tokens it must, and a test proves it by
  reading the contrasts instead of counting the keys.

### Fixed — the navigation, by trying it

Seven defects found by **using** the window, not by reading it. Three were
commands that lit up without doing anything; the others could only be seen on
screen.

- **`selection-bar` and `queue` weren't in the stock tree.** They were in the
  widget registry, in the parts registry and in the renderer, and `default_shell`
  didn't mount them: the bar's «Queue» button lit up and opened nothing, and
  choosing rows made the player **vanish** — it hides on purpose to make room for a
  bar nobody had mounted — with nothing in its place. A test now covers all nine
  variants of the tree.
- **The header and the body decided in two different orders which page you're
  looking at.** Opening an album from an artist's page showed its tracks under the
  artist's title — with no cover art and no «Play» — and the «‹ Artists» button led
  *forward*, into the album's page. The two case lists now have the same order, the
  button goes back where you came in and says so, and `Escape` closes one level at
  a time instead of two.
- **«Now playing» full screen left the third column open**: cover art, title,
  transport, rating and queue drawn twice side by side, with the full screen
  squashed into half a window. And its cover art was measured in `vh` — the whole
  window — instead of against the space it actually has, so on an 820-pixel window
  the title was cut off on the second line.
- **A click on a playlist from Settings did nothing**: the playlist lit up in the
  bar and the page stayed the settings one.
- **The «Accent follows the cover art» switch was local state nobody read.** The
  compiler listed the tokens that would follow the cover art (`dynamic_tokens`) and
  nobody, neither in the window nor in Rust, extracted a color from it. The
  function now really exists, and the switch commands it: see «three holes in the
  skin format» above.
- **The button that removes a row from a playlist wrapped**: `.riga` declared
  seven columns and relied on `grid-auto-columns` for the eighth, which is the
  property of implicit *column* tracks — with row flow the eighth child goes onto a
  new row, and the × appeared under its row.
- **The selection bar ended up under the third column**, «Close» included: the
  notification already had the rule that stops it earlier, this one didn't.

### Added — the whole Spotify account, by two roads

Up to here Spotify was imported **one link at a time**. What was missing was the
big gesture: bringing in all the playlists, saved tracks, albums, followed
artists and listening history without pasting thirty addresses by hand.

There are two roads because they're complementary, not alternative, and neither
of the two is enough:

- **OAuth consent** (`aether-spotify::account`) gives the ISRC — matching's step
  zero, which the keyless reader no longer receives — and can be repeated whenever
  you like. But since February 2026 an application in Development Mode requires the
  owner to have **Premium active**: if it lapses, it stops working and Spotify
  warns nobody. The screen says so **before** connecting.
- **The ZIP archive** Spotify sends on request (`aether-archivio`) asks nothing of
  anyone and carries **years** of history, where the API gives fifty rows. In
  exchange it arrives in two pieces weeks apart, and only one of them may be
  opened: what's missing stays empty and the preview says which half.

The pivot is that both produce the **same domain value** (`AccountSnapshot`), so
from there on the code is one: one plan, one confirmation, one transaction. The
window has a single screen, and doesn't know where what it's showing came from.

What follows from that:

- **`aether-oauth`**, extracted: PKCE, the loopback server and the keychain sat
  inside `aether-cloud` and knew about Google. Now they serve two masters and name
  neither.
- **The scopes are all read-only**, with a test forbidding their widening. Aether
  must not be able to touch anybody's account.
- **`play_history.source`** distinguishes imported listens from real ones, and it's
  what makes the import **undoable**: «forget the imported listens» is a targeted
  `DELETE` plus a recomputation, instead of a restore from backup — that is,
  instead of also losing everything done in the meantime.
- **`playlists.spotify_playlist_id`**: without it, renaming a playlist on Spotify
  would create a second one here at the next sync, because `PlaylistKey` is born
  from the name.
- The missing tracks end up in `spotify_wanted` and the yt-dlp queue starts by
  itself, as it already did for a single link.

Three defects only running it revealed:

- **The duration the archive never writes.** `SpotifyTrack.duration_ms` is always
  `None` in there — none of the four formats has a field for the duration — and
  `counts_as_play` without a duration falls back to the four-minute threshold: it
  would have discarded **every listen of every song shorter than four minutes**, in
  silence and under the label «too short». Now matching comes before the threshold,
  and the duration is given by the library. No unit test would have caught it: the
  hand-written snapshots all had it.
- **The declared total counts podcasts too, the tracks don't.** A playlist of fifty
  songs plus a podcast came out as «50 of 51» on every read, and `prepara_playlist`
  **refuses to replace** a playlist that arrived incomplete: that playlist would
  have become impossible to reimport, forever, because of a podcast.
- **Since March 2026 the response bodies too** have renamed `tracks` to `items` and
  `track` to `item`, and the guide declares it for playlists while staying silent
  about the other lists. Both names are read: getting the right one wrong imports
  zero tracks from a full account, **with no error at all**.

### Added — the controls that existed only as an explanation

Five things the core was already doing that couldn't be seen or changed from the
window. They weren't missing features: they were features **present and
unreachable**, which is worse, because the code that does them keeps running and
nobody can correct it if it's wrong.

- **The downloads folder can be chosen.** `download.folder` was read in
  `scarica.rs` and nobody wrote it: the yt-dlp queue always ended up in the first
  watched folder, and which one that was depended on the order they had been added
  in. The card shows where the tracks actually end up — the choice or the fallback,
  distinctly — and **warns** when the chosen folder sits outside the watched ones:
  files arrive there and don't appear in the library, which to the person looking
  is indistinguishable from a failed download.
- **ReplayGain can be switched off.** The engine has always applied it with the
  −18 LUFS reference; the switch was drawn disabled with its reason next to it. Now
  there is `player.replaygain`, and the default value is **on** — not out of
  preference but out of continuity: switching it off quietly in an update would
  change the volume for people who have the tags with nothing explaining it.
- **The listening history can be read.** `play_history` had been written since day
  one and the tab next to «Queue» was disabled. It was a small defect as long as
  that table contained only the listens made in here; after importing a Spotify
  account it contains years, and a screen that doesn't show them is the difference
  between having imported and not having done so. Every row also says **where it
  comes from**, which is the other half of `source`.
- **The return journey is visible.** `scarico:riconciliato` went out and nobody
  listened to it. It wasn't just a missing piece of news: that pass **closes** rows
  of `spotify_wanted` without going through the queue, so the panel went on showing
  tracks «waiting» that no longer existed.
- **A save to Drive has a bar.** `nuvola:avanzamento` was listened to only by the
  restore dialog: a «Save now» from the settings showed «Saving…» and nothing else,
  for tens of seconds.

### Added — `AETHER_DATI`, to try things without risking them

The environment variable replaces the application's data folder. It exists
because almost everything irreversible Aether does — reorganizing the files on
disk, importing a whole account, restoring a backup — can be read in a unit test
and can only be **judged** by watching it happen in a real window, on a library
that resembles somebody's. Without it, those two things are the same library: the
only one, the user's. With it, you copy the database into some folder and break
everything as well.

It's read in one place, at startup; the rest of the program sees a path and
nothing else, as before.

### Fixed — an absence written as an empty string

`nuvola.email`, `nuvola.file_id`, `nuvola.impronta` and `nuvola.client_id` were
«deleted» by writing `""` into them. An empty string and an absent row are two
different states, and every reader had to remember a
`.filter(|e| !e.is_empty())` so as not to show a nameless account in place of no
account. Now there is `settings::forget`, which removes the row — and there's the
reason that would be enough on its own: an identity the user asked to forget
shouldn't stay written in a file the backup copies away.

### Fixed — a lost audio device now says so, and can be reopened

It was the known defect of the previous phase: playback stopped and the window
went on saying it was playing. `Motore::dispositivo_perso()` and `causa_perdita()`
had existed since day one and **nobody read them**, so a vanished sound card — a
virtual device switching off, a USB headset unplugged — was indistinguishable from
a track that doesn't start.

Now the clock notices it on the edge, the window shows a band with the cause, and
there's a **Reopen** button that opens the new engine **before** throwing away the
old one, restoring volume, equalizer and normalization. It works in the case where
the engine never opened at all too: an application started with no sound card now
declares it instead of staying mute, and `riproduzione_stato` answers with a
stopped state and the reason instead of failing.

### Added — playlists from files, and playlists that write themselves

- **M3U, M3U8, PLS and XSPF**, reading and writing (`playlist_file.rs`, pure).
  BOM, CRLF, relative and absolute paths, `file://` with `%NN` sequences. The
  import matches first by path — exact, then by file name — and then falls back on
  the **same four-step ladder** as the Spotify import, so a playlist exported from
  another program finds its tracks even if the files have been moved. What it
  doesn't find it **lists**, with the path that was written there.
- **Smart playlists.** The `is_smart` and `rules` columns had been in the schema
  since day one and nobody wrote them. Now a set of rules on eleven fields —
  artist, album, genre, year, rating, favorite, plays, duration, added, last
  played, format — becomes **parameterized SQL**: never a string concatenation, and
  a test puts `'; DROP TABLE tracks; --` inside it to prove it. The preview is live
  while you write: you watch the count fall from 1421 to 5 as you type.

Three defects found by running things, not by reading the code:

- **`<trackList>` begins with `<track`.** Searching for the bare substring made
  the list's opening be taken for a track, whose block ended at the first real
  `</track>`: **the first track of every XSPF disappeared**, always, on perfectly
  valid files.
- Smart playlists said «0 tracks» in the sidebar: the count came from
  `playlist_tracks`, where there is no row for them.
- A condition just added was declared «doesn't hold up» before anything had been
  written into it. Incomplete rules are now neither sent nor saved.

### Added — scrobbling: ListenBrainz and Last.fm

Sending out what you listen to, with the **same rule** that counts everything
else: half the track or four minutes, whichever comes first, never under thirty
seconds. It isn't a convenience of reusing a function — it's that `play_count`,
the history and the scrobble must count the **same thing**, and a second measure
written here would be the defect `listen.rs` exists to correct, back in through
the service door.

- **`aether-scrobble`**, a new crate, doesn't see `rusqlite`. It isn't hygiene:
  the queue empties a thousand listens at a time, and if this crate could touch
  the connection the natural thing to write would be «read a row, send it, delete
  it» — that is, holding the library lock for the whole duration of the network,
  with playback stopped behind it.
- **A queue on disk** (`scrobble_queue`, migration 6). What doesn't go out
  because there's no network isn't lost and restarts by itself, even after a
  shutdown. The rows carry **the tags as they were then**, not a `track_id`:
  between the listen and the sending the track may have been deleted, moved or
  retagged, and what has to be sent is what was listened to then.
- **The two orphaned error codes find their callers.**
  `settings.lastfmNotConfigured` and `settings.lastfmNoPendingToken` sat in the
  catalog without a single caller: the second is precisely Last.fm's consent
  requested when the first leg was never done — because that consent is in two
  legs and in between there's a person coming back from the browser.
- **The history imported from Spotify can be sent to ListenBrainz in bulk**, and
  it's the circle closing: years of listening become your own history on a service
  that belongs to no platform. Only there, and it isn't a preference — Last.fm
  refuses old dates and has a daily ceiling.

Two things worth more than the sum of the lines they cost:

- **A rejected block is split instead of being thrown away.** ListenBrainz
  evaluates the whole document: a single listen with an impossible date makes it
  answer `400` to all thousand. Marking the block as failed would throw away nine
  hundred and ninety-nine good listens because of one, in silence and according to
  the rules. It halves until the culprit is left alone.
- **An ignored listen is a delivered listen.** If Last.fm answers «3 accepted, 2
  ignored», those two will never come back accepted — the date is what it is.
  Putting them back in the queue would mean resending them forever.

`aether-net` has learned a second header: ListenBrainz doesn't send `Retry-After`
but `X-RateLimit-Reset-In`, and without reading it a `429` would wait blindly for
thirty seconds when two were enough — multiplied by every block of a queue that
empties.

### Added — the settings can be searched, and taken away

Nine sections are past the point at which a thing is found by scrolling.

- **A search across the sections.** The index is written by hand and not derived
  from the page's text, because the page's text is that of the **open** section: an
  engine seeing a ninth of what is there would say «it isn't there» about things
  that are. Half the index is synonyms — nobody searches for «normalization», you
  search for «volume» or «replaygain».
- **The settings profile**: a JSON file with your own choices, to be reopened on
  another computer or after a reinstall. With the **plan** before applying, like
  every other irreversible thing here: `profilo_piano` is `profilo_importa` in a
  transaction that gets abandoned, so the list you read isn't a prediction, it's
  the result.
- **The shortcuts can be reassigned.** `tastiera.ts` was a `switch`: changing one
  meant recompiling, and showing them on screen meant rewriting them by hand in a
  second list that sooner or later drifts. Now the table the listener consults is
  the one the card draws. You press the key instead of writing its name, and `Esc`
  can't be assigned: it's the way out of every field and every dialog.
- **The theme has come back into the core.** It was in `localStorage` for a reason
  that was good when it was written — it's a window preference, not library data.
  In the meantime two readers of *all* the preferences have been born, the Drive
  backup and the profile, and a preference in `localStorage` ends up in neither:
  whoever restored a backup got the right skin and the wrong theme. It's now
  `ui.theme`, and the first opening after the update recovers the old choice and
  rewrites it inside.

Three decisions that are visible only if something goes wrong:

- **The profile carries a list of inclusions, not of exclusions.**
  `nuvola.dispositivo` must not travel: two computers with the same identifier
  ruin each other's backup, and you notice months later. Nor must `player.queue`,
  which contains identifiers of `tracks` rows — on another library they name
  different songs. The flip side of an inclusion list is that forgetting a key is
  silent, so the export **declares** which keys it left behind.
- **A profile from a future version is refused instead of being half read.**
  Reading the keys it recognizes seems generous and produces a half-configured
  machine, without saying which half.
- **A key assigned to two commands is declared, not refused.** The moment it
  happens is halfway through a move — for an instant both have it — and refusing
  the second assignment would force you to take the steps in the right order
  without saying so. In the meantime the first in the list wins, so there is no
  instant of unpredictable behavior.

### Known defects

- **Spotify's OAuth consent has never been tested against real Spotify.** It
  requires a registered client id and a person's hands on the consent screen, so it
  remains the only part of the account import that hasn't been through a real
  network. The other road — the ZIP archive — has, on a whole account.
- **The same goes for the two scrobbling services.** The document that gets sent,
  Last.fm's signature, the reading of the responses and the fate of every row in
  the queue are tested; the first `200` from `api.listenbrainz.org` requires
  somebody's token.
- **An imported profile is only half visible immediately.** Theme, skin and
  shortcuts change while you watch; volume, equalizer and normalization are read by
  the audio engine when it opens, and stay as they were until a restart. The card
  writes this down instead of leaving it to be discovered.
- One thing the core can do and the IPC doesn't expose: the lyrics in
  `tracks.lyrics`. Its control is drawn **disabled, with the reason on screen**,
  instead of being omitted or — worse — faked. The spectrum's bands, volume
  normalization and the listening history were the other three entries in this list
  and are no longer.
- **The dynamic accent moves the tokens, not the colors rewritten in full.** The
  family's four entries and everything the compiler turned into `var(--accent)`
  follow the cover art; a skin that had repeated the same color **literally**
  somewhere else — a border, a shadow — would be left behind, and the window would
  be half one color and half the other. There's no way of noticing it on your own:
  two identical colors in the document don't declare themselves to be the same
  color. The two stock skins don't do it.
- **A queue started from a paginated list is as long as the pages downloaded.**
  «The queue is the list you're looking at» remains the rule, but that list now
  grows as you scroll: starting from the first track of a two-hundred-and-fifty
  library queues two hundred, and after scrolling to the end, all of them. It hides
  nothing that could have been seen — the ceiling used to be two hundred and that
  was that — but it's a difference worth stating.

### Added — in the previous rewrite (TypeScript/Electron)

What follows belongs to the old tree. It stays here because the Rust core
inherits its decisions, not its code.

- **Error core**: an `AppError` as a serializable record, with a single catalog of
  domain, severity, retryability and i18n key. It crosses the IPC and the network
  without loss, in place of the ten subclasses that in the old tree died at the
  first hop.
- **Typed IPC contract**, structured logger, supervisor, and a resilience layer
  (retry, timeout, circuit breaker, rate limiter).
- **Database layer**: an abstract driver, a migration chain unified from the two
  divergent ones, opening with diagnosis, parity proved rather than agreed.
- **Playback**: explicit state, engine errors translated into codes, recovery.
- **`.aeskin` skin format**: a skin is data, not CSS. Token registry, parametric
  effects with a declared cost, parts registry, compiler, and a package format
  with the guards for an untrusted archive (path traversal, zip bomb, lied-about
  type).
- **Library of installed skins**, with an injected filesystem so it runs both on
  `node:fs` and on Android's Storage Access Framework.
- **Skin transport**: routes, execution of an alignment plan, and the two ends over
  HTTP. Tested on a real port, with two libraries aligning in both directions.
- **Skin Studio logic**: draft and contrast check.
- **Release infrastructure**: electron-builder configuration, CI and release
  workflows, a guard on version alignment.
- **Complete scanning**: from disk to a queryable library. Writing in batches, each
  in its own transaction — an interrupted scan leaves a correct and incomplete
  library, which the next pass finishes, instead of leaving nothing. Aggregates
  rebuilt from the tracks, FTS5 search with the operators quoted. Measured on the
  real library: 1421 tracks in 19.5 s the first time, 0.1 s the second.
- **A track that moves is not a new track**: disappearances are paired with new
  files by track key, and the row is updated instead of being deleted and
  recreated. Without that, reorganizing the library — the feature added shortly
  before — would zero the play counts, favorites and ratings of every track, and
  remove them from every playlist.
- **Statistics merging** (`aether-domain::merge`): commutative and idempotent, with
  every rule chosen in the direction that doesn't destroy — a count goes up and not
  down, a zero doesn't erase a rating, a favorite removed isn't put back by the
  other device. The importer needs it and device-to-device sync will need it
  identically.
- **Importer from the old database**: it brings play counts, ratings, favorites,
  history, playlists and tombstones — the only things a scan cannot rebuild. The
  old database is opened read-only and isn't touched. On the real library: 261
  tracks found again out of 297, 409 listens, 82 history rows, the automatic
  playlist with its rules.

### Fixed

Defects found in the old tree and not carried into the new one:

- `classifyDownloadFailure` had opposite defaults on desktop (`permanent`) and
  mobile (`transient`) for the same failure.
- Eight orphaned i18n keys, from an error table duplicated in three places.
- ExoPlayer's error code was discarded; Howler showed «2» in the UI.
- `AppError.from` looked for the errno only at the top of the value it received:
  `fetch` reports a connection refusal as `TypeError: fetch failed` with
  `ECONNREFUSED` one link below, and the LAN transport's commonest case lost its
  domain and its retryability.

## [1.0.0] — 2026-07-17

Published from the old tree, which remains in `legacy/Aeter/` as a read-only
reference; the installer is `legacy/Aeter/release/Aether Setup 1.0.0.exe`. The
changes before this line have not been reconstructed: the history is in the old
tree's git log.
