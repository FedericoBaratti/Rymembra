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

use aether_app::playback::{Collocazione, SchedaSorgente};
use aether_catalogo::Cataloghi;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::Fonte;
use aether_domain::indirizzo::estensione_da_url;
use aether_domain::scelta::Candidato;
use aether_net::{Cancello, FlussoHttp};

/// Il riferimento a un brano che non è un file.
///
/// Due campi e nient'altro, perché due sono le domande: **quale catalogo** — che
/// decide da quale [`aether_net::Rete`] escono i byte, con le sue scadenze, il
/// suo modo di trattare un `429` e la sua riserva di connessioni — e **a che
/// indirizzo**.
///
/// Non c'è la licenza e non c'è la disponibilità, e non è una dimenticanza: qui
/// non si scrive niente sul disco. Il cancello che le guarda è quello di
/// `aether_catalogo::prelievo`, e sta prima della rete perché prima della rete
/// deve stare; ascoltare mentre arriva è la cosa che [`Disponibilita::SoloAscolto`]
/// **permette**, e chiederne il permesso una seconda volta qui vorrebbe dire
/// due implementazioni della stessa regola.
///
/// [`Disponibilita::SoloAscolto`]: aether_domain::esterno::Disponibilita::SoloAscolto
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
/// `aether_catalogo::audius` scrive `/v1/tracks/<id>/stream` e non un URL
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
/// `aether_catalogo::riconosci` è una allowlist rigida, e il suo modulo dice
/// perché — «un link che non si riconosce è un link a cui non si va, ed è la
/// garanzia che Aether non vada mai a bussare dove non è invitato». Due copie di
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
/// crede* che arrivino i byte; l'allowlist dice dove Aether è invitato ad
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
    (aether_catalogo::riconosci(pulito)?.fonte == fonte).then(|| RiferimentoFlusso {
        fonte,
        url: pulito.to_owned(),
    })
}

/// Apre il flusso di un riferimento, con la rete del catalogo che lo ha dato.
///
/// **Va in rete**, e quindi va chiamata dove la rete si può aspettare: dentro
/// la scadenza di [`brano_di`](super::brano_di), senza nessun lucchetto in
/// mano. Costa una richiesta `Range` da [`aether_net::flusso::FINESTRA`] byte —
/// l'inizio del brano, che è la prima cosa che il decodificatore chiederà —
/// più, su Audius, la scelta del nodo.
///
/// # Perché la rete è quella del catalogo e non una nuova
///
/// Perché in quella `Rete` ci sono lo `User-Agent` con cui Aether si presenta,
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
        // la verità: questa copia di Aether Jamendo non ce l'ha.
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
/// L'indirizzo di partenza è passato da `aether_catalogo::riconosci`, e il
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
    let Some(ospite) = aether_net::http::ospite(bersaglio) else {
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
    aether_catalogo::riconosci(&format!("https://{ospite}{percorso}"))
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

/// Un flusso di rete, vestito da [`aether_play::Flusso`].
///
/// # Perché serve un involucro
///
/// Perché il tratto è di `aether-play` e il tipo è di `aether-net`, e nessuno
/// dei due è di qui: la regola dell'orfano vieta di scrivere quell'`impl` in un
/// terzo crate. È la stessa ragione per cui `aether-app` ne ha uno suo
/// (`playback::Adattatore`) per i file, e la stessa forma.
///
/// Non è però solo una formalità del compilatore. Questo modulo è **l'unico
/// posto dell'albero che conosce tutti e due i lati**: `aether-app` non vede la
/// rete e `aether-catalogo` non vede il motore. Il punto in cui un flusso HTTP
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

impl aether_play::Flusso for FlussoDiRete {
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
    /// `aether_play::Flusso::remoto`.
    fn remoto(&self) -> bool {
        true
    }

    /// Un salto dopo un salto, un altro brano o uno stop: la richiesta in
    /// corso smette di aspettare la rete. Vedi `FlussoHttp::ascolta_annullo`.
    fn ascolta_annullo(&mut self, annullo: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.0.ascolta_annullo(annullo);
    }
}

/// Da un flusso aperto alla sorgente che il motore sa suonare.
///
/// Quel che il motore riceve è indistinguibile da un file: stesso `track_id`,
/// stessa durata dichiarata dal database, stesso guadagno ReplayGain. È il
/// motivo per cui gapless, dissolvenza, equalizzatore e spettro funzionano senza
/// una riga in più — il motore non ha mai saputo cosa ci fosse dietro i byte.
///
/// Il suggerimento di formato lo ricava
/// [`aether_domain::indirizzo::estensione_da_url`]: la regola sta nel dominio
/// perché la usano tre casse che non si vedono fra loro — questa, il prelievo
/// dei cataloghi e la libreria — e due copie divergerebbero alla prima
/// correzione fatta da una parte sola. Divergevano già.
pub(super) fn sorgente_da_flusso(
    scheda: &SchedaSorgente,
    url: &str,
    flusso: FlussoHttp,
) -> aether_play::Sorgente {
    aether_play::Sorgente {
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
    /// `aether_catalogo::riferimento`. Questa prova esiste per accorgersi il
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
                "«{indirizzo}» non è un posto dove Aether sia invitato"
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
            "https://discoveryprovider.audius.co/v1/tracks/aB3dE/stream?app_name=Aether",
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
    /// Passa dal tratto `Sorgente` di `aether-net`, che esiste esattamente per
    /// questo: provare il cablaggio contro un servizio vero vorrebbe dire non
    /// provarlo.
    #[derive(Debug)]
    struct FintaRete(Vec<u8>);

    impl aether_net::flusso::Sorgente for FintaRete {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<aether_net::Pezzo, AppError> {
            let inizio = usize::try_from(da).unwrap_or(usize::MAX);
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap_or(0))
                .min(self.0.len());
            Ok(aether_net::Pezzo {
                byte: self.0.get(inizio..fine).unwrap_or(&[]).to_vec(),
                totale: Some(u64::try_from(self.0.len()).unwrap_or(0)),
                url_finale: None,
            })
        }
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
