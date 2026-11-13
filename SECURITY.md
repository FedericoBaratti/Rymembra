# Reporting a vulnerability

## How

Use **GitHub's private reporting**: on the repository page, *Security* →
*Report a vulnerability*. It reaches only the people who maintain the project
and stays private until we decide together to open it.

Don't open a public issue for a vulnerability. This isn't ceremony: issues are
indexed, and the first person to read a report filed in the open wouldn't be
the person who can fix it.

If private reporting isn't enabled on the repository, open a public issue that
says only «I found a security problem, how do I send it to you?» — no details —
and we'll sort it out from there.

## What to expect

Rymembra is maintained by one person, in spare time. So: **a reply within a
week**, and if it doesn't arrive, push — it means the notification got lost.
There's no cash reward and there's no bug bounty program.

When the fix ships, the release names it, and your name goes next to it if you
want it there.

## Which versions get fixes

The latest published one, and nothing else. There are no maintenance branches:
anyone on an older version updates. Rymembra says so on its own: the
installer’s copy within half an hour while it runs, the phone within six hours
while the app runs; the Microsoft Store copy is updated by the Store.

## What counts as a vulnerability here

Rymembra is a local application with no account and no Rymembra server.
There’s no network surface listening — with three exceptions, all closed
until used and worth naming: the OAuth return port, “Listen together” while
a PC hosts, and the device link while “Reachable from the internet” is on;
on the phone, add the components other apps can reach — and there is no
other user’s data to expose. The things that really matter are below, and they’re the ones that
can get code executed or carry off something that isn’t ours.

**The update chain.** It's the only part of Rymembra that downloads an executable
and launches it. The installer is verified with minisign against the public key
compiled into the binary (`plugins.updater.pubkey` in `tauri.conf.json`), and
if the signature doesn't check out it isn't run. Any way of getting something
installed while skipping that verification — or of getting a `latest.json` that
doesn't come from us accepted — is the most important report anyone can send.
The installer itself runs with the rights of the user, asks for no administrator
and, before copying the new files, silently uninstalls the previous version of the
program (never its data); a way to make that step remove or run something it
shouldn't is a report too.

On the phone the APK goes through the same minisign key and more. Rymembra
announces an APK only if it is an asset of the project’s GitHub release with
its signature beside it (`<apk>.sig`, read first and capped at 16 KiB), and
follows redirects only to GitHub’s own hosts. Before every handoff to
Android’s installer — a copy already in the cache included — it verifies the
file on disk against that signature with the updater’s public key, and the
version in the signature must be the one announced; then it opens the APK and
refuses it unless it is the same package signed with the same certificate as
the installed Rymembra (or a key that proves it rotated from it). A file that
fails either check is deleted. Getting a file past the signature, or another
package or another signer past the certificate check, is as important a
report as the one about the installer. If
Windows refuses to start the installer (AppLocker, WDAC, an antivirus), Rymembra
says so and closes; it must never stay alive without a window.

**The OAuth return port.** The consent screens of Google, Microsoft (from 3.2,
for OneDrive) and Dropbox (from 3.2) open in the system browser and come back
to this machine, on a port open for the length of one authorization
(`rymembra-oauth::loopback`): one chosen by the system for Google and
Microsoft, one of three fixed ports (53682–53684) for Dropbox, which compares
them. From 3.2 the apps behind those screens are the user's own, and each user
registers the return addresses in them, exactly: `http://localhost` at
Microsoft, which then accepts any port; `http://127.0.0.1:53682`,
`http://127.0.0.1:53683` and `http://127.0.0.1:53684` at Dropbox; none at
Google, whose «Desktop app» clients accept any loopback port. Whatever the
address says, the socket listens only on `127.0.0.1`, and with PKCE: if you
can get it to accept a code that came from someone else, or keep it open
beyond that, it's a problem.

**Cloud folders (from 3.2).** A cloud folder is read with the account's token
or, for WebDAV, with a password sent in every request — over `https` only, and
an `http://` address is refused before anything is sent. Temporary links and
pre-authenticated download addresses are permissions in their own right: they
must not reach the log, the database or an error shown in the window. A way to
make Rymembra send a token — or a Google app's client secret — to a host that
is not the service's own (for example by following a `nextLink` or a
redirect), or to read outside the folder the user chose, is a problem.

Only the chosen folders, what is under them and the parents that make up their
paths are kept in the database; changes elsewhere on the drive are dropped, and
what an earlier version kept from elsewhere is deleted at the account’s next
pass — or at the next start, for an account waiting to be reconnected. A
connected account with no folder chosen asks the service nothing. A WebDAV
listing takes only the direct children of each folder, visits each folder once
and stops at 32 levels. Every download has a byte cap chosen by its caller,
checked against the declared length and before each block is written.
Passwords, temporary links and upload sessions do not appear in debug output
or in errors.

What is read is tied to the version the library knew. WebDAV sends the file’s
strong ETag as `If-Match`, and Dropbox compares the content hash and the
revision, so a file changed on the service while it is being cached stops with
`cloud.fileChanged` instead of being stored as a mix of two versions; a `206`
that starts at a byte other than the one asked for is refused. OneDrive and
Google Drive requests are not tied to a version yet. A server certificate the
system does not recognize is refused (`net.untrustedCertificate`): there is no
“continue anyway” and no consent by fingerprint. The cache of the tracks played
has a lock per process, on a `blocco` file in its folder: a second process on
the same data folder runs without a cache and leaves the first one’s copies
alone. A file that Windows keeps only online (a placeholder of OneDrive,
Dropbox or Google Drive on the PC) is not opened by a scan or by the analysis,
since opening it would make the sync app download it.

**«Listen together», while a PC hosts.** From 3.1 a PC that **hosts** a
listening session listens on the local network, on TCP and UDP port 47811 (or
one of the nine after it, or any free one), for as long as it hosts and not a
second more; with the feature off, or on but not hosting, nothing listens
(`rymembra-insieme`, and `PRIVACY.md` § 4-ter for what travels). What is
supposed to hold:

- every connection is Noise `XX_25519_ChaChaPoly_SHA256`, and a PC that is not
  paired can do exactly two things after the handshake: say its name and ask
  to pair. Pairing needs the same four-digit code confirmed on both screens,
  built on a commit–reveal exchange so that a machine in the middle cannot
  steer the two codes to match. A request to pair does not bring the host’s
  window forward and shows no code: it appears as a row — “A PC calling itself
  “name” is asking to pair”, the name chosen by the other side — with “Show the
  code” and “No”, for one minute. Only “Show the code” starts the exchange; “No”,
  or the minute running out, closes it before anything is sent, and the same
  key brings no new row for ten minutes. One row at a time on a host, at most
  three per origin in ten minutes, and a name equal to that of a paired PC is
  flagged, with “No” as the main button. Only one pairing runs at a time on a
  host, an attempt is counted from that click, and pairing stops after three
  failures in ten minutes from one key, ten from any. A PC that is not paired
  gets at most eight connections in all and one per address, may send nothing
  larger than 4 KiB, and is dropped after two minutes if it does not pair;
- a PC can read **only** the files of the tracks in the session queue, each
  through a random 16-byte permission, only while it is a member of the
  session, and only while the file still has the size and date it had when the
  permission was given. A member cannot take over a permission that belongs to
  the host or to another member;
- every message is size-checked before anything is allocated (1 MiB) and
  validated field by field before it is used; messages over 8 KiB travel
  deflated, and inflating one stops at the same 1 MiB without reserving more —
  a PC that is not paired cannot send a compressed message at all, so its
  4 KiB limit cannot be stretched by a decompression bomb; no connection holds
  more than 8 MiB of answers waiting to be sent; a host accepts at most 48
  connections, 6 per address and 4 handshakes at a time (2 per address, whose
  first message must arrive within a second), and answers discovery at most 4
  times a second per address and 50 in all, with an answer shorter than the
  question. A PC looking for hosts takes at most two hosts per round from the
  same address, keeps a few candidate addresses per host, and a fake answer
  arriving after the real one does not replace the address it already uses.
- whatever reaches the discovery port and is not a question is dropped and the
  next datagram is read at once: only a real socket failure pauses. Messages
  waiting for the session are capped at 256 and 8 MiB per connection; beyond
  that the connection is not read (TCP holds the sender back), nothing is
  dropped, and trust is checked again when the message gets its place.

Reading a file that is not in the session queue, getting anything past the
handshake without the code, two matching codes with a machine in the middle, a
datagram or a message that crashes the host or makes it allocate without
bound, or a way to make it answer someone else's address faster than those
limits: all problems.

**Your devices over the internet.** With *Reachable from the internet* on, a
device runs an iroh endpoint (QUIC, TLS 1.3, ed25519 keys) that anyone on the
internet can reach through its relay or its public address
(`rymembra-lontano`, and `PRIVACY.md` § 4-quinquies for what travels). With the
feature off nothing listens. What is supposed to hold:

- QUIC chooses the protocol in the handshake (ALPN), and each has its own gate.
  `rymembra/lontano/1` serves only keys written in `dispositivi` and not
  revoked: any other key is closed before it can send a byte of a request.
  `rymembra/insieme/1`, which carries «Listen together» away from home, has
  the same gate, and above it every rule of «Listen together» above still
  applies — the Noise handshake must prove the paired «Listen together» key,
  and the per-address limits count by device key instead.
  `rymembra/associa/1` is closed whenever no QR code is on screen;
- linking needs the QR code's random 16-byte token, valid for ten minutes and
  **once**, compared in constant time; a new code ends the previous one, and
  each device key gets at most ten attempts a minute while a code is on screen.
  A wrong token does not end the code: at 128 bits it cannot be guessed, and
  ending it after a few errors would let anyone on the same network expire
  every code before the right device scanned it. A right token is not enough:
  the device showing the code asks its user, naming the other, and only a
  «yes» links;
- a linked device can list only tracks in local and network folders and read
  only the file of such a track, by its id, while it is still under a watched
  folder; the size and date are checked around every read. No path ever
  crosses the connection, in either direction;
- every frame is size-checked before anything is allocated (1 MiB), and so is
  what it inflates to: 16 KiB for a link request — read before its token is
  checked — 64 KiB for a request from a linked device, 8 MiB for an answer. A
  read asks for at most 2 MiB, a list for at most 500 entries; a device has at
  most 8 requests served at a time and at most four live connections, and a
  fifth closes the oldest;
- the device's secret key lives in the system keychain
  (`lontano.chiave.<device>`), is never overwritten, and is never sent; the key
  of «Listen together» exchanged when linking is used only to recognise that
  device in a session.
- the gate is checked before **every** request, not only when a connection
  opens: forgetting a device — on either side — or removing it from the list
  closes all of that key's connections and `rymembra/insieme/1` streams, and
  the 8 requests served at a time are counted per key, across connections. A
  forget or a link changes the gate at once — even while a scan keeps the
  library busy — and no later read of the library undoes it;
- the link carries the name the other device gives itself and the screens show
  a short fingerprint of its ed25519 key (60 bits) to compare; turning on
  «Reachable from the internet» needs its own yes, separate from linking.
  Forgetting a device also removes the «Listen together» key its QR gave, and a
  QR code never unblocks a key the user blocked;
- «Download here» writes only audio files with the extensions the scan reads,
  and listens reported by a linked device are capped (120 an hour), never dated
  in the future and never move a clock back;
- names are resolved only through the system's DNS servers, with no public
  fallback, and no plain-HTTP request is made.
- turning the feature off closes the node at once — no new connection is
  accepted, every open one closes, the QR code stops being valid — even while
  a command in progress still holds it; that command ends with
  `lontano.spento`. Without a QR code on screen an attempt on
  `rymembra/associa/1` is not even counted, and leaving the page that shows the
  code cancels it, and so does sending the window to the tray. Covers of linked
  devices are cached per device, read only behind the gate, kept and shown only
  when their bytes are a JPEG, PNG or WebP image, and every `rymembra-cover`
  response says `X-Content-Type-Options: nosniff`; the window is told never to
  keep them.

Reaching a library — or a «Listen together» session — with a key that is not
linked, linking without the user's «yes» or with a used or expired token,
reading a file outside the shared folders, a frame that crashes a device or
makes it allocate without bound: all problems.

**Untrusted input.** A `.ryskin` skin is a zip archive somebody sends you, and
so is the Spotify GDPR export. `rymembra-skin::package` has explicit ceilings —
on the manifest, on the files, on the paths — and they exist to stop unpacking
something from writing outside its own folder or filling the disk. A path that
escapes the install folder, a zip that explodes, a manifest that sends the
compiler into recursion: all problems.

The ceilings are on what is really inflated, not only on what an archive
declares: a skin with more than 64 entries or zip64 is refused before it is
opened, its images are capped at 4096 × 4096 pixels read from their header, a
profile entry is read at most up to its declared size within a budget for the
whole archive, and the JSON of the Spotify archive and of sync documents is
value-counted before it is parsed. The “previous database” you import, which
can be any SQLite file someone sent you, is opened read-only as `immutable` —
its `-wal` and `-shm` are never read — with `trusted_schema=OFF` and
`query_only`, and the tables read from it must be real tables, not views or
virtual tables. Windows device names (`con`, `nul`…) are
refused as ids and file names. A profile only **adds** to the library: it
never deletes, switches off or clears anything — in a playlist both libraries
have it only appends the tracks missing here, never removing, reordering or
turning it into a smart one, and a different playlist with the same name
enters beside yours — a skin it would make the one worn passes the
same review as an installed `.ryskin` first, and a cover hash that is not
lowercase hex never becomes a path. Sync documents are validated value by
value and cannot add folders on network hosts this PC does not already use,
however the path is written (`\\host`, `//host`, mixed separators,
`\??\UNC\host`); the plan of a Drive restore does not even look for such
folders, it lists them as not checked. A sync device waiting to be accepted
is trusted only through “Accept”: renaming it does not trust it. Sync
documents are not signed yet: the sync folder must be writable by you
alone. Catalog
downloads start only from the catalog's own addresses, follow redirects only
inside it, and must be audio. Error messages of AI services are cleaned of the
API key, and what a service sends back is capped: 4 MiB of text and 64 MiB read
from the network for a reply, two minutes of silence once it has begun, and a
line of more than 1 MiB without a line break is refused as not being a stream
at all.

**A skin cannot redraw a consent dialog.** The dialogs that ask for a yes are
marked `data-consenso` — linking a device, the «Listen together» code, invites
and join requests, the firewall question, deleting from disk, reviewing or
replacing a skin, and from this version also the tracks a scan would remove,
a watched folder that is already there under another name, the plan of a
Drive restore and the plan of a profile import. The skin compiler emits
nothing that reaches them (no layers, images, pseudo-elements, part styling or
fonts; view transitions pause while one is on screen), and the engine's own
stylesheet sets the system font in an `@layer` a skin cannot override; the
skin's colours still apply. The veil of a consent dialog sits above
notifications and menus, which a skin redraws. Questions that arrive on their
own — a link or «Listen together» request — open above the Skin Studio too.
A skin that makes a consent dialog say something else than what it asks is a
problem. One limit is known: a skin can still apply `filter`, `opacity` or a
`clip` to a part that contains a dialog, and that reaches the dialog too.

**Secrets at rest.** OAuth tokens, from 3.1 the private key that identifies
the PC to the others in «Listen together», the private key of the device link,
and from 3.2 the refresh tokens of
the cloud accounts, the WebDAV passwords and the client secret of each Google
app the user registers (`nuvola.app.<app>.segreto`; one typed in 3.1 stays at
`google.client_secret` until it is typed again), live in the system keychain
(`rymembra-oauth::portachiavi`), not in the database. The IDs of the user's
apps are in the database (`nuvola_app`), and are not secrets: an app's ID is
in the address of every consent screen it opens. Nothing is compiled into the
executable: from 3.2 Rymembra has no OAuth client of its own for any service,
so there is no shared ID or secret to read out of it. The cache of cloud
tracks holds plain copies of files the user played; it is local, capped, and
can be emptied. If a secret ends up anywhere else
— in a log, in the database, in a temporary file — that's a problem, and it
stays one even if it's «only» in the clear on the user's own disk. The entries
saved by the previous version are moved to the current ones the first time they
are read, and the old one is removed only after the copy succeeded.
Disconnecting a cloud folder’s account deletes its secrets from the keychain
and does not send the service a revocation (disconnecting the Drive backup
does, unless a Drive folder goes through the same app): the token it forgets
stays valid on the service’s side until the user withdraws the app’s access
there.

The log (`PRIVACY.md` § 8) is written so as to contain nothing personal, and it
isn't sent to anyone. If something ends up in there that shouldn't, say so: it's
the same promise, and it holds even when the thing breaking it is a line written
by mistake.

Audio outputs appear in the log only as a short fingerprint of their name, and
control characters coming from outside (a service's error, a device's declared
platform) are written visibly, so nobody can add a line to the log. Links handed
to the system browser are always `https` pages with a host and no credentials.

**The window shows only Rymembra’s own page**, on Windows and on Android.
Every navigation to any other address — a web page, `about:blank`, `data:`,
`file:`, `javascript:`, one of Rymembra’s own local schemes, a window opened
by a script — is refused, opens nothing, and leaves a line in the log
(`src-tauri/src/pagina.rs`). On the computer the browser’s own context menu is
off outside text fields, and so are its keys for reloading, printing, saving,
viewing the source and going back and forward. A way to load another page in
the window, or to keep one there, is a problem.

**The documents the window reads are a closed list of five.** *Settings ›
Updates › Documents* shows the license, the third-party notices, the privacy
policy, the terms and the fonts’ license from the copy installed with the app
(`documento_locale`, `src-tauri/src/comandi.rs`): the window sends one of those
names, never a path, and the core reads the file of that name next to the
executable — UTF-8 text, at most 8 MiB, shown as plain text. On the phone
Kotlin reads them from `assets/legale/` of the APK (`Ponte`, `legale.leggi`)
and accepts the same five names only. The buttons that open the same documents
on the web go through a closed list of their own (`apri_documento`), whose
addresses are built in Rust from the repository’s. A way to read any other file
through either is a problem.

**On the phone, other apps are the other side.** The music service accepts
only trusted controllers — the app itself, the system (notification, lock
screen, Bluetooth), an app the user allowed to read notifications (a watch’s,
a music remote), Android Auto, the car’s media centre and Google Assistant,
checked by uid and, for Android Auto, the car and the Assistant, by package
and certificate; the cover provider serves only files of the cover folder,
read-only; the file provider shares only two cache folders; the main activity
has an empty task affinity. The page in the window gets no Android permission
of its own: a request for the camera, the microphone, the location or
anything else is always denied (`telefono::crea_la_finestra`), and the
WebView has content access, geolocation and windows opened by scripts off.
Another app browsing the library, controlling playback or reading a file it
was not given, or the page obtaining a permission: all problems.

## What isn't one

- **The unsigned installer.** It's known and declared: `README.md`, the body of
  every release and the updates window all say so. An OV certificate requires
  identity validation and an annual cost, and until there is one, SmartScreen
  warns. No need to report it.
- **«Rymembra talks to `archive.org`».** Yes, when you ask it to. `PRIVACY.md`
  lists every request the program can make and what it puts inside. If you find
  **one that isn't on that list**, that one counts.
- **Dependencies with an open but unreachable advisory.** Useful to know, and
  they get updated anyway; but a report saying «`cargo audit` says something»
  without a path that reaches it is not a vulnerability in Rymembra.

## If you found something while playing around

That's perfectly fine. No preamble needed, no elegant proof of concept needed,
and no apology needed for having looked. Send me what you did and what happened,
even in three lines.
