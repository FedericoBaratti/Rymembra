//! L'accesso ai file, dietro un tratto.
//!
//! Sul desktop un brano è un percorso e `std::fs` lo apre. Su Android un brano è
//! un URI di MediaStore o una concessione dello Storage Access Framework, e per
//! aprirlo serve passare da Java. Le due cose non si assomigliano abbastanza da
//! poter essere la stessa funzione, e non si assomigliano abbastanza poco da
//! giustificare due scansioni diverse.
//!
//! Quindi: la scansione parla con questo tratto, e chi la esegue gli passa
//! l'implementazione della sua piattaforma. La **decisione** su cosa inserire e
//! cosa togliere resta una funzione pura in `aether-domain`, uguale ovunque; qui
//! cambia solo il modo di ottenere l'elenco e i byte.
//!
//! È anche ciò che rende la scansione provabile senza toccare il disco: un
//! doppio in memoria implementa il tratto e i test girano su alberi di file
//! finti, compresi quelli che un filesystem vero non lascerebbe costruire.

use std::io::{Read, Seek};
use std::ops::ControlFlow;
use std::path::Path;

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::scan_plan::DiscoveredFile;

/// Ciò che serve a un lettore di metadati: leggere e riposizionarsi.
///
/// I formati audio hanno i tag in testa (ID3v2) o in coda (ID3v1, APE), quindi
/// non basta uno stream in avanti.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// L'esito di una camminata: quel che si è trovato, e se si è trovato tutto.
///
/// # Perché «completa» viaggia insieme ai file
///
/// Perché senza, un elenco parziale e un elenco intero si assomigliano come una
/// radice morta e una radice vuota — ed è lo stesso errore, un passo più in là.
/// La sonda di [`MusicFiles::radice_raggiungibile`] guarda la cartella *prima*
/// di camminarci dentro; se la rete cade **durante** la camminata di una
/// libreria grande, quella sonda ha già detto sì, `walkdir` salta in silenzio i
/// rami che non risponde più, e quel che torna è una fotografia di mezza
/// libreria che il piano legge come «l'altra metà è sparita».
///
/// Non è un caso di scuola: è la stessa cancellazione che la sonda esiste per
/// impedire, con la rete caduta un momento più tardi.
/// Niente `Default`, e apposta: il suo `completa` sarebbe `false`, cioè
/// «parziale», che è il valore su cui a valle si decide di non cancellare
/// niente. Un campo del genere va scritto da chi sa com'è andata la camminata,
/// non ereditato per distrazione da chi non ci ha pensato.
///
/// # Il terzo caso, che qui dentro non c'è
///
/// Una camminata **arenata** — la share che non fallisce e non finisce, che
/// resta lì — non è né completa né parziale: è una camminata che non è tornata.
/// Questo tipo non sa esprimerla, perché per esistere bisogna che la funzione
/// che lo costruisce sia rientrata.
///
/// Il terzo caso vive quindi un piano più in su, in [`crate::library::cammina`],
/// che consuma [`MusicFiles::walk_a_rate`] a rate e conta il silenzio fra due
/// consegne. Lì una radice arenata si salta **senza risondare**: la sonda
/// risponderebbe «sì» — dalla cache del sistema, o perché la cartella si apre e
/// solo il contenuto non arriva — e quel «sì» farebbe passare l'elenco monco
/// per un elenco vero.
#[derive(Debug, Clone)]
pub struct Camminata {
    /// I file trovati.
    pub file: Vec<DiscoveredFile>,
    /// Nessun ramo è andato perso.
    ///
    /// `false` dice soltanto «questo elenco è parziale», non «la rete è giù»:
    /// una sottocartella a permessi negati su un disco locale lo abbassa
    /// esattamente come una share che muore. A distinguere i due casi è chi
    /// chiama, risondando la radice — vedi [`crate::library::cammina`].
    ///
    /// È lo stesso `bool` che [`MusicFiles::walk_a_rate`] restituisce alla fine
    /// della sua consegna, e i due non devono mai divergere: chi implementa
    /// l'uno implementa l'altro.
    pub completa: bool,
}

/// Da dove arrivano i file musicali.
pub trait MusicFiles: Send + Sync {
    /// Elenca ricorsivamente i file sotto una radice.
    ///
    /// **Non filtra**: restituisce quel che trova, comprese le estensioni non
    /// audio e i file troppo piccoli. Il filtro è una decisione, e le decisioni
    /// stanno nel dominio, dove si provano. Una camminata che filtrasse da sé
    /// renderebbe invisibile al piano il motivo per cui un file è stato
    /// scartato — che è l'informazione che serve quando un brano «non compare».
    ///
    /// Una radice illeggibile non è un errore fatale: si riporta vuota, perché
    /// un disco esterno staccato non deve impedire la scansione degli altri.
    ///
    /// **Ma «vuota» non vuol dire «da svuotare»**: chi chiama deve prima chiedere
    /// a [`Self::radice_raggiungibile`] se quella cartella esiste ancora, perché
    /// una camminata vuota su una radice morta e una su una radice davvero
    /// svuotata si assomigliano come due gocce d'acqua, e trattarle uguali
    /// significa cancellare l'intera libreria di chi ha staccato il NAS.
    ///
    /// **E «parziale» non vuol dire «completa»**: chi implementa deve abbassare
    /// [`Camminata::completa`] appena perde un ramo, per lo stesso motivo un
    /// passo più in là. Vedi [`Camminata`].
    ///
    /// **Torna quando ha finito**, e su una share che non risponde «quando ha
    /// finito» può voler dire mai: chi non se lo può permettere — la scansione,
    /// che gira mentre la finestra deve restare viva — passa da
    /// [`Self::walk_a_rate`], che consegna man mano e a cui si può smettere di
    /// dare ascolto.
    fn walk(&self, root: &str) -> Result<Camminata, AppError>;

    /// Come [`Self::walk`], ma consegnando ogni file appena lo si trova.
    ///
    /// # Perché a rate
    ///
    /// Perché la domanda che serve alla scansione non è «quanto è durata
    /// l'enumerazione», che su una libreria da centomila file dura
    /// legittimamente dei minuti, ma «da quanto non arriva più niente». Le due
    /// si somigliano solo finché il disco risponde: una share viva e lenta
    /// consegna piano ma consegna, una share morta smette e basta. Con un
    /// [`Self::walk`] intero le due sono la stessa attesa, e limitarla vorrebbe
    /// dire scegliere fra dichiarare morta una libreria grande e non accorgersi
    /// mai di una share caduta.
    ///
    /// `su_file` viene chiamato una volta per file, nell'ordine in cui la
    /// camminata li incontra; restituendo [`ControlFlow::Break`] dice «basta
    /// così» e la camminata si ferma lì.
    ///
    /// # Il `bool`
    ///
    /// È [`Camminata::completa`], con lo stesso significato e lo stesso peso a
    /// valle: `true` vuol dire «nessun ramo è andato perso», e solo su un `true`
    /// chi chiama si può permettere di concludere che quel che non è arrivato
    /// non c'è. Una camminata **interrotta** da `su_file` restituisce quindi
    /// `false`: chi ha detto basta non ha visto tutto, per definizione.
    ///
    /// Il corpo di serie chiama [`Self::walk`] e consegna quel che ha
    /// restituito: è corretto per ogni implementazione — i doppi di prova, un
    /// provider Android — e sbagliato per una cosa sola, il tempo. Chi cammina
    /// su un filesystem che può non rispondere lo sovrascrive consegnando
    /// davvero man mano, altrimenti il silenzio che chi consuma sta misurando è
    /// un silenzio che comincia soltanto alla fine.
    fn walk_a_rate(
        &self,
        root: &str,
        su_file: &mut dyn FnMut(DiscoveredFile) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        let camminata = self.walk(root)?;
        for file in camminata.file {
            if su_file(file).is_break() {
                return Ok(false);
            }
        }
        Ok(camminata.completa)
    }

    /// La radice esiste ed è leggibile **adesso**?
    ///
    /// Distingue «radice vuota» da «NAS spento»: trattarle uguali significava
    /// marcare «sparito» ogni brano della seconda alla prima scansione a rete
    /// giù, e con lui portarsi via voti, preferiti e cronologia — le sole cose
    /// in tutta la libreria che una riscansione non sa ricostruire.
    ///
    /// Il valore di serie è `true`, cioè «non lo so, fai come prima»: le
    /// implementazioni che non hanno un modo di chiederlo — un doppio in
    /// memoria, un provider Android — non devono per questo bloccare la
    /// scansione.
    fn radice_raggiungibile(&self, root: &str) -> bool {
        let _ = root;
        true
    }

    /// Apre un file in lettura.
    ///
    /// `Sync` oltre a `Send` perché lo stesso flusso serve al motore audio, e
    /// symphonia lo pretende: il suo `MediaSource` è `Send + Sync`. Non è un
    /// requisito gratuito — significa che un'implementazione non può nascondere
    /// uno stato mutabile senza lucchetto dietro un `&self` — ma è quello che
    /// permette a lettura dei tag e riproduzione di passare dallo stesso tratto
    /// invece che da due.
    fn open(&self, path: &str) -> Result<Box<dyn ReadSeek + Send + Sync>, AppError>;
}

/// Traduce un errore di I/O nel catalogo, distinguendo i casi che cambiano cosa
/// deve fare il chiamante.
///
/// La distinzione non è cosmetica: `NotFound` durante una scansione significa
/// «il file è appena sparito, salta e prosegui», mentre `PermissionDenied`
/// significa «tutta questa cartella è inaccessibile, dirlo all'utente». Il
/// vecchio albero li appiattiva entrambi in un avviso nel log.
pub fn io_error(path: &str, err: &std::io::Error) -> AppError {
    use std::io::ErrorKind as K;
    // Prima di tutto il resto: i guasti di rete arrivano da Windows come codici
    // grezzi che `ErrorKind` non sa nominare — diventerebbero un generico
    // `fs.readFailed`, cioè «questo file non si legge», quando la verità è «la
    // condivisione non c'è più». Le due frasi mandano a cercare il guasto in due
    // posti diversi, e solo una delle due offre «Riprova».
    if errore_di_rete(err) {
        return AppError::new(ErrorCode::FsNetworkUnavailable {
            path: Some(path.to_owned()),
        })
        .with_cause(err.to_string());
    }
    let code = match err.kind() {
        K::NotFound => ErrorCode::FsNotFound {
            path: path.to_owned(),
        },
        K::PermissionDenied => ErrorCode::FsPermissionDenied {
            path: path.to_owned(),
        },
        K::StorageFull | K::QuotaExceeded => ErrorCode::FsDiskFull {
            path: Some(path.to_owned()),
        },
        K::InvalidFilename => ErrorCode::FsPathInvalid {
            path: path.to_owned(),
        },
        _ => ErrorCode::FsReadFailed {
            path: path.to_owned(),
            detail: Some(err.kind().to_string()),
        },
    };
    AppError::new(code).with_cause(err.to_string())
}

/// Il guasto viene dalla rete, non dal file?
///
/// L'elenco dei numeri di sistema e il perché di ognuno stanno in
/// [`aether_domain::errors::rete`], e non qui, perché la stessa domanda se la
/// fa anche il motore audio quando la condivisione muore **a metà** di un
/// brano: due elenchi separati sarebbero due elenchi che un giorno diranno
/// cose diverse sullo stesso guasto.
fn errore_di_rete(err: &std::io::Error) -> bool {
    aether_domain::errors::rete::e_di_rete(err)
}

/// I file musicali sul filesystem locale.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFiles;

impl MusicFiles for LocalFiles {
    /// Ricostruita sopra [`MusicFiles::walk_a_rate`], e non il contrario.
    ///
    /// Il ciclo su `walkdir` sta di là perché è di là che serve consegnare man
    /// mano; qui si raccoglie quel che quello consegna. Due cicli sarebbero due
    /// idee su cosa conta come ramo perso, e prima o poi una camminata intera e
    /// una a rate direbbero due cose diverse sulla stessa cartella.
    fn walk(&self, root: &str) -> Result<Camminata, AppError> {
        let mut file = Vec::new();
        let completa = self.walk_a_rate(root, &mut |trovato| {
            file.push(trovato);
            ControlFlow::Continue(())
        })?;
        Ok(Camminata { file, completa })
    }

    fn walk_a_rate(
        &self,
        root: &str,
        su_file: &mut dyn FnMut(DiscoveredFile) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        // Ogni ramo perso abbassa questo, e non è pignoleria: è l'unica
        // differenza fra «qui non c'è più niente» e «di qui non si è visto
        // tutto», e a valle decide se dei brani si cancellano.
        let mut completa = true;
        // `walkdir` con i link simbolici non seguiti: seguirli permetterebbe a
        // un anello di far girare la scansione all'infinito, e a un link verso
        // la cartella superiore di contare ogni brano due volte.
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = match entry {
                Ok(entry) => entry,
                // Una sottocartella illeggibile non ferma la camminata: si perde
                // quel ramo, non l'intera libreria. Ma **si dice**.
                Err(_) => {
                    completa = false;
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                // Un file elencato di cui non si legge la data è un file su cui
                // non si può decidere niente: sparisce dall'elenco, quindi
                // l'elenco non è più intero.
                completa = false;
                continue;
            };
            let path = entry.path().to_string_lossy().into_owned();
            let trovato = DiscoveredFile {
                path,
                size_bytes: metadata.len(),
                modified_ms: modified_ms(&metadata),
            };
            // Chi ha detto basta non ha visto tutto: `false`, non `completa`.
            if su_file(trovato).is_break() {
                return Ok(false);
            }
        }
        Ok(completa)
    }

    fn open(&self, path: &str) -> Result<Box<dyn ReadSeek + Send + Sync>, AppError> {
        let file = std::fs::File::open(Path::new(path)).map_err(|err| io_error(path, &err))?;
        Ok(Box::new(std::io::BufReader::with_capacity(
            BUFFER_LETTURA,
            file,
        )))
    }

    fn radice_raggiungibile(&self, root: &str) -> bool {
        let percorso = root.to_owned();
        // Con la scadenza e non con una `metadata` nuda: su una share morta
        // quella chiamata non fallisce, aspetta il timeout di SMB — e la sonda
        // che serve a proteggere la libreria diventerebbe essa stessa il motivo
        // per cui la scansione si pianta.
        //
        // Scaduta vale **irraggiungibile**: la radice che non risponde in otto
        // secondi non è una radice da cui si possa concludere che dei brani sono
        // spariti.
        crate::scadenza::con_scadenza(SONDA_RADICE_NOME, SONDA_RADICE, move || {
            std::fs::metadata(Path::new(&percorso)).is_ok()
        })
        .unwrap_or(false)
    }
}

/// Quanto si aspetta una radice prima di dichiararla irraggiungibile.
///
/// Otto secondi: molto più di quel che serve a un disco locale (microsecondi) o
/// a una share viva (millisecondi), molto meno dei quaranta e passa che Windows
/// impiega a rinunciare da solo su un percorso di rete morto. Il numero non deve
/// essere preciso — deve solo stare largo sul caso buono e stretto sul caso
/// cattivo.
const SONDA_RADICE: std::time::Duration = std::time::Duration::from_secs(8);

/// Il nome che il filo della sonda porta nel diario dei panici.
const SONDA_RADICE_NOME: &str = "sonda-radice";

/// Quanti byte alla volta si chiedono al filesystem leggendo un brano.
///
/// Sessantaquattro kibibyte, e il numero conta soltanto sulla rete. Su un disco
/// locale la dimensione del buffer sposta qualche microsecondo; su SMB **ogni
/// riempimento è un giro di rete**, e con gli otto kibibyte di serie di
/// `BufReader` un file da mezzo megabyte diventa una sessantina di andate e
/// ritorni con la latenza del Wi-Fi addosso a ognuna.
///
/// Sessantaquattro e non di più perché è la lettura massima che SMB2 negozia di
/// suo con Windows: chiederne di più non fa meno giri, li fa uguali con un
/// buffer più grosso. E per un lettore di tag il buffer grosso è sprecato due
/// volte, perché `lofty` guarda la testa del file e poi salta in coda: quel che
/// si legge in mezzo lo si è letto per niente.
const BUFFER_LETTURA: usize = 64 * 1024;

/// La data di modifica in millisecondi interi.
///
/// Il troncamento è deliberato e deve stare qui, non a valle: è la forma con cui
/// il database la salva e con cui la scansione confronta. Se il confronto
/// avvenisse fra un valore troncato e uno con i decimali, ogni passata vedrebbe
/// cambiato ogni file e una riscansione da centomila brani rileggerebbe tutti i
/// metadati invece di nessuno.
///
/// Un orologio che non si legge dà 0: un file «modificato all'epoca zero» verrà
/// riletto a ogni scansione, che è lento ma corretto — il contrario, dargli
/// l'ora attuale, lo farebbe sembrare aggiornato per sempre.
fn modified_ms(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn la_camminata_trova_i_file_annidati_e_non_filtra() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let nested = dir.path().join("Album").join("CD1");
        std::fs::create_dir_all(&nested).expect("cartelle");
        for name in ["a.mp3", "b.txt", "c.flac"] {
            let mut f = std::fs::File::create(nested.join(name)).expect("file");
            f.write_all(b"x").expect("scrittura");
        }

        let root = dir.path().to_string_lossy().into_owned();
        let found = LocalFiles.walk(&root).expect("camminata");

        // Anche il .txt: filtrare è una decisione, e sta nel dominio. Qui si
        // riporta quel che c'è, altrimenti il piano non potrebbe dire PERCHÉ un
        // file non è entrato in libreria.
        assert_eq!(found.file.len(), 3);
        assert!(found.file.iter().any(|f| f.path.ends_with("b.txt")));
        // Niente si è perso per strada: è la condizione che permette al piano di
        // fidarsi di questo elenco per decidere delle rimozioni.
        assert!(found.completa, "una cartella sana si cammina per intero");
    }

    #[test]
    fn la_camminata_a_rate_consegna_gli_stessi_file_di_quella_intera() {
        // Le due non devono divergere mai, e non per eleganza: la scansione
        // cammina a rate, ogni altro chiamante cammina intero, e se le due
        // vedessero cartelle diverse la libreria dipenderebbe da chi l'ha
        // guardata.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let nested = dir.path().join("Album").join("CD2");
        std::fs::create_dir_all(&nested).expect("cartelle");
        for name in ["a.mp3", "b.txt", "c.flac"] {
            let mut f = std::fs::File::create(nested.join(name)).expect("file");
            f.write_all(b"x").expect("scrittura");
        }
        let root = dir.path().to_string_lossy().into_owned();

        let mut a_rate = Vec::new();
        let completa = LocalFiles
            .walk_a_rate(&root, &mut |trovato| {
                a_rate.push(trovato);
                ControlFlow::Continue(())
            })
            .expect("camminata a rate");
        let intera = LocalFiles.walk(&root).expect("camminata intera");

        assert_eq!(completa, intera.completa);
        assert!(completa, "una cartella sana si cammina per intero");
        let percorsi: Vec<&str> = a_rate.iter().map(|f| f.path.as_str()).collect();
        let attesi: Vec<&str> = intera.file.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(percorsi, attesi, "stessi file, stesso ordine");
    }

    #[test]
    fn chi_dice_basta_ferma_la_camminata_a_rate() {
        // Il patto dell'annullamento visto da qui: chi smette di ascoltare
        // ferma l'enumerazione invece di lasciarla girare per nessuno. E quel
        // che torna è `false`, cioè «parziale»: chi ha detto basta non ha
        // visto tutto, e su un elenco così non si cancella niente.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        for name in ["a.mp3", "b.mp3", "c.mp3", "d.mp3"] {
            std::fs::write(dir.path().join(name), b"x").expect("file");
        }
        let root = dir.path().to_string_lossy().into_owned();

        let mut quanti = 0_usize;
        let completa = LocalFiles
            .walk_a_rate(&root, &mut |_| {
                quanti += 1;
                if quanti >= 2 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            })
            .expect("camminata a rate");

        assert_eq!(quanti, 2, "si ferma al secondo, non arriva al quarto");
        assert!(
            !completa,
            "una camminata interrotta non è un elenco su cui fidarsi"
        );
    }

    #[test]
    fn una_radice_inesistente_non_fa_cadere_la_scansione() {
        // Il disco esterno staccato: si riporta vuoto, e le altre radici
        // vengono scansionate lo stesso.
        let found = LocalFiles.walk("Z:/non/esiste").expect("nessun errore");
        assert!(found.file.is_empty());
        // Vuota **e** dichiarata parziale: non aver potuto guardare non è la
        // stessa cosa di aver guardato e non aver trovato niente, ed è la
        // differenza su cui il piano decide se cancellare.
        assert!(
            !found.completa,
            "una radice che non si apre non produce un elenco su cui fidarsi"
        );
    }

    #[test]
    fn la_sonda_delle_radici_distingue_quel_che_ce_da_quel_che_non_ce() {
        // La sonda vera, non quella dei doppi: tutte le prove sulle radici morte
        // in `library.rs` girano su `FintoDisco`, che sovrascrive
        // `radice_raggiungibile` — quindi l'implementazione che poi gira in
        // produzione, con il filo e la scadenza, non era esercitata da niente.
        //
        // Quel che si può provare senza una share vera è il resto: che la sonda
        // risponda, e che risponda giusto nei due casi normali. Il ramo della
        // scadenza scaduta è provato in `scadenza.rs`, dove non serve un NAS.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let root = dir.path().to_string_lossy().into_owned();
        assert!(
            LocalFiles.radice_raggiungibile(&root),
            "una cartella che esiste deve rispondere"
        );
        assert!(
            !LocalFiles.radice_raggiungibile("Z:/non/esiste/di/sicuro"),
            "e una che non esiste no: da qui passa la decisione di non cancellare"
        );
    }

    #[test]
    fn la_sonda_di_una_radice_viva_non_ci_mette_niente() {
        // Il numero che conta non è otto secondi, è «molto meno di otto
        // secondi»: la sonda gira una volta per radice a ogni scansione, e se
        // costasse gira sull'ordine dei secondi anche a disco sano sarebbe lei
        // il motivo per cui una scansione parte tardi.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let root = dir.path().to_string_lossy().into_owned();
        let prima = std::time::Instant::now();
        assert!(LocalFiles.radice_raggiungibile(&root));
        let passato = prima.elapsed();
        assert!(
            passato < std::time::Duration::from_secs(1),
            "la sonda su un disco locale ci ha messo {passato:?}"
        );
    }

    #[test]
    fn la_data_di_modifica_e_intera() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let path = dir.path().join("a.mp3");
        std::fs::write(&path, b"x").expect("scrittura");
        let root = dir.path().to_string_lossy().into_owned();
        let found = LocalFiles.walk(&root).expect("camminata");
        let file = found.file.first().expect("un file");
        assert!(file.modified_ms > 0, "l'orologio deve essere leggibile qui");
        // Due camminate di fila devono dare lo stesso valore: se il troncamento
        // non fosse stabile, ogni passata vedrebbe «cambiato» ogni file.
        let ancora = LocalFiles.walk(&root).expect("camminata");
        assert_eq!(
            ancora.file.first().map(|f| f.modified_ms),
            Some(file.modified_ms)
        );
    }

    #[test]
    fn gli_errori_di_io_si_distinguono() {
        use aether_domain::errors::ErrorCodeKind;
        let not_found = io_error("x", &std::io::Error::from(std::io::ErrorKind::NotFound));
        let denied = io_error(
            "x",
            &std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        // Durante una scansione i due vogliono reazioni diverse: il primo «salta
        // e prosegui», il secondo «dillo all'utente». Il vecchio albero li
        // appiattiva entrambi in un avviso nel log.
        assert_eq!(not_found.code().kind(), ErrorCodeKind::FsNotFound);
        assert_eq!(denied.code().kind(), ErrorCodeKind::FsPermissionDenied);
    }

    #[cfg(windows)]
    #[test]
    fn i_guasti_della_share_si_riconoscono_dal_numero() {
        use aether_domain::errors::ErrorCodeKind;
        // Questi tre sono quelli che si vedono davvero staccando la rete mentre
        // Aether legge: senza il ramo dei codici grezzi arriverebbero tutti come
        // `fs.readFailed`, cioè «il file è rotto» invece di «la share non c'è».
        for numero in [53, 59, 64] {
            let err = io_error(
                "//srv/musica/a.mp3",
                &std::io::Error::from_raw_os_error(numero),
            );
            assert_eq!(
                err.code().kind(),
                ErrorCodeKind::FsNetworkUnavailable,
                "il codice {numero} deve dire «rete», non «file»"
            );
            assert!(err.code().is_retryable(), "la rete torna: si ritenta");
        }
        // …e un file che davvero non c'è resta un file che non c'è: il ramo
        // nuovo non deve inghiottire il caso normale.
        let mancante = io_error("C:/m/a.mp3", &std::io::Error::from_raw_os_error(2));
        assert_eq!(mancante.code().kind(), ErrorCodeKind::FsNotFound);
    }
}
