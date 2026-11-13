# Privacy

Last updated: 12 November 2026.

**Rymembra has no telemetry and no analytics, sends no crash reports of its
own, and sends nothing to me.** There is no Rymembra server. There is no account to create. There is
nothing I can see of what you do. (When you host a «Listen together» session,
your own PC serves the other PCs on your network that you paired with it — or,
away from home, the devices you linked — and nobody else: § 4-ter. When you
link your devices over the internet, each serves the others you linked,
through public relays that see only encrypted bytes: § 4-quinquies.)

That said, the program does talk to the network — a player that looks for cover
art and downloads music necessarily does. This document lists **every single
request** it can make, when it makes it, and what goes inside. If you find one
that isn't written here, that's a defect: report it. The browser engine the
window is drawn with is a part of Windows or of Android with data collection
of its own, which Rymembra can only partly switch off: § 6-bis says what.

---

## What stays on your computer

Everything Rymembra knows about you:

- **The library**, in an SQLite database in the application's data folder
  (`%APPDATA%\io.github.federicobaratti.rymembra` on Windows, or
  `…rymembra.store` for the Microsoft Store copy, § 6): tracks, playlists,
  ratings, favorites, listening history, play counts. Before each upgrade
  that changes the database, Rymembra also keeps a full copy of it beside
  it — `rymembra.db.schema-NNN.bak`, the last two — with everything the
  library held at that moment. If you use “Go back to the copy from before
  the update” (*Settings › Library and data*), the library you had until
  then stays beside it too, as another full copy,
  `rymembra.db.messa-da-parte.bak` (then `…messa-da-parte-2.bak` and on),
  until you delete it yourself.
- **The settings**, in the same folder.
- **The cover art**, in a file cache next to the database.
- **Your skins and Skin Studio drafts**, in the `skin` folder beside it —
  including, from 3.0, the conversations with the Studio's chat (§ 2-quater),
  and a skin you replaced by installing another with the same id, kept aside
  in `skin\sostituite` so you can put it back.
- **The secrets** — the Google Drive tokens, the client secret of the Google
  app you registered, the scrobbling credentials, the API keys of any language
  model you configure and, from 3.1, the key that identifies this PC to the
  others in «Listen together» — in the **operating system's keychain**, not in
  a file. On Windows that's Credential Manager.
- **The PCs you paired** for «Listen together», if any: their public key, the
  name they had and the last address (`ip:port`) at which they answered on
  your network, in the same database (§ 4-ter).
- **The devices you linked** over the internet, if any (§ 4-quinquies): their
  public key, the name and platform they gave, the id each uses for sync,
  their “Listen together” key, the last addresses (`ip:port`) at which they
  were reached — away from home, often their public IP address — and when
  they were linked and last seen, in the same database. A device that forgot
  you stays there, marked as unlinked, until you remove it. Next to it, the
  listens of their tracks made here and not yet delivered, and in the local
  application data folder (`%LOCALAPPDATA%` on Windows) a cache of the covers
  of the libraries you browsed, 50 MB at most.
- **The Google account of the backup**, if you connect it: its email
  address, which Google gives Rymembra with the `openid email` permission,
  shown in Settings and kept in the database until you disconnect (§ 4).
- **The cloud accounts you connected** for music folders, if any: the name
  shown — the account’s email address, when the service gives one — the list
  of the chosen folders’ files — with the folders above them
  that make up their paths, and nothing else of the drive — and the cursor of
  each service's change feed, in the same database; their tokens and passwords
  in the keychain; the tracks you played, up to the cache's ceiling, in the local
  application data folder (§ 4-quater).
- **The apps you registered** with Microsoft, Google and Dropbox, if any, so
  that Rymembra can reach your OneDrive, your Drive and your Dropbox: the name
  you gave each one and its ID, in the same database (an app's ID is not a
  secret); a Google app's client secret in the keychain (§ 4-quater).

If you come from the previous version, the first time Rymembra starts it
transfers that version's data automatically: the library, the settings, the
cover art, the skins and drafts, and the secrets it had saved in the keychain,
which are moved to the new entries rather than copied. The installer removes the
previous version but never its data. All of this happens on your computer; no
request is made for it.

None of this leaves your computer, except in the cases this document
describes one by one. On by default: the titles and artists of the tracks
and albums that metadata enrichment looks up (§ 2) — or, for similarity,
their MusicBrainz identifiers (§ 2-ter) — and, while the lyrics panel is
open, the artist, title, album and duration of the track playing and of the
next one (§ 2-bis); never your plays, ratings or files. Only if you turn
them on or ask for them: the backup and sync, through a shared folder or
your Google Drive (§ 4); your listens, to the scrobbling service you connect
(§ 3); the skin and the chat you send to a language model (§ 2-quater); a
lyric you give back to LRCLIB (§ 2-bis); what a listening session shares
with the devices you paired (§ 4-ter); the lists, covers and files that the
devices you linked browse (§ 4-quinquies);
the requests to the cloud services whose folders you add (§ 4-quater); and
the profile archive, which you export yourself, where you choose (§ 4-bis).

### When you uninstall

Uninstalling Rymembra from Windows (*Settings → Apps*) removes the program. What
else goes depends on the box «Delete the application data» («Cancella i dati
dell'applicazione») that the uninstaller shows; an update, and an uninstall run
with `/P` or `/S`, never show it and never delete data.

| What | Without the box | With the box |
|---|---|---|
| The library, settings, covers, skins and drafts, the log, the copies of the database taken before a migration and a library set aside by “Go back to the copy from before the update” (`%APPDATA%\io.github.federicobaratti.rymembra`) | stay | removed |
| The window’s own data, the cache of tracks played from cloud folders and the cache of your linked devices’ covers (`%LOCALAPPDATA%\io.github.federicobaratti.rymembra`) | stay | removed |
| The installer's registry key (`HKCU\Software\Federico Baratti\Rymembra`) | stays | removed |
| The secrets in Windows Credential Manager — account tokens, API keys, the keys that identify this PC in «Listen together» and to your devices | **stay** | **stay** |
| The previous version's window data (`%LOCALAPPDATA%\io.github.federicobaratti.aether`) | stays | stays |
| The installers downloaded by updates (`%TEMP%\Rymembra-<version>-updater-*`, about 15 MB each) | stay | stay |
| The picture Windows shows in its media controls for a track without cover art (`%TEMP%\rymembra-scheda-senza-copertina.png`, a few KB, the same for everyone) | stays | stays |
| Your music, lyrics files, exported profiles, and the backup and sync files on your Google Drive | never touched | never touched |

The uninstaller never touches Credential Manager. To take the secrets away too,
disconnect the accounts in Rymembra before uninstalling; or, afterwards, delete
in *Credential Manager → Windows Credentials* the entries whose name ends with
`io.github.federicobaratti.rymembra`. A copy from the Microsoft Store keeps its
files — its data folder is `…rymembra.store` — in its own folder under
`%LOCALAPPDATA%\Packages\`, which Windows removes with the app; a library it
copied from the installer’s copy (§ 6) stays where it was, and its secrets
in Credential Manager stay the same way.

### On an Android phone

The same things, in the places Android gives an app:

- **The library, the settings, the cover art and the skins** are in the app's
  private storage (`/data/user/0/io.github.federicobaratti.rymembra`), which
  no other app can read.
- **The secrets** are encrypted with an AES key that lives in the **Android
  Keystore** — generated by the system, never exportable, on most phones held
  by the hardware — and only the encrypted text is stored, in the app's
  private preferences. If a secret cannot be read or written, the account
  does not connect and Rymembra says so; it never falls back to keeping the
  secret in plain text or in memory.
- **Android's own backup and device-to-device transfer are turned off** for
  Rymembra (`allowBackup="false"` and exclusion rules for every storage
  area): nothing of Rymembra's goes to the Google backup of your phone, or to
  a new phone when you move to one. Your library travels only through
  Rymembra's own backup, if you turn it on (§ 4).

What the app asks Android for, and why:

| Permission | Why |
| --- | --- |
| Music and audio | Reading your tracks, and the covers inside their tags. Asked at first start, with Android’s dialog; if you put it off, the empty library and Settings › Folders and scanning offer it again. Rymembra does not ask for *Photos and videos*. |
| All files access | Reading the covers, the lyrics (`.lrc`) and the `.cue` files next to your tracks, writing an `.lrc` beside a track if you switch that on (§ 8), and deleting a track when you ask. It is not used to write the tags of your files: Rymembra writes tags only into the files it downloaded itself (§ 8). Granted in Android's settings page; Rymembra works without it, with those things left out. |
| Notifications | The notifications of a “Download here” in progress and of the wait for a service’s consent in the browser. Asked the first time one of the two starts, not at first start, and once per launch at most; without it both still work, with no notification you can see. The player’s notification does not need it: Android exempts the notifications of music sessions. |
| Foreground service (media playback) | Keeping the music playing with the screen off. Android allows it only together with the player's notification. |
| Foreground service (data sync) | Finishing a «Download here» from another linked device with the screen off (§ 4-quinquies): only while tracks are downloading, with a notification that says which one, stopped at the end of the queue. Nothing is sent anywhere for it. |
| Internet, network state | The same requests as on the computer, described in the rest of this document, except the update check, which on the phone is a different request, to GitHub’s API (§ 6). |
| Wi-Fi state, Wi-Fi multicast | «Listen together» (§ 4-ter): finding the other Rymembra instances on your network. The multicast lock is held only while the function is on. |
| Keep the device awake (Wi-Fi lock) | Keeping the Wi-Fi from dozing with the screen off during a «Listen together» session, and while a track of another linked device is playing or downloading (§ 4-quinquies). Released as soon as neither is true. |
| Camera | Reading the QR code that links a device (§ 4-quinquies). Asked the first time you press «Scan QR code», used only while the scanner is open; the frames never leave the phone. Denied, you can paste the link instead. |
| Nearby devices (local network, Android 17 and later) | «Listen together» (§ 4-ter), and servers on your own network: from Android 17 an app sees the devices on your home network only with this permission. Rymembra asks for it when you turn the function on, host, pair or join, and before connecting a WebDAV server or a language model on a private address; denied, Rymembra says so and nothing else changes. It is not used to locate you: Rymembra talks only to the addresses of your own network. |
| Install apps | Handing a downloaded update to Android's installer (§ 6). Android asks you to allow it the first time, and asks you to confirm every installation; Rymembra cannot install anything by itself. |

The player's notification shows the title, the artist, the album and the
cover of what is playing, **also on the lock screen**, where anyone holding
the phone can see it — this is how Android shows every music player. The
system decides that, not Rymembra: if you want it hidden, Android's
lock-screen notification settings hide it.

**Android Auto**, when the phone is connected to a car, browses Rymembra's
library through Android's media session: the recent tracks, the playlists,
the albums, the artists, the queue and the search results — titles, artists,
albums, durations and covers — go from Rymembra to the Android Auto app on the
same phone, which draws them on the car's screen. A search spoken in the car is
recognized by Android, not by Rymembra, which receives only the words. Nothing
of this goes through a server of Rymembra's, which has none; what the Android
Auto app does with what it shows is in Google's privacy statement.

**No other app on the phone can browse the library.** Android's media session
is open only to Rymembra itself, to the system — the notification, the lock
screen, quick settings, Bluetooth, and an app you allowed to read your
notifications, such as a watch’s or a music remote’s — to Android Auto and to
Google Assistant, each recognized by its package and by Google’s
certificate, and to the media center of a car that runs Android. Any other
app is turned away: it cannot browse the library, the
history or the playlists, see what is playing, control playback or change
favorites.

**The covers** are served to the system — the car, the lock screen, the
playback-resumption card — by a read-only content provider that any app on
the phone could ask: Android Auto and SystemUI cannot read a file in
Rymembra's private storage, and they ask without a permission of
Rymembra's. It answers only for a file in the cover cache, whose names are
hashes of the cover's bytes, and refuses any other path; the library, the
settings and the secrets are not reachable from it.

**The window** is Android System WebView, the browser engine of the phone.
That WebView can send Google usage and diagnostic data for the apps that use
it, when you agreed to share usage data with Google; Rymembra tells it not
to, with the `android.webkit.WebView.MetricsOptOut` setting of its manifest.
The page inside the window gets no Android permission of its own: a request
for the camera, the microphone or the location is always refused, and the
QR code is read by Rymembra’s own scanner, outside the page.

**Deleting a track** (§ 8) sends the file to Android's trash, after Android's
confirmation: the Files app restores it for 30 days, then Android deletes it.
Where Android has no trash — before Android 11, or a file its media index does
not list — the deletion is permanent, and the confirmation says so first.

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

Reading a link shows a preview with the cover of what it points to. Rymembra
downloads that picture from the address the catalog's answer gives — an
`archive.org` address, or for Audius an image host of its network, run by
third parties — following its redirects, and hands it to the window already
downloaded.

When the download queue fetches a track whose file was already chosen — one
imported from a link, or kept with «Keep a copy» from «Explore» — it first
reads that item's page again (`archive.org/metadata/<identifier>`), or that
Audius track (`/v1/tracks/<identifier>`), to know its license, its permission
and its page as they are today. It is one more request, carrying the
identifier and nothing of yours.

Rymembra identifies itself with the same `User-Agent` in every request of its
HTTP client — every web request in this document but the computer's update
check, which on purpose carries none naming Rymembra (§ 6; on Android the check
goes through this client, and carries it), and the device link's, which iroh
makes without one (§ 4-quinquies):
`Rymembra/<version> (+https://github.com/federicobaratti/rymembra)`, where
`<version>` is the program's, e.g. `Rymembra/3.2.0`, the same for every copy
of that version. It doesn't disguise itself as a browser and it can't: the
constructor that allowed that was removed along with the code that needed it.

---

## 2. Metadata and cover art

These start on their own, on a background thread, when the library holds tracks
with no album, no cover art, or a title that looks derived from a file name.
Automatic enrichment **can be switched off** in Settings › Folders and
scanning › Metadata. In the same place, **iTunes and Deezer each have a
switch of their own, and both are off by default** — for everyone, also
after updating from a version that used them: their terms do not clearly
cover this use, so turning them on is your call, and the note next to each
switch says why. With both off, a single track is not identified on its own,
only whole albums.

| Host | What reaches it | Rate |
| --- | --- | --- |
| `musicbrainz.org` | The title, the artist and the number of tracks of an album you're trying to identify; for a single track, its title and artist (or the title alone); then the MusicBrainz identifier of each candidate record, to read it | One request every 1.1 s |
| `coverartarchive.org`, and `archive.org` where it redirects | The MusicBrainz identifier of a record | One every 250 ms |
| `itunes.apple.com` | Only if you switch it on: the artist and title of each track being identified, for a second opinion, the genre and the cover | One every 3.1 s |
| `api.deezer.com` | Only if you switch it on: the artist and title of each track being identified, for another opinion and the cover | One every 220 ms |
| Apple’s and Deezer’s image servers (`*.mzstatic.com`, `*.dzcdn.net`) | Only with those two on: the request for the cover that iTunes or Deezer pointed to | One every 250 ms |

The cover of the service whose answer wins is downloaded from the address in
that answer, following its redirects; Rymembra checks only that it is an
`https` address. The Cover Art Archive answers every cover with a redirect to
the Internet Archive, which serves the image.

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
| `lrclib.net` | Artist, title, album and duration of the track; for a track with no artist, its title alone, in a free search (`/api/search?q=`) | One every 250 ms |

It's worth being precise about **when** it starts, because that's the part that
matters:

- only for the track you're listening to at that moment and the one queued
  after it — so that its lyrics are ready when it starts — and **only while the
  lyrics panel is open**. With that panel closed, nothing starts; the next
  track is asked for even if you then skip it or take it out of the queue;
- or when you press «Fill the library» in Settings › Folders and scanning ›
  Lyrics, which is the only sweep over the whole library and you're the one
  who launches it.

There is no background thread going through the library on its own. The
difference from the metadata in section 2 is deliberate: there, files are being
tidied; here, you'd be telling somebody else what you're listening to right now.

As with the others, LRCLIB receives no identifier of yours: no key, no cookie,
no session. And as with the others, it switches off — Settings › Folders and
scanning › Lyrics. Switched off, what remains are the lyrics you already have
in your files.

**No lyric goes up** to the Google Drive backup, nor into sync: not the ones
downloaded from the catalog, which can be downloaded again and would be
redistributed for no technical reason, and not the ones you synced or wrote
yourself, nor your offset corrections. Yours stay in the library and, if you
chose so, in an `.lrc` next to the file. To take them to another computer
there is the profile archive (§ 4-bis), which you export yourself, where you
choose.

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

**When it starts.** On the background thread, in the same passes as the
metadata, and **only for tracks that already have a MusicBrainz identifier** —
the ones enrichment has recognized, or whose tags already carried one (a
library tagged with MusicBrainz Picard, for example). It runs even when there
is nothing to enrich, and even with enrichment switched off. On a library with
no such identifier, nothing starts. Each identifier is asked about **once every three months**: the answer
is the residue of years of aggregated listening and doesn't change from one day
to the next, so asking often would be traffic spent getting the same number
back.

**It switches off** with its own switch, “Find similar tracks with
ListenBrainz”, in Settings › Folders and scanning › Metadata. Until you touch
it, it follows the switch of automatic enrichment, as it did before it had
one of its own: if you had turned enrichment off, it stays off. Switched off, the sound and your listening
remain: the queue keeps working, it knows a little less.

**Why ListenBrainz and not something else.** Because MetaBrainz's data is CC0 —
public domain, commercial use included — and because it's the same foundation
Rymembra already sends scrobbles to if you asked it to (section 3). The two stay
separate: **scrobbling goes through your account, this doesn't.** If you haven't
connected any ListenBrainz account, this feature works all the same and
completely anonymously, because there's no account for it to lean on.

---

## 2-quater. The language model in the Skin Studio

This starts **only if you configure one**, in Settings › AI models. Until you
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
- **what is selected in the Studio** — the name of a part, a token, a color or
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
- to OpenRouter only, two courtesy headers naming the application (`Rymembra`,
  and this repository's address), the same information the `User-Agent`
  already carries.

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
the list of changes each answer made. The pictures are not saved. «New
conversation» empties it. Discarding the draft does **not** delete it: so that
the discard can be undone, the conversation is moved, with the draft’s own
files, next to the snapshot taken before discarding
(`skin\istantanee\<id>\<snapshot>\chat.json`), and it goes only when that
snapshot drops out of the last twenty kept for the skin. It is **not** part of
the Google Drive backup (§ 4) nor of the profile archive (§ 4-bis): both carry
only the draft's `skin.json`.

**The key lives in the operating system's keychain**, one entry per profile
(`ia.<profile>.chiave` under the usual Rymembra service), never in the library
database. The window never receives it: the core reads it an instant before each
request and drops it afterwards.

**A whole conversation is one request per turn.** There is no background thread,
nothing periodic, and nothing that starts on its own — every request is one you
made by pressing Send. Closing the window stops a generation in progress. Each
request asks the model for at most the profile’s “Longest reply, in tokens” — 4096
unless you changed it — in a `max_tokens` field, which is a number and nothing
about you. One service wants that field under another name: when it answers a
custom profile with a 400 that asks for `max_completion_tokens` (OpenAI’s
reasoning models do), the same request is sent once more under that name, so
for such a service a turn costs two requests with the same content. A reply is
read up to 4 MiB of text, and a service that stays silent for two minutes after
it began answering is dropped.

The one other request is the list of models. Saving a profile in Settings › AI
Models, and pressing «Test and read the models» there, ask the service for the
models it offers (`GET …/models`): it is the connection test, and it fills the
model menu. It carries your API key, if the profile has one, and for
OpenRouter the two courtesy headers — nothing of the Studio, the chat or the
skin.

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
| `ws.audioscrobbler.com`, `www.last.fm` | If you connect Last.fm | Artist, title, album, album artist, track number, duration and the MusicBrainz recording identifier when the library knows them, and the instant you **started** listening. While a track plays, a «now playing» with the same fields and no time |
| `api.listenbrainz.org` | If you connect ListenBrainz | Artist, title, album, track number, duration, the MusicBrainz identifier when known, the instant you started listening, and the name and version of the program. A «playing now» while a track plays, without the time |

A listen counts when you have heard half the track or four minutes of it,
whichever comes first, and never for a track shorter than thirty seconds. The
time heard is measured while the music actually plays: a pause does not count,
and from 3.2 neither does a computer asleep with a track open. A track you
quit in the middle counts too, if you had heard enough of it. The queue of
what's still to be sent lives on your disk, it is sent when Rymembra starts and
whenever a new listen joins it, and there's a button that empties it without
sending it.

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

This starts **only if you connect it**, and it isn't on by default. It goes
through a Google app **you** register in the Google Cloud Console — Rymembra
has none of its own — and the consent you give is to that app (§ 4-quater,
«The app registrations»).

| Host | When | What reaches it |
| --- | --- | --- |
| `accounts.google.com` | On connecting | The consent screen opens in the **system browser**, never inside Rymembra's window, with your app's ID. Besides the Drive permission, Rymembra asks for `openid email`, so that Settings can show which account is connected: Google gives it the account’s email address, which stays in the library database until you disconnect |
| `oauth2.googleapis.com` | On connecting, on renewals, and on disconnecting | The token exchange; on disconnecting, the refresh token, to revoke the consent (see below) |
| `www.googleapis.com` | Thirty seconds after start, then every fifteen minutes, and two minutes after a burst of changes; on a library that did not change, a pass is one request that lists the files and their fingerprints | The backup files, described below |
| Google’s content servers (`*.googleusercontent.com`) | When a restore downloads a file and Drive redirects the download there | The request for that file’s bytes; the token does not follow the redirect |

The backup carries what a scan cannot rebuild — not the audio files, not the
cover art, not the lyrics:

- **the library file**, `rymembra-libreria.v1.json.gz`: for each track with
  something to say, its play count, last play, rating and favorite (not the
  dated history of each listen, which only the profile archive of § 4-bis
  carries); your playlists, with their names, descriptions, rules and
  tracks; the **paths of your watched folders**, as you chose them — which
  often contain your Windows user name, as in `C:\Users\<name>\Music` — each
  with the moment it was added; the list of installed skins and Studio
  drafts, each with the date it last changed; what you removed — tracks taken
  out of the library, deleted playlists, removed watched folders,
  uninstalled skins, discarded drafts — with the moment you removed it, so
  that it does not come back from another computer or from a restore; and
  this computer’s random device identifier, which says which copy wrote it.
  The skin you wear is not in it: that stays each computer’s own;
- **each installed skin**, as its package (`rymembra-skin-<id>.ryskin`, up
  to 20 MB). When two computers have different versions of the same skin,
  the more recently changed one wins, and a package goes up only when this
  computer’s version is the one that wins;
- **each Skin Studio draft**, as its `skin.json` alone
  (`rymembra-bozza-<id>.json.gz`), by the same rule: not its undo history,
  and not the conversation with the chat (§ 2-quater).

It all ends up in the application-reserved folder of **your** Drive, which
belongs to your app: no other program sees it. Up there Rymembra deletes
three things only: extra copies of the library file — two computers saving
for the first time at the same moment can create two — once they are merged
into the one just written; extra copies of the same skin or draft package;
and the package of a skin or draft you removed, once the merged list no
longer has it and its removal is recorded. A library file that cannot be
read is renamed (`….corrotto-<time>`) and kept, not overwritten. The backup
files that the previous version left there are recognized and renamed in
place the next time it runs, so nothing is uploaded twice; the rename is one
more request to Google Drive, of the same kind as the backup itself.

**A restore** shows its plan first. To build it, Rymembra looks on the disk
only for the backup’s folders on a drive letter, on this computer, or on a
network host this PC already uses for its watched folders or downloads: a
folder on any other network host is listed as not looked for, because
asking whether `\\host\share` exists means opening a connection to that
host, and it is added — and later scanned — only if you go on with the
restore. A playlist of the backup that only has the same name as one made
here enters beside it, as “Name (Drive)”, and yours is left as it is.

The refresh token lives in the operating system's keychain. Disconnecting the
account first sends it to Google's revocation address, which withdraws the
consent you gave your app, and then deletes it; your app stays registered.
When a Google Drive music folder (§ 4-quater) goes through the same app,
nothing is revoked — Google would withdraw that folder's access too — and the
token is only deleted. What has already been uploaded stays on your Drive
until you delete it yourself.

### Sync between devices

Sync, in *Settings › Sync between devices*, is off until you turn it on, and
keeps its documents in one place you choose: a shared folder, or your Drive.
Either way each device writes one file of its own,
`rymembra-<device>.v1.json.gz`, and reads the others’. It carries, for each
track, this device’s play count, the last play, the rating, the favorite
and **the position where you stopped** a track left halfway; the playlists,
with their names, descriptions, rules and tracks; the deletions; the watched
folders, with their paths; and the skins and Studio drafts by id, with a
fingerprint of each — not their contents. It does not carry the dated
history of each listen, lyrics, accounts, secrets or the devices you linked.
Rymembra looks for changes twenty seconds after it starts, then every five
minutes, and thirty seconds after a burst of changes.

**Through a shared folder** — one that Syncthing, OneDrive or Dropbox keeps
in step, for example — Rymembra makes no network request of its own: it
writes its file in a `dispositivi` subfolder, and what carries it further is
the program that syncs that folder, under its own terms.

**Through Drive**, it uses the same connection and the same app as the
backup: the file goes up to the application-reserved folder of your Drive,
and the others’ come down from there, through `www.googleapis.com`, at the
times above.

A sync document written by another PC can add watched folders here, but never
one on a network host (`\\host\…`) that this PC does not already use for its
watched folders or its downloads, so a sync document cannot make this PC open
a connection to a host of someone else's choosing.

---

## 4-bis. The profile archive

This touches no network at all. It is here because it is the one file Rymembra
writes that is *meant* to be carried somewhere else, and you should know what
is inside it before you hand it to anyone.

“Export the profile…”, in *Settings › Library and data › Rymembra profile*,
writes a `rymembra-profilo.ryprofile` where you ask for it. It is a zip; you
can open it with any archive manager and read what it
holds:

| Inside | What it is |
| --- | --- |
| `manifesto.json` | What the archive claims to be, and an explicit list of what it left out |
| `preferenze.json` | Your settings — theme, skin, equalizer, shortcuts, watched folders |
| `sincronia/rymembra-<device>.v1.json.gz` | Play counts, ratings, favorites, resume positions, playlists |
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
- **The cloud accounts you connected and the apps you registered** for them
  (§ 4-quater). The cloud folders you chose are among the watched folders and
  do travel; on the other PC they wait until you connect their account there.
  Neither the backup nor synchronization carries accounts or apps either.
- **Similarity data and the weekly picks**, which are derived and are
  recomputed from whatever library they land in.

Importing one **adds and never removes**: listens are summed, playlists that
exist only in the profile arrive as they are, covers and skins are written
only where none exist, and a lyric you synced by hand is never overwritten.
In a playlist both libraries have — the same playlist, carried from one to the
other — it only appends at the end the tracks missing here, once each and in the profile’s order: it does not remove or
reorder anything, does not bring back a track you took out of it here, and
does not change its name, description, type or rules. A different playlist
that only has the same name as one of yours enters beside it, as
“Name (.ryprofile)”. It never deletes a
playlist, switches off a watched folder, removes a like or clears a rating,
and a date from the future becomes the moment of the import. A skin the
profile would make the one you wear goes through the same review as an
installed `.ryskin` first; if it does not pass, it is installed but not
worn, and the plan says so. While it prepares the plan, Rymembra checks on the
disk only the folders the profile names on a drive letter or on a network host
this computer already uses; any other is listed as missing and proposed for
remapping without being looked up, and the scan reaches it only if you keep it
as it is. Before writing anything, Rymembra saves what your computer looks like
right now to `<data folder>\profilo\prima-<timestamp>.ryprofile`,
keeping the last three. That backup restores your settings — not the merged
history, which is additive and has no undo, and which the interface says so
rather than promising a return that does not exist.

---

## 4-ter. Listen together, on your own network

From 3.1, two or more PCs with Rymembra on the same network can listen to the
same music at the same moment: one **hosts**, the others **follow**; from this
version, devices linked with a QR code can do it away from home too («Away
from home», below). This section is everything that feature puts on the
network. It is **off by default**, and while it is off Rymembra opens no
socket for it and sends no packet: not one.

### While it is on, and you are not in a session

- **Looking for hosts.** Every ten seconds, and right away when you open its
  panel, Rymembra sends a **40-byte datagram**: a fixed tag, a format version
  and sixteen random bytes. Nothing else — no name, no title, nothing about
  you. It goes out as a broadcast on each of this PC's local networks, which
  routers do not forward, and as a single datagram to each address you typed by
  hand in *Settings › Sync between devices › Listen together*, which goes
  wherever that address is —
  and to the last address at which each paired PC answered (see «Pairing»).
- **Only a PC that is hosting answers**, with **34 bytes**: the port to connect
  to and an 8-byte fingerprint of its public key. So a hosting PC is
  recognizable on your network; its name is not. These datagrams are the only
  bytes «Listen together» sends **in the clear**.
- **A connection to each host it sees**, at most sixteen, kept open so an
  invitation can reach you. Every connection is encrypted and authenticated —
  Noise `XX`, with X25519 keys and ChaCha20-Poly1305 — and inside it the two
  PCs tell each other their **name**: the one in *Settings › Sync between
  devices › Listen together*,
  which by default is the Windows computer name. That name is said to any
  Rymembra that completes the handshake, paired or not. Nothing else is said to
  a PC that is not paired.

### Pairing

The first time two PCs meet, the one asked to pair shows a row — “A PC
calling itself “name” is asking to pair” — with “Show the code” and “No”;
nothing goes further unless someone there presses “Show the code”, within a
minute. Then both screens show the **same four-digit code**, and both people
confirm it. The code is computed from the encrypted handshake
and from random numbers each side committed to before seeing the other's, so
a third PC in the middle cannot arrange for two matching codes. After that
each PC remembers the other's **public key** and name, in the table
`insieme_pari` of its own library database, and can forget or block it at any
time (*Settings › Devices*). Three failed pairings in ten minutes from the same
PC, or ten from any, and pairing stops for a while.

Two devices linked with a QR code (§ 4-quinquies) are paired here too, without
the four-digit code: the QR link is a stronger proof, and each tells the other
its «Listen together» public key inside that encrypted connection.

When a paired PC that hosts answers on your network, this PC remembers the
**address** it answered at (`ip:port`, in the table `insieme_indirizzi`; a
device reached over the internet leaves no address there), and from then on
asks there too when it looks for hosts: it finds that PC on networks that do
not pass broadcasts. Forgetting the PC forgets its address. Whoever answers at
a remembered address must still prove the paired PC's key.

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
| To recognize a copy a member already has: the file's size, its modification date, a hash of its first and last 64 KiB, the library's content key — and its path **only** when it is a network share (`\\server\…`), never a path on a local disk | the owner of the track, to every member |

A member can read **only** the files of the tracks in the session queue, each
through a random 16-byte permission that ends with the session, when the track
leaves the queue, or when that member leaves. Your library, your history, your playlists, your
ratings and any other file stay where they are.

Tracks that belong to another PC are **not counted** in your listening
history and are **not scrobbled**; your own tracks count on your PC as they
always did, and so does an identical copy of a shared track, when the session
found one on your disk.

### Away from home

Two devices linked with a QR code (§ 4-quinquies) can listen together when
they are not on the same network. Each tells the devices it is linked to,
inside the link, whether «Listen together» is on and hosting (§ 4-quinquies,
«Between linked devices»), and a linked device that hosts shows in the panel
as «via internet». No datagram is sent to look for it and no port is opened
beyond the link's own; a device that the search on your network also finds is
reached there, at home.

Everything in the table above then travels **inside the link**: each «Listen
together» connection is one more connection between the two devices' keys
(QUIC, protocol `rymembra/insieme/1`), with the usual Noise handshake inside
it — encrypted twice. A key that is not linked is turned away before the
handshake, and a linked device must still prove its paired «Listen together»
key. A relay, when there is one, sees the two keys, the two addresses, when and
how much, and nothing else (§ 4-quinquies, «Who you talk to»).

On mobile data, joining, accepting an invitation, inviting and admitting
someone over the internet follow «Wi-Fi only» (§ 4-quinquies).

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

## 4-quater. Folders in the cloud

This starts **only if you add a folder from a service**. Nothing is sent to a
service you have not connected, and an account you connected but have chosen
no folder in is left alone too: no listing and no change check until you add
one. (Connecting, and browsing the account to pick a folder, do talk to the
service, as the table says.)

| Host | When | What reaches it |
| --- | --- | --- |
| `login.microsoftonline.com` | Connecting OneDrive, and on token renewals | The consent opens in the **system browser**; then the token exchange |
| `graph.microsoft.com` | Browsing, listing, every change check (about once a minute), every track you play | The requests for your files' names, sizes and dates, and for the bytes of the tracks |
| OneDrive's download servers (`*.1drv.com`, `*.sharepoint.com`…) | Every track you play, or whose tags are read | Range requests on the pre-authenticated address Graph gives for that file |
| `accounts.google.com`, `oauth2.googleapis.com` | Connecting Google Drive, and on renewals | As for the backup, with the `drive.readonly` scope |
| `www.googleapis.com` | Browsing, listing, change checks, tracks | As for OneDrive |
| Google's content servers (`*.googleusercontent.com`) | Every track you play, or whose tags are read, when Drive redirects the bytes there | Range requests on the address Drive redirects to; the token does not follow the redirect |
| `www.dropbox.com`, `api.dropboxapi.com` | Connecting Dropbox, renewals, browsing, listing, temporary links — two per track opened, when its version is known, the second tied to the file’s revision | As for OneDrive |
| `notify.dropboxapi.com` | Waiting for changes | Only the change cursor: no token |
| Dropbox's content servers (`*.dl.dropboxusercontent.com`) | Every track you play, or whose tags are read | Range requests on a four-hour temporary link |
| The WebDAV server you typed | Connecting, browsing, listing, change checks, tracks | Your user name and app password, in every request, over `https` only; with the track bytes, the file’s own ETag in an `If-Match` header, so that a file changed meanwhile is refused instead of served |

What these requests carry is the folder you chose, its file names, sizes and
dates, and the byte ranges of the tracks being read. **Nothing about your
library, your plays or your ratings goes to any service.** The scope is
read-only everywhere: Rymembra never writes, moves or deletes a file on a
service.

The tracks' tags are read from the service in the background, a few hundred
kilobytes per track, from the beginning and the end of each file, and never
more than 8 MiB for one. A WebDAV server that ignores `Range` cannot serve the
end of a file without sending what comes before it, so there Rymembra reads
through at most 8 MiB to reach a spot, and the tag at the end of a larger file
is skipped. That is the one piece of traffic you did not start by pressing play,
and it happens once per file (again only if the file changes).

**What is kept of the drive.** The change feeds of OneDrive, Google Drive and
Dropbox report changes anywhere on the drive. Rymembra keeps in its database
only the folders you chose, what is under them and the folders above them that
make up their paths; the other changes are dropped. Rows that an earlier
version kept from elsewhere on the drive, and those of a folder you removed
from the library, are deleted at the account’s next pass — or, for an account
that is waiting to be reconnected and so has no pass, when Rymembra starts. The tracks of a
removed folder stay in the library, but do not open until you choose the
folder again.

**Secrets.** The refresh token of each account, the WebDAV password and the
client secret of a Google app live in the operating system's keychain, never in
the database. (A secret you typed for the backup in 3.1 stays under the entry
3.1 used until you type it again; then it moves to the app's own entry and the
old one is removed.) Disconnecting an account deletes its token or password,
together with its listing and its cached tracks. For a cloud folder that is
all it does: it does not send the service a revocation, as disconnecting the
Drive backup does (§ 4), so the permission you gave your app stays valid on the
service’s side until you withdraw it there. Reconnecting with a different
account than the one being reconnected is refused, and cancelling a connection
saves neither the account nor its password.

**The cache.** The tracks you play are kept on the disk, in the local
application data folder (not the roaming one), up to a ceiling you choose
(200 MB by default). They are plain copies of your own files. The cache can be
switched off and emptied in *Settings → Folders and scanning*. When a copy is
complete, Rymembra measures its loudness and its fingerprint from it, on this
computer; a complete copy still waiting to be measured is kept until it is, so
for a while the cache can grow up to twice the ceiling. A track larger than
twice the ceiling (a whole CD image, a long hi-res file) is not cached at all:
it is streamed directly every time.

**The app registrations.** OneDrive, Google Drive and Dropbox — and the Drive
backup of § 4 — are reached through apps that **you** register: at Microsoft
Entra, in the Google Cloud Console and in Dropbox's App Console, under your
own developer account. Rymembra has no app of its own for any of them, and
nothing of the kind is compiled into it. You type each app's ID in *Settings →
Folders and scanning → Your apps*: the IDs stay in the local database, because
an app's ID is not a secret — it is in the address of every consent screen.
The one secret, a Google app's client secret, stays in the keychain. When you
connect, Microsoft, Google and Dropbox see that **your** app asked, for your
account; the registration itself — its name, its return addresses, its
permissions — lives in your account on their side. Rymembra's author sees
nothing: not your app, not its ID, not your consent.

The buttons that open a platform's console (`entra.microsoft.com`,
`console.cloud.google.com`, `www.dropbox.com/developers`) open it in the
**system browser**, and only when you press them; Rymembra itself sends
nothing there.

**MEGA** is not part of this version, and Rymembra makes no request to it.

---

## 4-quinquies. Your devices over the internet

From this version a PC and a phone — or two PCs — that you link with a QR
code can reach each other **away from home**: the phone browses and plays the
PC's music on mobile data, and the PC the phone's. There is still no
Rymembra server; this section is everything the feature puts on the network.
It is **off by default** (*Settings › Devices › Reachable from the
internet*), and while it is off Rymembra opens no socket for it and sends no
packet.

### Who you talk to

The connection is made by **iroh**, an open-source library by n0: QUIC
between two ed25519 keys, **encrypted end to end** (TLS 1.3). Each device has
its own key, created the first time the feature is turned on and kept in the
system keychain (`lontano.chiave.<device>` in Credential Manager; the Android
Keystore on the phone), never in the database. A device's **public** key is
its name on the network.

| Host | When | What reaches it |
| --- | --- | --- |
| `dns.iroh.link` (n0) | While the feature is on and the device awake: when it starts, when its addresses change, and every five minutes even if nothing changed | Your device's **public key** and its **home relay**, signed with its key (a pkarr record); and, to find a linked device, a lookup of that device's public key, over HTTPS |
| Your network's DNS servers | While the feature is on | The names of the relays and of `dns.iroh.link`, and, to find a linked device, `_iroh.<its public key>.dns.iroh.link` — **in the clear**, as every DNS question (see below) |
| The BitTorrent «mainline» DHT — a network of strangers' computers, joined through `router.bittorrent.com`, `dht.transmissionbt.com`, `dht.libtorrent.org` and `relay.pkarr.org` | The same; the record again every hour | The same record, as a second place to find each other that no single company runs. As any node of that network does, a device reachable from the internet can, after a while, start answering other nodes' requests and keeping their records |
| n0's public relays (`use1-1`, `usw1-1`, `euc1-1`, `aps1-1` `.relay.n0.iroh.link`), or the relay you typed in *Settings › Devices › Relay* | While the feature is on | A connection from your device, which tells the relay your **public key** and **IP address**, and the HTTPS probes with which a device finds the nearest relay. When two devices cannot reach each other directly — both behind a mobile operator's NAT, for instance — their traffic passes through it: the relay sees the two keys, the two addresses, when and how much, and **only encrypted bytes**. It is also how a device learns its own public address |
| Your router (UPnP, NAT-PMP, PCP) | While the feature is on | A request to open a port towards this device, so the other can reach it directly |
| Your local network (mDNS) | While the feature is on, and on Android 17 only with the «Nearby devices» permission | This device's public key and its addresses on your network, so a linked device at home finds it without going out |
| The linked device | See below | See below |

n0's relays and DNS server are a free public service, with limits and without
guarantees. The page of n0’s public relays
(`docs.iroh.computer/iroh-services/relays/public`) links no privacy
statement; it says that they cannot read the traffic they forward, that they
see the source and destination IP addresses, the connection times and the
amount of data, and that traffic is rate-limited to prevent abuse. A relay
you run yourself (`iroh-relay`) sees the same and keeps it at home. The
public key published in the DNS and in the DHT says nothing about you by
itself, but it is the same every day: whoever knows it can tell when your
device is on, and n0 sees it come back every five minutes while it is.

**Which DNS servers.** Only the system's: on Android those of the active
network, on Windows those of the network adapters. Android's Private DNS and
the DNS over HTTPS of Windows are not used, so the questions travel
unencrypted to your network's servers, and whoever runs them — your router,
your operator, a hotel's network — can see the public key of the linked
device being looked for. If those servers do not answer, nothing else is
asked: there is no fallback to Cloudflare, Google, Quad9 or any other public
resolver.

### Linking

The QR code carries a link, `rymembra://collega/…`: the public key of the
device showing it, a random 16-byte token, up to four of its direct
addresses — which can include addresses on your home network — the custom
relay, if you set one, and the **name** of the device showing it, so whoever
reads the code or receives the link reads that name too. Before asking to
link, the device that read it shows that name and a short fingerprint of the
other's key, to compare with the one shown next to the QR code. Anyone who
reads the code can ask to link, for ten minutes and once; **the device showing
it asks you to allow it**, naming the other («“Nothing Phone” wants to link»),
and nothing is linked without that yes. Neither showing a code nor opening a
link turns «Reachable from the internet» on by itself: if it is off, Rymembra
asks first. When the link is made, the two devices tell each other their
**name**, their platform, the id each uses for sync, and the public key of
«Listen together»; each writes the other in the `dispositivi` table of its
own library. «Forget» removes it there and tells the other device, which
marks it as unlinked.

On the phone, **the camera** is used only by the QR scanner, only while it is
open; the frames are read on the phone, by the ZXing library, and never
stored or sent anywhere.

### Between linked devices

Only between devices you linked, and only inside the encrypted connection; a
key that is not linked is turned away before it can ask anything.

| What | From whom, to whom |
| --- | --- |
| The device's name, its Rymembra version, and whether «Listen together» is on and hosting | each, to every linked device it can reach |
| The lists of your albums, artists and tracks — titles, artists, albums, durations, years, track numbers, and your play counts, likes and ratings — and the results of a search typed on the other device | the owner, to the device browsing it |
| The covers of those albums | the owner, to the device browsing it, which keeps them in a cache of its own (50 MB at most, `copertine-lontane`) |
| The **bytes of the file** of a track the other device plays or downloads, with its size and date | the owner, to that device |
| A listen of one of your tracks played on the other device: when, for how long | the device that played it, to you |
| A «Listen together» session, when one of the two hosts and the other joins: what § 4-ter, «In a session», lists, inside a second encryption of its own | between host and members |

**Only tracks in local and network folders** are shared: not the folders in
the cloud, not the catalog tracks, not your playlists, history, settings or
secrets, and never a file's path. The other device asks for a track by its id
in your library, and Rymembra reads it only if it is still a file under one of
your watched folders.

A track of another device that you play is **counted on its owner**, not
here: the listen waits in the `dispositivi_ascolti` table until the owner is
reachable, and the owner records it and scrobbles it as its own. A track you
**download** («Download here») becomes a file of yours, in your download
folder, and from then on counts here.

On a metered network — mobile data, a hotspot — the phone asks before playing
or downloading another device's tracks, and before a «Listen together» session
over the internet (*Settings › Devices*: Ask, Yes, Wi-Fi only). The lists and
the presence always pass.

### The phone

The phone is reachable only while Rymembra is on screen or playing, and for
two minutes after; then its connection goes to sleep, and comes back by
itself. While a session of «Listen together» or a track of another device is
playing, it holds Android's **Wi-Fi lock**, so the Wi-Fi does not doze
between packets with the screen off.

### The log

As for everything else (§ 8): error codes, counts, durations, the platform of
a device that linked — as it declared it, cut to 16 letters and digits — and
whether a request was allowed. Never a key, an address, a device name, a title
or a token.

---

## 5. The shops, when you press the button

In the «To buy» list there are three buttons — Bandcamp, Qobuz, Discogs — that
open a **search** in the system browser.

They don't start on their own: only the click starts them. They aren't affiliate
links, there's no tracking, and the address is built by the program from a
closed list of three domains written in the code — deliberately, so nothing else
can be slipped in, not even by mistake. What those sites know about you from
there on is between you, them and their own policies.

*Settings › Updates › Documents* is the same kind of thing. The license, the
third-party notices, this document, the terms and the fonts’ license are read
from the copy installed with Rymembra, with no request at all; “Open the latest
version online” and the buttons for the source code and for reporting a
problem open a GitHub page in the system browser, and only when you press
them.

---

## 6. The update check

This is **the request that goes out by itself even with an empty library and
nothing connected**, and that's why it gets its own section instead of a line at
the bottom of a list. It is not the only one that starts without a click —
metadata enrichment does too, on by default (§ 2), and so do the services you
connected yourself — but it is the one every copy installed from the installer
makes. § 7 lists them all.

| Host | When | What reaches it |
| --- | --- | --- |
| `github.com` | Ten seconds after the window is ready, then every thirty minutes; after a failure for lack of network, again after one, five and fifteen minutes | Nothing: a GET to a public file |
| The GitHub file host that `github.com` redirects to (today `release-assets.githubusercontent.com`) | At every check, because `latest.json` is a release file and GitHub serves it from there; and if you press «Update», for the installer | The same GET, following GitHub's redirect; then the request for the installer file |

**One check at every start, but not one per click.** Until 2.4 the startup check
was skipped when the last successful one was less than half an hour old, which
meant that whoever reopened Rymembra after twenty minutes never got a check at
all. From 3.0 the threshold is five minutes: it is there for a
shortcut double-clicked or a start that crashes and repeats, so that opening and
closing Rymembra ten times in a minute still makes one request, not ten. Pressing
«Check now» is the exception, and deliberately so — a button you pressed has to
do something.

The file it downloads is called `latest.json`, sits among the latest release's
assets, and is public: it's the same one you'd see opening the releases page in
a browser. Inside are a version number, the date, the headings of that
release’s changelog — at most eight, then how many more — the installer’s
address and its signature.

**Nothing of yours goes into the request.** No identifier, no serial number, no
counter, no information about your library. Not even the version you have
installed travels: the comparison is done by the program on your computer, after
reading the file. The request doesn't even carry a `User-Agent` naming Rymembra,
and that is deliberate rather than forgotten — the code says so where the request
is built. What it carries is the one of the library that makes it, Tauri's
updater, with that library's version: `tauri-plugin-updater/2.13.0` in this
release. What GitHub can infer is that somebody, from a certain IP address,
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
any of it. The same copy keeps a library of its own, in
`%APPDATA%\io.github.federicobaratti.rymembra.store` (and its cache in
`%LOCALAPPDATA%\io.github.federicobaratti.rymembra.store`), which Windows
places in the application’s private folder under `%LOCALAPPDATA%\Packages\`;
uninstalling it from Windows removes that folder. It does not open the
library of a copy installed with the installer on the same PC: two copies
opening the same database would damage it. The first time it starts, once
and only while its own library is still empty, it offers to **copy** the
library it finds in the folder without the suffix — the installer copy’s,
or its own from before this version — with its covers and skins, writing
over nothing; then it closes. The original stays where it was, untouched,
and no request is made for it. The connected accounts in the Windows
Credential Manager are the same for both copies.

**On Android it is a different request.** An app cannot replace itself on
Android, and Tauri's updater does not exist there, so the phone asks GitHub's
API for the latest release of the same project instead of `latest.json`:

| Host | When | What reaches it |
| --- | --- | --- |
| `api.github.com` | Ten seconds after the app is ready, if the last successful check is more than six hours old, then every six hours while the app stays alive; after a failure for lack of network, again after one, five and fifteen minutes | A GET to the public description of the latest release, with Rymembra's `User-Agent` |
| `github.com`, and the GitHub file host it redirects to (`release-assets.githubusercontent.com` or `objects.githubusercontent.com`; no other host is followed) | When a newer release carries an APK for the phone: its `latest.json`, for the notes. If you press «Update»: the APK’s signature (`<apk>.sig`), then the APK | GET requests for those public files, with Rymembra’s `User-Agent` |

Two things differ from the computer, and both are written here so that the
section above stays true for it. The request goes through Rymembra's HTTP
client, so it carries the `User-Agent` of every other request,
`Rymembra/<version> (+https://github.com/federicobaratti/rymembra)`: GitHub
learns which version is asking. GitHub's API refuses requests without a
`User-Agent`, and that string is a constant for the reason given in § 1. And
the APK is checked twice: against the same key the computer's updater uses,
and against the app already installed. It is offered only if it is an asset
of the project's GitHub release with its minisign signature beside it, and
downloaded over `https` following redirects only to `github.com` and the two
file hosts in the table. Before Android's installer sees it, Rymembra
verifies it against that signature, for the version announced, and then
opens it; it refuses it — deleting it, and saying so in the update banner —
unless the signature holds and it is this same app (the same package) signed
with the same certificate as the installed Rymembra, or with a key that
proves it rotated from it. A copy signed with another key, such as a test
build, does not download the APK at all, since Android would refuse it, and
the banner says so. Then
Android checks again: it installs an APK **over** Rymembra only if it is
signed with the same key as the one already installed.

The APK is downloaded into the app's cache and handed to Android's installer,
which asks you to confirm; the first time, Android also asks you to allow
Rymembra to install apps. The same switch in *Settings → Updates* turns the
check off.

---

## 6-bis. The window’s engine

Rymembra’s window is a web page drawn by the system’s own engine: Microsoft
Edge WebView2 on Windows, Android System WebView on a phone. Those engines
belong to Microsoft and Google, and collect data of their own, which
Rymembra does not receive.

**On Windows**, according to Microsoft’s page on WebView2 data and privacy:

- WebView2 collects **required** diagnostic data, “necessary to maintain
  performance and reliability”, regardless of the Windows *Diagnostic data*
  setting. Rymembra cannot switch it off.
- **Optional** diagnostic data — WebView2’s API and SDK usage, creation
  failures, browser events relevant to WebView2 — is collected only when
  optional diagnostic data is on in Windows (*Settings › Privacy & security ›
  Diagnostics & feedback*). That setting is yours.
- **If a WebView2 process crashes**, one or more minidump files are created
  and sent to Microsoft. A minidump can hold part of the page’s memory, that
  is, the titles and artists on screen. Microsoft lets an app turn this off
  only through an option of the WebView2 environment
  (`IsCustomCrashReportingEnabled`) that the libraries Rymembra is built on
  do not expose; in this version **it is not switched off**.
- **SmartScreen**, which checks the addresses a page visits with Microsoft,
  is off in Rymembra’s window (wry, the library under Tauri, starts WebView2
  with it disabled), and the window opens no address but its own.

WebView2 keeps its own data — cache, storage, crash reports — in
`%LOCALAPPDATA%\io.github.federicobaratti.rymembra\EBWebView` (§ 8).

**On Android**, Android System WebView can send Google usage and diagnostic
data for the apps that use it when you agreed to share usage data with
Google; Rymembra’s manifest tells it not to (see “On an Android phone”).

---

## 7. What *doesn't* happen, and is worth saying

- **Little at startup, and all of it listed here.** Ten seconds after the
  window is ready comes the update check of § 6, a GET to a public file — or
  not even that, if the last one is less than five minutes old. Ninety seconds
  in, metadata enrichment (§ 2), on by default, looks at the library and asks
  MusicBrainz and the cover services only about what is missing — iTunes and
  Deezer only if you switched them on — and similarity asks ListenBrainz
  (§ 2-ter); each can be switched off, and then it asks nothing. If you connected Last.fm or
  ListenBrainz, the listens left in the queue are sent (§ 3). The services you
  connected yourself — the cloud folders (§ 4-quater), the Drive backup (§ 4) —
  go on talking to their own servers; sync, if you turned it on, looks for
  changes twenty seconds in, on Drive if that is where you keep it (§ 4); and
  with «Listen together» switched on, looking for hosts on your own network
  starts with the program (§ 4-ter).
  With «Reachable from the internet» on, the device link starts too: its
  record in n0's DNS (again every five minutes) and in the DHT, a connection
  to a relay, and one to each linked device (§ 4-quinquies). There's no other
  ping, and nothing goes out on closing, except telling the PCs of a «Listen
  together» session that you left.
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
  carry no name and nothing about you. The device link does not use it either:
  its connections are QUIC with TLS 1.3 between the two devices' keys, and it
  makes no plain-HTTP request — the check for a captive portal, which its
  library would make in the clear, is switched off, and its probes of the
  relays are HTTPS. What goes in the clear is the signed record of
  § 4-quinquies — a public key and a relay address — in the DNS and the DHT;
  at home, in mDNS, the device's key and its addresses on your network; and
  the DNS questions of § 4-quinquies, to your network's DNS servers.
- **No external domain in the window.** The content policy (`tauri.conf.json`)
  names no source outside this computer: besides `'self'` and `data:`, only
  Rymembra's own local protocols — `rymembra-cover:` for the cover art cache,
  `rymembra-skin:` for a skin's images and fonts, `ipc:` for the commands —
  each also written as `http://<name>.localhost`, which is how WebView2 sees
  the same local source. The preview cover art arrives as `data:` URIs already
  downloaded by the core: the page never talks to a catalog directly, and
  cannot.
- **No cookies, no local storage** holding personal data.
- **No writing outside the folders you pointed at**, apart from the
  application's data folder and its local counterpart in `%LOCALAPPDATA%`
  (§ 8).

---

## 8. What Rymembra does *not* touch

**Your music files are not modified.** Rymembra opens them to read them — to
scan them, to decode them, to measure them — and never to write. Up to 2.3.0
that was not true, and this section used to say so; from 2.3.1 the complete
list of what Rymembra writes is the one below, and your `.flac` and `.mp3` files
are not on it.

Opening a file can still make something else write it: in a folder that the
OneDrive, Dropbox or Google Drive app on your PC keeps with its files on
demand, a file that exists only as a placeholder is downloaded by that app as
soon as anything reads it. So a scan, and the analysis, do not read those
files, only ask for their attributes: they count them (“N online-only files not downloaded”) and go on, and
the tracks come in once the files are on the PC. Playing one opens it, and so
downloads it.

What Rymembra writes, and where:

- **Files it downloaded itself**, in the download folder you chose. Rymembra
  writes the tags and the cover art into those it took from a catalog,
  because it is the one that created them: a file that arrived a minute ago
  with no artist and no title is not "your file" in the sense this section is
  about. The tracks copied from a linked device with “Download here”
  (§ 4-quinquies) are written as the other device sent them.
- **The lyrics files it saves next to a track**, only if you switch on «Also
  write synced lyrics to an .lrc next to the track» in *Settings › Folders and
  scanning › Lyrics* (from 3.0 it is off by default, and the lyrics you sync
  stay in the library) — `name.lrc` and, when you time the words,
  `name.a2.lrc`. These are *new* files, written beside the track; the track
  itself is not opened. From 2.3.2, when one of them is already there and
  Rymembra is about to replace it with something different, the version that
  was there is kept once as `name.lrc.bak` (or `name.a2.lrc.bak`) instead of
  being overwritten.
- **The playlists you export**, `.m3u`, `.m3u8`, `.pls` and `.xspf`, where you
  ask for them.
- **The profile archive** (`.ryprofile`), where you ask for it.
- **The skin packages you export** from the Skin Studio (`.ryskin`), where you
  ask for them.
- **This PC's synchronization file**, `dispositivi\rymembra-<device>.v1.json.gz`,
  in the shared folder you chose for syncing, if you chose one (§ 4); if you
  chose Drive instead, the same file goes to your Drive and nothing is
  written here.
- **The application's data folder** —
  `%APPDATA%\io.github.federicobaratti.rymembra` — which holds the library
  database with the copies taken before the last two upgrades of it
  (`rymembra.db.schema-NNN.bak`) and, if you went back to one of them, the
  library set aside (`rymembra.db.messa-da-parte.bak`), the cover art cache,
  the installed skins and the ones you replaced, and the log described
  below. On the first start after an upgrade, Rymembra also renames the data
  folder of the previous version into this one (and the files inside to the
  new names), without overwriting or deleting anything.
- **Its local counterpart** — `%LOCALAPPDATA%\io.github.federicobaratti.rymembra`,
  which does not roam with your Windows profile — where Rymembra keeps the
  cache of the cloud tracks you played (`nuvola-cache`, § 4-quater), the
  cache of the covers of your linked devices (`copertine-lontane`,
  § 4-quinquies), the window engine’s own data (`EBWebView`, § 6-bis) and,
  after a start that failed, `avvio.log` (see «The log» below).

Rymembra sends none of it anywhere by itself — a synchronization file goes
only as far as the program that syncs its folder takes it — and nothing on
that list is a file you already had.

### The one thing that reaches a file you already had — from 2.3.2

A track's context menu ends with **«Delete from disk»**, and it does what it
says: the file goes to the **Recycle Bin**, and the library row goes with it —
except where Windows has no Recycle Bin: a network share or a drive letter
mapped onto one, a removable drive (a USB stick, an SD card, a CD) or a RAM
disk, and a file whose location Rymembra could not determine. The drive is the
one the file is really on, also behind a junction or a folder where another
drive is mounted, and a drive whose type Windows cannot tell counts as one
with no Recycle Bin. There the file is deleted **permanently**, and only after
a confirmation that says so and lists the files that will go for good.
It is the only thing in Rymembra that reaches a music file you already had, so
the conditions on it are the ones you would want. It never runs by itself and
is never part of a scan, an enrichment pass or any other background work; it
asks first, with «Cancel» as the primary button; everywhere else it goes to
the Recycle Bin rather than being deleted, so the operating system still holds
your copy; and when a file will not go, the row stays, because a library that
claims a file is gone while it is still on disk is worse than one that claims
too much.

Its neighbor in that menu, **«Remove from library»**, does not touch the disk
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
`.m3u`, `.m3u8`, `.pls` and `.xspf` playlists, `.lrc` and `.a2.lrc` lyrics
when you switch them on, and the profile archive.

**The one exception is gone.** Versions up to 2.3.0 *did* rewrite your tags
during enrichment, and kept a copy of what was there before. Until 3.1, a
button in *Settings → Metadata* could write those old tags back into the files;
it was promised for a single release in 2.3.1 and was removed in 3.2.0. The
copies stay in the database, where nothing reads them, and nothing in Rymembra
opens the music files you already had for writing.

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
runs through it. Apart from what a technical cause can carry (see the cloud
folders below), it's the only thing of yours that ends up there, and it's kept
on purpose, because it's the first line to look at when Rymembra won't open: it
says whether the database was where it should have been. Deleting it from the
file before attaching it takes nothing away from the rest.

**What doesn't end up in there:** your track titles, artist names, the paths of
your music files, what you listen to and when. This isn't a promise about the
good intentions of whoever writes the code: where a message would have wanted to
name a track there's its row number, and where it would have wanted to write a
path there's the file extension alone. A log gets sent to someone, and sending
the list of what a person listens to would not be a diagnosis.

Nor the names of your audio outputs. Windows names Bluetooth headphones after
what they announce, often their owner's name, so an output is written as a
short fingerprint of its name (`uscita=[1a2b3c4d]`: the first eight
hexadecimal digits of a BLAKE3 hash, the same for the same output in every
session). Whoever knows the exact name can check it against the fingerprint;
nobody can read the name back from it. Nor the cloud folders you chose: a
chosen folder that disappeared from the service, was moved or renamed, or
could not be followed is written as the account's internal identifier
(`onedrive-k3x7q2ab`) and a count. The technical cause of a failed request is
still written, and for a WebDAV server that answered with a redirect it can
name the address it redirected to.

Text that comes from outside — the body of an error a service returns, the
platform a device declares when it links — cannot add lines of its own: line
breaks and the other control characters in it are written visibly (`\n`,
`\u{…}`), in `avvio.log` too.

**Nothing goes out from there.** The log **is not sent to anyone**, ever, and
there’s no command that could send it by itself: a program that can post its
own logs by itself is a program with telemetry, and there isn’t any here. On
the computer the window can only open that folder in the file manager, from
the button in *Settings → Updates*; on the phone the same button, “Share the
log”, hands a copy to the app you pick in Android’s share sheet, and only
when you press it. What to do with it afterwards is up to you.

The files rotate when they reach two megabytes, and three are kept: the oldest
is deleted. Deleting them all by hand breaks nothing.

**When Rymembra fails before the log opens** — WebView2 missing or broken, a
data folder that cannot be found — there is nowhere to write those lines yet,
so one more file can appear, *outside* the data folder:
`%LOCALAPPDATA%\io.github.federicobaratti.rymembra\avvio.log` (or the
temporary folder, if Windows does not say where `%LOCALAPPDATA%` is). Only the
lines of that failed start go into it — the crash and its cause, or why
Rymembra refused to start with its data folder: the previous version's data
could not be moved (with the folder involved), or the variable that chooses
the data folder, `RYMEMBRA_DATI`, names a relative one (with the value as it
was written) — under the same rules as the log: no track, no path of your
music, your account name replaced by `%APPDATA%`, `%LOCALAPPDATA%` and
`%USERPROFILE%`. It starts over
when it passes 256 kilobytes, it is not sent anywhere either, and deleting it
breaks nothing.

---

## 8-bis. This website

The project’s website — the landing page in four languages and the technical
documentation — is a set of static pages served by GitHub Pages. GitHub logs
the IP address of everyone who visits a GitHub Pages site, for security
purposes, as GitHub’s own documentation says; what it does with it is in
GitHub’s privacy statement.

The pages set no cookies, run no analytics or visitor counter, and load
nothing from other domains: the fonts, the pictures and the soundtrack come
from the site itself. In your browser’s local storage they keep two things,
which no request carries: the language you picked and the point the
soundtrack had reached. The download buttons point at the files of the
project’s GitHub release, and the other links — GitHub, the Microsoft Store,
GitHub Sponsors, an email address — are ordinary links that do nothing until
you follow them.

---

## 9. If something changes

This document lives in the repository next to the code. If a future version
added a network request, the corresponding line would appear here in the same
commit — and `CHANGELOG.md` would say so. A program that talks to the network
without saying so in its own changelog is a program you can't trust, and that
holds even when the person writing it is me.
