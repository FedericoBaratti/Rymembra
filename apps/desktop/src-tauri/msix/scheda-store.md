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

Inoltre: Esplora, per ascoltare dai cataloghi liberi senza salvare niente sul disco; testi sincronizzati da LRCLIB; copertine e informazioni da MusicBrainz; cartelle di musica su OneDrive, Google Drive, Dropbox o un server WebDAV, lette senza mai modificarle; «Ascolta insieme», la stessa musica nello stesso istante su più dispositivi della stessa rete; il collegamento via internet con un codice QR, con Rymembra per Android: la musica del telefono e degli altri tuoi PC entra nella stessa libreria, anche fuori casa; l'avvio con Windows, se lo accendi tu; scrobbling su Last.fm e ListenBrainz; backup e sincronizzazione su Google Drive (con un'app Google che registri tu) o su una cartella condivisa; skin che ridisegnano l'interfaccia, con un editor visivo.

Nessuna telemetria, nessun account da creare, nessun server di Rymembra. Di serie il programma cerca da solo copertine e informazioni sui brani in cataloghi pubblici come MusicBrainz (si spegne nelle Impostazioni); tutto il resto parte solo quando lo usi o lo accendi tu, e l'informativa sulla privacy elenca ogni richiesta che il programma può fare. È gratuito e a codice aperto (licenza MIT), e va avanti grazie alle donazioni.
```

### Descrizione breve (max 1.000)

```
Un lettore musicale per Windows per la musica che possiedi: riproduzione senza pause, volume uniforme, una libreria che si aggiorna da sola, la ricerca di ciò che manca nei cataloghi liberi (Internet Archive, Audius) e la musica dei tuoi dispositivi in una libreria sola, anche fuori casa. Nessuna telemetria, nessun account.
```

### Novità di questa versione (max 1.500)

```
Una libreria sola per i tuoi dispositivi collegati: i brani del telefono e degli altri PC compaiono in Album, Artisti, Brani, Preferiti, Cerca e nelle playlist, ognuno con l'icona del suo dispositivo. Un brano che sta su più dispositivi è una voce sola e suona la copia migliore. Se un dispositivo è spento i suoi brani restano nell'elenco, in grigio. Il filtro «Tutti · Questo dispositivo · nome» prende il posto di «Sfoglia». Mi piace, stelle, playlist e «Scarica qui» funzionano anche su quei brani, il testo si chiede al dispositivo che ha il brano e la pagina dell'artista mostra album, tutti i brani e le partecipazioni.
Impostazioni › Avvio e chiusura: «Avvia con Windows», spento finché non lo accendi, apre Rymembra ridotta a icona quando accedi. Con «La X chiude la finestra e Rymembra resta nel vassoio» la musica e i tasti multimediali continuano e la finestra si chiude davvero.
I link rymembra://collega/… aprono Rymembra e chiedono conferma prima di collegare un dispositivo. Per collegare due PC: «Copia il link» su uno, «Incolla il link» sull'altro.
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
Una libreria sola con la musica dei tuoi dispositivi collegati (telefono, altri PC), anche fuori casa, con un codice QR
Avvio con Windows e vassoio: parti all'accesso, ridotta a icona; chiudi la finestra e la musica continua (spento finché non lo accendi)
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

Also: Explore, to listen to the free catalogs without saving anything to disk; synced lyrics from LRCLIB; covers and metadata from MusicBrainz; music folders on OneDrive, Google Drive, Dropbox or a WebDAV server, read without ever changing them; “Listen together”, the same music at the same moment on several devices of the same network; the internet link with a QR code, with Rymembra for Android: the music of your phone and your other PCs joins one library, even away from home; start with Windows, if you turn it on; scrobbling to Last.fm and ListenBrainz; backup and sync to Google Drive (through a Google app you register yourself) or to a shared folder; and skins that repaint the interface, with a visual editor.

No telemetry, no account to create, no Rymembra server. Out of the box the program looks up covers and track information in public catalogs such as MusicBrainz (it can be switched off in Settings); everything else starts only when you use it or turn it on, and the privacy policy lists every request the program can make. It is free and open source (MIT license), and keeps going thanks to donations.
```

### Short description (max 1,000)

```
A music player for Windows for the music you own: gapless playback, even loudness, a library that updates itself, a search for what’s missing in the free catalogs (Internet Archive, Audius), and the music of your devices in one library, even away from home. No telemetry, no account.
```

### What's new in this version (max 1,500)

```
One library for your linked devices: the tracks of your phone and your other PCs show up in Albums, Artists, Tracks, Favorites, Search and playlists, each with the icon of its device. A track that is on several devices is one entry and plays the best copy. If a device is off, its tracks stay in the list, greyed out. The “All · This device · name” filter takes the place of “Browse”. Hearts, stars, playlists and “Download here” work on those tracks too, the lyrics are asked of the device that has the track, and the artist page shows albums, all tracks and appearances.
Settings › Startup and closing: “Start with Windows”, off until you turn it on, opens Rymembra minimized when you sign in. With “The X closes the window and Rymembra stays in the tray” on, the music and the media keys go on and the window really closes.
rymembra://collega/… links open Rymembra and ask for confirmation before linking a device. To link two PCs: “Copy link” on one, “Paste link” on the other.
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
One library with the music of your linked devices (phone, other PCs), even away from home, with a QR code
Start with Windows and the tray: starts when you sign in, minimized; close the window and the music goes on (off until you turn it on)
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
No sign-in needed: add any folder with music files in Settings > Folders and scanning, then play a track.

Network use, all listed in the privacy policy; no Rymembra server, no telemetry. On by default, each switchable in Settings: musicbrainz.org, coverartarchive.org (→ archive.org), labs.api.listenbrainz.org (metadata, covers, similarity); itunes.apple.com and api.deezer.com only if the user turns them on. On user action only: archive.org and audius.co (Explore, links), lrclib.net (lyrics), the cloud folders the user adds (Microsoft Graph, Google Drive, Dropbox, WebDAV), Drive backup and sync, scrobbling, a language model the user configures.

Two peer-to-peer features, off by default: "Listen together" (while a PC hosts, it listens on TCP/UDP 47811 on the local network, found by LAN broadcast; Windows asks about the firewall) and "Reachable from the internet" (an iroh QUIC endpoint, encrypted end to end, using n0's DNS at dns.iroh.link, relays *.relay.n0.iroh.link, the BitTorrent mainline DHT, UPnP/NAT-PMP and mDNS; it serves only devices the user linked with a QR code).

Features that need the user's own accounts show as "not configured": Drive backup and cloud folders (an app the user registers; none is built in), scrobbling, AI-assisted skin editing.

No YouTube downloads or Spotify scraping; catalog tracks are fetched only when their license allows it.

"Support" only opens GitHub Sponsors; nothing is unlocked.

runFullTrust: a Win32 desktop program (Rust, Tauri 2, WebView2) that reads and writes music folders chosen at run time, uses the audio device, Credential Manager and a notification-area icon; it cannot run in an AppContainer.

Start with Windows is a desktop:StartupTask (Enabled=false) that the user turns on in Settings, and Windows lets them turn it off. A rymembra: link handler opens the app, which asks before linking.

The in-app update check is off in this package. Needs the WebView2 Runtime (in Windows 11 and current Windows 10).
```
