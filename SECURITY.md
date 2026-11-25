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
while the app runs; the Microsoft Store copy is updated by the Store, and the
Google Play build of the Android app by Google Play.

## What counts as a vulnerability here

Rymembra is a local application with no account and no Rymembra server.
There’s no network surface listening — with three exceptions, all closed
until used and worth naming: the OAuth return port, “Listen together” while
a PC hosts, and the device link while “Reachable from the internet” is on;
on the phone, add the components other apps can reach, and on Windows the
`rymembra:` links the system hands to the program (below) — and there is no
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

On the computer the update can wait for “At close”: the package is downloaded
and its minisign signature, bound to the version, is checked at that moment —
`Update::download`, not `install` — and it is then held **in memory**, never
written to disk, until a person closes Rymembra (“Exit” from the tray, or the ×
or Alt+F4 when there is no tray icon). Only then is the installer extracted to a
temporary folder (`%TEMP%\Rymembra-<version>-updater-*`) and started, without
the restart switch, so Rymembra does not reopen. When Windows shuts down or
signs the user out the program exits on its own, and nothing is installed: an
installer launched while Windows is killing processes would stop halfway.
Installing a package that did not pass the check, or making an installer start
when nobody closed the program, is a report. The package waits as long as it
takes (the card asks again after three days), and is thrown away if the release
it belongs to is no longer the one announced. The download stops at 256 MiB —
the plugin keeps the package in memory, and the only other limit used to be
60 seconds without data, which every piece resets — and turning the check off
stops one in progress and discards a package that finishes afterwards. The
manifest, `latest.json`, is read without a ceiling on the computer, because the
plugin offers none. Just before “Install now” runs the installer a small file
with the time is left in the data folder, so that the copy it restarts opens
normally; it is read once and ignored after five minutes.

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

The phone now does both checks — the signature and the certificate — as soon as
the download ends, without handing the file over; Android’s installer opens only
when the person taps “Install”, and the checks run again right before the
handoff: the minisign signature, kept with the waiting package, is verified once
more against the file as it is on disk then — a file that changed in the cache
while it waited is deleted and never handed over — and the Kotlin side repeats
the package and certificate check on the APK it is given. The **Google Play build** is a second variant of the same app
(`-Pdistribuzione=play`) with no updater at all: its manifest removes
`REQUEST_INSTALL_PACKAGES`, the core is told the distribution
(`BuildConfig.DISTRIBUZIONE`) and refuses to check for updates, never contacting
GitHub, and the installer class refuses to run. A build that asks for that
permission, or that checks GitHub on the Play variant, is a report. The release
workflow builds only the `github` variant: the Play one is made by hand.

**The release chain.** The key that signs updates and the key that signs the
APK live in the secrets of the protected `release` environment, in three jobs
that each wait for an approval: `segreti`, right after the tag, checks that the
five secrets are there and have the right shape, that the Android keystore
opens with its password and alias, and that its certificate is the one pinned in
the repository, so that a wrong secret shows up in two minutes; `firma` and
`firma-apk` sign. Before anything is published the workflow also verifies the
minisign signature of the installer, the one inside `latest.json` and the
APK’s against the bytes (BLAKE2b-512 and Ed25519, with Node’s own crypto, as the
updater does), and not only the key id and the version: a `.sig` that is not the
file’s stops the release instead of being found out by every installed copy. A
way to publish a signature that does not belong to the file it sits beside is a
report.

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
Dropbox or Google Drive on the PC) is not read by a scan or by the analysis,
since reading it would make the sync app download it.

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
  folder — as the folders are now: one you take away stops being shared at once
  (`CONDIVISO` checks the watched folders on every question, and a device that
  mirrored it already drops those rows at its next round, about five minutes
  later); the size and date are checked around every read. No path ever
  crosses the connection, in either direction;
- **the library is shared as a catalog** — from this version a device asks the
  other for a summary (`CatalogoRiassunto`: the generation and, for each bucket
  of 500 ids, a fingerprint) and then for the buckets that changed
  (`CatalogoSecchio`), keeping the rows it receives in its own `tracks` table.
  Every question, old or new, goes through one condition, `CONDIVISO` in
  `rymembra_app::library::condivisa`: a file in a watched local or network
  folder, never a cloud folder, a catalog track or a row that came from another
  device, so a third device cannot see the first through the second. A row has
  no path: the other device asks for the file by id and Rymembra reads it only
  if it is still shared;
- **questions that did not exist are behind a capability.** A device declares
  `catalogo` (`CatalogoRiassunto`, `CatalogoSecchio`, `Valuta`) and `testi`
  (`Testo`, `TestoScrivi`) in its presence; `Nodo::chiedi` refuses to send one to
  a device that has not declared it — or whose presence it has not seen yet —
  with `lontano.protocollo`, the code an older build would answer with, and the
  connection stays up. The answering side does not trust that: the gate (linked,
  not revoked) is checked on every question, a question it does not know is
  `Sconosciuta` and is refused, and every request is capped at 64 KiB open;
- **the two writes a linked device can make** are `Valuta` (a heart or a star
  rating) and `TestoScrivi` (a correction to the lyrics: an offset, a text timed
  by hand, a candidate chosen, “not this one”). Both act only on a track the
  answering device shares, so a cloud track, a catalog track, a track of a third
  device or an id that does not exist is `lontano.fileCambiato`; both count
  against 60 writes a minute for each device (`SCRITTURE_AL_MINUTO`), after
  which the answer is `lontano.irraggiungibile` and the sender keeps the write in
  its outbox and tries again. A heart or rating is validated (stars from 0 to 5,
  a clock that is a real instant), a clock later than now is brought back to now,
  and the newest clock wins for each field — a heart and a rating keep a clock
  each. A lyrics correction is applied with the commands’ own logic and
  **only to the database, never to a file**: a linked device does not write a
  `.lrc` in the owner’s music folders. It carries the clock of when it was
  decided (`quandoMs`, optional like every field added to the protocol, which
  keeps the number `/1`; without one it counts as made now), and a correction decided before the last
  touch to a text timed by hand on the owner does not override it — the answer
  is “done”, without writing, so that it does not wait in the outbox for thirty
  days. A text a correction replaces is kept as a copy of one level (never in a
  profile, the Drive backup or sync), which the lyrics panel can bring back; a device
  can write at most 100 texts in 24 hours (beyond that the answer is
  `lontano.irraggiungibile` and the sender keeps them), because 60 writes a
  minute is some 86,000 texts a day. The limit of that
  copy is one level: two replacements of the same track by a hostile device
  exhaust it, which is declared, not fixed. The text a device sends as “timed by
  hand” is kept on the other side as a copy that follows the owner
  (`proprietario`): it is never published as the work of the one who listens,
  and “not this one” discards it. The limits of a text are 48,000 bytes, 4,000
  lines, 40,000 words, times up to 12 hours, an offset of at most an hour and no
  control characters — a text over 48,000 bytes is refused before it reaches the
  reader — and what is read from outside is capped at 256 KiB. **The lyrics
  reader itself bounds its cost, whatever it is given**: 32 distinct times and
  512 timed words per line, 16,000 rows and 200,000 words in all, 16 secondary
  lines per row, and the excess is dropped, not kept as text — a tag of
  `[0:0]` and `<0:0>x` of a few dozen kilobytes used to cost about a gigabyte,
  one of 256 KiB tens of gigabytes, and the same text reached the reader from
  LRCLIB, a `.lrc` file and a tag already in 3.2.0. Every entry point caps the
  bytes first (256 KiB for a `.lrc`, a tag, a field of LRCLIB; 48,000 for a
  correction), and a text that makes the reader take seconds or gigabytes is a
  problem. Reading lyrics never takes the library lock;
- **what arrives is validated field by field before it touches the library**
  (`rymembra_app::specchio::convalida`): at most 200,000 rows for a device,
  names of 200 characters, genre 64, codec 24, keys 300; a bucket with a bad row
  is discarded alone. The quality a device declares is compared with the file
  size and duration it also declares (`Qualita::da_riga`), because a declared
  “FLAC at 384 kHz” would otherwise beat every local copy; what the check cannot
  catch — an MP3 at 192 kbps passed off as a 44.1 kHz FLAC — is a known limit.
  A mass removal is bounded: rows leave in blocks of 2,000, 10,000 at most per
  round, so a hostile device cannot make this one hold the library lock for tens
  of seconds; at most 20,000 new rows are kept in memory to match changed ids; a
  round stops after 2,000 buckets or 10 minutes; at least 50 rows and a fifth of
  a device’s catalog need the user’s answer; a row that disappears stays hidden
  for seven days before it goes; and whether the device is still linked is
  checked **inside** the write transaction, so a bucket that arrives after
  “Forget” does not bring the tracks back;
- **a track of another device never leaves this one**: not shared onward, not in
  the profile, the Drive backup, sync, an exported playlist file or a scrobble,
  and never counted in the statistics (a restore or an account import does not
  write onto them). There are two exceptions. The text of its lyrics, when it was
  fetched, sits in `lyrics` like every text, so it goes into a profile you
  export yourself. And a playlist that contains such a track travels with its
  key — artist, title and album, normalized, in plain text — into the profile,
  the Drive backup and the sync file, because it must find the track again;
  nothing else of the row goes with it. A track whose key is longer than 300
  bytes is not shared, since a cut key could name another track. A linked
  device that declares the same MusicBrainz id as one of your albums cannot
  change that album’s title, artist or cover on your side, and enrichment never
  reads, looks up or rewrites the rows of another device;
- **“Download here” from the library** takes ids of library rows
  (`lontano_scarica_righe`) and builds the requests from the mirror, skipping
  rows the owner no longer sends; a set from 50 tracks asks the user first
  (`lontano_scarica_anteprima`: the number of tracks and the bytes it knows),
  and more than 5,000 goes in blocks. What the other device declares in its
  first answer is not taken on its word: the size must be close to the one in
  its catalog, never zero and never above 8 GiB; the disk space is reserved
  before the first byte on Windows (on Android a full disk shows up while
  writing), a file that ends early does not count as downloaded, two queues
  never write the same part (`<name>.<8 digits>.part`), and a file that
  appears meanwhile is never replaced;
- every frame is size-checked before anything is allocated (1 MiB), and so is
  what it inflates to: 16 KiB for a link request — read before its token is
  checked — 64 KiB for a request from a linked device. An answer is read in two
  steps, the kind first and then the body with the ceiling of that kind: a
  presence 16 KiB; “done”, a refusal and the head of an answer of bytes 1 KiB;
  data 8 MiB and at most 512 Ki JSON nodes — so 8 KB that open into 8 MiB cost
  about 10 MiB, not 150 to 260 — and an answer of bytes is never longer than
  the request asked for (2 MiB a read, 1 MiB a cover), which its head says
  before anything is allocated. A read asks for at most 2 MiB, a list for at
  most 500 entries; a device has at most 8 requests served at a time and at
  most four live connections, and a fifth closes the oldest. A device that sends
  more than 30 presence announcements a minute is refused, and one that sends 30
  wrong requests a minute — unreadable, or not arriving within 30 seconds — is
  ignored for a while, not disconnected; each 64 KiB piece of an answer has 30
  seconds, so a device that never reads cannot keep the seats, and a link that
  never opens its stream is closed after 10; the node writes at most 20 diary
  lines a minute and the mailbox of the window’s thread holds 512 messages, so a
  device that insists cannot wake it without end or rotate the diary away;
- the device's secret key lives in the system keychain
  (`lontano.chiave.<device>`), is never overwritten, and is never sent; the key
  of «Listen together» exchanged when linking is used only to recognize that
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
  and listens reported by a linked device are capped (120 an hour, the same
  listen again counted too), never dated in the future and never move a clock
  back;
- names are resolved only through the system's DNS servers, with no public
  fallback, and no plain-HTTP request is made.
- turning the feature off closes the node at once — no new connection is
  accepted, every open one closes, the QR code stops being valid — even while
  a command in progress still holds it; that command ends with
  `lontano.spento`. Without a QR code on screen an attempt on
  `rymembra/associa/1` is not even counted, and leaving the page that shows the
  code cancels it, and so does closing the window to the tray. The cover of a
  track of another device is asked for by its fingerprint (16 to 128 hex digits,
  checked on both sides), only from a device that answers and has a row with
  that fingerprint, three at a time for each device, and answered as a
  320-pixel JPEG — and only for the cover of a track that is shared (or of an
  album that the shared catalog lists): any other fingerprint is answered as one
  that does not exist, so a linked device cannot learn whether another image is
  in the library. The asking side takes at most 1 MiB, recognizes a JPEG, PNG or
  WebP image from its first bytes (a GIF or a BMP is refused), reads its sides
  from the header and refuses it above 1024 pixels before decoding anything,
  decodes it with 32 MiB of memory at most and two decodes at a time in the
  whole process (a decoder that falls over does not take the thread of the
  covers of «Listen together» with it), encodes it again as a JPEG of its own
  and saves it under the fingerprint the device declared — unless a cover with
  that fingerprint is already in the store: a linked device does not decide what
  is in the store of the one that linked it — and the store keeps at most
  256 MiB of such covers, after which it asks for no more until room is made. The `rymembra-cover` protocol serves only fingerprints, and every
  response says `X-Content-Type-Options: nosniff`.

Reaching a library — or a «Listen together» session — with a key that is not
linked, linking without the user's «yes» or with a used or expired token,
reading a file outside the shared folders, a frame that crashes a device or
makes it allocate without bound: all problems.

**Links that open Rymembra.** `rymembra://collega/…` is the link a QR code
carries. On Android the system camera or a message opens it in the main
activity; on Windows, from this version, the installer registers the handler in
`HKCU\Software\Classes\rymembra` — for the current user only, and removed on
uninstall only if its command points at the folder being removed — and the Store
package declares the protocol, so any web page can hand a link to the program.
That makes the link **untrusted input**: the command line is read at startup and
again when a second copy is started (the first one is told, and a second
`--avvio-automatico` does nothing), only the exact prefix is accepted, at most
1,024 characters of `A-Za-z0-9-_/=`, and whatever is smuggled after the closing
quote of `"%1"` — the usual way to inject an argument — is ignored. A link that
passes goes to the same question as on the phone, naming the device and its key
fingerprint, and nothing is linked without a “yes”; opening a link does not turn
on “Reachable from the internet” by itself. A second link that arrives while
the first waits for the answer is discarded, with a line in the diary — the
first one wins — and neither it nor a command line that is not a linking link
brings the window to the front. A link that links without that
question, an argument that gets through, or a handler that points anywhere but at
Rymembra, is a problem.

**`unsafe` and the system.** The workspace forbids `unsafe`, with two declared
exceptions that have a lint table of their own and a `// SAFETY:` comment on
each block (`undocumented_unsafe_blocks` is on): seven places in
`rymembra-barra` — the taskbar buttons (`finestra.rs`), the type of a drive
(`unita.rs`), background mode and the battery (`sottofondo.rs`), USB eject
notifications (`espulsione.rs`), the Shift key held at startup (`maiusc.rs`),
the three calls that read, write and delete a value in `HKCU` for Start with
Windows (`registro.rs`) and the never-shown native window that the media keys
hang on (`messaggi.rs`: its class, `CreateWindowExW`, `DestroyWindow` and a
procedure that hands everything to `DefWindowProcW`) — and `src/jvm.rs` in
`rymembra-android`, where the two pointers `tao` hands over become `jni`
objects. The list is in the comments of both `Cargo.toml` files and of the
workspace root; WinRT calls of the `windows` crate are safe and need none.
Start with Windows writes one value, `HKCU\…\Run\Rymembra`, and only reads and
deletes `HKCU\…\Explorer\StartupApproved\Run\Rymembra`, which is Windows’ to
write when you turn the entry off in Task Manager: it never overrides a “turned
off” that Windows recorded, and a test copy with a data folder of its own never
writes either.

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
alone. Each one now carries a capability mark (`capacita`, inside `content`),
which says only what the device that wrote it can do and is read with a cap —
16 marks of 64 bytes — because the document is another device’s; 3.2.0 ignores
it, and nothing acts on it yet. Catalog
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
skin's colors still apply. The veil of a consent dialog sits above
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

**On the phone, other apps are the other side.** The music service lets only
trusted controllers browse — the app itself, the system (notification, lock
screen, Bluetooth), an app the user allowed to read notifications (a watch’s,
a music remote), Android Auto, the car’s media center and Google Assistant,
checked by uid and, for Android Auto, the car and the Assistant, by package
and certificate. What does not go through that gate is a media key, or a “play
from search” sent to the main activity: any app can send them, as to every
Android music app, and the session cannot tell them from a headset’s — a secret
token would break the real Bluetooth and car controls. They only move what
plays (play, pause, next, previous, stop, or a search by name) and read
nothing. The cover provider serves only files of two folders,
read-only, exported with no permission because the system is the one that asks:
the cover store (`xx/<fingerprint>[.m|.t].jpg`) and a second root for the cache
`copertine-lontane` (`lontane/<64 hex>/<2–128 hex>.jpg`), each opened only if its
canonical path is still inside its root. The second root holds, today, only the
covers of a “Listen together” session, in a reserved folder of sixty-four
zeros, under a name derived from a hash of the cover’s access token — never the
token, which is a read permission and would end up in a `content://` address —
and Rust removes them when the session ends; the provider itself would open any
file of that shape, so what keeps the root narrow is what Rust writes there. The
file provider shares only two cache folders; the main activity has an empty task
affinity. The background service (`Presenza`, `connectedDevice`, with the
permission `FOREGROUND_SERVICE_CONNECTED_DEVICE`) and the service that keeps a
“Download here” going are not exported. The media-button receiver
(`RicevitoreTasti`) **is** exported, because the system sends the media key, and
for that reason it is switched off by default: `Presenza` enables it only while
“Stay active in the background” is on and disables it again when that is turned
off; it is a subclass of Media3’s receiver whose guard starts the music service
only when the core is running **and** the background service is wanted, so a
play key after a restart without the core starts nothing and cannot make Android
close the app. The power-on receiver (`AvvioAccensione`, permission
`RECEIVE_BOOT_COMPLETED`) is not exported, listens to `BOOT_COMPLETED` — after
the first unlock, never to the locked-boot broadcast — and to
`MY_PACKAGE_REPLACED`, which the system sends only to the app it has just
replaced (the update stops the process, and without it the service would stay
off until the app was opened); it is not direct-boot aware, because the
library, the settings and the Keystore live in credential-encrypted storage,
and it starts the background service only when “Stay active in the
background” is on. Starting the core from a service, with no
activity, shares the process with the window that may be born later: Android’s
`ndk-context` is set once by whichever starts first, and Kotlin — which alone knows
whether an activity was ever created — decides whether the service may set it. The
Car App Library (`androidx.car.app:app` 1.7.0, Apache-2.0,
listed in `THIRD-PARTY-NOTICES.md`) adds two components that are not exported —
a permission activity and a notification receiver — no permission, and a query
for the Android Auto connection provider; Rymembra uses it only to know whether
the phone is in a car (`CarConnection`, “Start with the car”), a question
answered by that local provider, and the answer never leaves the phone. The page
in the window gets no Android permission
of its own: a request for the camera, the microphone, the location or
anything else is always denied (`telefono::crea_la_finestra`), and the
WebView has content access, geolocation and windows opened by scripts off.
Another app browsing the library, reading the history, changing a setting or
reading a file it was not given, or the page obtaining a permission: all
problems.

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
