//! Il brano che non è un file.
//!
//! Il riconoscimento di un indirizzo di catalogo, l'apertura della connessione
//! con la rete di quel catalogo, e l'involucro che fa di un flusso HTTP una
//! sorgente che il motore non distingue da un file.
//!
//! Fa parte di [`crate::riproduzione`]: la regola dei due lucchetti — prima il
//! lettore, poi la libreria — è scritta là e vale anche qui. Con un corollario
//! che tocca proprio questo modulo: aprire una connessione è un'attesa, e le
//! attese si aspettano a mani vuote.

use rymembra_app::playback::{Collocazione, SchedaSorgente};
use rymembra_catalogo::Cataloghi;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::esterno::Fonte;
use rymembra_domain::indirizzo::estensione_da_url;
use rymembra_domain::scelta::Candidato;
use rymembra_net::{Cancello, FlussoHttp};

/// Il riferimento a un brano che non è un file.
///
/// Due campi e nient'altro, perché due sono le domande: **quale catalogo** — che
/// decide da quale [`rymembra_net::Rete`] escono i byte, con le sue scadenze, il
/// suo modo di trattare un `429` e la sua riserva di connessioni — e **a che
/// indirizzo**.
///
/// Non c'è la licenza e non c'è la disponibilità, e non è una dimenticanza: qui
/// non si scrive niente sul disco. Il cancello che le guarda è quello di
/// `rymembra_catalogo::prelievo`, e sta prima della rete perché prima della rete
/// deve stare; ascoltare mentre arriva è la cosa che [`Disponibilita::SoloAscolto`]
/// **permette**, e chiederne il permesso una seconda volta qui vorrebbe dire
/// due implementazioni della stessa regola.
///
/// [`Disponibilita::SoloAscolto`]: rymembra_domain::esterno::Disponibilita::SoloAscolto
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RiferimentoFlusso {
    /// Da quale catalogo, cioè con quale rete si apre.
    pub(super) fonte: Fonte,
    /// L'indirizzo dei byte. Per Audius un percorso, che vuole ancora un nodo
    /// davanti: vedi [`apri_flusso`].
    pub(super) url: String,
}

/// Il percorso con cui Audius conserva un brano invece di un indirizzo.
///
/// `rymembra_catalogo::audius` scrive `/v1/tracks/<id>/stream` e non un URL
/// intero, e la ragione sta scritta là: Audius non ha un server ma una rete di
/// nodi che entrano ed escono, e un indirizzo con dentro il nodo di oggi fra un
/// mese punta a una macchina che non c'è più. Il nodo lo rimette
/// `Audius::prepara` al momento di andare a prendere i byte.
const PERCORSO_AUDIUS: &str = "/v1/tracks/";

/// Questo brano è un flusso? Allora ecco da dove.
///
/// # Quale campo si legge
///
/// `source_service` e `fonte_url`, che la migrazione 26 ha messo su `tracks`
/// accanto a un `path` che adesso può essere nullo — gli stessi nomi e gli
/// stessi valori che `desiderati` ha dalla migrazione 10. Fino alla 2.3.2 qui
/// si leggeva `path`, perché in `SchedaSorgente` non c'era altro, e il commento
/// che stava in questo punto prometteva che sarebbe cambiata **solo questa
/// funzione**: è quel che è successo. Tutto ciò che sta a valle — [`apri_flusso`],
/// [`sorgente_da_flusso`], il ramo dentro [`brano_di`](super::brano_di) —
/// riceve un [`RiferimentoFlusso`] e non sa da quale colonna sia uscito.
///
/// La differenza non è cosmetica. Prima la domanda era «questo testo somiglia a
/// un indirizzo?», e la risposta andava data guardandosi dai percorsi di
/// Windows che le somigliano. Adesso è la riga a dichiararsi, e quel che resta
/// da controllare è solo che si dichiari una cosa lecita.
///
/// # Perché il riconoscimento passa dal catalogo e non da un elenco di qui
///
/// Perché un elenco di domini scritto qui sarebbe il **secondo**:
/// `rymembra_catalogo::riconosci` è una allowlist rigida, e il suo modulo dice
/// perché — «un link che non si riconosce è un link a cui non si va, ed è la
/// garanzia che Rymembra non vada mai a bussare dove non è invitato». Due copie di
/// quella regola sono due copie che un giorno diranno cose diverse, e il giorno
/// in cui succede si va a bussare da qualche parte per sbaglio.
///
/// Di quel che `riconosci` restituisce serve solo la **fonte**: l'indirizzo
/// resta quello che c'era scritto, perché quello sono i byte, mentre l'`id` che
/// il catalogo ne ricava nomina il concerto e non il file. È un cancello, non un
/// traduttore.
pub(super) fn riferimento_di(scheda: &SchedaSorgente) -> Option<RiferimentoFlusso> {
    let Collocazione::Catalogo { fonte, url } = &scheda.collocazione else {
        return None;
    };
    riferimento_da_testo(*fonte, url)
}

/// Il cancello vero e proprio, su una fonte e un indirizzo.
///
/// Separata da [`riferimento_di`] per poterla provare senza un database e senza
/// una finestra: è una decisione su del testo, e le decisioni su del testo si
/// provano come chiamate di funzione.
///
/// # Perché si ricontrolla quel che la riga dichiara
///
/// Perché fra la riga e questa funzione c'è un database, e un database è un
/// file che si può aprire con altri strumenti. La colonna dice da dove *si
/// crede* che arrivino i byte; l'allowlist dice dove Rymembra è invitato ad
/// andare. La seconda non si delega alla prima — è la differenza fra un
/// programma che non bussa dove non deve e un programma che non bussa dove non
/// deve *finché nessuno gli scrive un indirizzo in tabella*.
fn riferimento_da_testo(fonte: Fonte, url: &str) -> Option<RiferimentoFlusso> {
    let pulito = url.trim();
    if pulito.is_empty() {
        return None;
    }
    // Il percorso di Audius è un caso a parte perché non è un indirizzo, e
    // `riconosci` — che di indirizzi si occupa — non lo vedrebbe. Il controllo
    // che vale qui è che la forma sia quella che `Audius::prepara` sa
    // completare: un percorso `/v1/tracks/…`, senza nodo davanti.
    if fonte == Fonte::Audius {
        return pulito
            .starts_with(PERCORSO_AUDIUS)
            .then(|| RiferimentoFlusso {
                fonte,
                url: pulito.to_owned(),
            });
    }
    // `https://` e non anche `http://`: `Rete` nasce con `https_only`, quindi un
    // indirizzo in chiaro non partirebbe comunque, e riconoscerlo qui vorrebbe
    // dire promettere un'apertura che fallirà più in là con un errore che parla
    // d'altro.
    if !pulito.starts_with("https://") {
        return None;
    }
    // La fonte riconosciuta dall'indirizzo deve essere **quella** che la riga
    // dichiara: un indirizzo dell'Internet Archive in una riga che dice Jamendo
    // aprirebbe una connessione con la rete sbagliata, cioè con le scadenze e i
    // limiti di frequenza di un servizio verso un altro.
    (rymembra_catalogo::riconosci(pulito)?.fonte == fonte).then(|| RiferimentoFlusso {
        fonte,
        url: pulito.to_owned(),
    })
}

/// Apre il flusso di un riferimento, con la rete del catalogo che lo ha dato.
///
/// **Va in rete**, e quindi va chiamata dove la rete si può aspettare: dentro
/// la scadenza di [`brano_di`](super::brano_di), senza nessun lucchetto in
/// mano. Costa una richiesta `Range` da [`rymembra_net::flusso::FINESTRA`] byte —
/// l'inizio del brano, che è la prima cosa che il decodificatore chiederà —
/// più, su Audius, la scelta del nodo.
///
/// # Perché la rete è quella del catalogo e non una nuova
///
/// Perché in quella `Rete` ci sono lo `User-Agent` con cui Rymembra si presenta,
/// la scadenza che quel servizio merita, il modo di leggere un `Retry-After` e
/// la riserva di connessioni già calda. È la rete **dei flussi** del catalogo
/// (`rete_flusso`), non quella delle domande: fino alla 2.4.0 era la seconda, e
/// i suoi trenta secondi complessivi facevano morire a metà canzone le finestre
/// da due megabyte su una linea lenta. Una `Rete` nuova per brano sarebbe un
/// saluto TLS per canzone e un limite di frequenza contato da capo ogni volta,
/// contro archivi pubblici che ci ospitano gratis — cioè il modo di farsi
/// bloccare per maleducazione.
///
/// # Che cosa restituisce, oltre al flusso
///
/// L'indirizzo da cui si ricava il formato. Di solito è quello della riga; per
/// un FLAC dell'Internet Archive entrato in libreria con la 2.4.0 è l'MP3 che si
/// suona al suo posto (vedi `ArchivioOrg::da_ascoltare`), e dire al
/// decodificatore «è un FLAC» mentre arrivano fotogrammi MP3 vorrebbe dire
/// fargli cercare il contenitore sbagliato per primo.
///
/// # Dove può portare un rimando
///
/// Solo dove il catalogo è di casa: ogni flusso si apre con il suo
/// [`cancello_di`], e un rimando che il cancello rifiuta non riceve nemmeno la
/// richiesta.
///
/// # Errori
///
/// Quelli di `Rete::intervallo` (`net.*`, `download.*`) e, per Audius,
/// `catalogo.notAvailable` se non risponde nessun nodo. `net.badSchema` per un
/// rimando che porta fuori dal catalogo.
/// `playback.sourceUnavailable` per una fonte che di byte non ne dà: è il caso
/// dell'archivio di Spotify e di un file di playlist, che nominano un brano e
/// non lo consegnano.
pub(super) fn apri_flusso(
    cataloghi: &Cataloghi,
    rif: &RiferimentoFlusso,
) -> Result<(FlussoHttp, String), AppError> {
    match rif.fonte {
        // Per un FLAC entrato in libreria con la 2.4.0 si suona l'MP3 dello
        // stesso brano. Costa una scheda la prima volta, dentro la stessa
        // scadenza dell'apertura.
        Fonte::InternetArchive => {
            let da_suonare = cataloghi.archivio().da_ascoltare(&rif.url);
            let flusso = FlussoHttp::nuovo(
                cataloghi.archivio().rete_flusso(),
                da_suonare.clone(),
                cancello_di(rif.fonte, &da_suonare),
            )?;
            Ok((flusso, da_suonare))
        }
        // Il nodo si sceglie **adesso**: è lo stesso passo che fa il prelievo, e
        // per la stessa ragione. Un `Candidato` con il solo indirizzo dentro
        // perché è tutto quel che `prepara` guarda, e perché costruirne uno
        // finto completo vorrebbe dire inventare una licenza per una funzione
        // che non la legge.
        Fonte::Audius => {
            let pronto = cataloghi.audius().prepara(&Candidato {
                url: rif.url.clone(),
                ..Candidato::default()
            })?;
            let cancello = cancello_di(rif.fonte, &pronto.url);
            let flusso = FlussoHttp::nuovo(cataloghi.audius().rete_flusso(), pronto.url, cancello)?;
            Ok((flusso, rif.url.clone()))
        }
        // Il catalogo per cui `FlussoHttp` è nato: da Jamendo non si tiene
        // niente, e ascoltare mentre arriva è l'unico modo lecito che c'è. Senza
        // la feature il ramo non esiste e la fonte cade nel caso generale, che è
        // la verità: questa copia di Rymembra Jamendo non ce l'ha.
        #[cfg(feature = "jamendo")]
        Fonte::Jamendo => {
            let flusso = FlussoHttp::nuovo(
                cataloghi.jamendo().rete_flusso(),
                rif.url.clone(),
                cancello_di(rif.fonte, &rif.url),
            )?;
            Ok((flusso, rif.url.clone()))
        }
        altra => Err(AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: None,
            path: Some(rif.url.clone()),
        })
        .with_cause(format!(
            "da {} non arrivano byte da suonare",
            altra.etichetta()
        ))),
    }
}

/// Il cancello dei rimandi di un flusso: dove un «vai là» può portare i byte.
///
/// `partenza` è l'indirizzo che si apre davvero — quello passato da
/// [`riferimento_da_testo`], o per Audius quello col nodo davanti — e resta lo
/// stesso per tutti i salti: il cancello giudica ogni bersaglio contro il
/// catalogo da cui si è partiti, non contro il salto prima.
fn cancello_di(fonte: Fonte, partenza: &str) -> Cancello {
    let partenza = partenza.to_owned();
    Cancello::nuovo(move |bersaglio| bersaglio_ammesso(fonte, &partenza, bersaglio))
}

/// Se un rimando di un flusso di `fonte`, partito da `partenza`, può andare a
/// `bersaglio`.
///
/// # La stessa allowlist dell'indirizzo di partenza
///
/// L'indirizzo di partenza è passato da `rymembra_catalogo::riconosci`, e il
/// bersaglio deve passare **dallo stesso posto**: un secondo elenco di domini
/// scritto qui sarebbe il secondo, cioè quello che un giorno dice una cosa
/// diversa (vedi [`riferimento_di`]). Ma `riconosci` legge link pubblici, e il
/// bersaglio di un rimando non lo è: `archive.org/download/…` rimanda a
/// `ia800305.us.archive.org/27/items/…`, un percorso che `riconosci` non ha
/// motivo di conoscere. Quel che del bersaglio conta è l'**ospite**, e allora
/// si chiede a `riconosci` esattamente quello: lo stesso indirizzo di partenza,
/// con l'ospite del bersaglio al posto del suo. Se è ancora un link di quel
/// catalogo, l'ospite è di casa; `evilarchive.org` e `archive.org.esempio.com`
/// no, per le stesse ragioni per cui non lo sono come link.
///
/// # Audius, che un'allowlist di domini non ce l'ha
///
/// Audius non ha un server ma una rete di nodi gestiti da chi vuole, ognuno col
/// suo dominio: il nodo di partenza viene dall'elenco che `api.audius.co`
/// consegna, non da `riconosci`, e rimanda a un nodo di contenuti che può stare
/// sotto qualunque nome. Un elenco di domini per Audius sarebbe un elenco che
/// smette di suonare metà del catalogo il giorno in cui entra un nodo nuovo.
/// Quel che si può esigere da un salto di Audius, e si esige, è che resti
/// cifrato e che vada verso un **nome pubblico**: mai un indirizzo IP scritto a
/// cifre, mai `localhost`, mai un nome da rete di casa. Un nodo ostile non può
/// così usare il lettore per bussare al router o a un servizio di questa
/// macchina.
fn bersaglio_ammesso(fonte: Fonte, partenza: &str, bersaglio: &str) -> bool {
    if !bersaglio.starts_with("https://") {
        return false;
    }
    let Some(ospite) = rymembra_net::http::ospite(bersaglio) else {
        return false;
    };
    if fonte == Fonte::Audius {
        return nome_pubblico(ospite);
    }
    // Il percorso della partenza, query compresa: `riconosci` guarda il
    // percorso per capire *che cosa* il link nomina, e deve trovarci quel che
    // ci ha trovato la prima volta.
    let percorso = partenza
        .strip_prefix("https://")
        .and_then(|resto| resto.find('/').and_then(|inizio| resto.get(inizio..)))
        .unwrap_or("/");
    rymembra_catalogo::riconosci(&format!("https://{ospite}{percorso}"))
        .is_some_and(|riferimento| riferimento.fonte == fonte)
}

/// Un nome che si risolve nel DNS pubblico, e non un indirizzo di casa.
///
/// Si guarda l'ultima etichetta: un nome pubblico finisce con un dominio di
/// primo livello fatto di lettere (o di punycode, `xn--…`). È la regola che
/// esclude in un colpo solo `127.0.0.1`, `10.0.0.1`, `[::1]`, le forme
/// esadecimali o decimali di un IPv4 che alcuni risolutori accettano
/// (`0x7f.1`, `2130706433`) e i nomi senza punto; i suffissi da rete di casa si
/// escludono per nome.
fn nome_pubblico(ospite: &str) -> bool {
    let ospite = ospite.trim_end_matches('.').to_ascii_lowercase();
    let Some((_, ultima)) = ospite.rsplit_once('.') else {
        return false;
    };
    let lettere = ultima.len() >= 2 && ultima.chars().all(|c| c.is_ascii_alphabetic());
    let punycode = ultima.starts_with("xn--");
    let di_casa = [
        "localhost",
        "local",
        "internal",
        "lan",
        "home",
        "arpa",
        "test",
        "invalid",
    ]
    .contains(&ultima);
    (lettere || punycode) && !di_casa
}

/// Un flusso di rete, vestito da [`rymembra_play::Flusso`].
///
/// # Perché serve un involucro
///
/// Perché il tratto è di `rymembra-play` e il tipo è di `rymembra-net`, e nessuno
/// dei due è di qui: la regola dell'orfano vieta di scrivere quell'`impl` in un
/// terzo crate. È la stessa ragione per cui `rymembra-app` ne ha uno suo
/// (`playback::Adattatore`) per i file, e la stessa forma.
///
/// Non è però solo una formalità del compilatore. Questo modulo è **l'unico
/// posto dell'albero che conosce tutti e due i lati**: `rymembra-app` non vede la
/// rete e `rymembra-catalogo` non vede il motore. Il punto in cui un flusso HTTP
/// diventa qualcosa che si può suonare non poteva stare altrove.
struct FlussoDiRete(FlussoHttp);

impl std::io::Read for FlussoDiRete {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl std::io::Seek for FlussoDiRete {
    fn seek(&mut self, verso: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.seek(verso)
    }
}

impl rymembra_play::Flusso for FlussoDiRete {
    /// Quanto è lungo, se il servizio lo ha dichiarato.
    ///
    /// Non è un di più: `MediaSource::is_seekable` di symphonia risponde
    /// guardando questo, e un `None` qui vuol dire il cursore che sparisce dalla
    /// barra prima ancora che il brano cominci.
    fn lunghezza(&self) -> Option<u64> {
        self.0.lunghezza()
    }

    /// I byte arrivano dalla rete: il decodificatore salta con la modalità
    /// grossolana e la riga dei dati tecnici prende il bitrate da qui. Vedi
    /// `rymembra_play::Flusso::remoto`.
    fn remoto(&self) -> bool {
        true
    }

    /// Un salto dopo un salto, un altro brano o uno stop: la richiesta in
    /// corso smette di aspettare la rete. Vedi `FlussoHttp::ascolta_annullo`.
    fn ascolta_annullo(&mut self, annullo: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.0.ascolta_annullo(annullo);
    }
}

/// I byte di un brano che sta sul disco di un altro PC della sessione, vestiti
/// da [`rymembra_play::Flusso`].
///
/// Gemello di [`FlussoDiRete`], con una differenza sola che è tutto il punto:
/// **`remoto()` è falso**. Per un catalogo «remoto» vuol dire salti
/// grossolani, perché un salto accurato su un MP3 rileggerebbe megabyte da un
/// server lontano. Qui la stessa scelta sarebbe la peggiore: in una sessione il
/// salto deve finire **esattamente** sul fotogramma che l'host sta suonando,
/// e un salto «a grandi linee» lascerebbe al regolatore un errore di decine di
/// millisecondi da correggere a gradini dopo ogni riapertura. In una LAN
/// rileggere qualche megabyte costa poco: vedi la misura nel rapporto della
/// fase 1 e la prova `una_riapertura_in_rete_si_misura`.
pub(super) struct FlussoDiPari(FlussoHttp);

impl FlussoDiPari {
    /// L'involucro intorno a un flusso, per le prove degli altri moduli: qui
    /// lo costruisce solo [`apri_da_pari`].
    #[cfg(test)]
    pub(super) const fn per_prova(flusso: FlussoHttp) -> Self {
        Self(flusso)
    }
}

impl std::io::Read for FlussoDiPari {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl std::io::Seek for FlussoDiPari {
    fn seek(&mut self, verso: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.seek(verso)
    }
}

impl rymembra_play::Flusso for FlussoDiPari {
    /// La dimensione del file, come l'ha detta chi lo serve: senza, symphonia
    /// dichiarerebbe il flusso non percorribile e il salto non si farebbe.
    fn lunghezza(&self) -> Option<u64> {
        self.0.lunghezza()
    }

    /// Falso: vedi il tipo.
    fn remoto(&self) -> bool {
        false
    }

    /// Un salto o una voce nuova: la lettura in corso smette di aspettare
    /// l'altro PC. La connessione di lettura la butta il trasporto (vedi
    /// `LettoreRemoto`), che è il solo modo di annullare una richiesta già
    /// partita.
    fn ascolta_annullo(&mut self, annullo: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.0.ascolta_annullo(annullo);
    }
}

/// Quel che serve a fare di una sorgente di un pari un brano: tutto tranne i
/// byte, che li dà la chiusura.
pub(super) struct DaPari {
    /// La maniglia: il `track_id` che il motore annuncerà.
    pub(super) maniglia: i64,
    /// L'estensione, per il riconoscimento del formato.
    pub(super) estensione: Option<String>,
    /// La durata dichiarata dalla voce.
    pub(super) durata_ms: u64,
    /// La correzione dichiarata dalla voce: la stessa su ogni PC.
    pub(super) replaygain_db: Option<f32>,
    /// Il picco dichiarato dalla voce.
    pub(super) picco: Option<f32>,
    /// Il tratto, per un brano d'immagine CD.
    pub(super) segmento: Option<rymembra_play::Segmento>,
}

/// Apre una voce che sta sul disco di un altro PC: la sorgente dalla
/// chiusura della sessione, la finestra scorrevole di [`FlussoHttp`] sopra, e
/// il riconoscimento del contenitore — tutto dentro la scadenza dei flussi,
/// come un brano di catalogo e per la stessa ragione: è la rete, e chi ha
/// premuto play deve avere una risposta.
///
/// Senza nessun lucchetto in mano: vedi il preambolo del modulo padre.
///
/// # Errori
///
/// Quelli della chiusura (il trasporto: `insieme.*`), quelli della lettura
/// tradotti come per un flusso interrotto, `net.timeout` alla scadenza e
/// `playback.formatUnsupported` per un contenitore che non si riconosce.
pub(super) fn apri_da_pari(
    apri: super::ospiti::ApriInRete,
    dati: DaPari,
    formato: rymembra_play::FormatoUscita,
) -> Result<rymembra_play::BranoAperto, AppError> {
    let esito = rymembra_app::scadenza::con_scadenza_o_caduta(
        super::APERTURA_BRANO_NOME,
        super::APERTURA_FLUSSO,
        move || {
            let sorgente = apri()?;
            let flusso = FlussoHttp::da(sorgente)?;
            let sorgente = rymembra_play::Sorgente {
                track_id: dati.maniglia,
                media: Box::new(FlussoDiPari(flusso)),
                estensione: dati.estensione,
                durata_ms: dati.durata_ms,
                replaygain_db: dati.replaygain_db,
                picco: dati.picco,
                segmento: dati.segmento,
            };
            rymembra_play::BranoAperto::apri(sorgente, formato)
                .map_err(super::interruzione_di_flusso)
        },
    );
    esito.unwrap_or_else(|mancato| {
        Err(super::apertura_mancata(
            &mancato,
            super::APERTURA_FLUSSO,
            true,
            None,
        ))
    })
}

/// Apre un brano di catalogo per una voce di sessione: la stessa strada di un
/// brano di catalogo della libreria, con la maniglia come `track_id` e i
/// numeri della voce al posto di quelli di una riga.
///
/// # Errori
///
/// Quelli di [`apri_flusso`], `net.timeout` alla scadenza, e
/// `internal.aborted` se il lettore non c'è più.
pub(super) fn apri_da_catalogo(
    app: &tauri::AppHandle,
    maniglia: i64,
    rif: &RiferimentoFlusso,
    scheda: &super::ospiti::SchedaVoce,
    formato: rymembra_play::FormatoUscita,
) -> Result<rymembra_play::BranoAperto, AppError> {
    let cataloghi = super::cataloghi_di(app, "apertura di una voce di catalogo")?;
    let finta = SchedaSorgente {
        track_id: maniglia,
        collocazione: Collocazione::Catalogo {
            fonte: rif.fonte,
            url: rif.url.clone(),
        },
        durata_ms: i64::try_from(scheda.durata_ms).unwrap_or(i64::MAX),
        replaygain_db: scheda.replaygain_db.map(f64::from),
        picco: scheda.picco.map(f64::from),
    };
    let rif = rif.clone();
    let esito = rymembra_app::scadenza::con_scadenza_o_caduta(
        super::APERTURA_BRANO_NOME,
        super::APERTURA_FLUSSO,
        move || {
            let (flusso, da_suonare) = apri_flusso(&cataloghi, &rif)?;
            let sorgente = sorgente_da_flusso(&finta, &da_suonare, flusso);
            rymembra_play::BranoAperto::apri(sorgente, formato)
                .map_err(super::interruzione_di_flusso)
        },
    );
    esito.unwrap_or_else(|mancato| {
        Err(super::apertura_mancata(
            &mancato,
            super::APERTURA_FLUSSO,
            true,
            None,
        ))
    })
}

/// Il separatore fra la fonte e l'indirizzo in un riferimento di voce.
///
/// I due punti, perché nessun nome di [`Fonte::nome`] li contiene: il primo
/// che si incontra separa sempre la fonte, e quel che resta — un `https://…`
/// coi suoi due punti, o un percorso di Audius — è l'indirizzo intero.
const SEPARATORE_RIFERIMENTO: char = ':';

/// Il riferimento con cui un brano di catalogo viaggia in una sessione:
/// `<fonte>:<indirizzo>`, con il nome stabile della fonte.
///
/// Il nome stabile e non l'etichetta, perché è quello che sta già in tabella e
/// attraversa l'IPC: lo rilegge [`Fonte::da_testo`] su qualunque PC, anche uno
/// più vecchio di un catalogo che questo conosce — e allora vale una fonte che
/// byte non ne dà, cioè una voce che non si apre invece di una voce aperta
/// con la rete sbagliata.
pub(super) fn riferimento_in_testo(fonte: Fonte, url: &str) -> String {
    format!("{}{SEPARATORE_RIFERIMENTO}{url}", fonte.nome())
}

/// Il contrario di [`riferimento_in_testo`], **con il cancello di questo PC**.
///
/// Il riferimento arriva da un altro PC, e chi lo manda può mandare quel che
/// vuole: passa quindi da [`riferimento_da_testo`], la stessa allowlist di una
/// riga della libreria. Un indirizzo che qui non è ammesso non si apre, anche
/// se l'altro PC lo ha suonato.
pub(super) fn riferimento_da_voce(testo: &str) -> Option<RiferimentoFlusso> {
    let (fonte, url) = testo.split_once(SEPARATORE_RIFERIMENTO)?;
    riferimento_da_testo(Fonte::da_testo(fonte), url)
}

/// Da un flusso aperto alla sorgente che il motore sa suonare.
///
/// Quel che il motore riceve è indistinguibile da un file: stesso `track_id`,
/// stessa durata dichiarata dal database, stesso guadagno ReplayGain. È il
/// motivo per cui gapless, dissolvenza, equalizzatore e spettro funzionano senza
/// una riga in più — il motore non ha mai saputo cosa ci fosse dietro i byte.
///
/// Il suggerimento di formato lo ricava
/// [`rymembra_domain::indirizzo::estensione_da_url`]: la regola sta nel dominio
/// perché la usano tre casse che non si vedono fra loro — questa, il prelievo
/// dei cataloghi e la libreria — e due copie divergerebbero alla prima
/// correzione fatta da una parte sola. Divergevano già.
pub(super) fn sorgente_da_flusso(
    scheda: &SchedaSorgente,
    url: &str,
    flusso: FlussoHttp,
) -> rymembra_play::Sorgente {
    rymembra_play::Sorgente {
        track_id: scheda.track_id,
        media: Box::new(FlussoDiRete(flusso)),
        estensione: estensione_da_url(url),
        durata_ms: u64::try_from(scheda.durata_ms).unwrap_or(0),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "sono decibel: la precisione di un f32 è un milionesimo di dB"
        )]
        replaygain_db: scheda.replaygain_db.map(|db| db as f32),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "un picco lineare fra 0 e qualche unità: un f32 basta e avanza"
        )]
        picco: scheda.picco.map(|picco| picco as f32),
        segmento: None,
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un file della libreria non diventa mai un flusso.
    ///
    /// La prova che conta di più delle altre: qui dentro passa **ogni** brano
    /// della libreria, e un riconoscimento troppo largo vorrebbe dire un file
    /// del disco mandato a cercare in rete. Dalla migrazione 26 la garanzia è
    /// più forte di prima — un file porta `Collocazione::File`, e da lì non si
    /// esce — ma resta provata su [`riferimento_di`], che è la funzione vera:
    /// una prova sul solo cancello non direbbe niente del cablaggio.
    #[test]
    fn un_file_della_libreria_non_diventa_mai_un_flusso() {
        for percorso in [
            r"C:\Musica\Pink Floyd\Animals\01 - Pigs on the Wing.flac",
            r"\\nas\musica\raccolta\02 - Dogs.mp3",
            // I tre che somigliano di più a un indirizzo di catalogo. I primi
            // due passavano già prima; il terzo e il quarto no, e sono la
            // ragione per cui questa prova è cambiata: fino alla 2.3.2 un
            // indirizzo scritto in `path` *diventava* un flusso, ed era l'unico
            // modo di ascoltarne uno. Adesso `path` significa «file», e basta.
            r"D:\archive.org\download\gd77\t01.flac",
            "https://archive.org/download/gd77/t01.flac",
            "/v1/tracks/aB3dE/stream",
            "",
        ] {
            let scheda = SchedaSorgente {
                track_id: 1,
                collocazione: Collocazione::File(percorso.to_owned()),
                durata_ms: 1000,
                replaygain_db: None,
                picco: None,
            };
            assert_eq!(
                riferimento_di(&scheda),
                None,
                "«{percorso}» sta in `path`: è un file, non un flusso"
            );
        }
    }

    /// Un indirizzo di catalogo sì, e con l'indirizzo intatto.
    ///
    /// L'indirizzo **non** si riscrive: `riconosci` sa ricavare da un link
    /// l'identificativo del concerto, ma i byte da suonare sono quel file lì, e
    /// sostituirlo con la pagina dell'item vorrebbe dire suonare dell'HTML.
    #[test]
    fn un_indirizzo_di_catalogo_diventa_un_flusso_senza_essere_riscritto() {
        let file = "https://archive.org/download/gd1977-05-08/gd77-05-08d1t01.flac";
        assert_eq!(
            riferimento_da_testo(Fonte::InternetArchive, file),
            Some(RiferimentoFlusso {
                fonte: Fonte::InternetArchive,
                url: file.to_owned(),
            })
        );

        // Audius conserva un percorso e non un indirizzo, apposta: il nodo di
        // oggi fra un mese non c'è più. Vedi [`PERCORSO_AUDIUS`].
        let percorso = "/v1/tracks/aB3dE/stream?ext=wav";
        assert_eq!(
            riferimento_da_testo(Fonte::Audius, percorso),
            Some(RiferimentoFlusso {
                fonte: Fonte::Audius,
                url: percorso.to_owned(),
            })
        );

        // E ci arriva passando da una riga di libreria, che è la strada vera.
        let scheda = SchedaSorgente {
            track_id: 9,
            collocazione: Collocazione::Catalogo {
                fonte: Fonte::InternetArchive,
                url: file.to_owned(),
            },
            durata_ms: 754_000,
            replaygain_db: None,
            picco: None,
        };
        assert_eq!(
            riferimento_di(&scheda),
            Some(RiferimentoFlusso {
                fonte: Fonte::InternetArchive,
                url: file.to_owned(),
            })
        );
    }

    /// Dove non si è invitati non si bussa.
    ///
    /// Compreso il suffisso che somiglia: `evilarchive.org` finisce per
    /// `archive.org`, e la ragione per cui non passa sta in
    /// `rymembra_catalogo::riferimento`. Questa prova esiste per accorgersi il
    /// giorno in cui questo modulo smettesse di passare da quel cancello.
    #[test]
    fn un_dominio_che_non_ci_ha_invitati_non_diventa_un_flusso() {
        for indirizzo in [
            "https://esempio.invalido/musica/brano.mp3",
            "https://evilarchive.org/download/gd77/t01.flac",
            "https://archive.org/",
            // In chiaro non si esce: `Rete` nasce `https_only`, e riconoscerlo
            // qui vorrebbe dire promettere un'apertura che fallirebbe più in là
            // dicendo un'altra cosa.
            "http://archive.org/download/gd77/t01.flac",
        ] {
            assert_eq!(
                riferimento_da_testo(Fonte::InternetArchive, indirizzo),
                None,
                "«{indirizzo}» non è un posto dove Rymembra sia invitato"
            );
        }
    }

    /// Una riga che dichiara una fonte e ne porta un'altra non si apre.
    ///
    /// È il caso che esiste **solo** dalla migrazione 26 in poi, perché prima
    /// la fonte si deduceva dall'indirizzo e le due non potevano discordare. Un
    /// indirizzo dell'Archive dentro una riga che dice Audius passerebbe il
    /// controllo del dominio e aprirebbe la connessione con la rete sbagliata:
    /// le scadenze, lo `User-Agent` e il limite di frequenza di un servizio
    /// spesi verso un altro.
    #[test]
    fn una_riga_che_dichiara_la_fonte_sbagliata_non_si_apre() {
        assert_eq!(
            riferimento_da_testo(Fonte::Audius, "https://archive.org/download/gd77/t01.flac"),
            None
        );
        assert_eq!(
            riferimento_da_testo(Fonte::InternetArchive, "/v1/tracks/aB3dE/stream"),
            None,
            "un percorso di Audius non è un indirizzo dell'Archive"
        );
    }

    /// Una fonte che non consegna byte lo dice, senza chiedere niente a nessuno.
    ///
    /// Le prove girano senza rete, ed è ciò che rende questa prova capace di
    /// dire qualcosa: se il rifiuto arrivasse *dopo* la richiesta, qui uscirebbe
    /// un errore di trasporto invece di `playback.sourceUnavailable`.
    #[test]
    fn da_una_fonte_che_non_consegna_non_parte_nessuna_richiesta() {
        let cataloghi = Cataloghi::nuovi();
        let esito = apri_flusso(
            &cataloghi,
            &RiferimentoFlusso {
                fonte: Fonte::ArchivioSpotify,
                url: "https://archivio.esempio/brano".to_owned(),
            },
        );
        assert!(
            matches!(
                esito.as_ref().map_err(AppError::code),
                Err(ErrorCode::PlaybackSourceUnavailable { .. })
            ),
            "invece di rifiutare ha risposto: {:?}",
            esito.map(|_| ()).map_err(|e| e.code().kind().code())
        );
    }

    /// Un rimando dell'Internet Archive va dove l'Archive tiene i file, e da
    /// nessun'altra parte.
    ///
    /// È la promessa che il commento di `Rete::intervallo_finche` faceva già
    /// nella 2.4.0 e che il codice non manteneva: lì si guardava solo lo schema,
    /// e dopo che `ureq` aveva già seguito il rimando.
    #[test]
    fn un_rimando_dellarchivio_resta_nellarchivio() {
        let partenza = "https://archive.org/download/gd1977-05-08/gd77-05-08d1t01.mp3";
        let cancello = cancello_di(Fonte::InternetArchive, partenza);
        for buono in [
            "https://ia800305.us.archive.org/27/items/gd1977-05-08/gd77-05-08d1t01.mp3",
            "https://dn720003.ca.archive.org/0/items/gd1977-05-08/gd77-05-08d1t01.mp3",
            "https://IA800305.US.ARCHIVE.ORG:443/27/items/x/y.mp3",
            "https://archive.org/download/gd1977-05-08/altro.mp3",
        ] {
            assert!(cancello.ammette(buono), "«{buono}» è dell'Archive");
        }
        for cattivo in [
            "https://evilarchive.org/27/items/x/y.mp3",
            "https://archive.org.esempio.com/27/items/x/y.mp3",
            "https://archive.org@esempio.com/27/items/x/y.mp3",
            "https://esempio.com/archive.org/download/x/y.mp3",
            "http://ia800305.us.archive.org/27/items/x/y.mp3",
            "https://127.0.0.1/download/x/y.mp3",
        ] {
            assert!(!cancello.ammette(cattivo), "«{cattivo}» non è dell'Archive");
        }
    }

    /// Un rimando di Audius può andare da un nodo qualunque, ma non a casa.
    #[test]
    fn un_rimando_di_audius_va_verso_un_nome_pubblico() {
        let cancello = cancello_di(
            Fonte::Audius,
            "https://discoveryprovider.audius.co/v1/tracks/aB3dE/stream?app_name=Rymembra",
        );
        for buono in [
            "https://creatornode2.audius.co/tracks/cidstream/Qm?signature=x",
            "https://audius-content-1.esempio-nodi.com/tracks/cidstream/Qm",
            "https://nodo.xn--p1ai/tracks/cidstream/Qm",
        ] {
            assert!(cancello.ammette(buono), "«{buono}» è un nodo possibile");
        }
        for cattivo in [
            "https://127.0.0.1/tracks/cidstream/Qm",
            "https://192.168.1.1/",
            "https://[::1]:8443/",
            "https://0x7f.1/",
            "https://2130706433/",
            "https://localhost/",
            "https://router.lan/",
            "https://stampante.local/",
            "http://creatornode2.audius.co/tracks/cidstream/Qm",
        ] {
            assert!(
                !cancello.ammette(cattivo),
                "«{cattivo}» è casa, o in chiaro"
            );
        }
    }

    /// Un file finto in memoria, al posto di un catalogo.
    ///
    /// Passa dal tratto `Sorgente` di `rymembra-net`, che esiste esattamente per
    /// questo: provare il cablaggio contro un servizio vero vorrebbe dire non
    /// provarlo.
    #[derive(Debug)]
    struct FintaRete(Vec<u8>);

    impl rymembra_net::flusso::Sorgente for FintaRete {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<rymembra_net::Pezzo, AppError> {
            let inizio = usize::try_from(da).unwrap_or(usize::MAX);
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap_or(0))
                .min(self.0.len());
            Ok(rymembra_net::Pezzo {
                byte: self.0.get(inizio..fine).unwrap_or(&[]).to_vec(),
                totale: Some(u64::try_from(self.0.len()).unwrap_or(0)),
                url_finale: None,
            })
        }
    }

    /// Il riferimento di una voce di catalogo va e torna, e al ritorno passa
    /// dal cancello di questo PC.
    #[test]
    fn il_riferimento_di_una_voce_va_e_torna_dal_cancello() {
        let url = "https://archive.org/download/gd1977-05-08/gd77-05-08d1t01.mp3";
        let testo = riferimento_in_testo(Fonte::InternetArchive, url);
        assert_eq!(testo, format!("internet-archive:{url}"));
        assert_eq!(
            riferimento_da_voce(&testo),
            Some(RiferimentoFlusso {
                fonte: Fonte::InternetArchive,
                url: url.to_owned(),
            })
        );
        // Audius, che porta un percorso e non un indirizzo.
        let audius = riferimento_in_testo(Fonte::Audius, "/v1/tracks/aB3dE/stream");
        assert!(riferimento_da_voce(&audius).is_some());
        // Quel che un altro PC può mandare e qui non si apre: un dominio che
        // non ci ha invitati, una fonte che non dà byte, una forma storta.
        for cattivo in [
            "internet-archive:https://evilarchive.org/download/x/y.mp3",
            "internet-archive:http://archive.org/download/x/y.mp3",
            "audius:https://127.0.0.1/v1/tracks/x/stream",
            "archivio-spotify:https://archive.org/download/x/y.mp3",
            "sconosciuta:https://archive.org/download/x/y.mp3",
            "https://archive.org/download/x/y.mp3",
            "",
        ] {
            assert_eq!(riferimento_da_voce(cattivo), None, "«{cattivo}»");
        }
    }

    /// Un file in memoria servito come lo servirebbe un altro PC: ogni pezzo
    /// costa un ritardo fisso (l'andata e ritorno) più il tempo dei suoi byte
    /// sulla linea. Conta le richieste e i byte, per la misura.
    #[derive(Debug)]
    struct PariFinto {
        byte: Vec<u8>,
        andata_e_ritorno: std::time::Duration,
        byte_al_secondo: u64,
        richieste: std::sync::atomic::AtomicU64,
        letti: std::sync::atomic::AtomicU64,
    }

    impl rymembra_net::flusso::Sorgente for PariFinto {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<rymembra_net::Pezzo, AppError> {
            let inizio = usize::try_from(da)
                .unwrap_or(usize::MAX)
                .min(self.byte.len());
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap_or(0))
                .min(self.byte.len());
            let pezzo = self.byte.get(inizio..fine).unwrap_or(&[]).to_vec();
            let n = u64::try_from(pezzo.len()).unwrap_or(0);
            let linea = std::time::Duration::from_nanos(
                n.saturating_mul(1_000_000_000)
                    .checked_div(self.byte_al_secondo)
                    .unwrap_or(0),
            );
            std::thread::sleep(self.andata_e_ritorno + linea);
            self.richieste
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.letti
                .fetch_add(n, std::sync::atomic::Ordering::Relaxed);
            Ok(rymembra_net::Pezzo {
                byte: pezzo,
                totale: Some(u64::try_from(self.byte.len()).unwrap_or(0)),
                url_finale: None,
            })
        }
    }

    /// Quanto costa riaprire una voce che sta su un altro PC e saltare al
    /// punto dell'host: è la mossa `Riapri` del regolatore, e deve stare dentro
    /// l'anticipo di 250 ms con cui l'host programma le partenze.
    ///
    /// Un MP3 vero (`site/audio/machina.mp3`, 3,6 MB) servito da un pari
    /// finto, con l'involucro di sessione — `remoto()` falso, quindi salto
    /// **accurato**: symphonia conta i fotogrammi dall'inizio del file, e per
    /// arrivare a tre quarti li deve leggere quasi tutti. Si misura apertura
    /// più salto più il primo blocco decodificato, su tre linee: quella di
    /// `127.0.0.1`, una LAN via cavo da un gigabit, un Wi-Fi qualunque.
    ///
    /// Non è una prova di velocità — sul banco di prova i tempi cambiano col
    /// carico — e per questo è ignorata: si lancia a mano con
    /// `--ignored --nocapture`, e i numeri vanno nel rapporto.
    #[test]
    #[ignore = "misura: si lancia a mano con --ignored --nocapture"]
    fn una_riapertura_in_rete_si_misura() {
        let percorso = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../site/audio/machina.mp3");
        let byte = std::fs::read(&percorso).expect("il file della misura");
        let linee = [
            ("127.0.0.1", 200_u64, 1_000_000_000_u64),
            ("LAN 1 Gbit", 1_000, 110_000_000),
            ("Wi-Fi", 4_000, 20_000_000),
        ];
        for (nome, rtt_us, banda) in linee {
            let pari = std::sync::Arc::new(PariFinto {
                byte: byte.clone(),
                andata_e_ritorno: std::time::Duration::from_micros(rtt_us),
                byte_al_secondo: banda,
                richieste: std::sync::atomic::AtomicU64::new(0),
                letti: std::sync::atomic::AtomicU64::new(0),
            });
            let inizio = std::time::Instant::now();
            let flusso = FlussoHttp::da(Box::new(Condiviso(std::sync::Arc::clone(&pari))))
                .expect("il pari finto risponde");
            let sorgente = rymembra_play::Sorgente {
                track_id: -1,
                media: Box::new(FlussoDiPari(flusso)),
                estensione: Some("mp3".to_owned()),
                durata_ms: 0,
                replaygain_db: None,
                picco: None,
                segmento: None,
            };
            let mut decodificatore =
                rymembra_play::decodifica::Decodificatore::apri(sorgente, 48_000, 2)
                    .expect("l'MP3 si apre");
            let aperto = inizio.elapsed();
            let durata = decodificatore.fine_flusso_ms().unwrap_or(120_000);
            let bersaglio = durata.saturating_mul(3).checked_div(4).unwrap_or(0);
            decodificatore.cerca(bersaglio).expect("il salto riesce");
            let mut blocco = Vec::new();
            let _ = decodificatore.prossimo(&mut blocco);
            let totale = inizio.elapsed();
            println!(
                "[misura] {nome}: aperto in {} ms, salto a {} s e primo blocco in {} ms in tutto; \
                 {} richieste, {} KiB letti su {} KiB",
                aperto.as_millis(),
                bersaglio.checked_div(1000).unwrap_or(0),
                totale.as_millis(),
                pari.richieste.load(std::sync::atomic::Ordering::Relaxed),
                pari.letti
                    .load(std::sync::atomic::Ordering::Relaxed)
                    .checked_div(1024)
                    .unwrap_or(0),
                byte.len().checked_div(1024).unwrap_or(0),
            );
            assert!(!blocco.is_empty(), "dopo il salto si decodifica");
        }
    }

    /// Il pari finto, condiviso fra la misura e il flusso che lo legge.
    #[derive(Debug)]
    struct Condiviso(std::sync::Arc<PariFinto>);

    impl rymembra_net::flusso::Sorgente for Condiviso {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<rymembra_net::Pezzo, AppError> {
            self.0.pezzo(da, quanti)
        }
    }

    /// L'involucro di sessione dichiara i byte locali: è quel che fa saltare
    /// il decodificatore al fotogramma esatto.
    #[test]
    fn l_involucro_di_sessione_salta_preciso() {
        let Ok(flusso) = FlussoHttp::da(Box::new(FintaRete(vec![0; 64]))) else {
            panic!("la finta non fallisce mai");
        };
        let pari = FlussoDiPari(flusso);
        assert!(!rymembra_play::Flusso::remoto(&pari));
        assert_eq!(rymembra_play::Flusso::lunghezza(&pari), Some(64));
    }

    /// Quel che arriva al motore è indistinguibile da un file.
    ///
    /// È la prova del cablaggio intero, meno la rete: identificativo, durata dal
    /// database, guadagno, suggerimento di formato, lunghezza dichiarata — e i
    /// byte, che devono essere quelli e nell'ordine giusto. La lunghezza in
    /// particolare non è un di più: `MediaSource::is_seekable` di symphonia
    /// risponde guardando quella, e senza il cursore sparisce dalla barra.
    #[test]
    fn un_flusso_diventa_una_sorgente_che_il_motore_sa_suonare() {
        let byte: Vec<u8> = (0..3000_u32)
            .map(|n| u8::try_from(n % 251).unwrap_or(0))
            .collect();
        let Ok(flusso) = FlussoHttp::da(Box::new(FintaRete(byte.clone()))) else {
            panic!("la finta non fallisce mai");
        };
        let indirizzo = "https://archive.org/download/gd1977-05-08/gd77d1t01.flac";
        let scheda = SchedaSorgente {
            track_id: 4242,
            collocazione: Collocazione::Catalogo {
                fonte: Fonte::InternetArchive,
                url: indirizzo.to_owned(),
            },
            durata_ms: 754_000,
            replaygain_db: Some(-7.5),
            picco: Some(1.2),
        };

        let mut sorgente = sorgente_da_flusso(&scheda, indirizzo, flusso);
        assert_eq!(sorgente.track_id, 4242, "il brano ha perso il suo nome");
        assert_eq!(
            sorgente.durata_ms, 754_000,
            "la durata la dice il database, non il flusso"
        );
        assert_eq!(sorgente.estensione.as_deref(), Some("flac"));
        assert!(
            sorgente
                .replaygain_db
                .is_some_and(|db| (db + 7.5).abs() < 0.001),
            "il guadagno non è arrivato: {:?}",
            sorgente.replaygain_db
        );
        assert!(
            sorgente
                .picco
                .is_some_and(|picco| (picco - 1.2).abs() < 0.001),
            "il picco misurato non è arrivato: {:?}",
            sorgente.picco
        );
        assert_eq!(
            sorgente.media.lunghezza(),
            Some(3000),
            "senza lunghezza il cursore sparisce dalla barra"
        );

        let mut letti = Vec::new();
        let quanti = std::io::Read::read_to_end(&mut sorgente.media, &mut letti).unwrap_or(0);
        assert_eq!(quanti, 3000);
        assert_eq!(letti, byte, "i byte non sono quelli, o sono spostati");
    }
}
