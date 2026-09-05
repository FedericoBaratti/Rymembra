# Aether terms of use

Last updated: 19 August 2026.

Aether is a free program, distributed under the MIT license. This document is
not the contract between you and me — that's `LICENSE`, and it says the program
is given «as is». This document is about something different and more concrete:
**what you can do with the music Aether gets for you**, because those terms
aren't set by me and I can't waive them on your behalf.

It's worth reading once. It's short.

---

## 1. The program is yours, the music isn't

Aether's code is MIT: do what you like with it.

The music you download through Aether is **not covered by that license**. Every
track comes from a catalog, and carries with it the conditions that catalog
and that artist put on it. Aether records each track's license next to the track
itself, and shows it in the queue, precisely so this distinction doesn't get
lost.

The normal case is simple: what you download you can listen to, keep, put on
your phone, burn to a CD for yourself. The conditions below are about
**redistributing** and **making money**.

---

## 2. The non-commercial constraint, which applies to you too

Two of the sources Aether uses carry a *non commercial* clause, and it isn't a
formality.

### Live Music Archive (archive.org/details/etree)

The collection's terms say that access to and any further distribution of the
material must be «**strictly noncommercial**». The artists there gave permission
to record and trade their concerts on that condition, and on no other.

In practice, for what you download from there:

- you may listen to it, copy it for yourself, share it with someone without
  asking for money;
- you may **not** sell it, nor put it inside something you sell, nor use it for
  advertising, nor monetize it in any form;
- you may **not** upload it to a platform that puts advertising on top of it.

### Jamendo (once it's enabled)

The *Jamendo API Terms of Use* define «commercial use» as «any monetary
compensation, including any revenue arising from affiliation programs or
advertising». From their API you **listen**: the terms explicitly forbid caching
the content or offering offline access to it, and Aether does neither. The
Jamendo module sits behind a feature that can be switched off for exactly this
reason.

### And for Aether itself

It's the reason **Aether is and stays free**. A paid version would violate those
clauses. Donations are accepted as support for the program's development, not as
payment for the music — no donation unlocks content, no track sits behind a
payment.

---

## 3. What Aether refuses to do, and why

Some things a program like this *could* do, and that Aether does not:

- **It doesn't download from YouTube.** Their *API Developer Policies*
  § III.E.1.a forbid it; § III.I.7 forbids separating the audio from the video;
  § III.I.9 forbids playing it from a player that isn't visible. A music player
  with a library is precisely what those rules rule out.
- **It doesn't scrape Spotify.** No private endpoints, no reconstructed tokens,
  no one else's client id. The only Spotify thing it reads is the export Spotify
  hands **you** on request, which is yours by right of data portability.
- **It doesn't download what the license doesn't allow.** The check happens
  before any network request: a track marked listen-only isn't even asked for.
  If a catalog says «you listen and you don't carry it away», Aether doesn't
  test whether the server would let it.
- **It doesn't circumvent protection measures.** No DRM is touched, in any form.

If you find a way to make Aether do one of these things, that's a defect. Report
it.

---

## 3-bis. Lyrics, and what LRCLIB is

Song lyrics are protected works, like the songs. Aether takes three routes to
them, and none of the three is «we have them under license»:

1. **What's already in your files** — an `.lrc` next to the track, or the tag
   inside the file. It's yours, and it's the first source looked at.
2. **LRCLIB**, a public and free catalog fed by the people who use it. It has
   an open API, made for music players: Aether queries it the way a browser
   queries a site, with no keys, no page scraping and no circumventing anything.
3. **What you sync yourself**, which stays a file on your disk.

The part usually left unsaid, and said here: LRCLIB **is not a distributor with
sub-publishing contracts**. It's a database built by the community. Aether
breaks no rule by reading it, and that doesn't make the lyrics that come out of
it «licensed». Whoever hosts that catalog answers takedown requests from
rights holders; Aether redistributes nothing, keeps no copies elsewhere and does
not upload them into your backup.

Musixmatch, Genius, AZLyrics, LyricFind and the Chinese catalogs stay out, and
not out of laziness: either they want a license a local player doesn't have, or
they can only be read by scraping a page written for a browser. The project's CI
verifies on every change that none of those names has crept back into the code —
the same rule that keeps the downloader tools out.

### If you choose to give back

After syncing a lyric by hand you can send it to LRCLIB. It's a button, pressed
one at a time, and the fourth route — «somebody else's lyrics» — doesn't exist:
**only** what you synced yourself gets published, never what was in a file's tag
and never what has just arrived from the catalog.

It's worth saying plainly what you're doing when you press it. A song's lyric
stays the rights holder's even when you were the one who put the timings in:
what you're sharing are the **timings**, and the timings travel with the words
because separated they're no use to anyone. If you neither wrote those words nor
have the right to distribute them, sending them to a public catalog is your
decision and not one Aether takes for you — and that's the reason there's no way
for it to happen without your having decided it.

What goes to LRCLIB goes there under LRCLIB's license, not Aether's: the
catalog hosts it and answers for what it hosts. Aether keeps no copy, doesn't
resell it and doesn't put it back into circulation elsewhere. Your `.lrc` stays
on your disk, identical, whether you press that button or not.

---

## 4. What's left to buy

When no free catalog has a track, Aether doesn't retry forever and doesn't
pretend it's a fault: it puts it in «To buy», with links to Bandcamp, Qobuz and
Discogs.

I earn nothing from those links. They aren't affiliate links, there's no
tracking, and the address is built by the program from a closed list of three
domains — precisely so nothing else can be slipped in.

---

## 5. Your library is yours

Aether works on files that sit on your disk and on an SQLite database that sits
in your data folder. There's no server, there's no account, there's nothing I
can switch off.

Some operations **modify your files**: writing tags during metadata enrichment,
and the reorganization that moves files on disk. Both show a plan before acting
and both know how to go back, but a backup of the files you care about remains a
good idea — that goes for any program that writes to your data, this one
included.

---

## 6. No warranty

As `LICENSE` says: the program is provided «as is», without warranty of any
kind. I'm not answerable for lost data, damaged files or full disks.

I'm not answerable either for what you do with the music you download. I show
you each track's conditions; respecting them is your business.

---

## 7. If something here is wrong

If you represent a catalog, a label or an artist and you believe Aether is
doing something it shouldn't, write. There's no formal procedure because there's
no company: there's a person, and the answer to a well-founded report is fixing
the code.
