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

Aether is maintained by one person, in spare time. So: **a reply within a
week**, and if it doesn't arrive, push — it means the notification got lost.
There's no cash reward and there's no bug bounty program.

When the fix ships, the release names it, and your name goes next to it if you
want it there.

## Which versions get fixes

The latest published one, and nothing else. There are no maintenance branches:
anyone on an older version updates, and the updater says so on its own within
half an hour.

## What counts as a vulnerability here

Aether is a local application with no account and no server. There's no network
surface listening — with **one** exception, and it's worth naming — and there
is no other user's data to expose. The things that really matter are four, and
they're the ones that can get code executed or carry off something that isn't
ours.

**The update chain.** It's the only part of Aether that downloads an executable
and launches it. The installer is verified with minisign against the public key
compiled into the binary (`plugins.updater.pubkey` in `tauri.conf.json`), and
if the signature doesn't check out it isn't run. Any way of getting something
installed while skipping that verification — or of getting a `latest.json` that
doesn't come from us accepted — is the most important report anyone can send.

**The OAuth return port.** Google's consent screen opens in the system browser
and comes back to `127.0.0.1`, on a port open for the length of one
authorization (`aether-oauth::loopback`). It's the only thing that listens, and
it listens for a few seconds: if you can get it to accept a code that came from
someone else, or keep it open beyond that, it's a problem.

**Untrusted input.** An `.aeskin` skin is a zip archive somebody sends you, and
so is the Spotify GDPR export. `aether-skin::package` has explicit ceilings —
on the manifest, on the files, on the paths — and they exist to stop unpacking
something from writing outside its own folder or filling the disk. A path that
escapes the install folder, a zip that explodes, a manifest that sends the
compiler into recursion: all problems.

**Secrets at rest.** OAuth tokens live in the system keychain
(`aether-oauth::portachiavi`), not in the database. If they end up anywhere else
— in a log, in the database, in a temporary file — that's a problem, and it
stays one even if it's «only» in the clear on the user's own disk.

The log (`PRIVACY.md` § 8) is written so as to contain nothing personal, and it
isn't sent to anyone. If something ends up in there that shouldn't, say so: it's
the same promise, and it holds even when the thing breaking it is a line written
by mistake.

## What isn't one

- **The unsigned installer.** It's known and declared: `README.md`, the body of
  every release and the updates window all say so. An OV certificate requires
  identity validation and an annual cost, and until there is one, SmartScreen
  warns. No need to report it.
- **«Aether talks to `archive.org`».** Yes, when you ask it to. `PRIVACY.md`
  lists every request the program can make and what it puts inside. If you find
  **one that isn't on that list**, that one counts.
- **Dependencies with an open but unreachable advisory.** Useful to know, and
  they get updated anyway; but a report saying «`cargo audit` says something»
  without a path that reaches it is not a vulnerability in Aether.

## If you found something while playing around

That's perfectly fine. No preamble needed, no elegant proof of concept needed,
and no apology needed for having looked. Send me what you did and what happened,
even in three lines.
