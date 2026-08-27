# Privacy

Ultimo aggiornamento: 20 agosto 2026.

**Aether non ha telemetria, non ha analytics, non ha crash reporting, e non
manda niente a me.** Non esiste un server di Aether. Non c'è nessun account da
creare. Non c'è niente che io possa vedere di quel che fai.

Detto questo, il programma parla con la rete — un lettore che cerca copertine e
scarica musica per forza lo fa. Questo documento elenca **ogni singola
richiesta** che può partire, quando parte, e cosa ci va dentro. Se ne trovi una
che non è scritta qui, è un difetto: segnalalo.

---

## Cosa resta sul tuo computer

Tutto ciò che Aether sa di te:

- **La libreria**, in un database SQLite nella cartella dati
  dell'applicazione (`%APPDATA%\io.github.federicobaratti.aether` su Windows): brani,
  playlist, valutazioni, preferiti, cronologia d'ascolto, conteggi di
  riproduzione.
- **Le impostazioni**, nella stessa cartella.
- **Le copertine**, in una cache di file accanto al database.
- **I segreti** — i token di Google Drive e le credenziali di scrobbling —
  nel **portachiavi del sistema operativo**, non in un file. Su Windows è il
  Credential Manager.

Niente di tutto questo lascia il tuo computer, tranne per il backup se lo
attivi tu (§ 4).

---

## 1. I cataloghi musicali

Partono quando **tu** chiedi qualcosa: incolli un link, o confermi
un'importazione che mette dei brani in coda.

| Host | Quando | Cosa gli arriva |
| --- | --- | --- |
| `archive.org` | Ricerca di un brano, lettura di un item, prelievo di un file | Il titolo e l'artista che stai cercando, o l'identificativo dell'item; il tuo indirizzo IP |
| `www.jamendo.com`, `prod-1.storage.jamendo.com` | *(Fase 2, non ancora attivo)* Ricerca e riproduzione | Come sopra, più il `client_id` che avrai configurato tu |
| `audius.co`, `api.audius.co` | *(Fase 2, non ancora attivo)* Ricerca e riproduzione | Come sopra |

Un link che non appartiene a uno di questi tre domini **non viene tentato**: il
programma rifiuta prima di aprire qualunque connessione. Non è prudenza
generica, è la regola che tiene Aether dentro i termini di chi gli dà la musica.

Aether si presenta sempre con lo stesso `User-Agent`:
`Aether/0.1 (+https://github.com/federicobaratti/aether)`. Non si traveste da
browser e non può farlo: il costruttore che lo permetteva è stato tolto insieme
al codice che ne aveva bisogno.

---

## 2. I metadati e le copertine

Partono da sole, su un filo di sottofondo, quando in libreria ci sono brani
senza album, senza copertina o con un titolo che sembra ricavato da un nome di
file. L'arricchimento automatico **si può spegnere** in Impostazioni › Metadati.

| Host | Cosa gli arriva | Ritmo |
| --- | --- | --- |
| `musicbrainz.org` | Artista, album e durate dei brani di un disco che stai cercando di identificare | Una richiesta ogni 1,1 s |
| `coverartarchive.org` | L'identificativo MusicBrainz di un disco | Una ogni 250 ms |
| `itunes.apple.com`, `is1.mzstatic.com` | Artista e titolo, per la copertina | Una ogni 350 ms |
| `api.deezer.com` | Artista e titolo, per la copertina | Una ogni 220 ms |

Nessuno di questi riceve un identificativo tuo: non c'è una chiave API, non c'è
un cookie, non c'è una sessione. Vedono un indirizzo IP e una stringa di
ricerca, come vedrebbero un motore di ricerca.

I ritmi non sono cortesia astratta: MusicBrainz chiede esplicitamente una
richiesta al secondo, e superarla significa farsi bloccare — per sé e per gli
altri che usano lo stesso programma.

---

## 2-bis. I testi delle canzoni

Aether guarda **prima** quel che hai già: un file `.lrc` accanto al brano, o il
testo dentro il tag del file. Quello non esce da casa tua e non passa da
nessuna rete.

Quando non c'è né l'uno né l'altro, lo chiede a LRCLIB.

| Host | Cosa gli arriva | Ritmo |
| --- | --- | --- |
| `lrclib.net` | Artista, titolo, album e durata del brano | Una ogni 250 ms |

Vale la pena essere precisi su **quando** parte, perché è la parte che conta:

- solo per il brano che stai ascoltando in quel momento, e **solo mentre il
  pannello del testo è aperto**. Chiuso quel pannello, non parte niente;
- oppure quando premi «Riempi la libreria» in Impostazioni › Cartelle, che è
  l'unica passata sull'intera libreria e la lanci tu.

Non c'è nessun filo di sottofondo che scorre la libreria da solo. La differenza
rispetto ai metadati della sezione 2 è deliberata: là si sistemano dei file,
qui si direbbe a qualcun altro cosa stai ascoltando adesso.

Come per gli altri, LRCLIB non riceve nessun identificativo tuo: nessuna chiave,
nessun cookie, nessuna sessione. E come per gli altri, si spegne — Impostazioni
› Cartelle › Testi. Spento, restano i testi che hai già nei file.

Nel backup su Google Drive **non sale nessun testo scaricato**: quel che viene
dal catalogo si ri-scarica, e ricaricarlo altrove sarebbe ridistribuirlo senza
nessun motivo tecnico. Salgono soltanto i `.lrc` che hai scritto tu e le
correzioni di scarto.

### Quando sei tu a mandare qualcosa

C'è un solo punto in tutta l'app in cui del contenuto esce verso un servizio
pubblico, ed è il pulsante **«Restituisci a LRCLIB»** che compare dopo aver
sincronizzato un testo a mano. Non c'è nessun altro modo di arrivarci: non è
automatico, non è in blocco, non è attaccato al salvataggio.

Cosa parte, esattamente:

| Cosa | Perché |
| --- | --- |
| Artista, titolo, album e durata del brano | Sono le chiavi con cui il catalogo lo ritrova |
| Le righe con i loro tempi | È il contributo |
| Le stesse righe senza i tempi | Il catalogo li tiene in due forme; il testo senza tempi è ricavato da quello con i tempi, mai da altro |

E cosa **non** parte: nessun percorso di file, nessun identificativo del
dispositivo, nessun account, nessun cookie. LRCLIB non chiede un account e
Aether non ne ha uno da dargli. Quel che arriva a chi ospita il catalogo è
l'indirizzo IP da cui arriva la richiesta, come per qualunque sito.

Due cose che vengono rifiutate prima ancora di partire, e che vale la pena
sapere:

- **il testo che stava dentro il tag di un file non si pubblica mai.** Non
  l'hai scritto tu, e mandarlo firmandolo sarebbe girare il lavoro di qualcun
  altro;
- **quel che è arrivato dal catalogo non ci torna.** Sarebbe rumore, e basta.

La regola sta nel nucleo (`testi::da_restituire`), non nella finestra: vale
anche se un giorno il pulsante fosse in un altro posto.

Con l'interruttore dei testi spento il pulsante non compare: hai già detto che
verso quel servizio non vuoi traffico, e questa ne è la forma più esplicita.

---

## 3. Lo scrobbling

Parte **solo se lo colleghi tu**, e manda esattamente quel che uno scrobble è.

| Host | Quando | Cosa gli arriva |
| --- | --- | --- |
| `ws.audioscrobbler.com`, `www.last.fm` | Se colleghi Last.fm | Artista, titolo, album e l'istante in cui hai finito di ascoltare |
| `api.listenbrainz.org` | Se colleghi ListenBrainz | Come sopra |

Contano solo gli ascolti arrivati almeno a metà brano. La coda di quel che deve
ancora partire vive sul tuo disco, e c'è un tasto che la svuota senza mandarla.

---

## 4. Il backup su Google Drive

Parte **solo se lo colleghi tu**, e non è acceso di serie.

| Host | Quando | Cosa gli arriva |
| --- | --- | --- |
| `accounts.google.com` | Al collegamento | Il consenso si apre nel **browser di sistema**, mai dentro la finestra di Aether |
| `oauth2.googleapis.com` | Al collegamento e ai rinnovi | Lo scambio dei token |
| `www.googleapis.com` | A ogni backup | Il documento del backup |

Il backup contiene la tua **libreria**: brani, playlist, valutazioni,
cronologia. Non i file audio. Finisce nella cartella riservata
all'applicazione del **tuo** Drive, dove nessun altro programma la vede.

Il token di aggiornamento sta nel portachiavi del sistema operativo. Scollegando
l'account viene cancellato; quel che è già stato caricato resta sul tuo Drive
finché non lo cancelli tu.

L'alternativa che non tocca nessuna rete c'è: sincronizzare su una cartella
condivisa.

---

## 5. I negozi, quando premi il tasto

Nella lista «Da comprare» ci sono tre tasti — Bandcamp, Qobuz, Discogs — che
aprono una **ricerca** nel browser di sistema.

Non partono da soli: parte solo il click. Non sono link affiliati, non c'è
tracciamento, e l'indirizzo lo costruisce il programma da un elenco chiuso di
tre domini scritto nel codice — apposta perché non ci si possa infilare altro,
nemmeno per errore. Quel che quei siti sanno di te da lì in poi è affare loro e
delle loro informative.

---

## 6. Il controllo aggiornamenti

Questa è **l'unica richiesta che Aether fa senza che tu gliel'abbia chiesta**, e
per questo ha una sezione sua invece di una riga in fondo a un elenco.

| Host | Quando | Cosa gli arriva |
| --- | --- | --- |
| `github.com` | Due minuti dopo l'avvio, poi ogni trenta minuti | Niente: una GET a un file pubblico |
| `objects.githubusercontent.com` | Solo se premi «Aggiorna» | La richiesta del file dell'installer |

Il file che scarica si chiama `latest.json`, sta fra gli allegati dell'ultima
release, ed è pubblico: è lo stesso che vedresti aprendo la pagina delle release
in un browser. Dentro ci sono un numero di versione, due righe di note e un
indirizzo.

**Nella richiesta non va niente di tuo.** Nessun identificativo, nessun numero
di serie, nessun conteggio, nessuna informazione sulla tua libreria. Non viaggia
nemmeno la versione che hai installata: il confronto lo fa il programma sul tuo
computer, dopo aver letto il file. Quel che GitHub può dedurne è che qualcuno,
da un certo indirizzo IP, ha chiesto un file pubblico — esattamente quel che
saprebbe di chiunque aprisse quella pagina con un browser.

**Non installa niente da solo.** Quando trova una versione nuova lo dice e si
ferma. Lo scaricamento e l'installazione partono da un tasto, perché installare
vuol dire chiudere Aether, e non è una cosa da fare mentre stai ascoltando. Se
rispondi «non ora», quella versione non te la chiede più: te lo richiede la
prossima.

**Quel che scarica viene verificato.** L'installer porta con sé una firma
crittografica, e Aether la controlla contro una chiave pubblica compilata dentro
l'eseguibile prima di eseguire qualunque cosa. Se non torna, l'installer viene
buttato via. Serve a un caso preciso: qualcuno che riesca a rispondere al posto
di GitHub — un proxy aziendale, un certificato di troppo nel magazzino del
sistema — non può farti installare un programma suo.

**Si spegne**, in *Impostazioni → Aggiornamenti*. Spento, Aether non contatta
mai GitHub: resterai su questa versione finché non verrai a premere «Controlla
adesso» o non scaricherai l'installer a mano. È acceso di serie, e questa è una
scelta: un lettore musicale che non si aggiorna è un lettore musicale che tiene
una libreria di rete vecchia di mesi.

---

## 7. Quel che *non* succede, e vale la pena dirlo

- **Nessuna richiesta all'avvio, tranne quella del § 6.** Aprire Aether non
  manda niente a nessuno per i primi due minuti, e dopo manda una GET a un file
  pubblico. Non c'è nessun altro ping, e non c'è niente che parta alla chiusura.
- **Nessuna richiesta in chiaro.** Il client HTTP rifiuta `http://` per
  costruzione: se una costante sbagliata finisse nel codice, la richiesta non
  partirebbe invece di uscire leggibile.
- **Nessun dominio esterno nella finestra.** La politica dei contenuti
  (`tauri.conf.json`) ammette solo `'self'` e `data:`. Le copertine delle
  anteprime arrivano come `data:` URI già scaricati dal nucleo: la pagina non
  parla mai direttamente con un catalogo, e non può.
- **Nessun cookie, nessun local storage** con dati personali.
- **Nessuna scrittura fuori dalle cartelle che hai indicato**, salvo la cartella
  dati dell'applicazione.

---

## 8. I file che Aether modifica

Due funzioni scrivono nei tuoi file, e conviene saperlo:

- **L'arricchimento dei metadati** riscrive i tag (e la copertina incorporata)
  dei brani che identifica con certezza. Si può spegnere, e sa tornare indietro.
- **Il riordino** sposta i file sul disco in `Artista/Album/NN - Titolo`.
  Mostra il piano prima, e ha un annullamento.

Nessuna delle due manda niente da nessuna parte: è tutto locale.

### Il diario

Dentro la cartella dati dell'applicazione — `%APPDATA%\io.github.federicobaratti.aether\diario`
— Aether tiene tre file di testo con quel che è andato storto: il filo che non è
partito, il dispositivo audio che è sparito, il codice d'errore di un comando
che ha risposto male. Servono a una cosa sola, e cioè a poter rispondere a
«non si apre» con qualcosa di più di «prova a reinstallarlo».

**Cosa ci finisce:** codici di errore (`db.openFailed`, `playback.deviceLost`),
la causa tecnica che li accompagna, il nome del filo che li ha prodotti, e il
percorso della **cartella dati dell'applicazione** — quella qui sopra, che su
Windows contiene il nome del tuo account perché ci passa dentro `%APPDATA%`. È
l'unica cosa tua che ci finisca, ed è tenuta apposta perché è la prima riga da
guardare quando Aether non si apre: dice se il database era dove doveva essere.
Cancellarla dal file prima di allegarlo non toglie niente al resto.

**Cosa non ci finisce:** i titoli dei tuoi brani, i nomi degli artisti, i
percorsi dei tuoi file musicali, quel che ascolti e quando. Non è una promessa
sulla buona volontà di chi scrive: dove un messaggio avrebbe voluto nominare un
brano c'è il suo numero di riga, e dove avrebbe voluto scrivere un percorso c'è
la sola estensione del file. Un diario si spedisce, e spedire la lista di cosa
qualcuno ascolta non sarebbe una diagnosi.

**Non parte da lì niente.** Il diario **non viene mandato a nessuno**, mai, e
non c'è nessun comando che possa farlo: un programma che sa spedire da sé i
propri log è un programma con la telemetria, e qui non ce n'è. L'unica cosa che
la finestra sa fare è aprire quella cartella nel gestore file, dal bottone in
*Impostazioni → Aggiornamenti*. Cosa farne dopo lo decidi tu.

I file si danno il cambio quando arrivano a due megabyte, e se ne tengono tre:
il più vecchio viene cancellato. Cancellarli tutti a mano non rompe niente.

---

## 9. Se cambia qualcosa

Questo documento vive nel repository accanto al codice. Se una versione futura
aggiungesse una richiesta di rete, la riga corrispondente comparirebbe qui nello
stesso commit — e il `CHANGELOG.md` lo direbbe. Un programma che parla con la
rete senza dirlo nel proprio changelog è un programma di cui non ci si può
fidare, e questo vale anche quando a scriverlo sono io.
