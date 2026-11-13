# La scheda del Microsoft Store

I testi da incollare in Partner Center, pronti. Ogni titolo dice il limite del campo,
e tutti i blocchi lo rispettano (controllato a mano al momento di scriverli: se si
modificano, ricontrollare). Il pacchetto dichiara due lingue (`it-IT` e `en-US`),
quindi la scheda va compilata in tutte e due.

I fatti qui dentro sono quelli del README e di `PRIVACY.md`: se una funzione cambia,
cambia anche il testo, perché la policy 10.1.1 del Store vuole che la scheda descriva
quel che il programma fa davvero.

## Campi che non sono testo

| Campo | Valore |
|---|---|
| Categoria | Musica |
| Prezzo | Gratuito |
| Sito web | `https://rymembra.com` |
| Informativa sulla privacy | `https://github.com/FedericoBaratti/Rymembra/blob/main/PRIVACY.md` |
| Contatto per l'assistenza | `https://github.com/FedericoBaratti/Rymembra/issues` |
| Sviluppato da | Federico Baratti |
| Requisiti | Windows 10 versione 2004 (build 19041) o successiva, 64 bit, con Microsoft Edge WebView2 Runtime (il pacchetto non lo installa: c'è in Windows 11 e nei Windows 10 aggiornati) |

---

## Italiano (it-IT)

### Descrizione (max 10.000)

```
Rymembra è un lettore musicale per Windows, pensato per chi ha già la propria musica sul computer.

Suona i file che hai sul disco (MP3, FLAC, M4A, AAC, OGG, Opus, WAV, AIFF) senza pause tra un brano e l'altro, con equalizzatore, dissolvenza incrociata e una coda che ricorda dove eri rimasto. Ogni brano suona allo stesso volume, perché Rymembra misura il volume percepito (EBU R128), e una riga accanto alla valutazione dice cos'è davvero il file che stai ascoltando: codec, frequenza, profondità di bit.

La libreria si costruisce sulle tue cartelle e le tiene d'occhio: un album copiato nella cartella della musica compare dopo pochi secondi, senza scansioni. Playlist manuali e automatiche, valutazioni, preferiti e cronologia di ascolto restano sul tuo computer. Funziona anche con la musica su un NAS o su un'unità di rete.

Quel che manca, lo cerca solo dove è consentito. Partendo da una playlist (l'archivio dati che Spotify ti consegna su richiesta, un file M3U o PLS, oppure un link) Rymembra riconosce quali brani hai già e cerca gli altri nei cataloghi liberi, Internet Archive e Audius, e li scarica soltanto quando la licenza di quel brano lo permette. Ogni brano porta con sé la licenza con cui è stato preso. Quello che nessun catalogo libero ha finisce in «Da comprare», con i link ai negozi dove l'artista viene pagato. Rymembra non scarica da YouTube e non accede a Spotify in modo non ufficiale.

Inoltre: Esplora, per ascoltare dai cataloghi liberi senza salvare niente sul disco; testi sincronizzati da LRCLIB; copertine e informazioni da MusicBrainz; cartelle di musica su OneDrive, Google Drive, Dropbox o un server WebDAV, lette senza mai modificarle; «Ascolta insieme», la stessa musica nello stesso istante su più dispositivi della stessa rete; il collegamento via internet con un codice QR, per ascoltare la musica del PC dal telefono anche fuori casa, con Rymembra per Android; scrobbling su Last.fm e ListenBrainz; backup e sincronizzazione su Google Drive (con un'app Google che registri tu) o su una cartella condivisa; skin che ridisegnano l'interfaccia, con un editor visivo.

Nessuna telemetria, nessun account da creare, nessun server di Rymembra. Di serie il programma cerca da solo copertine e informazioni sui brani in cataloghi pubblici come MusicBrainz (si spegne nelle Impostazioni); tutto il resto parte solo quando lo usi o lo accendi tu, e l'informativa sulla privacy elenca ogni richiesta che il programma può fare. È gratuito e a codice aperto (licenza MIT), e va avanti grazie alle donazioni.
```

### Descrizione breve (max 1.000)

```
Un lettore musicale per Windows per la musica che possiedi: riproduzione senza pause, volume uniforme, una libreria che si aggiorna da sola, la ricerca di ciò che manca nei cataloghi liberi (Internet Archive, Audius) e la tua musica sul telefono anche fuori casa. Nessuna telemetria, nessun account.
```

### Novità di questa versione (max 1.500)

```
Collega i tuoi dispositivi via internet: il PC mostra un codice QR, il telefono lo legge con Rymembra per Android, e da lì i due si ritrovano da soli, a casa e fuori, senza nessun server di Rymembra. Dal telefono sfogli la libreria del PC, la ascolti o ne scarichi i brani, e viceversa. È spento finché non accendi «Raggiungibile da internet» in Impostazioni › Dispositivi.
«Ascolta insieme» funziona anche fuori casa, fra i dispositivi collegati.
La finestra compare subito, anche quando la libreria va aggiornata, e dice cosa sta facendo.
I Brani si ordinano senza accenti né articoli, come Artisti e Album.
Dopo una scansione, l'analisi cede il passo agli altri programmi e, a batteria, usa un solo filo.
iTunes e Deezer sono spenti di serie, ognuno col suo interruttore.
Licenza, avvisi delle dipendenze, privacy e condizioni si leggono anche senza rete, dalle Impostazioni.
Le correzioni di una revisione completa del codice e della sua sicurezza.
```

### Funzioni (max 20 voci, 200 caratteri ciascuna)

```
Riproduzione senza pause, anche fra frequenze di campionamento diverse, con equalizzatore e dissolvenza incrociata
Tutti i brani allo stesso volume, con misura EBU R128 e supporto ai tag ReplayGain
Una riga dice cos'è davvero il file in riproduzione: codec, frequenza, profondità di bit e bitrate
Una libreria che segue le tue cartelle da sola, anche su NAS e unità di rete
Importa playlist da archivio Spotify, M3U, PLS o link e riconosce quello che hai già
Cerca ciò che manca solo nei cataloghi liberi, rispettando la licenza di ogni brano
Esplora: ascolta dai cataloghi liberi senza salvare niente sul disco
Testi sincronizzati da LRCLIB, copertine e informazioni da MusicBrainz
Scrobbling su Last.fm e ListenBrainz
Cartelle di musica su OneDrive, Google Drive, Dropbox o WebDAV, lette senza mai modificarle
«Ascolta insieme»: la stessa musica nello stesso istante su più dispositivi, in casa o via internet
La musica del PC sul telefono anche fuori casa, collegando i dispositivi con un codice QR
Backup e sincronizzazione su Google Drive o su una cartella condivisa
Skin personalizzabili, con un editor visivo
Nessuna telemetria e nessun account: niente passa da un server di Rymembra
```

### Parole chiave (max 7 termini, 40 caratteri ciascuno)

```
lettore musicale
player audio
FLAC
gapless
equalizzatore
playlist
libreria musicale
```

### Copyright e marchi (max 200)

```
© 2026 Federico Baratti. I nomi di prodotti e servizi citati sono marchi dei rispettivi proprietari: Rymembra non è affiliata a nessuno di loro.
```

### Termini di licenza applicabili (max 10.000)

```
Il codice sorgente di Rymembra è rilasciato con licenza MIT: https://github.com/FedericoBaratti/Rymembra/blob/main/LICENSE

La musica che Rymembra ottiene per te conserva la licenza della fonte da cui proviene (pubblico dominio, una variante Creative Commons, la Audius Open Music License o nessuna): i termini d'uso, compreso il vincolo non commerciale del Live Music Archive, sono qui: https://github.com/FedericoBaratti/Rymembra/blob/main/TERMS.md

I componenti di terze parti hanno le proprie licenze, elencate in THIRD-PARTY-NOTICES.md, che il programma include.
```

### Didascalie degli screenshot (facoltative)

1. `casa.png` — Home: riprendi dove eri, le raccolte della settimana e la coda
2. `album.png` — La libreria per album
3. `riproduzione.png` — In riproduzione, con il formato reale del file
4. `skin.png` — Skin personalizzabili
5. `studio.png` — Lo Studio, l'editor visivo delle skin
6. `fonti.png` — Da dove arriva la musica: solo cataloghi liberi, con le loro regole

---

## English (en-US)

### Description (max 10,000)

```
Rymembra is a music player for Windows, made for people who already have their music on their computer.

It plays the files on your disk (MP3, FLAC, M4A, AAC, OGG, Opus, WAV, AIFF) gaplessly, with an equalizer, crossfade and a queue that remembers where you stopped. Every track plays at the same loudness, because Rymembra measures perceived loudness (EBU R128), and a line next to the rating says what the file you are listening to really is: codec, sample rate, bit depth.

The library is built from your folders and keeps an eye on them: an album copied into your music folder appears a few seconds later, with no scan. Manual and automatic playlists, ratings, favorites and listening history stay on your computer. It also works with music on a NAS or a mapped network drive.

What's missing, it looks for only where that's allowed. Starting from a playlist (the data archive Spotify hands you on request, an M3U or PLS file, or a link) Rymembra recognizes which tracks you already have and looks for the rest in the free catalogs, Internet Archive and Audius, downloading a track only when its own license allows it. Every track keeps the license it was taken under. What no free catalog has goes into “To buy”, with links to the shops where the artist gets paid. Rymembra does not download from YouTube and does not access Spotify in any unofficial way.

Also: Explore, to listen to the free catalogs without saving anything to disk; synced lyrics from LRCLIB; covers and metadata from MusicBrainz; music folders on OneDrive, Google Drive, Dropbox or a WebDAV server, read without ever changing them; “Listen together”, the same music at the same moment on several devices of the same network; the internet link with a QR code, to play your PC’s music on your phone away from home, with Rymembra for Android; scrobbling to Last.fm and ListenBrainz; backup and sync to Google Drive (through a Google app you register yourself) or to a shared folder; and skins that repaint the interface, with a visual editor.

No telemetry, no account to create, no Rymembra server. Out of the box the program looks up covers and track information in public catalogs such as MusicBrainz (it can be switched off in Settings); everything else starts only when you use it or turn it on, and the privacy policy lists every request the program can make. It is free and open source (MIT license), and keeps going thanks to donations.
```

### Short description (max 1,000)

```
A music player for Windows for the music you own: gapless playback, even loudness, a library that updates itself, a search for what’s missing in the free catalogs (Internet Archive, Audius), and your music on your phone away from home. No telemetry, no account.
```

### What's new in this version (max 1,500)

```
Link your devices over the internet: the PC shows a QR code, your phone reads it with Rymembra for Android, and from then on the two find each other on their own, at home or away, with no Rymembra server. From the phone you browse the PC’s library, play it or download its tracks, and the other way round. It stays off until you turn on “Reachable from the internet” in Settings › Devices.
“Listen together” works away from home too, between linked devices.
The window appears right away, even when the library has to be updated, and says what it is doing.
Tracks sort without accents or leading articles, like Artists and Albums.
After a scan, the analysis yields to other programs and, on battery, uses a single thread.
iTunes and Deezer are off by default, each with its own switch.
The license, the dependency notices, the privacy policy and the terms can be read without a network, from Settings.
The fixes of a full review of the code and of its security.
```

### Features (max 20 items, 200 characters each)

```
Gapless playback, even across different sample rates, with an equalizer and crossfade
Every track at the same loudness, with EBU R128 measurement and ReplayGain tag support
A line tells you what the playing file really is: codec, sample rate, bit depth and bitrate
A library that follows your folders by itself, including NAS and network drives
Import playlists from a Spotify archive, M3U, PLS or a link, and recognize what you already have
Looks for what's missing only in free catalogs, respecting each track's license
Explore: listen to the free catalogs without saving anything to disk
Synced lyrics from LRCLIB, covers and metadata from MusicBrainz
Scrobbling to Last.fm and ListenBrainz
Music folders on OneDrive, Google Drive, Dropbox or WebDAV, read without ever changing them
“Listen together”: the same music at the same moment on several devices, at home or over the internet
Your PC’s music on your phone away from home, linking the devices with a QR code
Backup and sync to Google Drive or to a shared folder
Customizable skins, with a visual editor
No telemetry and no account: nothing goes through a Rymembra server
```

### Keywords (max 7 terms, 40 characters each)

```
music player
audio player
FLAC
gapless
equalizer
playlist
music library
```

### Copyright and trademark info (max 200)

```
© 2026 Federico Baratti. The product and service names mentioned are trademarks of their respective owners: Rymembra is not affiliated with any of them.
```

### Applicable license terms (max 10,000)

```
Rymembra's source code is released under the MIT License: https://github.com/FedericoBaratti/Rymembra/blob/main/LICENSE

The music Rymembra obtains for you keeps the license of the source it came from (public domain, a Creative Commons variant, the Audius Open Music License, or none): the terms of use, including the Live Music Archive's non-commercial constraint, are here: https://github.com/FedericoBaratti/Rymembra/blob/main/TERMS.md

Third-party components have their own licenses, listed in THIRD-PARTY-NOTICES.md, which the program includes.
```

### Screenshot captions (optional)

1. `casa.png` — Home: resume where you stopped, this week's collections and the queue
2. `album.png` — The library by album
3. `riproduzione.png` — Now playing, with the file's real format
4. `skin.png` — Customizable skins
5. `studio.png` — The Studio, the visual skin editor
6. `fonti.png` — Where the music comes from: free catalogs only, with their rules

---

## Note per la certificazione (max 2.000, una volta sola, in inglese)

```
No sign-in or test account needed: the app opens on the library and plays audio files from disk. To test: Settings > Folders and scanning, add any folder with music files, then play a track.

Network use, all listed in the privacy policy; no Rymembra server, no telemetry. On by default, each switchable in Settings: musicbrainz.org, coverartarchive.org (→ archive.org), labs.api.listenbrainz.org (metadata, covers, similarity); itunes.apple.com and api.deezer.com only if the user turns them on. On user action only: archive.org and audius.co (Explore, links), lrclib.net (lyrics), the cloud folders the user adds (Microsoft Graph, Google Drive, Dropbox, WebDAV), Drive backup and sync, scrobbling, a language model the user configures.

Two peer-to-peer features, off by default: "Listen together" (while a PC hosts, it listens on TCP/UDP 47811 on the local network, found by LAN broadcast; Windows asks about the firewall) and "Reachable from the internet" (an iroh QUIC endpoint, encrypted end to end, using n0's DNS at dns.iroh.link, relays *.relay.n0.iroh.link, the BitTorrent mainline DHT, UPnP/NAT-PMP and mDNS; it serves only devices the user linked with a QR code).

Features that need the user's own accounts show as "not configured": Drive backup and cloud folders (an app the user registers with Google, Microsoft or Dropbox; none is built in), scrobbling, AI-assisted skin editing.

No YouTube downloads, no Spotify scraping; Internet Archive and Audius tracks are fetched only when their license allows it.

"Support" only opens GitHub Sponsors; nothing is unlocked.

runFullTrust: a Win32 desktop program (Rust, Tauri 2, WebView2) that reads and writes music folders chosen at run time, uses the audio device and Credential Manager (optional account tokens) and a notification-area icon; it cannot run in an AppContainer.

The in-app update check is off in this package (a marker file next to the executable). Requires the WebView2 Runtime (in Windows 11 and current Windows 10).
```
