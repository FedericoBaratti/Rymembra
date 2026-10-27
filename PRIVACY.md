# Privacy

Last updated: 26 October 2026.

**Rymembra has no telemetry, no analytics, no crash reporting, and sends nothing
to me.** There is no Rymembra server. There is no account to create. There is
nothing I can see of what you do. (When you host a «Listen together» session,
your own PC serves the other PCs on your network that you paired with it, and
nobody else: § 4-ter.)

That said, the program does talk to the network — a player that looks for cover
art and downloads music necessarily does. This document lists **every single
request** it can make, when it makes it, and what goes inside. If you find one
that isn't written here, that's a defect: report it.

---

## What stays on your computer

Everything Rymembra knows about you:

- **The library**, in an SQLite database in the application's data folder
  (`%APPDATA%\io.github.federicobaratti.rymembra` on Windows): tracks, playlists,
  ratings, favorites, listening history, play counts.
- **The settings**, in the same folder.
- **The cover art**, in a file cache next to the database.
- **Your skins and Skin Studio drafts**, in the `skin` folder beside it —
  including, from 3.0, the conversations with the Studio's chat (§ 2-quater).
- **The secrets** — the Google Drive tokens, the scrobbling credentials, the
  API keys of any language model you configure and, from 3.1, the key that
  identifies this PC to the others in «Listen together» — in the **operating
  system's keychain**, not in a file. On Windows that's Credential Manager.
- **The PCs you paired** for «Listen together», if any: their public key and
  the name they had, in the same database (§ 4-ter).

If you come from the previous version, the first time Rymembra starts it
transfers that version's data automatically: the library, the settings, the
cover art, the skins and drafts, and the secrets it had saved in the keychain,
which are moved to the new entries rather than copied. The installer removes the
previous version but never its data. All of this happens on your computer; no
request is made for it.

None of this leaves your computer, except for the backup if you turn it on
yourself (§ 4), and what a listening session shares with the PCs you paired —
on your own network, and only if you start or join one (§ 4-ter).

---

## 1. The music catalogs

These start when **you** ask for something: you paste a link, or you confirm an
import that puts tracks in the queue.

| Host | When | What reaches it |
| --- | --- | --- |
| `archive.org` | Searching for a track, reading an item, fetching a file | The title and artist you're searching for, or the item identifier; your IP address |
| `api.jamendo.com`, `prod-1.storage.jamendo.com` | *(not compiled in this version)* Search and listening | As above, plus the `client_id` you will have configured yourself |
| `api.audius.co`, and whichever *discovery node* answers | Searching for a track, reading a link, fetching a file | The title and artist you're searching for, or the identifier; `app_name=Rymembra`, which is the same for every copy of the program; your IP address |

A link that doesn't belong to one of these three domains is **not attempted**:
the program refuses before opening any connection. That isn't generic caution,
it's the rule that keeps Rymembra inside the terms of the people who give it the
music.

Rymembra always identifies itself with the same `User-Agent`:
`Rymembra/0.1 (+https://github.com/federicobaratti/rymembra)`. It doesn't disguise
itself as a browser and it can't: the constructor that allowed that was removed
along with the code that needed it.

---

## 2. Metadata and cover art

These start on their own, on a background thread, when the library holds tracks
with no album, no cover art, or a title that looks derived from a file name.
Automatic enrichment **can be switched off** in Settings › Metadata.

| Host | What reaches it | Rate |
| --- | --- | --- |
| `musicbrainz.org` | Artist, album and track durations of a record you're trying to identify | One request every 1.1 s |
| `coverartarchive.org` | The MusicBrainz identifier of a record | One every 250 ms |
| `itunes.apple.com`, `is1.mzstatic.com` | Artist and title, for the cover | One every 350 ms |
| `api.deezer.com` | Artist and title, for the cover | One every 220 ms |

None of these receives an identifier belonging to you: there's no API key,
there's no cookie, there's no session. They see an IP address and a search
string, the way a search engine would.

The rates aren't abstract courtesy: MusicBrainz explicitly asks for one request
per second, and exceeding it means getting blocked — for yourself and for
everyone else using the same program.

---

## 2-bis. Song lyrics

Rymembra looks **first** at what you already have: an `.lrc` file next to the
track, or the lyric inside the file's tag. That doesn't leave your house and
doesn't cross any network.

When there's neither, it asks LRCLIB.

| Host | What reaches it | Rate |
| --- | --- | --- |
| `lrclib.net` | Artist, title, album and duration of the track | One every 250 ms |

It's worth being precise about **when** it starts, because that's the part that
matters:

- only for the track you're listening to at that moment, and **only while the
  lyrics panel is open**. With that panel closed, nothing starts;
- or when you press «Fill the library» in Settings › Folders, which is the only
  sweep over the whole library and you're the one who launches it.

There is no background thread going through the library on its own. The
difference from the metadata in section 2 is deliberate: there, files are being
tidied; here, you'd be telling somebody else what you're listening to right now.

As with the others, LRCLIB receives no identifier of yours: no key, no cookie,
no session. And as with the others, it switches off — Settings › Folders ›
Lyrics. Switched off, what remains are the lyrics you already have in your
files.

**No downloaded lyric goes up** in the Google Drive backup: what comes from the
catalog can be downloaded again, and re-uploading it elsewhere would be
redistributing it for no technical reason. Only the `.lrc` files you wrote
yourself and the offset corrections go up.

### When you're the one sending something

There is exactly one point in the whole app where content goes out to a public
service, and it's the **«Give it back to LRCLIB»** button that appears after
you've synced a lyric by hand. There is no other way to reach it: it isn't
automatic, it isn't in bulk, it isn't attached to saving.

What goes out, exactly:

| What | Why |
| --- | --- |
| Artist, title, album and duration of the track | They're the keys the catalog uses to find it again |
| The lines with their timings | That's the contribution |
| The same lines without the timings | The catalog keeps them in two forms; the untimed text is derived from the timed one, never from anything else |

And what does **not** go out: no file path, no device identifier, no account, no
cookie. LRCLIB doesn't ask for an account and Rymembra hasn't got one to give it.
What reaches whoever hosts the catalog is the IP address the request comes
from, as with any site.

Two things that are refused before they even start, and that are worth knowing:

- **a lyric that was inside a file's tag is never published.** You didn't write
  it, and sending it with your name on it would be passing off someone else's
  work;
- **what came from the catalog doesn't go back to it.** That would be noise,
  and nothing else.

The rule lives in the core (`testi::da_restituire`), not in the window: it holds
even if one day the button were somewhere else.

With the lyrics switch off the button doesn't appear: you've already said you
want no traffic towards that service, and this is its most explicit form.

---

## 2-ter. Similarity between tracks

Rymembra tries to work out **which of your tracks resemble each other**, so it can
propose something sensible when a record ends. It does this in three ways, and
two of the three never leave the house:

- **the sound** — thirty seconds from the middle of each file, analyzed on your
  computer. No byte of that analysis leaves it, ever, for any reason (audio
  leaves this computer in one case only: a listening session you start or join
  yourself, towards PCs you paired, on your own network — § 4-ter);
- **your listening** — what you've already heard and for how long, from your
  database;
- **the culture** — «people who listen to this also listen to that», which is
  the only one of the three that has to be asked of somebody.

Only the third touches the network.

| Host | What reaches it | Rate |
| --- | --- | --- |
| `labs.api.listenbrainz.org` | Up to 25 MusicBrainz identifiers of tracks or artists per request | One per second |

**What actually reaches it.** Recording and artist identifiers, that is, numbers
that mean «this song» in MusicBrainz's public catalog. Nothing else: no key,
no cookie, no session, no file name, no moment at which you listened to it, no
count of how many times. As with the cover art in section 2, they see an IP
address and a question.

**One thing to watch, though**, and it's the reason this section is separate
from section 2. A single request contains up to twenty-five tracks **from your
library**, together. Taken one by one they're anonymous questions; taken in
batches, the succession of batches is a blurred portrait of what you own. There
is nothing tying them to you — no identifier crosses the requests, and the order
of the batches is the database's and not your preferences' — but it would be
dishonest to write «one track at a time» and let you believe it's less than it
is.

**When it starts.** On the background thread, like the metadata, and **only for
tracks that already have a MusicBrainz identifier** — that is, the ones
enrichment has recognized. On a library that hasn't been enriched, nothing
starts. Each identifier is asked about **once every three months**: the answer
is the residue of years of aggregated listening and doesn't change from one day
to the next, so asking often would be traffic spent getting the same number
back.

**It switches off** in Settings › Metadata, together with the enrichment it
depends on. Switched off, the sound and your listening remain: the queue keeps
working, it knows a little less.

**Why ListenBrainz and not something else.** Because MetaBrainz's data is CC0 —
public domain, commercial use included — and because it's the same foundation
Rymembra already sends scrobbles to if you asked it to (section 3). The two stay
separate: **scrobbling goes through your account, this doesn't.** If you haven't
connected any ListenBrainz account, this feature works all the same and
completely anonymously, because there's no account for it to lean on.

---

## 2-quater. The language model in the Skin Studio

This starts **only if you configure one**, in Settings › AI Models. Until you
do, the Skin Studio chat talks to nobody: there is no default provider, no
built-in key, and no request.

There are two very different situations here, and the difference is the whole
point of this section.

| Host | When | What reaches it |
| --- | --- | --- |
| `localhost:11434` (Ollama) | If you configure a local model | The skin document, what is selected in the Studio and what you type in the chat; a picture of the Studio's preview if you switch on «Preview» |
| `localhost:1234` (Bionic / LM Studio) | As above | As above |
| `openrouter.ai` | If you configure an OpenRouter profile | As above, plus your API key |
| an address you write yourself | If you configure a custom profile | As above |

**With a local model, nothing leaves this computer.** Ollama and LM Studio are
programs running on your own machine; the request goes to `127.0.0.1` and stops
there. This is the reason Rymembra allows an unencrypted `http://` in exactly one
case — see the rewritten point in § 7.

**With OpenRouter, or any address of your own, something does leave.** Namely:

- **the skin document you have open** — the JSON manifest: colors, spacing,
  effects, animations, layout. It is a design file, not your data. From 3.0 a
  large document goes out in part: the sections your question, your selection
  and the open errors name, and a list of the names left out;
- **what is selected in the Studio** — the name of a part, a token, a colour or
  an animation, with the state and theme you are looking at — as one line in
  front of your message, so that «this» means something;
- **what you type in the chat**, and what the model has already answered in the
  same conversation (past a certain length, the older turns go out shortened);
- **a picture of the Studio's preview, only if you switch on «Preview»** under
  the chat. It is off until you do, and when it is on one picture goes with each
  question you send. The preview is the application running on **sample
  data** — invented albums, artists, tracks and covers that ship with the
  program — so the picture shows those and never your library. It is made
  inside the window from that preview, a JPEG at most 1,280 pixels wide, and it
  is sent once: later turns do not send it again;
- **your API key**, in the `Authorization` header, because that is how the
  service knows who is asking;
- two courtesy headers naming the application (`Rymembra`, and this repository's
  address), the same information the `User-Agent` already carries.

**What does not go, ever.** No track, no album, no artist, no playlist, no
listening history, no play count, no file name, no path, no identifier of you or
of this installation. The chat has no access to your library, and the request is
built by the core from the skin document, the selection, the conversation and,
if you asked for it, the picture of the sample preview — nothing else is in
scope to add. The core also checks that a picture is a PNG, JPEG or WebP image
before anything is sent.

**The conversation is kept on this computer.** From 3.0 it is saved next to the
draft it is about, in `skin\bozze\<id>\chat.json` inside the data folder: what
you wrote, what the model answered, the token counts the service reported and
the list of changes each answer made. The pictures are not saved. Discarding
the draft deletes it, and «New conversation» empties it. It is **not** part of
the Google Drive backup (§ 4) nor of the profile archive (§ 4-bis): both carry
only the draft's `skin.json`.

**The key lives in the operating system's keychain**, one entry per profile
(`ia.<profile>.chiave` under the usual Rymembra service), never in the library
database. The window never receives it: the core reads it an instant before each
request and drops it afterwards.

**A whole conversation is one request per turn.** There is no background thread,
nothing periodic, and nothing that starts on its own — every request is one you
made by pressing Send. Closing the window stops a generation in progress.

**Choosing a provider is choosing a privacy policy.** Rymembra can tell you what
it sends; it cannot tell you what the other end keeps. OpenRouter routes your
request to a third-party model provider of its own, each with its own retention
rules. If that matters to you, a local model is the option where the question
does not arise.

---

## 3. Scrobbling

This starts **only if you connect it**, and it sends exactly what a scrobble is.

| Host | When | What reaches it |
| --- | --- | --- |
| `ws.audioscrobbler.com`, `www.last.fm` | If you connect Last.fm | Artist, title, album and the instant you finished listening |
| `api.listenbrainz.org` | If you connect ListenBrainz | As above |

Only listens that got at least halfway through the track count. The queue of
what's still to be sent lives on your disk, and there's a button that empties it
without sending it.

**Your Last.fm application key lives in the operating system's keychain**, next
to the secret that goes with it. Up to 2.3.0 it sat in the settings table
instead, on the grounds that it is not a secret — it travels in the clear in the
consent URL, so that part was true. The conclusion was wrong: the question is
not whether it is secret but *whose it is*. It is a personal credential you
registered in your own name, and anyone holding it scrobbles as you until you
revoke it. That did not matter while the settings table stayed on one machine;
it started mattering in 2.3.1, when the profile archive became something you put
on a USB stick. Rymembra moves the key across once, at startup. If the keychain
does not answer, the key **stays where it is** rather than being lost, and the
profile leaves it out either way.

---

## 4. The Google Drive backup

This starts **only if you connect it**, and it isn't on by default.

| Host | When | What reaches it |
| --- | --- | --- |
| `accounts.google.com` | On connecting | The consent screen opens in the **system browser**, never inside Rymembra's window |
| `oauth2.googleapis.com` | On connecting and on renewals | The token exchange |
| `www.googleapis.com` | On every backup | The backup document |

The backup contains your **library**: tracks, playlists, ratings, history. Not
the audio files. It ends up in the application-reserved folder of **your**
Drive, where no other program sees it. The backup files that the previous
version left there are recognised and renamed in place the next time it runs, so
nothing is uploaded twice, and a file is removed only when an identical copy
under the new name is kept; the rename is one more request to Google Drive, of
the same kind as the backup itself.

The refresh token lives in the operating system's keychain. Disconnecting the
account deletes it; what has already been uploaded stays on your Drive until you
delete it yourself.

The alternative that touches no network exists: syncing to a shared folder.

---

## 4-bis. The profile archive

This touches no network at all. It is here because it is the one file Rymembra
writes that is *meant* to be carried somewhere else, and you should know what
is inside it before you hand it to anyone.

*Settings › Profile › Export* writes a `rymembra-profilo.ryprofile` where you ask
for it. It is a zip; you can open it with any archive manager and read what it
holds:

| Inside | What it is |
| --- | --- |
| `manifesto.json` | What the archive claims to be, and an explicit list of what it left out |
| `preferenze.json` | Your settings — theme, skin, equalizer, shortcuts, watched folders |
| `sincronia/rymembra-<device>.v1.json.gz` | Play counts, ratings, favourites, resume positions, playlists |
| `biblioteca.v1.json` | Listening history, your metadata corrections, lyrics, the "to buy" list |
| `copertine/…` | The cover art cache |
| `skin/…` | Installed skins and Skin Studio drafts |

What is **not** inside, and cannot be:

- **Your music files.** The archive names tracks, it does not carry them.
- **Any token, session key or credential.** Those live in the operating
  system's keychain, and the code that writes the profile cannot see it. That
  includes the Last.fm application key, from 2.3.1 (§ 3).
- **This computer's backup device identifier**, its audio output, its measured
  output latency, its spectrum quality setting, and the open nodes of the
  Folders panel. These describe a machine, not a person.
- **The PCs paired for «Listen together»**, and that feature's settings. A
  pairing is a fact about two machines, and an archive restored on a third one
  must not make it trust PCs it has never met (§ 4-ter). The same goes for the
  Drive backup and for synchronization: neither carries them.
- **Similarity data and the weekly picks**, which are derived and are
  recomputed from whatever library they land in.

Importing one **adds and never removes**: listens are summed, playlists merged,
covers and skins written only where none exist, and a lyric you synced by hand
is never overwritten. Before writing anything, Rymembra saves what your computer
looks like right now to `<data folder>\profilo\prima-<timestamp>.ryprofile`,
keeping the last three. That backup restores your settings — not the merged
history, which is additive and has no undo, and which the interface says so
rather than promising a return that does not exist.

---

## 4-ter. Listen together, on your own network

From 3.1, two or more PCs with Rymembra on the same network can listen to the
same music at the same moment: one **hosts**, the others **follow**. This
section is everything that feature puts on the network. It is **off by
default**, and while it is off Rymembra opens no socket for it and sends no
packet: not one.

### While it is on, and you are not in a session

- **Looking for hosts.** Every ten seconds, and right away when you open its
  panel, Rymembra sends a **40-byte datagram**: a fixed tag, a format version
  and sixteen random bytes. Nothing else — no name, no title, nothing about
  you. It goes out as a broadcast on each of this PC's local networks, which
  routers do not forward, and as a single datagram to each address you typed by
  hand in *Settings › Listen together*, which goes wherever that address is.
- **Only a PC that is hosting answers**, with **34 bytes**: the port to connect
  to and an 8-byte fingerprint of its public key. So a hosting PC is
  recognizable on your network; its name is not. These datagrams are the only
  bytes «Listen together» sends **in the clear**.
- **A connection to each host it sees**, at most sixteen, kept open so an
  invitation can reach you. Every connection is encrypted and authenticated —
  Noise `XX`, with X25519 keys and ChaCha20-Poly1305 — and inside it the two
  PCs tell each other their **name**: the one in *Settings › Listen together*,
  which by default is the Windows computer name. That name is said to any
  Rymembra that completes the handshake, paired or not. Nothing else is said to
  a PC that is not paired.

### Pairing

The first time two PCs meet, both screens show the **same four-digit code**,
and both people confirm it. The code is computed from the encrypted handshake
and from random numbers each side committed to before seeing the other's, so
a third PC in the middle cannot arrange for two matching codes. After that
each PC remembers the other's **public key** and name, in the table
`insieme_pari` of its own library database, and can forget or block it at any
time. Three failed pairings in ten minutes from the same PC, or ten from any,
and pairing stops for a while.

This PC's own key — the secret half — lives in Credential Manager, under
`insieme.chiave.<device>`, never in the database, and it is wiped from memory
when the feature stops.

### In a session

Only between PCs that are paired, and only inside the encrypted connection:

| What | From whom, to whom |
| --- | --- |
| The names of the PCs in the session | the host, to every member |
| What each member may do — listen, control, or control and change the queue — as the host chose it when they joined | the host, to every member |
| The session queue: title, artist, album, duration, track number, year, ReplayGain values | the host, to every member |
| The cover of each track in that queue | its owner, to whoever displays it |
| The **bytes of the file** of a track in that queue, when a member plays it | its owner, to that member |
| For a catalog track, only its reference: each PC fetches it from the catalog by itself, as in § 1 | the host, to every member |
| Pause, seek, skip, and the timing that keeps everyone in sync | between host and members |
| The gestures of a member, and the tracks a member adds to the queue, which then travel like the host's | a member, to the host, which passes them on |
| To recognise a copy a member already has: the file's size, its modification date, a hash of its first and last 64 KiB, the library's content key — and its path **only** when it is a network share (`\\server\…`), never a path on a local disk | the owner of the track, to every member |

A member can read **only** the files of the tracks in the session queue, each
through a random 16-byte permission that ends with the session, when the track
leaves the queue, or when that member leaves. Your library, your history, your playlists, your
ratings and any other file stay where they are.

Tracks that belong to another PC are **not counted** in your listening
history and are **not scrobbled**; your own tracks count on your PC as they
always did, and so does an identical copy of a shared track, when the session
found one on your disk.

### The firewall

Only the PC that **hosts** listens: on TCP and UDP port **47811**, or one of
the nine after it, or any free one. The first time, Windows asks whether to let
Rymembra through, and Rymembra explains what to choose before it happens. A PC
that follows opens no port.

To show you the address a colleague can type by hand («This PC's address»),
the hosting PC reads the addresses of its own network adapters and asks
Windows which one the default route goes through — a lookup on this computer
that sends nothing. The address is shown on your screen and is not written to
the log.

### The log

As for everything else (§ 8): error codes, counts, durations in milliseconds,
the name of a gesture («pause», «next»…), the level a member was given
(«listen», «control», «everything») and the row number of a paired PC, of a
connection or of a member. Never an address, a PC name, a title, a key, a permission or
a pairing code. The durations are there to answer one question when a
session feels slow — where the time went: how long a member waited for the
host's answer to a gesture, how long the gesture waited on the host, and how
long the session's own thread was busy.

---

## 5. The shops, when you press the button

In the «To buy» list there are three buttons — Bandcamp, Qobuz, Discogs — that
open a **search** in the system browser.

They don't start on their own: only the click starts them. They aren't affiliate
links, there's no tracking, and the address is built by the program from a
closed list of three domains written in the code — deliberately, so nothing else
can be slipped in, not even by mistake. What those sites know about you from
there on is between you, them and their own policies.

---

## 6. The update check

This is **the only request Rymembra makes without your having asked for it**, and
that's why it gets its own section instead of a line at the bottom of a list.

| Host | When | What reaches it |
| --- | --- | --- |
| `github.com` | Ten seconds after the window is ready, then every thirty minutes; after a failure for lack of network, again after one, five and fifteen minutes | Nothing: a GET to a public file |
| `objects.githubusercontent.com` | Only if you press «Update» | The request for the installer file |

**One check at every start, but not one per click.** Until 2.4 the startup check
was skipped when the last successful one was less than half an hour old, which
meant that whoever reopened Rymembra after twenty minutes never got the check
`TODO.md` asked for. From 3.0 the threshold is five minutes: it is there for a
shortcut double-clicked or a start that crashes and repeats, so that opening and
closing Rymembra ten times in a minute still makes one request, not ten. Pressing
«Check now» is the exception, and deliberately so — a button you pressed has to
do something.

The file it downloads is called `latest.json`, sits among the latest release's
assets, and is public: it's the same one you'd see opening the releases page in
a browser. Inside are a version number, two lines of notes and an address.

**Nothing of yours goes into the request.** No identifier, no serial number, no
counter, no information about your library. Not even the version you have
installed travels: the comparison is done by the program on your computer, after
reading the file. The request doesn't even carry a `User-Agent` naming Rymembra,
and that is deliberate rather than forgotten — the code says so where the request
is built. What GitHub can infer is that somebody, from a certain IP address,
asked for a public file — exactly what it would know about anyone opening that
page with a browser.

**It installs nothing on its own.** When it finds a new version it says so and
stops. (The installer, whether you download it by hand or the update check
brought it, removes the previous version of the program before copying the new
one; it touches no network for that, and it leaves the data folder alone.) Downloading and installing start from a button, because installing means
closing Rymembra, and that isn't something to do while you're listening. If you
answer «Not now», it doesn't ask you about that version again: it asks you about
the next one.

**What it downloads gets verified.** The installer carries a cryptographic
signature, and Rymembra checks it against a public key compiled into the
executable before running anything at all. If it doesn't check out, the
installer is thrown away. It serves one precise case: someone who manages to
answer in GitHub's place — a corporate proxy, one certificate too many in the
system store — can't make you install a program of theirs.

**It switches off**, in *Settings → Updates*. Switched off, Rymembra never
contacts GitHub: you'll stay on this version until you come and press «Check
now» or download the installer by hand. It's on by default, and that's a choice:
a music player that doesn't update is a music player holding a network library
that's months out of date.

**In the Microsoft Store version it doesn't exist.** A copy installed from the
Microsoft Store is kept up to date by the Store, not by Rymembra: it never asks
GitHub for `latest.json`, the table above doesn't apply to it, and the switch in
*Settings → Updates* says so instead of turning anything on. The Store, for its
part, is Microsoft's service, with Microsoft's privacy statement: what it learns
when you install or update an app is between you and it, and Rymembra doesn't see
any of it. The same copy writes the files it creates in the application's
private folder under `%LOCALAPPDATA%\Packages\` instead of `%APPDATA%`, which
Windows arranges by itself; uninstalling it from Windows removes that folder.
Files that already exist in `%APPDATA%` — the library of a copy installed with
the installer on the same PC — Windows, from version 1903 on, lets it use where
they are, and the connected accounts in the Windows Credential Manager are the
same for both copies. The two have not been tried side by side: keep one of
them per PC.

---

## 7. What *doesn't* happen, and is worth saying

- **No request at startup, except the one in § 6 — and «Listen together»'s, if you switched it on.** Opening Rymembra sends nothing
  to anyone for the first forty-five seconds, and after that sends a GET to a
  public file — or not even that, if it already did less than half an hour ago.
  There's no other ping, and nothing goes out on closing. The one exception is
  yours to choose: with «Listen together» switched on, looking for hosts on
  your own network starts with the program, and closing tells the PCs of a
  session that you left (§ 4-ter).
- **Nothing in the clear leaves for the internet.** The HTTP client refuses
  `http://` by construction, with exactly one exception: a language model
  running on your own machine (§ 2-quater). Ollama and LM Studio have no
  certificate and cannot have one, so a client built for one of them accepts
  plain HTTP — and only towards `localhost`, `127.0.0.1` or `[::1]`. The check
  is redone **on every request**, not once when the profile is saved, and that
  client follows no redirects: a local server answering «go to
  `http://somewhere-else/`» cannot use it to carry your request out of the house
  in the clear. Every other address, for every other feature, is still refused
  unless it is `https://`.

  This is a weaker promise than the one that used to be written here, and it is
  written out loud on purpose: a strong promise quietly worked around is worth
  less than a weaker one you can check.

  «Listen together» does not use the HTTP client: its connections are
  encrypted with Noise (§ 4-ter), and the only bytes it sends in the clear are
  the 40-byte discovery datagrams and the 34-byte answers of a host, which
  carry no name and nothing about you.
- **No external domain in the window.** The content policy (`tauri.conf.json`)
  admits only `'self'` and `data:`. The preview cover art arrives as `data:`
  URIs already downloaded by the core: the page never talks to a catalog
  directly, and cannot.
- **No cookies, no local storage** holding personal data.
- **No writing outside the folders you pointed at**, apart from the
  application's data folder.

---

## 8. What Rymembra does *not* touch

**Your music files are not modified.** Rymembra opens them to read them — to
scan them, to decode them, to measure them — and never to write. Up to 2.3.0
that was not true, and this section used to say so; from 2.3.1 the complete
list of what Rymembra writes is the one below, and your `.flac` and `.mp3` files
are not on it.

What Rymembra writes, and where:

- **Files it downloaded itself**, in the download folder you chose. Rymembra
  writes the tags and the cover art into those, because it is the one that
  created them: a file that arrived a minute ago with no artist and no title
  is not "your file" in the sense this section is about.
- **The lyrics files it saves next to a track** — `name.lrc` and, when you
  time the words, `name.a2.lrc`. These are *new* files, written beside the
  track; the track itself is not opened. From 2.3.2, when one of them is
  already there and Rymembra is about to replace it with something different,
  the version that was there is kept once as `name.lrc.bak` (or
  `name.a2.lrc.bak`) instead of being overwritten.
- **The playlists you export**, `.m3u` and `.pls`, where you ask for them.
- **The profile archive** (`.ryprofile`), where you ask for it.
- **The application's data folder** —
  `%APPDATA%\io.github.federicobaratti.rymembra` — which holds the library
  database, the cover art cache, the installed skins and the log described
  below. On the first start after an upgrade, Rymembra also renames the data
  folder of the previous version into this one (and the files inside to the
  new names), without overwriting or deleting anything.

Nothing on that list leaves your computer, and nothing on it is a file you
already had.

### The one thing that reaches a file you already had — from 2.3.2

A track's context menu ends with **«Delete from disk»**, and it does what it
says: the file goes to the **Recycle Bin**, and the library row goes with it.
It is the only thing in Rymembra that reaches a music file you already had, so
the conditions on it are the ones you would want. It never runs by itself and
is never part of a scan, an enrichment pass or any other background work; it
asks first, with «Cancel» as the primary button; it goes to the Recycle Bin
rather than being deleted, so the operating system still holds your copy; and
when a file will not go, the row stays, because a library that claims a file
is gone while it is still on disk is worse than one that claims too much.

Its neighbour in that menu, **«Remove from library»**, does not touch the disk
at all: it takes the row out and leaves the file exactly where it is.

### Why there is no "write the tags into my files" button

It would be an easy feature to add, and it is missing on purpose.

Writing a tag changes the file's modification date. The next scan sees a file
that changed, reads it again, and takes the tags in it as the truth — so
whatever Rymembra wrote comes back in through the front door, and the library
starts depending on the contents of your files again. That loop is exactly what
2.3.1 closed: what Rymembra works out about a track now lives in its own database
table, your correction always wins over it, and neither of them can be silently
undone by a re-scan.

Interoperability does not need that button. Everything another program needs to
read is already produced as a **new file** rather than as a change to yours:
`.m3u` and `.pls` playlists, `.lrc` and `.a2.lrc` lyrics, and the profile
archive.

**One exception, and it is on its way out.** Versions up to 2.3.0 *did* rewrite
your tags during enrichment, and kept a copy of what was there before. So that
you can still undo those old writes, *Settings → Metadata* offers a button
whose label says it writes into the files. It is the only thing in Rymembra that
opens your music for writing, it only appears while there is something left to
undo, it never runs by itself, and it will be removed in a future release once
those records are gone.

### The log

Inside the application's data folder —
`%APPDATA%\io.github.federicobaratti.rymembra\diario` — Rymembra keeps three text
files with what went wrong: the thread that didn't start, the audio device that
disappeared, the error code of a command that answered badly. They serve one
thing only, namely being able to answer «it won't open» with something more than
«try reinstalling it».

**What ends up in there:** error codes (`db.openFailed`,
`playback.deviceLost`), the technical cause that goes with them, the name of the
thread that produced them, and the path of the **application's data folder** —
the one above, which on Windows contains your account name because `%APPDATA%`
runs through it. It's the only thing of yours that ends up there, and it's kept
on purpose, because it's the first line to look at when Rymembra won't open: it
says whether the database was where it should have been. Deleting it from the
file before attaching it takes nothing away from the rest.

**What doesn't end up in there:** your track titles, artist names, the paths of
your music files, what you listen to and when. This isn't a promise about the
good intentions of whoever writes the code: where a message would have wanted to
name a track there's its row number, and where it would have wanted to write a
path there's the file extension alone. A log gets sent to someone, and sending
the list of what a person listens to would not be a diagnosis.

**Nothing goes out from there.** The log **is not sent to anyone**, ever, and
there's no command that could do it: a program that can post its own logs by
itself is a program with telemetry, and there isn't any here. The only thing the
window knows how to do is open that folder in the file manager, from the button
in *Settings → Updates*. What to do with it afterwards is up to you.

The files rotate when they reach two megabytes, and three are kept: the oldest
is deleted. Deleting them all by hand breaks nothing.

---

## 9. If something changes

This document lives in the repository next to the code. If a future version
added a network request, the corresponding line would appear here in the same
commit — and `CHANGELOG.md` would say so. A program that talks to the network
without saying so in its own changelog is a program you can't trust, and that
holds even when the person writing it is me.
