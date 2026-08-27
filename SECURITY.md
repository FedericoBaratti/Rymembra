# Segnalare una vulnerabilità

## Come

Usa la **segnalazione privata di GitHub**: nella pagina del repository,
*Security* → *Report a vulnerability*. Arriva solo a chi mantiene il progetto e
non è pubblica finché non lo decidiamo insieme.

Non aprire una issue pubblica per una vulnerabilità. Non è cerimonia: le issue
sono indicizzate, e la prima persona che leggerebbe una segnalazione aperta non
sarebbe chi può correggerla.

Se la segnalazione privata non è attiva sul repository, aprine una pubblica che
dice soltanto «ho trovato un problema di sicurezza, come te lo mando?» — senza
dettagli — e ci si organizza da lì.

## Cosa aspettarsi

Aether lo mantiene una persona sola, nel tempo che avanza. Quindi: **risposta
entro una settimana**, e se non arriva insisti pure, perché vuol dire che la
notifica si è persa. Non c'è un premio in denaro e non c'è un programma di
bug bounty.

Quando la correzione esce, la release la nomina e il tuo nome ci sta accanto se
lo vuoi.

## Le versioni che ricevono correzioni

L'ultima pubblicata, e nient'altro. Non ci sono rami di manutenzione: chi ha una
versione più vecchia si aggiorna, e l'updater lo dice da sé entro mezz'ora.

## Cosa conta come vulnerabilità, qui

Aether è un'applicazione locale senza account e senza server. Non c'è una
superficie di rete in ascolto — con **una** eccezione, e vale la pena dirla — e
non ci sono dati di altri utenti da esporre. Le cose che contano davvero sono
quattro, e sono quelle che possono far eseguire codice o portare via qualcosa
che non è nostro.

**La catena degli aggiornamenti.** È l'unica parte di Aether che scarica un
eseguibile e lo lancia. L'installer viene verificato con minisign contro la
chiave pubblica compilata dentro il binario (`plugins.updater.pubkey` in
`tauri.conf.json`) e se la firma non torna non viene eseguito. Qualunque modo di
far installare qualcosa saltando quella verifica — o di far accettare un
`latest.json` che non viene da noi — è la segnalazione più importante che si
possa mandare.

**Il porto di ritorno di OAuth.** Il consenso di Google si apre nel browser di
sistema e torna su `127.0.0.1`, su una porta aperta per il tempo di
un'autorizzazione (`aether-oauth::loopback`). È l'unica cosa che ascolta, ed è
in ascolto per pochi secondi: se si riesce a farle accettare un codice che
arriva da qualcun altro, o a tenerla aperta oltre, è un problema.

**Gli ingressi non fidati.** Una skin `.aeskin` è un archivio zip che qualcuno ti
manda, e l'archivio GDPR di Spotify pure. `aether-skin::package` ha dei tetti
espliciti — sul manifest, sui file, sui percorsi — e servono a impedire che
scompattare qualcosa scriva fuori dalla sua cartella o riempia il disco. Un
percorso che esce dalla cartella d'installazione, uno zip che esplode, un
manifest che manda in ricorsione il compilatore: sono tutti problemi.

**I segreti a riposo.** I token OAuth stanno nel portachiavi di sistema
(`aether-oauth::portachiavi`), non nel database. Se finiscono da qualche altra
parte — in un log, nel database, in un file temporaneo — è un problema, e
lo è anche se «solo» in chiaro sul disco dell'utente stesso.

Il diario (`PRIVACY.md` § 8) è scritto per non contenere niente di personale, e
non viene mandato a nessuno. Se ci finisce dentro qualcosa che non dovrebbe,
dillo: è la stessa promessa, e vale anche quando a romperla è una riga scritta
per sbaglio.

## Cosa invece non lo è

- **L'installer non firmato.** È noto e dichiarato: `README.md`, il corpo di
  ogni release e la finestra degli aggiornamenti lo dicono. Un certificato OV
  richiede la validazione dell'identità e un costo annuo, e finché non c'è,
  SmartScreen avvisa. Non serve segnalarlo.
- **«Aether parla con `archive.org`».** Sì, quando glielo chiedi.
  `PRIVACY.md` elenca ogni richiesta che il programma può fare e cosa ci mette
  dentro. Se ne trovi **una che non è in quell'elenco**, quella sì.
- **Le dipendenze con un avviso aperto ma non raggiungibile.** Utile saperlo, e
  si aggiornano comunque; ma una segnalazione con scritto «`cargo audit` dice
  qualcosa» senza un percorso che ci arrivi non è una vulnerabilità di Aether.

## Se hai trovato qualcosa mentre ci giocavi

Va benissimo. Non serve un preambolo, non serve una prova di concetto elegante,
e non serve scusarsi per aver guardato. Mandami cosa hai fatto e cosa è
successo, anche in tre righe.
