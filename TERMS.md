# Condizioni d'uso di Aether

Ultimo aggiornamento: 19 agosto 2026.

Aether è un programma gratuito, distribuito sotto licenza MIT. Questo documento
non è il contratto fra te e me — quello è il `LICENSE`, e dice che il programma
è dato «così com'è». Questo documento riguarda una cosa diversa e più concreta:
**cosa puoi fare con la musica che Aether ti procura**, perché quelle condizioni
non le stabilisco io e non posso rinunciarci per tuo conto.

Vale la pena leggerlo una volta. È corto.

---

## 1. Il programma è tuo, la musica no

Il codice di Aether è MIT: fanne quello che vuoi.

La musica che scarichi attraverso Aether **non è coperta da quella licenza**.
Ogni brano arriva da un catalogo, e porta con sé le condizioni che quel catalogo
e quell'artista gli hanno messo addosso. Aether registra la licenza di ogni
brano accanto al brano stesso, e la mostra nella coda, esattamente perché questa
distinzione non si perda.

Il caso normale è semplice: quel che scarichi lo puoi ascoltare, tenere,
mettere sul telefono, masterizzare su un CD per te. Le condizioni sotto
riguardano il **ridistribuire** e il **guadagnarci**.

---

## 2. Il vincolo non commerciale, che vale anche per te

Due delle fonti che Aether usa hanno una clausola *non commercial*, e non è una
formalità.

### Live Music Archive (archive.org/details/etree)

I termini della collezione dicono che l'accesso e ogni ulteriore distribuzione
del materiale devono essere «**strictly noncommercial**». Gli artisti presenti
lì hanno dato il permesso di registrare e scambiare i loro concerti a quella
condizione, e a nessun'altra.

In pratica, per quel che scarichi da lì:

- puoi ascoltarlo, copiarlo per te, condividerlo con qualcuno senza chiedere
  soldi;
- **non** puoi venderlo, né inserirlo in qualcosa che vendi, né usarlo per fare
  pubblicità, né monetizzarlo in nessuna forma;
- **non** puoi caricarlo su una piattaforma che ci mette la pubblicità sopra.

### Jamendo (quando sarà attivo)

I *Jamendo API Terms of Use* definiscono «uso commerciale» come «any monetary
compensation, including any revenue arising from affiliation programs or
advertising». Dalla loro API si **ascolta**: i termini vietano esplicitamente di
mettere il contenuto in cache o di offrirne un accesso offline, e Aether non lo
fa. Il modulo Jamendo esiste dietro una feature che si può spegnere proprio per
questo.

### E per Aether stesso

È la ragione per cui **Aether è e resta gratuito**. Una versione a pagamento
violerebbe quelle clausole. Le donazioni sono accettate come sostegno allo
sviluppo del programma, non come pagamento per la musica — nessuna donazione
sblocca contenuti, nessun brano è dietro un pagamento.

---

## 3. Cosa Aether si rifiuta di fare, e perché

Alcune cose che un programma del genere *potrebbe* fare, e che Aether non fa:

- **Non scarica da YouTube.** Le loro *API Developer Policies* § III.E.1.a lo
  vietano; § III.I.7 vieta di separare l'audio dal video; § III.I.9 vieta di
  riprodurlo da un player non visibile. Un lettore musicale con una libreria è
  precisamente quel che quelle regole escludono.
- **Non fa scraping di Spotify.** Niente endpoint privati, niente gettoni
  ricostruiti, niente client id altrui. L'unica cosa che legge di Spotify è
  l'archivio che Spotify consegna **a te** su richiesta, che è tuo per diritto
  di portabilità dei dati.
- **Non scarica ciò che la licenza non consente.** Il controllo avviene prima di
  qualunque richiesta di rete: un brano marcato solo-ascolto non viene nemmeno
  chiesto. Se un catalogo dice «si ascolta e non si porta via», Aether non prova
  se il server glielo lascerebbe fare.
- **Non aggira misure di protezione.** Nessun DRM viene toccato, in nessuna
  forma.

Se trovi un modo di far fare a Aether una di queste cose, è un difetto. Segnalalo.

---

## 3-bis. I testi, e cosa LRCLIB è

I testi delle canzoni sono opere protette, come le canzoni. Aether ne prende
tre strade, e nessuna delle tre è «li abbiamo in licenza»:

1. **Quel che è già nei tuoi file** — un `.lrc` accanto al brano, o il tag
   dentro il file. È tuo, ed è la prima fonte che si guarda.
2. **LRCLIB**, un catalogo pubblico e gratuito alimentato da chi lo usa. Ha
   un'API aperta, fatta apposta per i lettori musicali: Aether la interroga
   come farebbe un browser con un sito, senza chiavi, senza raschiare pagine e
   senza aggirare niente.
3. **Quel che sincronizzi tu**, che resta un file sul tuo disco.

La parte che di solito si tace, e che qui si dice: LRCLIB **non è un
distributore con contratti di sotto-edizione**. È una base dati costruita dalla
comunità. Aether non infrange nessuna regola nel leggerla, e questo non rende
«licenziati» i testi che ne escono. Chi ospita quel catalogo risponde alle
richieste di rimozione degli aventi diritto; Aether non ridistribuisce niente,
non ne tiene copie altrove e non li carica nel tuo backup.

Musixmatch, Genius, AZLyrics, LyricFind e i cataloghi cinesi restano fuori, e
non per pigrizia: o vogliono una licenza che un lettore locale non ha, o si
leggono soltanto raschiando una pagina scritta per un browser. La CI del
progetto verifica a ogni modifica che nessuno di quei nomi sia rientrato nel
codice — la stessa regola che tiene fuori gli strumenti di scaricamento.

### Se scegli di restituire

Dopo aver sincronizzato un testo a mano puoi mandarlo a LRCLIB. È un pulsante,
si preme uno per volta, e la quarta strada — «i testi di qualcun altro» — non
esiste: si pubblica **solo** quel che hai sincronizzato tu, mai quel che stava
nel tag di un file e mai quel che dal catalogo è appena arrivato.

Va detto chiaramente cosa stai facendo quando lo premi. Il testo di una canzone
resta dell'avente diritto anche quando le battute le hai messe tu: quel che
metti in comune sono i **tempi**, e i tempi arrivano insieme alle parole perché
separati non servono a niente. Se le parole di quel testo non le hai né scritte
né il diritto di diffonderle, mandarle a un catalogo pubblico è una tua
decisione e non una che Aether prende al posto tuo — ed è la ragione per cui
non c'è nessun modo di farlo accadere senza averlo deciso.

Quello che va a LRCLIB ci va con la licenza di LRCLIB, non con quella di
Aether: è il catalogo a ospitarlo e a rispondere di quel che ospita. Aether non
ne tiene copia, non lo rivende e non lo rimette in circolo altrove. Il tuo
`.lrc` resta sul tuo disco, identico, che tu prema quel pulsante o no.

---

## 4. Quel che resta da comprare

Quando nessun catalogo libero ha un brano, Aether non ci riprova all'infinito e
non finge che sia un guasto: lo mette in «Da comprare», con i link a Bandcamp,
Qobuz e Discogs.

Non guadagno niente da quei link. Non sono affiliati, non c'è tracciamento, e
l'indirizzo lo costruisce il programma da un elenco chiuso di tre domini —
proprio perché non ci si possa infilare altro.

---

## 5. La tua libreria è tua

Aether lavora su file che stanno sul tuo disco e su un database SQLite che sta
nella tua cartella dati. Non c'è un server, non c'è un account, non c'è niente
che io possa disattivare.

Alcune operazioni **modificano i tuoi file**: la scrittura dei tag durante
l'arricchimento dei metadati, e il riordino che sposta i file sul disco.
Entrambe mostrano un piano prima di agire ed entrambe sanno tornare indietro,
ma un backup dei file a cui tieni resta una buona idea — vale per qualunque
programma che scrive sui tuoi dati, incluso questo.

---

## 6. Nessuna garanzia

Come dice il `LICENSE`: il programma è fornito «così com'è», senza garanzia di
alcun tipo. Non rispondo di dati persi, file danneggiati o dischi pieni.

Non rispondo nemmeno di quel che fai con la musica che scarichi. Le condizioni
di ogni brano te le mostro; rispettarle è cosa tua.

---

## 7. Se qualcosa qui è sbagliato

Se rappresenti un catalogo, un'etichetta o un artista e ritieni che Aether stia
facendo qualcosa che non deve, scrivi. Non c'è una procedura formale perché non
c'è un'azienda: c'è una persona, e la risposta a una segnalazione fondata è
sistemare il codice.
