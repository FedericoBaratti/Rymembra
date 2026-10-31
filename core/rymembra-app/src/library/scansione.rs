//! La scansione che esegue il piano: il [`Deposito`] che presta la connessione, la squadra
//! delle letture in parallelo, la memoria dei rifiuti e le fasi di [`Scan::run_su`].

use super::*;

/// Dove va a finire una riga letta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Destinazione {
    /// Un brano che in libreria non c'era.
    Nuova,
    /// Una riga esistente, il cui file è cambiato sul posto.
    Aggiorna(i64),
    /// Una riga esistente il cui file si è spostato: si riscrive il percorso.
    Sposta(i64),
    /// Una riga **provvisoria** — entrata senza lettura, vedi
    /// [`riga_provvisoria`] — che la lettura completa. Si scrive come
    /// [`Destinazione::Aggiorna`]; cambia quel che succede se il lettore la
    /// rifiuta: vedi [`togli_provvisorie`].
    Provvisoria(i64),
}

/// Calcola il piano senza toccare niente.
///
/// È la chiamata che precede [`Scan::run`] quando si vuole mostrare all'utente
/// cosa sta per succedere — «sto per togliere 340 brani» — e lasciargli dire di
/// no prima che succeda.
///
/// La camminata è la stessa di [`cammina`], scadenze comprese: le radici che non
/// rispondono restano fuori dal piano, quelle che si arenano pure. Se qualcuno
/// ha chiesto di smettere, il piano che torna è **vuoto** e
/// [`Esplorazione::fermata`] lo dichiara.
///
/// Dalla finestra non si passa di qui ma da [`Scan::run_su`], che il piano se lo
/// calcola da sé: questa prende una `Connection` in prestito per tutta la
/// durata, camminata inclusa, ed è esattamente la cosa che sul desktop non si
/// può fare.
pub fn plan(scan: &Scan<'_>, connection: &Connection) -> Result<Esplorazione, AppError> {
    let fermata = |ricognizione: Ricognizione| Esplorazione {
        piano: ScanPlan::default(),
        radici_saltate: ricognizione.radici_saltate,
        fermata: true,
        rami_persi: ricognizione.rami_persi,
        non_verificati: 0,
    };
    let mut ricognizione = cammina(scan);
    if ricognizione.fermata {
        return Ok(fermata(ricognizione));
    }
    // I fogli come li vede la scansione vera, cache compresa: un'anteprima che
    // non espandesse le immagini mostrerebbe da inserire i dischi interi, e da
    // togliere i loro brani. La cache qui si legge e basta: la scrive chi
    // scansiona davvero.
    let cache = fogli_ricordati(connection)?;
    let con_brani = fogli_con_brani(connection, scan.rules)?;
    let fogli = scan.espandi_i_fogli(&mut ricognizione, &cache, &con_brani);
    if fogli.fermata {
        return Ok(fermata(ricognizione));
    }
    let known = known_tracks(connection)?;
    let immagini = immagini_dei_brani(connection)?;
    let (piano, non_verificati) =
        scan.pianifica(&mut ricognizione, &known, &immagini, &fogli.abbinamenti);
    Ok(Esplorazione {
        piano,
        radici_saltate: ricognizione.radici_saltate,
        fermata: false,
        rami_persi: ricognizione.rami_persi,
        non_verificati,
    })
}

/// Toglie dal piano le rimozioni che stanno sotto un ramo perso.
///
/// # Perché qui e non nel dominio
///
/// Perché il dominio decide su quel che gli si dà — radici, file trovati, righe
/// note — e un ramo perso non è nessuna delle tre: è un pezzo di radice su cui
/// la camminata non ha potuto dire niente. Il piano lo vede come «nessun file
/// trovato là sotto», che per lui è la stessa cosa di «là sotto non c'è più
/// niente». Correggerlo qui, dopo, tiene il dominio puro e la regola accanto
/// all'unico posto che sa dei rami.
///
/// # Cosa si risparmia, e cosa no
///
/// Solo le righe [`RemoveReason::Disappeared`] sotto un ramo perso: sono le
/// sole che il piano ha giudicato su un'assenza, e l'assenza sotto un ramo che
/// non si è letto non prova niente. Le righe doppie
/// ([`RemoveReason::DuplicateRow`]) restano da togliere: sono un fatto del
/// database, non del disco. E niente si aggiunge agli aggiornamenti: un file
/// sotto un ramo perso non è arrivato, quindi non c'è niente da rileggere — le
/// sue righe restano come sono, **né tolte né riscritte**.
///
/// Le righe risparmiate passano fra le `untouched`, che è quel che sono: fuori
/// dalla competenza di questa passata. Tenerle nel conto invece di farle
/// sparire serve anche alla guardia contro le stragi, che somma tutte le righe
/// conosciute per sapere quanto pesa una rimozione.
///
/// Restituisce quante righe ha risparmiato.
fn risparmia_i_non_verificati(piano: &mut ScanPlan, rami: &[String], rules: PathRules) -> usize {
    if rami.is_empty() {
        return 0;
    }
    let prima = piano.to_remove.len();
    piano.to_remove.retain(|rimozione| {
        rimozione.reason != RemoveReason::Disappeared
            || !rami
                .iter()
                .any(|ramo| is_under(&rimozione.track.path, ramo, rules))
    });
    let risparmiate = prima.saturating_sub(piano.to_remove.len());
    piano.untouched = piano.untouched.saturating_add(risparmiate);
    risparmiate
}

/// Chi custodisce la connessione, e la presta una fase per volta.
///
/// # Perché un tratto e non una `&mut Connection`
///
/// Perché sul desktop la connessione non è di nessuno: sta dietro un mutex che
/// duecento chiamanti si contendono, e chi lo tiene ferma la finestra, l'audio e
/// la chiusura dell'applicazione. Una scansione che ricevesse `&mut Connection`
/// obbligherebbe chi la chiama a prendere quel lucchetto **prima** e a
/// restituirlo **dopo**, cioè a tenerlo per tutta la camminata e tutta la
/// lettura dei file: i minuti in cui una share lenta fa aspettare.
///
/// Con un deposito la scansione chiede la connessione quando le serve
/// davvero — cinque o sei volte in tutto, ognuna per il tempo di una
/// transazione — e fra una richiesta e l'altra il lucchetto è di chi lo vuole.
///
/// # Perché un metodo generico e non una chiusura
///
/// Perché la connessione non può uscire dal metodo: chi implementa questo tratto
/// sul desktop tiene una guardia di mutex viva per la durata della chiamata, e
/// un `fn connessione(&mut self) -> &mut Connection` non avrebbe modo di dire
/// quando quella guardia si può lasciare andare. Il prezzo è che il tratto non è
/// oggetto-sicuro — niente `dyn Deposito` — e per questo [`Scan::run_su`] è
/// generica.
///
/// # Il contratto
///
/// - **Può aspettare.** Una chiamata a [`Self::con_connessione`] può bloccare
///   quanto serve; è chi la chiama a fare in modo che non ce ne siano dentro le
///   fasi lunghe.
/// - **Fra due chiamate il database cambia.** Il deposito non promette nessuna
///   continuità: un'altra scrittura può essere passata in mezzo. Vedi
///   [`Scan::run_su`] per cosa comporta.
/// - **L'esclusione di una seconda scansione non spetta a qui.** Il nucleo non
///   sa quante scansioni ci sono: prima l'esclusione era un effetto collaterale
///   del lucchetto tenuto tutto il tempo, e togliendo quello va rimessa dove
///   deve stare, cioè in chi decide di avviarne una.
pub trait Deposito {
    /// Presta la connessione per il tempo di `azione`.
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> Result<T, AppError>;
}

/// Il deposito più semplice che ci sia: la connessione è già in mano.
///
/// È il caso della riga di comando e delle prove, dove nessuno contende niente.
/// Sul desktop l'implementazione vera avvolge il mutex della libreria.
impl Deposito for Connection {
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> Result<T, AppError>,
    {
        azione(self)
    }
}

/// Un file che non è arrivato in tempo.
///
/// `FsNetworkUnavailable` e non un codice suo: per chi legge l'esito è la stessa
/// cosa — «questo file non si è potuto leggere, e la colpa non è del file» — e
/// soprattutto è la stessa cosa per [`Scan::file_illeggibile`], che su questo
/// codice fa la sonda e decide se abbandonare la radice. Un codice nuovo
/// vorrebbe dire ripetere lì la stessa domanda con un altro nome.
///
/// La causa dice quanto si è aspettato, con lo stesso lessico con cui il motore
/// audio racconta un'apertura scaduta: chi legge il diario incontra la stessa
/// frase per lo stesso guasto.
fn scaduto(path: &str, scadenza: Duration) -> AppError {
    AppError::new(ErrorCode::FsNetworkUnavailable {
        path: Some(path.to_owned()),
    })
    .with_cause(format!(
        "la lettura non è finita entro {} secondi",
        scadenza.as_secs()
    ))
}

/// Un file che ha fatto cadere il lettore dei tag.
///
/// `metadata.tagReadFailed`, cioè la stessa cosa che si dice di un file i cui
/// tag non si leggono: per chi guarda l'esito è esattamente quello, un file
/// storto da ritaggare o da buttare. **Non** `FsNetworkUnavailable` come una
/// scadenza, e la differenza è tutto il punto: quel codice fa sondare la radice
/// e conta verso l'abbandono, mentre un file malformato dice che il disco ha
/// consegnato benissimo — è il contenuto a essere sbagliato. Vedi [`Mancato`].
fn caduto(path: &str, messaggio: Option<&str>) -> AppError {
    let causa = messaggio.map_or_else(
        || "il lettore dei tag è caduto su questo file".to_owned(),
        |testo| format!("il lettore dei tag è caduto su questo file: {testo}"),
    );
    AppError::new(ErrorCode::MetadataTagReadFailed {
        path: Some(path.to_owned()),
    })
    .with_cause(causa)
}

/// Un lavoro per la squadra, col suo posto nell'ordine e la sua scadenza.
///
/// La scadenza viaggia col lavoro e non con la squadra: nella stessa passata
/// un file del disco e uno del cloud si aspettano per tempi diversi.
type Incarico<T> = (usize, Duration, Box<dyn FnOnce() -> T + Send + 'static>);

/// Fino a [`LETTURE_IN_VOLO`] lavori insieme, ognuno sul suo
/// [`crate::scadenza::Operaio`] e con la sua scadenza.
///
/// # Perché così
///
/// `Operaio::esegui` aspetta il suo lavoro: è fatto per uno alla volta, ed è
/// giusto così — è lui a sapere quando un filo si è appeso. Per averne quattro
/// in volo servono quattro operai, e qualcuno che aspetti ciascuno: sono i
/// quattro fili della squadra, che prendono gli incarichi da una coda sola e
/// rimandano gli esiti col loro posto. Chi coordina — il filo della scansione —
/// non aspetta mai un file preciso: aspetta il prossimo esito, un [`BATTITO`]
/// per volta, e intanto guarda se qualcuno ha chiesto di smettere.
///
/// # Chi si appende
///
/// Un file che non torna appende il filo dell'operaio, non quello della
/// squadra: alla scadenza l'operaio lo abbandona e risponde
/// [`Mancato::Scaduto`], e il filo della squadra passa al prossimo incarico con
/// un operaio nuovo.
///
/// # Lasciarla andare
///
/// Nessun `Drop` e nessun `join`: cadendo la squadra cade la coda, e ogni filo
/// esce appena finito quel che sta facendo — al più dopo una scadenza. È quel
/// che permette a un annullamento di non aspettare le letture in volo.
struct Squadra<T: Send + 'static> {
    /// La coda degli incarichi; `None` se il sistema non ha dato nemmeno un filo.
    coda: Option<std::sync::mpsc::Sender<Incarico<T>>>,
    /// Gli esiti, col posto dell'incarico.
    esiti: std::sync::mpsc::Receiver<(usize, Result<T, Mancato>)>,
    /// Per rispondere da sé quando non c'è nessuno a cui affidare: l'incarico
    /// vale scaduto, come per un `Operaio` che non riesce ad assumere.
    da_se: std::sync::mpsc::Sender<(usize, Result<T, Mancato>)>,
}

impl<T: Send + 'static> Squadra<T> {
    /// Assume `quanti` fili. Ogni incarico porta la sua scadenza.
    fn nuova(nome: &str, quanti: usize) -> Self {
        let (manda, ricevi) = std::sync::mpsc::channel::<Incarico<T>>();
        let ricevi = Arc::new(std::sync::Mutex::new(ricevi));
        let (rispondi, esiti) = std::sync::mpsc::channel();
        let mut assunti = 0_usize;
        for numero in 0..quanti {
            let coda = Arc::clone(&ricevi);
            let rispondi = rispondi.clone();
            let nome_operaio = format!("{nome}{numero}");
            let avviato = std::thread::Builder::new()
                .name(format!("rymembra-squadra-{nome}{numero}"))
                .spawn(move || {
                    let mut operaio = crate::scadenza::Operaio::nuovo(&nome_operaio);
                    loop {
                        // Il lucchetto solo per prendere: chi aspetta un
                        // incarico lo tiene, gli altri aspettano dietro di lui,
                        // e nessuno lavora con il lucchetto in mano.
                        let incarico = {
                            let Ok(guardia) = coda.lock() else {
                                return;
                            };
                            guardia.recv()
                        };
                        let Ok((posto, scadenza, lavoro)) = incarico else {
                            return;
                        };
                        let esito = operaio.esegui(scadenza, lavoro);
                        if rispondi.send((posto, esito)).is_err() {
                            return;
                        }
                    }
                });
            if avviato.is_ok() {
                assunti += 1;
            }
        }
        Self {
            coda: (assunti > 0).then_some(manda),
            esiti,
            da_se: rispondi,
        }
    }

    /// Affida un lavoro, da aspettare al più `scadenza`. Non aspetta niente.
    fn affida(
        &self,
        posto: usize,
        scadenza: Duration,
        lavoro: Box<dyn FnOnce() -> T + Send + 'static>,
    ) {
        let consegnato = self
            .coda
            .as_ref()
            .is_some_and(|coda| coda.send((posto, scadenza, lavoro)).is_ok());
        if !consegnato {
            let _ = self.da_se.send((posto, Err(Mancato::Scaduto)));
        }
    }

    /// Il prossimo esito, se arriva entro `attesa`.
    fn prossimo(&self, attesa: Duration) -> Option<(usize, Result<T, Mancato>)> {
        self.esiti.recv_timeout(attesa).ok()
    }
}

/// Una lettura da fare: un file intero, o i brani di un'immagine CD.
///
/// I brani della stessa immagine diventano **un lavoro solo**: l'immagine si
/// apre una volta, e ne escono tutte le righe. Aprirla una volta per brano
/// vorrebbe dire leggere quattordici volte la stessa intestazione — e su un
/// FLAC da quattrocento megabyte la stessa copertina incorporata — per un disco
/// solo.
struct Lavoro {
    /// Le voci del piano che il lavoro copre, nell'ordine in cui escono le
    /// righe: una per un file intero, una per brano per un'immagine.
    voci: Vec<(DiscoveredFile, Destinazione)>,
    /// L'immagine con il suo foglio, per i brani d'immagine.
    immagine: Option<Arc<Abbinamento>>,
}

impl Lavoro {
    /// Il percorso da cui si capisce sotto quale radice sta il lavoro.
    ///
    /// Il primo delle voci: i brani d'immagine stanno nella cartella del loro
    /// `.cue`, quindi sotto la stessa radice dell'immagine.
    fn percorso(&self) -> &str {
        self.voci.first().map_or("", |(file, _)| file.path.as_str())
    }
}

/// Raccoglie quel che va letto in lavori: i brani della stessa immagine
/// insieme, al posto del primo di loro; ogni altro file da solo.
fn in_lavori(
    da_leggere: Vec<(DiscoveredFile, Destinazione)>,
    immagini: &std::collections::HashMap<String, Arc<Abbinamento>>,
    rules: PathRules,
) -> Vec<Lavoro> {
    let mut lavori: Vec<Lavoro> = Vec::with_capacity(da_leggere.len());
    let mut del_foglio: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (file, destinazione) in da_leggere {
        let foglio = brano_d_immagine(&file.path).map(|(cue, _)| path_key(cue, rules));
        let abbinamento = foglio
            .as_ref()
            .and_then(|chiave| immagini.get(chiave).map(|abbinato| (chiave, abbinato)));
        let Some((chiave, abbinamento)) = abbinamento else {
            lavori.push(Lavoro {
                voci: vec![(file, destinazione)],
                immagine: None,
            });
            continue;
        };
        if let Some(lavoro) = del_foglio
            .get(chiave)
            .and_then(|&posto| lavori.get_mut(posto))
        {
            lavoro.voci.push((file, destinazione));
            continue;
        }
        del_foglio.insert(chiave.clone(), lavori.len());
        lavori.push(Lavoro {
            voci: vec![(file, destinazione)],
            immagine: Some(Arc::clone(abbinamento)),
        });
    }
    lavori
}

/// Un rifiuto come lo ricorda `file_illeggibili`: vedi la migrazione 032.
struct RifiutoRicordato {
    /// La dimensione che il file aveva.
    dimensione: i64,
    /// La data che aveva.
    modificato_ms: i64,
    /// La versione del lettore che l'ha rifiutato.
    versione: i64,
}

impl RifiutoRicordato {
    /// Il file è ancora quello rifiutato, davanti allo stesso lettore?
    fn vale_per(&self, file: &DiscoveredFile) -> bool {
        i64::try_from(file.size_bytes).is_ok_and(|dimensione| dimensione == self.dimensione)
            && file.modified_ms == self.modificato_ms
            && self.versione == crate::metadata::VERSIONE_LETTORE
    }
}

/// La memoria dei rifiuti, per chiave di percorso.
fn rifiuti_ricordati(
    connection: &Connection,
) -> Result<std::collections::HashMap<String, RifiutoRicordato>, AppError> {
    let mut statement = connection
        .prepare("SELECT chiave, dimensione, modificato_ms, versione FROM file_illeggibili")
        .map_err(|err| db_error("lettura dei file illeggibili", &err))?;
    let righe = statement
        .query_map([], |riga| {
            Ok((
                riga.get::<_, String>(0)?,
                RifiutoRicordato {
                    dimensione: riga.get(1)?,
                    modificato_ms: riga.get(2)?,
                    versione: riga.get(3)?,
                },
            ))
        })
        .map_err(|err| db_error("lettura dei file illeggibili", &err))?;
    righe
        .collect::<Result<std::collections::HashMap<_, _>, _>>()
        .map_err(|err| db_error("lettura dei file illeggibili", &err))
}

/// Un rifiuto di questa passata, da ricordare.
struct RifiutoNuovo {
    /// La chiave di percorso.
    chiave: String,
    /// La dimensione del file.
    dimensione: i64,
    /// La sua data.
    modificato_ms: i64,
    /// Il perché, come lo racconta l'errore.
    causa: String,
}

/// Il rifiuto dice qualcosa **del file**, e lo ridirà finché il file non cambia?
///
/// Due codici soli: il lettore ha avuto i byte e ha detto di no
/// (`metadata.tagReadFailed`), o il formato non si sa leggere o suonare
/// (`playback.formatUnsupported`). Tutto il resto — la rete, una scadenza, un
/// permesso, un file sparito fra la camminata e la lettura — riguarda il
/// momento, e ricordarlo nasconderebbe per sempre un brano buono. Il lettore
/// che si schianta (`Mancato::Caduto`) porta anche lui `metadata.tagReadFailed`,
/// ma non passa di qui: chi chiama lo tratta a parte.
fn rifiuto_deterministico(error: &AppError) -> bool {
    matches!(
        error.code().kind(),
        ErrorCodeKind::MetadataTagReadFailed | ErrorCodeKind::PlaybackFormatUnsupported
    )
}

/// Com'è andata la fase dei fogli.
#[derive(Default)]
struct Fogli {
    /// Le immagini abbinate ai loro fogli.
    abbinamenti: Vec<Abbinamento>,
    /// I fogli letti adesso, da scrivere nella cache.
    letti: Vec<FoglioLetto>,
    /// I brani d'immagine entrati al posto delle immagini.
    brani: usize,
    /// Qualcuno ha chiesto di smettere mentre si leggevano.
    fermata: bool,
}

/// L'esito di un lavoro di lettura, per chi lo gestisce: `None` se non lo si è
/// nemmeno tentato perché la sua radice era già morta.
type EsitoLettura = Option<Result<Result<Vec<TrackRow>, AppError>, Mancato>>;

/// Lo stato della lettura che passa da un lotto all'altro.
struct StatoLettura<'r> {
    /// Le radici che hanno smesso di rispondere **mentre** si leggeva.
    abbandonate: Vec<String>,
    /// Quante letture di fila sono scadute su ciascuna radice.
    scadenze_di_fila: std::collections::HashMap<&'r str, usize>,
    /// Le radici che hanno consegnato un file intero di recente: su quelle si
    /// tengono più letture in volo. Vedi [`Scan::run_con`].
    fiducia: std::collections::HashSet<&'r str>,
    /// Le voci gestite finora.
    fatti: usize,
    /// Come si legge ogni radice della passata, chiesto una volta sola alla
    /// sorgente: vedi [`MusicFiles::limiti`].
    limiti: std::collections::HashMap<&'r str, Limiti>,
    /// Quante voci si sono date da leggere, per radice: il conto che
    /// [`Limiti::letture_per_giro`] ferma.
    letture: std::collections::HashMap<&'r str, usize>,
    /// Le radici con letture rimandate a un altro giro: vedi
    /// [`ScanReport::rimasti`].
    rimandate: Vec<String>,
    /// Le sorgenti che hanno chiesto di rallentare, per conto (o per radice,
    /// fuori dal cloud): le loro letture si rimandano tutte.
    frenate: std::collections::HashSet<String>,
}

impl StatoLettura<'_> {
    /// I limiti di una radice; quelli del disco per un file fuori da tutte.
    fn limiti_di(&self, radice: Option<&str>) -> Limiti {
        radice
            .and_then(|root| self.limiti.get(root).copied())
            .unwrap_or(Limiti::DISCO)
    }

    /// Chi ha chiesto di rallentare, per una radice: il conto, perché la quota
    /// di un servizio è dell'account e vale per tutte le sue cartelle.
    fn fonte(radice: &str) -> &str {
        rymembra_domain::nuvola::conto_di(radice).unwrap_or(radice)
    }

    /// La sorgente di questa radice ha chiesto di rallentare?
    fn frenata(&self, radice: &str) -> bool {
        self.frenate.contains(Self::fonte(radice))
    }

    /// Le letture di questa radice si rimandano: si contano, e la radice si
    /// ricorda fra quelle rimandate.
    fn rimanda(&mut self, radice: &str, quante: usize, report: &mut ScanReport) {
        report.rimasti = report.rimasti.saturating_add(quante);
        if !self.rimandate.iter().any(|r| r == radice) {
            self.rimandate.push(radice.to_owned());
        }
    }
}

/// Il servizio ha chiesto di rallentare?
///
/// Un `429` (`net.rateLimited`) o una quota finita (`cloud.quotaExceeded`):
/// non sono guasti del file né della rete, e non sono scadenze. Non contano
/// verso l'abbandono della radice, non finiscono fra gli illeggibili, e non si
/// ricordano: la lettura si rimanda a un altro giro.
fn chiede_di_rallentare(error: &AppError) -> bool {
    matches!(
        error.code().kind(),
        ErrorCodeKind::NetRateLimited | ErrorCodeKind::CloudQuotaExceeded
    )
}

/// Quel che un lotto ha letto, in attesa della sua transazione.
#[derive(Default)]
struct LottoLetto {
    /// Le righe, con la loro destinazione.
    righe: Vec<(TrackRow, Destinazione)>,
    /// I rifiuti da ricordare.
    rifiutati: Vec<RifiutoNuovo>,
    /// Le chiavi lette con successo: se la memoria le ricorda come rifiutate,
    /// non lo sono più.
    riusciti: Vec<String>,
    /// Le righe provvisorie il cui file il lettore ha rifiutato: vedi
    /// [`togli_provvisorie`].
    provvisorie_rifiutate: Vec<(i64, String)>,
}

impl Scan<'_> {
    /// Qualcuno ha chiesto di smettere?
    ///
    /// `false` quando nessuno può chiederlo, che è il caso di chi non offre un
    /// Annulla.
    pub(super) fn fermato(&self) -> bool {
        self.fermati.is_some_and(|chiedi| chiedi())
    }

    /// La radice sorvegliata sotto cui sta questo file, se ce n'è una.
    ///
    /// Serve a due domande sole — «questa radice l'ho già dichiarata morta?» e
    /// «di quale radice è morta la rete?» — e usa la stessa `is_under` con cui
    /// il dominio decide se una riga è di competenza di questa passata. Due
    /// funzioni diverse per la stessa domanda vorrebbero dire che un giorno una
    /// riga può essere sorvegliata per l'una e non per l'altra.
    fn radice_di(&self, path: &str) -> Option<&str> {
        self.roots
            .iter()
            .find(|root| is_under(path, root, self.rules))
            .map(String::as_str)
    }

    /// Come si legge un file: coi limiti della radice sotto cui sta, chiesti
    /// alla sorgente ([`MusicFiles::limiti`]). Un file fuori da tutte le radici
    /// — non succede con i file della camminata — legge come il disco.
    fn limiti_di(&self, path: &str) -> Limiti {
        self.radice_di(path)
            .map_or(Limiti::DISCO, |root| self.files.limiti(root))
    }

    /// Quanto si aspetta un file letto con questi limiti.
    ///
    /// L'attesa del disco è quella di [`Scan::scadenze`], e non quella di
    /// [`Limiti::DISCO`]: è il numero che le prove accorciano per non aspettare
    /// venticinque secondi, ed è lo stesso della 3.1. Una sorgente che ne
    /// dichiara uno diverso — il cloud, che aspetta un `429` — tiene il suo.
    fn scadenza_per(&self, limiti: &Limiti) -> Duration {
        if limiti.per_file == LETTURA_BRANO {
            self.scadenze.brano
        } else {
            limiti.per_file
        }
    }

    /// Un file che non si è potuto leggere: è rotto lui, o è caduta la rete?
    ///
    /// La distinzione costa una sonda — una sola, la prima volta che una radice
    /// dà guai — e vale l'intera scansione: un guasto di rete non è un file
    /// illeggibile da elencare a schermo, è una cartella da smettere di
    /// interrogare. Continuare significherebbe una scadenza per ogni file
    /// rimasto, e un elenco di «illeggibili» lungo quanto la libreria.
    ///
    /// Ci passa anche il file che non è arrivato **in tempo**: una lettura
    /// scaduta è indistinguibile da un guasto di rete finché non si chiede alla
    /// radice come sta, ed è esattamente la domanda che si fa qui.
    ///
    /// Con una risposta sola, però, non si conclude tutto: una radice che alla
    /// sonda risponde e alle letture no resta qui un file illeggibile per volta.
    /// A contare quante volte di fila accade — e a smettere alla terza — è
    /// [`Scan::run_su`], che ha il conto sott'occhio; vedi [`SCADENZE_DI_FILA`].
    fn file_illeggibile(
        &self,
        file: &DiscoveredFile,
        error: AppError,
        abbandonate: &mut Vec<String>,
        illeggibili: &mut Vec<Unreadable>,
    ) {
        if error.code().kind() == ErrorCodeKind::FsNetworkUnavailable
            && let Some(root) = self.radice_di(&file.path)
            && !self.files.radice_raggiungibile(root)
        {
            abbandonate.push(root.to_owned());
            return;
        }
        // Tutto il resto — compreso un guasto di rete su una radice che alla
        // sonda risponde ancora, cioè un singhiozzo — resta quel che era: un
        // file che questa passata non ha letto, detto per nome.
        illeggibili.push(Unreadable {
            path: file.path.clone(),
            error,
        });
    }

    /// Come [`Scan::file_illeggibile`], per tutte le voci di un lavoro.
    ///
    /// La sonda la paga la prima voce sola: i brani di un'immagine sono lo
    /// stesso file, e chiedere quattordici volte alla radice come sta per lo
    /// stesso guasto vorrebbe dire quattordici sonde. Se la radice è morta, le
    /// altre voci non si elencano — non sono illeggibili, non si sono potute
    /// guardare —; se è viva, si elencano con lo stesso perché.
    fn lavoro_illeggibile(
        &self,
        voci: &[(DiscoveredFile, Destinazione)],
        error: &AppError,
        abbandonate: &mut Vec<String>,
        illeggibili: &mut Vec<Unreadable>,
    ) {
        let Some(((primo, _), resto)) = voci.split_first() else {
            return;
        };
        let morte_prima = abbandonate.len();
        self.file_illeggibile(primo, error.clone(), abbandonate, illeggibili);
        if abbandonate.len() > morte_prima {
            return;
        }
        for (file, _) in resto {
            illeggibili.push(Unreadable {
                path: file.path.clone(),
                error: error.clone(),
            });
        }
    }

    /// La fase dei fogli: legge i `.cue` cambiati, ricorda gli altri, e mette
    /// i brani d'immagine al posto delle loro immagini.
    ///
    /// # Senza il lucchetto
    ///
    /// La cache arriva da fuori, letta da chi chiama con una presa breve; qui
    /// si legge solo il disco. I fogli da leggere sono solo quelli nuovi o
    /// cambiati — dimensione o data diverse da quelle ricordate — e si leggono
    /// in parallelo, ognuno con la sua scadenza, al più [`FOGLIO_MASSIMO`]
    /// byte.
    ///
    /// # Un foglio che non si legge
    ///
    /// Su una rete che singhiozza un `.cue` può non arrivare, e il danno non
    /// sarebbe piccolo: l'immagine tornerebbe un brano unico per una passata —
    /// o passerebbe al foglio gemello — e i suoi brani risulterebbero spariti.
    /// Così no:
    ///
    /// - **se la cache lo ricorda**, vale quel che diceva, con la dimensione e
    ///   la data **di allora**: le voci virtuali tornano identiche a quelle
    ///   scritte in libreria, e nessun brano si rilegge con un foglio vecchio
    ///   che poi nessuno correggerebbe più — alla passata dopo, letto davvero,
    ///   il foglio porta la sua data nuova e i brani si rileggono;
    /// - **se non lo ricorda**, la musica della sua cartella non si giudica: i
    ///   file audio lì accanto escono da questa passata — uno di loro è
    ///   l'immagine, e non si sa quale — e la cartella diventa un ramo perso,
    ///   quindi niente di quel che ci stava si toglie.
    ///
    /// In tutti e due i casi la cartella entra fra i rami persi, che è quel che
    /// è: un pezzo di radice su cui questa passata non ha potuto dire tutto.
    ///
    /// `con_brani` sono le chiavi dei `.cue` che hanno già brani in libreria:
    /// fra due fogli per la stessa immagine vince il loro. Vedi
    /// [`fogli_con_brani`].
    fn espandi_i_fogli(
        &self,
        ricognizione: &mut Ricognizione,
        cache: &std::collections::HashMap<String, FoglioRicordato>,
        con_brani: &std::collections::HashSet<String>,
    ) -> Fogli {
        use rymembra_domain::cue::{FoglioTrovato, espandi_immagini, leggi_foglio};
        use std::io::Read as _;

        let mut esito = Fogli::default();
        let mut trovati_fogli: Vec<FoglioTrovato> = Vec::new();
        let mut da_leggere: Vec<(DiscoveredFile, String)> = Vec::new();
        for file in &ricognizione.trovati {
            if extension_of(&file.path) != "cue" {
                continue;
            }
            let chiave = path_key(&file.path, self.rules);
            match cache.get(&chiave) {
                Some(ricordato)
                    if ricordato.dimensione == file.size_bytes
                        && ricordato.modificato_ms == file.modified_ms =>
                {
                    if let Some(foglio) = ricordato.foglio.clone() {
                        trovati_fogli.push(FoglioTrovato {
                            cue: file.clone(),
                            foglio,
                        });
                    }
                }
                _ => da_leggere.push((file.clone(), chiave)),
            }
        }

        let mut senza_foglio: Vec<String> = Vec::new();
        if !da_leggere.is_empty() {
            let squadra =
                Squadra::<Result<Option<Foglio>, AppError>>::nuova("fogli", LETTURE_IN_VOLO);
            let mut affidati = 0_usize;
            let mut tornati = 0_usize;
            while tornati < da_leggere.len() {
                while affidati < da_leggere.len() && affidati - tornati < LETTURE_IN_VOLO {
                    let Some((file, _)) = da_leggere.get(affidati) else {
                        break;
                    };
                    let disco = Arc::clone(&self.files);
                    let percorso = file.path.clone();
                    // L'attesa della radice del foglio: un `.cue` del cloud è
                    // una richiesta HTTPS, non una lettura del disco.
                    let scadenza = self.scadenza_per(&self.limiti_di(&file.path));
                    squadra.affida(
                        affidati,
                        scadenza,
                        Box::new(move || {
                            let mut lettore = disco.open(&percorso)?;
                            let mut byte = Vec::new();
                            std::io::Read::take(&mut lettore, FOGLIO_MASSIMO.saturating_add(1))
                                .read_to_end(&mut byte)
                                .map_err(|err| crate::files::io_error(&percorso, &err))?;
                            if u64::try_from(byte.len()).unwrap_or(u64::MAX) > FOGLIO_MASSIMO {
                                return Ok(None);
                            }
                            Ok(leggi_foglio(&byte))
                        }),
                    );
                    affidati += 1;
                }
                let Some((posto, letto)) = squadra.prossimo(BATTITO) else {
                    if self.fermato() {
                        esito.fermata = true;
                        return esito;
                    }
                    continue;
                };
                tornati += 1;
                let Some((cue, chiave)) = da_leggere.get(posto) else {
                    continue;
                };
                match letto {
                    Ok(Ok(foglio)) => {
                        if let Some(foglio) = foglio.clone() {
                            trovati_fogli.push(FoglioTrovato {
                                cue: cue.clone(),
                                foglio,
                            });
                        }
                        esito.letti.push(FoglioLetto {
                            chiave: chiave.clone(),
                            dimensione: cue.size_bytes,
                            modificato_ms: cue.modified_ms,
                            foglio,
                        });
                    }
                    // Un guasto, una scadenza, un parser che cade: il foglio
                    // non si sa. Vedi la doc qui sopra.
                    Ok(Err(_)) | Err(_) => {
                        let cartella =
                            cartella_che_contiene(&cue.path).unwrap_or_else(|| cue.path.clone());
                        match cache.get(chiave) {
                            Some(ricordato) => {
                                if let Some(foglio) = ricordato.foglio.clone() {
                                    trovati_fogli.push(FoglioTrovato {
                                        cue: DiscoveredFile {
                                            path: cue.path.clone(),
                                            size_bytes: ricordato.dimensione,
                                            modified_ms: ricordato.modificato_ms,
                                        },
                                        foglio,
                                    });
                                }
                            }
                            None => senza_foglio.push(path_key(&cartella, self.rules)),
                        }
                        ricognizione.rami_persi.push(cartella);
                    }
                }
            }
        }

        // La musica accanto a un foglio che non si è letto e che nessuno
        // ricorda: una di quelle è l'immagine, e non si sa quale.
        if !senza_foglio.is_empty() {
            ricognizione.trovati.retain(|file| {
                !is_supported_audio_path(&file.path)
                    || !cartella_che_contiene(&file.path).is_some_and(|cartella| {
                        senza_foglio.contains(&path_key(&cartella, self.rules))
                    })
            });
        }

        if trovati_fogli.is_empty() {
            // Niente da espandere, ma i file veri dal nome che sembra un
            // brano d'immagine vanno buttati lo stesso: vedi `espandi_immagini`.
            ricognizione
                .trovati
                .retain(|file| brano_d_immagine(&file.path).is_none());
            return esito;
        }
        let espansione = espandi_immagini(
            std::mem::take(&mut ricognizione.trovati),
            trovati_fogli,
            con_brani,
            self.rules,
        );
        ricognizione.trovati = espansione.trovati;
        esito.brani = espansione
            .abbinamenti
            .iter()
            .map(|abbinamento| abbinamento.foglio.tracce.len())
            .sum();
        esito.abbinamenti = espansione.abbinamenti;
        esito
    }

    /// La guardia per radice, solo nelle scansioni prudenti.
    ///
    /// Una radice vista che in libreria ha almeno [`RADICE_SOSPETTA`] righe e
    /// sul disco **zero** file di musica vale «saltata»: va in
    /// `radici_saltate`, esce dalle viste, e niente di quel che ci stava si
    /// toglie. È il punto di mount che risponde vuoto perché il disco sotto non
    /// si è montato, la condivisione che si ricollega con una cartella vuota al
    /// posto di quella vera: la sonda dice sì, la camminata dice niente, e senza
    /// questa guardia una scansione partita da sola — al risveglio, al ritorno
    /// della rete — toglierebbe la radice intera senza che nessuno la guardi.
    ///
    /// Solo nelle prudenti, perché nelle altre qualcuno sta guardando: la
    /// guardia contro le stragi gli chiede conferma, e se la radice l'ha
    /// svuotata davvero può dire di sì.
    ///
    /// # Solo sulle cartelle sorvegliate
    ///
    /// Vale per le radici della passata che **sono** cartelle sorvegliate —
    /// [`Scan::radici_della_libreria`], per chiave di percorso — e non per le
    /// sottocartelle che la sorveglianza e i download guardano. Un punto di
    /// mount che cade svuota la cartella sorvegliata, non un album dentro; una
    /// sottocartella vuota con cinquanta brani noti è invece il caso di tutti i
    /// giorni: l'album spostato per intero, che la sorveglianza manda come «la
    /// cartella di prima e quella di adesso». Saltare la prima voleva dire che
    /// i suoi brani non sparivano, quindi non si appaiavano con quelli comparsi
    /// nell'altra: entravano doppi, e il riallineamento dopo toglieva i vecchi
    /// con ascolti, voti e playlist.
    ///
    /// Una sottocartella che risponde vuota per un guasto resta protetta dalla
    /// guardia contro le stragi, come ogni altra passata.
    fn guardia_per_radice(&self, ricognizione: &mut Ricognizione, known: &[KnownTrack]) {
        let e_musica =
            |path: &str| is_supported_audio_path(path) || brano_d_immagine(path).is_some();
        let sorvegliate: std::collections::HashSet<String> = self
            .radici_della_libreria
            .iter()
            .map(|radice| path_key(radice, self.rules))
            .collect();
        let sospette: Vec<String> = ricognizione
            .viste
            .iter()
            .filter(|radice| sorvegliate.contains(&path_key(radice, self.rules)))
            .filter(|radice| {
                let note = known
                    .iter()
                    .filter(|riga| is_under(&riga.path, radice, self.rules))
                    .take(RADICE_SOSPETTA)
                    .count();
                note >= RADICE_SOSPETTA
                    && !ricognizione.trovati.iter().any(|file| {
                        e_musica(&file.path) && is_under(&file.path, radice, self.rules)
                    })
            })
            .cloned()
            .collect();
        for radice in sospette {
            ricognizione.viste.retain(|vista| *vista != radice);
            ricognizione
                .trovati
                .retain(|file| !is_under(&file.path, &radice, self.rules));
            ricognizione.radici_vuote.push(radice.clone());
            ricognizione.radici_saltate.push(radice);
        }
    }

    /// Il piano su quel che si è visto: la guardia per radice, la decisione
    /// del dominio, e i rami persi tolti dalle rimozioni.
    ///
    /// Lo stesso per [`plan`] e per [`Scan::run_con`]: un'anteprima che
    /// decidesse in un altro modo direbbe «sto per togliere» una cosa e poi ne
    /// farebbe un'altra.
    ///
    /// `immagini` e `abbinamenti` servono a riprendere i brani d'immagine la
    /// cui immagine ha cambiato nome: vedi [`immagini_rinominate`].
    fn pianifica(
        &self,
        ricognizione: &mut Ricognizione,
        known: &[KnownTrack],
        immagini: &std::collections::HashMap<i64, String>,
        abbinamenti: &[Abbinamento],
    ) -> (ScanPlan, usize) {
        if self.prudente {
            self.guardia_per_radice(ricognizione, known);
        }
        let mut piano = plan_scan(
            ScanInput {
                roots: &ricognizione.viste,
                found: &ricognizione.trovati,
                known,
            },
            self.rules,
        );
        immagini_rinominate(
            &mut piano,
            ricognizione,
            known,
            immagini,
            abbinamenti,
            self.rules,
        );
        let non_verificati =
            risparmia_i_non_verificati(&mut piano, &ricognizione.rami_persi, self.rules);
        (piano, non_verificati)
    }

    /// Gestisce l'esito di un lavoro, **nell'ordine dei lavori**: la logica di
    /// sempre, per un file o per tutti i brani di un'immagine.
    fn gestisci<'s>(
        &'s self,
        stato: &mut StatoLettura<'s>,
        lavoro: &Lavoro,
        esito: EsitoLettura,
        report: &mut ScanReport,
        lotto: &mut LottoLetto,
    ) {
        let radice = self.radice_di(lavoro.percorso());
        stato.fatti = stato.fatti.saturating_add(lavoro.voci.len());
        let Some(esito) = esito else {
            return;
        };
        // Un risultato da una radice abbandonata nel frattempo — mentre questo
        // era in volo, un altro ha scoperto che la rete se n'è andata — vale
        // quanto non averlo tentato: niente sonda, niente elenco.
        if radice.is_some_and(|root| stato.abbandonate.iter().any(|a| a == root)) {
            return;
        }
        match esito {
            Ok(Ok(righe)) => {
                // La radice ha risposto: la serie di scadenze, se ce n'era una
                // cominciata, non è una serie.
                if let Some(root) = radice {
                    stato.scadenze_di_fila.remove(root);
                    stato.fiducia.insert(root);
                }
                for (mut row, (file, destinazione)) in righe.into_iter().zip(&lavoro.voci) {
                    // Si prende invece di clonarla: la riga sta per essere
                    // consegnata al lotto, e da lì in poi la copertina persa
                    // non serve più a nessuno.
                    if let Some(persa) = row.copertina_persa.take() {
                        report.cover_failures.push(Unreadable {
                            path: file.path.clone(),
                            error: persa,
                        });
                    }
                    if let Some(cover) = row.cover.as_ref() {
                        if cover.already_present {
                            report.covers_reused += 1;
                        } else {
                            report.covers_stored += 1;
                        }
                    }
                    lotto.riusciti.push(path_key(&file.path, self.rules));
                    lotto.righe.push((row, *destinazione));
                }
            }
            // Il servizio chiede di rallentare: la lettura si rimanda, e con
            // lei tutte quelle di questa sorgente in questo giro. Niente
            // scadenze di fila, niente illeggibili, niente memoria: vedi
            // [`chiede_di_rallentare`].
            Ok(Err(error)) if chiede_di_rallentare(&error) => {
                let radice = radice.unwrap_or("");
                stato.frenate.insert(StatoLettura::fonte(radice).to_owned());
                stato.rimanda(radice, lavoro.voci.len(), report);
            }
            Ok(Err(error)) if rifiuto_deterministico(&error) => {
                // Il file è arrivato, e il lettore ha detto di no: la radice
                // risponde, e il no si ricorda — vedi la migrazione 032.
                if let Some(root) = radice {
                    stato.fiducia.insert(root);
                }
                // Una riga provvisoria se ne va, come un file che non era mai
                // entrato: vedi `togli_provvisorie`, che la tiene se qualcuno
                // ci ha messo le mani.
                for (file, destinazione) in &lavoro.voci {
                    if let Destinazione::Provvisoria(id) = destinazione {
                        lotto.provvisorie_rifiutate.push((*id, file.path.clone()));
                    }
                }
                let causa = error.to_string();
                for (file, _) in &lavoro.voci {
                    lotto.rifiutati.push(RifiutoNuovo {
                        chiave: path_key(&file.path, self.rules),
                        dimensione: i64::try_from(file.size_bytes).unwrap_or(i64::MAX),
                        modificato_ms: file.modified_ms,
                        causa: causa.clone(),
                    });
                    report.unreadable.push(Unreadable {
                        path: file.path.clone(),
                        error: error.clone(),
                    });
                }
            }
            Ok(Err(error)) => {
                if let Some(root) = radice
                    && error.code().kind() == ErrorCodeKind::FsNetworkUnavailable
                {
                    stato.fiducia.remove(root);
                }
                self.lavoro_illeggibile(
                    &lavoro.voci,
                    &error,
                    &mut stato.abbandonate,
                    &mut report.unreadable,
                );
            }
            // Il lettore dei tag è caduto su questo file: `lofty` davanti a
            // un'intestazione malformata, un `assert!` di terzi. Un file
            // illeggibile, detto per nome, e nient'altro: **non** una scadenza,
            // e non conta verso l'abbandono della radice. Fino alla 2.4 lo era,
            // e tre file rotti di fila in una cartella del disco di casa la
            // facevano abbandonare come «rete non disponibile».
            //
            // Anzi: un file che fa cadere il lettore è un file che è
            // **arrivato**, byte per byte. La radice ha risposto, e la serie di
            // scadenze — se ce n'era una — si interrompe come dopo una lettura
            // riuscita. Non si ricorda: una caduta non è una risposta del
            // lettore, ed è giusto riprovare.
            Err(Mancato::Caduto { messaggio }) => {
                if let Some(root) = radice {
                    stato.scadenze_di_fila.remove(root);
                    stato.fiducia.insert(root);
                }
                for (file, _) in &lavoro.voci {
                    report.unreadable.push(Unreadable {
                        path: file.path.clone(),
                        error: caduto(&file.path, messaggio.as_deref()),
                    });
                }
            }
            // Non è tornato in tempo: il filo è ancora dentro la `open`, e da
            // qui in poi non lo si aspetta più. Vale un guasto di rete, e come
            // tale fa scattare la sonda della radice — perché la domanda è la
            // stessa: è lento questo file, o è morta la cartella?
            Err(Mancato::Scaduto) => {
                if let Some(root) = radice {
                    stato.fiducia.remove(root);
                }
                let limiti = stato.limiti_di(radice);
                self.lavoro_illeggibile(
                    &lavoro.voci,
                    &scaduto(lavoro.percorso(), self.scadenza_per(&limiti)),
                    &mut stato.abbandonate,
                    &mut report.unreadable,
                );
                // E poi la seconda domanda, quella che la sonda non fa: non «la
                // radice risponde?» ma «la radice serve a qualcosa?». Alla terza
                // scadenza di fila la risposta è no, e insistere costerebbe una
                // scadenza e una sonda per ognuno dei file rimasti.
                if let Some(root) = radice {
                    let di_fila = stato.scadenze_di_fila.entry(root).or_insert(0);
                    *di_fila += 1;
                    if *di_fila >= limiti.scadenze_di_fila
                        && !stato.abbandonate.iter().any(|a| a == root)
                    {
                        stato.abbandonate.push(root.to_owned());
                    }
                }
            }
        }
    }

    /// Esegue la scansione chiedendo la connessione una fase per volta.
    ///
    /// `on_progress` riceve `(fatti, totale)` sui soli file da leggere, che sono
    /// la parte lenta. Rimozioni e aggregati non entrano nel conteggio perché
    /// non hanno una durata percepibile.
    ///
    /// È [`Scan::run_con`] con l'osservatore di prima: le
    /// [`Avanzamento::Lettura`] vanno a `on_progress`, le
    /// [`Avanzamento::Camminata`] non le chiede nessuno.
    ///
    /// # Le fasi, e quali non toccano il deposito
    ///
    /// I numeri sono quelli che ritrovi nei commenti del corpo.
    ///
    /// 1. **La camminata.** Nessuna connessione in mano: è la fase che su una
    ///    share lenta dura minuti. Chi si ferma qui non ha deciso niente, e
    ///    infatti non scrive niente — nemmeno una presa.
    /// 1. ter. **I fogli `.cue`.** Una presa breve per la cache, se c'è almeno
    ///    un `.cue`; poi il disco, di nuovo senza niente in mano. Vedi
    ///    `espandi_i_fogli`.
    /// 2. **Il piano.** Una presa per leggere le righe note e la memoria dei
    ///    rifiuti, poi una funzione pura del dominio che decide.
    /// 3. **Le righe doppie, le sparizioni in sospeso e le due memorie.** Una
    ///    presa sola.
    /// 3. bis. **Le righe provvisorie**, per le radici che le chiedono
    ///    ([`Limiti::prima_le_righe`]): una presa per lotto, prima di leggere
    ///    qualunque file. Vedi [`riga_provvisoria`].
    /// 4. **La lettura dei file.** Di nuovo niente in mano, fino a
    ///    [`LETTURE_IN_VOLO`] file insieme, ognuno con una scadenza addosso: è
    ///    l'altra fase lunga.
    /// 5. **La scrittura di ogni lotto.** Una presa per lotto, una transazione
    ///    dentro.
    /// 6. **Le rimozioni rimaste.** Una presa.
    /// 7. **Gli aggregati.** Una presa, e solo per gli album e gli artisti che
    ///    le fasi 3, 5 e 6 hanno toccato: vedi [`ricostruisci_aggregati`].
    ///
    /// # Cosa può cambiare sotto i piedi
    ///
    /// Lasciando il deposito fra una fase e l'altra si accetta che il database
    /// cambi in mezzo. Le conseguenze sono quattro, e sono tutte trattate:
    ///
    /// - **Gli identificativi si riusano.** SQLite riassegna i numeri liberi:
    ///   la riga 412 che il piano voleva togliere può essere stata cancellata e
    ///   rimpiazzata da un'altra. Per questo si cancella per identificativo
    ///   **e** percorso — vedi `remove_tracks`.
    /// - **Una riga può essere sparita.** Non è un errore: `content_keys_of` la
    ///   salta, una cancellazione che non trova niente conta zero, e la passata
    ///   dopo decide da capo.
    /// - **`tracks.path` è UNIQUE.** Se un'altra scansione stesse inserendo gli
    ///   stessi file, un inserimento potrebbe fallire. È il motivo per cui
    ///   l'esclusione di una seconda scansione spetta a chi le avvia: qui non
    ///   c'è più il lucchetto tenuto tutto il tempo a garantirla per sbaglio.
    /// - **Le copertine no.** Lo store è indirizzato per contenuto e si scrive
    ///   in modo atomico: due scansioni che salvano la stessa immagine scrivono
    ///   lo stesso file, e `INSERT OR IGNORE` fa il resto.
    ///
    /// # Fermarsi
    ///
    /// Durante la camminata — e durante la lettura dei fogli — risponde
    /// [`Scan::fermati`]: non si è ancora deciso niente, quindi non si scrive
    /// niente e l'esito è un `cancelled` con il piano vuoto.
    ///
    /// Durante la lettura rispondono in due, e basta uno: `on_progress` che
    /// restituisce [`ControlFlow::Break`], o la stessa [`Scan::fermati`]. La
    /// scansione si ferma allora **all'esito che sta gestendo**: il lotto
    /// chiude la sua transazione con le righe che ha già gestito, e lì si
    /// smette. Le letture ancora in volo non si aspettano — su una share dove
    /// ogni apertura costa mezzo minuto sarebbero mezzo minuto fra l'Annulla e
    /// il momento in cui succede qualcosa — e quel che è stato scritto resta.
    ///
    /// Non è un annullamento nel senso di «disfare» — è «basta così»: la passata
    /// dopo trova una libreria giusta e incompleta e la finisce da sé. È il
    /// contrario di come si annulla di solito, e il motivo sta nella forma della
    /// scansione: ogni lotto sta nella sua transazione, apposta perché
    /// un'interruzione qualunque — un crollo, una chiusura, una macchina che si
    /// spegne — non lasci niente a metà. Un annullamento è solo l'interruzione
    /// che qualcuno ha chiesto, e trattarla diversamente vorrebbe dire scrivere
    /// un secondo percorso per lo stesso caso.
    pub fn run_su<D: Deposito>(
        &self,
        deposito: &mut D,
        mut on_progress: impl FnMut(usize, usize) -> ControlFlow<()>,
    ) -> Result<ScanReport, AppError> {
        self.run_con(deposito, |avanzamento| match avanzamento {
            Avanzamento::Lettura { fatti, totale } => on_progress(fatti, totale),
            Avanzamento::Camminata { .. } => ControlFlow::Continue(()),
        })
    }

    /// Come [`Scan::run_su`], con un osservatore più ricco: racconta anche la
    /// camminata. Vedi [`Avanzamento`].
    ///
    /// Un [`ControlFlow::Break`] durante la camminata vale un Annulla durante
    /// la camminata: niente scritto, `cancelled` e piano vuoto.
    ///
    /// # Le letture in parallelo
    ///
    /// Fino a [`LETTURE_IN_VOLO`] lavori insieme, sulla [`Squadra`]; gli esiti
    /// si gestiscono **in ordine**, sul filo di chi coordina, con la logica di
    /// sempre: abbandoni, scadenze di fila, sonda, copertine, avanzamento,
    /// annullamento. L'ordine conta perché è quello che dà un senso a «tre
    /// scadenze di fila», e perché un lotto interrotto deve aver scritto un
    /// prefisso dei suoi file, non quelli che per caso sono arrivati prima.
    ///
    /// **Una radice si guadagna il parallelo.** Finché non ha consegnato un
    /// file — all'inizio, e di nuovo dopo una scadenza o un guasto di rete —
    /// su di lei si legge un file alla volta: sulla share che muore a metà è
    /// il primo file a pagare l'attesa e a scoprirlo per tutti, invece di
    /// quattro scadenze insieme e quattro fili appesi. Le altre radici, intanto,
    /// leggono quanto possono. Una sorgente che non ha bisogno di questa
    /// prudenza — il cloud, sempre acceso — lo dice coi suoi limiti
    /// ([`Limiti::prima_uno_alla_volta`]), insieme a quanti file in volo, per
    /// quanto aspettarli e ogni quante righe scrivere.
    ///
    /// Fra un lotto e l'altro la squadra si svuota: si scrive solo quando non
    /// c'è più niente in volo, così il lucchetto non è mai in mano a qualcuno
    /// mentre un file si sta aprendo. Costa la coda di un lotto ogni
    /// cinquecento file.
    ///
    /// # Errori
    ///
    /// `db.*` se una presa del deposito fallisce. I file che non si leggono non
    /// sono errori: vedi [`ScanReport::unreadable`].
    pub fn run_con<D: Deposito>(
        &self,
        deposito: &mut D,
        mut avanzamento: impl FnMut(Avanzamento) -> ControlFlow<()>,
    ) -> Result<ScanReport, AppError> {
        let started = Instant::now();
        let mut tempi = TempiScansione::default();

        // ── 1. il disco, senza niente in mano ──
        let inizio = Instant::now();
        let mut ricognizione = cammina_con(self, &mut |cartelle, file| {
            avanzamento(Avanzamento::Camminata { cartelle, file })
        });
        tempi.camminata_ms = millisecondi(inizio);
        let cartelle_viste = ricognizione.cartelle;
        let file_visti = ricognizione.trovati.len();

        // ── 1 bis. chi si è fermato camminando non ha deciso niente ──
        //
        // E quindi non scrive niente: nessuna presa del deposito, nessuna
        // transazione. Il piano resta vuoto perché un piano calcolato su mezza
        // camminata direbbe «spariti» dei brani che stanno sotto radici che non
        // si è fatto in tempo a guardare.
        let fermata = |ricognizione: Ricognizione, tempi: TempiScansione| ScanReport {
            cancelled: true,
            radici_saltate: ricognizione.radici_saltate,
            rami_persi: ricognizione.rami_persi,
            elapsed_ms: started.elapsed().as_millis(),
            tempi,
            cartelle_viste,
            file_visti,
            ..ScanReport::default()
        };
        if ricognizione.fermata {
            return Ok(fermata(ricognizione, tempi));
        }

        // ── 1 ter. i fogli `.cue`: la cache con una presa breve, il disco senza ──
        let inizio = Instant::now();
        let ci_sono_fogli = ricognizione
            .trovati
            .iter()
            .any(|file| extension_of(&file.path) == "cue");
        let (cache_fogli, con_brani) = if ci_sono_fogli {
            deposito.con_connessione(|connection| {
                Ok((
                    fogli_ricordati(connection)?,
                    fogli_con_brani(connection, self.rules)?,
                ))
            })?
        } else {
            (
                std::collections::HashMap::new(),
                std::collections::HashSet::new(),
            )
        };
        let fogli = self.espandi_i_fogli(&mut ricognizione, &cache_fogli, &con_brani);
        tempi.fogli_ms = millisecondi(inizio);
        if fogli.fermata {
            return Ok(fermata(ricognizione, tempi));
        }

        // ── 2. le righe note, la memoria dei rifiuti, e il piano ──
        let inizio = Instant::now();
        let (known, rifiuti, fogli_in_cache, immagini_note, provvisorie) = deposito
            .con_connessione(|connection| {
                Ok((
                    known_tracks(connection)?,
                    rifiuti_ricordati(connection)?,
                    chiavi_dei_fogli(connection)?,
                    immagini_dei_brani(connection)?,
                    righe_provvisorie(connection)?,
                ))
            })?;
        // ── 2 bis. quel che sta sotto un ramo perso non è sparito ──
        //
        // Dentro `pianifica`, prima di tutto il resto, perché da qui in poi il
        // piano decide: le sparizioni diventano candidate agli spostamenti e
        // poi rimozioni. Una riga sotto una sottocartella illeggibile non deve
        // arrivare a nessuna delle due — non è stata vista, e non vederla non
        // vuol dire niente.
        let (piano, non_verificati) = self.pianifica(
            &mut ricognizione,
            &known,
            &immagini_note,
            &fogli.abbinamenti,
        );

        // Le due memorie si potano dove si è guardato: una riga di un file che
        // sotto una radice vista — e fuori dai rami persi — non c'è più, è una
        // riga di niente. Si confrontano con i trovati **dopo** l'espansione,
        // perché i rifiuti di un'immagine sono ricordati per brano.
        let da_potare = {
            let visti: std::collections::HashSet<String> = ricognizione
                .trovati
                .iter()
                .map(|file| path_key(&file.path, self.rules))
                .collect();
            let potabile = |chiave: &str| {
                !visti.contains(chiave)
                    && ricognizione
                        .viste
                        .iter()
                        .any(|radice| is_under(chiave, radice, self.rules))
                    && !ricognizione
                        .rami_persi
                        .iter()
                        .any(|ramo| is_under(chiave, ramo, self.rules))
            };
            let rifiuti: Vec<String> = rifiuti
                .keys()
                .filter(|chiave| potabile(chiave))
                .cloned()
                .collect();
            // Dalle chiavi lette adesso e non dalla cache della fase dei fogli:
            // quella si legge solo se c'è almeno un `.cue`, e quando l'ultimo
            // se n'è andato è proprio la sua riga che va tolta.
            let fogli: Vec<String> = fogli_in_cache
                .into_iter()
                .filter(|chiave| potabile(chiave))
                .collect();
            (rifiuti, fogli)
        };

        let mut report = ScanReport {
            plan: piano,
            radici_saltate: ricognizione.radici_saltate,
            radici_vuote: ricognizione.radici_vuote,
            non_verificati,
            rami_persi: ricognizione.rami_persi,
            cartelle_viste,
            file_visti,
            brani_d_immagine: fogli.brani,
            ..ScanReport::default()
        };
        tempi.piano_ms = millisecondi(inizio);

        // ── 3. una presa sola: le doppie se ne vanno, le sparizioni si tengono ──
        //
        // Due righe che nominano lo stesso file: quella che sopravvive sta per
        // ricevere il percorso come la camminata l'ha scritto, e `tracks.path` è
        // UNIQUE — se la doppia fosse ancora lì, l'aggiornamento fallirebbe.
        // Non sono candidate a essere spostamenti: il loro file non si è mosso,
        // è la riga a essere di troppo.
        //
        // Le sparizioni invece restano in sospeso: qualcuna è uno spostamento, e
        // lo si scopre solo leggendo i file nuovi.
        //
        // E le due memorie: i fogli appena letti nella cache, le righe di file
        // che non ci sono più fuori da entrambe.
        let inizio = Instant::now();
        let doppie: Vec<(i64, String)> = report
            .plan
            .to_remove
            .iter()
            .filter(|r| r.reason == RemoveReason::DuplicateRow)
            .map(|r| (r.track.id, r.track.path.clone()))
            .collect();
        let spariti: Vec<i64> = report
            .plan
            .to_remove
            .iter()
            .filter(|r| r.reason == RemoveReason::Disappeared)
            .map(|r| r.track.id)
            .collect();
        // Quali di queste erano brani d'immagine: servono all'appaiamento degli
        // spostamenti, che non deve mai attraversare il confine fra un file
        // intero e un pezzo di file. Vedi più sotto.
        let spariti_d_immagine: std::collections::HashSet<i64> = report
            .plan
            .to_remove
            .iter()
            .filter(|r| {
                r.reason == RemoveReason::Disappeared && brano_d_immagine(&r.track.path).is_some()
            })
            .map(|r| r.track.id)
            .collect();
        // Gli album e gli artisti che questa passata tocca, fase per fase: alla
        // fine si rifanno quelli e basta. Vedi [`ricostruisci_aggregati`].
        //
        // Vivono solo qui, in memoria: per questo ogni presa che scrive, da
        // qui alla fase 7, se cade passa da [`riparo_dopo_una_caduta`] prima
        // di dire l'errore.
        let mut toccati = Toccati::nessuno();
        let (tolte, mut sospese) = deposito
            .con_connessione(|connection| {
                let mut tolte = 0;
                if !doppie.is_empty() {
                    let tx = transazione_di_scrittura(connection)
                        .map_err(|err| db_error("apertura della transazione", &err))?;
                    let elenco: Vec<(i64, &str)> = doppie
                        .iter()
                        .map(|(id, path)| (*id, path.as_str()))
                        .collect();
                    let quali: Vec<i64> = doppie.iter().map(|(id, _)| *id).collect();
                    toccati.segna_brani(&tx, &quali)?;
                    tolte = remove_tracks(&tx, &elenco)?;
                    tx.commit()
                        .map_err(|err| db_error("chiusura della transazione", &err))?;
                }
                if !fogli.letti.is_empty() || !da_potare.0.is_empty() || !da_potare.1.is_empty() {
                    let tx = transazione_di_scrittura(connection)
                        .map_err(|err| db_error("apertura della transazione", &err))?;
                    aggiorna_le_memorie(&tx, &fogli.letti, &da_potare.0, &da_potare.1)?;
                    tx.commit()
                        .map_err(|err| db_error("chiusura della transazione", &err))?;
                }
                let sospese = content_keys_of(connection, &spariti)?;
                Ok((tolte, sospese))
            })
            .map_err(|err| riparo_dopo_una_caduta(deposito, &toccati, err))?;
        report.removed += tolte;
        tempi.scrittura_ms = tempi.scrittura_ms.saturating_add(millisecondi(inizio));

        // ── quel che va letto, in un elenco solo ──
        // Inserimenti e aggiornamenti differiscono solo per la presenza di una
        // riga da riusare; leggerli insieme tiene i lotti pieni anche quando gli
        // uni sono pochi e gli altri tanti.
        //
        // Senza i file che il lettore ha già rifiutato così come sono: il piano
        // li conta lo stesso — fra gli inserimenti o gli aggiornamenti, che è
        // quel che sarebbero — e la guardia contro le stragi somma su quei
        // conti, quindi toglierli da lì cambierebbe il suo denominatore.
        let mut da_leggere: Vec<(DiscoveredFile, Destinazione)> =
            Vec::with_capacity(report.plan.to_insert.len() + report.plan.to_update.len());
        let ricordato = |file: &DiscoveredFile| {
            rifiuti
                .get(&path_key(&file.path, self.rules))
                .is_some_and(|rifiuto| rifiuto.vale_per(file))
        };
        for file in &report.plan.to_insert {
            if ricordato(file) {
                report.illeggibili_noti += 1;
            } else {
                da_leggere.push((file.clone(), Destinazione::Nuova));
            }
        }
        for pending in &report.plan.to_update {
            if ricordato(&pending.file) {
                report.illeggibili_noti += 1;
            } else {
                // Una riga provvisoria di un giro di prima: la lettura la
                // completa, e un rifiuto la toglie. Vedi `Destinazione`.
                let destinazione = if provvisorie.contains(&pending.track_id) {
                    Destinazione::Provvisoria(pending.track_id)
                } else {
                    Destinazione::Aggiorna(pending.track_id)
                };
                da_leggere.push((pending.file.clone(), destinazione));
            }
        }

        let mut stato = StatoLettura {
            abbandonate: Vec::new(),
            scadenze_di_fila: std::collections::HashMap::new(),
            fiducia: std::collections::HashSet::new(),
            fatti: 0,
            limiti: self
                .roots
                .iter()
                .map(|root| (root.as_str(), self.files.limiti(root)))
                .collect(),
            letture: std::collections::HashMap::new(),
            rimandate: Vec::new(),
            frenate: std::collections::HashSet::new(),
        };

        // ── 3 bis. le righe prima dei tag: la prima passata del cloud ──
        //
        // I file nuovi di una radice che lo chiede (`Limiti::prima_le_righe`)
        // entrano adesso, senza lettura, dalla stessa via di tutti gli altri:
        // `insert_track` — quindi la generazione, l'indice di ricerca — e gli
        // aggregati alla fine. La lettura dopo li completa sul posto.
        //
        // **Non** se il piano ha delle sparizioni: un file nuovo può essere
        // un file spostato, e lo si scopre solo leggendolo e confrontando la
        // chiave di contenuto con quelle delle righe sparite
        // (`match_moved_tracks`). Una riga provvisoria entrata prima non
        // potrebbe più reclamare la riga vecchia, e ascolti, voti e playlist
        // se ne andrebbero con lei. Allora si legge come sempre.
        //
        // Le sparizioni di **tutta** la passata, non della radice del file
        // nuovo: i cambiamenti di un servizio fanno riguardare la cartella
        // vecchia di un rinomina e quella nuova, due radici di questa
        // scansione, e il file sparito sta nell'una mentre quello nuovo sta
        // nell'altra. Guardando la radice sola, i brani di una sottocartella
        // rinominata nel cloud rientravano nuovi, e i vecchi se ne andavano
        // con la loro storia.
        //
        // Né i brani d'immagine CD, che vengono dal foglio e dall'immagine
        // insieme e si leggono in un colpo solo.
        let con_sparizioni = report
            .plan
            .to_remove
            .iter()
            .any(|r| r.reason == RemoveReason::Disappeared);
        let da_mettere: Vec<usize> = da_leggere
            .iter()
            .enumerate()
            .filter(|(_, (file, destinazione))| {
                *destinazione == Destinazione::Nuova
                    && !con_sparizioni
                    && brano_d_immagine(&file.path).is_none()
                    && self
                        .radice_di(&file.path)
                        .is_some_and(|radice| stato.limiti_di(Some(radice)).prima_le_righe)
            })
            .map(|(posto, _)| posto)
            .collect();
        if !da_mettere.is_empty() {
            let inizio = Instant::now();
            for pezzo in da_mettere.chunks(LOTTO) {
                let righe: Vec<(usize, TrackRow)> = pezzo
                    .iter()
                    .filter_map(|posto| {
                        da_leggere.get(*posto).map(|(file, _)| {
                            (
                                *posto,
                                riga_provvisoria(file, self.files.tag_noti(&file.path)),
                            )
                        })
                    })
                    .collect();
                let now = now_ms();
                let entrate = deposito
                    .con_connessione(|connection| {
                        let tx = transazione_di_scrittura(connection)
                            .map_err(|err| db_error("apertura della transazione", &err))?;
                        let mut entrate: Vec<(usize, i64)> = Vec::with_capacity(righe.len());
                        for (posto, riga) in &righe {
                            insert_track(&tx, riga, now)?;
                            let nuova = tx.last_insert_rowid();
                            crate::incerti::riapplica(&tx, nuova)?;
                            entrate.push((*posto, nuova));
                        }
                        let ids: Vec<i64> = entrate.iter().map(|(_, id)| *id).collect();
                        toccati.segna_brani(&tx, &ids)?;
                        tx.commit()
                            .map_err(|err| db_error("chiusura della transazione", &err))?;
                        Ok(entrate)
                    })
                    .map_err(|err| riparo_dopo_una_caduta(deposito, &toccati, err))?;
                report.inserted += entrate.len();
                report.provvisorie += entrate.len();
                for (posto, id) in entrate {
                    if let Some((_, destinazione)) = da_leggere.get_mut(posto) {
                        *destinazione = Destinazione::Provvisoria(id);
                    }
                }
            }
            tempi.scrittura_ms = tempi.scrittura_ms.saturating_add(millisecondi(inizio));
        }

        let immagini: std::collections::HashMap<String, Arc<Abbinamento>> = fogli
            .abbinamenti
            .into_iter()
            .map(|abbinamento| {
                (
                    path_key(&abbinamento.cue.path, self.rules),
                    Arc::new(abbinamento),
                )
            })
            .collect();
        let mut lavori: std::collections::VecDeque<Lavoro> =
            in_lavori(da_leggere, &immagini, self.rules).into();
        let totale: usize = lavori.iter().map(|lavoro| lavoro.voci.len()).sum();

        // Quante letture in volo, al più, su tutta la passata: quante ne chiede
        // la radice che ne vuole di più. Sul disco sono le quattro di sempre.
        let in_volo_massimo = stato
            .limiti
            .values()
            .map(|limiti| limiti.in_volo)
            .max()
            .unwrap_or(LETTURE_IN_VOLO)
            .max(1);
        // ── 4. la lettura, di nuovo senza niente in mano ──
        //
        // Quattro fili per tutte le letture (sul disco), riusati finché uno non
        // gli si appende addosso: centomila file non sono centomila fili.
        // Assunta solo se c'è qualcosa da leggere: una riscansione invariata
        // non costa nemmeno i suoi quattro fili.
        let squadra = (!lavori.is_empty())
            .then(|| Squadra::<Result<Vec<TrackRow>, AppError>>::nuova("lettura", in_volo_massimo));
        while let Some(squadra) = squadra.as_ref().filter(|_| !lavori.is_empty()) {
            let inizio = Instant::now();
            // Il lotto: lavori interi fino a [`LOTTO`] voci — o al lotto più
            // piccolo fra le radici che ci entrano —, così i brani di
            // un'immagine non si spezzano fra due transazioni.
            let mut lotto_lavori: Vec<Lavoro> = Vec::new();
            let mut voci = 0_usize;
            let mut tetto = usize::MAX;
            while let Some(prossimo) = lavori.front() {
                let radice = self.radice_di(prossimo.percorso());
                let limiti = stato.limiti_di(radice);
                tetto = tetto.min(limiti.lotto.max(1));
                if voci >= tetto {
                    break;
                }
                let Some(lavoro) = lavori.pop_front() else {
                    break;
                };
                // Il tetto di letture del giro, o un servizio che ha chiesto di
                // rallentare: il lavoro non entra nel lotto, si conta fra i
                // rimasti, e la passata dopo lo ritrova da leggere.
                let radice = radice.unwrap_or("");
                let lette = stato.letture.get(radice).copied().unwrap_or(0);
                let oltre = limiti.letture_per_giro.is_some_and(|tetto| lette >= tetto);
                if oltre || stato.frenata(radice) {
                    stato.fatti = stato.fatti.saturating_add(lavoro.voci.len());
                    stato.rimanda(radice, lavoro.voci.len(), &mut report);
                    continue;
                }
                if let Some(conto) = stato
                    .limiti
                    .get_key_value(radice)
                    .map(|(chiave, _)| *chiave)
                {
                    *stato.letture.entry(conto).or_insert(0) += lavoro.voci.len();
                }
                voci += lavoro.voci.len();
                lotto_lavori.push(lavoro);
            }
            // Tutto rimandato: niente da leggere né da scrivere in questo giro
            // di lotto.
            if lotto_lavori.is_empty() {
                continue;
            }
            let mut letto = LottoLetto::default();
            let mut esiti: std::collections::BTreeMap<usize, EsitoLettura> =
                std::collections::BTreeMap::new();
            let mut in_volo: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            let mut affidati = 0_usize;
            let mut gestiti = 0_usize;
            while gestiti < lotto_lavori.len() {
                // Prima si gestisce il prossimo nell'ordine, se è arrivato:
                // affidare prima vorrebbe dire decidere con quel che si
                // sapeva un esito fa — un secondo file mandato sulla radice
                // che il primo sta per dichiarare morta.
                if let Some(esito) = esiti.remove(&gestiti) {
                    let Some(lavoro) = lotto_lavori.get(gestiti) else {
                        break;
                    };
                    self.gestisci(&mut stato, lavoro, esito, &mut report, &mut letto);
                    gestiti += 1;
                    // Due modi di sentirsi dire «basta», e ne basta uno. Il
                    // callback parla per chi sta guardando la barra; la
                    // bandiera parla per tutti gli altri — l'uscita del
                    // programma, un chiamante che di avanzamento non sa che
                    // farsene e passa un `Continue` fisso.
                    let fatti = stato.fatti;
                    if avanzamento(Avanzamento::Lettura { fatti, totale }).is_break()
                        || self.fermato()
                    {
                        report.cancelled = true;
                        break;
                    }
                    continue;
                }

                // Poi si affida quel che si può, nell'ordine: una radice morta non
                // si tenta nemmeno — ogni apertura su una share che non
                // risponde costa la scadenza intera, e su una libreria vera i
                // file rimasti sono decine di migliaia —, una radice senza
                // fiducia un file per volta.
                //
                // E mai più di [`LETTURE_IN_VOLO`] lavori oltre l'ultimo
                // gestito (o quanti ne chiede la radice più ingorda), arrivati
                // o no: un esito che torna in anticipo libera un filo, ma non
                // dice ancora niente della radice, e affidare al suo posto
                // vorrebbe dire continuare a tentare una share che il primo
                // esito in coda sta per dichiarare morta.
                while affidati < gestiti.saturating_add(in_volo_massimo) {
                    let Some(lavoro) = lotto_lavori.get(affidati) else {
                        break;
                    };
                    let radice = self.radice_di(lavoro.percorso()).unwrap_or("");
                    if stato.abbandonate.iter().any(|a| a == radice) {
                        esiti.insert(affidati, None);
                        affidati += 1;
                        continue;
                    }
                    // Un servizio che ha appena chiesto di rallentare: non si
                    // insiste nemmeno sul resto del lotto.
                    if stato.frenata(radice) {
                        stato.rimanda(radice, lavoro.voci.len(), &mut report);
                        esiti.insert(affidati, None);
                        affidati += 1;
                        continue;
                    }
                    let limiti = stato.limiti_di(Some(radice));
                    // Un file alla volta finché la radice non ne ha consegnato
                    // uno — la prudenza di una share appena svegliata — se la
                    // sorgente la chiede; il cloud no, è sempre acceso.
                    let finestra = if stato.fiducia.contains(radice) || !limiti.prima_uno_alla_volta
                    {
                        limiti.in_volo.max(1)
                    } else {
                        1
                    };
                    let gia = in_volo.entry(radice).or_insert(0);
                    if *gia >= finestra {
                        break;
                    }
                    *gia += 1;
                    report.letti += lavoro.voci.len();
                    // Tutto quel che serve al filo se lo porta via: l'`Arc`
                    // dei file, la cartella delle copertine, le voci. Un filo
                    // che si appende sopravvive a questa iterazione, e non può
                    // tenere in mano niente che appartenga a `self`.
                    let disco = Arc::clone(&self.files);
                    let copertine = self.covers.clone();
                    let voci: Vec<DiscoveredFile> =
                        lavoro.voci.iter().map(|(file, _)| file.clone()).collect();
                    let immagine = lavoro.immagine.clone();
                    squadra.affida(
                        affidati,
                        self.scadenza_per(&limiti),
                        Box::new(move || match immagine {
                            Some(abbinamento) => {
                                read_image_tracks(disco.as_ref(), &copertine, &abbinamento, &voci)
                            }
                            None => voci
                                .first()
                                .map(|file| read_track(disco.as_ref(), &copertine, file))
                                .transpose()
                                .map(|riga| riga.into_iter().collect()),
                        }),
                    );
                    affidati += 1;
                }
                // Il prossimo da gestire era su una radice morta: è già qui,
                // senza aspettare niente.
                if esiti.contains_key(&gestiti) {
                    continue;
                }

                // Altrimenti si aspetta il prossimo esito, un battito per
                // volta, guardando se qualcuno ha chiesto di smettere.
                match squadra.prossimo(BATTITO) {
                    Some((posto, esito)) => {
                        if let Some(lavoro) = lotto_lavori.get(posto) {
                            let radice = self.radice_di(lavoro.percorso()).unwrap_or("");
                            if let Some(gia) = in_volo.get_mut(radice) {
                                *gia = gia.saturating_sub(1);
                            }
                        }
                        esiti.insert(posto, Some(esito));
                    }
                    None => {
                        if self.fermato() {
                            report.cancelled = true;
                            break;
                        }
                    }
                }
            }
            tempi.lettura_ms = tempi.lettura_ms.saturating_add(millisecondi(inizio));
            let LottoLetto {
                mut righe,
                rifiutati,
                riusciti,
                provvisorie_rifiutate,
            } = letto;
            // I rifiuti ricordati che non lo sono più: se ne va la riga.
            let guariti: Vec<String> = riusciti
                .into_iter()
                .filter(|chiave| rifiuti.contains_key(chiave))
                .collect();

            // ── quali di questi file nuovi sono righe che si sono spostate ──
            //
            // Decisione pura: nessun disco, nessun database.
            //
            // In due giri separati, e mai a cavallo: i file interi con i file
            // interi, i brani d'immagine con i brani d'immagine. Un disco intero
            // che diventa i suoi brani — la prima scansione con i `.cue`, o un
            // `.cue` aggiunto accanto a un'immagine — non è uno spostamento:
            // appaiato a uno dei suoi pezzi, gli ascolti, il voto e le playlist
            // di settanta minuti di musica finirebbero su un brano solo. E la
            // chiave lo permette davvero: un'immagine FLAC che porta nei tag il
            // titolo del secondo brano ha la stessa chiave di contenuto di quel
            // brano, ed è successo su cinque dischi della libreria vera. Il caso
            // contrario — un `.cue` tolto, e i brani che tornano un file solo —
            // è lo stesso confine percorso all'indietro.
            if !sospese.is_empty() {
                let mut appaiate: Vec<i64> = Vec::new();
                for d_immagine in [false, true] {
                    // Solo le righe con una chiave: una riga letta ce l'ha
                    // sempre, e le posizioni devono restare allineate alle
                    // chiavi, perché è per posizione che tornano gli abbinamenti.
                    let posizioni: Vec<usize> = righe
                        .iter()
                        .enumerate()
                        .filter(|(_, (row, d))| {
                            *d == Destinazione::Nuova
                                && row.immagine.is_some() == d_immagine
                                && row.content_key.is_some()
                        })
                        .map(|(index, _)| index)
                        .collect();
                    let chiavi: Vec<&str> = posizioni
                        .iter()
                        .filter_map(|index| righe.get(*index))
                        .filter_map(|(row, _)| row.content_key.as_deref())
                        .collect();
                    let identita: Vec<RemovedIdentity<'_>> = sospese
                        .iter()
                        .filter(|(track_id, _)| spariti_d_immagine.contains(track_id) == d_immagine)
                        .map(|(track_id, content_key)| RemovedIdentity {
                            track_id: *track_id,
                            content_key: content_key.as_str(),
                        })
                        .collect();
                    for rematch in match_moved_tracks(&identita, &chiavi) {
                        let Some(&posizione) = posizioni.get(rematch.insert_index) else {
                            continue;
                        };
                        let Some(slot) = righe.get_mut(posizione) else {
                            continue;
                        };
                        slot.1 = Destinazione::Sposta(rematch.track_id);
                        appaiate.push(rematch.track_id);
                    }
                }
                sospese.retain(|(track_id, _)| !appaiate.contains(track_id));
            }

            // ── 5. e solo adesso si scrive: una presa, una transazione ──
            //
            // In scrittura dall'inizio: `riapplica` legge prima di scrivere, e
            // così i trigger delle orfane. Vedi `transazione_di_scrittura`.
            let inizio = Instant::now();
            let now = now_ms();
            let scritte = deposito
                .con_connessione(|connection| {
                    let tx = transazione_di_scrittura(connection)
                        .map_err(|err| db_error("apertura della transazione", &err))?;
                    let mut inserite = 0_usize;
                    let mut aggiornate = 0_usize;
                    let mut spostate = 0_usize;
                    // Le righe che si riscrivono, segnate **prima**: l'album e
                    // l'artista da cui un brano ritaggato o spostato esce. E le
                    // provvisorie che se ne vanno, che un album ce l'hanno.
                    let mut scritte_qui: Vec<i64> = righe
                        .iter()
                        .filter_map(|(_, destinazione)| match *destinazione {
                            Destinazione::Aggiorna(id)
                            | Destinazione::Sposta(id)
                            | Destinazione::Provvisoria(id) => Some(id),
                            Destinazione::Nuova => None,
                        })
                        .chain(provvisorie_rifiutate.iter().map(|(id, _)| *id))
                        .collect();
                    toccati.segna_brani(&tx, &scritte_qui)?;
                    for (row, destinazione) in &righe {
                        match *destinazione {
                            Destinazione::Nuova => {
                                insert_track(&tx, row, now)?;
                                // `riapplica` anche qui, da quando una correzione
                                // può precedere la riga: un file tolto dalla
                                // libreria e tornato — la cartella rimessa, il
                                // disco ricollegato — ritrova le sue correzioni per
                                // `content_key`, e il trigger della migrazione 023
                                // le ha appena rimesse in `track_overrides`. Senza
                                // questa chiamata starebbero lì senza toccare la
                                // riga. Per quasi tutti i brani nuovi costa una
                                // lettura che non trova niente.
                                let nuova = tx.last_insert_rowid();
                                crate::incerti::riapplica(&tx, nuova)?;
                                scritte_qui.push(nuova);
                                inserite += 1;
                            }
                            // La provvisoria si completa come un file ritaggato:
                            // stessa riga, i tag del file sopra, le correzioni di
                            // chi l'ha già toccata rimesse sopra ai tag.
                            Destinazione::Aggiorna(id) | Destinazione::Provvisoria(id) => {
                                let chiave = update_track(&tx, id, row, now)?;
                                // `update_track` ha appena rimesso i tag del file
                                // sopra la riga: senza questa chiamata basta che
                                // cambi la data di modifica di un file perché la
                                // correzione dell'utente sparisca da `tracks`
                                // restando orfana in `track_overrides`. La chiave è
                                // quella che `update_track` ha scritto — i tag più
                                // l'arricchimento rimesso — e non `row.track_key`.
                                crate::incerti::riapplica_sopra_i_tag(&tx, id, &chiave)?;
                                aggiornate += 1;
                            }
                            Destinazione::Sposta(id) => {
                                let chiave = update_track(&tx, id, row, now)?;
                                // Lo stesso, e qui conta doppio: un file spostato ha
                                // anche un `album_key` nuovo, che si ricalcola dal
                                // percorso d'arrivo e dall'album **corretto**.
                                crate::incerti::riapplica_sopra_i_tag(&tx, id, &chiave)?;
                                spostate += 1;
                            }
                        }
                    }
                    // E **dopo**: l'album e l'artista in cui ogni riga è entrata,
                    // correzioni a mano comprese.
                    toccati.segna_brani(&tx, &scritte_qui)?;
                    ricorda_i_rifiuti(&tx, &rifiutati, &guariti, now)?;
                    let tolte = togli_provvisorie(&tx, &provvisorie_rifiutate)?;
                    tx.commit()
                        .map_err(|err| db_error("chiusura della transazione", &err))?;
                    Ok((inserite, aggiornate, spostate, tolte))
                })
                .map_err(|err| riparo_dopo_una_caduta(deposito, &toccati, err))?;
            report.inserted += scritte.0;
            report.updated += scritte.1;
            report.moved += scritte.2;
            report.removed += scritte.3;
            tempi.scrittura_ms = tempi.scrittura_ms.saturating_add(millisecondi(inizio));

            // Il lotto è chiuso e scritto: è l'unico punto in cui fermarsi non
            // lascia niente a metà.
            if report.cancelled {
                break;
            }
        }
        // Le letture ancora in volo — solo dopo un annullamento — non si
        // aspettano: la squadra se ne va, e i suoi fili escono da soli.
        drop(squadra);

        // ── le radici con letture rimandate ──
        //
        // Lo stesso ragionamento delle radici morte qui sotto, senza dire che
        // non si sono viste: i file nuovi rimandati non hanno avuto la loro
        // occasione di reclamare una riga sparita — uno di loro può essere
        // quel brano, spostato —, e toglierla adesso porterebbe via ascolti,
        // voti e playlist che il giro dopo avrebbe ritrovato.
        if !stato.rimandate.is_empty() {
            let rimandate = std::mem::take(&mut stato.rimandate);
            let sotto_una_rimandata = |id: i64| {
                report
                    .plan
                    .to_remove
                    .iter()
                    .find(|r| r.track.id == id)
                    .is_some_and(|r| {
                        rimandate
                            .iter()
                            .any(|root| is_under(&r.track.path, root, self.rules))
                    })
            };
            sospese.retain(|(id, _)| !sotto_una_rimandata(*id));
        }

        // ── le radici che sono morte mentre si leggeva ──
        //
        // I loro brani non hanno avuto la loro occasione: non sono stati riletti
        // perché la share non risponde più, non perché non ci siano. Toglierli
        // sarebbe la stessa cancellazione che la sonda della camminata impedisce
        // all'inizio della passata, spostata di qualche minuto più in là — e a
        // quel punto costerebbe di più, perché una scansione arrivata a metà ha
        // già scritto tutto il resto.
        let abbandonate = stato.abbandonate;
        if !abbandonate.is_empty() {
            let sotto_una_morta = |id: i64| {
                report
                    .plan
                    .to_remove
                    .iter()
                    .find(|r| r.track.id == id)
                    .is_some_and(|r| {
                        abbandonate
                            .iter()
                            .any(|root| is_under(&r.track.path, root, self.rules))
                    })
            };
            sospese.retain(|(id, _)| !sotto_una_morta(*id));
            // E si dice: una passata che non ha visto la cartella sul NAS ha
            // fatto un lavoro parziale, e un «completata» che non lo dichiara fa
            // credere che i brani mancanti non ci siano più.
            for root in abbandonate {
                if !report.radici_saltate.contains(&root) {
                    report.radici_saltate.push(root);
                }
            }
        }

        // ── 6. le sparizioni che nessun file nuovo ha reclamato ──
        //
        // **Saltate** se ci si è fermati a metà, ed è la parte che rende
        // l'annullamento sicuro invece che distruttivo. Queste righe sono brani
        // il cui file non c'è più *dove era*, e restano in sospeso perché uno
        // dei lotti successivi potrebbe ritrovarli altrove — è così che
        // riorganizzare la libreria non azzera conteggi, preferiti e voti.
        //
        // Fermandosi al terzo lotto su dieci, i sette che non sono stati letti
        // non hanno avuto la loro occasione di reclamarli: cancellarli qui
        // vorrebbe dire che premere Annulla a metà di una scansione dopo aver
        // spostato una cartella distrugge le statistiche di tutti i brani che
        // ci stavano dentro.
        if !report.cancelled && !sospese.is_empty() {
            // ── e la guardia contro le stragi ──
            //
            // Quante righe la libreria conosceva prima di questa passata: le
            // invariate, quelle fuori dalle radici, quelle da rileggere e quelle
            // che il piano toglierebbe. Non c'è un contatore unico su `ScanPlan`
            // perché il piano racconta le decisioni, non l'anagrafica — e
            // sommarle qui tiene la somma accanto all'unica regola che la usa.
            let totale_conosciute = report
                .plan
                .unchanged
                .saturating_add(report.plan.untouched)
                .saturating_add(report.plan.to_update.len())
                .saturating_add(report.plan.to_remove.len());
            // Il caso vero: un disco che non si è montato all'avvio, una radice
            // che risponde ma è vuota perché il punto di mount è caduto, una
            // cartella rinominata fuori da Rymembra. Dopo la protezione delle
            // radici irraggiungibili e dei rami persi resta improbabile, e
            // proprio per questo non costa niente chiedere prima di farlo.
            //
            // Vale per ogni scansione, prudente o no: vedi `Scan::prudente`.
            // Quel che cambia è solo se l'elenco resta a chi può dire sì.
            let strage = sospese.len() > SOGLIA_RIMOZIONI
                && sospese.len().saturating_mul(FRAZIONE_SOSPETTA) > totale_conosciute;
            // Il percorso viene dal piano, cioè da com'era la riga quando la si
            // è giudicata: se nel frattempo qualcuno l'ha cambiata, la
            // cancellazione non la riconosce e non la tocca.
            let elenco: Vec<RimozioneInAttesa> = sospese
                .iter()
                .filter_map(|(id, _)| {
                    report
                        .plan
                        .to_remove
                        .iter()
                        .find(|r| r.track.id == *id)
                        .map(|r| RimozioneInAttesa {
                            id: *id,
                            path: r.track.path.clone(),
                        })
                })
                .collect();
            if strage {
                report.rimozioni_rinviate = sospese.len();
                if !self.prudente {
                    report.rimozioni_in_attesa = elenco;
                }
            } else {
                let inizio = Instant::now();
                let tolte = deposito
                    .con_connessione(|connection| {
                        let tx = transazione_di_scrittura(connection)
                            .map_err(|err| db_error("apertura della transazione", &err))?;
                        let coppie: Vec<(i64, &str)> = elenco
                            .iter()
                            .map(|rimozione| (rimozione.id, rimozione.path.as_str()))
                            .collect();
                        let quali: Vec<i64> = elenco.iter().map(|rimozione| rimozione.id).collect();
                        toccati.segna_brani(&tx, &quali)?;
                        let tolte = remove_tracks(&tx, &coppie)?;
                        tx.commit()
                            .map_err(|err| db_error("chiusura della transazione", &err))?;
                        Ok(tolte)
                    })
                    .map_err(|err| riparo_dopo_una_caduta(deposito, &toccati, err))?;
                report.removed += tolte;
                tempi.scrittura_ms = tempi.scrittura_ms.saturating_add(millisecondi(inizio));
            }
        }

        // ── 7. gli aggregati, quando i brani sono tutti al loro posto ──
        //
        // Solo quelli che questa passata ha toccato: una scansione che non ha
        // cambiato niente non rilegge più cinquantamila brani per riscrivere
        // gli stessi album. Per intero quando serve — la prima volta, dopo una
        // migrazione o «Rileggi tutti i tag»: vedi `ricostruisci_aggregati`.
        let inizio = Instant::now();
        report.aggregates = deposito.con_connessione(|connection| {
            if toccati.e_vuoto() {
                // Niente scritto, niente transazione: due conteggi e basta.
                if aggregati_in_regola(connection)? {
                    return conta_aggregati(connection);
                }
            }
            let esito = (|| {
                let tx = transazione_di_scrittura(connection)
                    .map_err(|err| db_error("apertura della transazione", &err))?;
                let aggregati = ricostruisci_aggregati(&tx, &toccati)?;
                tx.commit()
                    .map_err(|err| db_error("chiusura della transazione", &err))?;
                Ok(aggregati)
            })();
            // I brani di questa passata sono già scritti, in transazioni
            // chiuse prima di qui: se la ricostruzione cade — un `db.locked`
            // passeggero, o un guasto come quello delle chiavi doppie della
            // 3.0 — gli album restano quelli di prima mentre i brani sono già
            // quelli nuovi, e la libreria segnata in regola non lo saprebbe
            // mai. Le chiavi toccate da questa passata si perdono con lei:
            // l'unico modo di ritrovarle è la ricostruzione intera, e la si
            // chiede. Sforzo minimo — se non riesce neanche questa scrittura,
            // l'errore da dire resta quello vero. La stessa regola vale per le
            // prese di prima: vedi [`riparo_dopo_una_caduta`].
            if esito.is_err() {
                let _ = chiedi_ricostruzione_intera(connection);
            }
            esito
        })?;
        tempi.aggregati_ms = millisecondi(inizio);

        report.tempi = tempi;
        report.elapsed_ms = started.elapsed().as_millis();
        Ok(report)
    }

    /// Esegue la scansione su una connessione che si ha già in mano.
    ///
    /// La forma comoda per chi la connessione ce l'ha per sé — la riga di
    /// comando, le prove, il ponte Android. Sul desktop non si passa di qui:
    /// prendere il lucchetto della libreria prima di chiamare e restituirlo dopo
    /// è esattamente ciò che [`Scan::run_su`] esiste per non fare.
    pub fn run(
        &self,
        connection: &mut Connection,
        on_progress: impl FnMut(usize, usize) -> ControlFlow<()>,
    ) -> Result<ScanReport, AppError> {
        self.run_su(connection, on_progress)
    }
}

/// Una presa di scrittura della scansione è caduta prima della fase degli
/// aggregati: rimette in pari quel che le prese di prima hanno già scritto, e
/// restituisce l'errore com'era.
///
/// # Perché
///
/// Ogni lotto si conferma da sé, e le chiavi d'album e d'artista che ha
/// toccato vivono solo in memoria ([`Toccati`]), fino alla fase 7. Se una presa
/// in mezzo cade — un `db.locked` dopo il `busy_timeout` mentre un'altra
/// connessione scrive, un disco pieno, un vincolo violato — i lotti già
/// confermati restano, la fase 7 non parte, e la libreria resta segnata «in
/// regola»: alla passata dopo quei brani sono invariati, nessuno li tocca più,
/// e un album nuovo non nasce mai (finché non si fa «Rileggi tutti i tag»).
///
/// Prima si prova a rifare le chiavi raccolte fin lì, in una transazione sua;
/// se cade anche quella, si chiede la ricostruzione intera alla prossima
/// passata. Tutto sforzo minimo: l'errore da dire resta quello vero. Chiavi
/// segnate da una transazione poi annullata costano solo un album rifatto
/// uguale.
///
/// # Il limite
///
/// Se la causa è proprio il lucchetto del database, anche queste scritture
/// rischiano di scadere allo stesso modo, e il buco resta. Chiuderlo del tutto
/// vorrebbe dire segnare «aggregati da rifare» nella stessa transazione del
/// primo lotto, cioè un campo nuovo e una migrazione.
fn riparo_dopo_una_caduta<D: Deposito>(
    deposito: &mut D,
    toccati: &Toccati,
    errore: AppError,
) -> AppError {
    if toccati.e_vuoto() {
        return errore;
    }
    let _ = deposito.con_connessione(|connection| {
        let rifatti = (|| {
            let tx = transazione_di_scrittura(connection)
                .map_err(|err| db_error("apertura della transazione", &err))?;
            ricostruisci_aggregati(&tx, toccati)?;
            tx.commit()
                .map_err(|err| db_error("chiusura della transazione", &err))
        })();
        if rifatti.is_err() {
            let _ = chiedi_ricostruzione_intera(connection);
        }
        Ok(())
    });
    errore
}

/// Ricorda i rifiuti di un lotto e dimentica quelli che non lo sono più.
fn ricorda_i_rifiuti(
    tx: &Transaction<'_>,
    rifiutati: &[RifiutoNuovo],
    guariti: &[String],
    now: i64,
) -> Result<(), AppError> {
    if !rifiutati.is_empty() {
        let mut ricorda = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO file_illeggibili
                   (chiave, dimensione, modificato_ms, versione, causa, visto_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|err| db_error("memoria dei file illeggibili", &err))?;
        for rifiuto in rifiutati {
            ricorda
                .execute(rusqlite::params![
                    rifiuto.chiave,
                    rifiuto.dimensione,
                    rifiuto.modificato_ms,
                    crate::metadata::VERSIONE_LETTORE,
                    rifiuto.causa,
                    now,
                ])
                .map_err(|err| db_error("memoria dei file illeggibili", &err))?;
        }
    }
    if !guariti.is_empty() {
        let mut dimentica = tx
            .prepare_cached("DELETE FROM file_illeggibili WHERE chiave = ?1")
            .map_err(|err| db_error("memoria dei file illeggibili", &err))?;
        for chiave in guariti {
            dimentica
                .execute([chiave])
                .map_err(|err| db_error("memoria dei file illeggibili", &err))?;
        }
    }
    Ok(())
}
