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
anyone on an older version updates, and the updater says so on its own within
half an hour.

## What counts as a vulnerability here

Rymembra is a local application with no account and no Rymembra server. There's no network
surface listening — with **two** exceptions, and they're worth naming — and
there is no other user's data to expose. The things that really matter are
six, and they're the ones that can get code executed or carry off something
that isn't ours.

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
  steer the two codes to match; only one pairing runs at a time on a host, an
  attempt is counted from the moment it starts, and pairing stops after three
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
  question.

Reading a file that is not in the session queue, getting anything past the
handshake without the code, two matching codes with a machine in the middle, a
datagram or a message that crashes the host or makes it allocate without
bound, or a way to make it answer someone else's address faster than those
limits: all problems.

**Untrusted input.** A `.ryskin` skin is a zip archive somebody sends you, and
so is the Spotify GDPR export. `rymembra-skin::package` has explicit ceilings —
on the manifest, on the files, on the paths — and they exist to stop unpacking
something from writing outside its own folder or filling the disk. A path that
escapes the install folder, a zip that explodes, a manifest that sends the
compiler into recursion: all problems.

**Secrets at rest.** OAuth tokens, from 3.1 the private key that identifies
the PC to the others in «Listen together», and from 3.2 the refresh tokens of
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

The log (`PRIVACY.md` § 8) is written so as to contain nothing personal, and it
isn't sent to anyone. If something ends up in there that shouldn't, say so: it's
the same promise, and it holds even when the thing breaking it is a line written
by mistake.

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
