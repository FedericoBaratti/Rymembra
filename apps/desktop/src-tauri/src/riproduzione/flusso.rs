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

use aether_app::playback::SchedaSorgente;
use aether_catalogo::Cataloghi;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::Fonte;
use aether_domain::indirizzo::estensione_da_url;
use aether_domain::scelta::Candidato;
use aether_net::FlussoHttp;

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
/// # Quale campo si legge, e quale si leggerà
///
/// Oggi `path`, perché in [`SchedaSorgente`] non c'è altro: `tracks.path` è
/// `NOT NULL UNIQUE` e la libreria non ha ancora un posto per una traccia che
/// non è un file — è il buco dichiarato fra i limiti noti del README, e
/// chiuderlo vuole una migrazione, quindi `aether-app` e una minor in più.
///
/// Quando quella migrazione arriverà, i campi da guardare sono già scritti
/// nella migrazione 10 e vivono su `desiderati`: `fonte_url` per l'indirizzo,
/// `source_service` per la fonte, `disponibilita` per il resto. **Cambia solo
/// questa funzione**: tutto quel che sta a valle — [`apri_flusso`],
/// [`sorgente_da_flusso`], il ramo dentro [`brano_di`](super::brano_di) —
/// riceve un [`RiferimentoFlusso`] e non sa da quale colonna sia uscito.
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
    riferimento_da_testo(&scheda.path)
}

/// Il riconoscimento vero e proprio, su una stringa sola.
///
/// Separata da [`riferimento_di`] per poterla provare senza un database e senza
/// una finestra: è una decisione su del testo, e le decisioni su del testo si
/// provano come chiamate di funzione.
fn riferimento_da_testo(testo: &str) -> Option<RiferimentoFlusso> {
    let pulito = testo.trim();
    // Il percorso di Audius prima di tutto, perché non è un indirizzo e
    // `riconosci` — che di indirizzi si occupa — non lo vedrebbe. Non si
    // confonde con un percorso di disco: su Windows un percorso comincia con
    // una lettera di unità o con due barre rovesce, mai con `/v1/`.
    if pulito.starts_with(PERCORSO_AUDIUS) {
        return Some(RiferimentoFlusso {
            fonte: Fonte::Audius,
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
    Some(RiferimentoFlusso {
        fonte: aether_catalogo::riconosci(pulito)?.fonte,
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
/// la riserva di connessioni già calda. Una `Rete` nuova per brano sarebbe un
/// saluto TLS per canzone e un limite di frequenza contato da capo ogni volta,
/// contro archivi pubblici che ci ospitano gratis — cioè il modo di farsi
/// bloccare per maleducazione.
///
/// # Errori
///
/// Quelli di `Rete::intervallo` (`net.*`, `download.*`) e, per Audius,
/// `catalogo.notAvailable` se non risponde nessun nodo.
/// `playback.sourceUnavailable` per una fonte che di byte non ne dà: è il caso
/// dell'archivio di Spotify e di un file di playlist, che nominano un brano e
/// non lo consegnano.
pub(super) fn apri_flusso(
    cataloghi: &Cataloghi,
    rif: &RiferimentoFlusso,
) -> Result<FlussoHttp, AppError> {
    match rif.fonte {
        Fonte::InternetArchive => {
            FlussoHttp::nuovo(cataloghi.archivio().rete().clone(), rif.url.clone())
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
            FlussoHttp::nuovo(cataloghi.audius().rete().clone(), pronto.url)
        }
        // Il catalogo per cui `FlussoHttp` è nato: da Jamendo non si tiene
        // niente, e ascoltare mentre arriva è l'unico modo lecito che c'è. Senza
        // la feature il ramo non esiste e la fonte cade nel caso generale, che è
        // la verità: questa copia di Aether Jamendo non ce l'ha.
        #[cfg(feature = "jamendo")]
        Fonte::Jamendo => FlussoHttp::nuovo(cataloghi.jamendo().rete().clone(), rif.url.clone()),
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
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un percorso resta un percorso.
    ///
    /// La prova che conta di più delle altre: qui dentro passa **ogni** brano
    /// della libreria, e un riconoscimento troppo largo vorrebbe dire un file
    /// del disco mandato a cercare in rete. Le forme sono quelle vere di
    /// Windows, più le due che somigliano di più a un indirizzo.
    #[test]
    fn un_percorso_di_disco_non_diventa_mai_un_flusso() {
        for percorso in [
            r"C:\Musica\Pink Floyd\Animals\01 - Pigs on the Wing.flac",
            r"\\nas\musica\raccolta\02 - Dogs.mp3",
            r"D:\archive.org\download\gd77\t01.flac",
            "C:/Musica/https/brano.mp3",
            "",
            "   ",
        ] {
            assert_eq!(
                riferimento_da_testo(percorso),
                None,
                "«{percorso}» è un file, non un flusso"
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
            riferimento_da_testo(file),
            Some(RiferimentoFlusso {
                fonte: Fonte::InternetArchive,
                url: file.to_owned(),
            })
        );

        // Audius conserva un percorso e non un indirizzo, apposta: il nodo di
        // oggi fra un mese non c'è più. Vedi [`PERCORSO_AUDIUS`].
        let percorso = "/v1/tracks/aB3dE/stream?ext=wav";
        assert_eq!(
            riferimento_da_testo(percorso),
            Some(RiferimentoFlusso {
                fonte: Fonte::Audius,
                url: percorso.to_owned(),
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
                riferimento_da_testo(indirizzo),
                None,
                "«{indirizzo}» non è un posto dove Aether sia invitato"
            );
        }
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
        let scheda = SchedaSorgente {
            track_id: 4242,
            path: "https://archive.org/download/gd1977-05-08/gd77d1t01.flac".to_owned(),
            durata_ms: 754_000,
            replaygain_db: Some(-7.5),
        };

        let mut sorgente = sorgente_da_flusso(&scheda, &scheda.path, flusso);
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
