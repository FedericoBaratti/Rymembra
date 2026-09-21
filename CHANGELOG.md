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

- **patch** — everything the application does to itself, however much of it
  there is: fixes, and features too — see the second amendment below;
- **minor** — a release after which something *outside* Aether has to keep up:
  a skin that must declare a token it did not declare before, a phone that must
  send a field it did not send before. Nothing breaks in the meantime — that is
  what separates this from a major;
- **major** — an incompatible change to the skin format
  (`SKIN_FORMAT_VERSION`) or to the transport protocol
  (`SKIN_TRANSFER_PROTOCOL`), that is, the two points at which an updated device
  would stop understanding one that stayed put.

### Migrations and the number — amended in 2.3.1

That list used to end the minor clause with «and every database migration
(migrations only go forward: a minor is the signal that going back requires a
restore)». 2.3.1 migrates the schema and is a patch, so rather than leave the
document arguing with the release, here is what that clause was protecting and
where it went.

A migration cannot be undone. There is no `down` — a migration that claimed to
know how to go back would be claiming to rebuild information it had just thrown
away — and a database written by a newer version **refuses to open** in an
older one, with `db.versionAhead`, instead of being «fixed». That is true of
*every* migration, the purely additive ones included: a column nobody older
reads is still a `user_version` nobody older accepts.

Which is precisely why the number was the wrong place to say it. It made «this
release adds a column» and «this release adds a feature» the same
announcement, and it left no honest way to ship a fix that happens to need a
column. So the warning moves out of the number and into the text: **every
release that migrates names its migrations in its own entry and says what
going back would cost.** The number goes on meaning what it means everywhere
else. 2.3.1 is the first release under this rule, and its section is below.

### Features and the number — also amended in 2.3.1

The other clause in that list used to read «**minor** — new features», and
2.3.1 argued with it just as loudly. This release adds a Folders panel, a
window zoom, a guided tour, a word-level lyric editor, animations in the skin
format and a profile that carries the whole library — and it is a patch. Same
treatment: here is what that clause was protecting and where it went.

It was protecting somebody deciding whether to update now, and the way it did
that was by making the number a measure of **size**. Size is the one thing
about a release that two honest readers score differently. Is a folder tree
drawn from a column `tracks` has always had a new feature, or a second view of
one that was already on screen? Is a window zoom a feature, or the fix that a
laptop running at 1.75 had been owed since the first frame? Every release
under that rule spent a paragraph arguing which half it was in — the 2.3.0
entry below does it twice, in its first two sentences — and a rule that has to
be re-argued every time is not doing the work of a rule.

So the announcement moves out of the number and into the text, exactly as the
warning about migrations did one paragraph ago. **What a release adds is what
its entry says it adds**, and an entry is not shorter for the number in front
of it being smaller — the 2.3.1 entry below is the proof. The number goes back
to the single question it can answer the same way twice — whether a copy that
updated and a copy that did not still understand each other — and that
question is what `major` and `minor` were already about. `patch` gets
everything else, which is most of what happens here.

Entries older than 2.3.1 argue their number under the rule that was in force
when they were written. They are a record of what shipped and are left as they
were, not restated.

## [2.4.0] — 2026-09-21

**The release that finishes the other half.** Issue #1 asked for two things.
The first — issue templates in English — was done the day after it was opened.
The second was one sentence: «your exe doesn't do live searches and streaming».

That one was half true in a way that was hard to see from outside. Since 2.2.0
the player has known how to play bytes arriving from the network without
writing them anywhere: `aether_play::Sorgente` takes a stream, not a path, and
`FlussoHttp` reads a remote file through a sliding window using HTTP `Range`.
Gapless, crossfade, the equalizer and the spectrum all worked on it. What was
missing was not an engine. It was **a place in the library for a track that is
not a file**, and **a screen to search from** — and without those two, the
streaming branch was reachable in the code and unreachable from the interface.
No code path ever wrote a catalog address into `tracks.path`. Both are here.

**Explore.** A seventh entry in the sidebar: type a phrase, press Enter, and the
Internet Archive and Audius answer. Every result carries its license and a link
to its public page — not out of politeness, but because for some Creative
Commons licenses and for Audius's terms a visible credit is a condition of use.
«Listen» puts the track in the library and plays it; «Add» just keeps it. What
can be kept says so; what can only be streamed says that; what no free catalog
delivers still goes to the shopping list.

The search runs on Enter and not on every keystroke, and that is a decision, not
an omission: the library's search box queries an index that lives on your disk
and costs microseconds, while this one costs between one and five requests to
the Internet Archive and one to Audius. On the other side are public archives
that host us for free.

**Streaming tracks in the library.** Migration `026_brani_di_catalogo` makes
`tracks.path` nullable and puts four columns beside it — `source_service`,
`fonte_url`, `fonte_pagina`, `licenza`, `disponibilita` — with the same names
and the same values `desiderati` has had since migration 10. A track is now
**either** a file **or** a reference to a catalog, never neither and never both,
and that is a `CHECK` constraint rather than a convention written in a comment.
In the library such a track looks like any other — it is searchable, it appears
under its album and its artist, it can be queued, rated, favourited, and its
listening history syncs like everything else — with a «Streaming» pill beside
its title and, where a gesture needs a file, that gesture absent.

As for every migration, going back to 2.3.2 costs a copy of the database taken
before the update. The section below describes what it does and why it is the
only migration so far that rebuilds a table.

**Seeking got faster, and honest.** Measured against archive.org: a range
request costs about two seconds before the first byte arrives, of which nine
hundred milliseconds are the redirect to the node that actually holds the file.
That redirect is now followed once per track instead of once per request, and
the reading window grows from 256 KB up to 2 MB while the decoder reads
forward — which is what it does when it scans an MP3 that has no seek table.
What no amount of engineering removes is the download of the skipped bytes, so
a long seek inside a big MP3 is still a wait; the difference is that the cursor
now says «Finding the spot…» instead of claiming to be playing.

**The Live Music Archive was not there.** The search asked archive.org for
`mediatype:audio`, which is the obvious way to say «the sound files» and at the
Internet Archive means something narrower: concerts recorded with the
performers' permission are not `audio`, they are `etree`, their own mediatype,
for an archive that predates the generic one. Measured against the API,
`mediatype:audio AND collection:etree` returns **four** items in the whole
archive; `collection:etree` alone holds more than two hundred thousand. The
source this project names first was not sparse or badly ordered — it was
absent, and it did not look absent, because cover bands filed under
`audio_music` mention the artist in their description and answered in its
place. Searching «Grateful Dead» found four evenings by groups who play the
Grateful Dead and none by the Grateful Dead.

The query now names both mediatypes, and it sorts by downloads. The sort is not
cosmetic: of the items found only the first few are opened — each one is a
request to a free public service — so **the order is the filter**, and what
does not make the first four does not exist. Downloads are the closest thing to
a judgement the Archive publishes: not which track is the right one, but which
recording people have actually listened to, which on an archive where the same
concert exists on eight different tapes is exactly the question being asked.
The same search now opens with Barton Hall 1977.

**Explore keeps what it found.** Changing screen and coming back used to leave
an empty box and a blank page: the results and the typed phrase lived inside
the component, and the component is unmounted on every navigation. Coming back
meant running the search again — one to five requests to the Internet Archive
and one to Audius — which is the same cost the «search on Enter» rule exists to
respect, paid by the most ordinary gesture there is. The core had never thrown
the candidates away; the window forgot on its own.

**Two gestures that were missing from a result.** «Listen» replaces the queue,
which is right for «play this now» and wrong for «add it to what I am
listening to» — pressing it threw away a queue built by hand, without asking
and without saying. There is now a second button that appends. And the «Can be
kept» pill, which stated a permission that no button exercised, has one: «Keep
a copy» puts the track in the download queue with the exact address that was on
screen, so `procura` skips the search and fetches that file rather than a
similar one chosen by an algorithm. It does not start the pass — that pass
takes every waiting row, and starting three hundred downloads left over from an
import a month ago is not what one button should do.

**A result's title no longer collapses to nothing.** In the results list every
column except the title was non-shrinkable, so at the default window size with
the third column open the fixed columns took 604 pixels out of 639 and the
title — the only flexible element among six rigid ones — was squeezed to
**zero**. Of a result one could read the name of the concert and not the name
of the track. Below 900 pixels of content the row now splits in two: the title
and its buttons on the first line, everything else on the second. No column is
hidden, because here the album is the concert's title and it is the only thing
that tells ten files of the same show apart.

**Bitrate on M4A files was wrong by a factor of thirty.** `lofty` reads it from
the `esds` descriptor, where taggers write whatever they like: in a real
library all forty-four M4A files declared between 0 and 15 kbit/s where the sum
of bytes over seconds gives a hundred and thirty, and the technical line under
the controls said «M4A · 44.1 kHz · Stereo · 4 kbps» to somebody listening to a
perfectly ordinary file. A figure below thirty-two kbit/s is not a quiet file,
it is a wrong field, and the file's overall bitrate is used instead — which on
a file that really is that lean differs by a few percent, so the threshold can
afford to be generous. Migration
`027_bitrate_da_rileggere` asks the scan to read those files again, because
nothing about them changed and without it the wrong number would have stayed
for good.

**The two times over the spectrum.** In the full Now Playing view, with the
scene on, position and duration were dark text with a dark shadow over the dark
band of the scene: the shadow was written for light text on a dark background,
which is the dark theme, and in the light theme the two numbers that say where
one is disappeared. They now carry a small pill of their own — two rounded
rectangles, not the panel that used to box the scene in.

**The Skin Studio spoke half Italian.** Twenty-one group names crossed the IPC
as fixed Italian strings and were printed as they arrived: the tree said
CORNICE, LETTORE, SOVRAPPOSIZIONI above cards explaining the same parts in
English. The core now sends a stable key and the window picks the word, which
is what `Fonte::nome` and `Fonte::etichetta` have done everywhere else from the
start. The built-in skin's own description goes through the catalogue too; an
installed skin's does not, because that text belongs to whoever wrote it.

**And a skin can no longer promise a light theme it cannot keep.** The contrast
table measures tokens, and `themes.light` flips tokens. A part does not flip: a
surface that writes `#030305` into its own gradient paints that black in both
themes, because the format has no `themes.light` for parts. A skin written in
this program's own Studio declared `capabilities.light`, painted `section-card`
almost black, and with the light theme made **the whole Settings page black on
black** — while validation reported «0 errors, 0 warnings». Validation now
measures every surface a skin repaints against the text that goes on it, in
each theme the skin declares, and says which theme fails. It is a warning and
not an error: a dark surface with its own declared text colour is legitimate,
and that case is checked first.

### The number

This is the first minor since 2.3.0, and under the rule as amended in 2.3.1 a
minor has to be earned: something outside Aether has to keep up. Something
does. The contrast table now measures every surface a skin repaints, in each
theme the skin declares, and a skin that declared `capabilities.light` and
painted a surface dark passed validation yesterday with «0 errors, 0 warnings»
and does not today. Nothing about it stops working — the Studio still opens it,
the program still compiles it, the skin is still valid — but to get a clean
report back it has to declare the text colour that goes on that surface. That
is the shape the clause describes: a skin that must say something it did not
have to say before, with nothing breaking in the meantime.

`SKIN_FORMAT_VERSION` stays at 1 and the transport protocol is untouched, which
is what keeps this a minor and not a major: no document written for the old
rules is refused, and no device stops understanding another.

Everything else here — Explore, the streaming tracks, the two migrations — is
the application doing things to itself, and under the same amendments would
have been a patch on its own.

### Database

`026_brani_di_catalogo.sql` is the first migration in this tree that rebuilds a
table rather than adding to one, and it is worth saying why, because the
dangerous part is invisible.

SQLite cannot drop a `NOT NULL`: `ALTER TABLE` adds columns and nothing else.
The documented way round it is to build the new table, copy, drop the old one
and rename — and that last-but-one step is the trap. Ten tables reference
`tracks(id)` with `ON DELETE CASCADE`, and with foreign keys enabled `DROP
TABLE` performs an implicit delete that fires every one of those cascades.
Listening history, playlists, ratings and hand-made corrections would have
disappeared in silence, inside a migration that reported success. This was
reproduced before the migration was written, and with two different techniques.

So `Migration` gained a field. An entry can declare `ricostruisce: true`, and
for that entry — and only that entry — the migration runner turns `foreign_keys`
off outside the transaction and, before committing, runs `PRAGMA
foreign_key_check` and fails if anything is left pointing at nothing. Every
other migration behaves exactly as it did.

The migration also adds `idx_tracks_riferimento`, a unique index on
`(source_service, fonte_url)`: what `UNIQUE` on `path` does for files, it does
for references, so the same stream cannot be added twice.

`027_bitrate_da_rileggere.sql` is one statement: it sets `date_modified` to
zero on the rows whose stored bitrate is below thirty-two kbit/s and that have
a file underneath. That is this tree's existing way of saying «this row is to be
read again» — the three metadata corrections in `library.rs` use it — and the
next scan reopens those files and writes what it finds. The right number lives
in the file, and computing it here from `file_size` and `duration_ms` would
have been a measurement invented in the database that tomorrow disagrees with
the one the scan writes. It costs a tag read for the affected files only:
forty-four in a library of fourteen hundred. Going back to 2.3.2 costs, as
always, a copy of the database taken before the update; what this statement
changes is a modification date, the one column the scan rewrites by itself on
the first pass.

`file_size` and `date_modified` become nullable, for the reason a zero would
have been worse: a streamed track occupies nothing and has no modification date,
and a zero meaning «unknown» is the kind of value somebody sums a year later.
Every query that assumes a file now says `path IS NOT NULL` — the scan, the
Folders tree, enrichment, the sound fingerprint, playlist export, the profile's
roots — and in Rust `SchedaSorgente` carries a `Collocazione`, which is an enum
with two cases, so the compiler names every place that has to decide.

## [2.3.2] — 2026-09-17

**The release that finishes the track.** Three faults lived in the last three
seconds of every song — a scrubber that landed on the next track, a short one
that stopped the music for good, a queue that died a breath from its end — and
all three had the same cause, which nothing in the engine had ever written
down: the ring buffer between the decoder and the sound card is sized for
192 kHz across eight channels, so on an ordinary output it holds over three
seconds of music. For the last three seconds of every track, **the track you
hear and the track Aether is decoding are not the same track.** Around them,
the queue was fixed to play what it says it plays: «repeat one» repeats,
dragging to reorder works again, and a next track you removed no longer plays
anyway.

Two things that were missing rather than broken arrive with them. A track can
leave the library from its own context menu — «Remove from library», which
keeps the file, and «Delete from disk», which sends it to the Recycle Bin;
until now the only way out was to close Aether and delete the file in Explorer.
And lyrics can be chosen by hand from everything the catalogue has for a track,
with what you pick staying picked and what you drop staying dropped.

The rest of the entry is mostly one subject: **work you had already done, kept.**
A correction made by hand survives a watched folder disappearing and coming
back; lyrics follow a track whose name was corrected instead of being orphaned
under the old one; a hand-synced `.lrc` is written to the library first, so a
read-only folder costs you a warning rather than half an hour of tapping; and
an existing sidecar is never overwritten without a `.bak` beside it.

Four migrations — `022_testi_seguono_il_brano`, `023_correzioni_orfane`,
`024_indici_degli_ordinamenti` and `025_testi_scelti_e_scartati` — and, as for
every migration, going back to 2.3.1 costs a copy of the database taken before
the update. Each is described in the section below.

### Database

`022_testi_seguono_il_brano.sql` adds one trigger, `lyrics_segue_track_key`,
and changes no data. `lyrics` is keyed on `track_key`, and `track_key` is
recomputed whenever artist, title or album change — by enrichment, by
confirming an uncertain match, by a re-scan of retagged files. None of those
writers touched `lyrics`, so every correction orphaned the lyrics under the old
key: hand-synced timings, the offset nudged by ear, the catalogue's answer. The
trigger moves the row when no other file still carries the old key, only if
the row holds something (an empty «the catalogue didn't know that name» row
stays behind, because the name was wrong), and never over a row that already
has lyrics.

For the trigger to be right, a key must change once. A re-scan, an enrichment
pass and undoing enrichment used to rewrite a corrected track with its tags'
key and put the corrected one back a moment later, in the same transaction; the
lyrics followed the first move and, when another copy of the track (the
uncorrected MP3 next to the corrected FLAC) still carried the tags' key, stayed
on that copy. Those writers now leave a corrected track's key alone.

`023_correzioni_orfane.sql` adds a `correzioni_orfane` table and two triggers,
and changes no existing row. A track leaves the library when its file does —
and also when the file hasn't gone anywhere: a watched folder removed and added
back, an external disk unplugged during a scan, a share that didn't answer.
`track_overrides` cascades with the row, so the file came back under a new id
and every hand correction on it was gone. Now a correction is set aside under
the track's `content_key` (computed from the raw tags, so the same file brings
the same key back) before its row is deleted, and returns when a row with that
key is inserted; the scan re-applies it. Set-aside corrections expire after 90
days. Enrichment annotations are not kept this way: the enrichment pass
re-derives them, and restoring them without their fields would misdescribe the
row. Tracks scanned before migration 019 had no `content_key` until their file
changed on disk, so their corrections were still deleted with the row; the next
scan re-reads those files once and fills the key in.

`024_indici_degli_ordinamenti.sql` adds four indexes and changes no data. The
Tracks list sorted by shelf (`artist COLLATE NOCASE, album COLLATE NOCASE, …`),
by title, by most played, and the Albums list, fetch 200 rows at a time with
`LIMIT … OFFSET …`; no index had those shapes — `idx_tracks_artist` is not
`NOCASE`, and SQLite won't use it for a case-folded sort — so every page sorted
the whole table in a temporary B-tree. A test now reads SQLite's query plan and
fails if that comes back. The cost is a few hundred kilobytes on disk and a
little more work per write to `tracks`.

`025_testi_scelti_e_scartati.sql` adds two columns to `lyrics`, `scelta` and
`scartati`, and changes no data. Lyrics chosen by hand from «Other lyrics» were
stored like any catalogue answer, so the background pass could replace them
with the entry you had just passed over; lyrics dropped with «None of these»
came back with the next «Search again», or by themselves two weeks later.
`scelta` marks a row you chose, which catalogue answers no longer overwrite;
`scartati` lists the catalogue entries you dropped for that track, which count
as «not found» from then on. Choosing one from the list again takes it off.
Being on the lyrics row, both follow a corrected track through the 022
trigger.

**Going back to 2.3.1 needs a backup**, as for every migration: 2.3.1 refuses a
database at version 22 to 25 rather than guessing at it.

### Added — taking a track out of the library

A track's context menu ends with two new entries: **«Remove from library…»** and
**«Delete from disk…»**. Both work on the whole selection, both ask first.

Until now a track could only leave the library by its file leaving the disk: to
get rid of one you closed Aether, deleted the file in Explorer, and came back to
press «Scan». There was no entry for it anywhere, and the only code that removed
a row lived inside the scan, where it means «this file is not on this disk» —
which is a different statement.

Two entries and not one, because they answer two different questions.

«Remove from library» says «I don't want this in the list»: the row goes, the
file stays. It is the right answer for what got in by mistake — one watched
folder too many, an external disk scanned once — and it has a limit that is
stated instead of hidden. The library is an index of the disk, not the disk, and
no index can tell a file not to exist: a file still under a watched folder comes
back at the next scan. So the notice counts them and says so, and the
confirmation points at the other entry for those who want it gone.

«Delete from disk» says «I don't want this any more»: the file goes to the
Recycle Bin, and the row with it. To the Recycle Bin and not deleted, because a
menu entry that destroys without a way back is the wrong menu entry; and for the
same reason the row is removed **afterwards**, and only for the files that
actually got there. A library that claims a file is gone while it is still on
disk is worse than one that claims too much.

Both are confirmed, and the primary button in that window is «Cancel». What goes
away with the row is not just the row: the rating, the like, the play counts and
the place in playlists were built by listening, over years, and there is no undo
to offer for them. Where an undo does exist — the replaced queue — Aether offers
the undo and asks nothing.

Three things happen around the row, and none of them is visible:

- **The queue is swept.** It is a JSON list of identifiers in `settings`, not a
  table: no foreign key cleans it, and a stale identifier is a number that opens
  nothing — the engine finds it at the end of the current track, and answers
  with «source unavailable» instead of the next song.
- **The engine lets go of the file first**, when deleting from disk. On Windows
  an open file cannot go to the Recycle Bin, and the audio engine holds its file
  open for the whole track — including the next one, which gapless opens ahead
  of time. Deleting the track you are listening to now stops playback and works.
- **Albums and artists are rebuilt** in the same transaction, so a disc with no
  tracks left and an artist with no discs left go away with them, and an album
  emptied from its own page closes and returns to the grid. The grid, the
  artists, the Home shelves and the number beside each playlist are re-read
  afterwards: the track list takes the rows out by itself, to keep the place of
  whoever had scrolled, but how many tracks an album really has is not something
  a window that holds one page of it can work out.

When a file will not go, the row stays. Deleting stops at the first file that
refuses, takes out the rows of the ones already in the Recycle Bin, and says
why — as far as Windows will say. A file held open by another program makes the
shell give up without giving a reason, so the message names both causes it could
be, the other program or a drive that has no Recycle Bin, instead of picking one
and sending half the people to close a program that has nothing to do with it.

No sync tombstone is written, and that is not an omission: `tombstones.tracks`
travels and merges, but nothing consults it to keep a track from coming back —
both `sincronia` and `backup` say so in as many words. A track row is never
created by a sync; it is created by a scan reading a file that is there. If the
other device has that file, it should keep it.

New dependency: `trash 5`, for the Recycle Bin. A dependency and not four calls
of our own, for the reason already written next to `souvlaki`: talking COM to the
operating system is `unsafe` by definition, and the workspace forbids it. In
`Cargo.lock` it is seven packages (675 → 682) — `trash`, `urlencoding`, and a
fourth `windows` with four satellites.

### Changed — smaller things you would have noticed

- **A track's context menu gains «Go to album», «Go to artist» and «Show in
  folder».** It only had what you do *with* a track, and none of the ways to
  get somewhere *from* it. From search results and playlists they close the
  search or the playlist on the way, and a track with no album says so.
- **Clicking a folder's name opens it**, not only its 14-pixel triangle.
- **Full screen says «Now playing»** instead of «Now playing from the album»,
  which it said whether the queue came from an album, a playlist, a search or
  the Monday mixes. Nothing records where a queue was filled from — and after a
  restart nothing could — so the line said «album» and was wrong more often
  than not. The track's album is right below it, where it always was.
- **The Folders panel updates after a scan** — including the one that runs by
  itself when downloads finish, and after importing a profile. It showed the
  counts and folders from before until the watched folders changed or Aether
  restarted. Open folders and the selection stay as they are.
- **«Resume» on Home resumes that track, from where it was.** The position it
  offered belonged to whatever played last — «Resume at 0:29» on a 22-second
  track — and pressing it on a track already started sent it back there.
  Play in the bar after a restart also resumes from the saved position instead
  of 0:00, and a track that played to its end starts over.
- **The right column comes back when the window widens again**, if it was the
  window that closed it; a column closed by hand stays closed.
- **The floating player shows the file's format line** under the scrubber, as
  the column and full screen already did — below 1100 px it was nowhere.
- **Playlists in the collapsed sidebar show their initial** instead of seven
  identical icons.
- **An empty playlist says so**, and how to fill it, instead of showing an
  empty table.
- **Success notices dismiss themselves** after eight seconds; errors still
  stay.
- **Weekly mixes without a majority genre or artist are named after their
  first two artists** instead of «More like this, 2». Tracks with no artist tag
  are left out of that name: one such mix was called «Unknown artist, Led
  Zeppelin».
- Home no longer shows a «–» where a count would be; the Artists subtitle and
  the database line in Settings no longer speak in jargon; two messages that
  were hard-coded in Italian are translated.

### Changed — lists and sliders

- **Dragging the volume no longer re-renders the whole window at every
  pixel.** The slider follows the finger locally and talks to the engine at
  most 20 times a second; the engine answers with a two-field event instead of
  the full player state, which meant a database read per step. A volume
  changed by keyboard or from the tray icon after a drag now moves the slider;
  it stayed where the drag had left it.
- **The queue is virtualised**, like the track list: starting «All tracks» put
  eighteen thousand rows in the always-open right column. It also stopped
  re-fetching every row whenever one track was added, removed or moved; only
  the rows it does not have yet are requested.
- **Scan progress redraws the window at most four times a second** instead of
  every 25 files.
- **Opening an album or playlist no longer shows the previous one's tracks**
  for a moment, and a slow answer for the one you left no longer lands under
  the one you opened.

### Fixed — your own work, kept

- **Saving hand-synced lyrics no longer depends on being able to write next to
  the track.** The row in the library is written first; the `.lrc` beside the
  track follows, and if it can't be written — a read-only folder, a share —
  the editor says «saved in Aether, but not next to the track» instead of
  losing the half hour of tapping. A stale `.lrc` older than your sync no
  longer overrides it.
- **An existing `.lrc` or `.a2.lrc` is never overwritten or deleted without a
  copy.** The first time Aether replaces or removes one with different content,
  it is moved to `<name>.lrc.bak` (or `.a2.lrc.bak`).
- **The profile carries three more choices**: whether lyrics may be fetched
  online, whether to check for updates, and whether the guided tour was already
  seen. The first two are privacy choices, and losing them on a new computer
  meant requests going out before anyone noticed.
- **A profile brings corrected tracks back whole.** Corrections were looked up
  by the corrected key, which a freshly scanned file on another computer does
  not have yet; and everything filed under that key — favourites, ratings,
  playlists, history, lyrics, covers — was applied before them, so it found no
  track either. A correction now also carries the raw-tag key of its files
  (only when that key names one track on the exporting computer), and
  corrections are applied first. Older profiles still import as before.
- **The lyrics sync editor keeps your taps when the track ends.** It lived on
  «the track now playing»: when that changed, the editor closed with every tap
  in it and reopened, empty, on the next track. It now stays on the track it
  was opened for; while another one plays, taps are held and a notice offers
  to go back — to the same queue position, from just before the last tap. A
  slow save no longer puts one track's lyrics on another's panel.
- **«It's my library: merge anyway».** A profile whose library identity differs
  from this one's brought only settings, with no way round it — which also hit
  someone who exported a profile on the new computer before importing the old
  one. The plan still says the identities differ; the box lets you proceed.
- **Re-syncing lyrics keeps their translation lines.** Saving from the sync
  editor rebuilt every line without the one shown below it.
- **A word-by-word sync survives an `.lrc` that is merely touched.** A sidecar
  copied or moved after the sync was newer on disk, and took over with its
  line-level timings; it now does only when its lines differ.

### Fixed — lyrics

- **The library no longer freezes while a lyrics panel reads a network share.**
  The `.lrc` beside the track and the synced tag inside it were read with the
  library lock held; on a slow share every other command waited. The disk is
  now read between two short lock windows.
- **«Search again» no longer deletes lyrics you had.** A «not found» used to
  write `NULL` over the existing row; now it only records that it asked.
- **Choose the lyrics yourself.** «Other lyrics» lists every entry the
  catalogue has for the track, timed ones first, with the first two lines of
  each; «None of these» drops lyrics that came from the catalogue, and that
  entry does not come back — not with «Search again», not with the background
  pass. What you pick stays picked (see migration 025).
- **Untagged files are searched properly.** «Unknown artist» and «Unknown
  album» are no longer sent to the catalogue: the artist comes from the file
  name when it carries one, otherwise the catalogue is searched by title. A
  third round tries the main artist alone when the tags list several.
- **`.lrc` files in other encodings.** Sidecars in windows-1252, windows-1251,
  Shift-JIS, GBK or UTF-16 used to arrive with a replacement character in
  place of every non-ASCII letter; the encoding is now detected. A file that is
  UTF-8 except for a byte or two — written in UTF-8, then touched up by an
  editor that saves in the Windows codepage — no longer has all its accents
  ruined: the whole file was re-read in that codepage, turning every correct
  «è» into «Ã¨». The good text is kept and the stray bytes are read as
  windows-1252.
- **The timing nudge shows the sign of its buttons.** Two clicks on «−100»
  read «200 ms».
- **With «Ask the catalogue for lyrics» off, «Other lyrics» says so.** It said
  «The catalogue has no entries for this track» — a reply nobody had given, and
  one that sent you looking for the fault in the wrong place.
- **«Fill the library» is enabled by what a pass would actually ask for.** It
  went by the tracks with no lyrics at all, which is a different question: it
  was off on a library full of plain lyrics waiting for timings, and on when
  everything missing had already been asked for in the last fortnight.
- **`[mm:ss:cc]` timestamps** were read as hours, so the lyrics never lit up.
  Bare CR line endings are split. Two lines at the same timestamp — a
  translation under the original — now light up together, with the second
  shown below, instead of the original never lighting at all.
- **The preload of the next track's lyrics only runs while a lyrics panel is
  open**, as `PRIVACY.md` already promised.
- **The background pass no longer stalls on the same tracks.** A track that
  made the catalogue fail was retried at the head of every batch, and five of
  them in a row stopped the pass for good; they are now skipped for the rest of
  the pass and, when the failure is not transient, retried a day later.
- **With `AETHER_DATI` set**, the lyrics cache was written to the default data
  folder instead of the one in use.

### Changed — the guided tour and the update notice

- **The tour opens with a welcome card** saying what Aether does, before
  pointing at where things are.
- **The player steps are no longer skipped on a first run.** Four of the steps
  point at things that only exist while a track is loaded, and on a fresh
  library nothing was, so they were silently skipped. When the tour opens with
  an empty queue, the queue is filled with recent tracks — paused, and only if
  it was empty. That queue no longer counts as yours: the first real «Play»
  replaces it without offering to undo it.
- **«Not now» means not now.** The close button on the update notice used to
  skip that version for good; it now hides the notice for this session — also
  across a visit to the theme Studio — and «Skip this version» is a separate
  button.
- **«Check now» works with automatic checks off**, as the switch's hint says.
- **The notice and Settings agree.** Turning checks off or skipping the version
  in Settings now hides the notice, where «Update» would have failed.
- **«Update» pauses the music when the installer starts**, not before a
  download that can take a minute.
- **The update notice survives a restart.** The version found lived only in
  memory, and the startup check is skipped within 30 minutes of the last one,
  so reopening Aether hid it for half an hour. A found version is now
  remembered and forces the startup check.
- **«Check now» says «You have the latest version»** when it finds nothing,
  instead of just re-enabling the button.

### Fixed — network shares and a busy database

- **«The database refused a request» no longer stands in for a network
  fault.** `SQLITE_CANTOPEN`, `SQLITE_PROTOCOL` and `SQLITE_IOERR_LOCK` fell
  into the catch-all, whose message says the fault is almost always Aether's;
  they now say network share, disk, or busy. The favourite and rating commands,
  which built that error by hand, go through the same classifier, and a
  correction on a track that no longer exists says so instead of blaming the
  database.
- **More ways a share goes away are recognised**: a mapped drive that is not
  reconnected yet (2250), no network at all (1222), and a share that rejects
  the credentials (1219, 1244, 1326).
- **The window no longer freezes behind a stuck lock.** Synchronous commands
  run on the window's thread; when whoever holds the library is waiting on a
  dead disk they now give up after four seconds with «the database is busy»
  instead of waiting for ever. The player bar no longer disappears when that
  happens — and full screen no longer closes — because a track could not be
  re-read: the last known one is shown.
- **Writing old tags back into files releases the library while it writes**,
  instead of holding it for the whole run over thousands of files.
- **The same folder is watched once**, however it was spelled: `D:\Musica`,
  `d:\musica` and `D:\Musica\` used to be three watched folders.
- The network-share message pointed to a setting that does not exist; it now
  says what is going on and where the path can be read.

### Fixed — the renderer no longer burns a core while you look at a list

Albums and Tracks kept the renderer at 15–20% of a core with nothing moving on
screen. The cause was the «More on the way» sentinel at the bottom of every
paged list: its shimmer animates `background-position`, which the stylesheet
claimed the compositor handles alone. It does not — it repaints every frame,
off screen included. The shimmer now pauses until the sentinel is within
reach of the viewport; measured after the change, the renderer idles near
zero.

### Fixed — keys go to the control that has focus

The arrows on the volume slider moved the track by five seconds instead of
changing the volume, Space on a checkbox or a button played or paused, every
tap in the lyrics sync editor also paused the music, and Escape in a menu
closed the menu *and* what was under it. Global shortcuts now step aside for
sliders, radios, checkboxes, switches, focused buttons and anything inside a
modal, and the components that consume a key stop it where they take it.

### Fixed — replacing the queue can be undone

Enter on a folder replaced a 200-track queue with one track, with no way back.
Replacing a long queue, a queue you built by hand, or pressing «Clear» now
shows a notice with **Undo**, which brings back the tracks, the order, the
position in the track and whether it was playing. Replacing one album with
another stays silent: that is almost always what you meant.

The notice also shows over full screen. It used to be drawn in the page
underneath, which full screen covers and makes `inert`, so the Undo could be
neither seen nor pressed — nor could an error from a track started there.

### Fixed — the queue plays what it says

- **«Repeat one» repeats.** It played the track twice and stopped. The engine
  announced a track only when its id changed, and that announcement is what
  prepares the next one; the same track starting again went unannounced. A
  queue with the same track twice in a row (`A, A, B`) had the same fault from
  the other side: B never played. The engine now announces every *listen*, and a
  seek within one is still not a new one.
- **Dragging to reorder works again**, in the queue, in hand-made playlists
  and in the theme Studio. On Windows the drop never arrived: the window
  accepts files from the system, and with that on, WebView2 keeps HTML5 drops
  for itself. The rows now follow the pointer instead; dropping files on the
  window is unchanged. In the Studio that covers both gestures — reordering the
  layers of a surface, and carrying a widget or a zone from the palette into a
  slot of the shell — and the slot under the pointer is the one that lights up,
  even while the list scrolls itself near the edge.
- **A removed or reordered next track no longer plays.** The engine kept the
  track it had been given as next until a new one was opened, and if that one
  failed to open it kept it for good: remove the next track, and it played
  anyway while the bar showed another. The stale one is now dropped at once,
  and when the new next cannot be opened, the queue stops on it — shown, with
  the error — instead of replaying the track that just ended on «Play».
- **A track that won't open no longer splits the player in three.** Playing a
  missing file moved the queue but kept the old track in the speakers and the
  old queue on screen. Now the queue is the one you asked for, the bar shows
  the track that failed, stopped, and the error says why.
- **Removing the paused track from the queue no longer starts the music.** The
  next one is loaded at its start, still paused.
- **A seek near the end with crossfade on no longer runs the clock past the
  track.** The incoming track was announced halfway through the full
  crossfade even when the outgoing one had less left than that: with the
  12-second crossfade, a seek to the last second played the next track for
  five seconds while the bar showed the previous one at 0:27 of 0:22, and the
  queue moved late. It is now announced halfway through the overlap that
  actually happens.
- Time spent paused after «Undo» on a paused queue no longer counts as
  listening.
- **→ no longer sends a track back to its start** when the database has no
  duration for it. A duration of zero means «not known», and it was taken for
  «ends now», so that track could not be crossed at all.

### Fixed — the last seconds of a track

Three faults, all of them living in the same handful of seconds, and all of
them for the same reason: **the track you hear and the track Aether is decoding
are not the same track.** The ring buffer between the decoder and the sound card
is sized for the worst case a device could ask for — 192 kHz across eight
channels — so on an ordinary 48 kHz stereo output the same samples are over
three seconds of music. For the last three seconds of every track, the decoder
has already finished it and moved on to the next.

- **Dragging the scrubber in the last seconds of a track no longer breaks
  playback.** The seek was applied to whatever the decoder had open, which by
  then was the *next* track: you heard that one, from the point you had asked
  for, while the window still showed the title of the one before and the cursor
  ran off the end of the bar — 3.9 seconds into a track that lasts 2. Then that
  track ended with nothing left to play, and nothing happened: no new track, no
  stop, the queue frozen and the button still saying «playing». Only pressing
  something by hand got the music back. The seek now lands on the track you can
  hear — Aether keeps its decoder for as long as one of its samples is still
  waiting to be heard — and the one that was queued up goes back to being the
  next one, from its beginning. Where it cannot (two tracks shorter than the
  buffer, one after the other), the seek is dropped rather than misapplied.
- **A short track no longer stops the music.** A track shorter than those three
  seconds is decoded in full before the next one is ready, so the engine reached
  the end of it with nothing to attach, and what arrived a moment later arrived
  into a state nothing could leave. An album with a short intro or an interlude
  died on that track. The track that arrives late is now attached at once —
  which costs no gap, because the buffer is still full of the track that just
  finished.
- **Every track after the first no longer plays twice.** Same cause from the
  other end: the pre-loader is nudged twice at each start, and the second nudge
  handed over a track the engine had already taken, which then sat in the chamber
  behind itself. It now checks what is already loaded before loading it again.
- **The queue ends after a seek.** Every seek left a debt of a few thousand
  frames between what had been pushed towards the speakers and what was counted
  as played: the callback lowered the «buffer emptied» flag before emptying it,
  so the decoding thread started again while the flush was still running and had
  its first samples eaten uncounted. Tracks in the middle of a queue never
  noticed — moving on is the decoder finishing, not a count — but the *end* of a
  queue is that count, so the last track stopped a breath from the end and
  stayed there, playing nothing, for good.

### Fixed — dropping files on the window

Everything that was not a skin was added as a watched folder — including a
`.m3u8` and every MP3. Folders are still added; playlists open the import,
tracks that are already in the library go to the queue, and files the library
does not know get a sentence saying what to do instead of silence.

### Fixed — full screen at the default window size

With lyrics and queue both open, the centre column dropped under 300 px: title
cut mid-word, one word of artist per line, transport under the lyrics panel.
Below a width set in the stylesheet the two side panels now take turns. With
the spectrum on, the controls keep no background of their own — the scene runs
behind them, edge to edge — and what that would have cost in legibility, the
two times beside the scrubber and the unlit stars over bright bars, is paid by
a shadow on the text and the marks instead of by a panel over the scene. Focus
moves into the screen when it opens and back when it closes,
and the page underneath is `inert` so Tab no longer walks an invisible list.

### Fixed — smaller things

- The window buttons kept the startup language after switching it.
- The profile note said «your choices, not your library», which 2.3.1 made
  false.
- **The first click on «Apply» after typing a folder into an imported profile
  is no longer lost.** Leaving the field redrew the plan, which disabled the
  button between the press and the release — so the click landed on a disabled
  button and nothing happened. Redrawing the plan no longer disables «Apply»:
  it only ever changed the numbers on screen, never what gets applied.
- The confirmation before playing a large folder said the queue you are
  listening to «cannot be put back the way it was». It can, with «Undo» in the
  notice, and it now says so.
- The Folders panel is a text file again: it carried a literal NUL byte, which
  made git treat it as binary and hide every change to it.
- The first-run screen closed the moment the folder was written, before the
  scan progress and the «Listen» button it exists to show.
- **The first-run screen says what happened.** It covers the window, and with
  it the notices: a scan error was drawn underneath, and a scan that found
  nothing just turned «Continue» back on. Notices now show inside it, and a
  read with no tracks says so, naming the folders that did not answer. A
  folder dropped on it joins its list, ticked — it used to be added behind it
  and dropped again by «Continue». A long path no longer pushes the list out
  of the card.

## [2.3.1] — 2026-09-10

**The release that stops writing in your files.** Up to 2.3.0 two features
changed what was on your disk: enrichment rewrote tags every half hour, and the
reorganizer moved files into `Artist/Album`. One has been rewritten so that it
touches nothing, and the other has been retired. What Aether still writes is
now short enough to list, and it is listed, in [`PRIVACY.md`](PRIVACY.md) § 8.
[`TERMS.md`](TERMS.md) § 5 promised the opposite — «some operations modify your
files» — and has been rewritten to say what is true now instead of being left
to contradict, in the same download, the document beside it.

It is also a long entry carrying a patch number. Both of the clauses that
would have argued with that — «minor for every migration» and «minor for new
features» — have been amended above, in the section where they live, rather
than being quietly ignored down here. Alongside what it stops doing, it adds a
Folders panel, a window zoom, a guided tour, word-level lyric timing,
animations in the skin format and a profile that carries the whole library;
and it fixes the two long-standing faults that were losing your data in
silence — a hand-made correction that a re-scan threw away, and a nudge saved
before the lyrics arrived that locked those lyrics for good.

### Database

They are the reason the clause about migrations moved out of the number. They
are listed below rather than counted here, because a count in a paragraph goes
stale the moment another one is added — which is exactly what happened to this
sentence.

`019_identita_e_metadati.sql` adds a `content_key` column and its index to
`tracks`, repairs the `lyrics` rows that a stray offset had locked, and creates
the `track_meta_arricchita` table. Nothing is dropped and nothing is rewritten
in place.

`020_cronologia_unica.sql` gives `play_history` the unique index on
`(track_id, played_at)` it never had, after collapsing the duplicates already
in it. It exists because from this release a profile carries the library, and
the listening history with it: importing the same archive twice — which is the
ordinary gesture of somebody who is not sure they already did — used to double
the rows, silently, because neither of the two was wrong on its own. The
counts were safe (those come from `sync_ascolti` and merge as a CRDT); the
history, the year graph and «what did I listen to that afternoon» were not.
The rule it encodes is that **two plays of the same track in the same
millisecond are the same play**, and it is written where nobody can forget it
rather than in the one importer that happened to guard against it. Which is
why the deduplication runs before the index: on a library that has already
been doubled, this migration is also the repair.

`021_impronte_illeggibili.sql` deletes the rows of `track_impronta` that hold
no vector and were filed as `illeggibile`. Until this release the engine could
not decode Opus, and the fingerprinter goes through the same decoder: every
Opus in the library had been examined, refused, and recorded as refused, which
is why the affinity and the autoplay walked past those tracks. `illeggibile` is
by construction the retryable verdict — the fingerprinter comes back to it on
its own after a month — so deleting those rows throws nothing away. It only
removes the wait. `muto` and `corto` are left alone: the first is terminal, and
the second does not depend on which codecs we can open.

The extractor version was **not** bumped, and that is the point of doing it this
way. Bumping it invalidates every fingerprint in the library, which is the right
move when the measurement changes; here the measurement did not change, only
the list of files that can be opened. A few hundred rows are re-measured instead
of several thousand.

**Going back to 2.3.0 needs a backup.** Not because of what these migrations
do, but because migrations only go forward: 2.3.0 opens a 2.3.1 database and
refuses it, by design, rather than running queries against a schema it does not
know. Restore a copy of the database from before the update and the old version
opens it unchanged.

### Added — a Folders panel, in the sidebar

The library has always known where every file is; there was no screen that let
you walk it. There is one now, next to Songs, and it is a tree: roots, folders,
the tracks in them, expanded lazily, with the whole keyboard — arrows to move
and to open, Home and End, `*` to open every sibling, type-ahead with an
800 ms memory, Enter to play. Double-click or Enter on a folder plays it in
order: by disc and track number, which is to say in album order, when the
folder *is* an album.

**It is drawn from the database, never from the disk.** That is the whole
design and it is worth one paragraph. Walking the filesystem to draw a
navigation panel means, on a network share, forty seconds of Windows timeout
per expansion, and on a share that is switched off, a panel that never opens.
The rows of `tracks` are already at home, so the tree comes out of them —
about 10-20 ms for a library of eighteen thousand tracks, and no SMB traffic at
all. The price is stated rather than hidden: a folder that exists on disk but
has no indexed track in it does not appear. In a panel whose purpose is to
play things, a node that cannot be played is a node you learn to skip.

Whether the roots are actually *reachable* is a separate question, answered by
a separate command, on the side: the roots draw immediately from the paths
already known, and a probe with its own eight-second deadline marks the dead
ones and offers «Check again». The window never waits for the network, because
nothing it draws came from there.

`ui.folders.open` and `ui.folders.selected` remember where you were. They stay
**out of the profile** on purpose: they are paths on this machine, and on
another computer they would reopen nodes that are not there.

### Added — the window zooms, `Ctrl++` `Ctrl+-` `Ctrl+0`

Seven steps between 90% and 200% — the same seven every browser offers, so
that somebody who has pressed `Ctrl++` elsewhere already knows how many presses
it takes. One step down and five up, and the asymmetry is deliberate: the
smallest text the sheet uses is 10.5 px, which at 0.9 is 9.45 and one step
further down would be 8.4 — and an interface you cannot read is one you cannot
use to fix itself. Upward the failures are visible and `Ctrl+0` undoes them in
a keystroke, so upward there is room.

It is the **WebView's** zoom and not a scale in the stylesheet, and that is not
an implementation detail: two modules in this release measure the DOM — the
virtualised list and the tour's spotlight — and a CSS `zoom` would put them
astride two coordinate systems, where a `getBoundingClientRect` carries the
factor and a `scrollTop` does not. With the WebView's zoom neither notices
anything, because there is nothing to notice: they go on working in CSS pixels,
and the CSS pixels got bigger without changing their name.

No number crosses the IPC boundary. Three gestures go out — one up, one down,
back to true — and the scale of steps lives in one place, `preferenze::SCALA_ZOOM`.
`ui.zoom` is remembered like any other preference and, like `player.output`,
**does not travel in a profile**: it is the correction left over after
Windows' own scaling on *this* monitor, and on another one it describes nothing.

### Added — a guided tour, on the real interface

Ten steps, on first run, once. It is not a carousel of screenshots: each step
finds the actual element in the document, cuts a hole in the veil over it and
puts the bubble beside it — so what you are being shown is the program, at the
size and in the skin you are running it in. A step whose anchor is not on
screen is **skipped**, and that is a designed outcome rather than a failure:
the queue may be closed, nothing may be playing, the library may be empty.

What is remembered is the version of the *script*, not the version of the
application. The difference shows up at the first patch release: with the
application's number, a 2.3.2 that fixes a typo would reopen the tour for
everybody. The question worth asking is not «has the application changed» but
«has the tour got anything new to say», and only whoever writes the tour can
answer it. *Settings → Redo the tour* is the way back, and it is explicit.

### Added — what the file actually is, under «Now playing»

«FLAC · 44.1 kHz · Stereo · 1058 kbps». The four columns have been in the
database since the first version and nothing had ever read them back.

It says the **file**, not the output. If the sound card is resampling to
48 kHz this line still says 44.1, because the question it answers is «how is
this edition made», and the real output does not answer it — that would change
when you changed headphones, while the file stays what it is. The bitrate shows
even on lossless, because it is the true average and on a FLAC it tells you how
dense the transfer is. Missing columns simply drop out: an old library shows
what it has and nothing else, never a « · · ».

`player.fileFormat.visible`, on by default — the opposite default to the
spectrum's, for the opposite reason: this costs one `SELECT` on four columns
once per track, and when it is off the query is not made at all and the field
arrives empty. There is a new skin part, `np-formato`, so a skin can quieten it.

### Added — skins can animate

The skin format has had durations, curves and page transitions since it
existed, and the Studio exposed none of them. It now has a Motion panel, and a
skin can declare **named animations** — up to eight of them, two to six frames
each, with opacity, scale, translation and rotation — and bind them to a part's
`enter`, `hover`, `active`, `focus` or `disabled`. The line that decides what
belongs here, and it is written into the module: *a skin animates states of the
DOM, not events of the application.*

Three rules that are not negotiable, and each is there because of something
that would otherwise break:

- **No `infinite`.** The four endless animations in the application are each
  stopped by hand, one rule at a time, under `prefers-reduced-motion`. A skin
  cannot write those rules, so a skin's endless animation would be the only
  thing in Aether that cannot be stopped. A pulse is `direction: alternate`
  with two iterations.
- **Every emitted duration goes through `calc(… * var(--motion-scale))`**, or
  reduced motion is simply routed around. A test asserts that no `animation:`
  or `transition:` the compiler writes contains a duration outside that call —
  which caught two `::view-transition-*` rules that were already writing a bare
  `var(--dur-2)`.
- **A cost budget**, which ships in the same release as the field and not one
  later, so that no skin can ever have been written against its absence.

`SKIN_FORMAT_VERSION` stays 1 and there is no migration: every field is
optional, and a document that has never heard of `animations` was already
valid.

### Added — the profile carries the library, and it is a `.aeprofile`

The profile used to be seventeen lines of JSON: openable in an editor,
readable, and that was a property rather than an accident. It now carries the
listening history, the lyrics, the corrections, the covers and the skin
packages — hundreds of megabytes of binary — and a `.json` that contains three
hundred megabytes of archive is lying about its own name. So: a container of
its own, recognised by its **first bytes** and not by its extension, which
anybody can rename. `PK\x03\x04` is the new one, `{` is the old one, and the
old one is still **read** and no longer written.

Two things it does that a bigger JSON could not:

- **It never holds the archive in memory.** It is written from file to file
  through a buffer and read back one entry at a time straight onto the disk.
  There is no `Vec<u8>` in that module holding more than one entry.
- **Names are a closed list.** An archive comes from outside, and the only
  defence that holds against an entry called `../../.ssh/authorized_keys` is
  not having a path to normalise. A name not on the list does not get skipped,
  it gets the whole archive refused.

Importing still shows you the plan before it does anything, and now the plan
also covers merging the library: it proposes a remapping for every root that
does not exist on this machine, and re-run with those filled in it tells you
how many paths would follow. Migration `020` above is the other half of this
feature: without it, importing the same profile twice doubled your listening
history.

### Added — lyrics can now be timed word by word

The domain has been ready for a long time — `.a2.lrc` was already first in the
list of sidecars Aether looks for, the writer already emitted `<mm:ss.xx>` and
the reader already read them back — and the words were being thrown away in
between. *Sync* gains a **fourth step, optional and skippable**: you tap once
per word, and it reuses the same tap/redo/finish that the line step uses.

It comes after the line step and not instead of it, because word times lean on
line times. Saving writes `name.a2.lrc` **and** `name.lrc` without the words,
because no other player knows what an `a2` is: the richer file wins here, the
twin stays for everybody else.

### Added — Opus plays, and `.oga` with it

Aether already knew Opus and would not play it. The scan took the files —
`opus` has been in the closed list of audio extensions since this Rust core's
first commit, and was in the TypeScript tree before it, which is why the golden
scan fixture expects one — the library held them, lofty read their tags, and the
technical line under «Now playing» even knew how to write the word «Opus». Then
you pressed play and got `playback.formatUnsupported`.

The refusal was deliberate and it was written down: a `Codec` enum in
`aether-play` whose only job was to say which recognised formats the engine
could not decode, with a comment promising that the compiler would point at
every place to touch on the day an Opus decoder turned up. This is that day, and
the promise held — the places were exactly the ones it named.

**The container was never the problem.** Symphonia demuxes Ogg Opus in full: it
recognises the `OpusHead`, reads each packet's table of contents to get its
duration, and with gapless enabled says how much to trim from the head and the
tail. What it does not have is the codec. So there is no new file reader here,
only a
`Decoder` registered in a codec registry of our own, beside the ones symphonia
ships. It also applies the three things symphonia reads and then does not use:
the output gain, which RFC 7845 obliges a player to apply and which is where
R128 normalisation lands when a file is re-gained without being re-encoded; the
channel mapping family; and the pre-skip.

**The pre-skip is the one that looked handled and was not.** Every Opus file
opens with a few milliseconds the encoder produced only to prime its own
filters, and `OpusHead` says how many. Trimming them is the demuxer's job and
symphonia does it — with gapless on it puts a `trim_start` and a `trim_end` on
every packet, and its Vorbis decoder simply obeys. On Opus the `trim_start`
always comes through as zero. Measured: a fixture of exactly three seconds
handed back 144312 frames instead of 144000, the 312 priming frames sitting at
the head of every single track. The cause is that symphonia does not trust the
pre-skip it has just read and tries to *deduce* it from the pages, comparing the
first page's granule position against the duration of the packets in it — a sum
that works out on Vorbis and cannot on Opus, where the Ogg granule position
already counts the pre-skip, so the two numbers agree and the deduction
concludes there is nothing to remove. The tail deduces correctly, which is why
`trim_end` works. So the head is counted here instead, from the `OpusHead`, and
the test asserts the frame count exactly rather than within a tolerance —
because a tolerance wide enough to be comfortable is a tolerance wide enough to
have hidden this.

`.oga` comes with it for free. It is the extension Xiph recommends for audio in
an Ogg container: same container, same demuxer, and Vorbis, FLAC and Opus inside
one all play without a line of decoding of their own. It is the first entry in
`SUPPORTED_EXTENSIONS` that the old TypeScript tree did not have, so the list no
longer says «as in the old tree», and the constant says why.

**Which decoder, and how it was chosen.** Symphonia has none, so this had to
come from outside, and the choice was between binding libopus — the reference
implementation, and cmake plus thirty megabytes of C to build, and four more
cross-builds the day this crate runs on Android — and one of the three Opus
decoders written in Rust. All three were measured against the same fifteen files
cut by libopus, tone and noise, mono and stereo, 16 to 128 kbit/s:

- `opus-decoder` **panics** on five of the fifteen, every one of them at a low
  bitrate, on a shift overflow inside CELT's vector quantisation. In a debug
  build that is a panic; in release, where overflow checks are off, it would
  quietly be the wrong number instead — which is worse. And low bitrates are not
  a laboratory case: they are speech, podcasts, and most of the Opus on the web;
- `rusty-opus` does not panic and returns rubbish: it reports six times the
  samples that exist, and what comes out has a root-mean-square of zero;
- `opus-pure` decodes all fifteen, and the level it recovers matches what
  libopus recovers from the same file.

That is the one that ships. **The price is stated rather than hidden**: it is
Rust rather than C, so there is no cmake and no system library and it
cross-compiles for Android for free, but it is not «no unsafe» — it has some
seventy blocks of it, all for AVX2 and NEON intrinsics. The workspace's
`unsafe_code = "forbid"` governs our own code, not our dependencies, and the
module says so where someone will find it. It also raises the workspace's
minimum Rust to 1.88, which is now the true floor rather than an optimistic one.

The test fixture is three seconds of a 1 kHz tone cut by libopus itself, and
both of those choices are argued in place. A constant level — the trick that
makes the FLAC fixture a hundred and fifty-four bytes — would be useless here,
because Opus carries no DC and a fixed level comes back as near-silence: the
test would be green for the wrong reason. And half a second would fit in a
single Ogg page, where symphonia cannot tell head padding from tail padding and
the gapless trim never runs, so the fixture would exercise a case no real file
hits.

### Changed — a failed query now says which failure it was

Everything the database refused used to arrive as one code, `db.queryFailed`,
whose message suggests restarting Aether. Four cases have been pulled out of
it, and none of them is fixed by restarting:

- `db.locked` — somebody else is writing right now, usually the scan.
  Retryable, and it is the only database fault that clears on its own.
- `db.readOnly` — permissions, a read-only folder, write-protected media.
  Waiting achieves nothing until somebody changes something.
- `db.networkPath` — the data folder is on a share, and WAL needs a shared
  memory file that SMB does not provide. The message says to move the data
  folder to a local disk, because that is the fix.
- `db.ioFailed` — the same `SQLITE_IOERR` on a local path, which is the disk
  and not the location. Not retried, deliberately: a disk that misreads does
  not recover by being asked again, and an automatic retry would hide the one
  moment when it is worth making a backup.

None of the 477 call sites changed. The classification happens where the
`rusqlite::Error` is turned into an `AppError`, once.

**And WAL is now probed rather than assumed.** `PRAGMA journal_mode = WAL` on
a network share frequently answers `wal` and leaves the failure for later: the
`-shm` file is needed by the first *transaction*, not by the pragma, and on SMB
that is where it does not get created. Without the probe the fallback was dead
code precisely on the machines it was written for — you would have got the
right message, half an hour later, in the middle of a scan, with nothing done
about it. The probe is an empty `BEGIN IMMEDIATE; COMMIT;`: **5.5 microseconds**
measured on a local disk, against the 22 milliseconds of a full open with
migrations. `SQLITE_BUSY` and `SQLITE_LOCKED` count as **passed**, which is the
part not to get wrong — another writer is exactly what WAL exists to allow, and
treating it as a fault would make a healthy local library fall back to
`TRUNCATE` because two instances started at the same moment. Which journal is
in force is now printed in the startup log.

### Removed — the reorganizer

`Settings → Folders` no longer offers «Tidy this folder…», and the
`piano_riordino` / `esegui_riordino` / `annulla_riordino` commands are gone
along with the screen that drove them and the two modules behind it — about
1,900 lines.

It was removed rather than kept, and the reason is that it had stopped being
worth its weight. What it produced was a tree of `Artist/Album` on disk — and
the library already *is* that tree: the Albums and Artists screens group by
exactly the same key, and from this release the Folders panel walks the real
one. Keeping the feature meant keeping a JSONL journal, an undo, a plan screen
and twenty-six translated strings so that a thousand files could be moved
around to build a view that was already on the screen. It also meant keeping
the one place in Aether that moved files across a network share **while
holding the library lock**, which is to say the last thing that could freeze
the window for a minute at a time.

**Your files stay exactly where they are.** Nothing is moved back, nothing is
renamed, nothing is deleted. A library that was tidied by an older version is
still tidy, and a scan finds every one of those files where they are now — that
is what scanning does. The `riordino-*.jsonl` journals in the application's
data folder are kept as well: they are the record of what was moved and where
from, and deleting them would be throwing away the only copy of it.

### Changed — metadata live in the library, not in your files

Enrichment no longer opens your music. What it works out from MusicBrainz goes
into the row in `tracks` and is annotated in `track_meta_arricchita`, with its
source and its confidence; the file on disk is not opened for writing and its
modification date does not move.

That last detail is the whole point, and it removes a defect rather than just a
promise. Writing a tag changed the file's modification date, which meant the
next scan saw a changed file, read it again, and took the tags in it as the
truth — so unless the new date was read back and stored in the same
transaction, every enriched file was re-read in full on every scan, for ever.
There is now nothing to keep in step: `aggiorna_brano` does **not** write
`date_modified` or `file_size`, and writing them would be the bug rather than
the fix.

Three consequences worth naming:

- **Cover art is no longer embedded** into your files. Aether stores it in its
  own cache and shows it from there. Cover art that is already inside a file is
  still read and still used — nothing stopped working.
- **Your corrections always win.** The order in which a track's fields are
  resolved is written out once, in `aether_app::incerti`: raw tags, then
  `track_meta_arricchita`, then `track_overrides`. A correction made by hand is
  the last word, and — fixed in this release — it survives a re-scan, which it
  did not before.
- **«Forget the enrichment» is now one delete and a re-read.** It empties the
  table and reads the tags back off the files, which are intact. It no longer
  has thousands of files to rewrite in order to undo itself.

**The tags older versions already wrote are treated as yours now.** They are
not reverted on your behalf: rewriting thousands of files is exactly the thing
this release stopped doing, and after months those tags may well have been
approved, synced or edited somewhere else. The way back is still there, behind
a button of its own in *Settings → Metadata* whose label says that it writes
into the files. It appears only while there is something left to undo, and on a
library that has never seen an older version it does not appear at all.

**That button is going away.** `arricchimento_riporta_nei_file` is the only
thing left in Aether that opens your music for writing, it is kept for one
release as a way out and not as a feature, and it will be **removed in a future
release** once the records it works from are gone. This line is the notice.

**One thing that will look wrong and is not.** On a library coming from 2.3.0,
«Forget the enrichment» will say **zero** until a new pass has run. Those
tracks were enriched by the old code, so what changed them is in their files
and in the old undo table, not in the new one — there is genuinely nothing for
«forget» to forget. The Settings panel now says so on the spot, next to the
count, for as long as the case exists.

### Fixed — lyrics come up 30 ms later, and your own offsets did not move

The anticipation with which a lyric line lights up went from 150 ms to 120 ms.
It is a single constant in the core now — it used to be written by hand in two
places, once in Rust and once in TypeScript, and only one of them ever got
updated — and the new figure is the old one minus the output latency that this
release started measuring and compensating for.

**The per-track offsets you set by hand are untouched.** They live in
`lyrics.offset_ms`, they are added on top of the anticipation rather than
folded into it, and nothing in this change reads or writes them. A track you
nudged by −300 ms is still nudged by −300 ms.

### Fixed — with crossfade on, the reported position was wrong all the way through

The engine marks the incoming track at the halfway point of the fade, but it
declared that mark as starting from zero — while the incoming track had
already played half a fade's worth of frames. With a six-second crossfade the
position it reported was therefore three seconds behind, and it **stayed**
three seconds behind until the track ended. That is the single largest cause
of «the lyrics are out of step», and it was also wrong for the scrubber, the
time display, the headphone keys and the Windows panel, «resume where you
were», and the taps you make in *Sync* — a position that lies makes every
hand-made `.lrc` come out crooked. The two figures now describe the same
instant, and the comment beside them says that they must.

### Fixed — a nudge saved before the lyrics arrived locked them out for ever

Setting an offset on a track whose lyrics had not been fetched yet created the
row with `source = 'mano'`. From that moment the two functions that store a
fetched text both skip the row — they are written `WHERE source <> 'mano'`, so
as not to overwrite something you typed — and no lyric ever appeared for that
track again. It writes `source = ''` now, and migration `019` above repairs
the rows that are already in this state.

### Fixed — fine-tuning is reachable again

The offset strip in the lyrics view appeared only when the fit was judged
anything but «good», and the comment explaining that said an always-visible
control would be «a control in search of a problem». It was not true, and the
untruth cost a feature. The fit measures one thing: whether the times fall
inside the length of *this* file. It says nothing about where they fall. An
`.lrc` tapped against another edition of the same length, a master with half a
second of silence at the front, an output chain that lags by more than the
engine can measure — all three give a text that is out by a constant amount
**and** a fit of «good», and all three are straightened by those two buttons,
which were unreachable in exactly the commonest case.

The strip now shows whenever there are timed lines. What still depends on the
fit is the *sentence*: saying why the strip is there is worth it when there is
something to report, and when there is not, the strip steps back into a
quieter style instead of disappearing.

### Changed — a surface that covers everything now has to say who takes the pointer

`strumenti/classi.js` already held the stylesheet to two rules it had given
itself: every `z-index` comes from the layer scale, and every duration goes
through `--motion-scale`. A third one joins them. A rule that declares
`inset: 0` on a positioned element covers its whole container, and from there
two behaviours are possible with nothing in the CSS to tell them apart — the
click goes through, or it stops. The check asks the block to say which
(`pointer-events`, any value), or the selector to be listed in
`ATTESE_PUNTATORE` with its one-line reason. There are six exceptions: two are
the screen itself, three are veils — which exist to catch the click that closes
them — and one is a rule shared by two layers that each decide in their own
block.

It is written from a real failure, caught before it shipped. The scrim over the
cover in «Now playing» carried the class `.velo`, which is what a veil is called
here; when veils were given their step in the layer scale, the scrim inherited
it, rose above the header and the body, and swallowed every click on that
screen. Nothing was logged, because nothing ran: an `aria-hidden` element with
no handlers that intercepts everything has nothing to report. The scrim is now
called `np-scrim` and nothing else, and the ambient layer behind it and the
shimmer on the scan bar say `pointer-events: none` out loud.

## [2.3.0] — 2026-09-09

**Minor and not patch, because it is a new feature — but no migration.**
`ui.closeToTray` is one more row in the `settings` table, and the profiles of
the configured models live as JSON in that same table, which is key/value:
coming back from here to 2.2.0 costs nothing, the unread keys stay where they
are. The refinement pass that ships in the same release changes neither the
appearance nor the shape of the data — by the rule above that is a patch — and
rides along with the minor.

**And skins keep loading.** The `canvas.viz.*` block holds eighteen tokens now,
fifteen of them new, and every one of them is non-required: a skin that has
never heard of them declares none, an undeclared token inherits the base value,
and `check_skin` warns only about the required ones — so a skin written for
2.2.x opens unchanged and gets the scene this version ships.
`SKIN_FORMAT_VERSION` stays 1, which by the rule above is the whole reason this
is a minor and not a major. The one thing that did go away is
the part `viz-title`, and it was **retired rather than removed**: there is a
`RITIRATE` list, and both `stile_parte` and the `part` field of a shelf node
accept a retired name and draw nothing. Deleting it outright would have been a
hard error rather than a warning, that is, a third-party skin that styles a
component the application never drew would have stopped opening — which is a
strange way to punish somebody for being thorough.

### Added — the visualiser stopped being a fixed picture

The spectrum in «Now playing» was the last surface in the application that a
skin could not touch. Three variables reached the scene — `--viz-primary`,
`--viz-secondary` and `--accent-glow` — and in a stock skin **all three were the
same colour**: the first two both compiled to `var(--accent)`, and `tinta()`
throws the alpha away, so the third came back as that same RGB. Both `mix()` in
the fragment shader were therefore no-ops, and the whole scene reduced to
«accent times light» — a picture with one colour in it, inside the part of the
program whose entire point is that you choose the colours. A fourth variable,
`--viz-glow`, was declared, compiled into every sheet, checked by a fidelity
test, and **read by nobody**.

There are eighteen `canvas.viz.*` tokens now, and they are not all colours.
Three say what the scene is painted with: `primary` at the top of a bar,
`secondary` at its base — a dark value now, so that the gradient which used to
be a no-op is a gradient — and `tip`, the lit crest, which used to borrow the
accent's glow and is now a colour of its own. Six say what the room is shaped
like: `width`, `depth`, `height`, `fill`, the `lens` in degrees and the height
of the `eye`. Nine say how it behaves: where the `haze` starts, how strong the
`reflection` is, how much `ambient` light there is, how far the camera `sway`s
and how hard the `beat` pushes it, the `attack` and `release` of a bar in
milliseconds, and the `floor` in decibels.

Every numeric one carries bounds, and that is not tidiness. The Studio's slider
falls back to `min ?? 0` and `max ?? 1`, so a `Number` with no limits gets a
0..1 control that silently truncates whatever you drag onto it — which is
exactly why `canvas.viz.glow`, whose value is 20, could not be edited at all
until today. `canvas.scrubber.glow` gains bounds for the same reason, 0..40;
the two skins in this repository hold 6 and 10, so nothing that ships from here
has to move.

**The Studio shows the scene moving while you paint it.** Until now the preview
mounted an empty `<canvas>` with a dashed outline and a comment saying the scene
could not be lit «because there is no audio». That was a reason about the
*source* and not about the renderer, and the doctrine of that file is that where
a component can be mounted it is used and not imitated. So the real one is
mounted, on a synthetic source: a bass drum, a snare, a hi-hat, a bass line and
a pad on a grid of bars, written closed-form in the event number and allocating
nothing — which means two authors on two machines see the same image, and a
screenshot becomes a review document instead of an anecdote. It stops on three
conditions, and the second is not optional: unmount, a hidden window, and the
canvas scrolled out of view. The real scene falls asleep on silence; a synthetic
signal never falls silent, so without that guard the Studio left open behind
another window would burn a GPU indefinitely. With reduced motion it pushes one
row and stops: a still, correct picture of the colours.

**Four ready-made blocks** — «Classic», «Curated», «Flat» and «Deep» — each
applied in a single undo step. The strip that offers them is drawn generically
from the group of the token you have open, so the panel still names no token;
the blocks themselves live in Rust next to the registry, so the existing
validator owns them. One test applies each of them to `plain` and asserts the
result parses and warns about nothing, which catches both a token that has
disappeared and a number that has fallen outside its bounds — the two ways a
table of presets rots.

It is worth saying exactly what «Classic» does and does not do. It writes two
values, putting `canvas.viz.secondary` and `canvas.viz.tip` back onto
`color.accent`, and that is enough: both `mix()` collapse again and the 2.2.x
palette is back. It does not repeat the other sixteen, because they are already
at yesterday's number — the registry's defaults were *measured* off the 2.2.x
renderer rather than chosen, and repeating one here would be a second copy of a
default, the first thing to diverge the day the default moves. What «Classic»
cannot bring back is the reflection. That used to be a flat alpha, identical
from the floor to the top of a bar; it now fades upward, and it changed for
**every** skin, because it is the renderer drawing a reflection differently and
not a value that somebody moved. A knob for «draw it the old way» would be a
token that describes a version instead of describing an image, which is the kind
of knob this registry does not keep.

*Deliberately not done: no enumerated `TokenKind` for the presets.* It was the
obvious shape — a token whose value is one of four names — and it would have
made the presets the first entry in the registry to hold a **choice** rather
than a **value**. Everything downstream of `TOKENS` assumes a value: the
compiler writes it into a sheet, the Studio picks a control from its type, the
validator checks it against limits. A choice would have needed a fifth answer at
each of those points, in exchange for something a bundle of ordinary values
already expresses exactly. The four blocks write ordinary values, and a skin
that has applied one is indistinguishable from a skin whose author typed them.

*Deliberately not tokens, and it is a short list with one rule behind it:* a
number that is a budget, a correctness rule or a guarantee is not an appearance.
The half minute of memory, the number of floors in the cascade, the box budget
and the 33-millisecond step are the module's thesis; the 1.5 cap on device pixel
ratio defends somebody else's fan and battery; the lowest eighth of the bands
that drives the camera is a correctness rule; and the 0.004 minimum bar height
is the guarantee that says «it is there, and right now it is at zero» — a
guarantee with a knob on it is a guarantee somebody can take away.

**One thing came free.** The opening fade was a literal 500 milliseconds. It
reads `motion.dur.3` now, which is 450 by default: fifty milliseconds shorter,
imperceptible, and a skin that declares its long overlays slow gets a spectrum
that arrives slowly to match.

### Added — Settings › Playback: whether the spectrum shows, and what it may cost

The scene still starts off — somebody who opens «Now playing» came to look at
the cover — but the `i-eq` button used to forget the answer at every launch, and
now the choice sticks. `player.spectrum.visible` is a row in `settings` like
every other preference, and it **travels in a profile**, because «I want to see
the spectrum» is a taste of the person listening and stays true on any computer.

Beside it, `player.spectrum.quality`: automatic, high or low, automatic by
default, and the **third key deliberately kept out** of the profile's inclusion
list, after `player.queue` and `player.output`. The reason is `player.output`'s:
it describes this machine's graphics card, and «high» carried from a desktop to
a laptop names hardware that is not there. What makes this one worth arguing
about is that, unlike the other two, the damage would be **invisible**. A queue
naming rows that do not exist shows up immediately; an audio output that is gone
falls back to the system default and you notice. Here the scene simply keeps
drawing — worse, and hotter, on a machine nobody measured. A defect that never
surfaces is a defect that never gets fixed, which is why this omission needed
more argument than the two before it, not less.

There is no `sane()` clamping the value by hand. `Qualita` is
`#[serde(rename_all = "lowercase")]`, an «ultra» simply fails to deserialise,
and `read_json` already declares that malformed counts as absent — so the clamp
falls out of a rule written once instead of being re-implemented here. The
setter takes a `String` and gives back a `Qualita`, and that asymmetry in the
signature **is** the clamp, made visible to whoever calls it.

The card went into **Playback** and not «Appearance», and the line behind that
is worth stating because it is the one that divides this work in two: a skin
says how the scene looks, a setting says how much *this machine* is willing to
spend on it and whether it starts lit. The key's domain is `player.*` like
every other resident of that section, and «Appearance» is where the look lives
— which, by that line, is precisely the half that is not a setting. The card
brings no new CSS with it, and its entry in the settings search carries
symptoms among its synonyms — fan, battery, GPU, power draw — because those are
the words somebody types when they arrive here.

**And a promise nobody was keeping is now kept.** The comment on
`player.spectrum.bands` said the number of bars does not disappear when you
change device, «because preferences travel with the backup and with the sync».
That was not true of this key, and it is not true of nearly any key: the Drive
backup copies **two** rows of `settings` — the watched folders and the active
skin — and the sync copies **three**. The only mechanism that carries a
preference from one computer to another is the profile, and this key was not in
its list. It is now, next to `player.spectrum.visible`, and both comments have
been rewritten to describe what happens rather than what would have been nice.

*Deliberately not done: the bar count did not move into Settings.* The
difference between sixty-four bars and five hundred and twelve is not something
you can imagine from a number on another screen — you choose it while looking at
it, which is the argument already written where the control lives. The card in
Settings says where it is, and leaves it there.

### Added — Settings › Playback: choose the output, and a player that follows the cable

Until now the audio output was not managed at all: `uscita.rs` opened
`default_output_device()` and that was the whole of it. No list, no choice, no
way to send Aether to a USB DAC while the system stayed on the speakers —
`output_devices()` was not called anywhere in the workspace. What existed was
only fault *detection*: cpal signalled a dead stream, and a red band offered
**Reopen**, to be pressed by hand.

Three things were wrong with that, and all three are fixed.

**The band was a dead end.** Nothing ever looked to see whether a device had
come *back*: plugging the headphones in again did nothing, and the application
started without a sound card stayed a browsable catalogue until the next
restart. A new thread, `aether-dispositivi`, now re-reads the list every two
seconds and reopens by itself. It is a thread of its own and not the clock's:
enumerating WASAPI is a system call that a misbehaving driver can sit on for
hundreds of milliseconds, and the clock moves the scrubber four times a second.

**A changed default was invisible.** Switching output from the Windows settings
does not invalidate the endpoint that is already open: cpal raises no
`StreamError`, the `perso` flag stays down, the position keeps advancing, and
the sound keeps coming out of the previous card with nothing saying so. There
was no fault to see, which is why the comment in `uscita.rs` claiming to cover
«headphones unplugged, card changed» only ever covered the first. The watcher
does not look for faults: it asks `dispositivi::scegli` — **the same function
that decides at open time** — what it would open now, and compares that with
what is open. One line, and all four cases fall out of it.

**And now you can choose.** Settings › Playback grows an *Audio output* card:
system default, or a card by name, saved in `player.output`. The list keeps
itself up to date — there is no «refresh» button, because a refresh button is
the admission that a list can be stale. A device that is chosen and then
unplugged stays in the list, greyed out: removing the row would put the dot back
on «system default», that is, would claim the preference had been forgotten when
it is still written and will hold again as soon as the cable goes back in.

`player.output` deliberately stays **out** of `profilo.rs`'s inclusion list: it
is the name of a sound card, the machine-specific fact par excellence, and a
profile carrying «FiiO K11» to another computer would name nothing there. A test
holds the omission in place, because in an inclusion list a deliberate omission
and a forgotten one look exactly alike.

### Added — a chat inside the Skin Studio: ask for the change instead of writing it

The Studio already knew everything a language model needs in order to work
inside it, and told nobody. The vocabulary of what a skin may repaint is
**closed and declared** — 58 tokens with their type, 52 components, 11 effects
each with a minimal example produced by the real parser, the words allowed in a
shell node. Validation is **complete and never fails**: it answers with
positioned errors, warnings, measured contrasts, and a «did you mean» for every
misspelled name. And the source of truth is **text**, edited by path through six
pure functions with undo already wired to every write.

That is exactly the shape a model works well on: a closed schema to put in the
prompt, a textual format to edit by paths, and a validator that says what is
wrong in words the model can use to correct itself. All that was missing was
somebody to ask.

There are **two modes**, switched in the panel's header, and they are two cuts
of the same gesture — who presses Apply:

- **Agent** (the one it opens on) — the model applies, re-reads the validation
  **of the result**, and fixes what it broke. At most four rounds, with a Stop
  button that interrupts within a beat. Warnings do not restart a round: in the
  Studio a warning does not block an export, and it must not block this either.
  Neither does a round that changed nothing, nor one that produced the same
  document as the round before: a model that has dug its heels in would
  otherwise cost four paid requests for one result.
- **Proposal** — the model proposes, the panel shows the diff between what you
  have and what you would get, and you decide. It is the mode in which you learn
  what a request actually does.

It opens on Agent and not on Proposal, because Agent is the answer to the
question the panel exists for — «change this». Opening on Proposal would put a
step in front of every edit to guard against a gesture that is already reversible
twice over.

Either way the change goes through the same path a dragged slider does, so
**Ctrl+Z takes it back in one go**; in Agent mode a snapshot is taken before the
first round, because four rounds are four edits and undoing them one at a time
would be the wrong punishment for having tried.

The changes travel inside a fenced `aether-patch` block rather than through tool
calls, and that is not a shortcut: small local models get tool calling wrong far
more often than they get a fenced code block wrong. Reading that block is done
in Rust, in `aether-ia`, because it is the function that receives the least
predictable input in the whole application and `npm run verify` has no
TypeScript tests. Nine well-formed operations and one crooked one are worth
nine: what is rejected is named, not thrown away with the rest.

A step of a path is an object key **or a position in a list**, and that is the
difference between a chat that can repaint a gradient and one that cannot. A
skin manifest is full of lists — `parts.<name>.background` is a stack of layers,
every gradient carries its `stops`, a shell node carries its `children` — so the
most ordinary request there is («warm up the card's gradient») lands on
`["parts","section-card","background","0","stops","0","color"]`. Writing at the
position equal to a list's length appends to it; removing a position shifts the
rest down; a position further out is refused, because a hole in a list is a
`null` in the document. What cannot be honoured is not honoured halfway: the
operation is rejected **by name**, in the same list of reasons the model gets
back in Agent mode.

### Added — Settings › AI Models: four providers, several saved profiles

- **OpenRouter** for remote models — the only one that wants a key.
- **Ollama** (`http://localhost:11434/v1`) and **Bionic / LM Studio**
  (`http://localhost:1234/v1`) for models running on your own machine. Bionic is
  LM Studio's new agent app; it exposes no HTTP API of its own, so what Aether
  calls is LM Studio's local server — Developer tab, Start server.
- **Custom**, for any other OpenAI-compatible endpoint.

Several named profiles can coexist and one is active at a time. Each key lives
in the **operating system's keychain**, one entry per profile, never in the
library database and never crossing the IPC boundary: the core reads it an
instant before each request. Deleting a profile deletes the secret first and
stops if it cannot — otherwise the list would lose the only place where the name
of that keychain entry was written.

The «Test» button and the model list are the same request (`GET /models`): a
service that answers it answers everything — right address, process running, key
accepted.

OpenRouter counts tokens while streaming, but only tells whoever asks: Aether
asks, so the count under a finished answer is a real number on the one provider
of the four that charges for it.

### Added — models that think out loud, and the panel that shows it

A model that reasons before answering writes in two places. The OpenAI protocol
has `delta.content`; the ones that think add `delta.reasoning` — OpenRouter,
vLLM — or `delta.reasoning_content` — DeepSeek, LM Studio, part of Ollama — and
put **all** the reasoning there, which on a large model runs for a minute before
the first real word arrives in `content`.

Reading only `content` gives a panel that sits silent for that minute: anyone
watching concludes it is broken and presses Stop long before the answer. Mixing
the two gives a block of changes with the model's own discarded attempts inside
it, which does not apply. So the two travel separately all the way to the
window, which draws the reasoning in its own dimmed block — open and showing its
tail while it arrives, closed inside the message once the answer is there — and
never sends it back to the model or through the operation extractor.

### Added — two more ways for a service to be honest about failing

- A service that **ignores `stream: true`** and answers with the whole document
  at once is now read instead of refused. The answer is there and identical; a
  request that worked, and on a paid provider one already paid for, should not
  come back as an error.
- `ia.noAnswer`, for a service that accepts the request and closes without
  saying anything. It used to come out as `ia.badResponse` — «that was not an
  event stream» — which sends you to check the address, the one thing that is
  right. It happens on free models when the provider's queue expires before your
  turn, so it is retryable, and the sentence says to try again or pick a less
  crowded model.

### Added — `PRIVACY.md` § 2-quater

What the chat sends, and to whom. With a local model, nothing leaves the
machine. With OpenRouter or an address of your own, what leaves is **the skin
document you have open and what you type in the chat** — a design file and a
conversation. No track, no album, no playlist, no listening history, no file
name, no identifier of you or of this installation. And the note that choosing a
provider is choosing a privacy policy: Aether can tell you what it sends, not
what the other end keeps.

### Added — `aether-ia`

A new crate, and it earns its place by what it does **not** know: what a skin
is. It speaks the OpenAI-compatible dialect, decodes the event stream, and
recognizes a block of operations on any JSON document. The instructions that
describe tokens and components are built by the window, where the registry and
the document already are, and arrive as a message like any other.

The two pieces that get things wrong are the two that test without a network:
the stream decoder — because chunk boundaries have nothing to do with event
boundaries, and the resulting defect only shows up on long answers — and the
operation extractor. Both have a test for every way they can fail, one byte at
a time included.

No new dependency: `THIRD-PARTY-NOTICES.md` is unchanged.

### Added — Settings › Closing and background: the X can stop quitting

The window has no decorations, so the three buttons in the corner are React
(`BarraTitolo.tsx`) and the X goes through `finestra_chiudi`, which calls
`close()` and not `exit()` — «the same road as the system button, the event and
whoever listens to it, instead of going around it». That choice was made for
another reason, and it is what made this possible without touching the title bar
at all: something finally listens on that road.

With the switch on, closing **hides** the window and the music keeps playing.
Nothing had to be moved to make that true: the audio has always lived entirely
in the Rust process — `aether-play`, threads `aether-uscita` and
`aether-decodifica` — and the page does not contain a single `<audio>`,
`AudioContext` or `mediaSession`. There is nothing, on the window's side, that
hiding it stops. The OS media card keeps working for the same kind of reason:
SMTC is attached to the window's `HWND`, and `hide()` does not destroy it, which
is also why hiding is right where destroying the window would have orphaned the
session.

An icon appears in the notification area: left click reopens, right click offers
**Show Aether** and **Quit**. It is not decoration — a hidden window with no way
back is a lost program — and that is why `vassoio::nasconde` does not ask only
whether the preference is on: it asks whether the icon actually exists. If the
system refused it, or if the labels have not come down from the window yet, the
X goes back to closing. That is the right failure of the two available.

The menu labels travel **from** the window (`vassoio_lingua`), because every
text the user reads lives in `lingue/` where `strumenti/lingue.js` checks that
each language has them all. Written in Rust they would have been the only two
outside that check, and the symptom — a menu half in one language and half in
the other — looks too much like a translation slip for anyone to report it.

The `tray-icon` cargo feature is now on. `tray-icon` and `muda` were already in
`Cargo.lock`'s graph: nothing new is downloaded, something that was there gets
switched on.

Minimize is unchanged, and off is the default. An update that turned this on by
itself would make a program someone believed they had closed vanish into the
tray, with the only evidence being music that will not stop — the same doctrine
as `player.autoplay`, seen from the other side.

### Added — the sheet is checked in both directions now

`strumenti/classi.js` already refused a class in the markup with no rule behind
it; it now also fails on a rule in `stile.css` that no markup and no part of the
registry ever names, with `ATTESE_FOGLIO` as the allowlist for the names that
are composed far from where they are written. It paid for itself at once: it is
what found `.blocco` and `.intestazione.playlist`, and what noticed that
`playlist` in `parti/Navigazione.tsx` had never had a rule of its own — now
declared in `ATTESE_CLASSI`, where a deliberate absence is written down instead
of looking like an oversight.

`verify.yml` runs `strumenti/classi.js`. The `verify` npm script already did and
CI did not, which is the half of the check that catches nobody.

New tests: twelve on `aether-skin::document::effetto`, five on `media.rs`, four
on `vassoio.rs` — with `deve_esserci` pulled out so there was something to test
— and three plus three on the predicates of `aggiornamenti.rs` and
`sincronia.rs`.

### Changed — the spectrum costs a fraction of what it cost, and stops when nobody is looking

The drawing loop never idled, and the reason was one line in the wrong place: it
rearmed itself with `requestAnimationFrame` **before** its own early return, so
the branch that existed precisely to stop drawing an empty room during a pause
returned from a frame that had already booked the next one. A full frame at
every refresh, for a scene with nothing in it, for as long as the window stayed
open.

Around that, the loop paid for the same things over and over. `camera()`
returned a fresh `Float32Array(16)` on every call, and it was called once per
frame.
Reduced motion was asked by constructing a new `MediaQueryList` every frame
instead of subscribing once. The canvas size was taken from `clientWidth` and
`clientHeight` inside the loop, which is a layout read. And three tokens were
read off a live `CSSStyleDeclaration` every frame, under a comment claiming that
«costs nothing»: it does not — `getPropertyValue` on a live declaration forces a
style recalculation if the style is dirty, and the scrubber dirties it twenty
times a second.

None of that happens any more. Nothing is allocated per frame: the matrix is
written into a preallocated array, and the camera's direction is three scalars
instead of two arrays. The size arrives from a `ResizeObserver` on the canvas,
computed from `contentBoxSize` without touching layout, with `onScaleChanged`
covering the move to a monitor of a different density. The tokens are read **on
invalidation** and not per frame, and compared one at a time rather than as one
signature — so dragging a slider in the Studio costs one eyedropper and not
eighteen. The flag that asks for a re-read is raised by three narrow
`MutationObserver`s and a half-second safety net, and never by a `subtree`
observer on `document`, which would have fired for every row of every list that
repaints — that is, it would have been the cost this work exists to remove.

**And there is a rest machine, with four states:** alive, dozing, hidden and
lost. Alive goes to dozing on the silence rule that was already there, once the
past has finished leaving the room. Dozing goes back to alive on **any** event
whose maximum is above four, unconditionally and before every other check: that
is the strap that prevents the one failure worse than any waste this entry is
about, a black scene with the music playing. Hidden comes from
`visibilitychange` **or** from a resize to zero area confirmed by
`isMinimized()` — on Tauri
`document.visibilityState` is not reliable enough to be the only signal, and on
Windows a minimised window announces itself as a resize to nothing.

Being hidden **closes the tap**. `ipc.spettro(false)` shuts the emitter thread
and the tap inside the engine: thirty 4096-point transforms a second that stop
happening while nobody is looking at their result. It is the largest single item
of processor time in this release and it costs four lines, with one owner
remembering the tap's state so that unmounting cannot turn it off twice. If
WebGL 2 is missing the tap is never opened at all — until now it stayed open in
a different effect, and the engine transformed audio thirty times a second for a
canvas that drew nothing.

Rust got lighter in two places. `trasformata()` allocated a 4096-element vector
of complex numbers on every call — 32 KB, thirty times a second — while the
comment directly above it claimed to work «in the same space». The bit-reverse
permutation is now composed with the windowing in a single pass into a buffer
allocated at birth, guarded by the test asserting that the transform conserves
energy, which falls over immediately if that composition is wrong. And the ten
octave bands are behind a switch that is off by default: the ten-bar strip under
the cover art that used to draw them is gone, nothing else has ever received
them, and computing them was a scan of every bin and ten logarithms, thirty
times a second, for a number that landed in a field and died there. The function
is not deleted — the reduction is correct and belongs to the equaliser — so the
day a ten-bar strip comes back it gets switched on instead of rewritten.

**A lost context now comes back.** `webglcontextrestored` was not listened for
at all, so a driver restart or a laptop switching cards left a dead canvas until
the screen was closed and reopened. It rebuilds the two resource records and
**reloads the texture from the CPU-side rings**, which survived because they
live in ordinary memory and not on the card: half a minute of history comes back
with the picture.

Two smaller things in the shader belong here because they are behaviour and not
tidying. The `discard` is gone — with blending off and the colour premultiplied,
writing a near-zero `vec4` composites identically, but it also writes depth, and
a `discard` anywhere in a shader disables early-Z for the entire draw call. On a
grid seen edge-on, where overlap is nearly everything, that was the largest win
this shader had to give. And below a reflection strength of 0.004 the first
`drawElementsInstanced` is skipped outright: half the draw calls, free, for any
skin that asked for no reflection at all.

### Changed — reopening now resumes, and 2.3.0 says the opposite of 2.2.0 on purpose

2.2.0 wrote, in `riapri_audio` and in `ipc.ts`, that reopening had to be a
command and never automatic: cpal opened the system default, so unplugged
headphones meant a default back on the speakers, that is music in an office, at
night, in a meeting. It was right for as long as the output could not be chosen.

It can be chosen now, so the reasoning is reversed and the comments that carried
it have been rewritten rather than left to contradict the code. Whoever does not
want the sound to move pins their card, and the preference outranks the system
default; whoever stays on «system default» has asked precisely to follow the
system. What survives is the other half, and it still holds: the position is
restored, and the music restarts **only if it was playing**. Somebody who had
paused and then unplugged the headphones asked for nothing, and `Ripresa` now
carries that bit, read in the instant of the fault — the engine that reopens is
a new engine, and knows nothing of what the old one was doing.

**And the window says where the sound went.** Pressing Reopen used to say by
itself that something had changed; now the sound moves on its own, and without a
line saying so you hear the music come out of the speakers without knowing why.
There are two bands instead of one: `role="alert"` in warning colours for «there
is no audio», which stays and has a button, and `role="status"` in the accent
colour for «the sound moved to X», which leaves by itself after six seconds.
Giving both pieces of news in the same red would teach people to ignore the one
that matters.

The manual button stays. The watcher looks at the *list*, and there are faults a
list does not tell: a driver that wedges leaving the endpoint in place, an
exclusive-mode open stolen by another application. There the device is still
there with the same name, and there is nothing to compare — the button is the
last word.

**Deduplicated on the way past.** The band was written twice, in `App.tsx` and
in `parti/Colonna.tsx`, with the same `tSe` fallback and the same button in two
copies written months apart. Needing a third for the moved-device message meant
three places to update one sentence, so it is now one component,
`parti/AvvisoAudio.tsx`.

### Changed — what a 27B model on this machine taught the reader

Everything below was written against a real local model answering real requests,
not against a guess at what a model does. Five things, and each one was a round
that produced nothing until it was fixed:

- **The output contract is repeated after the document.** In front of it alone
  it lost: between the rules and the answer sit the vocabulary and twenty
  kilobytes of manifest, and the last shape a model sees is the shape it
  imitates — it answered with a ```json block holding a rewritten fragment.
  With eight lines at the end, the same model and the same question produce an
  `aether-patch` block.
- **JSON Patch spellings are read, not refused.** A model that has read ten
  thousand JSON Patches writes `path`, `value`, `replace`, `set`. Nothing in
  those is ambiguous inside a list of operations, and refusing them cost a round
  for a word. The protocol is still the Italian one; these are spellings that
  are read, never taught.
- **`add`, `push`, `append`, `insert`, `prepend` are the exception, and are
  refused** — they mean «insert before», which is not what "scrivi" does, and
  accepting them would silently overwrite the layer the model meant to push
  down. So the reason does not only say no: it says how, which in Agent mode is
  all the next round needs.
- **A rejected operation is named by its path**, not only by its number. Told
  «operation 1 was rejected», the model replied that it no longer knew which one
  that was and asked for it again — a correction round spent asking.
- **A round where nothing applied says so**, and asks for the whole change
  again. «Do not repeat what already works» after a round in which nothing
  worked told the model something was worth keeping, and it concluded there was
  nothing left to write.

With those five in, the request «warm the section-card gradient towards the
palette's brass, and add a vignette on top of that stack» comes back in one
round as three well-formed operations, applies, keeps the stack a stack, and
validates.

### Changed — the promise about requests in the clear, rewritten to be true

`PRIVACY.md` § 7 said: «No request in the clear. The HTTP client refuses
`http://` by construction.» Ollama and LM Studio speak plain HTTP on loopback
and cannot do otherwise — they have no certificate and cannot have one.

There were three ways out and two were wrong: dropping `https_only` (throwing
away the protection for everyone to fix one case), or calling those services
from the window with `fetch` (which would break the content policy and make the
window the one place in Aether that talks to the network on its own).

The third: a client that accepts `http://` **only towards this machine**, with
the host checked **on every request** and not once when the profile is saved,
and with redirects disabled — otherwise a local server answering «302
`http://elsewhere/`» would carry the request out of the house in the clear,
which is exactly what the host check exists to prevent. `localhost@evil.com` is
not this machine, and the check knows it.

The sentence in § 7 now reads: no request in the clear **leaves this computer**.
It is a weaker promise, and it is written out loud, because a strong promise
quietly worked around is worth less than a weaker one you can check.

### Changed — the settings index has groups, and AI Models moved

Thirteen entries in a column are not an index. The index now carries four
headings — **Preferences** (folders, appearance, AI models, closing),
**Listening**, **Services**, **The program** — and the sections themselves are
unchanged: same cards, same order inside each group.

*AI Models* was first placed after *Scrobbling*, and the comment defending that
was a good one: three cards in a row about a service Aether exchanges something
with, and models are the only setting in the whole program that can send
something the **user wrote** off this computer. What that ordering was
protecting, though, was that the question «what leaves, and towards whom» got
read — and an index is not where anyone reads it. It now sits inside the models
card, where the key gets pasted. The comment was rewritten rather than left to
contradict the code.

`ui.closeToTray` travels in `profilo.rs`'s inclusion list: how someone wants the
close button to behave is a habit, not a fact about this machine — the opposite
of `player.output`.

### Changed — the shape of the code, with the behaviour left alone

`apps/desktop/src-tauri/src/riproduzione.rs` was 3803 lines. It is now the
`riproduzione/` folder: `mod.rs` (the player, the state, the basic commands, the
two-locks rule), `flusso.rs` (the track that is not a file), `fili.rs` (the
preparer, the clock, the spectrum, the saves), `uscite.rs` (devices), `suono.rs`
(volume, EQ, normalisation), `riapertura.rs`, `coda.rs`. `main.rs` is untouched:
the commands still arrive through `mod.rs`'s `pub use`, so the split is
invisible from the side that registers them.

The three identical periodic threads — cloud, sync, enrichment — now come from
`stato::avvia_filo_periodico`, next to `Turno` and `aspetta_la_raffica`, where
that kind of timing already lived. `analisi` and `aggiornamenti` deliberately do
not, and the docblock says why rather than leaving the next reader to decide
whether they were forgotten.

«The extension you can get out of a URL» was written three times — the window,
the catalogue fetch, and a fresh copy that had just been born in `aether-app`.
Three copies of one rule are three answers waiting to diverge, so it now lives
once, in `aether_domain::indirizzo`. In the same spirit `aether-app` had nine identical
`db_error` and two «now in milliseconds»; they are `library::db_error` and
`library::now_ms`. In `aether-play`, `causa_perdita` and `codice_perdita` are
derived from one table instead of two lists that had to be kept in step by hand.

About forty items in `core/`'s crates became `pub(crate)` — each was used only
inside its own file — and every remaining `#[allow]` in `core/` is now
`#[expect(…, reason)]`, which fails when the thing it excuses goes away. The one
exception lives inside the `catalogo!` macro and is documented there. This does
narrow the public API of `core/`'s crates for anyone consuming them from outside
the repository: nothing in the tree did.

In `App.tsx` the cloud/sync/enrichment cluster is a `useNuvola` hook in
`nuvola.ts`, and `ipc.ts`'s DOM side effects — `applicaSkin`, `applicaAccento`,
`urlCopertina` — are in `aspetto.ts`, which leaves `ipc.ts` about talking to the
core. Four dead exports went with them: `linguaAttiva`, `indirizzoNodo`,
`percorsoNodo`, `NOMI_WIDGET`.

`stile.css` lost 252 lines of rules no markup wore any more — the `.mini*`
family of a miniature player that no longer exists, `.solo-voce`,
`.con-suggerimento`, `.blocco`, `.intestazione.playlist`. 10 557 lines to
10 305.

### Fixed — a fault that was perfectly recorded and perfectly mute

`ia.badResponse` carried the service's own explanation in a field of the error
code, and the fields of an error code do not cross the IPC boundary and are not
written to the diary. The line in the log read `ia.badResponse cause=—`: the one
piece of information that says whether the fault is theirs or ours, collected
and then dropped one step short. It now travels in the cause, where both the
diary and the window can see it.

### Fixed — the cursor dragged to the end said the file was damaged

**Dragging the cursor all the way to the end of a FLAC answered «This file is
damaged and cannot be decoded. If you have another copy, replace it and scan
again.»** On a file with nothing wrong with it, and with no «Retry», because the
catalog declares `playback.decodeFailed` never retryable. Not now and then:
every time.

`Decodificatore::cerca` handed symphonia the millisecond it had been asked for
without ever comparing it to the real length of the stream. symphonia 0.5.5
refuses with `SeekError(OutOfRange)` the moment `ts > n_frames` — FLAC, WAV,
AIFF, Ogg, MP4 — and `codec_params.n_frames` **was read nowhere in this
project**. The fallback translated that refusal into `playback.decodeFailed`.

The two numbers don't agree, and by construction they can't, which is the heart
of it: the duration the window shows comes from the **database** — written by
lofty at scan time, or taken from a remote catalog's metadata for a streamed
track, or rewritten by an import from outside — while the length symphonia
accepts is the container's, which `enable_gapless: true` shortens further still
by dropping the encoder's padding. The cursor dragged to the end sent
**exactly** the database's duration.

Eight routes arrived there with no ceiling over them: the cursor, synced lyrics
(where an `.lrc`'s timings are allowed to overrun the end by **ten seconds** and
stay clickable), the sync editor, the media keys, the keyboard, «resume where
you left off», the network's own «Retry», and the fingerprint pass.

So the ceiling lives in the **decoder**, the one place that holds the format
reader and therefore the only length symphonia will agree with; from there it
covers all eight. A margin of 120 ms, which sits under the granularity that
seeking already has — a FLAC block is 4096 frames, about 93 ms at 44.1 kHz.
Where the ceiling can't reach — MP3 declares no `n_frames` and runs out of bytes
scanning forward, that is `IoError(UnexpectedEof)` — the refusal is taken for
what it is, a track that has ended: the queue moves on instead of stopping with
a warning. **The network is looked at first**, and that needs saying: a share
that dies during a seek arrives as an `IoError` too, and taken for «end of
track» it would vanish in silence, with no «Retry» and no point to come back to
— that is, it would reopen exactly the wound 2.1.0 had closed.

**And the silent damage, which is worth a paragraph of its own.** On the
fingerprint route the same seek became `Misurato::Negata{ILLEGGIBILE}`: a
healthy track with a wrong duration in store was marked **unreadable**,
disappeared from affinity and from autoplay, and left not one line in the diary.
The fingerprint's window is now measured on the real length of the stream when
there is one.

Two callers took a ceiling of their own anyway, because the one at the bottom
would have saved them by changing what the gesture meant. «Forward five seconds»
from the media keys, pressed three seconds from the end, would have become «next
track» instead of «go to the end» — `apps/desktop/src-tauri/src/media.rs`, where
the function's own comment already promised «without leaving the track» and the
code did nothing but `saturating_add`. And the window's cursor now sends the
number it was already *showing* (`apps/desktop/src/parti/Scrubber.tsx`).

### Fixed — a reopened device puts the needle back in the groove

2.1.0 taught the application to say that the sound card had gone and to offer
**Reopen**. What was left was that reopening **lost the position**: the new
engine is born with nothing open, and the track started over — a twenty-minute
song unplugged at the fourteenth minute began again from the top. It was
declared in the tooltip, as a thing given up on.

Now the clock thread, in the exact instant it sees the device disappear, notes
the track and the millisecond with the **same** function the network branch was
already using — a note that lives outside the player's lock precisely so that it
survives the engine being replaced — and «Reopen» opens that track at that
point. **Playing again — but only if it was playing**: the note carries that bit
too, read in the same instant. This paragraph used to say «paused, always», and
the reason was a good one: `cpal` opens the system's *default* device, which
after an unplugged cable may be the laptop's speakers, and hearing the music
start on its own in a meeting is a defect. That reason fell with the output
picker, for the reasons set out under *reopening now resumes* — whoever does not
want the sound to move pins their card. What is left of it is the other half,
and it still holds: somebody who had paused and then unplugged asked for
nothing, and stays paused. `riapertura.rs` records the superseded rule next to
the branch that replaced it, rather than leaving the comment to contradict the
code. The tooltip in `it.json` and `en.json` has been rewritten, because it
promised the opposite.

**And the band no longer speaks two languages.** The cause of the fault was born
as an Italian sentence inside the engine, crossed the IPC boundary, and was
printed raw next to a translated string: with the interface in English it read
**«There is no audio. dispositivo non più disponibile.»** — half a line in one
language and half in the other, in the one panel that opens when something has
already gone wrong. What crosses the boundary now is a code, which gets
translated; the diary goes on writing the Italian sentence, because the diary is
Italian all the way through. It is the rule the same file already wrote for
`motivo_prossimo`: the code and not the sentence.

### Fixed — Aether introduced itself as version 0.1

To MusicBrainz, the Cover Art Archive, iTunes, Deezer, LRCLIB and ListenBrainz,
Aether said it was **`Aether/0.1`**, and had done since the beginning: the string
was written by hand in `core/aether-net/src/http.rs`, and
`strumenti/versione.js`, which keeps the other three declarations of the version
in step, never looked at it. Whoever runs MusicBrainz asks for a recognizable
version, and presenting one that doesn't exist is the way to get rate-limited —
that is, to trip exactly the switch that then stops an enrichment pass. The
compiler writes it now.

### Fixed — a prevented close no longer shuts down half the program

`main.rs`'s run loop raised `spegnimento::chiedi()` on *every* `CloseRequested`,
and that flag never comes back down by declared design. Left as it was, hiding
the window would have produced the worst of the two possible failures: music
still playing while the clock, the spectrum, the device watcher, the resume
position, scrobbling and the system media card all stopped for good — a live
program behaving like a dead one. The block is now guarded, and a close that was
prevented is not an exit.

### Fixed — one door for every core event, and dialogs that answer the keyboard

`useAscolto` (`pagine.ts`) carries a docblock that names the bug it exists to
prevent: a bare `listen()` cleaned up with `promessa.then(stop => stop())`
subscribes a second time when the effect restarts before the promise resolves,
and from then on the event arrives twice. Roughly twenty places in the frontend
were still writing exactly that pattern — the description of the mistake and the
mistake itself living in the same tree. All of them go through the hook now. On
`tauri://drag-drop` the second copy was not merely noise: a dropped skin
installed itself twice.

The worst case was `parti/LetturaLink.tsx`, which resubscribed at every open.
Fixing it has a declared side effect: the link bar can now appear already filled
in rather than empty. That is what subscribing once and keeping the subscription
costs, and it is the smaller of the two behaviours.

**Dialogs and menus can be driven from the keyboard.** `Chiedi.tsx` declares
`role="dialog"` and `aria-modal`, traps Tab inside itself, and gives the focus
back on close — when whoever opened it is still mounted and still focusable,
which is the only case in which handing focus back lands anywhere. `Menu.tsx`
walks with the arrows, Home and End, and restores focus too. The equalizer panel
declares `aria-modal`. A dialog that cannot be left without a mouse is a trap,
and these were.

**`import_esterno::salva_rapporto` no longer writes to stderr.** It held the
core's only `eprintln!`, and it announced a policy the code already states
better: the «carry on anyway» is right there in the `let … else`.

### Fixed — a library on a network share no longer freezes the window

Three Windows `Application Hang` events in one morning, the music on an SMB
share over Wi-Fi, and not one line in the diary. A scan held the library's lock
from beginning to end while opening files that, on a share which is slow rather
than broken, never fail — they wait — and every synchronous command waited with
it, the covers the window asks for and the shutdown included. The scan now asks
for the connection one phase at a time and holds nothing at all while it reads
the disk, and every step on the filesystem has a deadline: fifteen seconds of
silence between two files while walking, twenty-five to read one track. A root
that stalls is skipped without deleting anything — files nobody managed to
enumerate are not files that vanished — Cancel is heard during the walk too,
removals match the path as well as the id, and one unreachable root no longer
aborts the roots that still answer.

Around that: covers are read without the lock, a second scan is refused with
`library.scanBusy` instead of queueing, quitting gives the library 400 ms and
then leaves anyway after telling the scan to stop, and the diary finally says
`[stato] lucchetto della libreria non preso in 2 s: filo «…»` when someone
waits. Playback had the same silence in another shape: the decode thread was
spawned and its handle thrown away, so a panic on truncated bytes left the
output feeding zeros with «playing» still on the screen. It is watched now — a
fall raises the same reopen flag with a cause of its own, «the decoder stopped»,
and the engine gets rebuilt instead of staying mute; a replaced engine no longer
announces events, a track that has already brought the thread down is not put
back on, and the sample rate and channel count a file declares are checked
instead of standing in for an invented 44 100.

---

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
